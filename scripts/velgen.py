#!/usr/bin/env python3
"""Design the sixteen VELOCITY gradients — the pair of colours a theme lends
the velocity lane — against the same rules the Rust tests enforce, then emit
the Rust.

# What this solves that `palgen.py` does not

A strip colour is a painted SURFACE with ink printed on it, so `palgen` solves
a luminance CEILING. A velocity colour is a MARK drawn on the roll: a shaft
three points wide and a ring around its head. Nothing is printed on it, so the
ceiling does not apply — what applies is that it must be *seen*, against a roll
whose colour every theme derives differently and which moves again in dark
mode.

So the constraint here is a contrast FLOOR against two grounds at once, and a
separation floor between the two ends of the ramp. A gradient whose quiet end
cannot be found on the roll is a gradient that has thrown away the bottom half
of its range; a gradient whose ends measure the same is not one.

# Why the ends are hot and cool, and fixed that way

The owner's instruction is red for the loudest and green for the quietest on
the Default theme, and a per-theme pair after that. The HUE FAMILY is kept
across all sixteen — hot for loud, cool for quiet — for the reason the record
light and the playhead are the same red in every theme: a meaning that changes
with the wallpaper is a meaning learned twice. What each theme chooses is
WHICH hot and WHICH cool, so the pair sits in its own palette instead of being
stamped on it.

# What this file does NOT have to guarantee

Legibility is not solved here, and deliberately. `midi_editor::velocity_ink`
pushes whatever colour it is handed away from the roll until it clears the
floor, the same way `arrangement_panel::redder_of` does for the playhead — so a
USER theme naming two bad colours still gets a readable lane. This script
designs the sixteen that ship, and the Rust test proves they need no push on
their own themes, which is what makes them *chosen* rather than *corrected*.

Usage:
    python3 scripts/velgen.py report   # measure, and say where it is tight
    python3 scripts/velgen.py emit     # the Rust, ready to paste
"""
import colorsys
import json
import sys

INK_LIGHT = (0xE8, 0xDC, 0xC0)
INK_DARK = (0x1A, 0x1A, 0x1A)

# The floor a mark on the roll must clear. Three, not four and a half: this is
# a graphic and not a letter, and WCAG's own non-text threshold is 3:1.
FLOOR = 3.0
# How far apart the two ends must measure to read as a ramp rather than as one
# colour. Twenty-five is "a person can tell these apart"; the ends of a
# gradient should be obvious, not merely distinguishable.
SEPARATION = 45.0
# The chroma ceiling, from palgen's own: vivid enough to be a colour, never so
# vivid it fights the palette it is sitting in.
CHROMA_MAX = 72.0
# And a floor, so a colour that can only be seen by going grey is refused
# outright instead of shipping as a neutral pretending to be a hue.
CHROMA_MIN = 26.0


def _ch(v):
    v = v / 255.0
    return v / 12.92 if v <= 0.03928 else ((v + 0.055) / 1.055) ** 2.4


def luminance(c):
    return 0.2126 * _ch(c[0]) + 0.7152 * _ch(c[1]) + 0.0722 * _ch(c[2])


def contrast(a, b):
    x, y = luminance(a), luminance(b)
    hi, lo = max(x, y), min(x, y)
    return (hi + 0.05) / (lo + 0.05)


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


def chroma(c):
    _, a, b = _lab(c)
    return (a * a + b * b) ** 0.5


# ── the roll, exactly as `midi_editor::draw` derives it ────────────────────
#
# Copied rather than approximated: a design tool that solves against a
# DIFFERENT ground than the one the app paints is a tool that lies. If the
# derivation in `midi_editor.rs` changes, this changes with it, and the Rust
# test is what catches the drift.


def blend(a, b, t):
    return tuple(int(x + (y - x) * t) for x, y in zip(a, b))


def shade_up(c, t):
    return tuple(int(min(255, max(0, v + (255 - v) * t))) for v in c)


def wants_light_ink(c):
    return contrast(c, INK_LIGHT) > contrast(c, INK_DARK)


def legible_surface(c):
    """`recorder_panel::legible_surface`: the dead-zone guard."""
    if max(contrast(c, INK_LIGHT), contrast(c, INK_DARK)) >= 4.5:
        return c
    if contrast(c, INK_LIGHT) > contrast(c, INK_DARK):
        ink, toward = INK_LIGHT, (0, 0, 0)
    else:
        ink, toward = INK_DARK, (255, 255, 255)
    out = c
    for step in range(1, 51):
        out = blend(c, toward, step * 0.02)
        if contrast(out, ink) >= 4.6:
            break
    return out


def roll_face(meter_bg, fx_strip, dark):
    """The colour the piano roll is actually painted, in one mode."""
    if dark:
        return legible_surface(shade_up(meter_bg, 0.22))
    return legible_surface(blend(meter_bg, fx_strip, 0.34))


def _at_luminance(hue, sat, target):
    """The RGB on this hue and saturation that lands on `target` luminance."""
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


def solve(hue, faces, sat=0.86, ceiling=None):
    """The colour on `hue` that clears FLOOR against every face in `faces`.

    **Maximise CHROMA, not contrast, and that is the whole design.** The first
    cut of this took the candidate with the most headroom, which sounds like
    prudence and is not: against Sandstone's pale roll the most contrast is
    always the darkest colour, so both ends of the ramp ran to near-black and
    the "red to green" gradient came out as two blacks five `delta_e` apart.
    A gradient whose ends are the same colour is not one.

    So the floor is a GATE and colour is the objective: of everything that can
    be seen on this roll, take the most saturated. Against a dark roll that
    lands bright, against a pale one it lands deep, and either way it is still
    recognisably the hue the theme asked for.
    """
    best = None
    for target in [i / 600.0 for i in range(1, 600)]:
        c = _at_luminance(hue, sat, target)
        ch = chroma(c)
        if ch > CHROMA_MAX or ch < CHROMA_MIN:
            continue
        worst = min(contrast(c, f) for f in faces)
        if worst < FLOOR:
            continue
        # **Loud must never be out-shouted by quiet.** The quiet end is solved
        # with the loud end's own contrast as a CEILING, because without it the
        # cool hues — which stay under the chroma cap all the way to full
        # brightness, so the solver runs them there — came out at 6.7:1 against
        # a roll where the reds sat at 3.2:1. Every pianissimo note would have
        # been the most prominent thing in the lane. Prominence has to rise
        # with velocity or the gradient is telling the opposite of the truth.
        if ceiling is not None and worst > ceiling:
            continue
        # Chroma first; among equally colourful candidates prefer the one
        # nearer a comfortable margin rather than the one scraping the floor.
        score = (round(ch, 1), -abs(worst - 4.5))
        if best is None or score > best[0]:
            best = (score, c, worst)
    if best is None:
        # Nothing on this hue can be both seen and saturated at this setting.
        # Desaturate a step and try again rather than returning a lie.
        if sat > 0.40:
            return solve(hue, faces, sat - 0.10, ceiling)
        return None, 0.0
    return best[1], best[2]


# ── the sixteen, by identity ───────────────────────────────────────────────
#
# A hot hue and a cool one per theme, named for what the theme already is.
# Default is the owner's own instruction — red loudest, green quietest — and
# every other pair is that same idea wearing its theme's colours.
HUES = {
    "Default":    (2, 128),    # the instruction: plain red, plain green
    "Valentine":  (352, 162),  # rose-red against its own jade
    "Hydra":      (18, 158),   # its coral against its seafoam
    "Navi":       (8, 168),    # warm red on navy, cyan-green back
    "Mono":       (0, 130),    # neutral theme keeps the canonical pair
    "Studio":     (14, 96),    # console amber-red, olive
    "Aurora":     (338, 148),  # magenta-red, aurora green
    "Sandstone":  (12, 104),   # brick and moss, for a warm paper roll
    "Centennial": (355, 134),  # a flat red and a flat green, like the theme
    "Blueprint":  (20, 176),   # orange-red is the only warm on a blue print
    "Porcelain":  (348, 146),  # crimson and sage
    "Moss":       (24, 168),   # rust, and a cool green that is NOT the roll
    "Rhodes":     (6, 156),    # red over warm brown, jade back
    "Copper":     (16, 164),   # copper-red against its teal
    "Dusk":       (344, 170),  # rose and teal, both cooled for violet
    "Iris":       (330, 140),  # magenta-red, spring green
}


def themes():
    return json.load(open("/private/tmp/claude-501/-Users-ganten/"
                          "a525df4f-d8b8-4237-9509-53687aa6cac6/"
                          "scratchpad/themes.json"))


def default_theme():
    """Default is written in `factory_default()` and not in `factory()`."""
    return {"meter_bg": [0x0B, 0x0E, 0x0A], "fx_strip": [0xE3, 0xD9, 0xBD]}


def build():
    data = themes()
    data["Default"] = default_theme()
    out = []
    for name, (hot, cool) in HUES.items():
        t = data[name]
        faces = [
            roll_face(tuple(t["meter_bg"]), tuple(t["fx_strip"]), False),
            roll_face(tuple(t["meter_bg"]), tuple(t["fx_strip"]), True),
        ]
        loud, lm = solve(hot, faces)
        # The loud end first, then the quiet one UNDER it — see `solve`.
        quiet, qm = solve(cool, faces, ceiling=lm)
        out.append((name, loud, quiet, lm, qm, faces))
    return out


def hexs(c):
    return "0x%02x, 0x%02x, 0x%02x" % c


def report():
    print(f"{'theme':12s} {'loud':>9s} {'quiet':>9s} {'lo-c':>5s} {'qu-c':>5s} "
          f"{'dE':>5s}  faces(light/dark)")
    bad = 0
    for name, loud, quiet, lm, qm, faces in build():
        sep = de(loud, quiet)
        flag = ""
        if loud is None or quiet is None:
            flag = "  <-- UNSOLVED"
            bad += 1
        elif sep < SEPARATION:
            flag = f"  <-- ends only {sep:.0f} apart"
            bad += 1
        elif min(lm, qm) < FLOOR:
            flag = "  <-- below floor"
            bad += 1
        print(f"{name:12s} #{'%02x%02x%02x' % loud:>8s} #{'%02x%02x%02x' % quiet:>8s} "
              f"{lm:5.2f} {qm:5.2f} {sep:5.0f}  "
              f"#{'%02x%02x%02x' % faces[0]}/#{'%02x%02x%02x' % faces[1]}{flag}")
    print()
    print("floor", FLOOR, " separation", SEPARATION, " chroma max", CHROMA_MAX)
    print("FAIL" if bad else "all sixteen clear both grounds on their own theme")


def emit():
    for name, loud, quiet, *_ in build():
        print(f"            // {name}")
        print(f"            velocity_loud: c({hexs(loud)}),")
        print(f"            velocity_quiet: c({hexs(quiet)}),")


if __name__ == "__main__":
    {"report": report, "emit": emit}.get(
        sys.argv[1] if len(sys.argv) > 1 else "report", report
    )()
