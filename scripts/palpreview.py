#!/usr/bin/env python3
"""Render the designed palettes as a mixer mock-up, so they are LOOKED AT
before they are written into Rust. Pure-stdlib PNG, no PIL."""
import struct
import zlib

from palgen import PALETTES, INK_LIGHT, INK_DARK, wants_light_ink

SW, SH = 34, 96          # one strip
GAP, PAD = 3, 10
ROWGAP = 16
BG = (0x14, 0x12, 0x10)


class Canvas:
    def __init__(self, w, h, bg):
        self.w, self.h = w, h
        self.px = bytearray()
        for _ in range(h):
            self.px += bytes(bg) * w

    def rect(self, x, y, w, h, c):
        for yy in range(max(0, y), min(self.h, y + h)):
            base = yy * self.w * 3
            for xx in range(max(0, x), min(self.w, x + w)):
                o = base + xx * 3
                self.px[o:o + 3] = bytes(c)

    def png(self, path):
        raw = b""
        stride = self.w * 3
        for y in range(self.h):
            raw += b"\x00" + bytes(self.px[y * stride:(y + 1) * stride])

        def chunk(tag, data):
            c = struct.pack(">I", len(data)) + tag + data
            return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

        hdr = struct.pack(">IIBBBBB", self.w, self.h, 8, 2, 0, 0, 0)
        with open(path, "wb") as f:
            f.write(b"\x89PNG\r\n\x1a\n")
            f.write(chunk(b"IHDR", hdr))
            f.write(chunk(b"IDAT", zlib.compress(raw, 9)))
            f.write(chunk(b"IEND", b""))


def strip(cv, x, y, face):
    """One channel: the face, then everything printed straight onto it in
    the ink the face has earned -- the same rule the Rust uses."""
    ink = INK_LIGHT if wants_light_ink(face) else INK_DARK
    faint = tuple(round(i * 0.58 + f * 0.42) for i, f in zip(ink, face))
    cv.rect(x, y, SW, SH, face)
    cv.rect(x + 3, y + 4, SW - 6, 9, faint)        # the name plate
    cv.rect(x + 5, y + 6, SW - 16, 5, ink)         # the name
    cv.rect(x + 6, y + 18, SW - 12, SW - 12, ink)  # the icon
    cv.rect(x + 4, y + 44, SW - 8, 4, faint)       # second line
    cv.rect(x + 4, y + 54, 9, 26, (0x1A, 0x17, 0x12))   # meter well
    cv.rect(x + 5, y + 62, 7, 18, (0x3D, 0xC0, 0x5A))   # meter
    cv.rect(x + 18, y + 54, 5, 26, (0x24, 0x21, 0x1C))  # fader slot
    cv.rect(x + 16, y + 62, 9, 6, (0xE8, 0xDC, 0xC0))   # fader cap
    cv.rect(x + 6, y + 84, SW - 12, 5, ink)        # the dB readout


def main():
    rows = len(PALETTES)
    w = PAD * 2 + 18 * SW + 17 * GAP + 14        # 16 channels + click + master
    h = PAD * 2 + rows * SH + (rows - 1) * ROWGAP
    cv = Canvas(w, h, BG)
    for r, (name, (slots, click, master, _)) in enumerate(PALETTES.items()):
        y = PAD + r * (SH + ROWGAP)
        for i, c in enumerate(slots):
            strip(cv, PAD + i * (SW + GAP), y, c)
        x = PAD + 16 * (SW + GAP) + 14
        strip(cv, x, y, click)
        strip(cv, x + SW + GAP, y, master)
        print(f"row {r + 1}: {name}")
    cv.png("palettes.png")
    print(f"\nwrote palettes.png ({w}x{h})")


if __name__ == "__main__":
    main()
