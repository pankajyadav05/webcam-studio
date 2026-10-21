#!/usr/bin/env python3
"""Writes the placeholder app icons (a teal rounded square) using only the standard library.

Run from anywhere: python3 apps/desktop/scripts/make_icons.py
"""

import struct
import zlib
from pathlib import Path

TEAL = (0x0F, 0x76, 0x6E)
OUT = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"


def render(size: int) -> bytes:
    """Returns a PNG (RGBA) of a teal rounded square with a soft edge."""
    half = size / 2
    inset = size * 0.06  # transparent margin
    radius = size * 0.22
    rows = []
    for y in range(size):
        row = bytearray([0])  # filter type: none
        for x in range(size):
            # Signed distance from the rounded-square outline (negative inside).
            dx = abs(x + 0.5 - half) - (half - inset - radius)
            dy = abs(y + 0.5 - half) - (half - inset - radius)
            outside = (max(dx, 0) ** 2 + max(dy, 0) ** 2) ** 0.5
            dist = outside + min(max(dx, dy), 0) - radius
            alpha = max(0.0, min(1.0, 0.5 - dist))
            row += bytes((*TEAL, round(alpha * 255)))
        rows.append(bytes(row))
    raw = b"".join(rows)

    def chunk(kind: bytes, data: bytes) -> bytes:
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def make_ico(sizes: list[int]) -> bytes:
    """Returns an ICO file whose entries are PNG images."""
    images = [render(s) for s in sizes]
    offset = 6 + 16 * len(sizes)
    head = struct.pack("<HHH", 0, 1, len(sizes))
    entries = b""
    for size, png in zip(sizes, images):
        dim = 0 if size >= 256 else size  # 0 means 256
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(png), offset)
        offset += len(png)
    return head + entries + b"".join(images)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for name, size in [
        ("32x32.png", 32),
        ("128x128.png", 128),
        ("128x128@2x.png", 256),
        ("icon.png", 512),
    ]:
        (OUT / name).write_bytes(render(size))
    (OUT / "icon.ico").write_bytes(make_ico([16, 32, 48, 256]))


if __name__ == "__main__":
    main()
