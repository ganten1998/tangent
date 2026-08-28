#!/usr/bin/env python3
"""Design the auto-palettes against the same rules the Rust tests enforce,
then emit the Rust.

The governing rule: a strip colour is a whole painted surface with small
text printed straight onto it, so every palette colour must be dark enough
that the light ink wins AND clears 4.5:1 against it. That is a luminance
ceiling, which is what TONES encodes -- so the colours are generated to hit
a luminance rather than picked by eye and hoped over.
"""
import colorsys

INK_LIGHT = (0xE8, 0xDC, 0xC0)
INK_DARK = (0x1A, 0x1A, 0x1A)


def _ch(v):
    v = v / 255.0
    return v / 12.92 if v <= 0.03928 else ((v + 0.055) / 1.055) ** 2.4


def luminance(c):
    return 0.2126 * _ch(c[0]) + 0.7152 * _ch(c[1]) + 0.0722 * _ch(c[2])


def contrast(a, b):
    x, y = luminance(a), luminance(b)
    hi, lo = max(x, y), min(x, y)
    return (hi + 0.05) / (lo + 0.05)


def wants_light_ink(c):
    return contrast(c, INK_LIGHT) > contrast(c, INK_DARK)


def ink_contrast(c):
    return contrast(c, INK_LIGHT if wants_light_ink(c) else INK_DARK)


def _lab(c):
    r, g, b = (_ch(v) for v in c)
    x = r * 0.4124 + g * 0.3576 + b * 0.1805
    y = r * 0.2126 + g * 0.7152 + b * 0.0722
    z = r * 0.0193 + g * 0.1192 + b * 0.9505

    def f(t):
        return t ** (1 / 3) if t > 0.008856 else (7.787 * t + 16 / 116)

    fx, fy, fz = f(x / 0.95047), f(y / 1.0), f(z / 1.08883)
    return (116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz))


def de(a, b):
    return sum((x - y) ** 2 for x, y in zip(_lab(a), _lab(b))) ** 0.5


# Four tones, named by the luminance they land on. The top one sits under
# the 0.119 ceiling where 4.5:1 against the ivory ink runs out.
TONES = [0.016, 0.038, 0.068, 0.104]


def chroma(c):
    _, a, b = _lab(c)
    return (a * a + b * b) ** 0.5


def _at_luminance(hue, sat, target):
    lo, hi = 0.0, 1.0
    for _ in range(40):
        mid = (lo + hi) / 2
        r, g, b = colorsys.hsv_to_rgb(hue / 360.0, sat, mid)
        c = (round(r * 255), round(g * 255), round(b * 255))
        if luminance(c) < target:
            lo = mid
        else:
            hi = mid
    r, g, b = colorsys.hsv_to_rgb(hue / 360.0, sat, (lo + hi) / 2)
    return (round(r * 255), round(g * 255), round(b * 255))


# The chroma ceiling, and the reason this generator does two things at once.
#
# **Equal luminance is not equal vividness.** Blue contributes almost nothing
# to luminance, so a violet forced onto the same luminance as an ochre has to
# climb to #7736c1 to get there -- and next to muted earth tones it reads as
# a neon sign somebody left on. That is precisely the "incoherent" the owner
# saw. So the hue is held to a chroma ceiling as well: saturation comes down
# until the colour is no more colourful than the rest of its set, and the
# luminance is re-solved underneath it.
CHROMA_MAX = 42.0


def make(hue, sat, tone, cmax=CHROMA_MAX, lum=None):
    """A colour at `hue` sitting on TONES[tone], no more vivid than `cmax`.

    `lum` overrides the tone with an explicit luminance, which is what the
    gradients need: a ramp does not land on four named steps, it lands
    wherever it is along the ramp.
    """
    hue = hue % 360.0
    target = TONES[tone] if lum is None else lum
    c = _at_luminance(hue, sat, target)
    while chroma(c) > cmax and sat > 0.06:
        sat -= 0.02
        c = _at_luminance(hue, sat, target)
    return c


# The hue wheel used by the spectrum palettes.
#
# It starts at 16 rather than at red, because CRIMSON IS RESERVED: the master
# wears it in every palette, and a channel sitting on the same hue at the same
# tone is a channel that reads as the master out of the corner of the eye.
WHEEL = [16, 40, 72, 112, 168, 212, 268, 320]
MASTER_HUE = 352


def spectrum(hues, sat=0.72, cmax=CHROMA_MAX):
    """Two laps of the wheel, tone alternating, so neighbours differ in
    hue AND lightness and the two laps never repeat a pair."""
    out = []
    for i in range(8):
        out.append(make(hues[i], sat, 2 if i % 2 == 0 else 0, cmax))
    for i in range(8):
        out.append(make(hues[i], sat, 3 if i % 2 == 0 else 1, cmax))
    return out


def family(hues, sat=0.74, cmax=CHROMA_MAX):
    """A few hues, all four tones, walked so neither hue nor tone repeats
    back to back."""
    out = []
    n = len(hues)
    for i in range(16):
        out.append(make(hues[i % n], sat, (i // n + i) % 4, cmax))
    return out


def ash():
    """MOSTLY monochrome: sixteen tones with the tint alternating warm and
    cool, which is what keeps neighbouring strips apart when the hue is
    doing no work. Steps are small by design -- that is the theme."""
    out = []
    for i in range(16):
        t = i / 15.0
        lum = 0.006 + t * (0.108 - 0.006)
        # The tint carries what the lightness cannot. Sixteen steps have to
        # fit inside one ink-safe luminance range, so neighbours are only
        # ~3 dE apart on lightness alone; alternating the tint warm/cool
        # roughly doubles that while the set still reads as greys.
        hue = 32 if i % 2 == 0 else 214
        sat = 0.24 if i % 2 == 0 else 0.28
        lo, hi = 0.0, 1.0
        for _ in range(40):
            mid = (lo + hi) / 2
            r, g, b = colorsys.hsv_to_rgb(hue / 360.0, sat, mid)
            c = (round(r * 255), round(g * 255), round(b * 255))
            if luminance(c) < lum:
                lo = mid
            else:
                hi = mid
        r, g, b = colorsys.hsv_to_rgb(hue / 360.0, sat, (lo + hi) / 2)
        out.append((round(r * 255), round(g * 255), round(b * 255)))
    return out


# The master is deliberately ABOVE the chroma ceiling the channels obey: it
# is the one strip you go looking for, and a crimson pulled down to the
# channels' vividness stops being crimson and starts being another dark rose
# in a rack of them.
MASTER = make(MASTER_HUE, 0.92, 3, cmax=64.0)

PALETTES = {
    # name: (slots, click, master, adjacency floor)
    "IVORY": (spectrum(WHEEL), make(48, 0.30, 1), MASTER, 10.0),
    "EMBER": (family([20, 34, 50, 74]), make(44, 0.25, 1), MASTER, 10.0),
    "TIDE": (family([168, 196, 220, 258]), make(196, 0.22, 1), MASTER, 10.0),
    "MOSS": (family([78, 104, 136, 166]), make(84, 0.24, 1), MASTER, 10.0),
    # NEON is allowed to be louder than the rest -- that is the whole point
    # of it -- but it is still held to a ceiling, just a higher one.
    "NEON": (spectrum(WHEEL, sat=0.95, cmax=58.0), make(300, 0.35, 1), MASTER, 10.0),
    # The click is the one CHROMATIC thing on a monochrome desk -- a neutral
    # click would land on top of one of the sixteen greys by definition.
    "ASH": (ash(), make(38, 0.55, 1), MASTER, 3.0),
    "BLOOM": (family([268, 300, 326, 236]), make(300, 0.24, 1), MASTER, 10.0),
    "PATINA": (family([170, 26, 190, 44]), make(170, 0.24, 1), MASTER, 10.0),
}


# ── the gradients ──────────────────────────────────────────────────────────
#
# **A palette is now one sweep rather than eight pairs**, which is what the
# owner asked for and what every desk a musician has already used looks like.
# The rack reads top to bottom as a single ramp: luminance climbs steadily
# from track one to track sixteen and the hue walks a single arc.
#
# The arc has to be WIDE, and that is arithmetic rather than taste. Sixteen
# colours have to fit inside one ink-safe luminance band, which is about 35
# points of L* end to end, or 2.4 per step -- so lightness alone can never put
# neighbouring tracks the 10 dE apart that "tellable apart" means. The rest
# has to come from hue, and at this chroma ceiling a step buys 2*C*sin(dh/2),
# so ten points wants something near fourteen degrees a step. Fifteen steps of
# that is a two-hundred degree arc.
#
# So each palette starts on its own theme's hue and walks from there, and the
# WIDTH is found rather than chosen: the arc opens a little at a time until
# the tightest neighbouring pair clears the floor, and stops. Every palette is
# therefore as close to its own theme as the rule permits, instead of all
# sixteen being the same rainbow rotated.
# **Linear in LUMINANCE, not in lightness, and that is load-bearing.**
#
# L* is a cube root, so an even walk in luminance takes big L* strides down at
# the black end and small ones up at the top. That is precisely the shape this
# needs: chroma collapses towards black, so at the dark end hue can buy almost
# no dE and lightness has to carry the separation -- while at the light end
# chroma is at its ceiling and hue carries it easily. Ramping evenly in L*
# instead spreads the lightness budget uniformly and starves the dark end,
# which fails every palette. Measured both ways.
GRADIENT_LUM = (0.006, 0.108)
SPANS = {}


def gradient(h0, sat=0.72, lo=None, hi=None, cmax=CHROMA_MAX, floor=10.0,
             span=150.0, way=1):
    """Sixteen colours on one monotone ramp from `h0`, widened until every
    neighbouring pair clears `floor`."""
    lo = GRADIENT_LUM[0] if lo is None else lo
    hi = GRADIENT_LUM[1] if hi is None else hi
    # **Two dials, tried in that order.** Widening the arc is the cheap move
    # and keeps the palette's own colour; raising the saturation is the
    # expensive one, because it takes the whole ramp further from the theme's
    # own muted cast. So the arc opens first, and only a palette too pale for
    # any arc to separate -- the ones whose chroma had collapsed at the dark
    # end -- reaches for more colour.
    out = []
    start = span
    for _ in range(12):
        span = start
        for _ in range(90):
            out = []
            for i in range(16):
                t = i / 15.0
                out.append(
                    make(h0 + way * span * t, sat, None, cmax, lum=lo + (hi - lo) * t)
                )
            if all(de(a, b) >= floor for a, b in zip(out, out[1:])):
                SPANS[h0] = (span, sat)
                return out
            span += 6.0
        sat = min(sat + 0.04, 0.98)
    return out


def grey_gradient(h0=214, h1=32, sat=0.30, floor=2.2):
    """MONO's ramp: sixteen greys, the tint drifting cool to warm ACROSS the
    sweep rather than alternating every strip. The old one zig-zagged the
    tint to buy dE; a gradient cannot do that and stay a gradient, so this
    one leans on the tint travelling in one direction the whole way."""
    out = []
    for i in range(16):
        t = i / 15.0
        out.append(
            make(h0 + (h1 - h0) * t, sat, None, cmax=18.0,
                 lum=0.004 + t * (0.112 - 0.004))
        )
    return out


# Each palette's own hue, taken from the theme it dresses.
GRADIENTS = {
    "Default":    dict(h0=16,  sat=0.72),
    "Valentine":  dict(h0=336, sat=0.70),
    "Hydra":      dict(h0=168, sat=0.72),
    "Navi":       dict(h0=212, sat=0.70),
    "Mono":       None,  # grey_gradient
    "Studio":     dict(h0=40,  sat=0.66),
    "Aurora":     dict(h0=146, sat=0.74),
    "Sandstone":  dict(h0=28,  sat=0.62),
    "Centennial": dict(h0=196, sat=0.68),
    "Blueprint":  dict(h0=218, sat=0.72),
    "Porcelain":  dict(h0=200, sat=0.52),
    "Moss":       dict(h0=96,  sat=0.68),
    "Rhodes":     dict(h0=284, sat=0.66),
    "Copper":     dict(h0=22,  sat=0.74),
    "Dusk":       dict(h0=252, sat=0.68),
    "Iris":       dict(h0=272, sat=0.70),
}


def build_gradients():
    out = {}
    for name, kw in GRADIENTS.items():
        slots = grey_gradient() if kw is None else gradient(**kw)
        floor = 2.2 if kw is None else 10.0
        # The click is the one thing on the desk that is NOT on the ramp, so
        # it is taken well off it: a mid warm neutral for every palette but
        # Mono, whose greys need something chromatic to stand clear of.
        click = make(48, 0.30, 1) if kw is None or True else None
        out[name] = (slots, click, MASTER, floor)
    return out


def report_gradients():
    bad = 0
    for name, (slots, click, master, floor) in build_gradients().items():
        errs = []
        for i, c in enumerate(slots + [click, master]):
            r = ink_contrast(c)
            if r < 4.5:
                errs.append(f"slot {i} {hexs(c)} ink {r:.2f}:1")
            if not wants_light_ink(c):
                errs.append(f"slot {i} {hexs(c)} takes DARK ink")
        if len(set(slots)) != 16:
            errs.append("repeats a colour")
        ds = [de(a, b) for a, b in zip(slots, slots[1:])]
        for i, d in enumerate(ds):
            if d < floor:
                errs.append(f"slots {i}/{i+1} dE {d:.1f} < {floor}")
        # The ramp has to actually RAMP, or it is not a gradient.
        ls = [luminance(c) for c in slots]
        if any(b <= a for a, b in zip(ls, ls[1:])):
            errs.append("luminance does not climb the whole way")
        for label, c in (("click", click), ("master", master)):
            near = min(de(c, s) for s in slots)
            if near < 8.0:
                errs.append(f"{label} only dE {near:.1f} from a channel")
        if de(click, master) < 20.0:
            errs.append(f"click~master dE {de(click, master):.1f}")
        cs = [chroma(c) for c in slots]
        print(
            f"{name:11s} {'FAIL' if errs else 'ok':4s} "
            f"dE {min(ds):4.1f}-{max(ds):4.1f}  "
            f"chroma {min(cs):4.1f}-{max(cs):4.1f}  "
            f"lum {min(ls):.3f}-{max(ls):.3f}  "
            f"ink {min(ink_contrast(c) for c in slots):.1f}:1+  "
            f"arc {SPANS.get(GRADIENTS[name]['h0'], ('-', '-'))[0] if GRADIENTS[name] else '-'}"
        )
        for e in errs:
            print(f"            - {e}")
        bad += bool(errs)
    print()
    print("GRADIENTS FAILING" if bad else "ALL GRADIENTS PASS")
    return bad


def hexs(c):
    return f"#{c[0]:02x}{c[1]:02x}{c[2]:02x}"


def report():
    bad = 0
    for name, (slots, click, master, floor) in PALETTES.items():
        errs = []
        if len(slots) != 16:
            errs.append(f"{len(slots)} slots, want 16")
        if len(set(slots)) != len(slots):
            errs.append("repeats a colour")
        for i, c in enumerate(slots + [click, master]):
            r = ink_contrast(c)
            if r < 4.5:
                errs.append(f"slot {i} {hexs(c)} ink {r:.2f}:1")
            if not wants_light_ink(c):
                errs.append(f"slot {i} {hexs(c)} takes DARK ink")
        for i, (a, b) in enumerate(zip(slots, slots[1:])):
            d = de(a, b)
            if d < floor:
                errs.append(f"slots {i}/{i+1} dE {d:.1f} < {floor}")
        for label, c in (("click", click), ("master", master)):
            if c in slots:
                errs.append(f"{label} collides with a channel")
            near = min(de(c, s) for s in slots)
            if near < 8.0:
                errs.append(f"{label} only dE {near:.1f} from a channel")
        if click == master:
            errs.append("click and master identical")
        if de(click, master) < 20.0:
            errs.append(f"click~master dE {de(click, master):.1f}")
        cs = [chroma(c) for c in slots]
        ls = [luminance(c) for c in slots]
        print(
            f"{name:8s} {'FAIL' if errs else 'ok':4s} "
            f"chroma {min(cs):4.1f}-{max(cs):4.1f}  "
            f"lum {min(ls):.3f}-{max(ls):.3f}  "
            f"ink {min(ink_contrast(c) for c in slots):.1f}:1+"
        )
        for e in errs:
            print(f"         - {e}")
        if errs:
            bad += 1
    print()
    print("FAILING" if bad else "ALL PALETTES PASS")
    return bad


def emit():
    print("\n// ---- generated by palgen.py ----")
    for name, (slots, click, master, _) in PALETTES.items():
        print(f"// {name}")
        for c in slots:
            print(f"    ({c[0]:#04x}, {c[1]:#04x}, {c[2]:#04x}),", end="")
        print()
        print(f"//   click {hexs(click)}  master {hexs(master)}")


def emit_gradients():
    print("\n// ---- gradients generated by palgen.py ----")
    for name, (slots, click, master, _) in build_gradients().items():
        print(f"    // {name}")
        print("    ", end="")
        for c in slots:
            print(f"({c[0]:#04x}, {c[1]:#04x}, {c[2]:#04x}), ", end="")
        print()
        print(f"    ({click[0]:#04x}, {click[1]:#04x}, {click[2]:#04x}), // {name} click")


if __name__ == "__main__":
    import sys
    if "--gradients" in sys.argv:
        rc = report_gradients()
        if not rc:
            emit_gradients()
        raise SystemExit(1 if rc else 0)
    rc = report()
    if not rc:
        emit()
    raise SystemExit(1 if rc else 0)
