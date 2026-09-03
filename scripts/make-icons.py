#!/usr/bin/env python3
"""Convert the track-icon PNGs to raw 8-bit alpha the app can embed.

The set is openly licensed artwork the owner sourced and has full rights to
use; it was not drawn in-house, and nothing that ships should say it was.

tangent-ui has exactly one dependency (egui) on purpose, so there is no PNG
decoder in it and adding one to draw an icon would be a poor trade. These are
converted once, here, into a flat `SIZE*SIZE` block of coverage bytes that
`include_bytes!` can take as-is.
"""
import struct, zlib, sys, os, glob

SIZE = 192

def read_png(path):
    d = open(path, 'rb').read()
    assert d[:8] == b'\x89PNG\r\n\x1a\n', path
    i, idat, w, h, depth, ctype = 8, b'', 0, 0, 0, 0
    plte, trns = None, None
    while i < len(d):
        ln = struct.unpack('>I', d[i:i+4])[0]
        tag = d[i+4:i+8]
        body = d[i+8:i+8+ln]
        if tag == b'IHDR':
            w, h, depth, ctype = struct.unpack('>IIBB', body[:10])
        elif tag == b'IDAT':
            idat += body
        elif tag == b'PLTE':
            plte = body
        elif tag == b'tRNS':
            trns = body
        i += 12 + ln
    assert depth == 8, f'{path}: {depth}-bit not handled'
    chans = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[ctype]
    raw = zlib.decompress(idat)
    stride = w * chans
    out = bytearray(h * stride)
    prev = bytearray(stride)
    p = 0
    for y in range(h):
        f = raw[p]; p += 1
        line = bytearray(raw[p:p+stride]); p += stride
        for x in range(stride):
            a = line[x-chans] if x >= chans else 0
            b = prev[x]
            c = prev[x-chans] if x >= chans else 0
            if f == 1: line[x] = (line[x] + a) & 255
            elif f == 2: line[x] = (line[x] + b) & 255
            elif f == 3: line[x] = (line[x] + (a + b) // 2) & 255
            elif f == 4:
                pp = a + b - c
                pa, pb, pc = abs(pp-a), abs(pp-b), abs(pp-c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[x] = (line[x] + pr) & 255
        out[y*stride:(y+1)*stride] = line
        prev = line
    # coverage: the alpha channel where there is one, else luminance
    cov = bytearray(w*h)
    for j in range(w*h):
        px = out[j*chans:(j+1)*chans]
        if ctype == 6: cov[j] = px[3]
        elif ctype == 4: cov[j] = px[1]
        elif ctype == 3:
            cov[j] = trns[px[0]] if trns and px[0] < len(trns) else 255
        elif ctype == 0: cov[j] = px[0]
        else: cov[j] = max(px)
    return w, h, cov

def box_to(w, h, cov, n):
    """Area-average down to n x n. These shrink ~6x, so a box filter is the
    honest one: point sampling would drop whole strokes."""
    out = bytearray(n*n)
    for oy in range(n):
        y0, y1 = oy*h//n, max(oy*h//n + 1, (oy+1)*h//n)
        for ox in range(n):
            x0, x1 = ox*w//n, max(ox*w//n + 1, (ox+1)*w//n)
            s = cnt = 0
            for y in range(y0, y1):
                base = y*w
                for x in range(x0, x1):
                    s += cov[base+x]; cnt += 1
            out[oy*n+ox] = (s + cnt//2)//cnt if cnt else 0
    return out

src, dst = sys.argv[1], sys.argv[2]
os.makedirs(dst, exist_ok=True)
names = []
for path in sorted(glob.glob(os.path.join(src, '*.png'))):
    stem = os.path.splitext(os.path.basename(path))[0].replace(' ', '-').lower()
    w, h, cov = read_png(path)
    # trim to the drawn bounds, then re-pad square, so every icon fills its
    # cell the same amount however much white space the artwork carried.
    xs = [x for x in range(w) for y in range(h) if False]
    minx, miny, maxx, maxy = w, h, -1, -1
    for y in range(h):
        row = cov[y*w:(y+1)*w]
        for x in range(w):
            if row[x] > 8:
                if x < minx: minx = x
                if x > maxx: maxx = x
                if y < miny: miny = y
                if y > maxy: maxy = y
    if maxx < 0:
        print(f'  !! {stem}: nothing drawn'); continue
    side = max(maxx-minx+1, maxy-miny+1)
    cx, cy = (minx+maxx)//2, (miny+maxy)//2
    half = side//2 + 1
    sq = bytearray(side*side)
    for y in range(side):
        sy = cy - half + y + 1
        for x in range(side):
            sx = cx - half + x + 1
            if 0 <= sx < w and 0 <= sy < h:
                sq[y*side+x] = cov[sy*w+sx]
    small = box_to(side, side, sq, SIZE)
    open(os.path.join(dst, stem + '.a8'), 'wb').write(bytes(small))
    ink = sum(1 for v in small if v > 16) * 100 // (SIZE*SIZE)
    names.append((stem, w, h, ink))
print(f'{len(names)} icons -> {dst} at {SIZE}x{SIZE}')
for n, w, h, ink in names:
    print(f'  {n:<18} {w}x{h}  ink {ink}%')
