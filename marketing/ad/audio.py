"""Synthesizes the ad's 15 s soundtrack (120 BPM, A minor) with hits synced to ad.html's timeline -> out/audio.wav"""
import os, wave
import numpy as np

SR, DUR, BPM = 48000, 15.0, 120
N = int(SR * DUR)
T = np.arange(N) / SR
rng = np.random.default_rng(3)
dry = np.zeros((N, 2))
wet = np.zeros((N, 2))  # sent through the reverb


def hz(m):
    return 440.0 * 2 ** ((m - 69) / 12)


def place(buf, t0, sig, gain=1.0, pan=0.0):
    i = int(t0 * SR)
    if i >= N:
        return
    sig = sig[: N - i] * gain
    buf[i : i + len(sig), 0] += sig * np.sqrt(0.5 * (1 - pan))
    buf[i : i + len(sig), 1] += sig * np.sqrt(0.5 * (1 + pan))


def smooth(x, n):
    return np.convolve(x, np.ones(n) / n, mode="same")


def tt(d):
    return np.arange(int(d * SR)) / SR


def kick(d=0.45):
    t = tt(d)
    f = 45 + 95 * np.exp(-t / 0.035)
    s = np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t / 0.26)
    s[:120] += rng.standard_normal(120) * np.linspace(0.6, 0, 120)
    return np.tanh(s * 1.6)


def hat(d=0.08):
    n = rng.standard_normal(int(d * SR))
    return (n - smooth(n, 6)) * np.exp(-tt(d) / 0.022)


def clap(d=0.3):
    t = tt(d)
    n = rng.standard_normal(len(t))
    n = smooth(n - smooth(n, 30), 3)
    env = np.exp(-t / 0.09) + sum(np.exp(-np.maximum(t - k * 0.011, 0) / 0.006) * (t >= k * 0.011) for k in range(3)) * 0.6
    return n * env


def chime(m, d=1.6, decay=0.5):
    t = tt(d)
    f = hz(m)
    return sum(a * np.sin(2 * np.pi * f * k * t) * np.exp(-t / (decay / k ** 0.5)) for k, a in [(1, 1), (2, 0.35), (3.01, 0.18), (4.2, 0.06)]) * np.minimum(t / 0.002, 1)


def pluck(m, d=1.8, decay=0.6):
    t = tt(d)
    f = hz(m)
    s = sum(np.sin(2 * np.pi * f * k * t) / k * np.exp(-t * k / decay) for k in range(1, 7))
    return s * np.minimum(t / 0.004, 1)


def noise_swell(d, peak, lp=6):
    t = tt(d)
    env = np.where(t < peak, (t / peak) ** 2.5, np.exp(-(t - peak) / 0.07))
    n = smooth(rng.standard_normal(len(t)), lp)
    return n * env


# ---- pad (chord every 2 s) with sidechain pump once the beat starts
chords = [[57, 60, 64, 69], [53, 57, 60, 65], [55, 60, 64, 67], [55, 59, 62, 67]] * 2
roots = [33, 29, 36, 31] * 2
pad = np.zeros(N)
for i, ch in enumerate(chords):
    t0, d = i * 2.0, 2.5
    t = tt(d)
    env = np.minimum(t / 0.35, 1) * np.clip((d - t) / 0.5, 0, 1)
    seg = np.zeros(len(t))
    for m in ch:
        for det in (-0.0018, 0.0018):
            f = hz(m) * (1 + det)
            seg += sum(np.sin(2 * np.pi * f * k * t + k) / k ** 1.6 for k in range(1, 7))
    i0 = int(t0 * SR)
    n = min(len(seg), N - i0)
    pad[i0 : i0 + n] += (seg * env)[:n]
pad /= np.abs(pad).max()
beats = np.arange(2.0, 12.0, 60 / BPM)
pump = np.ones(N)
for b in beats:
    i = int(b * SR)
    k = np.arange(N - i) / SR
    pump[i:] = np.minimum(pump[i:], 1 - 0.6 * np.exp(-k / 0.13))
intro = np.clip(T / 1.5, 0, 1) ** 2
pad_gain = np.where(T < 2.0, 0.22 * intro, np.where(T < 12.0, 0.2, 0.26))
place(wet, 0, pad * pad_gain * pump, 0.9)
place(dry, 0, pad * pad_gain * pump, 0.5)

# ---- drums and bass, 2 s to 12 s
for j, b in enumerate(beats):
    place(dry, b, kick(), 0.7)
    place(dry, b + 0.25, hat(), 0.16, pan=0.3)
    if j % 2 == 1:
        place(dry, b, clap(), 0.32)
        place(wet, b, clap(), 0.18)
    if j % 4 == 3:
        place(dry, b + 0.125, hat(), 0.09, pan=-0.3)
for k8 in np.arange(2.0, 12.0, 0.25):
    root = roots[int(k8 // 2)]
    m = root + (12 if int(k8 * 4) % 4 == 2 else 0)
    t = tt(0.24)
    s = np.sin(2 * np.pi * hz(m + 12) * t) + 0.25 * np.sin(4 * np.pi * hz(m + 12) * t)
    place(dry, k8 + 0.02, np.tanh(1.8 * s) * np.exp(-t / 0.12) * np.minimum(t / 0.004, 1), 0.2)

# ---- intro: shimmer riser into the logo pop, then a riser into the drop
sw = tt(0.6)
place(wet, 0.0, np.sin(2 * np.pi * np.cumsum(600 + 1800 * (sw / 0.6) ** 2) / SR) * (sw / 0.6) ** 2 * 0.15)
place(dry, 0.6, kick(0.6), 0.55)
for m, dt in [(81, 0), (88, 0.03), (93, 0.06)]:
    place(wet, 0.6 + dt, chime(m, 2.0, 0.6), 0.16)
place(dry, 0.0, noise_swell(0.62, 0.6, 3), 0.1)
place(dry, 1.25, noise_swell(0.8, 0.75, 4), 0.22)
rs = tt(0.75)
place(wet, 1.25, np.sin(2 * np.pi * np.cumsum(220 + 1400 * (rs / 0.75) ** 3) / SR) * (rs / 0.75) ** 2 * 0.08)

# ---- scene cuts: whoosh, panned left to right
for c in (2.0, 5.5, 9.0, 12.0):
    w = noise_swell(0.55, 0.42, 5)
    for p in (-0.6, 0.6):
        place(dry, c - 0.42, w, 0.14, pan=p)

# ---- UI sounds
def tick(f=2600, d=0.03):
    t = tt(d)
    return np.sin(2 * np.pi * f * t) * np.exp(-t / 0.006)

for i in range(4):
    place(dry, 2.38 + i * 0.08, tick(2200 + i * 200), 0.08, pan=0.4)
for i in range(5):
    place(dry, 5.62 + i * 0.07, tick(2000 + i * 180), 0.07, pan=-0.4)
place(wet, 4.05, chime(88, 1.0, 0.25), 0.1)  # tooltip
place(dry, 7.0, tick(3200, 0.02), 0.35)  # mouse click
place(dry, 7.0, rng.standard_normal(240) * np.exp(-tt(0.005) / 0.001), 0.25)
tb = tt(0.5)
place(dry, 7.03, np.sin(2 * np.pi * np.cumsum(140 * np.exp(-tb / 0.08) + 55) / SR) * np.exp(-tb / 0.15), 0.6)  # block thud
place(wet, 7.15, chime(76, 1.2, 0.3), 0.12)  # toast
for k in range(5):  # activity log notifications
    t0 = 9.3 + k * 0.42
    place(wet, t0, chime([84, 88, 86, 91, 79][k], 1.2, 0.3), 0.13)
    place(dry, t0, chime([84, 88, 86, 91, 79][k], 0.4, 0.12), 0.08)

# ---- end card: impact, sustained pluck chord, CTA pop, shine sparkle
place(dry, 12.0, kick(0.9), 1.0)
sub = tt(1.6)
place(dry, 12.0, np.sin(2 * np.pi * 41 * sub) * np.exp(-sub / 0.55), 0.5)
cr = rng.standard_normal(int(2.5 * SR))
place(wet, 12.0, (cr - smooth(cr, 4)) * np.exp(-tt(2.5) / 0.7), 0.06)
for k, m in enumerate([57, 64, 69, 71, 72, 76]):
    place(wet, 12.0 + k * 0.035, pluck(m, 3.0, 1.2), 0.07)
    place(dry, 12.0 + k * 0.035, pluck(m, 3.0, 1.2), 0.05)
place(wet, 12.68, chime(81, 1.4, 0.35), 0.12)
for k, m in enumerate([93, 96, 100]):
    place(wet, 13.32 + k * 0.06, chime(m, 1.5, 0.4), 0.06)

# ---- reverb: decaying noise impulse, different per channel
ir_t = tt(1.8)
out = dry.copy()
for ch in range(2):
    ir = rng.standard_normal(len(ir_t)) * np.exp(-ir_t / 0.42)
    ir = smooth(ir, 3)
    ir /= np.sqrt((ir ** 2).sum())
    L = N + len(ir)
    rev = np.fft.irfft(np.fft.rfft(wet[:, ch], L) * np.fft.rfft(ir, L), L)[:N]
    out[:, ch] += rev * 0.55 + wet[:, ch] * 0.35

out = np.tanh(out * 1.3)
out *= 10 ** (-1 / 20) / np.abs(out).max()
out *= np.clip(T / 0.01, 0, 1)[:, None] * np.clip((DUR - T) / 0.4, 0, 1)[:, None]
os.makedirs(os.path.join(os.path.dirname(__file__), "out"), exist_ok=True)
with wave.open(os.path.join(os.path.dirname(__file__), "out", "audio.wav"), "wb") as w:
    w.setnchannels(2)
    w.setsampwidth(2)
    w.setframerate(SR)
    w.writeframes((out * 32767).astype("<i2").tobytes())
print("peak", np.abs(out).max(), "rms dB", 20 * np.log10(np.sqrt((out ** 2).mean())))
