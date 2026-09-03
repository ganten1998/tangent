#!/usr/bin/env python3
"""Turn the track-icon PNGs into the raw coverage the app embeds.

The set is openly licensed artwork the owner sourced and has full rights to
use. It was not drawn in-house, and nothing that ships should say it was.

`tangent-ui` depends on egui and nothing else, on purpose - its `Cargo.toml`
says that what is NOT a dependency there is the point - so there is no image
crate in the GUI and there is never going to be one. That leaves exactly one
place for a PNG decoder: here, offline, run by hand when a drawing changes.
What ships in the binary is the decoded result, one byte of coverage per
pixel, which `icons.rs` picks up with `include_bytes!` and hands to egui as a
mask.

The output is deliberately the dumbest format that could work: no header, no
palette, no compression - a bare 192x192 block of 8-bit coverage, row-major,
top row first. There is nothing in it to parse and so nothing to get wrong at
startup; a file of the wrong length is the only way it can be malformed, and
`icons.rs` catches that at COMPILE time rather than drawing a smear.

WHAT THE COVERAGE IS. The drawings are ink on nothing: every PNG here is
8-bit RGBA over a fully transparent ground, and some are drawn in white while
others are drawn in black. So the ALPHA channel is the shape and the colour
channels are not consulted at all - the same decision the app makes
downstream, where an icon is a mask and the THEME picks the ink. Reading
luminance instead would make the white drawings vanish on a light theme and
the black ones vanish on a dark one.

WHY EACH DRAWING IS TRIMMED TO ITS OWN BOUNDS. These arrive at
different scales inside their canvases - `dynamic` is a thin microphone using
a fifth of its frame, `acoustic` a guitar filling nine tenths of it. Embedding
them as drawn would put a huge guitar next to a tiny microphone in the same
picker row. Trimming each to the box its ink actually occupies and then
squaring that box makes every icon arrive at the same size, so the picker
reads as one set rather than as thirty-five drawings that happen to be
together. Squaring rather than stretching is what keeps a trombone long: the
SHORT axis is centred in the square, never scaled to fill it.

WHY THE PAD. The content stops short of the edge by `PAD` pixels on every
side. Two reasons. A drawing flush to the edge has no antialiased boundary
there, so it ends in a hard cut that reads as damage rather than as a line -
and `saxophone` and `violin` are drawn to within a few pixels of their
canvases, so this is not hypothetical. And the mixer strip's badge rect
reaches a whisker past the band it was laid out in, which was harmless for
the vector badges this set replaces because every one of them inset itself.
The pad is the margin those drawings used to draw for themselves.

WHY AREA AVERAGING. Going from ~1200px to 176 is a factor of seven, and
point-sampling at that ratio drops whole strokes: a guitar string one source
pixel wide lands between samples and the instrument loses its strings. Every
destination pixel here is the exact area-weighted mean of the source region
it covers, fractional edges included, so a stroke thinner than a destination
pixel survives as a fainter one instead of disappearing. It is done as two
one-dimensional passes, x then y, which is arithmetically identical to the
two-dimensional average and fast enough in plain Python to not want numpy -
another dependency this repo does not need.

    scripts/make-icons.py ~/Dropbox/tanicons2 assets/icons

Both arguments are optional and default to exactly that pair.
"""

import math
import os
import struct
import sys
import zlib

# The side of the block that ships, in pixels. Big enough that the largest
# place an icon is ever drawn - the picker at a couple of dozen points on a
# 2x display - is still sampling DOWN, and small enough that thirty-five of
# them come to about a megabyte of binary.
SIZE = 192

# The clear margin, per side, in output pixels. See WHY THE PAD above.
PAD = 8

# Coverage at or below this counts as nothing when the bounding box is
# measured. Not zero: some of these drawings carry a haze of 1s and 2s from
# whatever exported them, invisible when drawn but enough to push the box out
# to the whole canvas and undo the trim.
FLOOR = 8


def decode_png(path):
    """The alpha plane of an 8-bit RGBA PNG, as (width, height, bytes).

    Only the shape these drawings are actually in: 8 bits per channel, colour
    type 6, no interlacing. Anything else raises rather than guessing, because
    a redraw exported as a palette PNG would otherwise convert to a smear and
    nobody would find out until it was on screen.
    """
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"{path}: not a PNG")
    pos, idat, hdr = 8, [], None
    while pos + 8 <= len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            hdr = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
    if hdr is None:
        raise SystemExit(f"{path}: no IHDR")
    width, height, depth, colour, _comp, _filt, interlace = hdr
    if (depth, colour, interlace) != (8, 6, 0):
        raise SystemExit(
            f"{path}: need 8-bit RGBA, not interlaced; got depth={depth} "
            f"colour-type={colour} interlace={interlace}"
        )
    raw = zlib.decompress(b"".join(idat))

    # Undo the per-row filters. PNG's five filters each predict a byte from
    # its left neighbour, the byte above, or both; the loop is transcribed
    # from the spec because there is no library here to call.
    bpp, stride = 4, width * 4
    alpha = bytearray(width * height)
    prev = bytearray(stride)
    at = 0
    for y in range(height):
        kind = raw[at]
        at += 1
        line = bytearray(raw[at : at + stride])
        at += stride
        if kind == 1:  # Sub
            for i in range(bpp, stride):
                line[i] = (line[i] + line[i - bpp]) & 255
        elif kind == 2:  # Up
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 255
        elif kind == 3:  # Average
            for i in range(stride):
                left = line[i - bpp] if i >= bpp else 0
                line[i] = (line[i] + ((left + prev[i]) >> 1)) & 255
        elif kind == 4:  # Paeth
            for i in range(stride):
                left = line[i - bpp] if i >= bpp else 0
                upleft = prev[i - bpp] if i >= bpp else 0
                up = prev[i]
                guess = left + up - upleft
                da, db, dc = abs(guess - left), abs(guess - up), abs(guess - upleft)
                near = left if (da <= db and da <= dc) else (up if db <= dc else upleft)
                line[i] = (line[i] + near) & 255
        elif kind != 0:
            raise SystemExit(f"{path}: row {y} has filter {kind}")
        alpha[y * width : (y + 1) * width] = line[3::4]
        prev = line
    return width, height, alpha


def bounds(width, height, alpha):
    """The box the ink actually occupies, as (x0, y0, x1, y1) inclusive."""
    x0, y0, x1, y1 = width, height, -1, -1
    for y in range(height):
        row = alpha[y * width : (y + 1) * width]
        lo = next((x for x, v in enumerate(row) if v > FLOOR), None)
        if lo is None:
            continue
        hi = width - 1 - next(x for x, v in enumerate(reversed(row)) if v > FLOOR)
        y0, y1 = min(y0, y), y
        x0, x1 = min(x0, lo), max(x1, hi)
    if x1 < 0:
        raise SystemExit("blank drawing: nothing in it is above the coverage floor")
    return x0, y0, x1, y1


def resample_axis(src, src_len, lines, line_step, stride, dst_len, start, span):
    """Area-average one axis, returning `lines` runs of `dst_len` samples.

    `src` holds `lines` runs of `src_len` samples: run `k` starts at
    `k * line_step` and its samples are `stride` apart. `start` and `span`
    give the window in source coordinates - fractional, and allowed to hang
    off either end, where the source reads as zero. That overhang is how the
    square window around a wide drawing gets its empty top and bottom without
    a second code path.

    The taps for a destination sample are the same for every run, so they are
    worked out once per `j` and reused down the whole plane.
    """
    out = [0.0] * (lines * dst_len)
    scale = span / dst_len
    for j in range(dst_len):
        lo = start + j * scale
        hi = lo + scale
        taps = []
        for i in range(math.floor(lo), math.ceil(hi)):
            if 0 <= i < src_len:
                # How much of source sample `i` falls inside [lo, hi).
                weight = min(hi, i + 1.0) - max(lo, float(i))
                if weight > 0.0:
                    taps.append((i * stride, weight))
        for k in range(lines):
            base = k * line_step
            out[k * dst_len + j] = sum(src[base + o] * w for o, w in taps) / scale
    return out


def convert(png, out):
    width, height, alpha = decode_png(png)
    x0, y0, x1, y1 = bounds(width, height, alpha)
    box_w, box_h = x1 - x0 + 1, y1 - y0 + 1

    # The square window: the LONG axis of the ink, with the short axis centred
    # inside it. This is the step that makes a thin microphone and a fat
    # guitar arrive on screen at the same size.
    side = float(max(box_w, box_h))
    win_x = x0 - (side - box_w) / 2.0
    win_y = y0 - (side - box_h) / 2.0

    content = SIZE - 2 * PAD
    plane = [float(v) for v in alpha]
    # x first, across full source rows: `height` runs of `width` samples, one
    # step apart, `width` apart from each other.
    rows = resample_axis(plane, width, height, width, 1, content, win_x, side)
    # then y down the already-narrow columns: `content` runs of `height`
    # samples, `content` apart, starting one after another.
    cols = resample_axis(rows, height, content, 1, content, content, win_y, side)

    block = bytearray(SIZE * SIZE)
    for y in range(content):
        row = (y + PAD) * SIZE + PAD
        for x in range(content):
            # `cols` came back one run per COLUMN, so the indices swap here.
            block[row + x] = min(255, int(cols[x * content + y] + 0.5))
    open(out, "wb").write(bytes(block))
    return box_w, box_h, int(side)


def main():
    src = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser("~/Dropbox/tanicons2")
    dst = sys.argv[2] if len(sys.argv) > 2 else "assets/icons"
    os.makedirs(dst, exist_ok=True)
    names = sorted(f for f in os.listdir(src) if f.lower().endswith(".png"))
    if not names:
        raise SystemExit(f"no PNGs in {src}")
    for name in names:
        # The owner names files the way he says them - "upright bass.png" -
        # and the code wants a slug it can spell in an identifier.
        stem = os.path.splitext(name)[0].strip().lower()
        slug = stem.replace(" ", "-").replace("_", "-")
        box_w, box_h, side = convert(os.path.join(src, name), os.path.join(dst, slug + ".a8"))
        print(f"{name:24s} -> {slug + '.a8':22s} ink {box_w}x{box_h} in a {side} square")
    print(f"{len(names)} icons, {SIZE}x{SIZE} each, {PAD}px clear on every side")


if __name__ == "__main__":
    main()
