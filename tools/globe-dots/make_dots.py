"""Builds assets/geo/globe-dots.bin from Natural Earth's 1:110m Admin-0 countries.

Usage: python -I make_dots.py <ne_110m_admin_0_countries.geojson> <out.bin>

Points are spread evenly over the sphere (Fibonacci lattice); each one that falls inside a
country polygon is kept and tagged with that country. Every country also gets its Natural
Earth label point, which the globe uses for hotspots.

Output (little-endian):
  b"NEDOTS1\\0"
  u16 country count, then per country: 2 ASCII bytes ISO 3166-1 alpha-2, f32 lat, f32 lon
  u32 dot count, then per dot: i16 lat*100, i16 lon*100, u8 country index
"""
import json
import math
import struct
import sys

N = 24_000

# Places too small for the 1:110m map that still show up in traffic: (code, lat, lon).
EXTRA = [
    ("SG", 1.35, 103.82), ("HK", 22.32, 114.17), ("MO", 22.20, 113.55), ("LU", 49.82, 6.13), ("MT", 35.94, 14.38),
    ("BH", 26.07, 50.56), ("MC", 43.74, 7.42), ("LI", 47.17, 9.56), ("AD", 42.55, 1.60), ("SM", 43.94, 12.46),
    ("MU", -20.35, 57.55), ("BB", 13.19, -59.54), ("MV", 3.20, 73.22), ("SC", -4.68, 55.49), ("GI", 36.14, -5.35),
    ("JE", 49.21, -2.13), ("GG", 49.45, -2.58), ("IM", 54.24, -4.55), ("BM", 32.32, -64.76), ("KY", 19.31, -81.25),
    ("AG", 17.06, -61.80), ("LC", 13.91, -60.98), ("GD", 12.12, -61.68), ("KN", 17.36, -62.78), ("VG", 18.42, -64.64),
    ("CW", 12.17, -68.98), ("AW", 12.52, -69.97), ("PF", -17.68, -149.41), ("RE", -21.12, 55.54), ("MQ", 14.64, -61.02),
    ("GP", 16.27, -61.55), ("AS", -14.27, -170.13), ("GU", 13.44, 144.79), ("TO", -21.18, -175.20), ("WS", -13.76, -172.10),
]


def iso2(p):
    for k in ("ISO_A2_EH", "ISO_A2"):
        v = p.get(k)
        if v and v != "-99" and len(v) == 2:
            return v
    return None


def rings(geom):
    polys = geom["coordinates"] if geom["type"] == "MultiPolygon" else [geom["coordinates"]]
    return polys  # list of polygons; each is [outer, hole, hole...]


def inside(ring, lon, lat):
    c = False
    j = len(ring) - 1
    for i in range(len(ring)):
        xi, yi = ring[i]
        xj, yj = ring[j]
        if (yi > lat) != (yj > lat) and lon < (xj - xi) * (lat - yi) / (yj - yi) + xi:
            c = not c
        j = i
    return c


def main(src, out):
    feats = json.load(open(src, encoding="utf-8"))["features"]
    countries = []  # (code, lat, lon, polygons, bbox)
    for f in feats:
        code = iso2(f["properties"])
        if not code:
            continue
        polys = rings(f["geometry"])
        xs = [x for poly in polys for x, _ in poly[0]]
        ys = [y for poly in polys for _, y in poly[0]]
        countries.append((code, f["properties"]["LABEL_Y"], f["properties"]["LABEL_X"], polys, (min(xs), min(ys), max(xs), max(ys))))
    known = {c[0] for c in countries}
    table = [(c[0], c[1], c[2]) for c in countries] + [e for e in EXTRA if e[0] not in known]

    dots = []
    golden = math.pi * (3 - math.sqrt(5))
    for i in range(N):
        y = 1 - 2 * (i + 0.5) / N
        lat = math.degrees(math.asin(y))
        lon = math.degrees((golden * i) % (2 * math.pi))
        if lon > 180:
            lon -= 360
        for ci, (_, _, _, polys, (x0, y0, x1, y1)) in enumerate(countries):
            if not (x0 <= lon <= x1 and y0 <= lat <= y1):
                continue
            if any(inside(poly[0], lon, lat) and not any(inside(h, lon, lat) for h in poly[1:]) for poly in polys):
                dots.append((lat, lon, ci))
                break

    with open(out, "wb") as fh:
        fh.write(b"NEDOTS1\0")
        fh.write(struct.pack("<H", len(table)))
        for code, lat, lon in table:
            fh.write(code.encode("ascii") + struct.pack("<ff", lat, lon))
        fh.write(struct.pack("<I", len(dots)))
        for lat, lon, ci in dots:
            fh.write(struct.pack("<hhB", round(lat * 100), round(lon * 100), ci))
    print(f"{len(table)} countries, {len(dots)} land dots of {N}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
