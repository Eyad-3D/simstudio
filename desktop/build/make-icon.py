"""Generate build/icon.png — the app icon, matching the browser favicon.

Written by hand rather than exported from a design tool so the icon can be
regenerated anywhere without an image toolchain installed.
"""
from __future__ import annotations

import math
import struct
import zlib
from pathlib import Path

SIZE = 256
SS = 3  # supersample factor, for smooth edges
BLUE = (47, 111, 179)
WHITE = (255, 255, 255)


def rounded_rect(x: float, y: float, radius: float) -> bool:
    r, lo, hi = radius, radius, SIZE - radius
    cx = lo if x < lo else hi if x > hi else x
    cy = lo if y < lo else hi if y > hi else y
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def ring(x: float, y: float, cx: float, cy: float, rad: float, w: float) -> bool:
    d = math.hypot(x - cx, y - cy)
    return rad - w / 2 <= d <= rad + w / 2


def segment(x, y, x1, y1, x2, y2, w) -> bool:
    dx, dy = x2 - x1, y2 - y1
    length2 = dx * dx + dy * dy
    t = max(0.0, min(1.0, ((x - x1) * dx + (y - y1) * dy) / length2))
    return math.hypot(x - (x1 + t * dx), y - (y1 + t * dy)) <= w / 2


def shade(x: float, y: float):
    """Colour for a point, or None for transparent."""
    if not rounded_rect(x, y, SIZE * 6 / 32):
        return None
    s = SIZE / 32  # the favicon is authored on a 32-unit grid
    if ring(x, y, 10 * s, 16 * s, 4 * s, 2 * s):
        return WHITE
    if ring(x, y, 24 * s, 9 * s, 3.5 * s, 2 * s):
        return WHITE
    if ring(x, y, 24 * s, 23 * s, 3.5 * s, 2 * s):
        return WHITE
    if segment(x, y, 13 * s, 14.5 * s, 20.8 * s, 10 * s, 2 * s):
        return WHITE
    if segment(x, y, 13 * s, 17.5 * s, 20.8 * s, 22 * s, 2 * s):
        return WHITE
    return BLUE


def render() -> bytes:
    rows = []
    for py in range(SIZE):
        row = bytearray()
        for px in range(SIZE):
            acc = [0, 0, 0, 0]
            for sy in range(SS):
                for sx in range(SS):
                    x = px + (sx + 0.5) / SS
                    y = py + (sy + 0.5) / SS
                    c = shade(x, y)
                    if c is not None:
                        acc[0] += c[0]
                        acc[1] += c[1]
                        acc[2] += c[2]
                        acc[3] += 255
            n = SS * SS
            hits = acc[3] / 255 or 1
            row += bytes((
                round(acc[0] / hits), round(acc[1] / hits),
                round(acc[2] / hits), round(acc[3] / n),
            ))
        rows.append(bytes(row))
    return b"".join(b"\x00" + r for r in rows)


def chunk(tag: bytes, data: bytes) -> bytes:
    return (struct.pack(">I", len(data)) + tag + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF))


def main() -> None:
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(render(), 9))
        + chunk(b"IEND", b"")
    )
    out = Path(__file__).with_name("icon.png")
    out.write_bytes(png)
    print(f"wrote {out} ({len(png)} bytes)")


if __name__ == "__main__":
    main()
