#!/usr/bin/env python3
"""Turn the owner's signature PNG into the raw pixels the Welcome card embeds.

`tangent-ui` has no image decoder, on purpose (see `scripts/make-icons.py`),
so the PNG is decoded here, offline, run by hand when the signature changes,
and what ships is the result: assets/signature/ganten.rgba, a bare block of
premultiplied RGBA, 8 bits a channel, row-major, top row first, WIDTH by
HEIGHT, no header. `dialogs.rs` picks it up with `include_bytes!` typed to
its exact length, so a file of the wrong size fails to compile.

The drawing is ink on nothing (8-bit RGBA over a transparent ground) and is
kept in its own colours, so it reads on every theme the card wears. It is
scaled down by a whole factor, each output pixel the average of a block of
input pixels in premultiplied light, so a stroke's edge fades into the
ground rather than into a dark fringe.

    python3 scripts/make-signature.py
"""
import os
import struct
import sys
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "..", "assets", "signature", "ganten.png")
DST = os.path.join(HERE, "..", "assets", "signature", "ganten.rgba")
FACTOR = 2


def decode_rgba8(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    pos, idat, width, height = 8, b"", 0, 0
    while pos < len(data):
        (n,) = struct.unpack(">I", data[pos:pos + 4])
        kind = data[pos + 4:pos + 8]
        body = data[pos + 8:pos + 8 + n]
        if kind == b"IHDR":
            width, height, depth, colour, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert (depth, colour, interlace) == (8, 6, 0), "want 8-bit RGBA, not interlaced"
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    raw = zlib.decompress(idat)
    stride, bpp = width * 4, 4
    out = bytearray(height * stride)
    prev = bytearray(stride)
    i = 0
    for y in range(height):
        f = raw[i]
        line = bytearray(raw[i + 1:i + 1 + stride])
        i += 1 + stride
        for x in range(stride):
            a = line[x - bpp] if x >= bpp else 0
            b = prev[x]
            c = prev[x - bpp] if x >= bpp else 0
            if f == 1:
                line[x] = (line[x] + a) & 255
            elif f == 2:
                line[x] = (line[x] + b) & 255
            elif f == 3:
                line[x] = (line[x] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[x] = (line[x] + pr) & 255
        out[y * stride:(y + 1) * stride] = line
        prev = line
    return width, height, out


def main():
    w, h, px = decode_rgba8(SRC)
    ow, oh = w // FACTOR, h // FACTOR
    dst = bytearray(ow * oh * 4)
    n = FACTOR * FACTOR
    for oy in range(oh):
        for ox in range(ow):
            r = g = b = a = 0
            for dy in range(FACTOR):
                row = (oy * FACTOR + dy) * w * 4
                for dx in range(FACTOR):
                    k = row + (ox * FACTOR + dx) * 4
                    al = px[k + 3]
                    r += px[k] * al
                    g += px[k + 1] * al
                    b += px[k + 2] * al
                    a += al
            o = (oy * ow + ox) * 4
            # Premultiplied: each channel is colour times coverage, averaged.
            dst[o] = round(r / 255 / n)
            dst[o + 1] = round(g / 255 / n)
            dst[o + 2] = round(b / 255 / n)
            dst[o + 3] = round(a / n)
    open(DST, "wb").write(bytes(dst))
    print(f"{ow}x{oh}, {len(dst)} bytes -> {DST}")


if __name__ == "__main__":
    sys.exit(main())
