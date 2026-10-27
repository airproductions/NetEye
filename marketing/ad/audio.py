"""Synthesizes the ad's 15 s cinematic-tech soundtrack, with hits synced to ad.html's timeline -> out/audio.wav

Drone and reverse swells in the intro, brass "braam" hits on every cut, a 16th-note bass ostinato and clock
ticks that build through the three feature scenes, a dropout before the end card, then a big final hit.
"""
import json, os, wave
import numpy as np

SR, DUR = 48000, 15.0
N = int(SR * DUR)
T = np.arange(N) / SR
rng = np.random.default_rng(11)
dry = np.zeros((N, 2))
bed = np.zeros((N, 2))  # drone, bass and pad: the only bus the drums duck
wet = np.zeros((N, 2))  # sent through the reverb
# cue times shared with ad.html (timeline.js), so picture and sound can't drift apart
_src = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "timeline.js"), encoding="utf-8").read()
_obj = _src.index("{", _src.index("window.CUES"))
CUES = json.loads(_src[_obj : _src.rindex("}") + 1])
HITS = CUES["cuts"]
DROP = CUES["dropout"]


def hz(m):
    return 440.0 * 2 ** ((m - 69) / 12)


def tt(d):
    return np.arange(int(d * SR)) / SR


def place(buf, t0, sig, gain=1.0, pan=0.0):
    i = int(round(t0 * SR))
    if i < 0:  # starts before the spot: keep the part after 0 s
        sig, i = sig[-i:], 0
    if i >= N or len(sig) == 0:
        return
    sig = sig[: N - i] * gain
    buf[i : i + len(sig), 0] += sig * np.sqrt(0.5 * (1 - pan))
    buf[i : i + len(sig), 1] += sig * np.sqrt(0.5 * (1 + pan))


def smooth(x, n):
    return np.convolve(x, np.ones(n) / n, mode="same")


def saw(f, t, bright):
    """Band-limited saw; `bright` (array or float) sets how fast the upper harmonics fall away."""
    out = np.zeros(len(t))
    for k in range(1, 40):
        if f * k > 14000:
            break
        out += np.sin(2 * np.pi * f * k * t + k * 1.3) / k * np.exp(-(k - 1) / np.maximum(bright, 0.3))
    return out


def braam(notes, d=3.0, peak=0.25):
    """Brassy cinematic hit: detuned saw stack whose filter opens fast and closes slowly."""
    t = tt(d)
    bright = 1 + 16 * np.exp(-t / 0.35) * np.minimum(t / 0.06, 1)
    s = sum(saw(hz(m) * (1 + det), t, bright) for m in notes for det in (-0.004, 0, 0.004))
    env = np.minimum(t / 0.04, 1) * np.exp(-t / (d * peak * 2.2))
    return s * env / (len(notes) * 3)


def taiko(d=0.9, f0=78, f1=42):
    t = tt(d)
    f = f1 + (f0 - f1) * np.exp(-t / 0.05)
    body = np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t / 0.32)
    n = rng.standard_normal(len(t))
    skin = smooth(n, 12) * np.exp(-t / 0.04) * 1.5
    return np.tanh(1.4 * (body + skin))


def tick(f=3800, d=0.03):
    t = tt(d)
    return (np.sin(2 * np.pi * f * t) + 0.5 * np.sin(2 * np.pi * f * 1.51 * t)) * np.exp(-t / 0.005)


def glass(m, d=1.6, decay=0.45):
    t = tt(d)
    f = hz(m)
    return sum(a * np.sin(2 * np.pi * f * k * t) * np.exp(-t / (decay / k ** 0.6)) for k, a in [(1, 1), (2.76, 0.4), (5.4, 0.15)]) * np.minimum(t / 0.002, 1)


def reverse_swell(d=0.7):
    t = tt(d)
    n = smooth(rng.standard_normal(len(t)), 8)
    return n * (t / d) ** 3


# ---- drone: A1 + E2 with slow movement, present the whole spot
dr = (np.sin(2 * np.pi * 55 * T) + 0.5 * np.sin(2 * np.pi * 82.4 * T + 1) + 0.25 * np.sin(2 * np.pi * 110.2 * T)) * (0.8 + 0.2 * np.sin(2 * np.pi * 0.3 * T))
dr_env = np.clip(T / 1.2, 0, 1) * np.where(T < 12, 1, 0.6)
place(bed, 0, dr * dr_env, 0.12)
place(wet, 0, saw(55, T, 2.5 + 1.5 * np.sin(2 * np.pi * 0.2 * T)) * dr_env, 0.05)

# ---- harmony pad that follows the scenes: Am | F | Dm -> E | Am
pad = np.zeros(N)
h0, h1, h2, h3 = HITS
for t0, t1, ch in [(0, h0 + 0.1, [57, 64, 69]), (h0, h1 + 0.1, [57, 60, 64]), (h1, h2 + 0.1, [53, 57, 60]), (h2, 10.6, [50, 57, 62]), (10.5, h3, [52, 56, 59, 64]), (h3, DUR, [57, 64, 71, 72])]:
    t = tt(t1 - t0 + 0.4)
    env = np.minimum(t / 0.5, 1) * np.clip((t1 - t0 + 0.4 - t) / 0.4, 0, 1)
    seg = sum(saw(hz(m) * (1 + det), t, 3.0) for m in ch for det in (-0.003, 0.003)) * env / len(ch)
    i = int(t0 * SR)
    pad[i : i + len(seg)] += seg[: N - i]
place(wet, 0, pad, 0.11)
place(bed, 0, pad, 0.035)

# ---- intro: reverse swell into the logo hit at 0.6 s, ticks creep in, riser into the 2 s drop
place(wet, 0.6 - 0.7, reverse_swell(0.7), 0.12)
place(dry, 0.6, taiko(1.2, 70, 38), 0.5)
place(dry, 0.6, braam([33, 45, 52], 2.2, 0.35), 0.5)
for k, m in enumerate([81, 88, 93]):
    place(wet, 0.62 + k * 0.04, glass(m, 1.8, 0.5), 0.07)
for k, t0 in enumerate(np.arange(1.0, 2.0, 0.25)):
    place(dry, t0, tick(), 0.05 + 0.02 * k, pan=0.4 if k % 2 else -0.4)
rs = tt(0.9)
riser = sum(np.sin(2 * np.pi * np.cumsum(base * 2 ** (1.4 * (rs / 0.9) ** 2)) / SR) for base in (110, 220, 440)) / 3
place(wet, 1.1, riser * (rs / 0.9) ** 2.5, 0.09)
place(dry, 1.1, smooth(rng.standard_normal(len(rs)), 3) * (rs / 0.9) ** 4, 0.12)

# ---- every cut: reverse swell, braam, taiko, sub drop
for h, notes in zip(HITS, [[33, 45, 52, 57], [29, 41, 48, 53], [26, 38, 45, 50], [33, 45, 52, 57, 64]]):
    big = h == HITS[-1]
    # the big hit's swell ends where the dropout starts, so its peak isn't muted
    place(wet, (DROP if big else h) - 0.6, reverse_swell(0.6), 0.16 if big else 0.1)
    b = braam(notes, 3.2 if big else 2.4, 0.4 if big else 0.3)
    place(dry, h, b, 0.75 if big else 0.55)
    place(wet, h, b, 0.25)
    place(dry, h, taiko(1.4 if big else 0.9), 0.9 if big else 0.7)
    sd = tt(1.5 if big else 0.8)
    place(dry, h, np.sin(2 * np.pi * np.cumsum(30 + 35 * np.exp(-sd / 0.25)) / SR) * np.exp(-sd / (0.6 if big else 0.3)), 0.5)

# ---- 16th-note bass ostinato and clock ticks, first cut to the dropout, building in level and density
roots = lambda t: 33 if t < HITS[1] else 29 if t < HITS[2] else (26 if t < 10.5 else 28)
duck = np.ones(N)  # sidechain from the driving drum, applied to the bed bus only
END = DROP - 0.01  # the build stops just before the dropout
ticks = 0
for t0 in np.arange(HITS[0], END, 0.125):
    step = int(round((t0 - 2.0) / 0.125))
    m = roots(t0) + (12 if step % 4 == 2 else 0) + (7 if step % 8 == 7 else 0)
    t = tt(0.12)
    note = saw(hz(m), t, 2.2 + 3 * np.exp(-t / 0.02)) * np.exp(-t / 0.06) * np.minimum(t / 0.002, 1)
    level = 0.08 + 0.12 * (t0 - 2.0) / 10.0
    place(bed, t0, note, level * (1.15 if step % 4 == 0 else 1), pan=0.15 * np.sin(step))
    if step % 2 == 0 or t0 >= HITS[2]:
        ticks += 1  # count the ticks themselves, so they alternate sides whatever the density
        place(dry, t0, tick(4200 if step % 4 == 0 else 3600), 0.035 + 0.03 * (t0 - 2) / 10, pan=-0.5 if ticks % 2 else 0.5)
for t0 in np.arange(HITS[0], END, 1.0):  # driving low drum every half bar
    place(dry, t0, taiko(0.6, 64, 40), 0.35)
    i = int(t0 * SR)
    k = np.arange(N - i) / SR
    duck[i:] = np.minimum(duck[i:], 1 - 0.45 * np.exp(-k / 0.15))

# ---- tension riser into the end card, then a dropout right before the hit
rs = tt(1.35)
shep = sum(np.sin(2 * np.pi * np.cumsum(b * 2 ** (2.0 * (rs / 1.35) ** 1.5)) / SR) for b in (82.4, 164.8, 329.6, 659.2)) / 4
place(wet, 10.5, shep * (rs / 1.35) ** 2, 0.12)
place(dry, 10.5, smooth(rng.standard_normal(len(rs)), 2) * (rs / 1.35) ** 4, 0.14)
for k, t0 in enumerate(np.arange(11.0, END, 0.0625)):  # snare-ish roll
    n = rng.standard_normal(int(0.06 * SR))
    place(dry, t0, (n - smooth(n, 20)) * np.exp(-tt(0.06) / 0.02), 0.04 + 0.12 * k / 14, pan=0.2 if k % 2 else -0.2)

# ---- UI sound design, glassy and quiet under the music
LAG = 0.03  # a row's tick lands just after it starts to move
r0, rs = CUES["trafficRows"]
for i in range(4):
    place(dry, r0 + LAG + i * rs, tick(2600 + i * 300), 0.05, pan=0.5)
r0, rs = CUES["firewallRows"]
for i in range(5):
    place(dry, r0 + LAG + i * rs, tick(2400 + i * 260), 0.045, pan=-0.5)
place(wet, CUES["tooltip"] + 0.05, glass(88, 1.0, 0.25), 0.06, pan=0.4)  # tooltip
click = CUES["click"]
place(dry, click, tick(5200, 0.015), 0.3)  # mouse click
tb = tt(0.6)
place(dry, click + 0.03, np.sin(2 * np.pi * np.cumsum(120 * np.exp(-tb / 0.06) + 48) / SR) * np.exp(-tb / 0.2), 0.55)  # block thud
place(wet, click + 0.03, braam([41, 48], 1.2, 0.2), 0.2)
place(wet, CUES["toast"], glass(76, 1.0, 0.3), 0.06)  # toast
e0, es = CUES["events"]
for k in range(5):  # activity log notifications
    place(wet, e0 + k * es, glass([81, 84, 88, 86, 76][k], 1.0, 0.25), 0.07, pan=0.4)

# ---- end card: shimmer chord blooming over the final hit, sparkle on the rule's light
sh = tt(3.0)
shimmer = sum(np.sin(2 * np.pi * hz(m) * sh) * (0.7 + 0.3 * np.sin(2 * np.pi * (5 + k) * sh)) for k, m in enumerate([81, 88, 93, 95]))
place(wet, HITS[-1], shimmer * np.minimum(sh / 0.3, 1) * np.exp(-sh / 1.4) / 4, 0.12)
cr = rng.standard_normal(int(3.0 * SR))
place(wet, HITS[-1], (cr - smooth(cr, 4)) * np.exp(-tt(3.0) / 0.8), 0.05)
for k, m in enumerate([93, 96, 100]):
    place(wet, CUES["ruleLight"] + 0.02 + k * 0.07, glass(m, 1.4, 0.4), 0.04)

# ---- reverb: long decaying-noise impulse, decorrelated per channel
ir_t = tt(2.8)
out = dry + bed * duck[:, None]
for ch in range(2):
    ir = smooth(rng.standard_normal(len(ir_t)), 4) * np.exp(-ir_t / 0.7)
    ir /= np.sqrt((ir ** 2).sum())
    L = N + len(ir)
    rev = np.fft.irfft(np.fft.rfft(wet[:, ch], L) * np.fft.rfft(ir, L), L)[:N]
    out[:, ch] += rev * 0.6 + wet[:, ch] * 0.3

# dropout just before the end-card hit, a fade at the end
gap = 1 - 0.85 * np.clip((T - DROP) / 0.03, 0, 1) * np.clip((HITS[-1] - T) / 0.005, 0, 1)
out *= gap[:, None]
out = np.tanh(out * 1.4)
out *= 10 ** (-1 / 20) / np.abs(out).max()
out *= np.clip(T / 0.01, 0, 1)[:, None] * np.clip((DUR - T) / 0.5, 0, 1)[:, None]
os.makedirs(os.path.join(os.path.dirname(__file__), "out"), exist_ok=True)
with wave.open(os.path.join(os.path.dirname(__file__), "out", "audio.wav"), "wb") as w:
    w.setnchannels(2)
    w.setsampwidth(2)
    w.setframerate(SR)
    w.writeframes((out * 32767).astype("<i2").tobytes())
for i in range(0, 30, 2):
    s = out[i * SR // 2 : (i + 2) * SR // 2]
    print(f"{i / 2:4.1f}s {20 * np.log10(np.sqrt((s ** 2).mean()) + 1e-9):6.1f} dB")
