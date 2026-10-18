#!/usr/bin/env python3
"""Pepe's faces as pictures: one PNG per mood, drawn from the big sprite in
src/ui/mascot.rs, so the pepper in Slack is the one in the terminal. A run's
Slack message takes the face of how the run is going (see src/webhook.rs);
pepe-icon.png is the Slack app's icon, 512 pixels square as Slack asks.

    python3 site/build-avatars.py           # write site/img/pepe-*.png
    python3 site/build-avatars.py --check   # exit 1 when they are stale

The standard library only: zlib does the compressing.
"""

import os
import re
import struct
import sys
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SPRITE = os.path.join(ROOT, "src", "ui", "mascot.rs")
OUT = os.path.join(ROOT, "site", "img")

# The sprite's pixels, as mascot.rs colours them in true colour
COLOURS = {
    "K": (0x2C, 0x0A, 0x0B), "R": (0xEF, 0x33, 0x24), "r": (0xB8, 0x1C, 0x1E),
    "H": (0xFF, 0xA0, 0x82), "G": (0x7A, 0xC8, 0x46), "g": (0x2E, 0x78, 0x32),
    "S": (0x2E, 0x78, 0x32), "W": (0xFF, 0xFF, 0xFF), "P": (0x24, 0x08, 0x0A),
    "M": (0x24, 0x08, 0x0A), "B": (0xFF, 0x80, 0x9A), "T": (0xFF, 0x80, 0x9A),
    "Y": (0xFF, 0xD2, 0x50), "O": (0xFF, 0x78, 0x28), "C": (0x8C, 0xD2, 0xFF),
}
# Behind him: warm cream, which reads on Slack's light and dark themes alike
BACKGROUND = (0xFF, 0xF1, 0xE0)
# 26 pixels of sprite in a 32-pixel square, 16 screen pixels each
GRID, SCALE = 32, 16


def sprites():
    """Each mood's rows, by the name in the comment above them"""
    text = open(SPRITE, encoding="utf-8").read()
    big = text[text.index("const BIG:"):]
    big = big[: big.index("\n];") + 3]
    out = {}
    for name, body in re.findall(r"// (\w+)\s*\[(.*?)\]", big, re.S):
        out.setdefault(name, re.findall(r'"([^"]+)"', body))
    return out


def png(rows):
    width = height = GRID * SCALE
    pad_x = (GRID - len(rows[0])) // 2
    pad_y = (GRID - len(rows)) // 2
    lines = []
    for y in range(height):
        row = bytearray([0])  # no filter
        sy = y // SCALE - pad_y
        for x in range(width):
            sx = x // SCALE - pad_x
            pixel = rows[sy][sx] if 0 <= sy < len(rows) and 0 <= sx < len(rows[0]) else "."
            row += bytes(COLOURS.get(pixel, BACKGROUND))
        lines.append(bytes(row))
    raw = b"".join(lines)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def build():
    faces = sprites()
    files = {
        f"pepe-{name.replace('_', '-')}.png": png(rows)
        for name, rows in faces.items()
        if "blink" not in name
    }
    files["pepe-icon.png"] = files["pepe-happy.png"]
    return files


def main():
    check = "--check" in sys.argv
    stale = []
    for name, data in build().items():
        path = os.path.join(OUT, name)
        current = open(path, "rb").read() if os.path.exists(path) else None
        if current == data:
            continue
        stale.append(name)
        if not check:
            with open(path, "wb") as f:
                f.write(data)
    if check:
        if stale:
            print(f"stale: {', '.join(stale)}; run python3 site/build-avatars.py")
            sys.exit(1)
        print("site/img avatars are current")
    else:
        print(f"wrote {len(stale)} file(s)" if stale else "nothing to write")


if __name__ == "__main__":
    main()
