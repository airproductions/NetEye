"""Writes assets/neteye.ico: the eye logo (same drawing as theme::logo_rgba) at 16-256 px.

Usage: python -I tools/make_icon.py assets/neteye.ico
Each size is stored as a PNG inside the .ico, which Windows Vista and later read.
"""
import math
import struct
import sys
import zlib


def logo(size):
    s = float(size)
    rows = []
    for y in range(size):
        row = bytearray([0])
        for x in range(size):
            fx, fy = x + 0.5, y + 0.5
            r = s * 0.22
            dx = abs(fx - s / 2) - (s / 2 - r)
            dy = abs(fy - s / 2) - (s / 2 - r)
            d = math.hypot(max(dx, 0), max(dy, 0)) - r
            a = min(max(0.5 - d, 0), 1)
            if a <= 0:
                row += bytes(4)
                continue
            t = (fx + fy) / (2 * s)
            c0, c1 = (0x3D, 0x7B, 0xFF), (0x14, 0xC8, 0xB4)
            col = [c0[i] + (c1[i] - c0[i]) * t for i in range(3)]
            px, py = fx / s - 0.5, fy / s - 0.5
            lens = max(math.hypot(px, py - 0.16), math.hypot(px, py + 0.16)) - 0.34
            outline = abs(lens) * s - s * 0.05
            pupil = math.hypot(px, py) * s - s * 0.10
            white = min(max(0.5 - min(outline, pupil), 0), 1)
            col = [c + (255 - c) * white for c in col]
            row += bytes([int(col[0]), int(col[1]), int(col[2]), int(a * 255)])
        rows.append(bytes(row))

    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")


def main(out):
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    images = [logo(n) for n in sizes]
    head = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    entries = b""
    for n, img in zip(sizes, images):
        entries += struct.pack("<BBBBHHII", n % 256, n % 256, 0, 0, 1, 32, len(img), offset)
        offset += len(img)
    with open(out, "wb") as f:
        f.write(head + entries + b"".join(images))
    print(f"wrote {out} ({offset} bytes, {len(sizes)} sizes)")


if __name__ == "__main__":
    main(sys.argv[1])
