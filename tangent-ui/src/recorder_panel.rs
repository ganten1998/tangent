//! The Recorder band: a whole take's worth of controls in 200 points of window.
//!
//! Two pictures of the same machine, switched by [`RecordState`], because no
//! other recorder knows that its user is a pianist:
//!
//!   * **idle**, which is about FRAMING and destination — a wide camera
//!     preview, a live meter, the monitor levels, and every control that
//!     decides what the next take will be, and
//!   * **rolling**, which is about being readable from a piano bench two metres
//!     away — the preview collapses, the timecode and the meter become huge,
//!     and everything that could change the destination mid-take goes away.
//!
//! While playing, the pianist is looking at their hands. The preview's job is
//! finished before the take starts, which is the whole reason those are two
//! layouts and not one that dims.
//!
//! # Four groups, left to right
//!
//! The band was once a single row of controls at one size, and the owner's
//! verdict on it was that nothing said what it was. So it is now grouped, and
//! the groups are ordered by how often a hand goes near them:
//!
//!   1. **preview** — framing. Looked at before the first take of a session and
//!      then never again, so it is the one thing that shrinks when the take
//!      starts.
//!   2. **transport** — record, stop, meter, timecode. The biggest things in
//!      the band, because they are the ones read from the bench.
//!   3. **destination** — folder, name, devices, count-in, tempo, export. Set
//!      once and left alone, so it is the smallest type in the band and it
//!      disappears entirely while rolling.
//!   4. **instruments and monitor** — three instrument slots, each with its own
//!      small level knob and its own button that opens that plugin's window,
//!      then the click and input faders and the click switch. Touched every
//!      take, and the only group besides the transport that survives a live
//!      take: turning the click down halfway through is exactly when you need
//!      to, and so is reaching a preset in a plugin's own window.
//!
//! The monitor is last, at the right edge, and it is **pinned there** — same
//! rectangle in both layouts, down to the point. Everything else in the band
//! moves when a take starts: the preview collapses, the timecode triples, the
//! whole destination column vanishes. A fader that survives a take and then
//! slides a third of the window sideways at the moment it becomes the only
//! thing you might still want to touch is only half a survivor, so the monitor
//! is measured from the right edge of the band rather than from anything that
//! changes. See `MONITOR_W`.
//!
//! # Where the three slots' space came from
//!
//! Three instrument rows, each carrying a name, a level and two buttons, is a
//! lot to add to a band 200 points tall. Nothing was compressed to make room;
//! three things that deserved their space less gave it up:
//!
//!   * **the monitor's INSTRUMENT fader** — deleted outright. It set the level
//!     of "the instrument" back when there was one, and the per-slot knobs say
//!     the same thing three times more precisely. That is one of the four
//!     monitor rows back.
//!   * **the destination's INSTRUMENT row** — deleted too. It was a picker, and
//!     each slot's own name box is now that picker. The destination column went
//!     from eight rows to seven, so the seven that remain are each TALLER than
//!     the eight were.
//!   * **width, from the destination and the preview**. [`MONITOR_W`] went from
//!     0.28 of the body to 0.36; the preview's width cap went from 0.26 to 0.22,
//!     which only bites in the detached window (in the band the preview is
//!     bounded by the band's height long before it reaches either cap). The
//!     destination is the group `docs/RECORDER-PLAN.md` §5 describes as set once
//!     and left alone, and the preview is looked at twice a session; the slots
//!     are touched every take.
//!
//! There is no in-band control for hiding the elapsed clock. There was, it was
//! a box marked `CLOCK` with a line through it, and no one could tell what it
//! did. The setting lives in the right-click menu, where the row reads "Hide
//! Elapsed Time" and says so in words. [`RecorderView::hide_elapsed`] still
//! suppresses the readout; only the mystery box is gone.
//!
//! Like `piano.rs`, `fretboard_panel.rs` and `theory_panel.rs`, this module is
//! dumb: it is handed a [`RecorderView`] snapshot and it paints it, and a click
//! comes back out as a [`Hit`] for somebody else to act on. It opens no device,
//! reads no disk and knows no time — see `recorder.rs` for why that firewall is
//! load-bearing rather than tidy.
//!
//! **`draw` is a pure painter and `hit_test` is a pure function of geometry.**
//! Both go through one private [`Layout`], which is the single most important
//! thing in this file: a hit test that computes its own rectangles is a hit test
//! that stops matching the picture the first time the picture moves, invisibly,
//! and the compositor in `docs/RECORDER-PLAN.md` §6 needs to render this same
//! band into an offscreen 1920x1080 surface with no `Context` anywhere near it.
//!
//! See `docs/RECORDER-PLAN.md` §5.

use crate::fonts;
use crate::recorder::{
    DEFAULT_BPM,
    gain_text, gain_to_fader, timecode, DeviceLabel, ExportSpec, Level, Meters,
    NumField, Preview, RecordState, RecorderView, COUNT_IN_CHOICES, MAX_BPM, MIN_BPM,
};
use crate::ports::KnobUnit;
use crate::settings::Settings;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};

/// Band height for a 1300pt-wide window. At that width it gives a ~230x150
/// preview and still leaves ~1000pt for controls, which is the one dimension
/// this window has in abundance. Scaled with everything else (spec §3.2).
/// **165, down from 200.** The take's settings left the band for the menu and
/// took seven rows with them; what is left is a transport, two faders, two
/// meters and six slot rows, and none of those wanted the height the old
/// destination column did. The band was reaching a third of the way down the
/// window for controls that no longer live in it.
pub const BAND_H_AT_1300: f64 = 190.0;

/// Height of the Recorder band for a window `w` points wide.
///
/// **A pure function of width and of nothing else**, truncated like every other
/// band in the layout. Nothing about the camera may reach it: a 4:3 webcam that
/// made the window 90 points taller than a 16:9 one would resize the piano
/// because somebody unplugged a device, which is `docs/RECORDER-PLAN.md` §0's
/// named failure. The camera's aspect is dealt with entirely inside
/// [`fit_preview`], where it belongs.
/// Where the camera preview ends — the separator the arrange header's
/// default width lines up with, by the owner's word.
/// **Where the timecode cluster ends and the oscilloscope begins**, as a
/// fraction of the LCD box.
///
/// It was a flat 0.475 — an even split — and that is right only while the box
/// keeps its shape. Maximised it does not: the band's height is damped (see
/// [`band_height`]) while its width still follows the window, so the box goes
/// from about four times as wide as it is tall to more than six. An even
/// split spent all of that on digits that do not need it.
///
/// The cluster is a STACK OF NUMBERS, so what it needs is set by its height.
/// Holding its aspect constant and giving the surplus to the scope is the
/// owner's "that extra horizontal room? let the oscillator grow
/// horizontally". The scope goes from about half the box to two thirds of it
/// on a wide screen, and nothing moves at the sizes anybody works in.
///
/// One function because FOUR places need this number — the cluster's paint,
/// the rule between the halves, the scope's own rect, and two hit tests — and
/// a hit test that disagrees with the paint is a control you can see and
/// cannot press.
fn lcd_split(r: Rect) -> f32 {
    // 1.909 is 0.475 times the box's width-to-height ratio at the reference,
    // so the split is exactly what it always was at those sizes.
    if r.width() <= 0.0 {
        return 0.475;
    }
    (1.909 * r.height() / r.width()).clamp(0.28, 0.475)
}

/// **The LCD's scope glass**: its right-hand side, where the oscilloscope is
/// drawn - and, while a take runs, the take's own readout. One geometry for
/// both, so the readout covers exactly the glass and nothing the LCD's drag
/// cells own. `NOTHING`, never an inverted sliver, when there is no room.
fn lcd_scope_rect(lcd: Rect) -> Rect {
    if !lcd.is_positive() {
        return Rect::NOTHING;
    }
    let side = slice_h(lcd, lcd_split(lcd) + 0.025, 0.99);
    let r = slice_v(side, 0.07, 0.93).shrink2(Vec2::new(2.0, 0.0));
    if r.is_positive() {
        r
    } else {
        Rect::NOTHING
    }
}

pub fn camera_split_x(rect: Rect, view: &RecorderView<'_>) -> f32 {
    Layout::new(rect, view).preview.right()
}

/// Above this width the band stops growing in step with the window. See
/// [`band_height`].
const BAND_DAMP_FROM: f64 = 1600.0;

/// The most of its linear height the band may give back. See [`band_height`].
///
/// **0.63, down from 0.70**, which is the same ten per cent the exponent
/// takes: without it the floor would have caught the wide screens a few
/// hundred points past the owner's, and they would have kept the old height
/// while everything narrower got shorter.
const BAND_DAMP_FLOOR: f64 = 0.63;

/// How the band grows with width once past [`BAND_DAMP_FROM`].
///
/// **0.2758, down from 0.5 (a square root).** Solved, not picked: the owner
/// asked for "around another 10 percent height reduction" at the size they
/// actually work at, and this display is 2560 points wide, so the exponent is
/// whatever makes 2560 come out a tenth shorter than the root did —
///
/// ```text
/// (2560/1600)^p = 0.9 * sqrt(2560/1600)   =>   p = ln(0.9*sqrt(1.6))/ln(1.6)
/// ```
///
/// A flat 0.9 multiplier would have been the obvious way to write "ten per
/// cent shorter" and it is the wrong one: it applies at 1601 points as
/// hard as at 2560, so the band would SNAP 23 points shorter the moment a
/// dragged window crossed the threshold. The exponent is continuous there by
/// construction — at 1600 it is still 1.0 — and the reduction arrives
/// gradually as the window widens toward maximised, which is the only place
/// the owner asked for it.
const BAND_DAMP_EXP: f64 = 0.2758;

pub fn band_height(w: f32) -> f32 {
    // **The band stops growing in step with the window once the window is
    // wide.**
    //
    // Every band's height is a fraction of the WIDTH, which is right while a
    // window is a window: the whole layout scales together and the app looks
    // the same at any size. Maximised it stops being right. The band holds a
    // fixed set of controls — eight knobs, a fader, a meter, a timeline —
    // and none of them needs to be half again as tall because the screen got
    // wider. The owner: at 100 percent "the app feels perfect visually", and
    // full-screened the recorder "NEEDS to be around 15-30 percent shorter".
    //
    // So the law is linear up to `BAND_DAMP_FROM` — every window anybody
    // works in, unchanged, which is the part that must not move — and grows
    // as a FRACTIONAL POWER of the width above it. A power because it is the
    // honest middle: linear is what is wrong, constant would make the band
    // shrink to a sliver on a wide screen, and a root gives back a useful
    // slice while still growing.
    //
    // **Steeper since the owner asked for "around another 10 percent" at
    // maximised** — see [`BAND_DAMP_EXP`] for why that is one exponent and
    // not a multiplier. What the two versions come to, against the linear
    // height the band would have had:
    //
    //   width   linear   was    now    now vs was
    //   1600     233     233    233        0%     (nothing that fits a window moves)
    //   1920     280     256    245       -4%
    //   2560     374     295    266      -10%     (this display, maximised)
    //   3440     502     351    316      -10%     (floored, both)
    //
    // The floor is what stops a 5K or an ultrawide from taking the whole band
    // away: past a point the controls do need their room, and "shorter" was
    // asked for as a range and not as a direction to keep going in.
    let w = f64::from(w);
    let linear = BAND_H_AT_1300 * w / 1300.0;
    let h = if w > BAND_DAMP_FROM {
        let at_ref = BAND_H_AT_1300 * BAND_DAMP_FROM / 1300.0;
        (at_ref * (w / BAND_DAMP_FROM).powf(BAND_DAMP_EXP)).max(linear * BAND_DAMP_FLOOR)
    } else {
        linear
    };
    h.trunc() as f32
}

// ── palette ────────────────────────────────────────────────────────────────

/// The record red, and the only strong hue this band uses.
///
/// The chord strip's red, so the app has one red rather than two that nearly
/// match. It reads against both the cream and the near-black background, which
/// a darker red does not.
pub const REC_RED: Color32 = Color32::from_rgb(0xE8, 0x3A, 0x4E);

pub(crate) struct Palette {
    /// Also the letterbox bars. See [`fit_preview`].
    pub(crate) bg: Color32,
    /// Recessed fills: the preview well, the meter troughs and the fader
    /// tracks. A fader has to look like a slot with something in it, which is
    /// the same picture as a meter and gets the same colour.
    pub(crate) well: Color32,
    /// Control fills, so a thing you can click looks unlike a thing you read.
    pub(crate) field: Color32,
    pub(crate) ink: Color32,
    pub(crate) faint: Color32,
    pub(crate) line: Color32,
    /// The meter and the fader fills. Whatever a held key looks like on the
    /// piano — the user chose that colour once and this is the same "sound is
    /// happening" signal.
    pub(crate) accent: Color32,
    pub(crate) rec: Color32,
    /// The cluster's glass: the timecode digits and the scope trace, and the
    /// face they sit on. Theme roles — see `crate::theme`.
    pub(crate) lcd: Color32,
    pub(crate) lcd_face: Color32,
    /// The eight effect dials, and the master, which is its own colour.
    pub(crate) fx_knob: Color32,
    pub(crate) master: Color32,
    /// The varispeed dial's face. Its own role — see `Settings`.
    pub(crate) varispeed: Color32,
}

/// The two inks the band can wear.
pub const INK_LIGHT: Color32 = Color32::from_rgb(0xE8, 0xDC, 0xC0);
pub const INK_DARK: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);

/// One channel of [`luminance`]: sRGB to linear, the arithmetic WCAG writes.
///
/// Not called on the drawing path — [`srgb_to_linear_table`] holds its 256
/// answers — but it is where they come FROM, and the only place the curve
/// is written down.
fn srgb_to_linear(v: u8) -> f32 {
    let v = f32::from(v) / 255.0;
    if v <= 0.03928 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// The sRGB transfer curve for every channel value there is, built once.
///
/// **A channel is a `u8`, so the curve has 256 answers and no more.** They
/// were being recomputed instead: three `powf(2.4)` per colour, per
/// [`contrast_ratio`], and this app asks that question in a loop —
/// `legible_surface` walks up to fifty steps and measures at each one,
/// `quiet` twelve, `distinct_surface_at` sixteen, and the arrangement runs
/// them per clip and per header on every frame. The survey put it at three
/// to five thousand `powf` a frame, and up to ten microseconds in a single
/// `legible_surface` call when the lane colour lands in the dead zone.
///
/// The table is filled by [`srgb_to_linear`] itself rather than by a list of
/// numbers typed out here, and that is the whole safety argument: the
/// entries are not "close to" the arithmetic they replace, they ARE it,
/// evaluated once instead of a thousand times a frame. This has to be exact
/// and not merely near, because the 4.5:1 decisions built on it are
/// load-bearing — there is a band of luminance around 0.12 to 0.22 where
/// neither ink clears 4.5 and the SURFACE has to move
/// ([`legible_surface`]), and a colour sitting on that boundary would flip
/// sides if the curve moved in its last bit.
/// `the_luminance_table_is_the_arithmetic_it_replaced` proves all 256.
fn srgb_to_linear_table() -> &'static [f32; 256] {
    static LIN: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    // `v` comes from `0..256`, so the cast is the index's own range.
    LIN.get_or_init(|| std::array::from_fn(|v| srgb_to_linear(v as u8)))
}

/// Relative luminance, sRGB, as WCAG defines it.
///
/// The three lookups are total: a `u8` widened to `usize` cannot leave a
/// 256-long array, so there is no bounds check to fail however strange the
/// colour a document hands in. See [`srgb_to_linear_table`].
pub fn luminance(c: Color32) -> f32 {
    let lin = srgb_to_linear_table();
    0.2126 * lin[usize::from(c.r())]
        + 0.7152 * lin[usize::from(c.g())]
        + 0.0722 * lin[usize::from(c.b())]
}

/// Contrast ratio between two colours, 1.0 (identical) to 21.0 (black/white).
pub fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

/// Whether ink on this background has to be light.
///
/// **The band's ink follows its own background, not the theme.** The colour is
/// the user's to choose, so a rule that read `dark_mode` would put near-black
/// text on a dark walnut the moment somebody picked one in light mode — and the
/// band would be unreadable with nothing on screen explaining why.
///
/// It MEASURES both candidates rather than testing a brightness threshold, and
/// the difference is not academic: a mid grey sits below any sensible threshold
/// and yet takes dark ink far better than light — 5.3 against 2.7. A threshold
/// picked the ivory and produced the one genuinely unreadable band this whole
/// mechanism exists to prevent.
///
/// The worst case is a background whose luminance falls where the two curves
/// cross, at about 0.17, where the better of the two still gives 3.57 — above
/// WCAG's 3.0 for large text and below its 4.5 for body text, which is honest:
/// somebody who picks that exact colour gets a legible band and not a
/// comfortable one.
/// **Shared with the mixer**, which faces the same question about its strip
/// faces: what is printed on a surface has to answer to how light it is, and
/// there must be one answer rather than two that can drift apart.
/// Everything a floating PANEL is made of, derived from one colour.
///
/// **The same frozen pair kept turning up**: black-on-ivory in light mode and
/// its inverse in dark, hard-coded separately in the right-click menus, the
/// help card, the chord hover card, the dialogs, the theory band, the staff
/// and the arrangement's chips. Seven copies of one decision, none of which
/// any theme could reach, each found by a screenshot.
///
/// So it is one decision now. Hand it the desk's face and it answers with a
/// panel made of that face: a ground that can carry its own ink, the ink, a
/// quieter ink for secondary rows, a raised shade for buttons and the
/// highlighted row, a hover a step beyond that, and a well for typed fields.
#[derive(Clone, Copy)]
pub struct Surface {
    pub bg: Color32,
    pub ink: Color32,
    /// Secondary text — a ratio of ink to ground, so it holds on either.
    pub faint: Color32,
    /// Buttons, and the row under the pointer.
    pub raised: Color32,
    pub hover: Color32,
    /// The ground under a typed value: away from the panel in the direction
    /// that reads as recessed on this ground.
    pub well: Color32,
    pub edge: Color32,
}

/// Build a [`Surface`] from a face colour.
pub fn surface_of(face: Color32) -> Surface {
    let bg = legible_surface(face);
    let light_ink = wants_light_ink(bg);
    let ink = if light_ink { INK_LIGHT } else { INK_DARK };
    // The direction a RAISED thing goes: lighter on a dark panel, darker on a
    // pale one. That is also the direction the ink came from, so every step
    // this way costs contrast — which is why each one is pulled back below.
    let up = if light_ink { Color32::WHITE } else { Color32::BLACK };
    // And the direction a RECESSED thing goes, away from the ink. Contrast
    // only improves here, so it is free.
    let down = if light_ink { Color32::BLACK } else { Color32::WHITE };
    let step = |from: Color32, to: Color32, t: f32| {
        let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
        Color32::from_rgb(f(from.r(), to.r()), f(from.g(), to.g()), f(from.b(), to.b()))
    };
    // **As far as it can go and still be written on.** A button that reads as
    // raised is worth nothing if the word on it stops being readable, and the
    // panel ground is guaranteed legible, so walking back toward it always
    // converges.
    let hold = |target: Color32| {
        if contrast_ratio(ink, target) >= 4.6 {
            return target;
        }
        let mut out = target;
        for k in 1u8..=20 {
            out = step(target, bg, f32::from(k) * 0.05_f32);
            if contrast_ratio(ink, out) >= 4.6 {
                break;
            }
        }
        out
    };
    Surface {
        bg,
        ink,
        // **Quieter is a matter of WEIGHT, not of being harder to read.**
        // This was a flat blend a third of the way toward the ground with no
        // floor under it, and on the pale themes a third of the way toward a
        // near-white panel is already a grey nobody reads: the confirmation
        // dialog's "Kept:" line — the one somebody has to read to learn the
        // action is SAFE — came out at 3.45:1 on Copper. Blended as asked and
        // then walked back until it clears the same 4.5 every other word in
        // the app answers to. `arrangement_panel::quiet` had this right
        // already; it is here now so every surface gets it.
        faint: hold_ink(step(ink, bg, 0.34), ink, bg),
        raised: hold(step(bg, up, 0.14)),
        hover: hold(step(bg, up, 0.26)),
        well: step(bg, down, 0.30),
        edge: step(ink, bg, 0.45),
    }
}


/// Walk a quieted ink back toward full strength until it clears 4.5:1.
///
/// The companion to [`legible_surface`], from the other side: there the
/// SURFACE moves because neither ink can win on it; here the surface is
/// already fine and it is the deliberate softening that went too far.
fn hold_ink(soft: Color32, ink: Color32, bg: Color32) -> Color32 {
    const FLOOR: f32 = 4.5;
    if contrast_ratio(soft, bg) >= FLOOR {
        return soft;
    }
    // Back toward the full-strength ink, which is guaranteed to clear —
    // `bg` came through `legible_surface` — so this always terminates on
    // something readable.
    for k in 1u8..=12 {
        let t = 1.0 - f32::from(k) * (1.0 / 12.0);
        let f = |x: u8, y: u8| {
            (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8
        };
        let out = Color32::from_rgb(
            f(ink.r(), soft.r()),
            f(ink.g(), soft.g()),
            f(ink.b(), soft.b()),
        );
        if contrast_ratio(out, bg) >= FLOOR {
            return out;
        }
    }
    ink
}

/// CIE76 over D65 — enough to answer "can these two be told apart", which is
/// the only question ever asked of it.
///
/// **The companion to [`contrast_ratio`], and not a substitute for it.**
/// Contrast ratio is luminance only, so two colours of wildly different hue
/// at the same lightness score near 1:1 on it while being obviously different
/// to look at — the "equal luminance is not equal vividness" trap this app
/// has met before. Use `contrast_ratio` for "can this be READ on that", and
/// this for "is this a different colour from that".
pub fn delta_e(a: Color32, b: Color32) -> f32 {
    fn lab(c: Color32) -> (f32, f32, f32) {
        let ch = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        let (r, g, b) = (ch(c.r()), ch(c.g()), ch(c.b()));
        let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
        let x = f((r * 0.4124 + g * 0.3576 + b * 0.1805) / 0.95047);
        let y = f(r * 0.2126 + g * 0.7152 + b * 0.0722);
        let z = f((r * 0.0193 + g * 0.1192 + b * 0.9505) / 1.08883);
        (116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z))
    }
    let (a, b) = (lab(a), lab(b));
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt()
}

/// The nearest shade of `c` that can actually CARRY its own ink at 4.5:1.
///
/// **There is a dead zone**, and a derived surface lands in it sooner or
/// later. Between roughly 0.12 and 0.22 relative luminance neither the ivory
/// nor the near-black clears 4.5:1 — a mid tone is equally far from both — so
/// a panel derived from a pale theme's roles can come out a colour that
/// nothing can be written on. Rather than let the ink lose, the SURFACE moves:
/// away from whichever ink is already winning, by the least it takes.
///
/// Returns `c` untouched whenever `c` was fine, which is almost always.
/// A surface that can be TOLD APART from the one behind it.
///
/// **A different question from [`legible_surface`], and it needed asking.**
/// That one moves a surface until INK can be printed on it. This one moves a
/// surface until a person can see where it ENDS — which is what a meter's
/// well needs, because a well nobody can find is a meter drawn on nothing.
///
/// The loudness meter's well is the theme's LCD face, and on Sandstone that
/// role and the band's own ground are the same colour to within one percent:
/// the ladder was an invisible rectangle on that palette, in both modes.
///
/// Moved AWAY from the ground — darker on a light band, lighter on a dark
/// one — in small steps, and bounded: a well that had to travel further than
/// this to be seen would no longer be the colour the theme asked for.
pub fn distinct_surface(face: Color32, ground: Color32) -> Color32 {
    /// What "you can see where it ends" measures at. Well under the 4.5:1 a
    /// letter needs: this is an edge, not a word.
    const FLOOR: f32 = 1.25;
    distinct_surface_at(face, ground, FLOOR)
}

/// A palette colour, lifted until a heart drawn in it can be seen on `under`.
///
/// **A strip colour is a SURFACE colour**: the whole table is held under a
/// luminance ceiling so that light text printed on a painted channel still
/// clears 4.5:1. Drawn as a small sprite on the band's own dark face, half of
/// them are nearly invisible. This keeps the hue and raises it until it is
/// something you can actually see, which is the same rule the ink follows,
/// pointed the other way.
pub fn heart_tint(strip: Color32, under: Color32) -> Color32 {
    distinct_surface_at(strip, under, 3.5)
}

/// A palette colour as a BRIGHT shade, for the supporter heart's wave.
///
/// **`heart_tint` is the legibility floor; this is the look.** A strip colour
/// is deliberately muted — the whole table is held under a chroma ceiling so
/// sixteen painted channels sit beside each other without shouting — and a
/// muted colour on a sprite seven pixels wide is a grey smudge. So the hue is
/// pushed away from its own grey first, and only then lifted off the band, and
/// the floor it is lifted to is higher than the tint's: this is meant to be
/// seen from across the room, not merely read.
pub fn heart_shade(strip: Color32, under: Color32) -> Color32 {
    /// How much further from grey than the strip table allows. Above about
    /// this the darker palettes start clipping a channel and change hue.
    const VIVID: f32 = 2.2;
    /// **Where the brightest channel lands, for every shade.**
    ///
    /// Chroma alone was not enough and this is why: pushing a colour away
    /// from its own grey makes it more COLOURFUL without making it any
    /// LIGHTER — the mean is untouched — and the strip table is generated
    /// under a luminance ceiling, so every one of the sixteen starts dark. A
    /// wave of vivid dark colours on a seven-pixel sprite is a wave of
    /// smudges. Normalising the top channel is what makes each of them read
    /// as its hue at a glance, and it does it without moving the hue: the
    /// three channels are scaled by one factor.
    const LIFT_TO: f32 = 238.0;
    /// The floor the fill clears against the band.
    ///
    /// **Three, down from five, and the outline is what pays for it.** On a
    /// PALE band a bright colour has little contrast, so a floor of five
    /// dragged every shade back toward black — brightness and the floor
    /// wanting opposite things, with the floor always winning. The shape is
    /// carried by `heart_border`, which is measured against the FILL and is
    /// tested to stay tellable from it; the fill's own job is to be the
    /// colour. Three is the non-text threshold and it is what a graphic with
    /// its own outline needs.
    const ON_BAND: f32 = 3.0;
    let (r, g, b) = (
        f32::from(strip.r()),
        f32::from(strip.g()),
        f32::from(strip.b()),
    );
    let mean = (r + g + b) / 3.0;
    let push = |c: f32| (mean + (c - mean) * VIVID).clamp(0.0, 255.0);
    let (r, g, b) = (push(r), push(g), push(b));
    // One factor for all three, so the hue is exactly what it was.
    let top = r.max(g).max(b).max(1.0);
    let k = LIFT_TO / top;
    let lift = |c: f32| (c * k).clamp(0.0, 255.0) as u8;
    distinct_surface_at(
        Color32::from_rgb(lift(r), lift(g), lift(b)),
        under,
        ON_BAND,
    )
}

/// The outline around the heart. Black by default, and measured when it can't be.
///
/// **Black is the default and stays the default**, because a dark line reads as
/// an outline where a light one reads as a glow. But an outline has to be
/// tellable from TWO things — the band behind it and the heart inside it — and
/// those move independently: the band's face follows the theme, and the fill
/// follows whichever palette colour the wave is passing through.
///
/// The catch that made this take two goes: on a LIGHT band `heart_tint`
/// darkens the fill to clear it, so the fill and a black outline end up on the
/// same side and the outline disappears into the heart it is supposed to be
/// drawing. Measuring only against the band could not see that, because
/// against the band the black was fine.
///
/// Trying to clear both at once is what does not work, and the measurement
/// says so plainly: on a light band the fill is dark and the band is light, so
/// the outline is squeezed between two extremes and neither end clears both.
///
/// The way out is to notice which job belongs to which. `heart_tint` already
/// guarantees the FILL stands off the band at 3.5:1, so the shape is legible
/// before any outline is drawn — the outline's own job is only to be seen
/// against the heart it edges. So that is what it is measured against, and
/// black keeps ties, which is what keeps it the default everywhere ordinary.
pub fn heart_border(fill: Color32) -> Color32 {
    let black = Color32::from_rgb(0x0D, 0x0A, 0x08);
    let white = Color32::from_rgb(0xF2, 0xEC, 0xDF);
    if contrast_ratio(white, fill) > contrast_ratio(black, fill) {
        white
    } else {
        black
    }
}

/// The rim a SELECTED track wears, measured against the plate it sits on.
///
/// **Not a fixed warm white, which is what it used to be.** A channel can be
/// painted ANY of the strip colours, and the header plate behind it takes that
/// colour too — so the one rim that says "this track is selected" was being
/// drawn in near-white at low alpha over plates that were already near-white
/// on half the palettes. Invisible, and invisible in exactly the case where it
/// matters most: a multiple selection is only legible if you can see which
/// tracks are in it.
///
/// The ink is chosen by the plate, never inherited, and then pushed until it
/// measurably clears it. Blended here rather than drawn at an alpha, so the
/// colour that lands on screen is the colour that was measured.
pub fn selection_rim(under: Color32) -> Color32 {
    /// A rim is an EDGE, not a word: it has to be findable, not readable, so
    /// this sits well under the 4.5:1 a letter needs and a little over the
    /// 1.25 that only has to separate two surfaces.
    const FLOOR: f32 = 1.9;
    let ink = if wants_light_ink(under) {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    // Most of the way to a hairline, then only as far as it has to go.
    let mut out = toward(under, ink, 0.45);
    for _ in 0..16 {
        if contrast_ratio(out, under) >= FLOOR {
            break;
        }
        out = toward(out, ink, 0.12);
    }
    out
}

/// **A LIT colour that can be seen on its ground AND written on.**
///
/// The theme names one `key_active` and it is drawn on grounds at opposite
/// ends of the luminance axis: the piano's keys, the theory view's page in
/// light mode, the same page in dark. Mono is where that breaks — its
/// `#f0f0f0` measures 16.2:1 on the dark page and **1.25:1** on the light
/// one, which is a note you cannot see.
///
/// And it cannot be fixed in the theme table. `key_active` must clear 3.0:1
/// against the band (`every_factory_theme_is_readable_on_its_own_background`),
/// which forces luminance at or above 0.236; clearing 3.0:1 against Mono's
/// light page forces it at or below 0.196. Those windows do not overlap for
/// ANY colour, because it is an argument about luminance and not about hue.
/// So the value stays the theme's and the SURFACE it is drawn as is derived,
/// which is what every neighbour in these palettes already does.
///
/// Two constraints, and the second is load-bearing: the disc has to be
/// visible on the page, and the note name printed ON the disc has to be
/// readable. Deriving against the ground alone drops `lit_text` under 4.5:1
/// on eight theme-and-mode pairs. Because the mid greys are the documented
/// dead zone — where neither ink clears — the walk has to be allowed to run
/// straight through them rather than stopping in one.
pub fn lit_on(c: Color32, ground: Color32) -> Color32 {
    /// A disc is a graphic, so the non-text threshold applies to the disc.
    const ON_GROUND: f32 = 3.0;
    /// The name printed on it is text, so the text threshold applies to that.
    const ON_ITSELF: f32 = 4.5;
    let ok = |x: Color32| {
        let ink = if wants_light_ink(x) { INK_LIGHT } else { INK_DARK };
        contrast_ratio(x, ground) >= ON_GROUND && contrast_ratio(x, ink) >= ON_ITSELF
    };
    if ok(c) {
        return c;
    }
    // Away from the ground: toward black on a pale page, toward white on a
    // dark one — the direction rule `distinct_surface_at` uses.
    let away = if wants_light_ink(ground) {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    for step in 1u8..=40 {
        let out = toward(c, away, f32::from(step) * 0.025_f32);
        if ok(out) {
            return out;
        }
    }
    // Nothing on the line satisfied both. Keep the ground's own ink, which is
    // guaranteed to clear the page even if it is no longer the theme's hue —
    // a note you can see in the wrong colour beats one you cannot.
    if wants_light_ink(ground) {
        INK_LIGHT
    } else {
        INK_DARK
    }
}

/// [`distinct_surface`] with the floor named by the caller.
///
/// A well only has to be findable; a meter's FILL has to be readable at a
/// glance from across a room, and those are different numbers.
pub fn distinct_surface_at(face: Color32, ground: Color32, floor: f32) -> Color32 {
    if contrast_ratio(face, ground) >= floor {
        return face;
    }
    let away = if wants_light_ink(ground) {
        // A dark band: the well lifts off it.
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    let mut out = face;
    for _ in 0..16 {
        out = toward(out, away, 0.08);
        if contrast_ratio(out, ground) >= floor {
            break;
        }
    }
    out
}

/// **An eighth note, DRAWN.**
///
/// The theory send's switch. It was a font glyph — U+266A out of a terminal
/// face, then `nf-md-music-note` out of the Nerd Font set — and the owner's
/// note on both was the same: it does not read as a note. The Unicode one is
/// a square head on a straight flag; the Nerd Font one is rounder but the
/// head is a circle, and a notehead is not a circle. It is an OBLONG, and it
/// is TILTED, and that tilt is most of what the eye recognises.
///
/// So it is two shapes and a line, which is the road [`crate::app`]'s tick
/// mark already takes and for the same reasons: a shape the painter emits is
/// one this app decides the proportions of, one that scales to a
/// seventeen-point switch without a hinting engine's opinion, and one a test
/// can read back out of the shape list. A rasterised glyph is none of those.
///
/// Drawn to fill `r` — give it the switch's rect and it sizes itself.
pub fn draw_note(painter: &Painter, r: Rect, ink: Color32) {
    if !r.is_positive() {
        return;
    }
    let s = r.width().min(r.height());
    let c = r.center();
    // **The head: an oblong, tilted up to the right.** Twenty degrees is
    // engraving's own angle — far enough to read as deliberate at seventeen
    // points, not so far that it looks like it is falling over.
    let (rx, ry) = (s * 0.225, s * 0.150);
    let (sa, ca) = (-0.35_f32).sin_cos();
    let head = Pos2::new(c.x - s * 0.09, c.y + s * 0.21);
    let ring: Vec<Pos2> = (0..20_u8)
        .map(|i| {
            let t = f32::from(i) / 20.0 * std::f32::consts::TAU;
            let (x, y) = (rx * t.cos(), ry * t.sin());
            Pos2::new(head.x + x * ca - y * sa, head.y + x * sa + y * ca)
        })
        .collect();
    painter.add(egui::Shape::convex_polygon(ring, ink, Stroke::NONE));
    // **The stem, off the head's right shoulder**, which is where a stem
    // goes on a note that points up.
    let stem_x = head.x + rx * ca;
    let top = c.y - s * 0.40;
    let w = (s * 0.085).max(1.2);
    painter.line_segment(
        [Pos2::new(stem_x, head.y - ry * 0.2), Pos2::new(stem_x, top)],
        Stroke::new(w, ink),
    );
    // **The flag**, as a pennant rather than a curve. A quaver's flag really
    // is a curve, and a curve two pixels thick at this size is a smudge; the
    // straight one keeps the silhouette, which is the part that reads.
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(stem_x - w * 0.5, top),
            Pos2::new(stem_x + s * 0.26, top + s * 0.17),
            Pos2::new(stem_x - w * 0.5, top + s * 0.34),
        ],
        ink,
        Stroke::NONE,
    ));
}

pub fn legible_surface(c: Color32) -> Color32 {
    let light = contrast_ratio(c, INK_LIGHT);
    let dark = contrast_ratio(c, INK_DARK);
    if light.max(dark) >= 4.5 {
        return c;
    }
    // Commit to the ink that is closer, and move the ground away from it.
    let (ink, toward) = if light > dark {
        (INK_LIGHT, Color32::BLACK)
    } else {
        (INK_DARK, Color32::WHITE)
    };
    let mut out = c;
    for step in 1u8..=50 {
        let t = f32::from(step) * 0.02_f32;
        let f = |x: u8, y: u8| {
            (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8
        };
        out = Color32::from_rgb(
            f(c.r(), toward.r()),
            f(c.g(), toward.g()),
            f(c.b(), toward.b()),
        );
        if contrast_ratio(out, ink) >= 4.6 {
            break;
        }
    }
    out
}

pub fn wants_light_ink(c: Color32) -> bool {
    contrast_ratio(c, INK_LIGHT) > contrast_ratio(c, INK_DARK)
}

/// The band's own background, which is what the heart is drawn on.
///
/// The app needs it to measure the heart's colour and outline against, and the
/// band's palette is private — so this is the one value that leaves, rather
/// than the whole palette. Asking for `panel_face` instead would measure
/// against the fx strip, which is a different surface the heart is nowhere
/// near.
pub fn band_face(s: &Settings) -> Color32 {
    palette(s).bg
}

pub(crate) fn palette(s: &Settings) -> Palette {
    // The BAND's own background, and its own choice. It used to borrow the
    // piano's, so the recorder read as another band of the same window — which
    // is right until you want to tell a set of controls apart from an
    // instrument at a glance while playing.
    let bg = s.recorder_bg_color.to_color32();
    // Everything else follows the BACKGROUND, not `dark_mode`. See
    // `wants_light_ink`.
    let dark = wants_light_ink(bg);
    Palette {
        bg,
        // Derived FROM the background rather than fixed, so a recessed well
        // still looks recessed and a control still looks raised whatever the
        // band is coloured. Multiplying keeps the hue: a well in a walnut band
        // is a darker walnut, not a grey hole in it.
        well: shade(bg, if dark { 0.55 } else { 0.88 }),
        field: shade(bg, if dark { 1.35 } else { 0.94 }),
        ink: if dark { INK_LIGHT } else { INK_DARK },
        // Secondary text carries the folder preview, the disk line and the
        // WORD UNDER EVERY KNOB, so it is darkened on light and lightened on
        // dark until it reads at a glance.
        //
        // **And then held to the same 4.5:1 every other word answers to**,
        // which it was not. These two colours were picked against the stock
        // walnut band and then inherited by all sixteen themes: measured,
        // they came out between 3.17 and 3.61 against the band on EVERY one
        // of them, which is the WCAG floor for large text and this is not
        // large text — it is REVERB and LIMITER at nine points. The eighth
        // instance of the rule this app keeps learning: ink is a property of
        // the surface it lands on, never a colour inherited from a role. See
        // `hold_ink`, which `surface_of` has used since the fourth.
        faint: hold_ink(
            if dark {
                Color32::from_rgb(0x9a, 0x92, 0x80)
            } else {
                Color32::from_rgb(0x6b, 0x60, 0x4a)
            },
            if dark { INK_LIGHT } else { INK_DARK },
            bg,
        ),
        line: if dark {
            Color32::from_rgb(0x62, 0x5c, 0x50)
        } else {
            Color32::from_rgb(0x9c, 0x8f, 0x74)
        },
        accent: s.white_key_active_color.to_color32(),
        rec: REC_RED,
        lcd: s.scope_draw_color.to_color32(),
        lcd_face: s.meter_bg_color.to_color32(),
        fx_knob: s.fx_knob_color.to_color32(),
        master: s.master_knob_color.to_color32(),
        varispeed: s.varispeed_color.to_color32(),
    }
}

/// The background, lightened or darkened, keeping its hue.
///
/// `gamma_multiply` would fade it towards transparent; this scales the channels
/// so a walnut band gets a darker walnut well rather than a grey one.
fn shade(c: Color32, k: f32) -> Color32 {
    let f = |v: u8| (f32::from(v) * k).clamp(0.0, 255.0) as u8;
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

/// **Retired: nothing on this panel is bold any more.**
///
/// The band printed its words in Courier Prime BOLD and its numbers in the
/// regular weight, which on a surface this dense read as two different
/// panels. The owner's call was to take the bold out entirely — "NO BOLDED
/// TEXT ELEMENTS ANYWHERE" — and it may come back, so the face is named here
/// rather than deleted.
#[allow(dead_code)]
fn font_bold(size: f32) -> FontId {
    FontId::new(size, fonts::courier_bold())
}

fn font_light(size: f32) -> FontId {
    FontId::new(size, fonts::courier())
}

/// Advance width of one character as a fraction of the font size, for the
/// bundled monospaced faces. Used to size text to its box without laying it
/// out first, which a pure painter cannot do.
const ADV: f32 = 0.62;

/// A size at which `text` fits across `r`, never above `nominal`.
///
/// Shrink rather than truncate. A destination path is most useful at its END —
/// `.../Movies/Tangent` — and clipping takes exactly that away, while a path
/// two points smaller than its neighbours is merely a long path.
fn fit_text(r: Rect, text: &str, nominal: f32) -> f32 {
    let n = text.chars().count().max(1) as f32;
    nominal.min(r.width() / (n * ADV)).max(0.0)
}

/// Below this a glyph is a smudge, and a smudge reads as a rendering fault
/// rather than as "your window is too small". The band draws nothing instead.
const MIN_TEXT: f32 = 5.0;

// ── the shared layout ──────────────────────────────────────────────────────

/// Every rectangle the band uses, computed once from the rect and the state.
///
/// **`draw` and `hit_test` both go through this and neither computes a
/// rectangle of its own.** That is what keeps a click landing on the thing it
/// looks like it is on: the two cannot drift, because there is only one set of
/// numbers. It is also what makes the properties that matter testable without a
/// screen — that the destination controls are gone mid-take, that the faders
/// are not, and that the clock is not drawn when the user asked for no clock.
///
/// A rectangle that is not offered is [`Rect::NOTHING`], which contains no
/// point and intersects nothing, so an absent control cannot be clicked by
/// accident and cannot collide with a present one in the overlap test.
/// `Rect::ZERO` would not do: it contains the origin.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    /// The take is live, in any of its three senses.
    rolling: bool,
    /// The box the camera image is fitted INTO. Fixed by the band; never by
    /// the camera.
    preview: Rect,

    // ── transport ──
    /// The green button: play from the playhead. Play only - a press while
    /// the song rolls does nothing, and Pause is its own button. Seated in
    /// BOTH layouts, in the same place - a take is capture only - and
    /// `NOTHING` only under a realtime export, which owns the transport
    /// (`fill_rolling`).
    play: Rect,
    /// The arrangement's red button: seated under a take as at rest,
    /// `NOTHING` under a realtime export.
    record: Rect,
    /// Stop: the transport and the arrangement's record, never a take (the
    /// cassette ends that). Seated in every layout, an export included - it
    /// is how an export is cut short.
    stop: Rect,
    meter: Rect,
    /// The take's readout - the count-in digit, the take's clock, FINISHING -
    /// on the LCD's scope glass. [`Rect::NOTHING`] at rest and when the
    /// clock is hidden.
    timecode: Rect,

    // ── the timeline ──
    /// The waveform with the playhead in it, inline in the band — the panel
    /// that used to hang off the track icon, promoted to the space the slot
    /// selector and the fader rows gave up. Click it to LOCATE; click it empty
    /// to import. It is the first face of the arrangement, so it is drawn as
    /// one lane of a thing that will one day show several.
    timeline: Rect,
    /// The LCD cluster on the timeline's left: the big bars|beats readout
    /// and the Start/End/Length panel — the owner's REAPER transport.
    lcd: Rect,
    /// Whether a file is loaded, which decides what a click on the timeline
    /// MEANS: a locate when there is something to locate in, the import
    /// dialog when there is not. Layout state rather than a view lookup so
    /// [`Layout::targets`] stays a pure function of `self`.
    timeline_loaded: bool,
    /// The × in the timeline's top-right corner that unloads the file. The
    /// BACKING strip that used to carry the remove gesture retired at v13.
    timeline_close: Rect,

    /// The master column: the output meter, the limiter's gain reduction
    /// beside it, and the master knob under both.
    ///
    /// **A different signal from `meter`.** The VU shows what is being
    /// RECORDED — the input when there is one — and this shows what LEAVES:
    /// after the effects, after the limiter, after the master. On a machine
    /// with an interface plugged in those are two genuinely different things,
    /// and neither one answers the other's question.
    /// The dB scale, **on both sides of the ladders**. Each channel is read
    /// against the numbers next to it; a right channel three inches from the
    /// only scale is a right channel nobody reads accurately.
    master_scale: [Rect; 2],
    master_bars: Rect,
    master_knob: Rect,

    /// The loudness ladder, left of the master. Its scale runs down one side
    /// only: the numbers on a LUFS ladder are three characters wide and the
    /// column is half the master's, so a scale at each side would leave no
    /// ladder between them.
    /// The regular decibel scale down the RIGHT of the ladder: a tick every
    /// six LU with its number beside it. The named targets are words on the
    /// LEFT — see [`crate::recorder::LUFS_MARKS`] — and the two never share
    /// a side, because a column of words against a column of numbers is
    /// how you tell at a glance which is which.
    lufs_ticks: Rect,
    lufs_bar: Rect,
    /// The number under the ladder, and the target that cycles the window —
    /// momentary, short-term, integrated. See [`LoudnessWindow`].
    lufs_readout: Rect,
    /// The varispeed dial, at the foot of the loudness column.
    ///
    /// **The same seat the master knob has under its own ladder**, which is
    /// what puts it in the gap between the LIMITER and the MASTER that the
    /// owner asked for and what makes the two feet line up. It is not one of
    /// the eight — it is not an effect, it changes the SPEED — so it is not
    /// in `fx` and it is not drawn like them.
    vari: Rect,

    /// The six effect knobs, in [`Fx::ALL`] order, under the meters.
    ///
    /// The whole CELL, label included, and the whole cell is the drag target:
    /// a knob is thirty points across and a control you can only grab by its
    /// own diameter is a control you keep missing.
    fx: [Rect; Fx::ALL.len()],
    /// Where a typed tempo goes: a small box under the knob, drawn only while
    /// one is being typed.
    tempo_entry: Rect,
    /// The TAP chip, in the tempo cell's slack beside the knob's circle.
    /// `NOTHING` when the band is too narrow to seat it, and while a take
    /// rolls (the knob itself yields to the clock then).
    pub tempo_tap: Rect,


    /// **Where `PITCH CONT` may print** — wider than the dial's own cell.
    ///
    /// The word is the varispeed's readout (the dial has no number), and it
    /// was sized to the CELL, which is one knob wide: `fit_text` cut it to
    /// about five points and the `× 2` that was supposed to make it shout
    /// never bit at all. The dial has a gap of clear band on either side, so
    /// the word is allowed to use it — right up to its neighbours and no
    /// further.
    vari_label: Rect,
    /// The supporter heart, bottom-right of the band.
    ///
    /// **Not a hit** as far as this module is concerned: the band knows where
    /// it goes and nothing more. Cycling its colour and raising the thanks card
    /// on hover belong to whoever owns the licence and the pointer, which is
    /// the app, so it reads this rect and does both itself.
    heart: Rect,

    // ── destination ──
    /// Opens the menu the take's settings moved into. A button rather than
    /// only a right-click, because a control nobody can find is a control that
    /// was removed.
    setup: Rect,
    /// The cassette: capture the whole session, apart from the arrangement.
    capture: Rect,
    /// The project button: the session menu — open, save, save as, template.
    project: Rect,
    /// PAUSE: hold the playhead where it stands. Stop is the one that goes
    /// home; two buttons, two meanings, the owner's order.
    pause: Rect,
    dest: Rect,
    /// The button that shows the folder in the file manager.
    ///
    /// Its own target rather than a second affordance inside the FOLDER box:
    /// that box already carries "Choose...", and a box that opens a picker when
    /// you click the left of it and a Finder window when you click the right is
    /// a box nobody can predict.
    reveal: Rect,
    name: Rect,
    /// Live grey text under the name field. Teaches the naming scheme without
    /// a help page, and is not clickable.
    folder: Rect,
    /// How long the disk will last, beside the folder preview. Also not
    /// clickable: it is an answer, not a question.
    disk: Rect,
    count_in: Rect,
    tempo: Rect,
    /// The time signature, typed rather than dragged. NOT `meter` — that name
    /// is the level meter's, twenty lines up.
    time_sig: Rect,
    export: Rect,
    /// The tick beside Export: show the take's folder as soon as it is done.
    open_when_done: Rect,


    /// Hairlines in the gaps between the groups. Not controls, and not hits —
    /// they are the cheapest way to make four groups look like four groups
    /// rather than like eighteen boxes in a row.
    rules: [Rect; 2],
}

/// A horizontal slice of `r`, by fraction of its width.
fn slice_h(r: Rect, from: f32, to: f32) -> Rect {
    Rect::from_min_max(
        Pos2::new(r.left() + r.width() * from, r.top()),
        Pos2::new(r.left() + r.width() * to, r.bottom()),
    )
}

/// A vertical slice of `r`, by fraction of its height.
fn slice_v(r: Rect, from: f32, to: f32) -> Rect {
    Rect::from_min_max(
        Pos2::new(r.left(), r.top() + r.height() * from),
        Pos2::new(r.right(), r.top() + r.height() * to),
    )
}

/// A one-point vertical hairline down `r` at absolute `x`.
fn rule_x(r: Rect, x: f32) -> Rect {
    if !r.is_positive() {
        return Rect::NOTHING;
    }
    Rect::from_min_max(Pos2::new(x - 0.5, r.top()), Pos2::new(x + 0.5, r.bottom()))
}

/// The camera preview's width over its height. The commonest sensor shape, and
/// the shape of the file a take writes.
const PREVIEW_ASPECT: f32 = 16.0 / 9.0;

impl Layout {
    /// Nothing at all, for a rect too small to hold a band. Every field absent
    /// rather than degenerate: negative rectangles produce NaN centres, and a
    /// NaN centre is a shape drawn nowhere and a hit test that never matches.
    fn empty(rolling: bool) -> Self {
        Self {
            rolling,
            preview: Rect::NOTHING,
            heart: Rect::NOTHING,
            vari_label: Rect::NOTHING,
            play: Rect::NOTHING,
            record: Rect::NOTHING,
            stop: Rect::NOTHING,
            meter: Rect::NOTHING,
            timecode: Rect::NOTHING,
            timeline: Rect::NOTHING,
            lcd: Rect::NOTHING,
            timeline_loaded: false,
            timeline_close: Rect::NOTHING,
            master_scale: [Rect::NOTHING; 2],
            master_bars: Rect::NOTHING,
            master_knob: Rect::NOTHING,
            lufs_ticks: Rect::NOTHING,
            lufs_bar: Rect::NOTHING,
            lufs_readout: Rect::NOTHING,
            vari: Rect::NOTHING,
            fx: [Rect::NOTHING; Fx::ALL.len()],
            tempo_entry: Rect::NOTHING,
            tempo_tap: Rect::NOTHING,
            setup: Rect::NOTHING,
            capture: Rect::NOTHING,
            project: Rect::NOTHING,
            pause: Rect::NOTHING,
            dest: Rect::NOTHING,
            reveal: Rect::NOTHING,
                name: Rect::NOTHING,
            folder: Rect::NOTHING,
            disk: Rect::NOTHING,
                    count_in: Rect::NOTHING,
            tempo: Rect::NOTHING,
            time_sig: Rect::NOTHING,
            export: Rect::NOTHING,
            open_when_done: Rect::NOTHING,
            rules: [Rect::NOTHING; 2],
        }
    }

    fn new(rect: Rect, view: &RecorderView<'_>) -> Self {
        let rolling = view.state.is_active();
        // The SAME margin on all four sides. It used to be a fraction of the
        // height, which on a wide band is a thin top-and-bottom against a
        // generous left-and-right — and with the status line under the body
        // the bottom read as twice the top.
        let pad = (rect.height() * 0.055).clamp(1.0, 10.0);
        let gap = (rect.height() * 0.05).clamp(1.0, 10.0);
        let inner = rect.shrink(pad);
        if !inner.is_positive() {
            return Self::empty(rolling);
        }

        // **The preview keeps the band's own margin on three sides.** It is
        // the camera inset a take will carry, so it is not a control among
        // controls: it is a picture, and a picture hung with a different gap
        // under it than beside it reads as an accident. The same `pad` above
        // it, to the left of it, and below it — which means it takes the full
        // height of `inner` and the status line starts to its right.
        let pv_w = (inner.height() * PREVIEW_ASPECT).min(inner.width() * 0.24);
        let preview = Rect::from_min_max(
            inner.min,
            Pos2::new(inner.left() + pv_w, inner.bottom()),
        );

        // Everything else lives to the right of it.
        let rest = Rect::from_min_max(
            Pos2::new(preview.right() + gap, inner.top()),
            inner.max,
        );
        if !rest.is_positive() {
            return Self::empty(rolling);
        }
        // The status line spans what the preview leaves, in both layouts, so
        // "no audio input selected" is in the same place before and during a
        // take.
        // **One line of small text, and usually empty.** It was a fifth of
        // **The heart holds the band's true bottom-right corner.**
        //
        // It held the TOP-right once, and the master column was cut short of
        // the band's edge to leave it room — which is what stopped the master
        // hugging the right and cost the loudness ladder the width it was
        // short of. Then it moved to the columns' floor, still held off the
        // corner itself because the status line's dismiss × lived there.
        //
        // The status line is GONE — its messages are windows now — so the
        // corner is free and the heart takes it. Nothing else is drawn there:
        // the master's knob is centred between its two scales, so the room
        // under the right-hand scale is margin and nothing else.
        //
        // Too-narrow bands skip it rather than draw a smudge.
        let heart = {
            // A sixth again as big as it was. It is the way into the thanks
            // card and it is a credit, and at twelve points it was neither
            // findable nor legible as a heart.
            // **The BOX, not the sprite.** `chord_strip::heart_grid` now pays
            // for the outline's overhang out of the box it is given rather
            // than letting it spill past the edge and be clipped on one side,
            // so a box sized to the sprite alone would draw a sprite an eighth
            // smaller than this band has always shown. The margin is added
            // back here, once, where the box is chosen.
            // **A SQUARE box**, because the heart is pixel art and its cells
            // have to stay square — see `chord_strip::heart_grid`. The sprite
            // is seven wide by six tall, so a square box sizes it by its
            // WIDTH and leaves a sliver of margin above and below; that
            // margin is what centres it.
            //
            // Ten per cent off what it was: big enough to read as a heart,
            // small enough not to sit on the master's scale.
            let sprite = (inner.height() * 0.063).clamp(8.0, 15.0);
            let side = sprite * crate::chord_strip::HEART_BOX
                * crate::chord_strip::HEART_ASPECT;
            let (w, h) = (side, side);
            if w > inner.width() * 0.06 {
                Rect::NOTHING
            } else {
                Rect::from_min_size(
                    Pos2::new(inner.right() - w - 1.0, inner.bottom() - h - 1.0),
                    Vec2::new(w, h),
                )
            }
        };
        // **To the same bottom line as the camera.** The columns used to
        // stop a status row short of the band's edge while the preview ran
        // all the way down — the owner asked why the camera goes lower than
        // everything else. The status is an OVERLAY now: the columns take
        // the full height, and a message (rare) draws on a plate over the
        // bottom sliver for as long as it exists.
        let body = rest;
        if !body.is_positive() {
            return Self::empty(rolling);
        }
        let mut l = Self {
            preview,
            timeline_loaded: !view.track.is_empty(),
            // The message never runs into the clip warning, whatever either
            // says: they share the row and neither may be the reason the other
            // is unreadable.
            // **The whole row.** It shared it with a `CLIPPED` word that has
            // gone: the lamp on the VU face says that, in the place somebody
            // watching levels is already looking.
            heart,
            ..Self::empty(rolling)
        };

        if rolling {
            l.fill_rolling(body, gap, view);
        } else {
            l.fill_idle(body, gap, view);
        }
        l
    }

    /// Idle: preview, transport, destination, monitor.
    ///
    /// In that order deliberately. The device pickers are NOT first, because
    /// after the first session they are the controls nobody ever touches again;
    /// the faders are last because that is where they are during a take, and
    /// they are the one group that may not move between the two layouts.
    fn fill_idle(&mut self, body: Rect, gap: f32, view: &RecorderView<'_>) {
        // The preview wants to be landscape, so its width follows the band's
        // own HEIGHT — never the camera's aspect, which is the whole point.
        // It gives up about a fifth of what it used to claim: three faders and
        // an instrument row moved in, and a framing view you look at twice a
        // session may not crowd the controls you use every take.
        //
        // The 0.22 cap only binds in a DETACHED window, where the band is tall
        // for its width; in the band itself the height term is smaller than
        // either bound long before the cap matters.
        // **Three groups now, not four.** The take's settings went to the
        // menu, so the band is what you look at (the preview and the name),
        // what you touch every take (transport, meters, faders), and what you
        // load into it (the slots). The middle group is more than twice the
        // width it had, which is where the bigger meters and the reachable
        // faders come from.
        // The picture on the far left, full height; the words in their own
        // column beside it. They used to be stacked in one narrow column,
        // which made the preview short AND the type small — the two were
        // fighting over the same points and both lost.

        let _ = view;

        // One source for the middle, shared with the rolling layout and with
        // `fill_faders`, so the timeline cannot drift from the meters beside
        // it. See `Layout::middle_of`.
        let middle = Self::middle_of(body, gap);
        if !middle.is_positive() {
            return;
        }
        self.rules[0] = rule_x(body, middle.left() - gap * 0.5);
        self.fill_transport(middle);
        self.fill_faders(body, gap, false);
    }

    /// The middle group's rectangle AT REST, whatever the state.
    ///
    /// The faders are placed from this in both layouts, so the two controls
    /// somebody reaches for with their eyes on their hands do not move when a
    /// take starts. Everything else in the group is free to grow — the clock
    /// triples and the meters get the room the preview gives up — but a fader
    /// that slid sideways at the moment it became the only thing you might
    /// still want to touch would be half a survivor. Same argument the monitor
    /// column has always made; the faders took it with them when they moved.
    fn middle_of(body: Rect, _gap: f32) -> Rect {
        // **There is no instrument column any more.** The desk's insert bays
        // are the instrument rack now, so the third of the band the slot
        // selector was using is width the timeline and the meters have. And
        // nothing is held back at the bottom either: the heart's strip used
        // to be carved off here, which stopped every column — the meters, the
        // dials, the MASTER — a credit's height above the band's edge. The
        // credit lives in the status row now and the columns run to the foot.
        body
    }


    /// The left column of the middle group: tempo, two faders, transport.
    ///
    /// **Four rows in one column, and they belong together.** They were three
    /// separate things — a words column with the name and the tempo in it, a
    /// fader pair, and a transport bar at the foot of the words — which is
    /// three columns' worth of margin for two faders and three buttons. What
    /// is here now is the strip a hand rests on during a take: how loud the
    /// click is, how loud the input is, how fast, and go.
    fn fill_faders(&mut self, body: Rect, gap: f32, rolling: bool) {
        let m = Self::middle_of(body, gap);
        if !m.is_positive() {
            return;
        }
        // Their own column, between the transport and the meters, and centred
        // in it: two rows in the middle of a tall column rather than two rows
        // pinned to the bottom of the band, which is what made the bottom of
        // the window read as empty.
        // **The knobs left this column**, and the faders got the width back —
        // see `fill_transport`, where the three of them are now a row under
        // the meters. What they were was a stack ten points wide, which is a
        // knob you cannot read and cannot land on.
        let col = slice_h(m, 0.00, FADER_COL);

        // **The timeline, where the three fader rows were.** The click, the
        // input and the backing track are channels on the desk now, each with
        // a real fader there — what this column shows instead is WHEN: the
        // backing track's waveform with the playhead in it, inline in the
        // band. Click it to locate; click it empty to import. It is the first
        // face of the arrangement, drawn as one lane of a thing that will one
        // day show several.
        let tl_box = slice_v(col, 0.02, 0.66);
        // **The whole box is the LCD now** — the owner retired the overview
        // strip for the moment: a longer timecode with bigger digits, and
        // the Start/End/Length of the arrangement's VIEWPORT beside it,
        // every part of every number draggable. The overview's machinery
        // stays below, dormant, for the day it earns its room back.
        self.lcd = tl_box;
        self.timeline = Rect::NOTHING;
        // The × went with the overview: the timeline empties from the
        // arrangement now (select and Delete).
        self.timeline_close = Rect::NOTHING;
        // The hairline between the timeline and the meters, centred in the
        // gap `fill_transport` leaves — placed here because this function
        // runs for BOTH layouts, so the rule cannot move when a take starts.
        self.rules[1] = rule_x(m, col.right() + m.width() * ROW_PAD);

        // **The transport, under the faders it belongs with.** Three squares
        // of one size, one centred in each third of the row: they are one
        // group, and a stop bigger than the cog beside it reads as more
        // important than it is.
        // **Equal air at both ends of the row.**
        //
        // It had none on the left and a slice on the right: the row started
        // flush at the middle group's own left edge — half a gap from the
        // rule there — while the rule on the RIGHT is pushed out by two
        // percent of the group's width. So the tempo knob sat against its
        // separator and the cog had room to breathe from its one, which is
        // what the owner saw.
        //
        // Measured from the two rules rather than from the column, so the
        // margins cannot drift apart again when either rule moves. The right
        // edge does not move — `right_rule - pad` is `col.right()`, which is
        // where the row already ended; only the left comes in to match it.
        let bar = {
            let row = slice_v(col, 0.70, 1.00);
            let pad = m.width() * ROW_PAD;
            let left_rule = m.left() - gap * 0.5;
            let right_rule = col.right() + pad;
            let (l, r) = (left_rule + pad, right_rule - pad);
            if r > l {
                Rect::from_min_max(Pos2::new(l, row.top()), Pos2::new(r, row.bottom()))
            } else {
                row
            }
        };
        // **The tempo is a knob at the head of the transport**, and that is
        // what makes the transport one thing rather than three buttons and a
        // number that happened to be nearby.
        //
        // **The same cell as an effect knob, to the point.** They are the same
        // control and the eye reads them as a set; a tempo knob a few points
        // off the size of the three beside it looks like a mistake nobody can
        // name. Sized from the first effect cell, which `fill_transport` has
        // already placed — not from a fraction of this row, which would agree
        // with it only by luck and only at one window size.
        //
        // **And it stays through a take.** The take's clock used to take this
        // slot, on the ruling that the tempo could not change mid-take; the
        // owner wants the main controls intact while recording takes, and the
        // take's readout lives on the LCD's glass now. See `fill_rolling`.
        let head = if self.fx[0].is_positive() {
            let size = Vec2::new(
                self.fx[0].width().min(bar.width() * 0.5),
                self.fx[0].height().min(bar.height()),
            );
            // **Sat on the bottom of the row, not floated in the middle of
            // it.** The tempo knob is one of eight and the other seven end at
            // the foot of their own column; centring this one left it a dozen
            // points higher than the rest, with the gap under it reading as
            // the band's bottom margin — which is what "a huge unused margin
            // below the buttons" actually was.
            Rect::from_center_size(
                Pos2::new(bar.left() + size.x * 0.5, bar.bottom() - size.y * 0.5),
                size,
            )
        } else {
            slice_h(bar, 0.00, 0.28)
        };
        // **One row, evenly spaced, spanning exactly the LCD above it.**
        //
        // The owner's rule: the tempo is as far from the tap chip as the tap
        // chip is from Record, and so on down the row. That is EDGE gaps, not
        // centre pitch — the knob cell is wider than a transport button, so
        // equal pitch would put visibly different air between different pairs
        // and read as an accident.
        //
        // And the row fills the bar end to end, which is the same width as
        // the timecode cluster above (both are vertical slices of `col`), so
        // the two read as one instrument rather than a wide readout with a
        // short strip under it.
        let (line, from_knob) = knob_centre(head).map_or((bar.center().y, None), |c| {
            (c.y, knob_face(head).map(|k| k.radius * 2.0))
        });
        let side = from_knob
            .map_or(bar.height() * TRANSPORT_SIDE, |d| d * TRANSPORT_SIDE)
            .min(bar.width() * 0.125)
            // Never so small that a button stops being one - see
            // `TRANSPORT_MIN_SIDE`, and never taller than the row.
            .max(TRANSPORT_MIN_SIDE)
            .min(bar.height());
        // What stands in the row, left to right, by WIDTH. The knob keeps its
        // own cell width; the tap chip is a peer now rather than a passenger
        // in the knob's slack; every transport icon is one square.
        // **The same nine slots in BOTH layouts**, occupied or not. The row
        // has always kept one rule above the others: Stop stands exactly
        // where it stood when the take started. A rolling row that dropped
        // the tap chip and the three buttons it does not use would slide
        // everything left and break that on the one press that matters most.
        // So the grid is fixed and the rolling state simply leaves slots
        // empty.
        let tap_w = (side * 1.9).clamp(24.0, 38.0);
        const ICONS: usize = 7;
        let mut widths: Vec<f32> = Vec::with_capacity(ICONS + 2);
        // **The knob is spaced by what you SEE, not by the cell it lives in.**
        // It is sized from an effect cell, which is wider than the circle
        // inside it; spacing by the cell put a visibly bigger gap between the
        // knob and the tap chip than between any other pair, which is exactly
        // the unevenness the owner named. The cell is trimmed to the circle
        // (plus the room its ticks need), so every gap in the row is the gap
        // between two things a person can point at.
        // **The slot is the knob's VISIBLE extent — ticks included.**
        //
        // Two mistakes to avoid here and both were made. Spacing by the CELL
        // leaves a fifth of a diameter of dead air inside the knob's own
        // slot, so the row reads left-justified however even the arithmetic
        // is. Spacing by the bare circle (2 radii) is worse: `knob_face`
        // budgets the radius from the SHORTER side of the cell, so narrowing
        // the cell below the drawing's own width feeds back and the knob
        // comes out smaller than the six it is supposed to match.
        //
        // The drawing runs to 1.26 radii — the skirt the ticks stand in — so
        // its width is 2.52 radii, and a cell exactly that wide leaves the
        // radius EXACTLY where it was: the shorter side is unchanged, so the
        // budget is unchanged. Same knob as the others, no dead air.
        //
        // **And never narrower than its own NAME.** Trimmed to the dial alone,
        // the slot at 900 points was 14.0 wide and "TEMPO" needs 15.5 at the
        // smallest size the band will set - so the word vanished from the one
        // knob that is a transport control rather than a send. Widening the
        // cell cannot shrink the knob: its radius is budgeted from the shorter
        // side, and in a short band that is the height.
        let caption_w = TEMPO_WORD.chars().count() as f32 * ADV * MIN_TEXT + 0.5;
        let knob_w = knob_face(head).map_or(head.width(), |f| {
            let name = if f.label_h > 0.0 { caption_w } else { 0.0 };
            (f.radius * 2.52).max(name).min(head.width())
        });
        widths.push(knob_w);
        widths.push(tap_w);
        for _ in 0..ICONS {
            widths.push(side);
        }
        let total: f32 = widths.iter().sum();
        // The gap that makes the row span the bar exactly. Never negative:
        // at the narrowest band the items simply sit shoulder to shoulder
        // rather than overlapping.
        // **A floor under the gap, because touching is overlapping.** Two
        // hit regions that share an edge are two controls a press between
        // them can land on either of — the overlap guard says so, and it is
        // right. Where the row cannot afford the floor, every item shrinks
        // to pay for it rather than the row spilling past the bar.
        const MIN_GAP: f32 = 3.0;
        let slots = widths.len().saturating_sub(1) as f32;
        let mut gap = if slots > 0.0 {
            (bar.width() - total) / slots
        } else {
            0.0
        };
        if gap < MIN_GAP {
            gap = MIN_GAP;
            let room = (bar.width() - gap * slots).max(1.0);
            let scale = (room / total).min(1.0);
            for w in &mut widths {
                *w *= scale;
            }
        }
        let total: f32 = widths.iter().sum();
        let _ = total;
        // **The even spacing is HORIZONTAL only.** Each item keeps the
        // vertical placement it already had — the knob sits on the foot of
        // the row, the buttons ride the knob's centre line — because a cell
        // that inherited the bar's own height collapsed to nothing on a short
        // band and took every control in the row with it.
        let mut x = bar.left();
        let mut spans: Vec<(f32, f32)> = Vec::with_capacity(widths.len());
        for w in &widths {
            spans.push((x, x + w));
            x += w + gap;
        }
        let centre = |i: usize| spans.get(i).map(|(a, b)| (a + b) * 0.5);
        // The knob keeps its HEIGHT (the circle is budgeted from the shorter
        // side, so the drawn size does not move) and takes the trimmed width.
        let head = centre(0).map_or(head, |cx| {
            Rect::from_center_size(
                Pos2::new(cx, head.center().y),
                Vec2::new(knob_w, head.height()),
            )
        });
        // **The tempo stays in the row through a take**, in both layouts - the
        // owner: "the main buttons need to be intact while recording takes".
        // The take's own readout, which used to take this slot, lives on the
        // LCD's glass now (see `fill_rolling`). A tempo moved mid-take plays
        // at the new tempo while the take's `.mid` keeps the one tempo mark it
        // started with: its event times stay exact, only its bar lines after
        // the move stop matching. During a count-in it is held still - see
        // `RecorderState::tempo_held`.
        let _ = rolling;
        self.tempo = head;
        self.tempo_tap = centre(1).map_or(Rect::NOTHING, |cx| {
            Rect::from_center_size(
                Pos2::new(cx, line),
                Vec2::new(tap_w.min(widths[1]), (side * 0.9).clamp(11.0, 16.0)),
            )
        });
        // Slot 0 is the knob (or the clock), slot 1 the tap chip, and the
        // seven transport slots follow — in both layouts.
        let first_icon = 2;
        let icon = |i: usize| {
            let at = first_icon + i;
            centre(at).map_or(Rect::NOTHING, |cx| {
                // Never wider than the span it was given: the spans are what
                // the even spacing guarantees, and a square that overhung one
                // would reach into its neighbour's gap.
                let d = side.min(widths.get(at).copied().unwrap_or(side));
                Rect::from_center_size(Pos2::new(cx, line), Vec2::splat(d))
            })
        };
        // The typing box, under the knob and only ever drawn while somebody is
        // typing into it. Allowed to hang below the column: it is an overlay
        // for as long as a number takes to enter, and the alternative is a
        // permanent gap under the transport reserved for nothing.
        self.tempo_entry = Rect::from_min_size(
            Pos2::new(self.tempo.center().x - self.tempo.width() * 0.44, self.tempo.bottom()),
            Vec2::new(self.tempo.width() * 0.88, self.tempo.height() * 0.42),
        );
        if rolling {
            // **The whole transport, under a take as without one.** The take
            // is the capture - a film of the window and the desk - and the
            // owner's use for it is a project breakdown: play the song,
            // stop, locate, record a part, all on camera. So record, play,
            // stop and pause stay exactly where they are and do exactly what
            // they do at rest; only the CASSETTE starts or ends the take.
            // This row used to keep only a steady red dot, the stop and the
            // cassette, on the ruling that "pressing either would mean
            // nothing" - which was true only because the take drove the
            // transport itself.
            //
            // No folder and no cog: everything they open is refused while a
            // take is writing (the devices, the destination, another
            // project), and a button that opens a panel of dead rows is
            // worse than no button.
            self.record = icon(0);
            self.play = icon(1);
            self.stop = icon(2);
            self.pause = icon(3);
            self.capture = icon(4);
        } else {
            self.record = icon(0);
            self.play = icon(1);
            self.stop = icon(2);
            self.pause = icon(3);
            self.capture = icon(4);
            self.project = icon(5);
            // The cog LAST, by the owner's word: transport first, then the
            // session, then the place you go to change how it all works.
            self.setup = icon(6);
            // Seated only where it can be SEEN. The floor under `side` keeps
            // that true at every width the window allows; this is what keeps
            // it true in a detached band dragged smaller than that.
            if !cog_fits(self.setup) {
                self.setup = Rect::NOTHING;
            }
        }
    }

    /// The master column: scale, bars, gain reduction, knob.
    ///
    /// Laid out from the BOTTOM: the knob is a fixed share and the meter takes
    /// whatever is left, because a knob that shrinks with the window stops
    /// being grabbable long before a meter stops being readable.
    fn fill_master(&mut self, m: Rect, knob_size: Vec2) {
        if !m.is_positive() || !(knob_size.x > 0.0 && knob_size.y > 0.0) {
            return;
        }
        // **The heart holds the very corner** and the ladder now runs past
        // it to the VU faces' top line — the column moved LEFT instead, so
        // the two share the height without sharing any pixels.
        // The knob sits at the foot of the column, the same size as an effect
        // knob, **centred between the two ladders rather than on the column**.
        // The column carries the scale down its left-hand side, so its centre
        // is not the meters' centre — and a knob that belongs to the pair
        // above it has to line up with the pair and not with the margin.
        let bars = slice_h(m, MASTER_SCALE_W + 0.03, 1.0 - MASTER_SCALE_W - 0.03);
        let knob = Rect::from_center_size(
            Pos2::new(bars.center().x, m.bottom() - knob_size.y * 0.5),
            Vec2::new(knob_size.x.min(m.width()), knob_size.y),
        );
        if !knob_fits(knob) {
            return;
        }
        // **Straight down to the knob.** There was a readout strip in here —
        // a recess with the output level in it, which is where the reduction
        // used to be drawn. The reduction reads against the scale now, and
        // what the strip was holding open was ladder.
        let meter = Rect::from_min_max(m.min, Pos2::new(m.right(), knob.top() - 4.0));
        if !meter.is_positive() {
            return;
        }
        self.master_knob = knob;
        // Left to right: the numbers, the two bars, then the reduction.
        // **The scale is between nothing and everything** — it is read against
        // the bars, so it goes next to them and not off in a corner.
        // **One split meter, not two.** The pair used to be most of the
        // column and read as two separate bars that happened to be adjacent;
        // narrow and close together they read as one stereo meter with a seam
        // down it, which is what a master is.
        self.master_scale = [
            slice_h(meter, 0.00, MASTER_SCALE_W),
            slice_h(meter, 1.0 - MASTER_SCALE_W, 1.0),
        ];
        self.master_bars = slice_h(meter, MASTER_SCALE_W + 0.03, 1.0 - MASTER_SCALE_W - 0.03);
    }

    /// The loudness column: the scale, the ladder, and the reading under it
    /// — **the whole height of the band, and nothing else in it.**
    ///
    /// The varispeed dial used to stand at its foot, which cost the ladder a
    /// knob's height at the bottom and gave the dial a knob's height to fit
    /// a reading into. See [`VARI_COL`]. The gain and the limiter were tried
    /// here too and went back to the grid: they read as a second, competing
    /// pair of dials beside the pitch control.
    /// The eight effect cells, in the room they are given.
    ///
    /// **Its own method because the room changes twice.** The grid is laid
    /// out once against the VU group's provisional width, and again once the
    /// pitch dial's face is known and the group is trimmed to sit one gap
    /// from it — see `CLUSTER_GAP`. Two call sites, one layout.
    fn refit_fx_grid(&mut self, knobs: Rect) -> bool {
        let row = |top: f32, bottom: f32| slice_v(knobs, top, bottom);
        // 46/54: the dials took one more percent each out of the gap between
        // the rows. Eight percent of the column still stands between HPF's
        // word and the reverb knob's skirt, which is twice the gap that
        // collision happened at.
        let (top, bottom) = (row(0.00, 0.46), row(0.54, 1.00));
        // A quarter each with a fiftieth of the row between them, which is
        // the same proportional gutter the thirds had.
        let col = |i: f32| (i * 0.25 + 0.01 * i.min(1.0).ceil(), (i + 1.0) * 0.25);
        let cell = |r: Rect, i: usize| {
            let i = i as f32;
            let (a, b) = (i * 0.25, (i + 1.0) * 0.25);
            let pad = 0.012;
            slice_h(r, a + if i > 0.0 { pad } else { 0.0 }, b - if i < 3.0 { pad } else { 0.0 })
        };
        let _ = col;
        let cells = [
            cell(top, 0),
            cell(top, 1),
            cell(top, 2),
            cell(top, 3),
            cell(bottom, 0),
            cell(bottom, 1),
            cell(bottom, 2),
            cell(bottom, 3),
        ];
        // **A control nobody can see is not a control.** `draw_knob` refuses a
        // cell too small to be a knob rather than drawing a smudge, so the
        // layout has to refuse it too — otherwise the band keeps a live drag
        // target over blank panel. One predicate, asked by both.
        //
        // **Answers whether it TOOK.** All eight or none — seven knobs where
        // there should be eight is worse than none, because the missing one
        // is the one somebody goes looking for — and the caller has to know
        // which: the right end trims this group to make room for the pitch
        // dial, and if the grid cannot live in what is left then the dial is
        // the one that has to give instead.
        let ok = cells.iter().copied().all(knob_fits);
        if ok {
            self.fx = cells;
        }
        ok
    }

    fn fill_lufs_sized(&mut self, c: Rect, bar_w: f32) {
        if !c.is_positive() {
            return;
        }

        // **A SMALLER reading than it had.** It says `M -14.2` — a letter
        // and four digits — and at a fifth of the column it was the largest
        // type in the band's right-hand end, printed under the one thing
        // there that is actually read closely. A tenth, floored so it does
        // not vanish and capped so it stays a caption.
        let read_h = (c.height() * 0.10).clamp(8.0, 13.0);
        let ladder = Rect::from_min_max(
            c.min,
            Pos2::new(c.right(), c.bottom() - read_h - 2.0),
        );
        if !ladder.is_positive() {
            return;
        }
        // **As wide as the column, but it starts where the BAR does.** The
        // reading is centred under the meter (see `draw_lufs`), and this is
        // also the shape of the CycleLufs target — so a press anywhere along
        // the reading cycles the window, and nothing to the LEFT of the bar
        // belongs to the loudness column at all. That last part is what lets
        // the pitch dial sit an even distance from the bar.
        self.lufs_readout = Rect::from_min_max(
            Pos2::new(c.left(), c.bottom() - read_h),
            c.max,
        );
        // Scale on the LEFT, ladder to its right: the numbers are read
        // against the bar beside them, and the master's own scale is
        // immediately to the right of this column — putting one here too
        // would set two columns of numbers back to back with nothing but
        // numbers between the two ladders.
        // **The meter, and its numbers. No words.**
        //
        // It carried the four named targets as words — CD, STREAM, PODCAST,
        // BROADCAST — down one side, which read well and put a strip of text
        // between the pitch dial and the thing the dial is supposed to sit an
        // even distance from. The owner's call was to drop them: with a
        // labelled tick every two LU the numbers say where you are, and the
        // targets are numbers anybody who cares already knows.
        //
        // The bar takes the column's own left edge, which is what makes the
        // dial's distance to it measurable at all.
        // The bar is the width it was ASKED for — the master meter's — and
        // the numbers take what is left beside it.
        //
        // **Capped against what the NUMBERS need, not against a fraction.**
        // It used to be `ladder.width() * 0.8`, which was slack while the
        // number cell was nearly three times the ink; once that cell was cut
        // to the ink the fraction started biting, and the loudness bar came
        // out narrower than the master's — breaking the one thing this column
        // is measured against. The bar may take everything the numbers and
        // their separation do not.
        let want = bar_w.max(2.0);
        let room = (ladder.width() - lufs_ticks_ink_w(want) - LUFS_TICK_GAP).max(2.0);
        let w = want.min(room);
        self.lufs_bar = Rect::from_min_max(
            ladder.min,
            Pos2::new(ladder.left() + w, ladder.bottom()),
        );
        self.lufs_ticks = Rect::from_min_max(
            Pos2::new(self.lufs_bar.right() + LUFS_TICK_GAP, ladder.top()),
            ladder.max,
        );
    }

    /// The varispeed's own column: the dial at the foot, its reading above.
    ///
    /// `foot` is the master knob — not a size but the rectangle itself,
    /// because the dial has to line up with it exactly. Two knobs at the
    /// same size but a few points apart vertically read as a mistake, and
    /// this is the one place in the band where two knobs of different kinds
    /// stand side by side.
    ///
    /// **The CELL reaches above the dial**, and that is the fix: `self.vari`
    /// used to be the dial's own rectangle, so `knob_face` carved its label
    /// strip out of a knob's height and the reading came out squeezed
    /// against the dial. A cell with a line of room over it is what every
    /// other knob in this band has.
    /// **The pitch dial's cell: SQUARE, anchored to `c`'s right edge.**
    ///
    /// The circle is centred in whatever cell it is given, so a cell wider
    /// than it is tall puts the drawn dial somewhere in the middle with
    /// margin either side — and then no caller can say where its edges are.
    /// A square cell makes the circle fill it, so `face.left()` and
    /// `face.right()` ARE the dial, which is what the four clusters at this
    /// end of the band are spaced against. See `CLUSTER_GAP`.
    ///
    /// Sized by `knob_face`'s OWN rule for the label strip rather than an
    /// approximation of it: guessing that height from the master knob's
    /// instead made the cell too narrow and the dial vanished at 650 points
    /// wide.
    ///
    /// A free function so the layout can ask where the dial WILL be before
    /// placing the VU group to its left — the group is trimmed to one gap
    /// from the face, and it cannot be trimmed to something not yet decided.
    fn vari_cell(c: Rect, foot: Rect) -> Rect {
        if !c.is_positive() || !foot.is_positive() {
            return Rect::NOTHING;
        }
        let (share, lo, hi) = KNOB_LABEL;
        // A line for the reading, on top of the dial's own height, so the
        // dial stays exactly the master knob's size and height.
        let label = (foot.height() * share / (1.0 - 2.0 * share)).clamp(lo, hi);
        let top = (foot.top() - label).max(c.top());
        let cell_h = foot.bottom() - top;
        let wanted = cell_h * share;
        let strip = if wanted >= lo { wanted.min(hi) } else { 0.0 };
        let w = (cell_h - strip * 2.0).max(1.0).min(c.width());
        Rect::from_min_max(
            Pos2::new(c.right() - w, top),
            Pos2::new(c.right(), foot.bottom()),
        )
    }

    /// The pitch dial's own column: the dial at the foot, its reading above.
    ///
    /// `foot` is the master knob — not a size but the rectangle itself,
    /// because the dial has to line up with it exactly. Two knobs at the
    /// same size but a few points apart vertically read as a mistake, and
    /// this is the one place in the band where two knobs of different kinds
    /// stand side by side.
    fn fill_vari(&mut self, cell: Rect) {
        if knob_fits(cell) {
            self.vari = cell;
        }
    }

    fn fill_transport(&mut self, t: Rect) {
        if !t.is_positive() {
            return;
        }
        // **Two columns, and the right one is stacked.** The record button,
        // the stop and the clock live at the foot of the words column; the
        // faders have the left of this group; and the right of it is the
        // meters with the three effect knobs in a row underneath.
        //
        // The knobs were a stack in a column ten points wide, squeezed between
        // the faders and the meters, and at that size the word over each one
        // did not fit and the dial itself was a smudge. Across the bottom of
        // the meter column each of them is a third of three hundred points
        // instead — the same three controls, three times the diameter.
        // **The master takes the right edge of the transport.** It is the end
        // of the signal path and it is where the eye already goes last, which
        // is the same reason it is on the right of every desk ever made.
        // 0.02 off the timeline's rule rather than 0.04: the VU faces are
        // sized by this column's width, and the margin was twice what the
        // rule between them needs.
        // **Clear of its rule, by the same margin the transport row keeps.**
        //
        // The rule between the timeline and the meters stands at
        // `FADER_COL + ROW_PAD`, and this column began at `FADER_COL + 0.02`
        // — the same place — so the VU faces sat right on the separator
        // while everything else in the band had air. One `ROW_PAD` past it,
        // like the row below.
        // The VU group takes what is left after the three clusters to its
        // right have been placed; `VARI_COL` is only its provisional end,
        // trimmed below once the pitch dial's face is known.
        let right = slice_h(t, FADER_COL + 0.02 + ROW_PAD, VARI_COL);
        // **Three columns at the right end, left to right: PITCH, LOUDNESS,
        // MASTER** — the owner's order, and the master hugs the band's own
        // edge.
        //
        // The pitch dial is FIRST because it is a control and the two things
        // to its right are readouts: the hand travels to it, the eye travels
        // past it. Loudness then sits directly beside the master meter it is
        // read against.
        //
        // It stopped at 0.985 to leave the heart a corner. The heart moved
        // to the floor (see `heart` in `fill_idle`), so the last fifteen
        // thousandths of the band are the master's, and the master is
        // THINNER than it was — 14% where it had 18% — because the loudness
        // ladder beside it is read against numbers and needed the room more.
        let vari_col = slice_h(t, VARI_COL + 0.005, LUFS_COL);
        let lufs_col = slice_h(t, LUFS_COL + 0.005, MASTER_COL);
        // **A hair short of the very edge.** The band keeps an inner margin
        // all the way round, and the status line's dismiss button lives in
        // the bottom-right corner of it: a master column taken to 1.0 put
        // its knob's cell half a point into that button at a 500-point band,
        // which the overlap guard caught. Still the last column, still
        // hugging — see `the_bands_right_end_is_three_columns...`.
        let master_col = slice_h(t, MASTER_COL + 0.005, 0.995);
        self.meter = slice_v(right, 0.0, METER_SHARE);
        let knobs = slice_v(right, METER_SHARE + 0.03, 1.0);
        // Side by side, a quarter each, with the gap inside the cell: the
        // label is centred over the dial and `draw_knob` centres the pair in
        // whatever it is handed.
        // **Two rows of FOUR.** Four sends on top — the three that add a room
        // and the phaser — then the four that shape what leaves: a high-pass,
        // a low-pass, the gain and the limiter that is always the last thing
        // in the chain. Eight across one row would put every knob below the
        // size `knob_fits` will accept at any window this app opens at.
        //
        // **Going from three to four costs the knobs nothing.** A knob's
        // diameter is set by the shorter side of its cell and these cells are
        // twice as wide as they are tall, so a quarter of the row is still
        // three times the width the circle needs — what shrinks is the margin
        // beside it. The words shrink to fit and LIMITER, the longest of the
        // eight, still lands well inside a quarter cell. See `knob_face` and
        // `fit_text`.
        // **The gap between the rows is bigger than it looks like it needs to
        // be**, because a knob is bigger than its cell: the tick marks stand
        // out to 1.26 of the face radius and are drawn OUTSIDE the rectangle
        // the face was measured in (see `draw_knob`). At a four percent gap
        // the word HPF landed on the reverb knob's skirt.
        self.refit_fx_grid(knobs);
        // **After the effect knobs, because it is measured against one.** The
        // master is the same control as the six beside it and has to be the
        // same size; taking a share of its own column instead would make it
        // half again as big, which reads as a different kind of thing.
        self.fill_master(master_col, self.fx[0].size());
        // **The right end is laid out RIGHT TO LEFT, with one gap.**
        //
        // Four clusters live past the timecode's rule — the VU faces and
        // their knobs, the pitch dial, the loudness meter, the master — and
        // the owner asked for the distance between each pair to be the same.
        // Sliced as four columns they were not: a column's edge is not where
        // its contents start, so the loudness column's left edge was a strip
        // of air and the pitch dial's cell was mostly margin. The eye
        // measures the DRAWN things.
        //
        // So the master is placed first (it owns the band's right edge),
        // then each cluster is set one `gap` to the left of the one after
        // it, sized to its own contents. The VU group takes whatever is
        // left, which is what it did before and what it can most afford.
        let gap = t.width() * CLUSTER_GAP;
        // **Only when the master is actually there.** `fill_master` refuses
        // a band too short for its knob and leaves its rects `NOTHING`,
        // whose width is negative infinity — and every measurement below is
        // taken off it.
        if !self.master_bars.is_positive() {
            self.fill_lufs_sized(lufs_col, lufs_col.width() * 0.58);
            self.fill_vari(Self::vari_cell(vari_col, self.master_knob));
            return;
        }
        // Measured off the master rather than stated twice, so the two cannot
        // drift — see `LUFS_BAR_OF_MASTER`.
        let bar_w = self.master_bars.width() * LUFS_BAR_OF_MASTER;
        // **The numbers take what NUMBERS need, not what the bar is wide.**
        //
        // This reserved nine tenths of the bar's width for a column of at most
        // two digits, and then another third of a gap of slack on top. None of
        // that is drawn, but all of it sits between the loudness meter and the
        // master's own scale — so the two ended up much further apart than the
        // even spacing they are laid out to, which is what the owner saw. The
        // gap between the clusters is unchanged; what has gone is the air
        // inside this one.
        // Exactly the numbers, their padding, and the step off the bar — see
        // `lufs_ticks_ink_w`. Anything more is dead air inside the cluster,
        // which is what put a pitch dial of space beside the master.
        let ticks_w = lufs_ticks_ink_w(bar_w) + LUFS_TICK_GAP;
        // **Measured to the master's first INK, not to its column's edge.**
        //
        // The master's left scale is RIGHT-aligned against its ladders (see
        // `draw_master`), so the numbers hug the bars and the left third of
        // that cell is empty air. Spacing the loudness cluster against the
        // cell's edge therefore put a gap of `gap` PLUS that air between the
        // two — about a pitch dial's width at 1300, which is exactly what
        // the owner kept seeing and what three earlier passes at this did not
        // fix, because every one of them measured rectangles that were
        // already evenly spaced.
        //
        // The four clusters are even when the gaps between what is DRAWN are
        // even. So this works out where the widest label actually starts, by
        // the same arithmetic `draw_master` sizes it with, and spaces against
        // that.
        let scale0 = self.master_scale[0];
        let scale_ink_left = {
            let step = self.master_bars.height() / 9.0;
            let size = fit_text(
                Rect::from_min_size(scale0.min, Vec2::new(scale0.width() - 3.0, step)),
                MASTER_SCALE_FIT,
                step * 0.86,
            );
            let used = master_scale_ink_w(size);
            (scale0.right() - 2.0 - used).max(scale0.left())
        };
        let lufs_right = scale_ink_left - gap;
        let lufs_col = Rect::from_min_max(
            Pos2::new(lufs_right - (bar_w + ticks_w), lufs_col.top()),
            Pos2::new(lufs_right, lufs_col.bottom()),
        );
        self.fill_lufs_sized(lufs_col, bar_w);
        // **The pitch dial, one gap left of the loudness BAR.**
        //
        // Its column reaches all the way back to the faders, and that is
        // deliberate: the dial takes a SQUARE cell anchored to this column's
        // right edge (see `fill_vari`), so the left edge only has to be far
        // enough away not to squeeze it. Pinning it to a fraction of the
        // band is what squeezed it to four points at 650 wide — the right
        // edge had moved and the left had not.
        let vari_col = Rect::from_min_max(
            Pos2::new(t.left(), vari_col.top()),
            Pos2::new(self.lufs_bar.left() - gap, vari_col.bottom()),
        );
        let cell = Self::vari_cell(vari_col, self.master_knob);
        // **The VU group is trimmed to one gap left of the dial's FACE, and
        // BEFORE the dial is committed.** Not of its cell: a cell is mostly
        // margin and the eye measures the circle. Before, because the grid
        // has to be re-laid inside the trimmed room and a control cannot be
        // spaced against something that has not been decided yet.
        let mut cell = cell;
        let mut trimmed_ok = false;
        if let Some(f) = knob_face(cell) {
            let want = f.face.left() - gap;
            if want > right.left() + 8.0 {
                let trimmed = Rect::from_min_max(right.min, Pos2::new(want, right.bottom()));
                if self.refit_fx_grid(slice_v(trimmed, METER_SHARE + 0.03, 1.0)) {
                    self.meter = slice_v(trimmed, 0.0, METER_SHARE);
                    trimmed_ok = true;
                }
            }
        }
        if !trimmed_ok {
            // **The band is at its floor and the grid cannot give.** Eight
            // knobs that do not fit are eight knobs nobody can use, and the
            // dial is one control: it takes what is left beside the group
            // rather than overlapping it. Below that it is refused
            // outright, which is what `knob_fits` is for.
            let left = right.right() + gap;
            cell = Rect::from_min_max(
                Pos2::new(cell.left().max(left), cell.top()),
                cell.max,
            );
        }
        self.fill_vari(cell);
        self.fill_vari_label();
        self.centre_lufs_between_the_word_and_the_master();
    }

    /// **Put the loudness meter midway between the PITCH CONT text and the
    /// master's own numbers.**
    ///
    /// It was spaced one cluster-gap off the master and the dial was spaced
    /// off IT, which is even by rectangle and not by eye: the word is wider
    /// than the circle under it, so the space to the word's right was smaller
    /// than the space to the master, and the meter sat left of where it looks
    /// like it belongs. Measured from the WORD's right edge, which is the
    /// last ink on that side.
    ///
    /// Run after the dial is placed, because it depends on where the dial
    /// ended up: the dial is trimmed against the knob group and cannot be
    /// predicted from a fraction.
    fn centre_lufs_between_the_word_and_the_master(&mut self) {
        if !self.lufs_bar.is_positive() || !self.master_bars.is_positive() {
            return;
        }
        let strip =
            (knob_face(self.vari).map_or(0.0, |k| k.label_h) * 2.0).max(KNOB_LABEL.1);
        // The left bound: the word if there is one, else the dial's own face.
        let left = match vari_word_ink(self.vari, self.vari_label, strip) {
            Some(w) => w.right(),
            None => knob_face(self.vari).map_or(self.vari.right(), |f| f.face.right()),
        };
        let step = self.master_bars.height() / 9.0;
        let size = fit_text(
            Rect::from_min_size(
                self.master_scale[0].min,
                Vec2::new(self.master_scale[0].width() - 3.0, step),
            ),
            MASTER_SCALE_FIT,
            step * 0.86,
        );
        let right = (self.master_scale[0].right() - 2.0 - master_scale_ink_w(size))
            .max(self.master_scale[0].left());
        let cluster = self.lufs_ticks.right() - self.lufs_bar.left();
        if !(right - left).is_finite() || right - left <= cluster {
            return;
        }
        let shift = left + (right - left - cluster) * 0.5 - self.lufs_bar.left();
        if !shift.is_finite() || shift.abs() < 0.5 {
            return;
        }
        for r in [
            &mut self.lufs_bar,
            &mut self.lufs_ticks,
            &mut self.lufs_readout,
        ] {
            if r.is_positive() {
                *r = r.translate(Vec2::new(shift, 0.0));
            }
        }
    }

    /// **The word's own strip, reaching into the air either side of the dial.**
    ///
    /// Set after everything around the dial has been placed, because it is
    /// bounded by the neighbours and not by a fraction: the knob group on the
    /// left, the loudness bar on the right. A hair short of each, so the word
    /// can grow until it is ABOUT to touch them and no further — which is the
    /// size the owner asked for.
    fn fill_vari_label(&mut self) {
        if !self.vari.is_positive() {
            self.vari_label = Rect::NOTHING;
            return;
        }
        // One point of daylight, so "about to be disruptive" stays "about".
        const CLEAR: f32 = 1.0;
        let left = if self.meter.is_positive() {
            self.meter.right() + CLEAR
        } else {
            self.vari.left()
        };
        let right = if self.lufs_bar.is_positive() {
            self.lufs_bar.left() - CLEAR
        } else {
            self.vari.right()
        };
        // Never NARROWER than the dial's own cell — a band too tight for the
        // air is still a band where the word may use its own column.
        let left = left.min(self.vari.left());
        let right = right.max(self.vari.right());
        self.vari_label = Rect::from_min_max(
            Pos2::new(left, self.vari.top()),
            Pos2::new(right, self.vari.bottom()),
        );
    }

    /// Rolling: the at-rest layout, readable from the bench. The preview keeps
    /// its size and the transport row its places (a realtime export takes
    /// record, play, pause and the tempo away), and the take's readout - the
    /// count-in beat, the clock - goes onto the LCD's scope glass.
    fn fill_rolling(&mut self, body: Rect, gap: f32, view: &RecorderView<'_>) {
        // **The same preview, at the same size**, placed in `new` for both
        // layouts. It used to collapse to a strip when a take started, back
        // when it was a framing check. It is the camera inset the recording
        // will carry now, so the moment a take starts is the moment it matters
        // most — and the band does not rearrange itself under the eye either
        // way.

        // **The at-rest middle, not the one the narrower preview leaves.** The
        // preview collapses when a take starts; the transport group may not
        // follow it left, or the meters slide under the faders — which are
        // pinned to the at-rest geometry and do not move — and the band
        // reshuffles at the one moment nobody can afford to look at it.
        let t = Self::middle_of(body, gap);
        if !t.is_positive() {
            return;
        }
        // The faders survive a take for the same reason they always did: the
        // click is turned down mid-performance more than any other control
        // here. They keep the bottom of the group, where they are at rest.
        // **Transport first, faders second**, the same way round as at rest.
        // The tempo knob is sized from the effect knobs, and those are placed
        // in `fill_transport` — so asking for them the other way round would
        // measure against a rectangle that is still `NOTHING`.
        self.fill_transport(t);
        self.fill_faders(body, gap, true);
        // **Except under a realtime EXPORT**, which is the one take that owns
        // the transport: it is playing the song start to end into a file, and
        // the host refuses a record, a play or a pause into it. Stop stays -
        // it ends the export early, on purpose - and so does the cassette.
        if view.exporting {
            self.record = Rect::NOTHING;
            self.play = Rect::NOTHING;
            self.pause = Rect::NOTHING;
            // And the tempo: a tempo move retimes every MIDI clip mid-render.
            self.tempo = Rect::NOTHING;
            self.tempo_tap = Rect::NOTHING;
        }

        // **The take's readout, on the LCD's scope glass.** It used to take
        // the tempo knob's slot in the transport row - six and a half points
        // tall at the 900-point window, a number nobody could read from a
        // bench, and the price was a control the owner wants in the row
        // mid-take. The glass is the one free surface big enough: the count-in
        // digit and the take's clock are drawn there at forty to seventy
        // points, over the oscilloscope, for as long as the take runs. The
        // glass is not a target and the LCD's drag cells are all to its left,
        // so nothing that can be pressed moves or is covered.
        self.timecode = lcd_scope_rect(self.lcd);

        // The one thing `hide_elapsed` suppresses is a CLOCK - and with it
        // off, the glass keeps its scope for the whole take, which is what a
        // breakdown video may want. The count-in beat
        // is the number the player is counting, and "FINISHING" is the reason
        // not to close the lid yet; hiding either would be hiding the wrong
        // thing under the name of a performance setting.
        if view.hide_elapsed
            && matches!(view.state, RecordState::Rolling)
            && view.count_beat.is_none()
        {
            self.timecode = Rect::NOTHING;
        }
        // **A glass too small for the digits keeps its scope.** The readout
        // bails below `MIN_TEXT`; the glass it would have taken must not go
        // blank for a number nobody draws.
        if self.timecode.is_positive() {
            let state = readout_state(view);
            let text = readout_text(state, view.position_s, view.elapsed_s);
            if readout_size(self.timecode, state, &text) < MIN_TEXT {
                self.timecode = Rect::NOTHING;
            }
        }
    }

    /// Where one effect knob's cell is.
    ///
    /// A method rather than `self.fx[n]` at each call site — see the note in
    /// `targets`, where eight literal indices went stale the moment the row
    /// gained a knob.
    fn cell(&self, fx: Fx) -> Rect {
        self.fx.get(fx.index()).copied().unwrap_or(Rect::NOTHING)
    }

    /// Every clickable region and what it means, in one place.
    ///
    /// [`hit_test`] reads this and so does the test that proves no two of them
    /// overlap, so a control that moves onto another one fails a test rather
    /// than quietly swallowing its clicks.
    fn targets(&self) -> [(Rect, Produces); 38] {
        use Produces::{AlongV, Fixed};
        // The word over a knob, which is the same thing for a dial.
        // **Nothing when there is no word.** Below a certain height the label
        // is not drawn at all (see `knob_face`), and a zero-height target is
        // still a target as far as an overlap check is concerned.
        // A GAP under it, the way every other pair of targets in this layout
        // is separated: two rectangles that share an edge are two rectangles a
        // press on that edge can land in either of.
        // **The READOUT's own strip, at the FOOT of the cell.**
        //
        // It sat at the top, which was right while the reading was up there.
        // When the name and the reading swapped places this stayed where it
        // was, so "click to type a number" was the label's target and the
        // number itself took nothing: a dial you could no longer type into,
        // and a name that opened a number field.
        let cap = |cell: Rect| {
            knob_face(cell)
                .filter(|f| f.label_h > 0.0)
                .map_or(Rect::NOTHING, |f| {
                    Rect::from_min_max(
                        Pos2::new(cell.left(), cell.bottom() - f.label_h * 0.88),
                        cell.max,
                    )
                })
        };
        // **What is left of the cell once the READING is taken out of it.**
        // The whole cell used to be the drag target, on the reasoning that a
        // knob you can only grab by its own diameter is one you keep missing —
        // still true of everything above the number. The number is a target of
        // its own, so the two cannot both have it.
        let dial = |cell: Rect| {
            knob_face(cell).map_or(Rect::NOTHING, |f| {
                Rect::from_min_max(
                    cell.min,
                    Pos2::new(cell.right(), cell.bottom() - f.label_h),
                )
            })
        };
        // **The numbers come FIRST.** A label sits inside its knob's cell;
        // it is scanned before the control it belongs to, so the smaller,
        // more specific target wins.
        [
            (cap(self.tempo), Fixed(Hit::Type(NumField::Tempo))),
            (self.tempo_tap, Fixed(Hit::TapTempo)),
            (cap(self.master_knob), Fixed(Hit::Type(NumField::Master))),
            // **Indexed through `Fx::index()`, never by a literal.** These
            // were written as `self.fx[0]` … `self.fx[5]` against the order
            // the six knobs happened to be in; adding the phaser at position
            // three silently handed the PHASER's rectangle to the HPF and
            // every knob after it shifted by one. Nothing overlapped and
            // nothing was unreachable, so only a test that pressed each
            // knob's own cell could see it. Asking the enum where it sits
            // makes the pairing structural.
            (cap(self.cell(Fx::Reverb)), Fixed(Hit::Type(NumField::Fx(Fx::Reverb)))),
            (cap(self.cell(Fx::Delay)), Fixed(Hit::Type(NumField::Fx(Fx::Delay)))),
            (cap(self.cell(Fx::Chorus)), Fixed(Hit::Type(NumField::Fx(Fx::Chorus)))),
            (cap(self.cell(Fx::Phaser)), Fixed(Hit::Type(NumField::Fx(Fx::Phaser)))),
            (cap(self.cell(Fx::Hpf)), Fixed(Hit::Type(NumField::Fx(Fx::Hpf)))),
            (cap(self.cell(Fx::Lpf)), Fixed(Hit::Type(NumField::Fx(Fx::Lpf)))),
            (cap(self.cell(Fx::Gain)), Fixed(Hit::Type(NumField::Fx(Fx::Gain)))),
            (cap(self.cell(Fx::Limiter)), Fixed(Hit::Type(NumField::Fx(Fx::Limiter)))),
            // **The picture is the picker.** Not while a take is rolling: the
            // camera cannot be changed mid-take, and the dialog it opens is
            // modal over the one gesture that matters then, which is Stop.
            (
                if self.rolling { Rect::NOTHING } else { self.preview },
                Fixed(Hit::PickCamera),
            ),
            // **The clip latch is cleared by pressing the meter it is shown
            // on.** There was a word `CLIPPED` in the status line to press
            // instead; the lamp on the VU face says the same thing in the
            // place somebody is already looking, and two indicators for one
            // fact is one of them being wrong.
            (self.meter, Fixed(Hit::DismissClip)),
            (self.play, Fixed(Hit::Play)),
            (self.record, Fixed(Hit::Record)),
            (self.stop, Fixed(Hit::Stop)),
            (self.capture, Fixed(Hit::Capture)),
            (self.pause, Fixed(Hit::Pause)),
            (self.project, Fixed(Hit::ProjectMenu)),
            (self.setup, Fixed(Hit::OpenSetup)),
            // Up the cell for more, which is the direction every knob in every
            // studio turns and the direction the two faders beside them move.
            (dial(self.master_knob), AlongV(Hit::SetMaster)),
            // Up for faster, which is the direction every dial on this panel
            // turns for more — and, on a tape machine, the direction the
            // pitch goes.
            //
            // `dial(cell)` here like every other knob: the circle is drawn
            // inside exactly this region — see `vari_face`, which was what
            // had drifted below it.
            (dial(self.vari), AlongV(Hit::SetVarispeed)),
            // **The whole ladder, and the number under it.** A loudness
            // reading is read, not aimed at: the target that cycles the
            // window has to be the thing somebody is already looking at, and
            // a thirty-point-wide bar is not something to ask a hand to hit
            // twice.
            (self.lufs_bar.union(self.lufs_readout), Fixed(Hit::CycleLufs)),
            (dial(self.cell(Fx::Reverb)), AlongV(|v| Hit::SetFx(Fx::Reverb, v))),
            (dial(self.cell(Fx::Delay)), AlongV(|v| Hit::SetFx(Fx::Delay, v))),
            (dial(self.cell(Fx::Chorus)), AlongV(|v| Hit::SetFx(Fx::Chorus, v))),
            (dial(self.cell(Fx::Phaser)), AlongV(|v| Hit::SetFx(Fx::Phaser, v))),
            (dial(self.cell(Fx::Hpf)), AlongV(|v| Hit::SetFx(Fx::Hpf, v))),
            (dial(self.cell(Fx::Lpf)), AlongV(|v| Hit::SetFx(Fx::Lpf, v))),
            (dial(self.cell(Fx::Gain)), AlongV(|v| Hit::SetFx(Fx::Gain, v))),
            (dial(self.cell(Fx::Limiter)), AlongV(|v| Hit::SetFx(Fx::Limiter, v))),
            (self.dest, Fixed(Hit::ChooseFolder)),
            (self.reveal, Fixed(Hit::RevealFolder)),
            (self.open_when_done, Fixed(Hit::ToggleOpenWhenDone)),
            (self.count_in, Fixed(Hit::CycleCountIn)),
            // Turned like the sends, and typed into on a DOUBLE click — see
            // `draw_tempo`. `SetTempo` carries beats rather than 0..=1, so the
            // producer converts on the way out.
            // The tempo dial yields the chip's column — the panel's
            // no-overlap rule is load-bearing, and the knob's circle
            // never lived in that slack anyway.
            (
                if self.tempo_tap.is_positive() {
                    let d = dial(self.tempo);
                    Rect::from_min_max(
                        d.min,
                        Pos2::new(self.tempo_tap.left() - 2.0, d.bottom()),
                    )
                } else {
                    dial(self.tempo)
                },
                AlongV(|t| Hit::SetTempo(knob_to_tempo(t))),
            ),
            (self.time_sig, Fixed(Hit::EditTimeSignature)),
            (self.export, Fixed(Hit::Export)),
        ]
    }
}

// ── the preview ────────────────────────────────────────────────────────────

/// The whole texture, every time. See [`fit_preview`].
const FULL_FRAME: Rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));

/// Where a camera frame of `source_px` pixels goes inside the box `into`.
///
/// **Letterboxed, not cropped**, and the destination rect shrinks rather than
/// the `uv` — `Painter::image` takes both, and using `uv` to fill the box would
/// crop the sensor frame. Cropping hides what the camera is actually going to
/// record, and framing a piano shot is precisely the job of seeing the edges.
/// So the uv stays [`FULL_FRAME`] and the bars are painted in the band's own
/// background colour.
///
/// The result is centred and always inside `into`, which is the property that
/// keeps the camera's aspect out of the layout: whatever shape the frame is,
/// the box it lands in was decided by [`band_height`] alone.
///
/// Nothing to draw — no box, or a source with no pixels — is
/// [`Rect::NOTHING`] rather than a division by zero.
pub fn fit_preview(into: Rect, source_px: Vec2) -> Rect {
    if !into.is_positive() || source_px.x <= 0.0 || source_px.y <= 0.0 {
        return Rect::NOTHING;
    }
    let source = source_px.x / source_px.y;
    let size = if source > into.width() / into.height() {
        Vec2::new(into.width(), into.width() / source)
    } else {
        Vec2::new(into.height() * source, into.height())
    };
    Rect::from_center_size(into.center(), size)
}

// ── what a click means ─────────────────────────────────────────────────────

/// What is under a click. The app turns one of these into a request and
/// performs it after the frame; nothing here does anything.
///
/// `PartialEq` but **not** `Eq`, because four of the variants carry a float.
/// Comparing two of those for equality is nearly always the wrong question
/// anyway — "is this the same control I was already dragging?" is the right
/// one, and that is [`Hit::is_same_control`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    /// The cassette: start or stop a CAPTURE of the whole session — the
    /// take machinery, apart from the arrangement. See
    /// `RecorderRequest::Capture`.
    Capture,
    /// The folder: the PROJECT menu — new, open, save, save as, template.
    ProjectMenu,
    /// PAUSE: the playhead holds where it stands. Stop goes home.
    Pause,
    /// Open the menu the take's settings moved into.
    OpenSetup,
    /// The take-settings popup's DONE button. It only ever closes the popup,
    /// which is why it is a hit here and not an action anywhere else.
    CloseSetup,
    /// The × on the status row: put the message away.
    ///
    /// **A message with no way out has to be right about everything.** The
    /// video path's "frames were dropped" line was cleared by nothing at all —
    /// it sat on the owner's screen for eight minutes and across several takes
    /// — and it could not be pressed, because it was painted rather than laid
    /// out.
    /// Reachable only from the popup: the four rows below have no home in the
    /// band, and a control that exists in exactly one place needs no rule about
    /// which place wins.
    ShowAudioStatus,
    /// The AUDIO tier's door to the plugin-folder picker — the menu's
    /// "Add a Folder...", one click from the cog.
    PluginFolders,
    /// Scan the plugin folders again, without adding one.
    ///
    /// Its own control because the two are different questions: a folder is
    /// added once and a rescan is wanted every time a plugin is installed.
    /// The menu has carried a "Rescan for Plugins" row all along; the card
    /// carried no way to do it at all.
    RescanPlugins,
    /// The cog's STREAM door: the ReaStream send settings, in the AUDIO tier
    /// by the owner's word ("move it to the settings cog in audio section").
    StreamToObs,
    ToggleCountInInTake,
    ToggleHideElapsed,
    /// Hear the live input through the monitors. **Session-only, never
    /// persisted** — see `TangentApp::input_monitor`: the failure mode is a
    /// room feeding back the moment the app launches. It lived on a
    /// right-click of the band's microphone icon; the icon left with the
    /// input fader when the desk took the inputs, and this panel is where
    /// the switch survives, beside the click's own in-take tick.
    ToggleInputMonitor,
    /// The green button: audition from the playhead without making a take.
    Play,
    Record,
    Stop,
    ChooseFolder,
    /// The PROJECT tier's own folder: where documents are written. Its own
    /// setting because projects stopped being a subfolder of the takes —
    /// see `Settings::project_root`.
    ChooseProjectFolder,
    RevealProjectFolder,
    /// And where an export lands when the export dialog names no folder.
    ChooseExportFolder,
    RevealExportFolder,
    /// And where saved PRESETS live — kits, built-in effect settings, Keys
    /// patches. The MEDIA tier's fourth row.
    ChoosePresetFolder,
    RevealPresetFolder,
    /// Show the destination folder in the file manager.
    RevealFolder,
    /// Show the take's folder as soon as the take is finished.
    ToggleOpenWhenDone,
    NameField,
    PickCamera,
    PickAudio,
    /// Step the count-in through `COUNT_IN_CHOICES` - off, 1, 2, 4 BARS - and
    /// back round to off. A length off that list (the metronome panel's 3 or
    /// 5-8 bars) steps to the next choice above it: 3 to 4, and 5-8 round to
    /// off. Stepped by [`next_count_in`], in `TangentApp::apply_recorder_hit`.
    CycleCountIn,
    Export,
    /// Whether the click is also recorded into the file. Lives in the
    /// take-settings popup; the click itself is a channel on the desk now.
    ToggleMetronomeInTake,
    /// Open a field for typing, and nothing else.
    ///
    /// **A gesture of its own, on a target of its own.** Dragging, resetting
    /// and typing are three things a control has to do and a press has only so
    /// many meanings: the drag is the handle, the double click is the reset,
    /// and typing is the NUMBER — the dB at the end of a fader, the word over
    /// a knob. Pressing what the value is written on to change it is the one
    /// mapping nobody has to be told.
    Type(NumField),
    /// Choose an audio file to play along to. The empty timeline is the
    /// button: a box that says what a click will do, where the file will show.
    /// Put the playhead HERE — how far along the timeline the press landed,
    /// 0..=1 of the file's length. A click on the waveform is a locate: where
    /// you point is where play auditions from and where record starts
    /// writing. The app turns the fraction into seconds, because the file's
    /// length is the host's fact, not the painter's.
    SeekTimeline(f32),
    /// Unload the backing file: the × in the timeline's corner. The BACKING
    /// strip that used to carry this gesture retired at v13.
    ClearTrack,
    /// Put every clip latch out. **The VU face is the target.**
    ///
    /// A latch that clears itself is one the performer never sees, because
    /// they were looking at their hands. A latch with no way to clear it is
    /// one that stays lit for the rest of the session and stops meaning
    /// anything — the same failure, slower. It was a word in the status line
    /// and is the meter now: one indicator, pressed where it is shown.
    DismissClip,
    /// The master, as a FADER POSITION 0..=1 — not a gain. Same curve as the
    /// four faders, because it is one; it just wears a knob.
    SetMaster(f32),
    /// One of the six effect knobs, 0..=1. **One variant, not six**: they are
    /// the same control six times over, and six parallel variants meant six
    /// parallel arms in every match that ever touches one.
    SetFx(Fx, f32),
    /// Tempo dragged, already clamped to `MIN_BPM..=MAX_BPM`.
    SetTempo(f64),
    /// The little TAP chip beside the tempo knob: each press is one beat
    /// of the player's own count, and the tempo follows the run of them.
    TapTempo,
    /// Type a time signature into the band. A CLICK opens it, because unlike
    /// every other numeric cell there is nothing to drag: "6/8" is two numbers
    /// and a slash, not a point on a continuum.
    EditTimeSignature,
    /// The varispeed dial, as a POSITION 0..=1 with 0.5 at twelve o'clock.
    ///
    /// Its own variant rather than a ninth `SetFx`, because it is not an
    /// effect: nothing about it goes through the effects chain, its resting
    /// place is the middle of its travel rather than an end, and it is the
    /// only control in the band that changes what the master's SAMPLE RATE
    /// means. See `crate::varispeed`.
    SetVarispeed(f32),
    /// The loudness ladder: step to the next window — momentary, short-term,
    /// integrated. A click rather than a menu, because there are three of
    /// them and they have an order.
    CycleLufs,
    /// Put the integrated reading back to nothing and start measuring again.
    ///
    /// **Only the integrated one has this and only the integrated one needs
    /// it**: momentary and short-term are windows that forget by themselves,
    /// and an integrated reading is a gated mean over everything since it was
    /// last cleared — which, without this, is everything since the app
    /// opened, including the twenty minutes of silence before you pressed
    /// play.
    ResetLufs,
    /// The MEDIA tier's fifth folder row: where captured impulse responses
    /// live. Appended at the enum's END, like every variant this enum
    /// gains across parallel lanes.
    ChooseIrFolder,
    RevealIrFolder,
}

impl Hit {
    /// The value the four dragged variants carry in [`Hit::ALL`].
    ///
    /// Halfway along, which is a real point on every one of their travels and
    /// not an end that might be special. Nothing compares against it: the
    /// reachability test asks [`Hit::is_same_control`], because the whole point
    /// of a dragged control is that the value depends on where you pressed.
    const MIDWAY: f32 = 0.5;

    /// The same control, carrying `v` instead.
    ///
    /// For a caller that computed the value itself rather than reading it off
    /// a position — which is what a relative drag does. Anything that carries
    /// no 0..=1 value comes back unchanged.
    #[must_use]
    pub fn with_value(self, v: f32) -> Hit {
        let v = v.clamp(0.0, 1.0);
        match self {
            Hit::SetFx(fx, _) => Hit::SetFx(fx, v),
            Hit::SetMaster(_) => Hit::SetMaster(v),
            Hit::SetVarispeed(_) => Hit::SetVarispeed(v),
            // **The tempo, which was missing.** Every other knob's drag goes
            // through here to turn "how far the hand has moved" into a value;
            // without an arm the tempo fell to `other => other` and every
            // frame of a drag re-applied the hit the PRESS produced — which is
            // absolute, so the knob jumped to wherever it was first touched
            // and then would not move. It has never been relatively draggable;
            // the other seven becoming consistent is what made it obvious.
            Hit::SetTempo(_) => Hit::SetTempo(knob_to_tempo(v)),
            other => other,
        }
    }

    /// Every control, which is what the reachability test iterates. The
    /// exhaustive match in [`Hit::label`] is what makes adding a variant
    /// without adding it here a compile error rather than an untested control.
    ///
    /// The four per-slot controls appear once per slot, because "reachable" is
    /// a question about slot 2 that slot 0 cannot answer for it.
    pub const ALL: [Hit; 41] = [
        Hit::Play,
        Hit::Record,
        Hit::Stop,
        Hit::Pause,
        Hit::Capture,
        Hit::ProjectMenu,
        Hit::StreamToObs,
        Hit::OpenSetup,
        Hit::ChooseFolder,
        Hit::RevealFolder,
        Hit::ChooseProjectFolder,
        Hit::RevealProjectFolder,
        Hit::ChooseExportFolder,
        Hit::RevealExportFolder,
        Hit::ChoosePresetFolder,
        Hit::RevealPresetFolder,
        Hit::ToggleOpenWhenDone,
        Hit::NameField,
        Hit::PickCamera,
        Hit::PickAudio,
        Hit::CycleCountIn,
        Hit::Export,
        Hit::ToggleMetronomeInTake,
        Hit::SetMaster(Hit::MIDWAY),
        Hit::Type(NumField::Tempo),
        Hit::DismissClip,
        Hit::SetFx(Fx::Reverb, Hit::MIDWAY),
        Hit::SetFx(Fx::Delay, Hit::MIDWAY),
        Hit::SetFx(Fx::Chorus, Hit::MIDWAY),
        Hit::SetFx(Fx::Phaser, Hit::MIDWAY),
        Hit::SetFx(Fx::Hpf, Hit::MIDWAY),
        Hit::SetFx(Fx::Lpf, Hit::MIDWAY),
        Hit::SetFx(Fx::Gain, Hit::MIDWAY),
        Hit::SetFx(Fx::Limiter, Hit::MIDWAY),
        Hit::SetTempo((MIN_BPM + MAX_BPM) * 0.5),
        Hit::EditTimeSignature,
        Hit::SetVarispeed(Hit::MIDWAY),
        Hit::CycleLufs,
        Hit::ResetLufs,
        Hit::ChooseIrFolder,
        Hit::RevealIrFolder,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Hit::Record => "Record armed channels into the arrangement",
            Hit::Capture => "Capture the session — video, audio, MIDI — apart from the arrangement",
            Hit::ProjectMenu => "Project — new, open, save, save as, template",
            Hit::Pause => "Pause — hold the playhead here; Stop goes home",
            Hit::OpenSetup => "Take settings",
            Hit::CloseSetup => "Done",
            Hit::ShowAudioStatus => "Setup: the audio system, devices, rate and buffer",
            Hit::PluginFolders => "Add a VST3 folder - Tangent rescans on the pick",
            Hit::RescanPlugins => "Look through the plugin folders again",
            Hit::StreamToObs => "Stream the master out to OBS over ReaStream",
            Hit::ToggleCountInInTake => "Record the count-in into the take",
            Hit::ToggleHideElapsed => "Hide the elapsed time",
            Hit::ToggleInputMonitor => "Hear the live input",
            Hit::Play => "Play from the playhead, without recording",
            Hit::Stop => "Stop",
            Hit::ChooseFolder => "Choose folder",
            Hit::RevealFolder => "Show the folder",
            Hit::ChooseProjectFolder => "Choose the project folder",
            Hit::RevealProjectFolder => "Show the project folder",
            Hit::ChooseExportFolder => "Choose the export folder",
            Hit::RevealExportFolder => "Show the export folder",
            Hit::ChoosePresetFolder => "Choose the preset folder",
            Hit::RevealPresetFolder => "Show the preset folder",
            Hit::ChooseIrFolder => "Choose the impulse-response folder",
            Hit::RevealIrFolder => "Show the impulse-response folder",
            Hit::ToggleOpenWhenDone => "Show the take when it is finished",
            Hit::NameField => "Take name",
            Hit::PickCamera => "Camera",
            Hit::PickAudio => "Audio input",
            Hit::CycleCountIn => "Count-in",
            Hit::Export => "Export",
            Hit::ToggleMetronomeInTake => "Record the click into takes",
            Hit::SetFx(fx, _) => fx.describe(),
            Hit::SetMaster(_) => "Master  -  right-click to type, double-click for 0 dB",
            Hit::DismissClip => "Levels  -  click to clear the clip lamp",
            Hit::Type(_) => "Click to type a number",
            Hit::SeekTimeline(_) => "Backing track  -  click to set the playhead",
            Hit::ClearTrack => "Backing track  -  remove the file",
            Hit::SetTempo(_) => "Tempo",
            Hit::TapTempo => "Tap the tempo - each press is one beat",
            Hit::EditTimeSignature => "Time signature",
            Hit::SetVarispeed(_) => {
                "Varispeed  -  pitch follows speed; centre is off, right-click for the range"
            }
            Hit::CycleLufs => "Loudness  -  click for momentary, short-term, integrated",
            Hit::ResetLufs => "Loudness  -  right-click to start the integrated reading again",
        }
    }

    /// Whether this control is DRAGGED rather than clicked.
    ///
    /// Exactly the value-carrying variants: they are the ones whose value is a
    /// function of where the pointer is, so they are the ones the app has to
    /// keep following while the button is held. A button has nothing to follow.
    pub fn is_draggable(&self) -> bool {
        matches!(
            self,
            Hit::SetFx(..) | Hit::SetMaster(_) | Hit::SetTempo(_) | Hit::SetVarispeed(_)
        )
    }

    /// Whether this is one of the eight knobs.
    ///
    /// They share a gesture set the faders do not: a right-click types a
    /// number into one, a double-click puts it back to what it ships as, and a
    /// plain tap does NOTHING — because the first click of a double-click is a
    /// tap, and a control that opened a text box under every double-click
    /// could never be reset by one.
    pub fn is_knob(self) -> bool {
        matches!(
            self,
            Hit::SetFx(..) | Hit::SetTempo(_) | Hit::SetMaster(_) | Hit::SetVarispeed(_)
        )
    }

    /// What a double-click puts this control back to.
    ///
    /// **Nothing applied, for everything that applies something.** All six
    /// effects go to zero, which for the filters is a corner out at the edge
    /// of hearing and for the limiter is a threshold of 0 dB — in every case
    /// the position where the knob is not doing anything.
    ///
    /// Everything that is a LEVEL goes to what it ships as instead, which is
    /// unity for the four that carry sound and -6 dB for the click. "Nothing
    /// applied" for a fader would be silence, and a reset that mutes the thing
    /// you were listening to is a reset nobody uses twice.
    pub fn reset_to(self) -> Option<Hit> {
        let ships_at = |g: f32| gain_to_fader(g);
        let d = crate::recorder::Gains::default();
        Some(match self {
            // **The limiter resets to where it SHIPS, not to "not
            // applied".**
            //
            // Fully clockwise is a threshold above everything, catching
            // nothing — which was both "not applied" and the shipped state
            // while those were the same thing. They are not any more: the
            // limiter ships in circuit at −1 dBFS as a backstop under an
            // echo that can self-oscillate. A double-click that bypassed it
            // would put the guard rail back in the drawer, silently, as part
            // of a gesture people use to tidy up. One constant answers for
            // both — see `recorder::LIMITER_SHIPS_AT`.
            Hit::SetFx(Fx::Limiter, _) => {
                Hit::SetFx(Fx::Limiter, crate::recorder::LIMITER_SHIPS_AT)
            }
            // The LPF's knob is a cutoff, so "not applied" is fully OPEN —
            // 20 kHz, letting everything through; resetting it to zero is a
            // filter slammed shut, which is the opposite of a reset.
            Hit::SetFx(Fx::Lpf, _) => Hit::SetFx(Fx::Lpf, 1.0),
            Hit::SetFx(fx, _) => Hit::SetFx(fx, 0.0),
            Hit::SetTempo(_) => Hit::SetTempo(DEFAULT_BPM),
            Hit::SetMaster(_) => Hit::SetMaster(ships_at(d.master)),
            // Dead centre, which is the one place this dial is OFF. It is
            // also the only knob here whose "not applied" is the MIDDLE of
            // its travel rather than an end — see `crate::varispeed`.
            Hit::SetVarispeed(_) => Hit::SetVarispeed(crate::varispeed::Varispeed::CENTRE),
            _ => return None,
        })
    }

    /// Which control this is, for [`Hit::is_same_control`]: the variant, plus
    /// the index for the ones that carry one.
    ///
    /// **The index is half the answer.** `mem::discriminant` alone says
    /// `SetSlotGain(0, _)` and `SetSlotGain(1, _)` are the same control, and a
    /// caller that believes it is still dragging the knob it grabbed would then
    /// set slot 1's level from a drag that started on slot 0's knob — silently,
    /// and only once the pointer wandered a row.
    ///
    /// `SetFx` is the same trap and it arrived the same way: six knobs that
    /// used to be six variants became one variant carrying which, and a
    /// discriminant cannot see the difference between the reverb and the
    /// limiter. Grabbing REVERB and dragging down onto HPF would have set the
    /// high-pass.
    fn control_key(self) -> (std::mem::Discriminant<Hit>, usize) {
        let index = match self {
            Hit::SetFx(fx, _) => fx.index(),
            // Two `Type`s are two controls: the dB at the end of slot 1 and
            // the dB at the end of slot 2 are not the same box.
            Hit::Type(f) => num_field_key(f),
            _ => 0,
        };
        (std::mem::discriminant(&self), index)
    }

    /// Whether two hits are the same CONTROL, ignoring any value one carries.
    ///
    /// `SetTempo(90.0)` and `SetTempo(140.0)` are one control being dragged,
    /// which is the question a caller holding the mouse button down actually
    /// has, and the question `==` cannot answer. `SetFx(Fx::Reverb, 0.2)` and
    /// `SetFx(Fx::Hpf, 0.2)` are two different controls at the same position,
    /// which is the question `==` gets right and a discriminant gets wrong.
    pub fn is_same_control(self, other: Hit) -> bool {
        self.control_key() == other.control_key()
    }
}

/// A number for each field, so `control_key` can tell two `Type`s apart.
fn num_field_key(f: NumField) -> usize {
    match f {
        NumField::Master => 102,
        NumField::Tempo => 104,
        NumField::Meter => 105,
        NumField::Varispeed => 106,
        NumField::VariMin => 107,
        NumField::VariMax => 108,
        // Offset well past the band's own fields: there can be one of these
        // per bar, and two controls that answer `true` to `is_same_control`
        // are two controls a drag can slide between.
        NumField::MeterAt(bar) => 1_000 + bar as usize,
        // Past every bar a song can have, so a flag's field and a
        // signature's can never share an id.
        NumField::MarkerName(i) => 2_000_000 + i,
        NumField::Fx(fx) => 200 + fx.index(),
    }
}

/// Which typeable field a hit belongs to, if any.
///
/// Exactly the draggable hits and no others, which is the point: a control you
/// can drag to a number is a control you should be able to type the number
/// into, and there is no third kind.
pub fn num_field(hit: Hit) -> Option<NumField> {
    match hit {
        Hit::EditTimeSignature => Some(NumField::Meter),
        // The knob IS the box: a double click on it opens the field. There is
        // no separate control to press.
        Hit::SetTempo(_) => Some(NumField::Tempo),
        Hit::SetFx(fx, _) => Some(NumField::Fx(fx)),
        Hit::SetMaster(_) => Some(NumField::Master),
        // **The pitch dial has no field, and that is the design.** Every
        // other control here can be typed into because every other control
        // prints a number you might want to match. This one prints nothing:
        // you find the pitch by ear, and a box to type 3.0 into is an
        // invitation to chase a value instead of listening. The owner's
        // call, and the machine the dial is modelled on works the same way.
        // The RANGE — how far the dial reaches — is still typed, in the
        // panel a right-click opens.
        Hit::SetVarispeed(_) => None,
        // **The one that opens a field IS a field.** Everything else here
        // reports which field it would be typed into if somebody asked; this
        // one is the asking.
        Hit::Type(f) => Some(f),
        // NOT `SetTempo`: that carries a committed value and has no box of
        // its own any more. `EditTempo` is the box.
        _ => None,
    }
}

/// What a rectangle produces when it is pressed.
///
/// Two kinds, because the dials are DRAGGED: their answer depends on where
/// along the rectangle the press landed. Making that a property of the region
/// rather than of the caller is what keeps [`hit_test`] a pure function of
/// geometry — the same point always produces the same hit, so the caller can
/// simply ask again on every pointer move.
#[derive(Debug, Clone, Copy)]
enum Produces {
    /// The same hit wherever inside it you press.
    Fixed(Hit),
    /// A hit carrying how far UP the rect the press landed: 0 at the bottom,
    /// 1 at the top. Every travelling control in this band is a round knob,
    /// where left-to-right would be a control that goes the wrong way half
    /// the time depending on which side of it you grabbed.
    AlongV(fn(f32) -> Hit),
}

impl Produces {
    /// The hit for a press at `pos` inside `r`.
    ///
    /// Takes the rect and the point rather than a fraction, because WHICH
    /// axis a travelling control measures is this enum's business and not the
    /// caller's — a caller that computed the fraction would have to know.
    fn hit(self, r: Rect, pos: Pos2) -> Hit {
        match self {
            Produces::Fixed(h) => h,
            Produces::AlongV(f) => f(up(r, pos)),
        }
    }

    /// A representative hit, for asking a producer WHICH CONTROL it is.
    ///
    /// The value it carries is meaningless and every caller ignores it:
    /// `label` and `is_same_control` are both about identity. A unit rect at
    /// the origin, so the answer does not depend on where anything is.
    fn control(self) -> Hit {
        self.hit(Rect::from_min_size(Pos2::ZERO, Vec2::splat(1.0)), Pos2::ZERO)
    }
}


/// How far along `r` the point `pos` fell, 0..=1 from its left edge to its
/// right.
///
/// Clamped, so both ends of a fader's travel are reachable rather than being a
/// value the last pixel rounds off the end of. `r.left()` is exactly 0.0,
/// `r.right()` is exactly 1.0, and the middle is exactly 0.5.
fn along(r: Rect, pos: Pos2) -> f32 {
    if r.width() <= 0.0 {
        return 0.0;
    }
    ((pos.x - r.left()) / r.width()).clamp(0.0, 1.0)
}

/// Which way a control travels.
///
/// A caller dragging one has to hold the OTHER axis still — see
/// [`drag_axis`] — or a fader loses the gesture the moment the hand drifts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragAxis {
    Horizontal,
    Vertical,
}

/// Which way the control under `hit` travels, if it travels at all.
///
/// **Read off the same table [`hit_test`] uses**, rather than matched on the
/// `Hit` variants here. A second list would be a second thing to update, and
/// the failure it produces is a knob that cannot be dragged at all: the caller
/// pins the axis the control actually moves along, and every pointer position
/// reports the same value.
pub fn drag_axis(rect: Rect, view: &RecorderView<'_>, hit: Hit) -> Option<DragAxis> {
    Layout::new(rect, view)
        .targets()
        .into_iter()
        .find(|(r, k)| r.is_positive() && (*k).control().is_same_control(hit))
        .and_then(|(_, k)| match k {
            Produces::AlongV(_) => Some(DragAxis::Vertical),
            Produces::Fixed(_) => None,
        })
}

/// How far the pointer must move to sweep `hit` end to end, in points.
///
/// **Every control here is dragged RELATIVELY**, knobs and faders alike: the
/// value follows how far the hand has moved rather than where it ended up. A
/// fader's travel is its own track, so it still feels one-to-one under the
/// pointer; what it gains is that the hand can leave the track and keep going,
/// and that pressing it never makes the handle jump.
///
/// `None` for anything that does not travel.
pub fn drag_travel(rect: Rect, view: &RecorderView<'_>, hit: Hit) -> Option<f32> {
    let l = Layout::new(rect, view);
    let (_, k) = l
        .targets()
        .into_iter()
        .find(|(r, k)| r.is_positive() && (*k).control().is_same_control(hit))?;
    match k {
        Produces::AlongV(_) => Some(KNOB_TRAVEL),
        Produces::Fixed(_) => None,
    }
}

/// How far the pointer must move to sweep a knob end to end, in points.
///
/// **Wider than the knob, by a lot.** A knob's cell is a few tens of points;
/// mapping that to the whole range is a control nobody can land on a number
/// with. The hand can leave the knob, leave the band, and go on turning.
///
/// 150 and not the 260 it shipped at: on a trackpad the sweep is the whole
/// pad, and dragging UP from a band dial met the top of the screen before
/// the top of the range - the owner had to turn one dial in instalments.
/// 150 matches the drumpad's knobs, so the two families finally agree on
/// what a full turn costs the hand.
pub const KNOB_TRAVEL: f32 = 150.0;

/// How far the pointer must move to sweep a FADER end to end, in points.
///
/// **Wider than the fader, and by more than a knob is.** A fader used to
/// travel its own track — one-to-one, which feels direct and is the reason
/// half its numbers could not be reached: seventy-two decibels across two
/// hundred points is a third of a decibel per point, and it reads to a tenth.
/// Any value a control can DISPLAY has to be reachable with the mouse alone,
/// so the travel is the number that makes a tenth of a decibel one point.
///
/// A big move is still a big move: -60 to unity is most of a screen. What
/// makes that acceptable is that neither of the two ways of getting there in a
/// hurry goes through the drag — a double click resets it, and the dB at the
/// end of the row can be typed into.
pub const FADER_TRAVEL: f32 =
    (crate::recorder::GAIN_MAX_DB - crate::recorder::GAIN_MIN_DB) * 10.0;

/// How much finer a drag is with the fine modifier held.
///
/// **Any readable value has to be reachable by hand.** A fader spans seventy-
/// two decibels and reads to a tenth of one; over its own track that is four
/// tenths of a decibel per point, so half the numbers it can display cannot be
/// landed on. Six times finer puts every one of them within reach without
/// making the ordinary gesture sluggish.
// A tenth, was a sixth: with the drags INTEGRATING now (fine scales each
// frame's motion, never the accumulated distance), the factor can afford to
// be truly fine — approximate at full speed, then land the exact number.
pub const FINE_DRAG: f32 = 0.1;

/// How far UP `r` the point `pos` fell, 0..=1 from its bottom edge to its top.
///
/// Screen y grows downward and knobs do not, so this is [`along`] inverted on
/// the other axis. Clamped for the same reason: both ends have to be reachable.
fn up(r: Rect, pos: Pos2) -> f32 {
    if r.height() <= 0.0 {
        return 0.0;
    }
    ((r.bottom() - pos.y) / r.height()).clamp(0.0, 1.0)
}

/// What is under `pos`, if anything.
///
/// The exact inverse of [`draw`], by construction rather than by discipline:
/// both read the same [`Layout`].
///
/// # Dragging
///
/// Six of the hits carry a value taken from where in their rectangle the press
/// landed. **The caller is expected to call this again on every pointer move
/// while the button is held**, and to keep hold of which control it grabbed —
/// this function has no memory, so a pointer that wanders off the track returns
/// whatever is under it now, or `None`. Comparing the new hit to the grabbed one
/// with [`Hit::is_same_control`] is how the caller tells "still dragging the
/// click fader" from "the pointer is over the record button", and — since the
/// three slot knobs are stacked one above another — "still on slot 0's knob"
/// from "the pointer has slipped onto slot 1's".
///
/// # While a take is live
///
/// [`Hit::Stop`], the two faders, the three slot knobs, the three editor
/// buttons and [`Hit::ToggleMetronome`] survive. Every destination control is
/// gone, which is not a courtesy — the output folder, the take name and the
/// devices are all decided at `T0` and a UI that lets you change them at 0:47 is
/// a UI that promises something it cannot do. The instrument PICKERS go with
/// them, for a harder reason still: loading a VST3 blocks the main thread for
/// seconds, and doing that during a take costs the take.
///
/// The levels survive for the opposite reason: turning the click down halfway
/// through a take is precisely when you need to, and the level you hear changes
/// nothing about the file. So do the editor buttons — reaching for a different
/// preset between two passes is a real thing, and it is the plugin's own window
/// that does it. The one monitor control that DOES change the file — whether the
/// click is recorded into it — goes away with the destination.
/// Where the Setup button is, so the app can open the menu at it.
/// Where the supporter heart goes: bottom-right of the band, in its own strip.
///
/// [`Rect::NOTHING`] when the band is too small to carry it. The app draws it
/// and owns every gesture on it; this is only the geometry, in the one file
/// that knows the band's geometry.
pub fn heart_rect(rect: Rect, view: &RecorderView<'_>) -> Rect {
    Layout::new(rect, view).heart
}

/// Where a knob is, for a caller that needs to reason about the gesture.
///
/// `None` when the band is too small to draw it — which is also when it must
/// not be draggable. Same predicate the painter uses.
pub fn knob_rect(rect: Rect, view: &RecorderView<'_>, hit: Hit) -> Option<Rect> {
    let l = Layout::new(rect, view);
    let r = match hit {
        Hit::SetFx(fx, _) => l.cell(fx),
        Hit::SetMaster(_) => l.master_knob,
        Hit::SetTempo(_) => l.tempo,
        Hit::SetVarispeed(_) => l.vari,
        _ => return None,
    };
    r.is_positive().then_some(r)
}

/// The room `PITCH CONT` may print in, for a caller that has to reason about
/// the dial's size — see `vari_face`.
pub fn vari_word_rect(rect: Rect, view: &RecorderView<'_>) -> Rect {
    Layout::new(rect, view).vari_label
}

/// The camera preview, for the right-click that opens the camera picker.
///
/// The device belongs to the picture it fills. It was a row in the take
/// settings, next to where the file goes and how long the count-in is, which
/// is a list of things about a TAKE — a camera is not one of them.
pub fn preview_rect(rect: Rect, view: &RecorderView<'_>) -> Option<Rect> {
    let r = Layout::new(rect, view).preview;
    r.is_positive().then_some(r)
}

pub fn setup_rect(rect: Rect, view: &RecorderView<'_>) -> Rect {
    Layout::new(rect, view).setup
}

/// The capture button, for the panel that hangs off it.
pub fn capture_rect(rect: Rect, view: &RecorderView<'_>) -> Rect {
    Layout::new(rect, view).capture
}

/// The record button, for the same reason.
pub fn record_rect(rect: Rect, view: &RecorderView<'_>) -> Rect {
    Layout::new(rect, view).record
}

/// Everything in the band a keyboard can reach and a screen reader can say.
///
/// **Built from `Layout::targets`, which already existed.** That list is the
/// panel's own enumeration of its controls — it is what `hit_test` searches —
/// so this is the same walk with a name attached to each rect rather than a
/// second, parallel description of the band that could drift from it. A
/// control added to `targets` and not named here comes out as its own debug
/// spelling, which is ugly and audible; one named here and removed from
/// `targets` simply disappears. Neither can silently disagree about WHERE a
/// control is, which is the failure that mattered.
///
/// In reading order, because allocation order is tab order.
pub fn a11y_nodes(rect: Rect, view: &RecorderView<'_>) -> Vec<crate::a11y::Node> {
    let l = Layout::new(rect, view);
    let id = |k: &str| egui::Id::new(("band", k));
    let mut out = Vec::new();
    for (r, produces) in l.targets() {
        if !r.is_positive() {
            continue;
        }
        let Some(node) = name_for(produces, r, view, &id) else {
            continue;
        };
        out.push(node);
    }
    out
}

/// The targets that are SURFACES rather than controls.
///
/// The meter you click to clear a clip light, the timeline you click to
/// locate in, the scope. They are named here rather than falling out of
/// `name_for`'s catch-all so that the partition is explicit and a control
/// added to `Layout::targets` cannot join them by accident — see
/// `every_control_in_the_band_can_be_reached_and_says_what_it_is`, which
/// asserts every positive target is either named or on this list.
///
/// `Hit::Type` is here for a different reason: it is the TYPED twin of a dial
/// that already has a node, and a tab order with both in it walks every knob
/// twice while a screen reader reads it twice.
///
/// Read only by that coverage test, deliberately: this list IS the partition
/// and `name_for`'s catch-all is checked against it, so a control added to
/// `Layout::targets` and forgotten cannot join the surfaces by accident. The
/// catch-all's own comment points here.
#[allow(dead_code)]
fn is_surface(hit: &Hit) -> bool {
    matches!(
        hit,
        Hit::Type(_)
            | Hit::DismissClip
            | Hit::SeekTimeline(_)
    )
}

/// The words for one target.
fn name_for(
    produces: Produces,
    r: Rect,
    view: &RecorderView<'_>,
    id: &dyn Fn(&str) -> egui::Id,
) -> Option<crate::a11y::Node> {
    use crate::a11y::Node;
    let fx_value = |fx: Fx| {
        knob_reading(view.fx_units[fx.index()], view.fx.get(fx))
    };
    let up = |norm: f32| crate::a11y::Travel { norm: norm.clamp(0.0, 1.0), up: true };
    Some(match produces {
        Produces::Fixed(hit) => match hit {
            Hit::Play => Node::button(id("play"), r, "Play"),
            Hit::Record => Node::button(id("record"), r, "Record"),
            Hit::Stop => Node::button(id("stop"), r, "Stop"),
            Hit::Pause => Node::button(id("pause"), r, "Pause"),
            Hit::Capture => Node::button(id("capture"), r, "Record a take"),
            Hit::ProjectMenu => Node::button(id("project"), r, "Project menu"),
            Hit::OpenSetup => Node::button(id("setup"), r, "Take settings"),
            // Disabled while the tempo is held - see `RecorderState::tempo_held`.
            Hit::TapTempo => Node::button(id("tap"), r, "Tap tempo").enabled(!view.tempo_held),
            Hit::Export => Node::button(id("export"), r, "Export"),
            Hit::ChooseFolder => Node::button(id("dest"), r, "Choose the take folder"),
            Hit::RevealFolder => Node::button(id("reveal"), r, "Show the take folder"),
            Hit::ChooseProjectFolder => {
                Node::button(id("proj-dest"), r, "Choose the project folder")
            }
            Hit::RevealProjectFolder => {
                Node::button(id("proj-reveal"), r, "Show the project folder")
            }
            Hit::ChooseExportFolder => {
                Node::button(id("exp-dest"), r, "Choose the export folder")
            }
            Hit::RevealExportFolder => {
                Node::button(id("exp-reveal"), r, "Show the export folder")
            }
            Hit::ChoosePresetFolder => {
                Node::button(id("preset-dest"), r, "Choose the preset folder")
            }
            Hit::RevealPresetFolder => {
                Node::button(id("preset-reveal"), r, "Show the preset folder")
            }
            Hit::ChooseIrFolder => {
                Node::button(id("irs-dest"), r, "Choose the impulse-response folder")
            }
            Hit::RevealIrFolder => {
                Node::button(id("irs-reveal"), r, "Show the impulse-response folder")
            }
            Hit::PickCamera => Node::button(id("camera"), r, "Choose a camera"),
            Hit::PickAudio => Node::button(id("audio"), r, "Choose an audio input"),
            Hit::ClearTrack => Node::button(id("cleartrack"), r, "Remove the loaded file"),
            Hit::EditTimeSignature => Node::readout(
                id("timesig"),
                r,
                "Time signature",
                format!("{}/{}", view.time_signature.beats, view.time_signature.unit),
            ),
            Hit::CycleCountIn => Node::readout(
                id("countin"),
                r,
                "Count-in",
                if view.count_in_beats == 0 {
                    "off".to_owned()
                } else {
                    format!("{} beats", view.count_in_beats)
                },
            ),
            // **A button and not a toggle, because the view does not carry
            // its state.** A `Kind::Toggle` whose bool was a guess would have
            // a screen reader confidently announcing "off" over a ticked box,
            // which is worse than not announcing it: a wrong readout is the
            // one kind a person cannot work around. When the view grows the
            // field, this becomes a toggle and the test below will say so.
            Hit::ToggleOpenWhenDone => {
                Node::button(id("openwhendone"), r, "Show the folder when a take finishes")
            }
            Hit::ToggleMetronomeInTake => Node::toggle(
                id("clickintake"),
                r,
                "Record the click into the take",
                view.metronome_in_take,
            ),
            Hit::ToggleCountInInTake => {
                Node::button(id("countinintake"), r, "Record the count-in into the take")
            }
            Hit::ToggleInputMonitor => Node::toggle(
                id("monitor"),
                r,
                "Hear the input through the monitors",
                view.input_monitor,
            ),
            Hit::ToggleHideElapsed => {
                Node::toggle(id("hideelapsed"), r, "Hide the elapsed time", view.hide_elapsed)
            }
            Hit::NameField => Node::readout(id("name"), r, "Take name", view.take_name.to_owned()),
            Hit::CloseSetup => Node::button(id("closesetup"), r, "Close the take settings"),
            Hit::PluginFolders => Node::button(id("pluginfolders"), r, "Add a plugin folder"),
            Hit::RescanPlugins => Node::button(id("pluginrescan"), r, "Rescan for plugins"),
            Hit::StreamToObs => Node::button(id("obs"), r, "Stream to OBS"),
            Hit::ShowAudioStatus => Node::button(id("audiostatus"), r, "Audio status"),
            // **The one control on the band a screen reader most needs to be
            // told about**, because it is the one whose engaged state is
            // otherwise invisible: a varispeed left on makes every export
            // come out at the wrong pitch, and its readout is a number in a
            // corner. So the value is the whole sentence, not just the
            // percentage.
            Hit::CycleLufs => Node::readout(
                id("lufs"),
                r,
                "Loudness",
                if view.loudness.fraction().is_some() {
                    format!(
                        "{} {} LUFS",
                        view.loudness.window.title(),
                        view.loudness.text()
                    )
                } else {
                    format!("{}, nothing measured yet", view.loudness.window.title())
                },
            ),
            Hit::ResetLufs => return None,
            // The typed twins of the dials below. A screen reader hearing
            // both would hear every knob twice, and the dial is the one that
            // can be nudged with an arrow.
            Hit::Type(_) => return None,
            // Surfaces, not controls — see `is_surface`, which is the list
            // the coverage test checks this catch-all against.
            _ => return None,
        },
        Produces::AlongV(f) => {
            // Which control this is, asked of the function itself rather than
            // tracked in a parallel list: `Produces` carries a fn pointer, and
            // the cheapest honest way to identify it is to see what it makes.
            match f(0.5) {
                // `up: true` on every one of them, because they all arrive
                // through `AlongV` — this band's dials are round, and a round
                // knob read left-to-right goes the wrong way from whichever
                // side of it you did not grab.
                Hit::SetMaster(_) => Node::slider(
                    id("master"),
                    r,
                    "Master",
                    f64::from(view.gains.master),
                    0.0,
                    1.0,
                    gain_text(view.gains.master),
                    // **The FADER's position, not the gain.** `Hit::SetMaster`
                    // carries where the knob stands and `fader_to_gain` turns
                    // it into amplitude over a decibel curve, so the two are
                    // only equal at the ends. Handing the gain over as the
                    // knob's position sent one press of the up arrow from
                    // -6 dB to -20.4 — it moved, which is exactly why a test
                    // that only asked "did anything change" would have passed.
                    up(gain_to_fader(view.gains.master)),
                ),
                Hit::SetFx(fx, _) => Node::slider(
                    id(fx.title()),
                    r,
                    fx.title().to_lowercase(),
                    f64::from(view.fx.get(fx)),
                    0.0,
                    1.0,
                    fx_value(fx),
                    // Raw, unlike the master: `Hit::SetFx` carries the mix
                    // itself, which `apply_recorder_hit` stores as-is.
                    up(view.fx.get(fx)),
                ),
                Hit::SetVarispeed(_) => Node::slider(
                    id("varispeed"),
                    r,
                    "Varispeed",
                    f64::from(view.varispeed.pos()),
                    0.0,
                    1.0,
                    if view.varispeed.engaged() {
                        format!(
                            "{}, {:+.1} semitones",
                            view.varispeed.reading(),
                            view.varispeed.semitones()
                        )
                    } else {
                        "off".to_owned()
                    },
                    // The POSITION, like the master's: the percentage it
                    // reads depends on the range, and an arrow key moves the
                    // dial and not the number.
                    up(view.varispeed.pos()),
                ),
                Hit::SetTempo(_) => Node::slider(
                    id("tempo"),
                    r,
                    "Tempo",
                    view.tempo_bpm,
                    f64::from(MIN_BPM),
                    f64::from(MAX_BPM),
                    format!("{:.0} BPM", view.tempo_bpm),
                    // NOT the tempo over its range: the knob's sweep is
                    // mapped through `knob_to_tempo`, so the fraction the
                    // pointer means is `tempo_to_knob` and nothing simpler.
                    up(tempo_to_knob(view.tempo_bpm)),
                )
                .enabled(!view.tempo_held),
                _ => return None,
            }
        }
    })
}

pub fn hit_test(rect: Rect, view: &RecorderView<'_>, pos: Pos2) -> Option<Hit> {
    if !rect.contains(pos) {
        return None;
    }
    let l = Layout::new(rect, view);
    // **The timeline's × first — the mixer palette's rule.** It sits ON the
    // seek surface, and the seek's fraction is measured against the whole
    // timeline, so the × cannot be carved out of the target without bending
    // the mapping ("click at 1:24 and something READS 1:24"). A pre-pass
    // keeps the no-overlap invariant on `targets` absolute and still lets
    // the corner mean remove rather than locate. Same gates as the seek:
    // a file loaded, and never while rolling.
    if !l.rolling && l.timeline_loaded && l.timeline_close.contains(pos) {
        return Some(Hit::ClearTrack);
    }
    l.targets()
        .into_iter()
        .find(|(r, _)| r.contains(pos))
        .map(|(r, k)| k.hit(r, pos))
}

/// The part of an empty camera pane that PICKS a camera rather than
/// yielding to the borderless window drag: all of it.
///
/// It was the pane's centre, a third each way, when an empty pane was a
/// grey box with a hint in the middle — unoccupied space, which the
/// owner's rule hands to the window. Then briefly a tag in the corner. The
/// pane is the spectrum analyzer now: a surface that is hovered for its
/// readout and right-clicked for its menu is occupied space, and a press on
/// it does what a press on it always did, which is open the camera list.
/// So the chip is the pane, the press handler's carve-out finds nothing to
/// carve, and there is no button to look for — the owner's word: "camera
/// button should not be there".
pub fn camera_chip(rect: Rect, view: &RecorderView<'_>) -> Rect {
    let l = Layout::new(rect, view);
    if !l.preview.is_positive() {
        return Rect::NOTHING;
    }
    l.preview
}

/// The seek fraction a pointer at `pos` means, X ONLY.
///
/// The scrub: once a press has landed on the timeline, the locate follows
/// the HAND — Ableton's gesture — and following in X alone is what forgives
/// the vertical slop a hand dragging sideways always has. Clamped, so a
/// drag off either end holds at the end rather than letting go. `None` on
/// the faces where the timeline is not seekable: empty. **Seekable
/// mid-take** - the take films the window and does not own the transport,
/// so moving the playhead under one is the breakdown video's ordinary
/// gesture, not a torn file.
pub fn timeline_fraction(rect: Rect, view: &RecorderView<'_>, pos: Pos2) -> Option<f32> {
    let l = Layout::new(rect, view);
    if !l.timeline_loaded || !l.timeline.is_positive() {
        return None;
    }
    Some(((pos.x - l.timeline.left()) / l.timeline.width()).clamp(0.0, 1.0))
}

/// The next count-in length in the cycle, in BARS (`COUNT_IN_CHOICES`).
///
/// The one place the chip's cycle is decided: the chip's arm in
/// `TangentApp::apply_recorder_hit` steps through it, so the control's label
/// and the effect of clicking it cannot disagree.
///
/// **The next choice ABOVE the length shown, not the one after it in the
/// list.** A length off the list - the metronome panel's COUNT row steps a
/// bar at a time, and a hand-edited file can hold anything - used to start
/// the cycle over at off, so 3 bars went to OFF at one press. It steps to 4
/// now; past the last choice it wraps round to off, as 4 itself does.
pub fn next_count_in(current: u32) -> u32 {
    COUNT_IN_CHOICES
        .iter()
        .copied()
        .find(|&c| c > current)
        .unwrap_or(COUNT_IN_CHOICES[0])
}

// ── drawing ────────────────────────────────────────────────────────────────

/// Draw the band. A pure painter: no `Ui`, no `Context`, no input, no clock.
///
/// The absence of a clock is not tidiness either. §5's re-export table promises
/// that a display-only video can be rebuilt after the fact by replaying the
/// recorded MIDI through these same `draw` functions, and that promise is worth
/// exactly as much as "the display at any instant is a function of the state at
/// that instant". One `animate_bool` here and a take could never be re-rendered.
pub fn draw(painter: &Painter, rect: Rect, view: &RecorderView<'_>, s: &Settings) {
    let p = palette(s);
    painter.rect_filled(rect, 0.0, p.bg);
    // Nothing escapes the band. Text is shrunk to fit its box rather than
    // truncated, but a pathologically long path still has to stop at the edge
    // instead of being painted over the piano.
    let painter = &painter.with_clip_rect(rect);
    let l = Layout::new(rect, view);

    draw_preview(painter, &l, view, &p);
    draw_transport(painter, &l, view, &p);
    draw_capture(painter, l.capture, view.state.is_active(), &p);
    draw_folder(painter, l.project, &p);
    draw_meter(painter, l.meter, view.meters, &p);
    draw_monitor(painter, &l, view, &p);
    // AFTER the monitor, which fills the LCD's face: the take's readout is
    // drawn on its glass.
    draw_readout(painter, &l, view, &p);
    if !l.rolling {
        draw_cog(painter, l.setup, &p);
    }
    // In both layouts now - see `fill_faders`. `draw_knob` draws nothing in a
    // NOTHING rect, which is what an export leaves.
    draw_tempo(
        painter,
        &l,
        view.tempo_bpm,
        typing_for(view, NumField::Tempo),
        view.turning == Some(NumField::Tempo),
        view.tempo_held,
        &p,
    );
    for r in l.rules {
        if r.is_positive() {
            painter.rect_filled(r, 0.0, p.line.gamma_multiply(0.55));
        }
    }

    // **The recording border is NOT here any more.** It used to ring this
    // band, which said the wrong thing twice: the take is the WINDOW, so a
    // border round one band does not mean "recording"; and this function is
    // the same one the compositor calls, so the border was burned into every
    // take. It rings the window now, from `TangentApp::paint`, which the
    // compositor never calls — see the comment there. A running take reads
    // from the lit cassette (`draw_capture`).
}

/// A control's chrome: a filled box with a hairline, so a thing you can click
/// looks unlike a thing you read.
fn control(painter: &Painter, r: Rect, p: &Palette) {
    if !r.is_positive() {
        return;
    }
    painter.rect_filled(r, 2.0, p.field);
    painter.rect_stroke(r, 2.0, Stroke::new(1.0_f32, p.line), StrokeKind::Inside);
}

/// How far a labelled box's text stands in from its edges.
///
/// Bounded by the WIDTH as well as the height, which the first version was not:
/// in a tall narrow box — a detached window's export button — a 30%-of-height
/// inset ate the whole box and the label silently stopped being drawn.
fn label_inset(r: Rect) -> f32 {
    (r.height() * 0.30).min(r.width() * 0.06)
}

/// A labelled control: the caption in the faint ink, the value after it.
///
/// One helper because most of the destination column is this shape, and eight
/// copies would drift apart the first time one of them was adjusted.
fn labelled(painter: &Painter, r: Rect, cap: &str, value: &str, colour: Color32, p: &Palette) {
    control(painter, r, p);
    label_text(painter, r, cap, value, colour, p);
}

/// The caption and the value, with no box under them.
///
/// Split out of [`labelled`] for the one place that has to draw the same pair
/// of words WITHOUT the chrome: a slot's name while a take is rolling, when it
/// is a thing to read and not a thing to press. Same insets, same sizing, so
/// the row does not jump at the moment the take starts.
fn label_text(painter: &Painter, r: Rect, cap: &str, value: &str, colour: Color32, p: &Palette) {
    let Some(inner) = label_inner(r) else {
        return;
    };
    let joined = format!("{cap} {value}");
    let size = fit_text(inner, &joined, inner.height() * 0.52);
    if size < MIN_TEXT {
        return;
    }
    paint_caption_value(painter, inner, cap, value, size, colour, p);
}

/// Where a labelled row's words go: the row, less its inset on each side.
fn label_inner(r: Rect) -> Option<Rect> {
    if !r.is_positive() {
        return None;
    }
    let inset = label_inset(r);
    let inner = Rect::from_min_max(
        Pos2::new(r.left() + inset, r.top()),
        Pos2::new(r.right() - inset, r.bottom()),
    );
    inner.is_positive().then_some(inner)
}

/// The caption in the faint ink, then the value, at one size.
fn paint_caption_value(
    painter: &Painter,
    inner: Rect,
    cap: &str,
    value: &str,
    size: f32,
    colour: Color32,
    p: &Palette,
) {
    let y = inner.center().y;
    let after = painter
        .text(
            Pos2::new(inner.left(), y),
            Align2::LEFT_CENTER,
            cap,
            font_light(size),
            p.faint,
        )
        .right();
    if value.is_empty() {
        return;
    }
    painter.text(
        Pos2::new(after + size * ADV, y),
        Align2::LEFT_CENTER,
        value,
        font_light(size),
        colour,
    );
}

/// **The smallest a device name is set, as a share of the row's own type.**
///
/// A labelled row shrinks its caption and its value TOGETHER and draws
/// nothing below `MIN_TEXT` - right for a path or a number, wrong for a device
/// name, which comes from the driver at whatever length the driver likes. The
/// owner's Linux input is the PipeWire node
/// `alsa_input.usb-Focusrite_Scarlett_Solo_USB_Y771VU50AAEF28-00.pro-input-0`:
/// 84 characters with the caption, 260 points at the smallest type, in a row
/// 197 to 309 points wide - so the AUDIO SETUP row was an EMPTY BOX in every
/// window narrower than about 1450 points, and 5.9-point type above that.
/// Below this floor the name is cut in the MIDDLE instead (the start names
/// the maker, the end the driver or the profile), so the row always says
/// something at a size somebody can read.
const DEVICE_FLOOR: f32 = 0.75;

/// `s` cut to `max` characters with an ellipsis in the middle, counted in
/// CHARACTERS - a driver name can hold a trademark sign - never bytes.
pub(crate) fn cut_middle(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_owned();
    }
    if max == 0 {
        return String::new();
    }
    let keep = max - 1;
    let head = keep - keep / 2;
    let tail = keep / 2;
    let h: String = s.chars().take(head).collect();
    let t: String = s.chars().skip(n - tail).collect();
    format!("{}\u{2026}{}", h.trim_end(), t.trim_start())
}

/// What a device row sets, and at what size: the whole name when it fits at
/// the floor, else the name cut to fit at the floor, else the caption alone.
fn device_row_text<'a>(inner: Rect, cap: &str, name: &'a str) -> (std::borrow::Cow<'a, str>, f32) {
    use std::borrow::Cow;
    let want = inner.height() * 0.52;
    let floor = (want * DEVICE_FLOOR).max(MIN_TEXT);
    let size = fit_text(inner, &format!("{cap} {name}"), want);
    if size >= floor {
        return (Cow::Borrowed(name), size);
    }
    let room = (inner.width() / (floor * ADV)).floor() as usize;
    let Some(budget) = room.checked_sub(cap.chars().count() + 1) else {
        return (Cow::Borrowed(""), fit_text(inner, cap, want));
    };
    let cut = cut_middle(name, budget);
    let size = fit_text(inner, &format!("{cap} {cut}"), want);
    (Cow::Owned(cut), size)
}

/// A labelled row whose value is a DEVICE name: never blank, never smudged.
/// See [`DEVICE_FLOOR`].
fn draw_device_row(painter: &Painter, r: Rect, cap: &str, name: &str, colour: Color32, p: &Palette) {
    control(painter, r, p);
    let Some(inner) = label_inner(r) else {
        return;
    };
    let (value, size) = device_row_text(inner, cap, name);
    if size < MIN_TEXT {
        return;
    }
    paint_caption_value(painter, inner, cap, &value, size, colour, p);
}

/// The camera, as a PANE OF THE WINDOW rather than a box in the Recorder band.
///
/// **This is the whole of the camera's presence in the app now.** It sits
/// beside the theory diagrams, it is what you frame a shot in, and — because
/// the video is the window's own bands laid into the video's frame — it is also
/// exactly what lands in the recording. There is no second model of where the
/// camera goes and therefore nothing for the two models to disagree about,
/// which is what every camera-layout bug in this app has been.
///
/// Public because it is painted from `app.rs`, from the two places that draw
/// bands: the live window and the offscreen compositor.
pub fn draw_camera_pane(
    painter: &Painter,
    rect: Rect,
    preview: Option<Preview>,
    camera: DeviceLabel<'_>,
    surround: Color32,
    s: &Settings,
    analyzer: &crate::analyzer::Analyzer,
) {
    // The band it SITS in, not the band it came from. The letterbox bars round
    // a 4:3 camera are the theory panel's own background, so the row reads as
    // one band with a camera in it rather than as a hole cut in the window.
    let mut p = palette(s);
    p.bg = surround;
    // The compositor's copy: the analyzer it is handed, and no pointer — a
    // hairline that follows the cursor does not belong in a recording.
    draw_camera(painter, rect, preview, camera, &p, analyzer, None, s.prefer_flats);
}

fn draw_preview(painter: &Painter, l: &Layout, view: &RecorderView<'_>, p: &Palette) {
    draw_camera(
        painter,
        l.preview,
        view.preview,
        view.camera,
        p,
        view.analyzer,
        view.pointer,
        view.prefer_flats,
    );
}

/// One camera box, wherever it is.
#[allow(clippy::too_many_arguments)]
fn draw_camera(
    painter: &Painter,
    r: Rect,
    preview: Option<Preview>,
    camera: DeviceLabel<'_>,
    p: &Palette,
    analyzer: &crate::analyzer::Analyzer,
    pointer: Option<Pos2>,
    prefer_flats: bool,
) {
    if !r.is_positive() {
        return;
    }
    // **No camera chosen: the pane is the spectrum analyzer.** The owner's
    // condition for it — "this is a TOOL and has to be useful to be
    // justified" — is what `analyzer.rs` answers; this is only where it
    // stands. It wears the cluster's LCD pair, so on Centennial it is the
    // scope's gold on black, and it takes the WHOLE pane.
    if preview.is_none() && matches!(camera, DeviceLabel::None) {
        // **And nothing else on it.** The first cut printed a CAMERA tag in
        // the corner so the picker had a word; the owner's answer was that
        // the button should not be there. The whole box is the picker — a
        // press anywhere on it opens the camera list, as it always did — and
        // the tooltip on the box says so.
        let style = crate::scope::ScopeStyle::themed(p.lcd, p.lcd_face);
        analyzer.draw(painter, r, &style, pointer, prefer_flats);
        painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, p.line), StrokeKind::Inside);
        return;
    }
    match preview {
        Some(pv) => {
            // The bars are the BAND's background, not a black mat: the box is
            // fixed by the layout and the frame is fitted into it, so whatever
            // is left over has to look like the band and not like a fault.
            painter.rect_filled(r, 0.0, p.bg);
            let dst = fit_preview(r, pv.size);
            if dst.is_positive() {
                painter.image(pv.texture, dst, FULL_FRAME, Color32::WHITE);
            }
        }
        None => {
            painter.rect_filled(r, 0.0, p.well);
            // Say what to do. An empty grey box is indistinguishable from a
            // camera pointed at a wall, and one of those is the user's problem
            // to fix while the other is not.
            let (top, hint) = match camera {
                // "on the right" was true when the camera picker was a box in
                // the middle of the band. It is behind the cog now, and a
                // placeholder pointing at somewhere the control has not been
                // for a release is worse than no placeholder.
                // **The hint is the gesture**, because the box IS the
                // control now: clicking the preview opens the picker. It said
                // "choose one under the cog", which was a direction to
                // somewhere else and stopped being true twice.
                DeviceLabel::None => ("NO CAMERA", "Select Camera"),
                DeviceLabel::Missing(_) => ("CAMERA NOT AVAILABLE", "Select Camera"),
                DeviceLabel::Open(_) => ("WAITING FOR CAMERA", "the first frame has not arrived"),
            };
            let size =
                fit_text(r, hint, r.height() * 0.11).min(fit_text(r, top, r.height() * 0.14));
            if size >= MIN_TEXT {
                painter.text(
                    Pos2::new(r.center().x, r.center().y - size),
                    Align2::CENTER_CENTER,
                    top,
                    font_light(size),
                    p.ink.gamma_multiply(0.8),
                );
                painter.text(
                    Pos2::new(r.center().x, r.center().y + size),
                    Align2::CENTER_CENTER,
                    hint,
                    font_light(size * 0.72),
                    p.faint,
                );
            }
        }
    }
    painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, p.line), StrokeKind::Inside);
}

fn draw_transport(painter: &Painter, l: &Layout, view: &RecorderView<'_>, p: &Palette) {
    // **Lit while a Play waits on a count's downbeat too.** Pressed during a
    // count-in, Play starts the song on the downbeat, and a triangle left dark
    // until then read as a press that had not registered. The Pause lit with
    // it is the one that takes it back.
    let playing = view.playing || view.play_pending;
    if l.play.is_positive() {
        // **The same kind of thing as record and stop**: a filled shape with a
        // hairline, at the transport's own size — not a labelled row. A
        // triangle of the record button's visual weight: an equilateral
        // triangle of side D is 43% of the circle's ink, so it is drawn on the
        // full rect where the square is inset.
        let r = l.play;
        let c = r.center();
        let h = r.height() * 0.5;
        let w = r.width() * 0.44;
        // The triangle, lit while the transport rolls — PAUSE is its own
        // button now, so this one never changes shape.
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(c.x - w * 0.7, c.y - h),
                Pos2::new(c.x - w * 0.7, c.y + h),
                Pos2::new(c.x + w * 1.1, c.y),
            ],
            if playing {
                PLAY_GREEN
            } else {
                toward(PLAY_GREEN, p.bg, 0.35)
            },
            Stroke::new(1.5_f32, p.line),
        ));
    }
    if l.record.is_positive() {
        let c = l.record.center();
        let rad = l.record.width() * 0.5;
        // Dimmed at rest and full red while the arrangement RECORDS — the
        // same grammar as the play triangle, which is how one row says
        // which of its machines is live.
        let fill = if view.arr_recording {
            p.rec
        } else {
            toward(p.rec, p.bg, 0.35)
        };
        painter.circle_filled(c, rad, fill);
        painter.circle_stroke(c, rad, Stroke::new(1.5_f32, p.line));
    }
    // The two glyphs sit in rects of the SAME size — and that alone did not
    // make them look the same size, which is the part the first attempt got
    // wrong. The circle fills its rect; the square was inset another 9%, so it
    // was 82% of the circle's diameter and read as the smaller control.
    //
    // A square of side D beside a circle of diameter D looks BIGGER, because it
    // is 27% more ink. Equal AREA is the honest match: side = D * sqrt(pi)/2 =
    // 0.886 D, which is the inset used below.
    if l.pause.is_positive() {
        // Two bars, the sign everything uses — dimmed like the resting
        // triangle, in its green so the pair read as one machine.
        let r = l.pause;
        let c = r.center();
        let bw = r.width() * 0.24;
        let gap = bw * 0.7;
        let col = toward(PLAY_GREEN, p.bg, if playing { 0.0 } else { 0.35 });
        for side in [-1.0_f32, 1.0] {
            painter.rect_filled(
                Rect::from_center_size(
                    Pos2::new(c.x + side * (gap + bw) * 0.5, c.y),
                    Vec2::new(bw, r.height() * 0.86),
                ),
                1.0,
                col,
            );
        }
    }
    if l.stop.is_positive() {
        // Present in BOTH layouts, in the same place. A stop button that only
        // appears once the take has started is one you have to find while your
        // hands are on the keys.
        //
        // Drawn like the record button and not like a menu row: a filled shape
        // with a hairline round it, at the same size. It used to sit inside a
        // `control` box, which is the chrome every LABELLED row in this band
        // wears — so the two transport buttons were not merely different sizes
        // but different KINDS of thing.
        //
        // The square is inset a little because an inscribed square is 27%
        // more ink than the circle it shares a diameter with, and equal
        // measurements are not equal weight.
        const STOP_INSET: f32 = (1.0 - 0.886) * 0.5;
        let stop = l.stop.shrink(l.stop.width() * STOP_INSET);
        // Lit while there is something for it to stop - the transport or
        // the arrangement record. Not the take: the cassette ends that.
        let live = view.playing || view.arr_recording;
        painter.rect_filled(stop, 1.0, if live { p.ink } else { p.faint });
        painter.rect_stroke(
            stop,
            1.0,
            Stroke::new(1.5_f32, p.line),
            StrokeKind::Inside,
        );
    }
}

/// Tape amber — the capture control's OWN colour, so it can never be
/// mistaken for the arrangement's red.
const TAPE_AMBER: Color32 = Color32::from_rgb(0xC9, 0x8B, 0x3D);

/// **How far the LIT amber must stand off the band.** TAPE_AMBER measures
/// 3.4:1 on the stock wine band and more on every dark factory band, so it is
/// used unchanged there; on the light bands (Sandstone, Celadon, Centennial)
/// it sits near the band's own lightness, and "lit" is carried by depth
/// instead - `distinct_surface_at`, the palette's own way of deriving a
/// surface that has to read.
const CASSETTE_LIT_FLOOR: f32 = 2.5;

/// The lit plate behind the cassette: the lamp's colour faded this far into
/// the band - a glow, not a second button.
const CASSETTE_GLOW_T: f32 = 0.45;

/// Lit body kept above and below each hub, in points - one pixel at 1x.
const HUB_CLEAR: f32 = 1.0;
/// The thinnest hub ring, for the hubs too small for a proper one.
const HUB_HAIRLINE: f32 = 0.75;

/// The cassette body's corner radius.
const BODY_CR: f32 = 2.0;

/// Below this side the plate's edge is left hard: a feather of a pixel only
/// greys the edge of something this small.
const CASSETTE_FEATHER_FROM: f32 = 12.0;
const CASSETTE_FEATHER_MAX: f32 = 3.0;

/// **The cassette's geometry, one answer for the painter and the tests.**
///
/// It was drawn with a 1.5-point rim and hubs of 22% of the body's height at
/// every size - which at 10.5 points (the 100% window on a 1440-point laptop)
/// left six or eight of the body's sixty-odd pixels showing amber: the rest
/// was rim, hub hole and hairline. The owner's screen measured the icon's
/// centre at 108,86,71 at rest and 115,92,72 during a take, a contrast of
/// 1.09:1 - the take button did not "glow amber" at the size it is drawn.
/// Colour could not fix that; the pixels had to be amber first.
#[derive(Debug, Clone, Copy)]
struct Cassette {
    /// The lit plate, inside the button's own square.
    plate: Rect,
    plate_cr: f32,
    /// The plate's soft edge, which also stays inside the square.
    feather: f32,
    body: Rect,
    rim: f32,
    hubs: [Pos2; 2],
    hub: f32,
    /// The hub's own ring, drawn INSIDE the hub: a hairline on the smallest
    /// hubs, and none only where no hub is drawn.
    ring: f32,
}

fn cassette(r: Rect) -> Cassette {
    let d = r.width().min(r.height());
    // The silhouette is unchanged.
    let body = Rect::from_center_size(r.center(), Vec2::new(r.width() * 0.98, r.height() * 0.62));
    // 1.05 points at the smallest button, 1.5 again from 15 points up.
    let rim = (d * 0.1).clamp(1.0, 1.5);
    // Never so big that a hub eats the lit body above and below it.
    let hub = (body.height() * 0.22)
        .min(body.height() * 0.5 - rim - HUB_CLEAR)
        .max(0.0);
    // Below 1.5 points a hairline, not nothing: on the light bands the
    // resting hole is the band's own colour against a body 1.5:1 from it,
    // and with no edge the owner's 10.5-point cassette read as a plain bar.
    let ring = if hub >= 1.5 {
        (hub * 0.4).min(1.0)
    } else if hub > 0.5 {
        HUB_HAIRLINE.min(hub)
    } else {
        0.0
    };
    let dx = body.width() * 0.24;
    let feather = ((d - CASSETTE_FEATHER_FROM) * 0.5).clamp(0.0, CASSETTE_FEATHER_MAX);
    let c = body.center();
    Cassette {
        // A centred feather ends exactly at the square's edge.
        plate: r.shrink(feather * 0.5),
        // Concentric with the body's own corners.
        plate_cr: BODY_CR + (r.height() - body.height()) * 0.5,
        feather,
        body,
        rim,
        hubs: [Pos2::new(c.x - dx, c.y), Pos2::new(c.x + dx, c.y)],
        hub,
        ring,
    }
}

/// The cassette's lamp colour on this band. See [`CASSETTE_LIT_FLOOR`].
fn cassette_lit(bg: Color32) -> Color32 {
    distinct_surface_at(TAPE_AMBER, bg, CASSETTE_LIT_FLOOR)
}

/// **The cassette: dimmed at rest, and LIT - a glowing plate under it -
/// while a take is capturing.** The play triangle's grammar, and the scope's
/// glow-under, core-over idiom. The cassette is the only control that reaches
/// a take, so whether one is running has to read at the smallest size the
/// row is drawn. See [`Cassette`].
fn draw_capture(painter: &Painter, r: Rect, active: bool, p: &Palette) {
    if !r.is_positive() {
        return;
    }
    let g = cassette(r);
    let lit = cassette_lit(p.bg);
    let glow = toward(lit, p.bg, CASSETTE_GLOW_T);
    if active {
        // The button's own square, lit, first - so the cassette sits on it.
        painter.add(
            egui::epaint::RectShape::filled(g.plate, g.plate_cr, glow).with_blur_width(g.feather),
        );
    }
    let body = if active { lit } else { toward(TAPE_AMBER, p.bg, 0.4) };
    painter.rect_filled(g.body, BODY_CR, body);
    painter.rect_stroke(g.body, BODY_CR, Stroke::new(g.rim, p.line), StrokeKind::Inside);
    if g.hub > 0.5 {
        // A window in the shell shows what is behind the tape: the lit plate
        // while a take runs, the band at rest.
        let hole = if active { glow } else { p.bg };
        for c in g.hubs {
            painter.circle_filled(c, g.hub, hole);
            if g.ring > 0.0 {
                painter.circle_stroke(c, g.hub - g.ring * 0.5, Stroke::new(g.ring, p.line));
            }
        }
    }
}

/// The folder: a tab and a body, the sign every file manager uses —
/// dimmed like the resting transport, in the band's own ink.
fn draw_folder(painter: &Painter, r: Rect, p: &Palette) {
    if !r.is_positive() {
        return;
    }
    let body = Rect::from_min_max(
        Pos2::new(r.left() + r.width() * 0.06, r.top() + r.height() * 0.30),
        Pos2::new(r.right() - r.width() * 0.06, r.bottom() - r.height() * 0.14),
    );
    let tab = Rect::from_min_max(
        Pos2::new(body.left(), r.top() + r.height() * 0.16),
        Pos2::new(body.left() + body.width() * 0.45, body.top() + 1.0),
    );
    let col = toward(p.ink, p.bg, 0.35);
    painter.rect_filled(tab, 1.0, col);
    painter.rect_filled(body, 1.5, col);
    painter.rect_stroke(body, 1.5, Stroke::new(1.2_f32, p.line), StrokeKind::Inside);
}

// ── the monitor ────────────────────────────────────────────────────────────

fn draw_monitor(painter: &Painter, l: &Layout, view: &RecorderView<'_>, p: &Palette) {
    // The timeline, where the slot selector and the fader rows were: the
    // instruments live on the desk now and the click, the input and the
    // backing track are channels there, each with a real fader. What this
    // band shows instead is WHEN.
    draw_timecode_cluster(painter, l.lcd, view, p, l.timecode.is_positive());
    draw_timeline(painter, l.timeline, view, p);
    // The ×, only when it can be pressed: a file loaded and no take rolling.
    if l.timeline_close.is_positive() && l.timeline_loaded && !l.rolling {
        let c = l.timeline_close.center();
        let arm = l.timeline_close.width().min(l.timeline_close.height()) * 0.24;
        let ink = Color32::from_rgba_unmultiplied(0xff, 0xf4, 0xe0, 140);
        let w = (arm * 0.5).clamp(1.0, 1.8);
        painter.line_segment(
            [Pos2::new(c.x - arm, c.y - arm), Pos2::new(c.x + arm, c.y + arm)],
            Stroke::new(w, ink),
        );
        painter.line_segment(
            [Pos2::new(c.x - arm, c.y + arm), Pos2::new(c.x + arm, c.y - arm)],
            Stroke::new(w, ink),
        );
    }
    // The loudness column, left of the master: the ladder, its targets, and
    // the varispeed dial at its foot.
    draw_lufs(painter, l, view, p);
    if l.vari.is_positive() {
        // **The size the grid prints its names at**, measured off the
        // LIMITER — the longest of the eight, so it is the one that sets the
        // row's size. Asking the cell it is actually drawn in means the two
        // rows of words cannot end up a point apart.
        let dial_label = {
            let cell = l.cell(Fx::Limiter);
            knob_face(cell).map_or(KNOB_LABEL.1, |f| {
                let bottom = Rect::from_min_max(
                    Pos2::new(cell.left(), cell.bottom() - f.label_h),
                    cell.max,
                );
                fit_text(bottom, Fx::Limiter.title(), f.label_h * 0.9)
            })
        };
        draw_varispeed(
            painter,
            l.vari,
            l.vari_label,
            view.varispeed,
            view.turning == Some(NumField::Varispeed),
            dial_label,
            p,
        );
    }
    // The master: what leaves, what the limiter is taking off, and the one
    // control that changes it.
    draw_master(painter, l, view, p);
    if l.master_knob.is_positive() {
        let field = NumField::Master;
        let g = view.gains.master;
        draw_knob(
            painter,
            l.master_knob,
            &Knob {
                value: gain_to_fader(g),
                label: "MASTER",
                typing: typing_for(view, field),
                turning: view.turning == Some(field),
                reading: gain_text(g),
                cap: p.master,
                dim: false,
            },
            p,
        );
    }
    // **All TEN, wherever each one lives.** Eight in the grid under the
    // meters — four sends, then the filters and the two assignable slots —
    // and the gain and the limiter at the foot of the loudness column.
    // `Layout::cell` is the one place that knows which is which.
    for fx in Fx::ALL {
        let field = NumField::Fx(fx);
        let v = view.fx.get(fx);
        draw_knob(
            painter,
            l.cell(fx),
            &Knob {
                value: v,
                label: fx.title(),
                typing: typing_for(view, field),
                turning: view.turning == Some(field),
                reading: knob_reading(view.fx_units[fx.index()], v),
                cap: fx.cap_themed(p),
                dim: fx == Fx::Limiter && view.limiter_off,
            },
            p,
        );
    }
}

/// The inline timeline: the backing track's waveform with the playhead in it.
///
/// The whole rectangle is the time axis — 0:00 at its left edge, the file's
/// end at its right — so the click that produces [`Hit::SeekTimeline`] and the
/// playhead drawn from [`fraction_of`] are exact inverses by construction.
/// Empty, it is an offer: click to import. It is the first face of the
/// arrangement, drawn as one lane of a thing that will one day show several.
fn draw_timeline(painter: &Painter, r: Rect, view: &RecorderView<'_>, p: &Palette) {
    if !r.is_positive() {
        return;
    }
    let track = view.track;
    painter.rect_filled(r, 2.0, METER_FACE);
    if track.wave.is_empty() && view.scrub.blocks.is_empty() && view.scrub.viewport.is_none() {
        // **The arrangement's empty field.** Eight faint lanes with nothing
        // standing on them — the same face the overview wears, minus the
        // clips. The import invitation died with the backing track: audio
        // arrives by dropping it on a lane, and an empty box that begs for
        // a file it can no longer mean is worse than a quiet one.
        let row_h = r.height() / crate::recorder::CHANNELS as f32;
        for i in 0..crate::recorder::CHANNELS {
            if i % 2 == 1 {
                let y = r.top() + row_h * i as f32;
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(r.left(), y),
                        Pos2::new(r.right(), y + row_h),
                    ),
                    0.0,
                    Color32::from_rgba_unmultiplied(0xff, 0xf4, 0xe0, 8),
                );
            }
        }
        // A file that imported but would not decode still says its name —
        // `TrackInfo::error` carries the why in the status line.
        if !track.is_empty() {
            let size = fit_text(r, &track.name, r.height() * 0.14);
            if size >= MIN_TEXT {
                painter.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    &track.name,
                    font_light(size),
                    p.faint,
                );
            }
        }
        return;
    }
    // **The ARRANGEMENT overview, when there is a document to show.** Eight
    // thin lanes, every clip a block in its channel's colour, the played
    // half of the strip carrying the same lit/dim boundary the wave mode
    // has always had. This is Ableton's overview strip on Tangent's desk;
    // the single wave below remains the face for hosts with no document.
    if !view.scrub.blocks.is_empty() || view.scrub.viewport.is_some() {
        let lanes = crate::recorder::CHANNELS as f32;
        let row_h = r.height() / lanes;
        // Faint stripes, so eight rows read as eight even where no clip
        // stands — the arrangement's own lane field in miniature.
        for i in 0..crate::recorder::CHANNELS {
            if i % 2 == 1 {
                let y = r.top() + row_h * i as f32;
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(r.left(), y),
                        Pos2::new(r.right(), y + row_h),
                    ),
                    0.0,
                    Color32::from_rgba_unmultiplied(0xff, 0xf4, 0xe0, 8),
                );
            }
        }
        let total = if view.scrub.domain_s > 0.0 {
            view.scrub.domain_s
        } else {
            track.seconds
        };
        let played = fraction_of(total, view.position_s).unwrap_or(0.0);
        for (lane, f0, fl, color) in &view.scrub.blocks {
            let y0 = r.top() + row_h * (*lane).min(crate::recorder::CHANNELS - 1) as f32;
            let x0 = r.left() + r.width() * f0.clamp(0.0, 1.0);
            let x1 = r.left() + r.width() * (f0 + fl).clamp(0.0, 1.0);
            let block = Rect::from_min_max(
                Pos2::new(x0, y0 + 1.0),
                Pos2::new(x1.max(x0 + 1.5), y0 + row_h - 1.0),
            );
            if !block.is_positive() {
                continue;
            }
            let fill = if *color != 0 {
                let (cr, cg, cb) = crate::mixer_panel::STRIP_COLORS
                    [(*color).min(crate::mixer_panel::STRIP_COLORS.len() - 1)];
                Color32::from_rgb(cr, cg, cb)
            } else {
                Color32::from_rgb(0x2E, 0x6B, 0x46)
            };
            painter.rect_filled(block, 1.5, fill);
        }
        // The unplayed half dims, so the strip carries the position the way
        // the wave always did.
        let px = r.left() + r.width() * played.clamp(0.0, 1.0);
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(px, r.top()), r.max),
            0.0,
            Color32::from_black_alpha(70),
        );
        // The arrangement's window, where a hand can pick it up. Drawn from
        // the same helper the app's pan gesture reads, so the grab and the
        // picture cannot drift.
        if view.scrub.viewport.is_some() {
            let vr = viewport_in(r, view);
            if vr.is_positive() {
                painter.rect_stroke(
                    vr,
                    2.0,
                    Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(0xff, 0xf4, 0xe0, 200)),
                    StrokeKind::Inside,
                );
            }
        }
        draw_timeline_captions(painter, r, view, p);
        draw_timeline_playhead(painter, r, view);
        return;
    }
    let mid = r.center().y;
    let half = r.height() * 0.40;
    // One column per point, sampled from the envelope — a decimation, not a
    // stretch. **Played is lit, unplayed is dimmed**: the boundary between
    // the two IS the playhead, so the picture carries the position even
    // before the line over it is found.
    let played = fraction_of(track.seconds, view.position_s).unwrap_or(0.0);
    let cols = r.width().max(1.0) as usize;
    for i in 0..cols {
        let t = i as f32 / cols as f32;
        let v = track.wave[(t * track.wave.len() as f32) as usize % track.wave.len()];
        let x = r.left() + i as f32;
        let ink = if t <= played {
            Color32::from_rgb(0x5f, 0xc9, 0x8a)
        } else {
            Color32::from_rgba_unmultiplied(0x5f, 0xc9, 0x8a, 96)
        };
        let h = (v * half).max(0.5);
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(x, mid - h), Pos2::new(x + 1.0, mid + h)),
            0.0,
            ink,
        );
    }
    draw_timeline_captions(painter, r, view, p);
    draw_timeline_playhead(painter, r, view);
}

/// The file's name, small over the top-left of the recess, and **the
/// playhead against the length over the top-right** — the number the
/// popup's clock used to carry, kept because it is the whole promise of
/// the locate: click at 1:24 and something READS 1:24 before anything is
/// pressed. Captions on the picture, not rows of their own. Shared by the
/// wave face and the arrangement overview, so the clock cannot move when
/// the picture changes mode.
fn draw_timeline_captions(painter: &Painter, r: Rect, view: &RecorderView<'_>, p: &Palette) {
    let track = view.track;
    let cap_h = (r.height() * 0.18).min(14.0);
    let clock = format!("{} / {}", time_text(view.position_s), time_text(track.seconds));
    let size = fit_text(
        Rect::from_min_size(r.min, Vec2::new(r.width() * 0.42, cap_h)),
        &clock,
        cap_h * 0.8,
    );
    if size >= MIN_TEXT {
        painter.text(
            Pos2::new(r.left() + 4.0, r.top() + 2.0),
            Align2::LEFT_TOP,
            &track.name,
            font_light(size),
            p.faint,
        );
        painter.text(
            // Short of the corner: the × that unloads the file lives there
            // (drawn by the caller, from the layout's own rect), and a clock
            // that moved when a take started would be worse than the gap.
            Pos2::new(r.right() - 20.0, r.top() + 2.0),
            Align2::RIGHT_TOP,
            &clock,
            font_light(size),
            // The number somebody watches, a shade brighter than the label.
            p.ink,
        );
    }
}

/// The playhead, over the top: ivory at rest, the record button's own red
/// while the transport moves, with a small cap so it reads as a marker
/// rather than a scratch on the picture.
fn draw_timeline_playhead(painter: &Painter, r: Rect, view: &RecorderView<'_>) {
    let total = if view.scrub.domain_s > 0.0 {
        view.scrub.domain_s
    } else {
        view.track.seconds
    };
    if let Some(t) = fraction_of(total, view.position_s) {
        let x = r.left() + t * r.width();
        let ink = if view.playing || view.state.is_active() {
            Color32::from_rgb(0xE8, 0x3A, 0x4E)
        } else {
            Color32::from_rgb(0xff, 0xf4, 0xe0)
        };
        painter.line_segment(
            [Pos2::new(x, r.top()), Pos2::new(x, r.bottom())],
            Stroke::new(1.5_f32, ink),
        );
        let cap = 4.0_f32.min(r.height() * 0.08);
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(x - cap, r.top()),
                Pos2::new(x + cap, r.top()),
                Pos2::new(x, r.top() + cap * 1.6),
            ],
            ink,
            Stroke::NONE,
        ));
    }
}

/// Where the arrangement's viewport stands ON the timeline, for the panel's
/// own drawing and the app's pan gesture — one function, so the grab and
/// the picture cannot drift. `NOTHING` when there is no viewport to steer.
fn viewport_in(r: Rect, view: &RecorderView<'_>) -> Rect {
    let Some((f0, fl)) = view.scrub.viewport else {
        return Rect::NOTHING;
    };
    if !r.is_positive() || fl <= 0.0 {
        return Rect::NOTHING;
    }
    Rect::from_min_max(
        Pos2::new(r.left() + r.width() * f0.clamp(0.0, 1.0), r.top()),
        Pos2::new(
            r.left() + r.width() * (f0 + fl).clamp(0.0, 1.0),
            r.bottom(),
        ),
    )
}

/// The viewport against the LAID-OUT timeline, for the app.
pub fn timeline_viewport_rect(rect: Rect, view: &RecorderView<'_>) -> Rect {
    let l = Layout::new(rect, view);
    if !l.timeline.is_positive() {
        return Rect::NOTHING;
    }
    viewport_in(l.timeline, view)
}

/// The timeline's own rectangle, for gestures that map a pointer to it.
pub fn timeline_rect(rect: Rect, view: &RecorderView<'_>) -> Rect {
    Layout::new(rect, view).timeline
}

/// `seconds` as `bars| beats| thousandths` — the parts, joined.
pub fn bars_beats(seconds: f64, bpm: f64, sig: crate::recorder::TimeSignature) -> String {
    let (b, bt, th) = bars_beats_parts(seconds, bpm, sig);
    format!("{b}| {bt}| {th:03}")
}

/// `seconds` as its three musical parts: bar, beat (both from one), and
/// thousandths of a beat.
pub fn bars_beats_parts(
    seconds: f64,
    bpm: f64,
    sig: crate::recorder::TimeSignature,
) -> (u64, u64, u64) {
    let unit = f64::from(sig.unit.max(1));
    let per_beat = (60.0 / bpm.clamp(20.0, 400.0)) * (4.0 / unit);
    let beats = seconds.max(0.0) / per_beat.max(1.0e-9);
    let per_bar = f64::from(sig.beats.max(1));
    let bar = (beats / per_bar) as u64;
    let in_bar = beats - bar as f64 * per_bar;
    let beat = (in_bar as u64).min(sig.beats.max(1) as u64 - 1);
    let thou = (((in_bar - beat as f64) * 1000.0) as u64).min(999);
    (bar + 1, beat + 1, thou)
}

/// The rows and the PART CELLS of the LCD's right half: three rows —
/// The REAPER cluster, whole-box edition: the big musical position with
/// minutes:seconds under it on the left, and the VIEWPORT's Start / End /
/// Length on the right — live numbers whether or not a single clip
/// exists, every part draggable by the unit it shows.
fn draw_timecode_cluster(
    painter: &Painter,
    r: Rect,
    view: &RecorderView<'_>,
    p: &Palette,
    glass_taken: bool,
) {
    if !r.is_positive() {
        return;
    }
    painter.rect_filled(r, 2.0, p.lcd_face);
    painter.rect_stroke(r, 2.0, Stroke::new(1.0_f32, p.line), StrokeKind::Inside);
    // The LEFT HALF carries the whole pre-scope cluster now, STACKED: the
    // big musical position with the clock beside it on top, the viewport's
    // Start/End/Length rows under them — every part still draggable. The
    // scope keeps the right half.
    let split = lcd_split(r);
    let half = slice_h(r, 0.02, split);
    let main = slice_v(half, 0.02, 0.52);
    painter.rect_filled(
        rule_x(r, r.left() + r.width() * (split + 0.015)),
        0.0,
        p.line.gamma_multiply(0.5),
    );
    // **The LCD's own face**, and not the band's: this is a seven-segment
    // readout and egui's monospace is what makes the digits line up in a
    // column. Renamed from `font` because the band's own `font` helper was
    // retired in the un-bolding and a bulk rename walked straight over the
    // calls to this one — the clock quietly changed typeface for a build.
    let lcd_font = |size: f32| FontId::new(size, fonts::courier());
    // The big number: where the playhead stands, musically — as large as
    // the face allows, the number somebody reads from the bench.
    let (b, bt, th) = bars_beats_parts(view.position_s, view.tempo_bpm, view.time_signature);
    // **Drawn part by part, into the same cells the drag is hit-tested
    // against.**
    //
    // It used to be one centred string against three cells cut at fixed
    // fractions of the box. A centred string MOVES: "9| 1| 000" and
    // "128| 4| 000" put their beats digit in completely different places,
    // while the cut stayed at 44% either way. So the zone you had to grab to
    // ride the beats sat somewhere off to the right of the beats digit, and
    // where exactly depended on the bar number — which is the owner's report,
    // and is not a thing that can be tuned into agreement because the two
    // sides were computed from different quantities.
    //
    // Sharing `lcd_playhead_cells` between the drawing and the hit test makes
    // them the same geometry rather than two geometries that ought to match.
    // The Start/End/Length rows under this one already worked that way, which
    // is why they were never wrong. A fixed column per field is also what the
    // transport this imitates does: the digits stop shifting sideways as the
    // playhead runs.
    let big_cells = lcd_playhead_cells(r);
    let big_box = slice_h(main, 0.0, 0.66);
    let parts = [format!("{b}"), format!("{bt}"), format!("{th:03}")];
    // **Sized on the WIDEST the clock can ever read, not on what it says right
    // now.** Sizing to the current string makes the digits grow and shrink as
    // the bar number gains a digit, which is a clock that moves while you read
    // it. This is also what the cell fractions are cut from, so the two agree.
    let size = fit_text(big_box, "000|0|000", big_box.height() * 0.62);
    if size >= MIN_TEXT {
        // Half a monospace glyph: the room the separator stands in, between
        // the field that ends and the field that begins.
        let gap = size * 0.34;
        // **The separators sit ON the boundaries, and the digits hug them.**
        //
        // Drawing each field right-aligned inside its own cell and then the
        // separator after it put the bar between one field and the empty half
        // of the NEXT field's cell — so the marks read as underlining the
        // digits rather than dividing them, and the number stopped looking
        // like a number. Anchoring the two inner edges to the boundary and
        // letting the outer edges run to the cell walls puts the three fields
        // back into one continuous reading.
        for (part, (cell, word)) in big_cells.iter().zip(parts.iter()).enumerate() {
            let (at, align) = match part {
                0 => (
                    Pos2::new(cell.right() - gap, cell.center().y),
                    Align2::RIGHT_CENTER,
                ),
                1 => (cell.center(), Align2::CENTER_CENTER),
                _ => (
                    Pos2::new(cell.left() + gap, cell.center().y),
                    Align2::LEFT_CENTER,
                ),
            };
            painter.text(at, align, word, lcd_font(size), p.lcd);
            if part < 2 {
                painter.text(
                    Pos2::new(cell.right(), cell.center().y),
                    Align2::CENTER_CENTER,
                    "|",
                    lcd_font(size),
                    p.lcd.gamma_multiply(0.45),
                );
            }
        }
        let small_box = slice_h(main, 0.66, 1.0);
        let clock = time_text(view.position_s);
        let ssize = fit_text(small_box, &clock, small_box.height() * 0.34);
        if ssize >= MIN_TEXT {
            painter.text(
                small_box.center(),
                Align2::CENTER_CENTER,
                &clock,
                lcd_font(ssize),
                p.lcd.gamma_multiply(0.55),
            );
        }
    }
    // Start / End / Length of the VIEWPORT — the timeline's own numbers,
    // never a clip's, live even over an empty song — compressed beside the
    // clock at the owner's word, every part draggable by the unit it shows.
    let (win_s, win_l) = view.view_window;
    let rows = [("Start", win_s), ("End", win_s + win_l), ("Length", win_l)];
    let side = slice_v(half, 0.54, 0.98);
    let cells = lcd_part_cells(r);
    for (i, (label, secs)) in rows.iter().enumerate() {
        let row = slice_v(side, i as f32 / 3.0, (i as f32 + 1.0) / 3.0);
        let lab = slice_h(row, 0.0, 0.22);
        let lsize = fit_text(lab, label, row.height() * 0.5);
        if lsize >= MIN_TEXT {
            painter.text(
                Pos2::new(lab.left() + 2.0, lab.center().y),
                Align2::LEFT_CENTER,
                *label,
                lcd_font(lsize),
                p.lcd.gamma_multiply(0.45),
            );
        }
        // Start and End are POSITIONS and count from one; Length is a
        // DURATION and counts from zero — thirty bars long is 30, not 31.
        let (b, bt, th) = bars_beats_parts(*secs, view.tempo_bpm, view.time_signature);
        let (b, bt) = if i == 2 { (b - 1, bt - 1) } else { (b, bt) };
        let words = [format!("{b}"), format!("{bt}"), format!("{th:03}")];
        for (part, cell) in cells[i].iter().enumerate() {
            if !cell.is_positive() {
                continue;
            }
            let csize = fit_text(*cell, &words[part], cell.height() * 0.58);
            if csize >= MIN_TEXT {
                painter.text(
                    Pos2::new(cell.right() - 3.0, cell.center().y),
                    Align2::RIGHT_CENTER,
                    &words[part],
                    lcd_font(csize),
                    p.lcd,
                );
            }
            if part < 2 {
                painter.text(
                    Pos2::new(cell.right() - 0.5, cell.center().y),
                    Align2::LEFT_CENTER,
                    "|",
                    lcd_font(csize.max(8.0) * 0.9),
                    p.lcd.gamma_multiply(0.4),
                );
            }
        }
    }
    // **The right half is the OSCILLOSCOPE** — the sibling session's
    // widget, the owner's pick: the speakers' mix without the click, on
    // the same black glass the timecode is behind.
    //
    // **Unless a take's readout has the glass** - the count-in digit, the
    // take's clock, FINISHING. See `fill_rolling`.
    if !glass_taken {
        view.scope.draw(
            painter,
            lcd_scope_rect(r),
            &crate::scope::ScopeStyle::themed(p.lcd, p.lcd_face),
        );
    }
}

/// Start, End, Length of the arrangement's viewport — each value in three
/// draggable cells, in the CLUSTER's left half beside the clock.
pub fn lcd_part_cells(lcd: Rect) -> [[Rect; 3]; 3] {
    let half = slice_h(lcd, 0.02, lcd_split(lcd));
    let side = slice_v(half, 0.54, 0.98);
    std::array::from_fn(|row| {
        let r = slice_v(side, row as f32 / 3.0, (row as f32 + 1.0) / 3.0);
        // The label owns the left fifth; the value cells the rest.
        let v = slice_h(r, 0.22, 1.0);
        let cuts = [0.0, 0.42, 0.66, 1.0];
        std::array::from_fn(|part| slice_h(v, cuts[part], cuts[part + 1]))
    })
}

/// The PLAYHEAD's own three cells over the big number — bars, beats,
/// thousandths — draggable like everything else on this glass.
pub fn lcd_playhead_cells(lcd: Rect) -> [Rect; 3] {
    let half = slice_h(lcd, 0.02, lcd_split(lcd));
    let main = slice_v(half, 0.02, 0.52);
    let big = slice_h(main, 0.0, 0.66);
    // **Cut where the CHARACTERS fall.** The glass is monospaced, so the
    // string "000|0|000" is nine equal columns: three for the bars, one for a
    // separator, one for the beat, one more separator, three for the
    // thousandths. The field boundaries are therefore the middles of the two
    // separators, at three and a half and five and a half characters of nine —
    // which is what makes a drag zone land under the digits it edits and the
    // separator land between two fields rather than beside one.
    let cuts = [0.0, 3.5 / 9.0, 5.5 / 9.0, 1.0];
    std::array::from_fn(|part| slice_h(big, cuts[part], cuts[part + 1]))
}

/// Which LCD part a press landed on: `(row, part)` — rows 0..=2 are
/// Start/End/Length, row 3 is the PLAYHEAD itself.
pub fn lcd_part_at(rect: Rect, view: &RecorderView<'_>, pos: Pos2) -> Option<(u8, u8)> {
    let l = Layout::new(rect, view);
    if !l.lcd.is_positive() || !l.lcd.contains(pos) {
        return None;
    }
    for (row, cells) in lcd_part_cells(l.lcd).iter().enumerate() {
        for (part, cell) in cells.iter().enumerate() {
            if cell.is_positive() && cell.contains(pos) {
                return Some((row as u8, part as u8));
            }
        }
    }
    for (part, cell) in lcd_playhead_cells(l.lcd).iter().enumerate() {
        if cell.is_positive() && cell.contains(pos) {
            return Some((3, part as u8));
        }
    }
    None
}
// ── the instrument slots ───────────────────────────────────────────────────

/// A plain word in a button-shaped box.
///
/// [`draw_open_button`] with its lit state taken out, for the buttons that are
/// pressed rather than toggled. Sharing the chrome is what stops the band
/// growing a second thing that is nearly, but not quite, a button.
fn draw_word_button(painter: &Painter, r: Rect, choices: &[&str], p: &Palette) {
    draw_word_button_sized(painter, r, choices, 0.46, p);
}

/// As [`draw_word_button`], with the text a chosen fraction of the box.
///
/// The fraction is a parameter because one panel wanted to be quieter than the
/// rest rather than because two sizes are a design: the backing track's panel
/// is the widest thing this file draws, so its rows are the tallest, and text
/// sized as a fraction of them came out half again as big as the same control
/// anywhere else. Sizing text off its own box is right until one box is
/// unusually large.
fn draw_word_button_sized(
    painter: &Painter,
    r: Rect,
    choices: &[&str],
    factor: f32,
    p: &Palette,
) {
    if !r.is_positive() {
        return;
    }
    painter.rect_filled(r, 2.0, p.field);
    painter.rect_stroke(r, 2.0, Stroke::new(1.0_f32, p.line), StrokeKind::Inside);
    let (text, size) = fit_label(r, choices, r.height() * factor);
    if size >= MIN_TEXT {
        painter.text(r.center(), Align2::CENTER_CENTER, text, font_light(size), p.ink);
    }
}

/// **A close box: the button, and an X DRAWN rather than typed.**
///
/// It used to go through `draw_word_button_sized` with the letter "X", which
/// is shrunk to fit and then dropped entirely below `MIN_TEXT`. The corner
/// square is sized off a title band — `(title.height() * 0.52).min(body.width()
/// * 0.16)` — so on a narrow panel the glyph fell under five points and the
/// button drew as an empty square with no indication of what it was for: the
/// owner's "a blank square to close it".
///
/// Two crossing strokes have no floor to fall under and no font to be missing
/// from, which is the same reasoning `draw_tick` and `draw_resync` already
/// carry: this crate has no icon font, so its icons are geometry.
fn draw_close_x(painter: &Painter, r: Rect, p: &Palette) {
    if !r.is_positive() {
        return;
    }
    painter.rect_filled(r, 2.0, p.field);
    painter.rect_stroke(r, 2.0, Stroke::new(1.0_f32, p.line), StrokeKind::Inside);
    let Some((arms, w)) = close_x_arms(r) else {
        return;
    };
    let stroke = Stroke::new(w, p.ink);
    for arm in arms {
        painter.line_segment(arm, stroke);
    }
}

/// The two arms of a close box's X, and how thick to draw them.
///
/// Split out so the shape can be tested without a painter: the property that
/// matters is that a square of ANY size the layout can hand over comes back
/// with two diagonals inside it, which is exactly what the glyph version could
/// not promise. `None` only for a square with no room at all.
fn close_x_arms(r: Rect) -> Option<([[Pos2; 2]; 2], f32)> {
    if !r.is_positive() {
        return None;
    }
    // Inset far enough that the arms never touch the border, and never so far
    // that they vanish: a third of the side, but at most enough to leave a
    // quarter of it as the mark.
    let side = r.width().min(r.height());
    let inset = (side * 0.30).min(side * 0.5 - side * 0.125);
    let b = r.shrink(inset);
    if !b.is_positive() {
        return None;
    }
    let w = (side * 0.12).clamp(1.0, 2.0);
    Some((
        [
            [b.left_top(), b.right_bottom()],
            [b.left_bottom(), b.right_top()],
        ],
        w,
    ))
}

/// The longest of `choices` that draws at `nominal` without being shrunk.
///
/// Ordered longest first. The last is the fallback and is shrunk to fit like any
/// other text, so one button can say "OPEN WINDOW" in the main window and "OPEN"
/// in a 640-point detached one without either of them being a smudge. A pure
/// function of the rectangle, so the two layouts and the offscreen compositor
/// all pick the same word.
fn fit_label<'t>(r: Rect, choices: &[&'t str], nominal: f32) -> (&'t str, f32) {
    for c in choices {
        if fit_text(r, c, nominal) >= nominal {
            return (c, nominal);
        }
    }
    match choices.last() {
        Some(c) => (c, fit_text(r, c, nominal)),
        None => ("", 0.0),
    }
}

/// A colour faded `t` of the way into the background it sits on.
///
/// Toward the BACKGROUND rather than scaled toward black, which is what
/// [`shade`] does. Scaling would darken a faded icon on a cream band as well as
/// on a walnut one, and on the cream band darker is more prominent — the "off"
/// state would shout.
fn toward(c: Color32, bg: Color32, t: f32) -> Color32 {
    let f = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Color32::from_rgb(f(c.r(), bg.r()), f(c.g(), bg.g()), f(c.b(), bg.b()))
}

/// **Every pickable badge**: the settings byte and the name the picker shows.
///
/// APPEND-ONLY - the code is saved in every desk and every document, so a
/// reordered table would re-badge every session anybody has kept. 0 is
/// "auto": nothing was chosen and the strip draws for itself.
///
/// The sourced drawing set replaced the vector badges without moving a
/// code. 1 to 24 keep their MEANING and point at the nearest new drawing, so
/// a desk saved before the change still shows a drum kit where it showed a
/// drum kit. The four old codes that had no drawing in the new set went back
/// into the pool and are ordinary slots again; the rest of the new
/// instruments run on from 25. Every one of them is frozen from here.
///
/// This is the CODE table, not the picker's list - four codes share the
/// saxophone and the picker offers it once. See [`ICON_PICKS`].
pub const ICONS: &[(u8, &str)] = &[
    (0, "auto"),
    (1, "drum kit"),
    (2, "acoustic"),
    (3, "bass"),
    (4, "electric"),
    (5, "nylon"),
    (6, "synth"),
    (7, "condenser"),
    (8, "dynamic"),
    (9, "triangle"),
    (10, "guiro"),
    (11, "tambourine"),
    (12, "maracas"),
    (13, "accordion"),
    (14, "cabasa"),
    (15, "digital wave"),
    (16, "single coil"),
    // The old set drew four sizes of saxophone and the new one draws a
    // saxophone. 17 is the one the picker offers; 18, 19 and 20 keep their
    // names so the table still says what a saved desk meant by them, and
    // all four draw the same horn.
    (17, "sax"),
    (18, "alto sax"),
    (19, "tenor sax"),
    (20, "bari sax"),
    (21, "wave"),
    (22, "melodica"),
    (23, "amp"),
    (24, "line in"),
    (25, "flute"),
    (26, "harmonica"),
    (27, "harp"),
    (28, "instrument"),
    (29, "keyboard"),
    (30, "metronome"),
    (31, "piano"),
    (32, "sitar"),
    (33, "snare"),
    (34, "trombone"),
    (35, "trumpet"),
    (36, "tuba"),
    (37, "upright bass"),
    (38, "violin"),
    (39, "djembe"),
    (40, "folder"),
    (41, "kalimba"),
    // The owner's file is `mariba.svg`; the instrument is a marimba, and the
    // picker says what the instrument is. The slug stays his.
    (42, "marimba"),
    (43, "master"),
    (44, "vibraphone"),
    (45, "vibraslap"),
    (46, "xylophone"),
    (47, "xylophone 2"),
];

/// **What the picker offers**: every DRAWING once, rather than every code.
///
/// The two lists were the same thing until four codes came to share one
/// saxophone, and a picker built from [`ICONS`] would then show the same horn
/// four times in a row with four different words under it - which reads as
/// four instruments the user cannot tell apart, and picking any of them looks
/// like the picker got it wrong. Choosing one of them writes 17; the other
/// three are only ever arrived at by loading a desk that already held them,
/// and they draw the same horn when it does.
///
/// Same shape as `ICONS` on purpose, so the picker's loop is unchanged.
pub const ICON_PICKS: &[(u8, &str)] = &[
    (0, "auto"),
    (1, "drum kit"),
    (2, "acoustic"),
    (3, "bass"),
    (4, "electric"),
    (5, "nylon"),
    (6, "synth"),
    (7, "condenser"),
    (8, "dynamic"),
    (9, "triangle"),
    (10, "guiro"),
    (11, "tambourine"),
    (12, "maracas"),
    (13, "accordion"),
    (14, "cabasa"),
    (15, "digital wave"),
    (16, "single coil"),
    (17, "sax"),
    (21, "wave"),
    (22, "melodica"),
    (23, "amp"),
    (24, "line in"),
    (25, "flute"),
    (26, "harmonica"),
    (27, "harp"),
    (28, "instrument"),
    (29, "keyboard"),
    (30, "metronome"),
    (31, "piano"),
    (32, "sitar"),
    (33, "snare"),
    (34, "trombone"),
    (35, "trumpet"),
    (36, "tuba"),
    (37, "upright bass"),
    (38, "violin"),
    (39, "djembe"),
    (40, "folder"),
    (41, "kalimba"),
    // The owner's file is `mariba.svg`; the instrument is a marimba, and the
    // picker says what the instrument is. The slug stays his.
    (42, "marimba"),
    (43, "master"),
    (44, "vibraphone"),
    (45, "vibraslap"),
    (46, "xylophone"),
    (47, "xylophone 2"),
];

/// The highest badge code. What the clamp on loaded desks answers to.
pub const ICON_MAX: u8 = 47;

/// **What a track with no chosen badge draws.**
///
/// One drawing for every track, whatever is on it. This used to branch on the
/// channel's kind - a keyboard for MIDI, a microphone for audio, a waveform
/// for a clip lane - and the owner's call is that the split is gone: a track
/// is a track, and the badge is the user's to choose if they want a different
/// one. The buses are the exception and they say so where they are drawn.
pub(crate) const DEFAULT_TRACK_ICON: crate::icons::Icon = crate::icons::Icon::Wave;

/// The drawing a badge code stands for, if it stands for one.
///
/// The one place a code becomes a picture, so the mixer, the arrangement and
/// the picker cannot end up disagreeing about what a code looks like. Mirrors
/// [`ICONS`]; `every_badge_code_is_dense_and_draws` is what holds the two
/// together.
pub(crate) const fn glyph(code: u8) -> Option<crate::icons::Icon> {
    use crate::icons::Icon;
    Some(match code {
        1 => Icon::Drumset,
        2 => Icon::Acoustic,
        3 => Icon::EBass,
        4 => Icon::EgHumbucker,
        5 => Icon::Classical,
        6 => Icon::Synthesizer,
        7 => Icon::Condenser,
        8 => Icon::Dynamic,
        9 => Icon::Triangle,
        10 => Icon::Guiro,
        11 => Icon::Tambourine,
        12 => Icon::Maracas,
        13 => Icon::Accordion,
        14 => Icon::Cabasa,
        15 => Icon::DigitalWave,
        16 => Icon::EgSingleCoil,
        17..=20 => Icon::Saxophone,
        21 => Icon::Wave,
        22 => Icon::Melodica,
        23 => Icon::Amp,
        24 => Icon::LineIn,
        25 => Icon::Flute,
        26 => Icon::Harmonica,
        27 => Icon::Harp,
        28 => Icon::Instrument,
        29 => Icon::Keyboard,
        30 => Icon::Metronome,
        31 => Icon::Piano,
        32 => Icon::Sitar,
        33 => Icon::Snare,
        34 => Icon::Trombone,
        35 => Icon::Trumpet,
        36 => Icon::Tuba,
        37 => Icon::UprightBass,
        38 => Icon::Violin,
        // The second set. Appended, as every code in this table is: a number
        // is written into every desk and every document that used it.
        39 => Icon::Djembe,
        40 => Icon::Folder,
        41 => Icon::Kalimba,
        42 => Icon::Mariba,
        43 => Icon::Master,
        44 => Icon::Vibraphone,
        45 => Icon::Vibraslap,
        46 => Icon::Xylophone1,
        47 => Icon::Xylophone2,
        _ => return None,
    })
}

/// **Draw badge `code`.** Answers false for 0 and for anything unknown, so the
/// caller can fall back to whatever the strip draws for itself - one dispatch,
/// so the mixer, the arrangement and the picker can never disagree.
///
/// `_bg` is what the badge is being drawn ON. The vector badges needed it:
/// they cut their details out in the ground's own colour, so a metronome's
/// pendulum was a stroke of the strip's face and not a hole. A mask has no
/// details to cut - it is coverage, tinted with `ink` - so nothing reads it
/// any more. The parameter stays because the picker in `dialogs.rs` calls this
/// positionally and a signature change there is not this change's to make.
pub(crate) fn draw_badge(painter: &Painter, b: Rect, ink: Color32, _bg: Color32, code: u8) -> bool {
    match glyph(code) {
        Some(icon) => {
            crate::icons::draw(painter, b, ink, icon);
            true
        }
        None => false,
    }
}

/// What a control is showing while it is being typed into, if it is.
///
/// A borrow out of the view rather than a flag, so that the panel draws the
/// text the app is actually holding — a copy would be a second place for the
/// caret and the characters to disagree.
fn typing_for(view: &RecorderView<'_>, field: NumField) -> Option<String> {
    view.editing
        .filter(|e| e.field == field)
        .map(crate::recorder::NumEdit::display)
}

/// The caret inside a caption+value box, after `typed_chars` characters of the
/// value.
///
/// Shared by the take name and both SIG fields, the TAKE tier's and the AUDIO
/// tier's. They looked identical when they were copies, which is exactly when
/// copies start to drift: the AUDIO tier's SIG drew no caret at all.
fn draw_caption_caret(
    painter: &Painter,
    r: Rect,
    cap: &str,
    shown: &str,
    typed_chars: usize,
    p: &Palette,
) {
    if !r.is_positive() {
        return;
    }
    let inset = label_inset(r);
    let inner = Rect::from_min_max(
        Pos2::new(r.left() + inset, r.top()),
        Pos2::new(r.right() - inset, r.bottom()),
    );
    if !inner.is_positive() {
        return;
    }
    let size = fit_text(inner, &format!("{cap} {shown}"), inner.height() * 0.52);
    if size < MIN_TEXT {
        return;
    }
    // The caret goes after what has been TYPED, which is not what is drawn
    // when the field is empty and showing a placeholder.
    let before = cap.chars().count() + 1 + typed_chars;
    let x = inner.left() + size * ADV * before as f32;
    painter.line_segment(
        [
            Pos2::new(x, r.center().y - size * 0.5),
            Pos2::new(x, r.center().y + size * 0.5),
        ],
        Stroke::new(1.5_f32, p.ink),
    );
}

/// The bone the fader caps are moulded in, and the shadow under one.
///
/// **Fixed rather than taken from the palette**, and it is the one thing in
/// this band that does not follow the background colour. A cap is a physical
/// object sitting in a slot cut into the panel: it reads as bone on walnut and
/// as bone on ivory, because it is always outlined and it always sits in a dark
/// channel. A cap that restyled itself for a light background would be a
/// painted-on rectangle, which is exactly the "strip of colour" the owner
/// rejected in the first place.
pub(crate) const CAP_BONE: Color32 = Color32::from_rgb(0xE8, 0xDE, 0xC4);

/// The channel the cap runs in: black, and the same black in every view.
///
/// **From the owner's photograph of the real desk**, where the caps are ivory
/// and the slots they run in are cut black into the panel — not a shade of the
/// panel, which is what a slider tinted from `well` produced: a pale line with
/// a slightly-less-pale block on it, a diagram of a fader rather than a fader.
///
/// Fixed like [`CAP_BONE`] and [`VU_FACE`], and for the same reason: a fader is
/// hardware let into the panel, so it is the same object whatever colour the
/// panel around it is.
pub(crate) const CAP_SLOT: Color32 = Color32::from_rgb(0x14, 0x12, 0x0F);

/// The ridges moulded across the cap, a shade under it so they read as cuts.
pub(crate) const CAP_RIB: Color32 = Color32::from_rgb(0xB4, 0xA8, 0x8C);

/// Where the red zone starts. The last 6 dB, drawn always rather than only once
/// something has gone wrong, so the top of the scale means something before the
/// first take instead of after it.
///
/// **The arc is drawn from `VU_MARKS`, and this is what holds the table to
/// its meaning.** `the_vu_needle_points_at_the_mark_the_face_prints` asserts
/// the red mark and this number are the same place, and
/// `the_peak_lamp_does_not_light_on_a_limited_mix` asserts it is still -6
/// dBFS — so the printed face cannot drift off the decibel it claims. Three
/// doc comments and one painting comment name it as well.
#[allow(dead_code)]
const CLIP_ZONE: f32 = 0.501; // -6 dBFS

/// Where the peak LAMP lights: the last fraction of a decibel.
///
/// Distinct from [`CLIP_ZONE`], which is where the face goes red. The arc and
/// the lamp answer different questions — "the average is high" and "a sample
/// hit the ceiling" — and giving them one number made the lamp useless.
const CLIP_AT: f32 = 0.977; // -0.2 dBFS

/// The play button's green: the meter's own healthy colour, because "the
/// transport is moving" and "signal is flowing" are the same kind of good
/// news.
const PLAY_GREEN: Color32 = Color32::from_rgb(0x3D, 0xC0, 0x5A);

/// The VU face, and the ink printed on it.
///
/// Fixed, like [`CAP_BONE`] and for the same reason: a meter is an instrument
/// let into the panel, not a region of it. It is a lit cream card with black
/// printing behind glass whatever colour the panel around it is, which is what
/// makes it read as a thing rather than as a drawing.
const VU_FACE: Color32 = Color32::from_rgb(0xE3, 0xD9, 0xBD);
const VU_PRINT: Color32 = Color32::from_rgb(0x24, 0x20, 0x1A);

/// Where each printed mark sits along the arc, left to right.
///
/// **Not linear in decibels**, because a VU face is not: the bottom half of the
/// scale is squeezed into the first fifth of the sweep and the marks open out
/// towards 0. Copying the real spacing is most of what makes a drawn VU read as
/// a VU rather than as a dial with numbers on it.
///
/// `0` is aligned to -6 dBFS, which is where this band's red zone has always
/// started — so the red arc on the face means exactly what the red end of the
/// old bar meant, and a take metered to the old picture meters the same on this
/// one. See [`CLIP_ZONE`].
/// **Every mark carries its number**, the way the face on the desk does.
///
/// Three labels was the wrong answer to real crowding: the crowding came from
/// printing "-20" and "-10" — a minus sign in front of every number on the
/// left two thirds of a scale, eating the width the digits needed. A real VU
/// prints the numbers BARE and puts one `−` at the left end and one `+` at the
/// right, which is what `draw_vu` does, and ten of them fit with room over.
///
/// The positions are the reference face's, not a formula's: the bottom of a VU
/// scale is compressed and the top is not, and no smooth curve gets 20 and 10
/// as close as they really are while leaving 0 to +3 as open as it really is.
const VU_MARKS: [(f32, f32, bool); 10] = [
    (-20.0, 0.00, true),
    (-10.0, 0.20, true),
    (-7.0, 0.30, true),
    (-5.0, 0.38, true),
    (-3.0, 0.48, true),
    (-1.0, 0.58, true),
    (0.0, 0.64, true),
    (1.0, 0.76, true),
    (2.0, 0.87, true),
    (3.0, 1.00, true),
];

/// dBFS at 0 VU. The red zone's edge, unchanged from the bar meter.
const VU_ZERO_DBFS: f32 = -6.0;

/// Where a level sits along the arc, 0 at the left stop and 1 at +3.
///
/// Interpolated between the printed marks rather than computed, so the needle
/// and the scale it is read against cannot disagree — which is the one failure
/// that would make a beautiful meter lie.
fn vu_frac(level: f32) -> f32 {
    if level <= 0.0 {
        return 0.0;
    }
    let vu = 20.0 * level.log10() - VU_ZERO_DBFS;
    if vu <= VU_MARKS[0].0 {
        return 0.0;
    }
    for w in VU_MARKS.windows(2) {
        let ((a_vu, a_f, _), (b_vu, b_f, _)) = (w[0], w[1]);
        if vu <= b_vu {
            return a_f + (b_f - a_f) * (vu - a_vu) / (b_vu - a_vu);
        }
    }
    1.0
}

/// Half the needle's sweep, in radians — a shade over 90 degrees end to end,
/// which is what a real VU movement swings.
///
/// It is not a free choice: with the pivot near the bottom edge the radius can
/// be no more than the face is tall, so the sweep is the only thing left that
/// decides how much of the window the arc spans. At 35 degrees the arc covered
/// half the card and the meter read as a small dial adrift on a large blank.
/// A VU window's width over its height.
///
/// Measured off the ISA One on the owner's desk: the glass is a hair under
/// five by three. That much width is not decoration — it is what puts ten
/// numbers along the arc without them touching, which is the difference
/// between a scale you read and a scale you infer. The face is fitted to this
/// and CENTRED in whatever box it was given, rather than stretched to fill it.
const VU_ASPECT: f32 = 1.65;

/// Half the needle's sweep, in radians.
///
/// **±56°, measured off the reference face**, not the ±46° this had. The two
/// end ticks sit 275 and 200 pixels out from the hub, which is 54° and 58° off
/// vertical. A narrower sweep drew the same arc into the middle of the glass
/// and left a finger of empty card down each side — the face looked oversized
/// for its own scale, which is the tell that the geometry is invented.
const VU_SWEEP: f32 = 0.97;

/// The level meter: one analogue VU per channel, modelled on the one in a
/// Focusrite ISA One.
///
/// **A needle rather than a bar**, and the difference is not decoration. A bar
/// is read by its edge, which means reading a number off a scale you have to
/// look at; a needle is read by its ANGLE, which is a shape, and a shape is
/// something you can take in from a piano bench two metres away while your
/// hands are busy. It is also the picture every person who has stood in front
/// of a tape machine already knows how to read.
///
/// It is driven by RMS, which is what a VU is: an average-responding meter with
/// slow ballistics. Peak lives in the lamp beside the face, because peak is a
/// yes-or-no question — did anything get too close — and a needle answering two
/// questions at once answers neither.
///
/// The meter is live before arming, which is the entire point of it: "I
/// recorded silence" is a failure class that dies at the sight of a moving
/// needle. Nothing here is gated on the state.
fn draw_meter(painter: &Painter, r: Rect, m: Meters, p: &Palette) {
    if !r.is_positive() {
        return;
    }
    let one = [m.left];
    let two = [m.left, m.right];
    let faces: &[Level] = if m.mono { &one } else { &two };
    let n = faces.len() as f32;
    let gap = (r.height() * 0.10).min(8.0);
    // **The face has an aspect, and the box it is given does not.** A VU window
    // is about half again as wide as it is tall; stretched to fill a long thin
    // cell it becomes a card with a small arc adrift in the middle of it, which
    // is the one way to make a drawn meter look like a drawing. So the face
    // takes the height it is given and only as much width as that height
    // deserves.
    //
    // And the PAIR is centred, not each face in a share of the row. A share
    // apiece is what put two 65-point meters at either end of a 600-point strip
    // while a take was rolling, which reads as two instruments on opposite
    // walls rather than as the left and right of one signal.
    // **Whichever of the two runs out first sets BOTH.** Capping the width at
    // the height's worth and then drawing the face down the full height is how
    // a landscape meter came out portrait and cut off: the aspect was written
    // down, the height ignored it, and the arc ran off the top of a tall card.
    let cell = (r.width() - gap * (n - 1.0)) / n;
    let fh = r.height().min(cell / VU_ASPECT);
    let fw = fh * VU_ASPECT;
    if fw <= 0.0 || fh <= 0.0 {
        return;
    }
    let top = r.center().y - fh * 0.5;
    let mut left = r.center().x - (fw * n + gap * (n - 1.0)) * 0.5;
    for lv in faces {
        let face = Rect::from_min_size(Pos2::new(left, top), Vec2::new(fw, fh));
        draw_vu(painter, face, *lv, m.clipped, p);
        left += fw + gap;
    }
}

/// One VU face.
fn draw_vu(painter: &Painter, face: Rect, lv: Level, clipped: bool, p: &Palette) {
    let (fw, fh) = (face.width(), face.height());
    if fw < 12.0 || fh < 10.0 {
        return;
    }
    painter.rect_filled(face, 2.0, VU_FACE);

    // The pivot sits just inside the bottom edge so its hub is visible, the way
    // it is on a real movement. The radius is whichever of the two dimensions
    // runs out first, so a wide short face and a tall narrow one both get a
    // needle that stays on the card.
    // The hub sits just inside the bottom edge, the way the chrome half-moon
    // does on the real one, and the arc is as big as the shorter of the two
    // dimensions allows. The width term leaves a card's-width margin at each
    // end rather than two points, so the scale ends inside the glass.
    // **The hub is BELOW the glass**, the way a real movement's is: what you
    // see is a needle coming up out of the bottom edge, not a pointer pinned
    // to a dot sitting on the card. Everything it draws is clipped to the
    // face, so the part below the edge simply is not there.
    let pivot = Pos2::new(face.center().x, face.bottom() + fh * 0.05);
    let radius = (fh * 0.80).min(fw * 0.46 / VU_SWEEP.sin()).max(1.0);
    let at = |f: f32, rr: f32| {
        let a = (f - 0.5) * 2.0 * VU_SWEEP;
        Pos2::new(pivot.x + rr * a.sin(), pivot.y - rr * a.cos())
    };

    // The arc, with its last stretch in red. Two polylines rather than one, so
    // the red is the SCALE going red and not a band drawn over it.
    let arc = |from: f32, to: f32, colour: Color32, w: f32| {
        let pts: Vec<Pos2> = (0_u8..=16)
            .map(|k| at(from + (to - from) * f32::from(k) / 16.0, radius))
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(w, colour)));
    };
    let hair = (fh * 0.022).max(1.0);
    // By VALUE, not by index: the scale gained a mark and an index into it is
    // a thing that goes quietly wrong when it does.
    let red_from = VU_MARKS
        .iter()
        .find(|(vu, ..)| *vu >= 0.0)
        .map_or(1.0, |(_, f, _)| *f);
    arc(0.0, red_from, VU_PRINT, hair);
    arc(red_from, 1.0, p.rec, hair * 1.6);

    // The printed marks, each with its number above it.
    let label = fh >= 40.0 && fw >= 76.0;
    // **The reference face's own proportion.** Its digits are 38 pixels on a
    // 485-pixel face — 7.8% — and that is not a stylistic choice, it is what
    // ten numbers along an arc of that radius will physically take without
    // touching. At 13.5% they overlapped into a smear, which is the same
    // mistake as printing minus signs: spending width the arc does not have.
    let num = fh * 0.085;
    for (vu, f, _) in VU_MARKS {
        let inner = radius * 0.86;
        let colour = if f >= red_from { p.rec } else { VU_PRINT };
        painter.line_segment([at(f, inner), at(f, radius)], Stroke::new(hair, colour));
        if label {
            // **Bare.** No sign in front of any of them — the two signs live
            // at the ends of the scale, which is where a face that has to fit
            // ten numbers along an arc puts them.
            painter.text(
                // Just inside the ticks, which start at 0.86. Further in is
                // a shorter arc and therefore less room, which is the other
                // half of why they were colliding.
                at(f, radius * 0.80),
                Align2::CENTER_CENTER,
                &format!("{:.0}", vu.abs()),
                font_light(num),
                colour,
            );
        }
    }
    // **No signs outboard of the scale.** A minus and a plus used to sit
    // beyond the first and last marks, on the argument that they are what let
    // the numbers between them be bare. The owner's call is that they do not
    // matter — a VU face is read by where the needle sits against the red,
    // not by arithmetic — and they were the two smallest marks on the
    // busiest part of the band.
    // **The nameplate outlives the numbers.** "VU" is what says what the
    // face IS, and it fits long after ten digits along the arc stop
    // fitting — the owner watched it vanish the moment the window came
    // under full size. Its own floor is only "a face worth naming".
    if fh >= 16.0 {
        painter.text(
            Pos2::new(pivot.x, pivot.y - radius * 0.30),
            Align2::CENTER_CENTER,
            "VU",
            font_light((fh * 0.16).max(7.0)),
            VU_PRINT,
        );
    }

    // The needle. **`vu`, not `rms`** — see `Level::vu`: the window `rms` is
    // measured over is whatever arrived between two repaints, so a needle
    // driven off it follows the waveform rather than the level, and does it
    // differently depending on how busy the app is. Clamped to the stops
    // rather than allowed off the card: a needle that leaves the face is a
    // rendering fault, and the lamp is what says the signal went past the end.
    let n = at(vu_frac(lv.vu).clamp(0.0, 1.0), radius * 0.97);
    // Thin. A VU needle is a wire with a counterweight, and at 3.5% of the
    // face it was a pointer drawn by somebody who had not looked at one.
    let needle = painter.with_clip_rect(face);
    needle.line_segment([pivot, n], Stroke::new((fh * 0.018).max(1.0), VU_PRINT));
    needle.circle_filled(pivot, (fh * 0.085).max(1.5), VU_PRINT);

    // The peak lamp: a yes-or-no answer to a yes-or-no question, and the
    // question is "did this clip", not "is this loud".
    //
    // **It used to light at [`CLIP_ZONE`], which is SIX decibels down.** That
    // is where the red arc on the face starts, and the red arc is right — a VU
    // face goes red well before the end, because a needle that slow is telling
    // you where the average is and the peaks are already higher. But a LAMP is
    // read as an alarm, and an alarm that goes off six decibels early goes off
    // on every properly-levelled mix: the owner reported it lit solidly with
    // the limiter holding a true peak of -1, which is a mix that has not
    // clipped and cannot. Latched once something really has, because the
    // person it is for was looking at their hands when it happened.
    let d = (fh * 0.12).clamp(3.0, 8.0);
    let hot = clipped || lv.hold >= CLIP_AT;
    // Bottom right, where the card is empty. Top right is where the arc's own
    // last inch is, and a lamp there sits ON the red the lamp is about.
    let at_lamp = Pos2::new(face.right() - d * 1.5, face.bottom() - d * 1.5);
    painter.circle_filled(
        at_lamp,
        d * 0.5,
        if hot {
            p.rec
        } else {
            toward(p.rec, VU_FACE, 0.86)
        },
    );
    // A ring, so an unlit lamp reads as a lamp that is not lit rather than as a
    // mark on the card.
    painter.circle_stroke(at_lamp, d * 0.5, Stroke::new(1.0_f32, shade(VU_FACE, 0.45)));

    painter.rect_stroke(
        face,
        2.0,
        Stroke::new(1.0_f32, if clipped { p.rec } else { shade(VU_FACE, 0.35) }),
        StrokeKind::Inside,
    );
}

/// What the big readout says.
///
/// Its own function so that the one thing about it that is easy to get wrong —
/// what a count-in shows — can be asserted without a screen.
///
/// During a count-in it is the BEAT, because a count-in is a musical
/// instruction and the number the player needs is the one the click is playing.
/// A countdown in seconds against a click in beats is two clocks disagreeing in
/// front of the person trying to come in on time.
fn readout_text(state: RecordState, position_s: f64, elapsed_s: f64) -> String {
    match state {
        // Just the number. It said "3 OF 12" when the count was a running
        // total, and "3 OF 6" once it became a beat within the bar — at which
        // point the second half stopped carrying anything: the bar's length is
        // already on screen in the SIG cell, and nobody counting a band in has
        // ever said "of six". What the player needs is the digit they are
        // saying out loud, as large as the box will draw it.
        RecordState::CountIn { beat, .. } => beat.to_string(),
        RecordState::Finishing => "FINISHING".to_owned(),
        // **The TAKE's own time while one runs.** This clock is drawn on the
        // LCD's scope glass during a take, beside the LCD's own playhead in
        // bars and seconds. It read the playhead too,
        // which was the take's length only because a take used to drive the
        // transport; now the song can be stopped, located or played in any
        // order under a take, and a take clock that froze with the song
        // would say nothing about how long the camera has been rolling.
        RecordState::Rolling => timecode(elapsed_s),
        // At rest there is no clock in the row (see `fill_faders`); the
        // playhead is the only sensible thing for this to mean if one is.
        RecordState::Idle => timecode(position_s),
    }
}

fn draw_readout(painter: &Painter, l: &Layout, view: &RecorderView<'_>, p: &Palette) {
    let r = l.timecode;
    if !r.is_positive() {
        return;
    }
    let state = readout_state(view);
    let text = readout_text(state, view.position_s, view.elapsed_s);
    let size = readout_size(r, state, &text);
    if size < MIN_TEXT {
        return;
    }
    // In the LCD's own face, on its glass - the same digits as the clock
    // beside it.
    painter.text(
        r.center(),
        Align2::CENTER_CENTER,
        &text,
        FontId::new(size, fonts::courier()),
        readout_ink(state, p),
    );
}

/// What the glass is showing: the take's own state - except while a count-in
/// recorded INTO the take is still clicking, when it is that count's beat.
fn readout_state(view: &RecorderView<'_>) -> RecordState {
    match (view.state, view.count_beat) {
        (RecordState::Rolling, Some(beat)) => RecordState::CountIn {
            beat,
            of: u32::from(view.time_signature.beats.max(1)),
        },
        (state, _) => state,
    }
}

/// **Sized on the widest text of the readout's current FORMAT, never on its
/// digits**: the take's clock holds one size from 0:00 to 59:59 and can step
/// down once, at the hour, when `timecode` grows a field (on a glass too
/// narrow for the longer text); a count-in is sized on two digits whatever
/// beat it is on. A clock sized on its own digits would jump at 9:59 ->
/// 10:00, and a count-in at 9 -> 10 beats.
fn readout_size(r: Rect, state: RecordState, text: &str) -> f32 {
    let (widest, share) = match state {
        RecordState::CountIn { .. } => ("00", 0.80),
        RecordState::Finishing => ("FINISHING", 0.50),
        _ if text.chars().count() > 5 => ("0:00:00", 0.62),
        _ => ("00:00", 0.62),
    };
    fit_text(r, widest, r.height() * share)
}

/// The count-in digit in the record red - the number the player counts along
/// to - and every other readout in the LCD's own ink.
fn readout_ink(state: RecordState, p: &Palette) -> Color32 {
    match state {
        RecordState::CountIn { .. } => count_red(p.rec, p.lcd_face),
        _ => p.lcd,
    }
}

/// **The record red, stood off the LCD's face** far enough to read as a big
/// digit (3:1, the large-type floor). The raw red goes to 2.36:1 on
/// Sandstone's pale glass; the walk changes only faces like that one, and
/// keeps it red.
fn count_red(rec: Color32, face: Color32) -> Color32 {
    if contrast_ratio(rec, face) >= 3.0 {
        return rec;
    }
    let mut t = 0.05;
    while t <= 0.60 + 1e-6 {
        for toward_c in [Color32::BLACK, Color32::WHITE] {
            let c = toward(rec, toward_c, t);
            if contrast_ratio(c, face) >= 3.0 {
                return c;
            }
        }
        t += 0.05;
    }
    rec
}

// ── the destination ────────────────────────────────────────────────────────


/// What a take will actually produce, in four words.
///
/// Drawn as the Export button's own value so the dialog is somewhere you go to
/// CHANGE the answer rather than to find out what it is.
fn export_summary(spec: &ExportSpec) -> String {
    let mut parts: Vec<String> = Vec::new();
    if spec.audio {
        parts.push("wav".to_owned());
    }
    if spec.midi {
        parts.push("midi".to_owned());
    }
    match spec.encoder_count() {
        0 => {}
        1 => parts.push("1 video".to_owned()),
        n => parts.push(format!("{n} videos")),
    }
    if parts.is_empty() {
        // `ExportSpec::problem` refuses this at the moment of recording, but
        // the band still has to say what it is looking at.
        return "nothing".to_owned();
    }
    parts.join(" + ")
}

/// The count-in, in the fewest words that are still true.
///
/// Beats rather than bars, matching the menu: "8 beats" is what the click
/// plays, and the app has no time signature to turn that into bars with.
/// The count-in, in BARS — which is what a click cycles and what the menu
/// offers. It read beats, and beats stopped being the setting when the time
/// signature arrived: the cell showed a number the control no longer changed.
fn count_in_text(bars: u32) -> String {
    match bars {
        0 => "off".to_owned(),
        1 => "1 bar".to_owned(),
        n => format!("{n} bars"),
    }
}

/// A tempo without a trailing `.0` on the whole numbers, which is nearly all of
/// them, and without losing the half that 92.5 has.
fn tempo_text(bpm: f64) -> String {
    if (bpm - bpm.round()).abs() < 0.05 {
        format!("{:.0}", bpm.round())
    } else {
        format!("{bpm:.1}")
    }
}


/// **Rescan: a circular arrow.**
///
/// Drawn rather than set, like the cog beside it — this crate has no icon
/// font and a glyph from the text font would be the only one on the card.
/// An arc most of the way round with a head on the leading end, which is the
/// sign for "do that again" everywhere; the gap in the arc is what stops it
/// reading as a plain ring.
fn draw_resync(painter: &Painter, r: Rect, p: &Palette) {
    if !r.is_positive() {
        return;
    }
    let c = r.center();
    let rad = r.width().min(r.height()) * 0.5 * 0.82;
    // Same bail as the cog: under this the arc and its head merge and it is a
    // blot, which reads as a fault rather than as a small icon.
    if rad < 4.0 {
        return;
    }
    let w = (rad * 0.26).max(1.0);
    let stroke = Stroke::new(w, p.ink);
    // Three quarters of a turn, opening at the top right where the head goes.
    const START: f32 = -std::f32::consts::FRAC_PI_2 + 0.55;
    const SWEEP: f32 = std::f32::consts::TAU * 0.78;
    const STEPS: usize = 24;
    let at = |a: f32| c + Vec2::new(a.cos(), a.sin()) * rad;
    let pts: Vec<Pos2> = (0..=STEPS)
        .map(|i| at(START + SWEEP * i as f32 / STEPS as f32))
        .collect();
    painter.add(egui::Shape::line(pts, stroke));
    // The head, on the START end, pointing the way the arc runs — back
    // towards the gap. A filled triangle, because two more strokes at this
    // size would not resolve.
    let tip = at(START - 0.30);
    let base = at(START + 0.22);
    let out = (base - c).normalized();
    let head = rad * 0.46;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, base + out * head * 0.5, base - out * head * 0.5],
        p.ink,
        Stroke::NONE,
    ));
}

/// The settings cog: the way into the take-settings popup.
///
/// **Solid, with stubby teeth.** The first cut drew a thin ring and ran eight
/// long thin spokes from it out to the edge, which is a ship's wheel — the
/// owner said so and was right. A gear is a SOLID body whose teeth barely
/// stand proud of it: the teeth are short and wide, the hub is a hole punched
/// through the middle, and there is nothing in between for a spoke to be.
fn draw_cog(painter: &Painter, r: Rect, p: &Palette) {
    if !r.is_positive() {
        return;
    }
    // Below this the teeth merge into the body and it is a smudge of a circle,
    // so nothing is drawn at all rather than something that reads as a fault.
    // The layout asks the same question and does not seat a cog it would not
    // draw - see `cog_fits`.
    if !cog_fits(r) {
        return;
    }
    let c = r.center();
    let rad = r.width().min(r.height()) * 0.5;
    // The body: most of the circle. The teeth reach from inside it to the
    // edge, so they read as bumps ON it rather than as arms coming OFF it.
    let body = rad * 0.74;
    painter.circle_filled(c, body, p.ink);
    const TEETH: u8 = 8;
    // Wide enough that eight of them nearly meet at the rim, which is what a
    // gear looks like; a thin one is a spoke however short it is.
    let tooth = Stroke::new(rad * 0.34, p.ink);
    for i in 0..TEETH {
        let a = std::f32::consts::TAU * f32::from(i) / f32::from(TEETH);
        let (sn, cs) = a.sin_cos();
        let d = Vec2::new(sn, -cs);
        painter.line_segment([c + d * (body * 0.72), c + d * rad], tooth);
    }
    // The hub, punched through to the band behind it. Big enough to read as a
    // hole at sixteen points, which is the size this is actually drawn at.
    painter.circle_filled(c, rad * 0.30, p.bg);
}

// ── the effect knobs ────────────────────────────────────────────────────────

/// The knob's sweep, in radians, centred on straight up.
///
/// 270 degrees, which is what a panel knob with an end stop turns and what
/// leaves the gap at the bottom that tells you where the ends are.
const KNOB_SWEEP: f32 = std::f32::consts::TAU * 0.75;

/// Tick marks around the skirt. Eleven on the 388, and eleven here.
const KNOB_TICKS: usize = 11;

/// The tempo knob's cap.
///
/// A darkened orange against the effects' blue. They are the same control and
/// deliberately so; the colour is what says this one moves the CLICK rather
/// than the sound.
const TEMPO_CAP: Color32 = Color32::from_rgb(0xb5, 0x5c, 0x18);

/// The blue cap on a Tascam 388's effect returns.
///
/// A real colour off the reference photograph rather than the band's accent,
/// because the whole point of these two controls is that they look like a
/// piece of hardware and not like the rest of the panel.
const KNOB_CAP: Color32 = Color32::from_rgb(0x1c, 0x6f, 0xd6);

/// One knob's number, in whatever unit it is measured in.
///
/// **Hertz rounds the way a person reads a frequency**, not the way a float
/// prints one: whole numbers below a kilohertz, one decimal above, and no
/// trailing `.0` on a round figure. "1.2 kHz" and "480 Hz" are what somebody
/// would say out loud; "1200.0000 Hz" is what the number happens to be.
pub fn knob_reading(unit: KnobUnit, t: f32) -> String {
    let t = t.clamp(0.0, 1.0);
    match unit {
        KnobUnit::Percent => format!("{:.0}%", t * 100.0),
        KnobUnit::Decibels { low, high } => format!("{:.1} dB", low + (high - low) * t),
        KnobUnit::Hertz { low, high } => {
            let hz = low * (high / low).powf(t);
            if hz >= 1_000.0 {
                let k = hz / 1_000.0;
                if (k - k.round()).abs() < 0.05 {
                    format!("{k:.0} kHz")
                } else {
                    format!("{k:.1} kHz")
                }
            } else {
                format!("{hz:.0} Hz")
            }
        }
    }
}

/// The inverse of [`knob_reading`] for a typed number.
///
/// **Only the knob's own unit is accepted.** Somebody typing into a filter is
/// typing a frequency; taking "500" as half travel there would silently set
/// 2 kHz, which is the kind of wrong that looks like the box not working.
pub fn knob_typed(unit: KnobUnit, text: &str) -> Option<f32> {
    match unit {
        KnobUnit::Percent => crate::recorder::parse_percent(text),
        KnobUnit::Decibels { low, high } => {
            let db = text
                .trim()
                .trim_end_matches(|c: char| c.is_whitespace() || c == 'b' || c == 'B')
                .trim_end_matches(['d', 'D'])
                .trim()
                .parse::<f32>()
                .ok()?;
            if !db.is_finite() {
                return None;
            }
            Some(((db - low) / (high - low)).clamp(0.0, 1.0))
        }
        KnobUnit::Hertz { low, high } => {
            let t = text.trim().trim_end_matches(|c: char| {
                c.is_whitespace() || c == 'z' || c == 'Z' || c == 'h' || c == 'H'
            });
            let (t, scale) = match t.strip_suffix(['k', 'K']) {
                Some(rest) => (rest.trim_end(), 1_000.0),
                None => (t, 1.0),
            };
            let hz = t.trim().parse::<f32>().ok()? * scale;
            if !hz.is_finite() || hz <= 0.0 {
                return None;
            }
            // Where that frequency sits on this knob, clamped to its ends: a
            // 40 Hz low-pass is a real wish and the answer is "as low as this
            // one goes", not a refusal.
            Some(((hz / low).log10() / (high / low).log10()).clamp(0.0, 1.0))
        }
    }
}

// ── the master meter ────────────────────────────────────────────────────────
//
// A segmented ladder, not a smooth bar. The segments are the point: an LED
// ladder tells you WHERE you are at a glance because the lit count is a number
// you read without reading, and a continuous fill is a length you have to
// measure against a scale. It is also what the hardware this panel is dressed
// as actually had next to its VUs.

/// The bottom of the output ladder, in dBFS. The top is 0.
const MASTER_FLOOR_DB: f32 = -60.0;

/// Where the ladder changes colour, in dBFS.
///
/// **-18 and -6, which are not arbitrary.** -18 dBFS is the digital home of
/// the old +4 dBu nominal level, so green means "where the meter was designed
/// to sit"; -6 is a hand's width from the wall and is where somebody should
/// start caring.
const MASTER_AMBER_DB: f32 = -18.0;
const MASTER_RED_DB: f32 = -6.0;

/// Segments in each ladder.
const MASTER_SEGMENTS: usize = 22;

/// The ladder's colours, lit. Unlit is these at a tenth.
const LED_GREEN: Color32 = Color32::from_rgb(0x3d, 0xc0, 0x5a);
const LED_AMBER: Color32 = Color32::from_rgb(0xe0, 0xa8, 0x22);
const LED_RED: Color32 = Color32::from_rgb(0xd6, 0x38, 0x28);

/// The strip behind the scale that says the limiter is working.
///
/// **Not green.** Reduction is not a level and there is no amount of it that
/// is "good"; a colour that said otherwise would be a claim about somebody's
/// music.
const LED_GR: Color32 = Color32::from_rgb(0xe8, 0x8c, 0x2a);

/// The recess the ladders sit in.
const METER_FACE: Color32 = Color32::from_rgb(0x14, 0x12, 0x12);

/// A linear amplitude as a fraction of the ladder, 0 at the floor and 1 at 0 dB.
///
/// Linear in DECIBELS, which is the only scale on which a meter's top half is
/// not the only half that moves.
fn master_fraction(linear: f32) -> f32 {
    if linear <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * linear.log10();
    ((db - MASTER_FLOOR_DB) / -MASTER_FLOOR_DB).clamp(0.0, 1.0)
}

/// The colour a segment takes when it is lit, by where it sits on the ladder.
fn led_colour(fraction: f32) -> Color32 {
    let db = MASTER_FLOOR_DB + fraction * -MASTER_FLOOR_DB;
    if db >= MASTER_RED_DB {
        LED_RED
    } else if db >= MASTER_AMBER_DB {
        LED_AMBER
    } else {
        LED_GREEN
    }
}

/// One segmented ladder. `filled` is 0..=1 of its length, from the bottom.
fn draw_ladder(painter: &Painter, r: Rect, filled: f32, hold: Option<f32>) {
    if !r.is_positive() {
        return;
    }
    painter.rect_filled(r, 1.0, METER_FACE);
    let inner = r.shrink(1.0);
    if !inner.is_positive() {
        return;
    }
    let seg_h = inner.height() / MASTER_SEGMENTS as f32;
    // A hairline of face colour between segments, but only while there is
    // room for one: below about a point a gap is the whole segment.
    let gap = (seg_h * 0.22).min(1.5);
    for i in 0..MASTER_SEGMENTS {
        // How far up the ladder this segment's top edge is.
        let at = (i as f32 + 1.0) / MASTER_SEGMENTS as f32;
        let y = inner.bottom() - i as f32 * seg_h;
        let (top, bottom) = (y - seg_h + gap, y);
        if bottom <= top {
            continue;
        }
        let lit = filled >= at - 0.5 / MASTER_SEGMENTS as f32;
        let base = led_colour(at);
        let colour = if lit {
            base
        } else {
            // Unlit, not absent: an LED you can see is off is what tells you
            // how much headroom is left.
            Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 34)
        };
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(inner.left(), top), Pos2::new(inner.right(), bottom)),
            0.0,
            colour,
        );
    }
    // The peak hold: a bright line, drawn over whatever it lands on, because a
    // hold that is a lit segment is indistinguishable from the level itself.
    if let Some(h) = hold.filter(|h| *h > 0.0) {
        let y = inner.bottom() - h.clamp(0.0, 1.0) * inner.height();
        painter.line_segment(
            [Pos2::new(inner.left(), y), Pos2::new(inner.right(), y)],
            Stroke::new(1.5_f32, Color32::from_rgb(0xff, 0xf4, 0xe0)),
        );
    }
}

/// The master column: what leaves, how hard the limiter is working, and the
/// one control that changes it.
/// The varispeed dial: a line, a recessed well, and the dial in it.
///
/// **Not drawn like the eight beside it, on purpose.** Those are moulded
/// knobs on a panel; this one sits in a machined recess with a line above it,
/// which is how the tape machines that had a pitch control drew theirs — and
/// it is the reading somebody has to be able to take in at a glance from
/// across a room, because a varispeed left engaged ruins a take silently.
///
/// The face is the theme's `varispeed` role, which ships as the app's own
/// ivory. The WELL is derived from the band's own ground so the recess reads
/// as a recess whatever the band is coloured, exactly as `Palette::well` is;
/// and the ink on the face is measured against the face, never inherited —
/// see `surface_of`.
fn draw_varispeed(
    painter: &Painter,
    cell: Rect,
    // Where the WORD may print — see `Layout::vari_label`. Wider than the
    // cell, because the word is this dial's readout and the cell is one knob
    // across.
    word: Rect,
    v: crate::varispeed::Varispeed,
    turning: bool,
    // The size the KNOB GRID prints its names at. Passed in rather than
    // worked out here, because "the same size as every dial" is a fact about
    // those dials and this one cannot see them — and computing it twice is
    // how two rows of words end up a point apart.
    dial_label: f32,
    p: &Palette,
) {
    let Some(KnobFace { face, radius: rad, label_h }) = vari_face(cell, word) else {
        return;
    };
    let engaged = v.engaged();
    // **A LABEL, and no reading at all.** The owner's design call, and the
    // reason is the machine this dial is modelled on: you find the pitch by
    // ear, and a number beside it invites somebody to chase a value instead
    // of listening. It is the one control in the band with no readout — the
    // right-click range panel is where the numbers live.
    //
    // The full-width rule that used to sit above it went with the reading.
    // It was there to separate this bay from the row of knobs to its left,
    // and it separates nothing now that the dial has a column of its own —
    // it drew across the loudness column's ladder and read as a fault.
    // The strip the word is SIZED against is the cell's own share; where it
    // is DRAWN is the top of the cell. `label_h` is already the word plus its
    // clearance — see `vari_face`.
    let strip = (knob_face(cell).map_or(0.0, |k| k.label_h) * 2.0).max(KNOB_LABEL.1);
    let label = Rect::from_min_max(
        Pos2::new(word.left(), cell.top()),
        Pos2::new(word.right(), cell.top() + strip),
    );
    let _ = label_h;
    // **As big as the room either side allows.**
    //
    // It was `fit_text` against the CELL — one knob wide — so the nominal
    // `× 2` never bit: at 1300 the word came out at five points and was
    // rescued by `MIN_TEXT`. The dial has clear band on both sides and this
    // word IS the readout, so it takes that room; `Layout::fill_vari_label`
    // is what stops it one point short of its neighbours. The nominal is
    // raised to match, and the strip's own height still caps it so a tall
    // band cannot print a word taller than the line it sits on.
    let size = vari_word_size(label, strip, dial_label);
    if size >= MIN_TEXT {
        // Lit when ENGAGED: this is the one control whose resting state has
        // to be unmistakable from across a room.
        let ink = if turning || engaged {
            p.ink
        } else {
            p.ink.gamma_multiply(0.7)
        };
        painter.text(
            // Centred on the DIAL, not on the room it was allowed — the word
            // belongs to the circle under it, and a word centred in the air
            // would drift away from the thing it names.
            Pos2::new(cell.center().x, label.top()),
            Align2::CENTER_TOP,
            PITCH_LABEL,
            font_light(size),
            ink,
        );
    }
    let c = face.center();
    // **A leader from the dial up to the word.**
    //
    // It runs from just above the recess to just under the label, so the two
    // read as one control rather than as a word that happens to sit over a
    // knob. In the MARKERS' colour — the same ink as the `-` and `+` either
    // side of the dial — because those three marks are the same kind of
    // thing: the panel's own annotation of what this control does, as
    // against `Palette::line`, which is the colour of the panel's seams.
    {
        let rim = c.y - rad * RECESS_R;
        let gap = (size * 0.3).clamp(1.0, 3.0);
        let bottom = rim - gap;
        let top = label.bottom() + gap;
        if bottom - top >= 2.0 {
            // **The WORD's own ink**, not the panel's seam colour: the line
            // belongs to the label it hangs from, and drawn in `faint` it read
            // as a scratch on the panel rather than as part of the control.
            // It therefore lights and dims with the word, which is the one
            // thing on this dial that says whether it is engaged.
            let ink = if turning || engaged {
                p.ink
            } else {
                p.ink.gamma_multiply(0.7)
            };
            painter.line_segment(
                [Pos2::new(c.x, top), Pos2::new(c.x, bottom)],
                Stroke::new(1.2_f32, ink),
            );
        }
    }
    // **The recess**: a darker version of the band's own shade, which is what
    // `well` already is, taken one step further so the dial reads as sunk
    // into the panel rather than sitting on it.
    // **Deeper than it was** — the owner's word. Two things make a recess
    // read as one: how far the ground is taken from the panel around it, and
    // a rim that is lit on top and shadowed underneath. It had the first at
    // half strength and none of the second.
    let well = shade(p.well, if wants_light_ink(p.bg) { 0.66 } else { 0.88 });
    painter.circle_filled(c, rad * RECESS_R, well);
    // The rim: a dark arc under the top edge and a light one over the
    // bottom, which is what a hole in a panel looks like with the light
    // coming from above.
    painter.circle_stroke(c, rad * RECESS_R, Stroke::new(1.0_f32, p.line));
    for (from, to, tint) in [
        (0.62_f32, 1.20, recess_bevel(well, false)),
        (1.20, 1.78, recess_bevel(well, true)),
    ] {
        let steps = 12;
        let pts: Vec<Pos2> = (0..=steps)
            .map(|k| {
                let a = std::f32::consts::PI * (from + (to - from) * k as f32 / steps as f32);
                c + Vec2::new(a.cos(), -a.sin()) * (rad * RECESS_R - 0.6)
            })
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(1.6_f32, tint)));
    }
    // The dial face, in the theme's own colour for it.
    let dial = surface_of(p.varispeed);
    painter.circle_filled(c, rad, dial.bg);
    // **No detent mark at twelve o'clock.** There was one — a stroke on
    // the recess above the dial — and with the leader coming down from the
    // word to the same point it read as one line broken by the rim. The
    // leader already says where twelve o'clock is; a second mark under it
    // was the same information drawn twice.
    //
    // The slot, which IS the pointer: across the whole face and through the
    // centre, the same grammar as the moulded knobs beside it.
    let t = v.pos().clamp(0.0, 1.0);
    let angle = std::f32::consts::PI + (t - 0.5) * KNOB_SWEEP;
    let dir = Vec2::new(angle.sin(), -angle.cos());
    painter.line_segment(
        [c - dir * rad * 0.82, c + dir * rad * 0.82],
        Stroke::new((rad * 0.19).max(1.2), dial.ink),
    );
    // The two ends, marked the way a pitch control marks them.
    if rad >= 9.0 {
        let mark = |s: &str, side: f32| {
            // **Twice what they were.** They are the only thing on this dial
            // that says which way is faster — there is no reading — so they
            // are the reading, and at nine points they were a hint rather
            // than a label.
            let size = (rad * 0.88).min(18.0);
            if size >= MIN_TEXT {
                painter.text(
                    Pos2::new(c.x + side * rad * 1.34, c.y + rad * 1.16),
                    Align2::CENTER_CENTER,
                    s,
                    font_light(size),
                    p.faint,
                );
            }
        };
        mark("-", -1.0);
        mark("+", 1.0);
    }
}

/// How much taller a line of type is than the size it was asked for.
///
/// Measured against the one case that showed it: at the sizes a fullscreen
/// band gives `PITCH CONT`, the descenders reached about a fifth of the em
/// below the baseline and landed on the dial. A little clearance on top of
/// that is what keeps the two apart at every size without moving the word
/// somewhere it looks lost.
const WORD_LINE: f32 = 1.22;

/// **Where `PITCH CONT` actually prints**, ink and all.
///
/// The word is centred over the DIAL and may be wider than the dial's cell —
/// it is allowed the air either side (see `Layout::vari_label`) — so its right
/// edge is further right than the circle's. That edge is what the loudness
/// meter is centred against, because the eye measures a gap from the last
/// thing it can see and the last thing it can see here is a letter.
fn vari_word_ink(cell: Rect, word: Rect, strip: f32) -> Option<Rect> {
    if !cell.is_positive() || !word.is_positive() {
        return None;
    }
    let size = vari_word_size(word, strip, KNOB_LABEL.1);
    if size < MIN_TEXT {
        return None;
    }
    let half = PITCH_LABEL.chars().count() as f32 * ADV * size * 0.5;
    let mid = cell.center().x;
    Some(Rect::from_min_max(
        Pos2::new(mid - half, cell.top()),
        Pos2::new(mid + half, cell.top() + size),
    ))
}

/// The word over the pitch dial. The owner's exact wording.
///
/// **Above it, and the only one.** Every other dial in this band prints its
/// value above and its name below; this one has no value to print — see
/// `draw_varispeed` — so the name takes the top and the space under the dial
/// is the dial's own.
const PITCH_LABEL: &str = "PITCH CONT";

/// **How tall `PITCH CONT` actually prints**, given the room it was allowed.
///
/// Shared by the painter and by `vari_face`, because the dial sits directly
/// under the WORD and not under the strip the word was allotted. Those were
/// two different numbers: the strip is a fixed share of the cell (thirty
/// points at 1300) and the word, once it was free to use the air either side,
/// came out at twelve — so the dial hung twenty points below the thing it
/// belongs to. One function, so the gap cannot come back.
fn vari_word_size(word: Rect, strip: f32, dial_label: f32) -> f32 {
    fit_text(
        Rect::from_min_size(word.min, Vec2::new(word.width(), strip)),
        PITCH_LABEL,
        dial_label * 3.4,
    )
    .min(strip * 0.92)
    .max(MIN_TEXT)
}

/// The varispeed's own layout, which is not a knob's.
///
/// **It has no readout, so the strip `knob_face` keeps at the foot for one is
/// free** — and the word wants it. `PITCH CONT` is set at twice the size of
/// every other dial's name, because it is the one control in the band with no
/// number under it: the word IS the readout, and it has to be legible from
/// across a room, since a varispeed left engaged ruins a take silently.
///
/// Shared with the hit test — see `on_knob_circle` — so the circle a
/// right-click has to land on is the circle that was drawn.
fn vari_face(cell: Rect, word: Rect) -> Option<KnobFace> {
    let base = knob_face(cell)?;
    // What the word actually takes, not the strip it was allotted.
    let strip = (base.label_h * 2.0).max(KNOB_LABEL.1);
    let word = if word.is_positive() { word } else { cell };
    // **The word's LINE, not its point size.** A font's glyphs run taller
    // than the number they are asked for — ascenders and descenders live
    // outside the em — so reserving exactly `size` put the bottom of the
    // letters onto the top of the dial. Only visible at the largest sizes,
    // which is why it showed up in fullscreen and nowhere else.
    let label_h =
        vari_word_size(word, strip, KNOB_LABEL.1) * WORD_LINE + KNOB_TICK_ROOM;
    // **The same foot every other dial's cell keeps.**
    //
    // This ran to `cell.bottom()`, which put the circle lower than the region
    // that answered the hand — and the first fix went the wrong way round,
    // dragging the CONTROL down to meet the drawing instead of lifting the
    // drawing up to the control. The word sits above the dial; the dial
    // belongs under it, in the room `dial()` has always treated as the
    // control. So the face ends where that region ends.
    let face = Rect::from_min_max(
        Pos2::new(cell.left(), cell.top() + label_h),
        Pos2::new(cell.right(), cell.bottom() - base.label_h),
    );
    if !face.is_positive() {
        return None;
    }
    // The RECESS is this dial's outer edge, where a knob's is its tick skirt.
    let radius = face.width().min(face.height()) * 0.5 / RECESS_R;
    (radius >= KNOB_MIN_R).then_some(KnobFace {
        face,
        radius,
        label_h,
    })
}

/// The pitch dial's recess, as a multiple of the dial's own radius. Named
/// because the rule above the dial is placed against it — a rule measured
/// from anything else ends up inside the well the moment the cell changes
/// shape, which is what it used to do.
const RECESS_R: f32 = 1.24;

/// The lit or shadowed edge of a recess, held to the same strength on every
/// theme.
///
/// **A fixed multiplier is not a fixed amount of shading.** These arcs were
/// `well * 0.72` and `well * 1.28`, which on a DARK well is a step of almost
/// nothing and on a pale one is a step of plenty — so on Sandstone,
/// Centennial and Porcelain the bevel stopped reading as a bevel and became
/// two curved shapes drawn on the left of the dial, present on no other theme.
/// The owner spotted them as exactly that.
///
/// Measuring the step instead of scaling it makes the recess look the same
/// depth everywhere, which is what a recess is.
fn recess_bevel(well: Color32, lit: bool) -> Color32 {
    /// Just enough to see an edge on. A bevel is a hint of where the light is
    /// coming from, not a line somebody drew.
    const STEP: f32 = 1.12;
    // Fine steps, because the STOPPING POINT is the whole answer here: a
    // coarse walk overshoots by however much its last step was worth, and the
    // two edges then land at different depths for no reason but rounding.
    let walk = |target: Color32, want: f32| {
        let mut out = well;
        for _ in 0..96 {
            if contrast_ratio(out, well) >= want {
                break;
            }
            out = toward(out, target, 0.01);
        }
        out
    };
    // **The SHADOW decides how deep the bevel is, both sides.**
    //
    // Contrast against a near-black well is bounded — there is nowhere darker
    // to go — so on the dark palettes the shadowed edge simply cannot reach
    // `STEP`. Letting the lit edge reach it anyway makes a recess with one
    // bright rim and nothing opposite, which reads as a drawn arc rather than
    // as a hole. So the lit side is held to whatever the dark side managed.
    let reach = contrast_ratio(walk(Color32::BLACK, STEP), well).min(STEP);
    walk(if lit { Color32::WHITE } else { Color32::BLACK }, reach)
}

/// The loudness column: a continuous bar, the industry marks beside it, the
/// reading under it.
///
/// **A bar, not a ladder of LEDs.** The master's segments are right for a
/// peak meter, where what matters is how close the top is; loudness is a mean
/// that moves slowly and is READ AGAINST A NUMBER, so what it needs is a
/// scale you can line an edge up with.
fn draw_lufs(painter: &Painter, l: &Layout, view: &RecorderView<'_>, p: &Palette) {
    use crate::recorder::{LUFS_HOT, LUFS_RANGE};
    if !l.lufs_bar.is_positive() {
        return;
    }
    let bar = l.lufs_bar;
    // The well it stands in, and its ink measured against that well rather
    // than assumed from the theme — see `surface_of`. And the well itself
    // moved off the BAND first: on one palette the LCD role and the band's
    // ground are the same colour, and the ladder was an invisible rectangle.
    // See `distinct_surface`.
    let face = surface_of(distinct_surface(p.lcd_face, p.bg));
    painter.rect_filled(bar, 1.0, face.bg);
    let inner = bar.shrink(1.0);
    let at = |lufs: f32| {
        let (lo, hi) = LUFS_RANGE;
        inner.bottom() - ((lufs - lo) / (hi - lo)).clamp(0.0, 1.0) * inner.height()
    };
    // **Every mark gets its LINE; only some get their number.**
    //
    // −14, −16 and −18 are two LU apart, which on a short band is a few
    // pixels: printed unconditionally they overlap into an unreadable smear
    // exactly when the column is smallest. The lines still all draw — a
    // target you can line the bar up against is useful without its label,
    // and the numbers are in the order the standards matter, so what
    // survives on a short band is the loudest and the streaming one.
    let scale = surface_of(p.bg);
    // **A tick every two LU, on BOTH edges of the bar, with its number
    // beside it.**
    //
    // Two because the ladder's floor came up to −30 (see `LUFS_RANGE`) and
    // thirty-three LU across the column will carry that many; six was the
    // step a −40 floor could afford. Both edges because a level is read
    // against whichever side of the bar the eye is nearest, and the words
    // that used to own the other edge are gone.
    {
        const STEP: f32 = 2.0;
        let (lo, _) = LUFS_RANGE;
        let size = lufs_tick_size(l.lufs_bar.width());
        let mut last = f32::NEG_INFINITY;
        let mut lufs = 0.0_f32;
        while lufs >= lo {
            let y = at(lufs);
            if y >= inner.top() && y <= inner.bottom() {
                // **The regular scale marks the bar's LEFT.** Its numbers
                // are to the right, but the named targets already own the
                // right edge — two sets of ticks on one edge is a ladder
                // nobody can count.
                for (a, b) in [
                    (inner.left(), inner.left() + inner.width() * 0.18),
                    (inner.right() - inner.width() * 0.18, inner.right()),
                ] {
                    painter.line_segment(
                        [Pos2::new(a, y), Pos2::new(b, y)],
                        Stroke::new(1.0_f32, face.faint.gamma_multiply(0.7)),
                    );
                }
                if l.lufs_ticks.is_positive()
                    && size >= MIN_TEXT
                    && (y - last).abs() >= size * 1.25
                {
                    last = y;
                    painter.text(
                        Pos2::new(l.lufs_ticks.left() + 1.0, y),
                        Align2::LEFT_CENTER,
                        &crate::recorder::meter_number(lufs),
                        font_light(size),
                        scale.faint.gamma_multiply(0.85),
                    );
                }
            }
            lufs -= STEP;
        }
    }
    // **Two bars tied together — one stereo meter with a seam down it**,
    // which is the master's own shape and the owner's word for this one.
    // A single bar said what the programme was doing and nothing about
    // which side was doing it.
    //
    // The colour is decided by the PROGRAMME's reading and not by each
    // channel's, so the meter goes red as one thing: a pair where the left
    // is hot and the right is not is still one master that is too loud.
    // **Red past the loudest target anybody publishes.** A loudness meter's
    // whole job is the comparison against a number somebody else chose, and
    // by the time a MEAN reaches full scale the argument was lost a long way
    // back — so the bar turns at −9 rather than at 0.
    let hot = view.loudness.now() > LUFS_HOT;
    // **Both fills moved off the WELL, not off the band.** The bar is drawn
    // inside its own recess, so that is the surface it has to be seen
    // against — and on one palette the theme's red and that recess were two
    // points apart. See `distinct_surface_at`.
    const FILL_FLOOR: f32 = 2.0;
    let cool = distinct_surface_at(p.accent, face.bg, FILL_FLOOR);
    let warm = distinct_surface_at(p.rec, face.bg, FILL_FLOOR);
    let seam = (inner.width() * 0.06).clamp(1.0, 3.0);
    let half = (inner.width() - seam) * 0.5;
    for (i, f) in view.loudness.bars().into_iter().enumerate() {
        let Some(f) = f else { continue };
        let x0 = if i == 0 {
            inner.left()
        } else {
            inner.left() + half + seam
        };
        let top = inner.bottom() - f * inner.height();
        let fill = Rect::from_min_max(
            Pos2::new(x0, top),
            Pos2::new(x0 + half, inner.bottom()),
        );
        if fill.is_positive() {
            // **The bar goes red past the loudest target on the ladder**, and
            // not past 0: a loudness meter's whole job is the comparison
            // against a number somebody else chose, and by the time a MEAN
            // reaches full scale the argument was lost a long way back.
            painter.rect_filled(fill, 0.0, if hot { warm } else { cool });
        }
    }
    painter.rect_stroke(bar, 1.0, Stroke::new(1.0_f32, p.line), StrokeKind::Inside);
    // The reading, and the letter that says which window it is.
    if l.lufs_readout.is_positive() {
        let r = l.lufs_readout;
        let text = format!("{} {}", view.loudness.window.label(), view.loudness.text());
        let size = fit_text(r, &text, r.height() * 0.78);
        if size >= MIN_TEXT {
            painter.text(
                // **Centred under the METER, not under the column.** The
                // column is words, then the bar, then the numbers, so a
                // reading centred in it sat off to the left of the thing it
                // is a reading of — the owner saw it as the text "starting
                // after the type letter".
                Pos2::new(l.lufs_bar.center().x, r.center().y),
                Align2::CENTER_CENTER,
                &text,
                font_light(size),
                surface_of(p.bg).ink,
            );
        }
    }
}

fn draw_master(painter: &Painter, l: &Layout, view: &RecorderView<'_>, p: &Palette) {
    if !l.master_bars.is_positive() {
        return;
    }
    let m = &view.master;
    // Two bars with a hair between them, or one when the output is mono.
    let bars = l.master_bars;
    let (left, right) = if m.mono {
        (bars, Rect::NOTHING)
    } else {
        (slice_h(bars, 0.0, 0.48), slice_h(bars, 0.52, 1.0))
    };
    draw_ladder(
        painter,
        left,
        master_fraction(m.left.peak.max(m.left.rms)),
        Some(master_fraction(m.left.hold)),
    );
    if right.is_positive() {
        draw_ladder(
            painter,
            right,
            master_fraction(m.right.peak.max(m.right.rms)),
            Some(master_fraction(m.right.hold)),
        );
    }

    // **The reduction is a strip behind the scale, and the scale is its
    // readout.** It hangs from 0 and reaches down by however many decibels the
    // limiter took off, against the same numbers the ladders are read against
    // — so 6 dB of reduction reaches the -6, and it needs no number of its own.
    // It had a column, and a column for something that is usually zero is a
    // column the meters could have had.
    // **Both sides, mirrored.** The numbers on the left are read against the
    // left channel and the ones on the right against the right; one scale for
    // a stereo pair means one of the two channels is always being estimated.
    for (side, face) in l.master_scale.into_iter().enumerate() {
        if !face.is_positive() {
            continue;
        }
        // The left scale is right-aligned against the ladders and the right
        // scale left-aligned against them, so both sit next to what they
        // measure rather than out at the column's edges.
        let (align, at_x) = if side == 0 {
            (Align2::RIGHT_CENTER, face.right() - 2.0)
        } else {
            (Align2::LEFT_CENTER, face.left() + 2.0)
        };
        // Small: eight numbers have to fit up the side of the ladder without
        // touching each other, and the ladder is what is being read.
        let step = l.master_bars.height() / 9.0;
        let size = fit_text(
            Rect::from_min_size(face.min, Vec2::new(face.width() - 3.0, step)),
            MASTER_SCALE_FIT,
            step * 0.86,
        );
        let inner = l.master_bars.shrink(1.0);

        // **The reduction, behind the numbers and no wider than they are.**
        // It hangs from 0 and reaches down by however many decibels the
        // limiter took off, against the same scale the ladders are read
        // against — so 6 dB of reduction reaches the -6, and it needs no
        // number of its own.
        //
        // Bounded by the WIDEST LABEL rather than by the column: the column is
        // as wide as the gap it was given and the strip is meant to sit under
        // the ticks, not sweep the whole margin.
        if view.gr_db > 0.0 && size >= MIN_TEXT {
            let wide = master_scale_ink_w(size);
            let (left, right) = if side == 0 {
                ((at_x - wide).max(face.left()), at_x)
            } else {
                (at_x, (at_x + wide).min(face.right()))
            };
            let down = (view.gr_db / -MASTER_FLOOR_DB).clamp(0.0, 1.0) * inner.height();
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(left, inner.top()),
                    Pos2::new(right, inner.top() + down),
                ),
                1.0,
                // Translucent: the numbers are drawn over it and have to stay
                // readable, which is the whole reason it lives here.
                Color32::from_rgba_unmultiplied(LED_GR.r(), LED_GR.g(), LED_GR.b(), 110),
            );
        }

        if size >= MIN_TEXT {
            for db in [0.0_f32, -6.0, -12.0, -18.0, -24.0, -36.0, -48.0, -60.0] {
                let t = (db - MASTER_FLOOR_DB) / -MASTER_FLOOR_DB;
                let y = inner.bottom() - t * inner.height();
                painter.text(
                    Pos2::new(at_x, y),
                    align,
                    // **No padding and no sign.** The right-hand scale used
                    // to be padded to three characters so its digits lined
                    // up in a column, and every number below the top carried
                    // a minus. Both are characters that say what the ladder
                    // already says: it descends, and it draws its 0. See
                    // `recorder::meter_number`.
                    crate::recorder::meter_number(db),
                    font_light(size),
                    p.faint,
                );
            }
        }
    }

}

/// The filters' caps.
///
/// **Violet, and it is the one hue nothing else here uses.** They were ivory,
/// which made two of the eight knobs read as blank caps with no colour at all
/// — the odd ones out rather than a pair. Violet sits opposite the panel's
/// warm brown, so it separates cleanly without competing with the sends'
/// blue, the limiter's green or the master's red, and it takes the same white
/// slot every other cap has.
const FILTER_CAP: Color32 = Color32::from_rgb(0x7b, 0x5c, 0xc9);

/// The limiter's cap.
///
/// **Green, and it was red until the master arrived.** Red is now the master's,
/// which is the right home for it: the master is the one knob on this panel
/// that can undo everything the others did. The limiter keeps a colour of its
/// own because it is neither a send nor a filter — a deep bottle green, which
/// is what the meter lamps on the gear this panel is dressed as were, and
/// which nothing else here is.
const LIMITER_CAP: Color32 = Color32::from_rgb(0x2f, 0x8f, 0x5c);

/// The gain's cap. Amber, because it is the one knob on the panel that can
/// make a take clip — and it sits beside the one that stops it.
const GAIN_CAP: Color32 = Color32::from_rgb(0xd8, 0x93, 0x2c);

/// A transport button's side, as a fraction of the tempo knob's diameter.
///
/// **Measured against the knob beside them**, which is what makes the four of
/// them one group. Smaller than it: the transport's job is Record, and three
/// buttons the size of the thing next to them read as the loudest object in
/// the band — which they are not; the meters are.
///
/// **0.82, up from 0.72.** The desk rework left the band with more room than
/// its controls were sized for, and the owner's word was to spend it: every
/// dial, button and meter up rather than empty wood between them.
const TRANSPORT_SIDE: f32 = 0.82;

/// **The smallest a transport button gets, in points.**
///
/// `TRANSPORT_SIDE` follows the tempo knob, and the knob follows the band's
/// height, which follows the window's WIDTH - so a narrow window shrinks every
/// button in the row together. At 900 points (the 100% window on a 1440-wide
/// laptop screen, the owner's 2013 Air) that came to 9.1 points, and the cog,
/// the one icon with a size below which it refuses to draw, disappeared while
/// its click target stayed live: the most important button in the band was an
/// invisible one. The floor keeps every icon a readable, pressable size. It is
/// half a point over twice [`COG_MIN_R`], so rounding at the threshold cannot
/// drop the cog, and it stays under the knob's diameter (11.1 at 900), so the
/// row keeps its rule of buttons smaller than the dial beside them.
///
/// The floor is `side`'s. Where the row cannot also afford `MIN_GAP` between
/// its nine items, `fill_faders` scales every width - icons included - below
/// it rather than let the row overlap; the cog is then held by its own floor
/// (`cog_fits`), which `every_control_is_drawn_at_every_width_the_window_allows`
/// checks at every width the window allows.
const TRANSPORT_MIN_SIDE: f32 = 10.5;

/// The smallest cog worth drawing, as a radius. Below it the teeth merge into
/// the body - see [`draw_cog`].
const COG_MIN_R: f32 = 5.0;

/// **Whether the settings cog can be drawn in `r`.** The layout and the
/// painter both ask, like [`knob_fits`]: a cog too small to paint must not be
/// left behind as a click target over blank panel.
fn cog_fits(r: Rect) -> bool {
    r.is_positive() && r.width().min(r.height()) * 0.5 >= COG_MIN_R
}

/// The tempo knob's name, one spelling for the painter and for the layout that
/// makes room for it.
const TEMPO_WORD: &str = "TEMPO";

/// The tempo box's share of the fader column's height.
///
/// The faders' share of the middle group's width.
///
/// The rest, less a gap, is the meters with the knob row under them. Named
/// because three places have to agree about it and the failure when they do
/// not is a knob drawn over a meter.
const FADER_COL: f32 = 0.46;

/// The air between the transport row and the rule at either end of it, as a
/// fraction of the middle group's width.
///
/// **One constant because there are two ends.** The right-hand rule was
/// placed with this offset and the left-hand end of the row had none, so the
/// tempo knob touched its separator while the cog stood clear of the other —
/// the owner lined the two up and asked for them to match.
const ROW_PAD: f32 = 0.02;

/// The air between the four clusters at the band's right end, as a fraction
/// of the middle group's width.
///
/// **One number for all three gaps.** They were four columns with their own
/// margins, and a column's edge is not where its contents start — so the
/// drawn distances came out uneven however evenly the columns were sliced.
const CLUSTER_GAP: f32 = 0.022;

/// The meters' share of the height of the column they share with the knobs.
///
/// **The meters give up the smaller half of what the knobs gained.** A VU face
/// is sized by whichever of its height and width runs out first and it was
/// width-limited before, so a shorter column costs it less than the number
/// suggests — and the band is taller now than it was, which pays for most of
/// it. See `BAND_H_AT_1300`.
/// **0.40, down from 0.46, which is where the dials got their size from.**
/// The VU pair is a picture of a level and reads at any height; a dial below a
/// certain diameter is three grey rings, and there are eight of them. The
/// meters gave up eight percent of the column and every knob on the panel is a
/// sixth bigger for it.
///
/// **And 0.36 now, for the same reason again.** The VU faces are sized by the
/// COLUMN'S WIDTH (see `draw_meter` — height stopped being their limit long
/// ago), so the strip they stand in was holding height the needles could not
/// use while the dials below were the thing that wanted it. The width the
/// faces do want came from the column edges — see `fill_transport`.
const METER_SHARE: f32 = 0.36;

/// Where the transport's right column stops and the master column starts.
///
/// 0.80: the master column lent the VU-and-knobs column a fiftieth of the
/// band. Its ladders are read against the scale beside them and lose nothing
/// legible at the width that remains; the VU faces are width-bound and grow
/// by exactly what anything to their left or right gives up.
const MASTER_COL: f32 = 0.880;

/// Where the PITCH dial's own column starts — the first of the three at the
/// right end.
///
/// **It used to have no column at all.** The dial was placed at the foot of
/// the loudness column, sharing its width and taking its bottom, so the
/// ladder stopped short of the band's own floor and the dial's reading had
/// only a knob's height to live in — the owner's "squashed behind its dial
/// because of the lufs strip". They are two controls about two different
/// things; they get two columns.
const VARI_COL: f32 = 0.680;

/// Where the VU-and-knobs column stops and the LOUDNESS column starts.
///
/// **The eleven-hundredths between here and `MASTER_COL` is what the
/// loudness ladder and the varispeed dial cost**, and it comes out of the
/// meters' column rather than the master's: the master ladder is read against
/// numbers on both sides and has none to give, while the VU faces are
/// *height*-bound at every width this app opens at — a face is sized by
/// whichever of its height and width runs out first (see `draw_meters`), and
/// the pair does not become width-bound until this column falls under about
/// two hundred and sixty points. Taking eighty from it costs the faces
/// nothing and the eight knobs nothing; what shrinks is the margin either
/// side of each circle.
const LUFS_COL: f32 = 0.745;




/// The dB scale's share of the master column's width, **at each side**. What
/// is left between the two is the ladders.
const MASTER_SCALE_W: f32 = 0.26;

/// The type size the loudness ladder's numbers are drawn at.
///
/// **Off the BAR, not off the cell they sit in.** Sizing them from their own
/// cell's width made the two chase each other: the cell could not be narrowed
/// to the ink without shrinking the ink, so it stayed `bar_w * 0.55` — nearly
/// three times what two digits need — and all of that slack sat between the
/// loudness meter and the master. The bar's width is the same number the old
/// formula was ultimately derived from, so the type is unchanged in practice.
fn lufs_tick_size(bar_w: f32) -> f32 {
    (bar_w * 0.253).clamp(0.0, 9.0)
}

/// The step between the loudness bar and its numbers.
///
/// **Almost touching**, the owner's word: the ladder and the figures beside it
/// are one object and a gap between them makes two.
const LUFS_TICK_GAP: f32 = 1.0;

/// How wide the loudness bar is against the master meter's.
///
/// **A thinner strip.** It used to be exactly the master's width, measured off
/// it so the two could not drift — a good rule for the wrong pair. The master
/// is two ladders reading a stereo output and is READ; the loudness meter is
/// one figure with a bar behind it, and at the master's width it was the
/// widest thing at that end of the band. Still measured off the master, so
/// the two still cannot drift; just not one to one.
const LUFS_BAR_OF_MASTER: f32 = 0.55;

/// How wide those numbers actually draw, cell padding included.
///
/// The floor comes from `LUFS_RANGE` and the spelling from
/// `recorder::meter_number`, so a ladder that one day reaches further down
/// widens this by itself.
fn lufs_ticks_ink_w(bar_w: f32) -> f32 {
    let (lo, _) = crate::recorder::LUFS_RANGE;
    let widest = crate::recorder::meter_number(lo);
    widest.chars().count() as f32 * ADV * lufs_tick_size(bar_w) + 2.0
}

/// What the master's dB scale is SIZED against.
///
/// Deliberately one character wider than anything it prints: the type is
/// chosen to fit a signed figure, which leaves the numbers a little air and
/// is the size this band has always drawn them at.
///
/// **Not the same question as how wide the numbers ARE** — see
/// `master_scale_ink_w`. Sizing wants the pessimistic string; spacing wants
/// the real one, and conflating them is what left a pitch dial of air beside
/// the loudness meter.
const MASTER_SCALE_FIT: &str = "-60";

/// How wide the master's dB scale actually draws, at type size `size`.
///
/// The labels come from `recorder::meter_number`, which prints the ABSOLUTE
/// value — `60`, not `-60` — so the ink is two characters wide however the
/// type was sized. Derived from that function rather than from a literal, so
/// a scale that one day grows a minus sign moves the spacing with it.
fn master_scale_ink_w(size: f32) -> f32 {
    let widest = crate::recorder::meter_number(MASTER_FLOOR_DB);
    widest.chars().count() as f32 * ADV * size
}

/// The smallest knob face worth drawing, as a radius.
///
/// Below this the body, the cap and the slot land inside a few points of each
/// other and it is three grey rings, not a control. 5.5 since the tick
/// skirt joined the size budget — the same on-screen threshold as the old
/// 7.0 was against the un-budgeted radius.
const KNOB_MIN_R: f32 = 5.5;

/// The label's share of a knob cell, and its bounds in points.
///
/// The top end matters now that the knobs are a row rather than a stack: a
/// cell three times as wide can carry a word three times the size, and a knob
/// whose name is set in six points beside a thirty-point dial reads as a dial
/// with a smudge over it.
const KNOB_LABEL: (f32, f32, f32) = (0.24, 6.0, 15.0);

/// Air between a dial's word and its tick skirt, in points.
///
/// **Because the ticks moved to the top.** They used to hang under the dial,
/// where the only thing they could run into was the reading; now they arc over
/// it, straight into the name — and the skirt reaches to 1.26 of the radius,
/// which `knob_face` sizes to touch the face exactly. A hair of room is the
/// difference between a scale and a scale with a word sitting on it.
const KNOB_TICK_ROOM: f32 = 1.5;

/// How large a dial's reading is set, as a fraction of its strip.
///
/// **One number for every dial**, so the eight effects, the master and the
/// tempo cannot end up three different sizes — and a touch under what it was,
/// which is room the ticks above it now want.
const KNOB_READING: f32 = 0.88;

/// What a knob gets out of its cell: where the face goes, how big it is, and
/// how much was left for the word above it.
struct KnobFace {
    face: Rect,
    /// **The body's radius, not the skirt's.** The ticks stand outside it and
    /// are deliberately not counted: sizing the knob to fit them made the
    /// visible circle a fifth smaller than the room it had, which is a knob
    /// that looks lost in its own cell.
    radius: f32,
    /// Points reserved for the label. **Zero means there is no room for one**,
    /// and the face takes the whole cell.
    label_h: f32,
}

/// How a knob would be laid out in `cell`, or `None` if there is no room.
///
/// **The layout and the painter both ask this**, which is what keeps them from
/// disagreeing about whether a knob is there — the failure that leaves a drag
/// target on blank panel.
///
/// **The word yields before the knob does.** In a short band the label eats a
/// third of the cell to say something nobody can read at five points, and what
/// is lost is the control. So below the label's own minimum the face takes
/// everything: a knob with no caption is still a knob, still grabbable, and
/// still says what it is set to the moment a hand is on it.
fn knob_face(cell: Rect) -> Option<KnobFace> {
    if !cell.is_positive() {
        return None;
    }
    let (share, lo, hi) = KNOB_LABEL;
    let wanted = cell.height() * share;
    let label_h = if wanted >= lo { wanted.min(hi) } else { 0.0 };
    // **Two strips — the owner's dial standard**: the NAME always above the
    // knob, the READING always below it. The face takes what is left between
    // them, and the two strips are the same height so the dial stays centred
    // whichever way round they are.
    //
    // The SKIRT is part of the budget: ticks stand out to 1.26 of the
    // radius, and a face sized to the strip put its lower ticks under the
    // label — the owner watched REVERB overdraw them. The whole drawing,
    // ticks included, now fits the band between the value and the label.
    let lay = |label_h: f32| {
        let pad = if label_h > 0.0 { KNOB_TICK_ROOM } else { 0.0 };
        let face = Rect::from_min_max(
            Pos2::new(cell.left(), cell.top() + label_h + pad),
            Pos2::new(cell.right(), cell.bottom() - label_h - pad),
        );
        let radius = face.width().min(face.height()) * 0.5 / 1.26;
        (face, radius)
    };
    let (mut face, mut radius) = lay(label_h);
    let mut label_h = label_h;
    // **The word yields before the knob does, as the doc above promises.**
    // It used to yield only when the WORD was under its own minimum, never
    // when the word was what pushed the knob under ITS minimum - so between
    // about 692 and 889 points of window every dial on the band vanished
    // (eight sends, the pitch dial and the master), although a knob with a
    // smaller caption, or none, had room to spare. Shrink the strips to what
    // leaves the knob its minimum; if that is less than a readable word, drop
    // the word and give the face the whole cell.
    if label_h > 0.0 && (!face.is_positive() || radius < KNOB_MIN_R) {
        // Aimed a hair ABOVE the minimum. Aimed at it exactly, the eight
        // cells of the send grid - one height, but measured from two rows -
        // came out either side of 5.5 by a float's last bit, and the grid
        // is all eight or none.
        let need = 2.0 * 1.26 * (KNOB_MIN_R + 0.01);
        let shrunk = ((cell.height() - need) * 0.5 - KNOB_TICK_ROOM).min(label_h);
        label_h = if shrunk >= lo { shrunk } else { 0.0 };
        (face, radius) = lay(label_h);
    }
    if !face.is_positive() {
        return None;
    }
    (radius >= KNOB_MIN_R).then_some(KnobFace {
        face,
        radius,
        label_h,
    })
}

/// Whether `pos` lands on the DIAL ITSELF — the circle and its tick skirt —
/// rather than anywhere in the knob's cell. The right-click gestures ask
/// this: a cell is mostly margin, and "right-click the dial" answering a
/// press two ticks into the empty paint ate the menu everywhere near a knob.
pub fn on_knob_circle(cell: Rect, word: Rect, pos: Pos2, vari: bool) -> bool {
    // **The varispeed is laid out differently**, and the right-click that
    // opens its range has to land on the circle that was actually drawn —
    // see `vari_face`. Told rather than guessed, because a rectangle cannot
    // say which control it belongs to.
    let f = if vari { vari_face(cell, word) } else { knob_face(cell) };
    match f {
        Some(f) => {
            let reach = if vari { RECESS_R } else { 1.26 };
            (pos - f.face.center()).length() <= f.radius * reach + 2.0
        }
        None => false,
    }
}

/// Where a knob's circle sits, for anything that has to line up with it.
///
/// **The centre of the FACE, both ways.** The circle used to hang from the top
/// of its face, so a cell taller than it was wide left the slack underneath —
/// and anything aligned to the cell's middle sat below the knob rather than
/// beside it.
fn knob_centre(cell: Rect) -> Option<Pos2> {
    knob_face(cell).map(|k| k.face.center())
}

fn knob_fits(cell: Rect) -> bool {
    knob_face(cell).is_some()
}

/// One knob, modelled on the Tascam 388's effect returns.
///
/// Black body, blue cap, and a pale slot across the cap that IS the pointer —
/// the 388 has no separate indicator line, the moulding is slotted and the slot
/// tells you where it is. A word over the top, and tick marks around the skirt
/// so a glance can tell a quarter turn from a half.
///
/// `value` is 0..=1 and the caller owns what that means.
/// One knob's worth of state, for [`draw_knob`].
///
/// A struct rather than seven positional arguments: two of them are strings
/// and two are bools, and a call site that got either pair the wrong way round
/// would compile.
struct Knob<'a> {
    /// 0..=1, whatever that means to the caller.
    value: f32,
    /// The word over it.
    label: &'a str,
    /// What is being typed into it, if anything — caret drawn in.
    typing: Option<String>,
    /// A hand is on it right now, so show the reading rather than the name.
    turning: bool,
    /// What the reading SAYS. A send is a percent and a tempo is a number of
    /// beats; the knob does not know which it is and should not guess.
    reading: String,
    /// The cap. The effects share one; the tempo has its own, which is what
    /// makes it read as part of the transport rather than a fourth send.
    cap: Color32,
    /// **Out of circuit**: drawn dark, cap and slot, so a dial that applies
    /// nothing does not read as one that does. The limiter, when its
    /// circuit row says so.
    dim: bool,
}

/// Which way tick `i` of [`KNOB_TICKS`] stands out from the dial's centre.
///
/// Its own function so the painter and the test that keeps the scale the right
/// way up are reading the same arithmetic. See the note in [`draw_knob`]: the
/// pointer cannot show which way up a dial is, because it is a slot drawn
/// straight through the middle and looks the same either way. The ticks are the
/// only thing that can, so they are the thing worth pinning.
fn knob_tick_dir(i: usize) -> Vec2 {
    let a = (i as f32 / (KNOB_TICKS - 1) as f32 - 0.5) * KNOB_SWEEP;
    Vec2::new(a.sin(), -a.cos())
}

fn draw_knob(painter: &Painter, cell: Rect, k: &Knob<'_>, p: &Palette) {
    let (value, label, typing, turning) = (k.value, k.label, k.typing.as_deref(), k.turning);
    if !cell.is_positive() {
        return;
    }
    // The label takes the top and the knob gets a square out of what is left.
    // Sized off the CELL rather than measured off the text, because a knob that
    // changed size with the length of the word over it would leave these two
    // different sizes.
    let Some(KnobFace {
        face,
        radius: rad,
        label_h,
    }) = knob_face(cell)
    else {
        return;
    };
    // **The owner's dial standard: the NAME on top, the FIELD below it.**
    //
    // The two were the other way round for a long time. A panel of dials is
    // read by finding the one you want and then reading it, which is the name
    // first and the number second — and every desk this is modelled on prints
    // the legend above the control and puts the readout under it. Typing
    // replaces the reading in place, with the caret where the hand put it,
    // never a box somewhere else.
    let strip = label_h.max(KNOB_LABEL.1);
    if label_h > 0.0 {
        let top = Rect::from_min_max(
            Pos2::new(cell.left(), cell.top()),
            Pos2::new(cell.right(), cell.top() + label_h),
        );
        let size = fit_text(top, label, label_h * 0.9);
        if size >= MIN_TEXT {
            painter.text(
                Pos2::new(cell.center().x, cell.top()),
                Align2::CENTER_TOP,
                label,
                font_light(size),
                p.faint,
            );
        }
    }
    let bottom_band = Rect::from_min_max(
        Pos2::new(cell.left(), cell.bottom() - strip),
        Pos2::new(cell.right(), cell.bottom()),
    );
    let (bottom_text, bottom_ink) = match typing {
        Some(typed) => (typed.to_owned(), p.ink),
        None => (
            k.reading.clone(),
            if turning { p.ink } else { p.ink.gamma_multiply(0.8) },
        ),
    };
    let size = fit_text(bottom_band, &bottom_text, strip * KNOB_READING);
    if size >= MIN_TEXT {
        painter.text(
            Pos2::new(cell.center().x, cell.bottom()),
            Align2::CENTER_BOTTOM,
            &bottom_text,
            font_light(size),
            bottom_ink,
        );
    }
    let c = face.center();
    let t = value.clamp(0.0, 1.0);
    // **The sweep is centred on UP, and the ticks therefore arc over the top.**
    //
    // It used to be centred on `PI`, which is straight DOWN, so the missing
    // quarter of the circle was at the top and the tick skirt hung under the
    // dial like a smile. Every panel this imitates puts the scale above the
    // control, where a hand on the knob is not covering it.
    //
    // The pointer moved with it and looks identical, which is why this went
    // unnoticed for so many versions: the slot is drawn from `-dir` to `+dir`
    // straight through the centre, so rotating it half a turn produces exactly
    // the same line. Only the ticks could ever have shown which way up the
    // dial was.
    let angle = (t - 0.5) * KNOB_SWEEP;
    let dir = |a: f32| Vec2::new(a.sin(), -a.cos());

    // **The ticks stand OUTSIDE the body, and outside the cell.** They are a
    // mark on the panel the knob is mounted through, not part of the knob, so
    // they are not what its size is measured against — see `knob_face`. Drawn
    // first, so the body covers their inner ends.
    let tick = Stroke::new((rad * 0.09).max(0.7), p.faint);
    for i in 0..KNOB_TICKS {
        let d = knob_tick_dir(i);
        painter.line_segment([c + d * (rad * 1.09), c + d * (rad * 1.26)], tick);
    }
    // The body: near-black, like the moulding, filling the face it was given.
    painter.circle_filled(c, rad, Color32::from_rgb(0x14, 0x12, 0x12));
    // The cap, inset, in its own colour.
    let cap = rad * 0.64;
    // A dim knob keeps its position and loses its light: the cap at a third
    // of its colour, the slot in the face's own grey rather than white.
    let (cap_ink, slot_ink) = if k.dim {
        (k.cap.gamma_multiply(0.33), Color32::from_rgb(0x4a, 0x50, 0x58))
    } else {
        (k.cap, Color32::from_rgb(0xd8, 0xe4, 0xf2))
    };
    painter.circle_filled(c, cap, cap_ink);
    // The slot. Across the whole cap and through the centre, which is what a
    // screwdriver slot looks like and what makes the angle readable at
    // fourteen points: a short pointer at one edge is a dot at this size.
    let d = dir(angle);
    painter.line_segment(
        [c - d * cap * 0.92, c + d * cap * 0.92],
        Stroke::new((cap * 0.30).max(1.0), slot_ink),
    );
}

/// The tempo, as a knob at the head of the transport.
///
/// **The same knob as the three effect sends, in a different colour.** That is
/// the whole point: a number floating beside three buttons is a number that
/// happened to be nearby, and a knob among them is part of the transport. The
/// cap is a darkened orange so it is not read as a fourth send.
///
/// Turned like the others — relatively, so the pointer can leave it and go on
/// turning — and DOUBLE-clicked to type, which is the one gesture the sends do
/// not have. A single tap on a send opens its field; the tempo is turned far
/// more often than it is typed, and a tap that opened a text box every time a
/// hand brushed it would be in the way.
fn draw_tempo(
    painter: &Painter,
    l: &Layout,
    bpm: f64,
    typing: Option<String>,
    turning: bool,
    held: bool,
    p: &Palette,
) {
    // The typed digits ride the knob's OWN strip — the same entry every
    // other dial wears. The box below was the odd one out on the desk,
    // and the owner asked for one language.
    draw_knob(painter, l.tempo, &tempo_knob(bpm, typing, turning, held), p);
    // The TAP chip, wearing the tempo's own orange so the pair reads as
    // one control. Absent when the layout could not seat it. Dimmed with the
    // knob while the tempo is held.
    if l.tempo_tap.is_positive() {
        let ink = if held { toward(TEMPO_CAP, p.bg, 0.55) } else { TEMPO_CAP };
        painter.rect_filled(l.tempo_tap, 3.0, Color32::from_black_alpha(70));
        painter.rect_stroke(l.tempo_tap, 3.0, Stroke::new(1.0_f32, ink), StrokeKind::Inside);
        painter.text(
            l.tempo_tap.center(),
            Align2::CENTER_CENTER,
            "TAP",
            FontId::new((l.tempo_tap.height() * 0.62).clamp(7.0, 10.0), fonts::courier()),
            ink,
        );
    }
}

/// The tempo knob as the painter draws it. **Dark while the tempo is held**
/// (a take's count-in, an export) - the limiter's out-of-circuit look: still
/// the control, still where the hand expects it, and plainly not turning now.
/// See `RecorderState::tempo_held`.
fn tempo_knob(bpm: f64, typing: Option<String>, turning: bool, held: bool) -> Knob<'static> {
    Knob {
        value: tempo_to_knob(bpm),
        label: TEMPO_WORD,
        typing,
        turning,
        reading: tempo_text(bpm),
        cap: TEMPO_CAP,
        dim: held,
    }
}

/// Where `bpm` sits on the knob, 0..=1.
///
/// Linear across the legal range, which over [`KNOB_TRAVEL`] points of travel
/// is about a beat a point — fine enough to land on a number by hand, and the
/// reading says which one.
pub fn tempo_knob_position(bpm: f64) -> f32 {
    tempo_to_knob(bpm)
}

fn tempo_to_knob(bpm: f64) -> f32 {
    ((bpm - MIN_BPM) / (MAX_BPM - MIN_BPM)).clamp(0.0, 1.0) as f32
}

/// The inverse of [`tempo_to_knob`], clamped to what the SMF writer and every
/// DAW's bar ruler will accept — and **rounded to a whole beat**.
///
/// Only the DIAL comes through here. A decimal tempo is still a tempo and is
/// still reachable, by typing it into the field, which goes to `Hit::SetTempo`
/// through `parse_bpm` and never touches this. But a knob is a hand on a
/// value: over `KNOB_TRAVEL` points the sweep is about a beat a point, so
/// every drag landed on 137.4212 and the reading said 137 — a number you could
/// not put back by hand, and a session file full of tempos nobody chose.
pub fn knob_to_tempo(t: f32) -> f64 {
    (MIN_BPM + f64::from(t.clamp(0.0, 1.0)) * (MAX_BPM - MIN_BPM)).round()
}

/// A tick box and its caption: two line segments rather than a glyph, because
/// no bundled face is guaranteed to carry a check mark and a tofu box in a
/// checkbox is indistinguishable from a ticked one.
///
/// The box is bounded by the WIDTH as well as the height. In a tall narrow slot
/// — a detached window's click switch — a box sized off the height alone eats
/// the whole control and the caption disappears.
fn draw_tick(painter: &Painter, r: Rect, cap: &str, on: bool, p: &Palette) {
    control(painter, r, p);
    if !r.is_positive() {
        return;
    }
    let inset = (r.height() * 0.25).min(r.width() * 0.08);
    let side = (r.height() - inset * 2.0).min(r.width() * 0.30);
    if side <= 0.0 {
        return;
    }
    let bx = Rect::from_min_size(
        Pos2::new(r.left() + inset, r.center().y - side * 0.5),
        Vec2::splat(side),
    );
    painter.rect_stroke(bx, 1.0, Stroke::new(1.0_f32, p.line), StrokeKind::Inside);
    if on {
        let s = Stroke::new((side * 0.16).max(1.0), p.ink);
        painter.line_segment(
            [
                Pos2::new(bx.left() + side * 0.22, bx.center().y),
                Pos2::new(bx.center().x - side * 0.02, bx.bottom() - side * 0.24),
            ],
            s,
        );
        painter.line_segment(
            [
                Pos2::new(bx.center().x - side * 0.02, bx.bottom() - side * 0.24),
                Pos2::new(bx.right() - side * 0.18, bx.top() + side * 0.24),
            ],
            s,
        );
    }
    let text = Rect::from_min_max(Pos2::new(bx.right() + inset, r.top()), r.max);
    let size = fit_text(text, cap, r.height() * 0.5);
    if size >= MIN_TEXT {
        painter.text(
            Pos2::new(text.left(), text.center().y),
            Align2::LEFT_CENTER,
            cap,
            font_light(size),
            p.ink,
        );
    }
}

/// A line of unboxed text in a row, left- or right-aligned.
/// What to draw, and how big, for a line that has to fit its row.
///
/// **Cut it rather than lose it.** `fit_text` shrinks to fit and returns a size
/// below [`MIN_TEXT`] when it cannot — and the caller used to take that as
/// "draw nothing". So a 155-character sentence explaining why a take came out
/// wrong was, on a narrower window, silently absent: the one line that had the
/// answer in it. A message the reader has to widen the window to finish is
/// still a message; an empty row is not.
fn fit_or_cut(r: Rect, text: &str) -> (std::borrow::Cow<'_, str>, f32) {
    let want = r.height() * 0.82;
    let size = fit_text(r, text, want);
    if size >= MIN_TEXT {
        return (std::borrow::Cow::Borrowed(text), size);
    }
    // Take characters off the end until what is left fits at the floor. From
    // the end, because the front of a sentence is the half that says what it
    // is about.
    let mut keep = text.len();
    while keep > 0 {
        keep -= 1;
        while keep > 0 && !text.is_char_boundary(keep) {
            keep -= 1;
        }
        let cut = format!("{}…", text[..keep].trim_end());
        if fit_text(r, &cut, want) >= MIN_TEXT {
            return (std::borrow::Cow::Owned(cut), MIN_TEXT);
        }
    }
    (std::borrow::Cow::Borrowed("…"), MIN_TEXT)
}

fn text_line(painter: &Painter, r: Rect, text: &str, colour: Color32, right: bool) {
    if !r.is_positive() || text.is_empty() {
        return;
    }
    let (text, size) = fit_or_cut(r, text);
    let text = text.as_ref();
    let (at, align) = if right {
        (Pos2::new(r.right(), r.center().y), Align2::RIGHT_CENTER)
    } else {
        (Pos2::new(r.left(), r.center().y), Align2::LEFT_CENTER)
    };
    painter.text(at, align, text, font_light(size), colour);
}

/// One-pixel top edge so the band reads as its own, matching the fretboard's.
/// Drawn by the caller, which knows what is above it.
pub fn draw_top_edge(painter: &Painter, rect: Rect, _s: &Settings) {
    // Dark mode no longer reaches the band edge — one look everywhere
    // but the theory window and the arrangement.
    let c = Color32::from_rgb(120, 100, 78);
    painter.rect_stroke(
        Rect::from_min_size(rect.min, Vec2::new(rect.width(), 1.0)),
        0.0,
        Stroke::new(1.0_f32, c),
        StrokeKind::Inside,
    );
}

// ── the popped-out window ──────────────────────────────────────────────────
//
// The fourth detachable surface, and deliberately the same shape of code as the
// other three (`chord_strip`, `fretboard_panel`, `theory_panel`): same
// close-to-reattach, same right-click-anywhere menu, same borderless
// drag-anywhere, same window level. Four popouts with four sets of habits would
// be worse than none.
//
// Like the theory window and unlike the other two, this one is an INPUT: a
// click starts a take. Everything below that differs from the chord strip — the
// hit in the outcome, the drag that stands aside for it, the ctrl-click guard —
// exists for that one reason.

/// Default size for the popped-out recorder when nothing is remembered.
///
/// Not the band's proportions, for the same reason the neck's popout is not
/// (D-UI-10): in its own window it should be legible rather than a slice of the
/// main one. §5's reason for detaching at all is a big framing view on a second
/// monitor, and the preview claims about a quarter of the width, so this is
/// about as small as the window gets while the destination column still holds a
/// device name at a readable size.
pub const DETACHED_DEFAULT: Vec2 = Vec2::new(720.0, 400.0);

/// The smallest window the band still works in.
///
/// **Raised from 480x270 when the monitor column arrived**, and unchanged when
/// the three instrument slots moved into it. The binding constraint is the same
/// as it always was — the destination column, not the preview. At 640x340 that
/// column is ~170pt across with seven rows of ~36pt, which holds
/// `EXPORT wav + midi` and `AUDIO Scarlett 2i2 USB` at about 11pt: it lost width
/// to the slots and got it back in row height, because the slots also took its
/// INSTRUMENT row away. In the same window a slot row is ~39pt tall and ~220pt
/// across, which is `1 Pianoteq 8` at 9pt, a knob, a dB reading at 6.4pt, an
/// `OPEN` button at 17pt and a cross. Below that the column is a stack of boxes with smudges in
/// them, which reads as a rendering fault rather than as "too small" — the same
/// failure `theory_panel::DETACHED_MIN` puts a floor under.
pub const DETACHED_MIN: Vec2 = Vec2::new(640.0, 340.0);

pub fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("tangent-recorder-window")
}

/// Outline for the popped-out window. The band's background is near-black in
/// dark mode and pale grey in light, so neither theme's fill separates the
/// window from the desktop behind it. One neutral grey reads against both,
/// exactly as it does for the other three popouts.
pub const BORDER_COLOR: Color32 = Color32::from_gray(0x5A);

/// What the app has to act on after showing the window.
#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct DetachedOutcome {
    /// The user closed the window. Close-to-reattach: the band comes back.
    pub close_requested: bool,
    /// Live inner size in points, recorded every frame. Feed it to
    /// `theory_panel::GeometryGuard::observe` rather than to settings directly.
    pub inner_size: Option<Vec2>,
    /// Live outer position in monitor coordinates, recorded every frame.
    pub outer_pos: Option<Pos2>,
    /// Right-click (or ctrl-click on macOS) happened at this monitor-space
    /// position, for the app context menu.
    pub context_menu_at: Option<Pos2>,
    /// A click landed on a control. Feed it to the same handler the band's
    /// clicks go to.
    ///
    /// Only the PRESS is reported here. A fader that is being dragged has to be
    /// re-hit-tested by the app while the button is held — see [`hit_test`].
    pub hit: Option<Hit>,
}

/// The recorder in its own window.
///
/// `builder_size` and `builder_pos` must stay constant for the lifetime of one
/// detachment, exactly as for the other three windows: egui diffs this builder
/// against the previous frame's builder rather than against the window's real
/// geometry, so a value that changed every frame would fight the user's own
/// resizes and drags.
///
/// `main_focused` decides the window LEVEL. A detached window is a piece of the
/// same app, so it rises and falls WITH the piano rather than being left
/// wherever the window stack last put it. By level rather than by raising:
/// always-on-top while we are frontmost is exactly "above our own window", and
/// dropping to Normal when we are not means it never floats over other
/// applications.
pub fn show_detached_window(
    ctx: &egui::Context,
    builder_size: Vec2,
    builder_pos: Option<Pos2>,
    borderless: bool,
    main_focused: bool,
    view: &RecorderView<'_>,
    s: &Settings,
) -> DetachedOutcome {
    let mut outcome = DetachedOutcome::default();
    let mut builder = egui::ViewportBuilder::default()
        .with_title("Tangent")
        .with_inner_size(builder_size)
        .with_min_inner_size(DETACHED_MIN)
        .with_resizable(true)
        .with_decorations(!borderless)
        .with_window_level(if main_focused {
            egui::viewport::WindowLevel::AlwaysOnTop
        } else {
            egui::viewport::WindowLevel::Normal
        });
    if let Some(pos) = builder_pos {
        builder = builder.with_position(pos);
    }

    ctx.show_viewport_immediate(viewport_id(), builder, |vp, _class| {
        crate::shell::viewport_ui(vp, |ui| {
            // THE WINDOW'S OWN RECT, used for both the drawing and the hit test
            // below. The band's rect is a completely different rectangle —
            // different width, different height, different origin — and a hit
            // test run against it would return whatever is at that point IN THE
            // MAIN WINDOW, which here means pressing Record because the pointer
            // happened to be where the record button is somewhere else. Nothing
            // in this closure may reach for a band rect, which is why none is
            // passed in.
            let rect = ui.max_rect();
            draw(ui.painter(), rect, view, s);
            painter_border(ui.painter(), rect);

            let (close, inner_rect, outer_rect, pressed, secondary, pointer, ctrl) =
                ui.input(|i| {
                    (
                        i.viewport().close_requested(),
                        i.viewport().inner_rect,
                        i.viewport().outer_rect,
                        i.pointer.primary_pressed(),
                        i.pointer.secondary_clicked(),
                        i.pointer.interact_pos(),
                        i.modifiers.ctrl,
                    )
                });

            outcome.close_requested = close;
            // The fallback matters in a host that reports no window geometry at
            // all: without it the guard sees `None` forever and the window is
            // never remembered, rather than never poisoned.
            outcome.inner_size = inner_rect.map(|r| r.size()).or(Some(rect.size()));
            outcome.outer_pos = outer_rect.map(|r| r.min);

            // Ctrl-click IS the right-click on macOS, and this window has
            // twenty-six click targets. Without the guard, ctrl-clicking to
            // open the menu over the transport starts a take at the same time.
            let ctrl_as_context = cfg!(target_os = "macos") && ctrl;
            let menu = secondary || (pressed && ctrl_as_context);
            if menu {
                if let (Some(pos), Some(inner)) = (pointer, inner_rect) {
                    outcome.context_menu_at = Some(inner.min + pos.to_vec2());
                }
            }

            if pressed && !menu {
                if let Some(p) = pointer {
                    outcome.hit = hit_test(rect, view, p);
                }
            }

            // Borderless drag-anywhere, minus the parts that are not
            // "anywhere". With no title bar the whole window is the drag
            // handle, so an unguarded StartDrag means pressing Record picks the
            // window up and carries it off instead of starting the take. The
            // press only becomes a drag when it hit nothing.
            if borderless && pressed && !menu && outcome.hit.is_none() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        });
    });
    outcome
}

fn painter_border(painter: &Painter, rect: Rect) {
    painter.rect_stroke(
        rect.shrink(0.5),
        0.0,
        Stroke::new(1.0_f32, BORDER_COLOR),
        StrokeKind::Middle,
    );
}

/// **Everything in the band that a person can reach, drawn where it can be
/// reached** - or the names of what is not.
///
/// The question the owner asked of the 100% window on a 1440-point laptop
/// screen, as one predicate for the band's own sweep and for the app's
/// default-size test, so the two cannot mean different things by "all of it".
/// A control counts when the painter would draw it, not when its rectangle is
/// merely positive: the cog used to have a live click target and no picture,
/// and no test noticed because every one of them asked about rectangles.
///
/// Captions yield before knobs do (see `knob_face`), so a dial with no word
/// over it is present. What is checked is that the tempo knob's word shows
/// wherever the sends' words do: it went missing on its own at 900 points,
/// while all eight of theirs stood.
#[cfg(test)]
pub(crate) fn every_control_drawn(rect: Rect, view: &RecorderView<'_>) -> Result<(), Vec<String>> {
    let l = Layout::new(rect, view);
    let mut missing = Vec::new();
    let rolling = l.rolling;
    let mut need = vec![
        ("the camera preview", l.preview),
        ("the clock", l.lcd),
        ("the VU meters", l.meter),
        ("the loudness meter", l.lufs_bar),
        ("the master meter", l.master_bars),
        ("record", l.record),
        ("play", l.play),
        ("stop", l.stop),
        ("pause", l.pause),
        ("the cassette", l.capture),
    ];
    // The tempo is in the row in every layout but an export's.
    if !view.exporting {
        need.push(("TAP", l.tempo_tap));
    }
    if !rolling {
        need.push(("the project folder", l.project));
    }
    for (name, r) in need {
        if !r.is_positive() {
            missing.push(name.to_owned());
        }
    }
    if !rolling && !cog_fits(l.setup) {
        missing.push("the settings cog".to_owned());
    }
    if !view.exporting {
        match knob_face(l.tempo) {
            None => missing.push("the tempo knob".to_owned()),
            Some(f) => {
                let sends_named = l.fx.iter().any(|c| knob_face(*c).is_some_and(|k| k.label_h > 0.0));
                let top = Rect::from_min_max(
                    l.tempo.min,
                    Pos2::new(l.tempo.right(), l.tempo.top() + f.label_h),
                );
                let named = f.label_h > 0.0 && fit_text(top, TEMPO_WORD, f.label_h * 0.9) >= MIN_TEXT;
                if sends_named && !named {
                    missing.push("the word TEMPO".to_owned());
                }
            }
        }
    }
    for (i, c) in l.fx.iter().enumerate() {
        if !knob_fits(*c) {
            missing.push(format!("the {} dial", Fx::ALL[i].title()));
        }
    }
    // By the painter's own face, which is stricter than `knob_fits`: it takes
    // the PITCH CONT word's line out of the cell before it sizes the dial.
    if vari_face(l.vari, l.vari_label).is_none() {
        missing.push("the pitch dial".to_owned());
    }
    if !knob_fits(l.master_knob) {
        missing.push("the master knob".to_owned());
    }
    if missing.is_empty() { Ok(()) } else { Err(missing) }
}

#[cfg(test)]
mod tests {
    /// **The luminance channel curve as it was computed before the table,
    /// kept as the specification of it.**
    ///
    /// This is the body `luminance` had, verbatim. It exists so the table
    /// can be checked against the arithmetic rather than against a list of
    /// numbers somebody transcribed — a golden list would say the table has
    /// not changed since it was written down, which is not the question.
    pub(super) fn luminance_reference(c: Color32) -> f32 {
        let ch = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    }

    /// **The table IS the arithmetic, to the bit, for every input it can be
    /// asked about.**
    ///
    /// Not "within an epsilon". Everything legible in this app is decided by
    /// comparing a contrast ratio against a fixed floor — 4.5, 4.6, 3.0 —
    /// and a colour whose ratio lands on one of those floors flips sides if
    /// the curve moves in its last bit. `legible_surface` would then walk to
    /// a different surface, which is a visible change to a painted lane, and
    /// the dead zone at 0.12-0.22 luminance is exactly where the borderline
    /// colours live.
    ///
    /// A channel is a `u8`: 256 inputs, and the table holds all of them. The
    /// rest of `luminance` — the three weights and their sum — is the same
    /// expression it always was, so 256 bit-equal channel values is the
    /// whole proof for all 16.7 million colours. The sweep below is belt and
    /// braces against exactly that reasoning being wrong somewhere (a
    /// contraction the table's loads permit and the inlined `powf` did not,
    /// say), and it costs a fraction of a second.
    #[test]
    fn the_luminance_table_is_the_arithmetic_it_replaced() {
        for v in 0..=255u8 {
            let want = srgb_to_linear(v);
            let got = srgb_to_linear_table()[usize::from(v)];
            assert_eq!(
                want.to_bits(),
                got.to_bits(),
                "channel {v}: the table says {got:e}, the curve says {want:e}"
            );
        }
        // Every (r, g) pair, with b walked across its whole range as they
        // go, and then the greys and the pure channels, which are where the
        // hand-picked theme colours actually sit.
        let mut checked = 0u32;
        let mut check = |c: Color32| {
            let (want, got) = (luminance_reference(c), luminance(c));
            assert_eq!(
                want.to_bits(),
                got.to_bits(),
                "{c:?}: the table says {got:e}, the arithmetic says {want:e}"
            );
            checked += 1;
        };
        for r in 0..=255u8 {
            for g in 0..=255u8 {
                let b = (u32::from(r) * 7 + u32::from(g) * 13) as u8;
                check(Color32::from_rgb(r, g, b));
            }
        }
        for v in 0..=255u8 {
            check(Color32::from_rgb(v, v, v));
            check(Color32::from_rgb(v, 0, 0));
            check(Color32::from_rgb(0, v, 0));
            check(Color32::from_rgb(0, 0, v));
        }
        assert_eq!(checked, 256 * 256 + 4 * 256, "the sweep did not run");
    }

    /// **The badge table is dense, unique, and every code draws.** The
    /// picker iterates `ICON_PICKS` and the strips dispatch on the code; a
    /// code that vanished or collided would re-badge every saved session.
    #[test]
    fn every_badge_code_is_dense_and_draws() {
        use super::{draw_badge, ICONS, ICON_MAX};
        use egui::{Color32, Pos2, Rect, Vec2};
        assert_eq!(ICONS.len(), usize::from(ICON_MAX) + 1, "the table and the max drifted");
        for (i, (code, name)) in ICONS.iter().enumerate() {
            assert_eq!(usize::from(*code), i, "{name} is out of place");
            assert!(!name.is_empty(), "code {code} has no name");
        }
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let b = Rect::from_min_size(Pos2::ZERO, Vec2::splat(20.0));
                assert!(
                    !draw_badge(ui.painter(), b, Color32::BLACK, Color32::WHITE, 0),
                    "auto claimed a picture - the kind draws it"
                );
                for code in 1..=ICON_MAX {
                    assert!(
                        draw_badge(ui.painter(), b, Color32::BLACK, Color32::WHITE, code),
                        "code {code} has no picture"
                    );
                }
                assert!(
                    !draw_badge(ui.painter(), b, Color32::BLACK, Color32::WHITE, ICON_MAX + 1),
                    "an unknown code claimed a picture"
                );
            });
        });
    }

    /// **Every code points at a drawing, and `glyph` agrees with `ICONS`.**
    ///
    /// The table and the dispatch are two lists written by hand, which is the
    /// arrangement that eventually drifts. This is what stops it drifting
    /// silently: a code named in one and missing from the other is a badge
    /// the picker offers and the desk cannot wear, or the reverse.
    #[test]
    fn the_table_and_the_dispatch_name_the_same_codes() {
        use super::{glyph, ICONS, ICON_MAX};
        for (code, name) in ICONS.iter().skip(1) {
            assert!(glyph(*code).is_some(), "{name} ({code}) has no drawing");
        }
        assert!(glyph(0).is_none(), "auto is not a drawing");
        assert!(glyph(ICON_MAX + 1).is_none(), "a code past the end drew something");
    }

    /// **13 to 16 are ordinary slots, and these are what is in them.**
    ///
    /// Four codes came free when the new set arrived with no drawing
    /// for what they used to hold, and the first four of the new instruments
    /// took them rather than the table growing four holes. That is the whole
    /// reason it is dense. Named one by one rather than walked in a loop:
    /// this is the assignment a later edit could quietly reshuffle, and a
    /// reshuffle re-badges every saved desk that has used one since.
    #[test]
    fn the_four_freed_codes_hold_their_new_instruments() {
        use super::glyph;
        use crate::icons::Icon;
        assert_eq!(glyph(13), Some(Icon::Accordion));
        assert_eq!(glyph(14), Some(Icon::Cabasa));
        assert_eq!(glyph(15), Some(Icon::DigitalWave));
        assert_eq!(glyph(16), Some(Icon::EgSingleCoil));
    }

    /// **The four saxophone codes are one horn.**
    ///
    /// The old set drew a soprano, an alto, a tenor and a baritone; the new
    /// one draws a saxophone. All four codes still load - a desk that chose
    /// the tenor keeps choosing the tenor - and all four draw the same
    /// picture, which is why the picker offers it once.
    #[test]
    fn every_saxophone_code_draws_the_one_saxophone() {
        use super::{glyph, ICON_PICKS};
        use crate::icons::Icon;
        for code in 17..=20 {
            assert_eq!(glyph(code), Some(Icon::Saxophone), "code {code}");
        }
        let offered: Vec<u8> = ICON_PICKS
            .iter()
            .filter(|(c, _)| (17..=20).contains(c))
            .map(|(c, _)| *c)
            .collect();
        assert_eq!(offered, vec![17], "the picker offers the horn more than once");
    }

    /// **The picker's list is every DRAWING once, and nothing else.**
    ///
    /// It cannot simply be `ICONS` any more, and the two ways of getting that
    /// wrong are both silent: offer a code twice and the same picture appears
    /// twice under two words, drop one and an instrument in the set is
    /// unreachable from the only place a badge is chosen.
    #[test]
    fn the_picker_offers_every_drawing_exactly_once() {
        use super::{glyph, ICONS, ICON_PICKS};
        // Every pick is a real row of the code table, spelled the same way.
        for (code, name) in ICON_PICKS {
            let found = ICONS.iter().find(|(c, _)| c == code);
            let Some((_, real)) = found else {
                panic!("the picker offers code {code}, which the table has no row for");
            };
            assert_eq!(real, name, "code {code} is named two different things");
        }
        // "auto" is offered and is not a drawing.
        assert_eq!(ICON_PICKS.first(), Some(&(0u8, "auto")));
        // And the drawings offered are the whole set, once each.
        let mut offered: Vec<&str> = ICON_PICKS
            .iter()
            .filter_map(|(c, _)| glyph(*c))
            .map(crate::icons::Icon::slug)
            .collect();
        let n = offered.len();
        offered.sort_unstable();
        offered.dedup();
        assert_eq!(offered.len(), n, "the picker offers one drawing twice");
        let mut all: Vec<&str> = crate::icons::Icon::ALL.iter().map(|i| i.slug()).collect();
        all.sort_unstable();
        assert_eq!(offered, all, "the picker and the icon set disagree");
    }

    /// **A maximised window gets a shorter band, and the scope gets the room.**
    ///
    /// The owner: at 100 percent "the app feels perfect visually", but
    /// full-screened the recorder "NEEDS to be around 15-30 percent shorter",
    /// and "that extra horizontal room? let the oscillator grow
    /// horizontally".
    ///
    /// **Then, having lived with it: "lower the max scaling of the master in
    /// maximized and full screen to be even shorter … by around another 10
    /// percent height reduction."** So the range asked for is the old one
    /// times nine tenths — 24 to 37 per cent — and the band at this display's
    /// maximised width lands in the middle of it.
    ///
    /// Both halves are here because they are one change: the band's height
    /// stops following the width, which makes the LCD box relatively wider,
    /// and the scope takes what the digits do not need.
    #[test]
    fn a_wide_window_shortens_the_band_and_widens_the_scope() {
        // **Nothing moves at the sizes anybody works in.** This is the
        // assertion that protects the view the owner already likes.
        for w in [900.0f32, 1300.0, 1600.0] {
            let linear = (BAND_H_AT_1300 * f64::from(w) / 1300.0).trunc() as f32;
            assert_eq!(
                band_height(w),
                linear,
                "the band changed at {w}, which is a window and not a screen"
            );
        }

        // Wide: shorter, and inside the range that was asked for. 3440 sits
        // on the floor, which is 0.63 of linear and therefore exactly 37%.
        for (w, lo, hi) in [(2560.0f32, 0.24, 0.32), (3440.0, 0.24, 0.375)] {
            let linear = BAND_H_AT_1300 * f64::from(w) / 1300.0;
            let got = f64::from(band_height(w));
            let cut = 1.0 - got / linear;
            assert!(
                cut >= lo && cut <= hi,
                "at {w} the band is {:.1}% shorter, outside {:.0}-{:.0}%",
                cut * 100.0,
                lo * 100.0,
                hi * 100.0
            );
        }
        // And never more than the floor, however wide the screen.
        for w in [5120.0f32, 7680.0] {
            let linear = BAND_H_AT_1300 * f64::from(w) / 1300.0;
            let cut = 1.0 - f64::from(band_height(w)) / linear;
            assert!(cut <= 0.375, "at {w} the band lost {:.0}%", cut * 100.0);
        }

        // **The scope grows.** Measured as its share of the LCD box, which is
        // what "grow horizontally" means when the box itself is growing.
        let share = |w: f32| -> f32 {
            let r = Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(w, band_height(w)),
            );
            let l = Layout::new(r, &RecorderView::empty());
            1.0 - lcd_split(l.lcd)
        };
        let at_1600 = share(1600.0);
        let at_2560 = share(2560.0);
        let at_3440 = share(3440.0);
        assert!(
            (at_1600 - 0.525).abs() < 0.01,
            "the split moved at a working size: scope has {at_1600}"
        );
        assert!(
            at_2560 > at_1600 + 0.05,
            "the scope did not take the extra room: {at_1600} then {at_2560}"
        );
        assert!(at_3440 > at_2560, "wider still and the scope stopped growing");
        assert!(at_3440 <= 0.72, "the scope crowded the digits out: {at_3440}");
    }

    /// **A panel summoned at a point lands CENTRED on it, both ways.**
    ///
    /// `Cmd+M` hands the middle of the surface as a one-point anchor, and
    /// the placement hung the panel BELOW whatever it was given - so it came
    /// up across the middle horizontally and under it vertically. The
    /// owner: "metronome menu is now horizontally but not vert centered on
    /// cmd m. needs both to be in the middle of the screen."
    ///
    /// And the other half, which is what stops this being a one-line change
    /// that moves the chip's panel too: a real button still gets a panel
    /// hanging off it rather than one sitting on top of it.
    #[test]
    fn a_panel_summoned_at_a_point_is_centred_on_it() {
        use super::met_popup_rect;
        for (w, h) in [(1400.0_f32, 900.0_f32), (900.0, 600.0), (2560.0, 1440.0)] {
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, h));
            let middle = screen.center();
            let summoned = met_popup_rect(screen, Rect::from_center_size(middle, Vec2::splat(1.0)));
            assert!(summoned.is_positive(), "the summoned panel has no room at {w}x{h}");
            assert!(screen.contains_rect(summoned), "it hangs off the surface at {w}x{h}");
            assert!(
                (summoned.center().x - middle.x).abs() < 0.5,
                "not centred across at {w}x{h}: {} against {}",
                summoned.center().x,
                middle.x
            );
            assert!(
                (summoned.center().y - middle.y).abs() < 0.5,
                "not centred DOWN at {w}x{h}: {} against {}",
                summoned.center().y,
                middle.y
            );

            // A button keeps its panel underneath it.
            let chip = Rect::from_min_size(Pos2::new(w * 0.2, h * 0.1), Vec2::splat(22.0));
            let hung = met_popup_rect(screen, chip);
            assert!(
                hung.top() >= chip.bottom(),
                "a button's panel covered the button at {w}x{h}"
            );
            assert!(
                (hung.center().x - chip.center().x).abs() < 0.5,
                "a button's panel is no longer centred on it at {w}x{h}"
            );
        }
    }

    /// **The metronome panel answers every control it draws.**
    ///
    /// It was an egui dialog and read as a foreign object — the owner's "it's
    /// cut off or looks wonky". Painted like the dials' own panels it is
    /// themed by construction, but a painted panel has to hit-test itself, so
    /// this walks every cell it draws and asks what is under it.
    #[test]
    fn the_metronome_panel_answers_every_cell_it_draws() {
        use super::{met_hit_test, met_popup_rect, MetHit, MET_DIVS, MET_UNITS};
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0));
        let anchor = Rect::from_min_size(Pos2::new(300.0, 80.0), Vec2::splat(22.0));
        let panel = met_popup_rect(screen, anchor);
        assert!(panel.is_positive(), "the panel has no room");
        assert!(screen.contains_rect(panel), "the panel hangs off the surface");

        let l = MetLayout::new(screen, anchor);
        // Every division and every note value is reachable, and each answers
        // with ITS OWN value rather than a neighbour's.
        for (cell, (d, _)) in MetLayout::cells(l.rows[3], MET_DIVS.len())
            .iter()
            .zip(MET_DIVS)
        {
            assert_eq!(met_hit_test(screen, anchor, cell.center()), Some(MetHit::Div(d)));
        }
        for (cell, u) in MetLayout::cells(l.rows[2], MET_UNITS.len()).iter().zip(MET_UNITS) {
            assert_eq!(met_hit_test(screen, anchor, cell.center()), Some(MetHit::Unit(u)));
        }
        // Minus and plus step the beats; the count between them is a READOUT
        // and pressing it must not change anything.
        let beats = MetLayout::cells(l.rows[1], 3);
        assert_eq!(met_hit_test(screen, anchor, beats[0].center()), Some(MetHit::Beats(-1)));
        assert_eq!(met_hit_test(screen, anchor, beats[2].center()), Some(MetHit::Beats(1)));
        assert_eq!(
            met_hit_test(screen, anchor, beats[1].center()),
            None,
            "the beat count acted like a button"
        );
        let sound = MetLayout::cells(l.rows[4], 2);
        // Default is the FIRST of the two and Custom the second — the order
        // the owner asked for, and the one the drawing paints.
        assert_eq!(met_hit_test(screen, anchor, sound[0].center()), Some(MetHit::SoundReset));
        assert_eq!(met_hit_test(screen, anchor, sound[1].center()), Some(MetHit::SoundPick));
        assert_eq!(met_hit_test(screen, anchor, l.close.center()), Some(MetHit::Close));

        // **COUNT-IN, on the beats row's own shape.** It moved here from the
        // Setup panel - two tiers away from the thing it counts - so it has
        // to answer like everything else the panel draws. The middle cell is
        // a READOUT and must not change anything, the same rule the beat
        // count follows three rows above it.
        let count = MetLayout::cells(l.rows[5], 3);
        assert_eq!(met_hit_test(screen, anchor, count[0].center()), Some(MetHit::CountIn(-1)));
        assert_eq!(met_hit_test(screen, anchor, count[2].center()), Some(MetHit::CountIn(1)));
        assert!(
            !matches!(
                met_hit_test(screen, anchor, count[1].center()),
                Some(MetHit::CountIn(_))
            ),
            "the count-in readout stepped the count"
        );
        // And the in-take row answers across its whole width: it is one
        // question with two words, not two controls.
        for cell in MetLayout::cells(l.rows[6], 2) {
            assert_eq!(
                met_hit_test(screen, anchor, cell.center()),
                Some(MetHit::CountInInTake)
            );
        }

        // The tempo answers along its whole ROW, not just the two-point well
        // — a hand a little high still moves it — and reads 0 at the left end
        // and 1 at the right.
        let track = met_tempo_track(screen, anchor);
        let y = l.rows[0].center().y;
        match met_hit_test(screen, anchor, Pos2::new(track.left() + 0.5, y)) {
            Some(MetHit::Tempo(t)) => assert!(t < 0.02, "the left end reads {t}"),
            other => panic!("the tempo track's left end answered {other:?}"),
        }
        match met_hit_test(screen, anchor, Pos2::new(track.right() - 0.5, y)) {
            Some(MetHit::Tempo(t)) => assert!(t > 0.98, "the right end reads {t}"),
            other => panic!("the tempo track's right end answered {other:?}"),
        }
        assert!(
            matches!(
                met_hit_test(screen, anchor, Pos2::new(track.center().x, l.rows[0].top() + 1.0)),
                Some(MetHit::Tempo(_))
            ),
            "the tempo row is a two-point target"
        );

        // Outside the panel is nobody's — the caller is what swallows the
        // press, and it needs `None` to know the press left.
        assert_eq!(met_hit_test(screen, anchor, Pos2::new(5.0, 5.0)), None);

        // **It stays on the surface wherever the button is**, including a
        // button hard against an edge, where a panel centred on it would
        // otherwise hang half off.
        for corner in [
            Pos2::new(0.0, 0.0),
            Pos2::new(1380.0, 0.0),
            Pos2::new(0.0, 880.0),
            Pos2::new(1380.0, 880.0),
        ] {
            let a = Rect::from_min_size(corner, Vec2::splat(22.0));
            let panel = met_popup_rect(screen, a);
            assert!(screen.contains_rect(panel), "at {corner:?} the panel left the surface");
        }
        // And a surface with no room panics on nothing.
        let tiny = Rect::from_min_size(Pos2::ZERO, Vec2::new(30.0, 20.0));
        let _ = met_popup_rect(tiny, anchor);
        let _ = met_hit_test(tiny, anchor, Pos2::new(5.0, 5.0));
    }

    /// **The close box always shows an X.**
    ///
    /// It was a shrunk glyph, and `fit_label` drops text under `MIN_TEXT`
    /// rather than drawing it small — so on a narrow panel the corner square
    /// came out empty: the owner's "a blank square to close it". Geometry has
    /// no such floor. This asks every square size the layout can produce for
    /// two real diagonals.
    #[test]
    fn the_close_box_is_never_a_blank_square() {
        for side in [4.0_f32, 6.0, 9.0, 12.0, 14.7, 20.0, 40.0] {
            let r = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::splat(side));
            let (arms, w) = close_x_arms(r)
                .unwrap_or_else(|| panic!("a {side}-point close box drew nothing"));
            assert!(w >= 1.0, "a {side}-point arm is {w} thick");
            for arm in arms {
                let d = arm[0] - arm[1];
                assert!(
                    d.x.abs() > 0.5 && d.y.abs() > 0.5,
                    "a {side}-point arm is not diagonal: {arm:?}"
                );
                for end in arm {
                    assert!(r.contains(end), "a {side}-point arm left the box at {end:?}");
                }
            }
            // The two arms cross, or it is a pair of parallel strokes.
            assert!(
                (arms[0][0].y - arms[1][0].y).abs() > 0.5,
                "the {side}-point arms do not cross"
            );
        }
        // And nothing at all for a square with no room.
        assert!(close_x_arms(Rect::NOTHING).is_none());
    }

    /// **The dial lands on whole beats; the FIELD is where decimals live.**
    ///
    /// The owner: "decimal tempos should only be typable in the field --
    /// draggin the dial should now only read non decimal values". Over
    /// KNOB_TRAVEL points the sweep is about a beat a point, so every drag
    /// used to land on something like 137.4212 under a readout that said 137.
    #[test]
    fn the_tempo_dial_lands_on_whole_beats() {
        for step in 0..=200 {
            let t = step as f32 / 200.0;
            let bpm = knob_to_tempo(t);
            assert!(
                (bpm - bpm.round()).abs() < 1e-9,
                "the dial produced {bpm} at {t}"
            );
            assert!(bpm >= f64::from(MIN_BPM) - 0.5);
            assert!(bpm <= f64::from(MAX_BPM) + 0.5);
        }
        // The ends are exact, not rounded off the edge of the range.
        assert_eq!(knob_to_tempo(0.0), f64::from(MIN_BPM).round());
        assert_eq!(knob_to_tempo(1.0), f64::from(MAX_BPM).round());

        // And a TYPED decimal survives: it never comes through the knob, and
        // the readout shows the tenth rather than hiding it.
        assert_eq!(crate::recorder::parse_bpm("137.5"), Some(137.5));
        assert_eq!(tempo_text(137.5), "137.5");
        assert_eq!(tempo_text(137.0), "137");
    }


    /// **The loudness meter is themed end to end: its well, its fill, its
    /// ticks and its numbers.**
    ///
    /// The owner's ask, and it is the rule this app has had to learn eight
    /// times: ink is a property of the SURFACE it lands on, never inherited
    /// from a themed role. The meter has four things stacked on one well and
    /// every one of them is measured here against the well it is actually
    /// drawn on, on all sixteen palettes, in both modes.
    #[test]
    fn every_theme_dresses_the_loudness_meter() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let p = palette(&set);
                let mode = if dark { "dark" } else { "light" };
                let well = surface_of(distinct_surface(p.lcd_face, p.bg));
                let mut check = |what: &str, fg: Color32, bg: Color32, floor: f32| {
                    let r = contrast_ratio(fg, bg);
                    if r < floor {
                        bad.push(format!("{} {mode}: {what} {r:.2}:1", theme.name));
                    }
                };
                // **The well against the panel**, or the meter is an
                // invisible rectangle somebody has to know is there.
                check("the well on the band", well.bg, p.bg, 1.25);
                // **The fill against the well** — both of them, since the
                // bar changes colour past the loudest target.
                let cool = distinct_surface_at(p.accent, well.bg, 2.0);
                let warm = distinct_surface_at(p.rec, well.bg, 2.0);
                check("the fill", cool, well.bg, 2.0);
                check("the hot fill", warm, well.bg, 2.0);
                // **The ticks and the numbers.** The ticks are drawn on the
                // WELL, the numbers beside it on the band.
                check("the ticks", well.faint, well.bg, 1.4);
                check("the numbers", surface_of(p.bg).faint, p.bg, 3.0);
                // And the reading under it.
                check("the reading", surface_of(p.bg).ink, p.bg, 4.5);
                // **Hot must be tellable from not-hot.** A meter that goes
                // red is only useful if the red is a different colour.
                let d = delta_e(cool, warm);
                if d < 20.0 {
                    bad.push(format!(
                        "{} {mode}: the hot fill is {d:.1} from the ordinary one",
                        theme.name
                    ));
                }
            }
        }
        assert!(
            bad.is_empty(),
            "the loudness meter is not readable:\n  {}",
            bad.join("\n  ")
        );
    }

    /// **The four clusters past the timecode's rule are evenly spaced, and
    /// the loudness meter is the master meter's width.**
    ///
    /// The VU faces and their knobs, the pitch dial, the loudness meter, the
    /// master. Sliced as four columns the DRAWN distances came out uneven
    /// however evenly the columns were cut, because a column's edge is not
    /// where its contents start — the loudness column began with a strip of
    /// air and the pitch dial's cell was mostly margin. Measured on what is
    /// drawn, which is what the eye measures.
    ///
    /// **And that last part was still only half true.** This measured to
    /// `master_scale[0].left()` — the scale's CELL — while the scale's
    /// numbers are right-aligned inside it and start a third of the way
    /// across. The loudness numbers had the mirror-image problem, being
    /// left-aligned in a cell nearly three times their width. So two pockets
    /// of dead air sat INSIDE the clusters, this test measured neither, and
    /// three separate attempts at the owner's "pitch dial worth of empty
    /// space" changed rectangles that were already even. The gaps below are
    /// now taken from the ink itself, through the same helpers the painter
    /// sizes its type with.
    #[test]
    fn the_four_clusters_at_the_right_end_are_evenly_spaced() {
        for w in [900.0_f32, 1300.0, 1503.0, 2000.0] {
            let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, band_height(w)));
            let v = RecorderView::empty();
            let l = Layout::new(r, &v);
            let Some(face) = vari_face(l.vari, l.vari_label) else { continue };
            if !l.lufs_bar.is_positive() || !l.master_bars.is_positive() {
                continue;
            }
            // The four, left to right, by what is DRAWN in each.
            let knobs = l.cell(Fx::Lpf).union(l.cell(Fx::Limiter));
            // Where the master's scale actually starts printing.
            let step = l.master_bars.height() / 9.0;
            let size = fit_text(
                Rect::from_min_size(
                    l.master_scale[0].min,
                    Vec2::new(l.master_scale[0].width() - 3.0, step),
                ),
                MASTER_SCALE_FIT,
                step * 0.86,
            );
            let master_ink_left = (l.master_scale[0].right() - 2.0
                - master_scale_ink_w(size))
            .max(l.master_scale[0].left());
            // **The loudness meter is CENTRED between the PITCH CONT text
            // and the master's numbers**, and the other two gaps are equal.
            //
            // The rule used to be three equal gaps measured from the dial's
            // FACE. The owner's correction: the word is wider than the circle
            // under it and the word is the last ink on that side, so the space
            // the eye judges runs from the WORD. Measured from there, three
            // equal gaps put the meter left of centre.
            let strip =
                (knob_face(l.vari).map_or(0.0, |k| k.label_h) * 2.0).max(KNOB_LABEL.1);
            let word_right = vari_word_ink(l.vari, l.vari_label, strip)
                .map_or(face.face.right(), |r| r.right());
            let (before, after) = (
                l.lufs_bar.left() - word_right,
                master_ink_left - l.lufs_ticks.right(),
            );
            assert!(
                before > 0.0 && after > 0.0,
                "the loudness meter touches a neighbour at {w}: {before:.1} / {after:.1}"
            );
            assert!(
                (before - after).abs() <= 1.0,
                "the loudness meter is not centred at {w}: {before:.1} before, {after:.1} after"
            );
            // And the dial still sits one clear gap off the knob group.
            assert!(
                face.face.left() - knobs.right() > 0.0,
                "the pitch dial runs into the knobs at {w}"
            );

            // **The loudness meter is a fixed share of the master meter's
            // width**, measured off it rather than stated twice so the two
            // cannot drift. It was one-to-one; see `LUFS_BAR_OF_MASTER` for
            // why a thinner strip.
            assert!(
                (l.lufs_bar.width() - l.master_bars.width() * LUFS_BAR_OF_MASTER).abs()
                    < 1.0,
                "the loudness meter is {:.1} wide against the master's {:.1} at {w}",
                l.lufs_bar.width(),
                l.master_bars.width()
            );
        }
    }

    /// **Nothing on this panel is bold.** The owner's call, and it is one
    /// grep rather than an eye: the band used the bold face for its words
    /// and the regular one for its numbers, which on a surface this dense
    /// read as two panels.
    #[test]
    fn the_band_prints_nothing_in_bold() {
        let src = include_str!("recorder_panel.rs");
        // The one mention that may remain is the retired helper, which is
        // named and unused so the face is not lost.
        // The DRAWING half of the file only: the test module below names
        // the face in its own assertions, which is not the band printing
        // anything.
        let drawing = &src[..src.find("\nmod tests {").unwrap_or(src.len())];
        let uses: Vec<&str> = drawing
            .lines()
            .filter(|l| l.contains("courier_bold()"))
            .map(str::trim)
            .collect();
        assert_eq!(
            uses,
            vec!["FontId::new(size, fonts::courier_bold())"],
            "the band is printing something in bold again"
        );
        // And no CALLER of it — the definition itself of course names it.
        let callers = drawing.matches("font_bold(").count();
        assert_eq!(
            callers, 1,
            "something calls the retired bold helper"
        );
    }

    /// **The band's right end: LOUDNESS, VARISPEED, MASTER, in that order,
    /// and the master hugs the edge.**
    ///
    /// Every part of this was a complaint. The varispeed had no column and
    /// stood at the loudness ladder's foot, so the ladder stopped short of
    /// the floor and the dial's reading had a knob's height to live in. The
    /// master was cut short of the band's right edge to leave the heart a
    /// corner. The owner asked for all of it.
    #[test]
    fn the_bands_right_end_is_three_columns_and_the_master_hugs_the_edge() {
        for w in [900.0_f32, 1300.0, 2000.0] {
            let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, band_height(w)));
            let v = RecorderView::empty();
            let l = Layout::new(r, &v);
            let ladder = l.lufs_bar.union(l.lufs_ticks);
            assert!(ladder.is_positive(), "no loudness ladder at {w}");
            assert!(l.vari.is_positive(), "no varispeed at {w}");
            assert!(l.master_bars.is_positive(), "no master at {w}");

            // Left to right — PITCH, LOUDNESS, MASTER — with no two of them
            // overlapping.
            assert!(
                l.vari.right() <= ladder.left(),
                "the pitch dial runs into the ladder at {w}"
            );
            // **Against the master's INK, not its cell.** Its dB numbers are
            // right-aligned inside that cell, so its left third is empty air —
            // and the loudness meter is now centred in the space between the
            // PITCH CONT text and where those numbers actually start, which
            // puts its own cell a hair into that emptiness. What must not
            // happen is the two sets of ink touching.
            let step = l.master_bars.height() / 9.0;
            let size = fit_text(
                Rect::from_min_size(
                    l.master_scale[0].min,
                    Vec2::new(l.master_scale[0].width() - 3.0, step),
                ),
                MASTER_SCALE_FIT,
                step * 0.86,
            );
            let master_ink = (l.master_scale[0].right() - 2.0 - master_scale_ink_w(size))
                .max(l.master_scale[0].left());
            assert!(
                ladder.right() <= master_ink,
                "the ladder runs into the master's numbers at {w}"
            );

            // **Nothing is to the right of the master.** Measured against
            // the band's own controls rather than against the window edge,
            // because the band keeps an inner margin all the way round and
            // "hugs the edge" means "is the last column", not "touches the
            // glass".
            // **The COLUMNS, not the status line.** The status line runs
            // the whole width under them and its dismiss button holds the
            // band's own bottom-right corner; it is not a column and never
            // was one, so it is not what "the master is last" is about.
            let rightmost = l
                .targets()
                .into_iter()
                .filter(|(rect, _p)| {
                    rect.is_positive()
                })
                .fold(f32::NEG_INFINITY, |acc, (rect, _)| acc.max(rect.right()));
            assert!(
                l.master_scale[1].right() >= rightmost - 1.0,
                "something reaches {rightmost} past the master's {} at {w}",
                l.master_scale[1].right()
            );

            // **The ladder runs the whole column**, with only its reading
            // under it — no dial takes a bite out of the bottom any more.
            assert!(
                l.lufs_bar.bottom() >= l.lufs_readout.top() - 4.0,
                "something is between the ladder and its reading at {w}"
            );

            // **The varispeed's cell is TALLER than its dial**, which is
            // what gives the reading a line of its own. Compared against the
            // master knob, which is the dial's own size.
            assert!(
                l.vari.height() > l.master_knob.height() + 3.0,
                "the varispeed reading has no room at {w}: cell {} vs knob {}",
                l.vari.height(),
                l.master_knob.height()
            );
            // ...and its dial still lines up with the master's.
            assert!(
                (l.vari.bottom() - l.master_knob.bottom()).abs() < 0.51,
                "the two dials sit on different lines at {w}"
            );

            // The master is THINNER than the loudness column is wide now —
            // the owner's trade, and the reason the ladder is readable.
            assert!(
                l.master_scale[1].right() - l.master_scale[0].left() < r.width() * 0.16,
                "the master is still the widest thing at the right end at {w}"
            );
        }
    }

    /// The heart keeps the corner it was moved to, and takes no press that
    /// belongs to anything else.
    #[test]
    fn the_heart_sits_on_the_columns_floor_and_steals_nothing() {
        for w in [900.0_f32, 1300.0, 2000.0] {
            let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, band_height(w)));
            let v = RecorderView::empty();
            let l = Layout::new(r, &v);
            let h = heart_rect(r, &v);
            if !h.is_positive() {
                continue;
            }
            assert!(r.contains_rect(h), "the heart left the band at {w}");
            // Bottom-RIGHT: past the middle in both axes.
            assert!(h.center().x > r.center().x, "the heart is not at the right at {w}");
            assert!(h.center().y > r.center().y, "the heart is not at the foot at {w}");
            // Clear of the master's knob, which is the control nearest it.
            assert!(
                !h.intersects(l.master_knob),
                "the heart is drawn over the master knob at {w}"
            );
            assert_eq!(
                hit_test(r, &v, h.center()),
                None,
                "the heart is sitting on a control at {w}"
            );
        }
    }
    use super::*;
    use crate::recorder::{Gains, Preview};

    pub(super) fn band(w: f32) -> Rect {
        Rect::from_min_size(Pos2::new(0.0, 350.0), Vec2::new(w, band_height(w)))
    }

    /// **Every control in the take-settings panel can actually be clicked.**
    ///
    /// The regression this exists for, in full: 4.19.0 moved the camera and the
    /// audio input out of this panel and onto the controls they feed, and took
    /// the AUDIO STATUS row out with them. That one was not moved anywhere.
    /// `Hit::ShowAudioStatus` kept its variant, kept its tooltip, kept its arm
    /// in `app.rs` — and had no rectangle in any layout, so the panel that says
    /// what rate the two streams are running at could not be opened from
    /// anywhere in the app. Nothing failed. Nothing warned. It was simply gone,
    /// and it stayed gone for a release.
    ///
    /// A rect-by-rect walk rather than a list of Hits, so this cannot be
    /// satisfied by a target that exists in `targets()` and is covered by
    /// something drawn over it.

    /// **One derivation for every floating panel, checked on every theme.**
    ///
    /// The frozen pair — black-on-ivory light, its inverse dark — was
    /// hard-coded SEVEN times: the right-click menus, the help card, the
    /// chord hover card, the dialogs, the theory band, the staff and the
    /// arrangement's chips. Each was found by a screenshot rather than a
    /// test, one at a time, over three rounds of the same report. This is the
    /// test that stops an eighth.
    #[test]
    fn every_theme_makes_a_readable_panel_out_of_its_own_face() {
        let mut bad: Vec<String> = Vec::new();
        let mut grounds = std::collections::HashSet::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = Settings::default();
                theme.apply(&mut set);
                let face = set.dark_role("fx_strip").filter(|_| dark).map_or_else(
                    || {
                        let c = set.fx_strip_color.to_color32();
                        if dark { crate::mixer_panel::shade_face(c, 0.88) } else { c }
                    },
                    |c| c.to_color32(),
                );
                let p = surface_of(face);
                grounds.insert((p.bg.r(), p.bg.g(), p.bg.b(), dark));
                let mode = if dark { "dark" } else { "light" };
                let mut check = |what: &str, fg: Color32, bg: Color32, floor: f32| {
                    let r = contrast_ratio(fg, bg);
                    if r < floor {
                        bad.push(format!("{} {mode}: {what} {r:.2}:1", theme.name));
                    }
                };
                // Words, wherever they land: the panel, a button, a field.
                check("ink on panel", p.ink, p.bg, 4.5);
                check("ink on a button", p.ink, p.raised, 4.5);
                check("ink on hover", p.ink, p.hover, 4.5);
                check("ink in a field", p.ink, p.well, 4.5);
                // And the parts have to be tellable from the panel.
                check("button edge", p.edge, p.bg, 1.6);
                check("button", p.raised, p.bg, 1.05);
                check("field", p.well, p.bg, 1.05);
                check("faint", p.faint, p.bg, 3.0);
            }
        }
        assert!(bad.is_empty(), "panels do not hold up:\n  {}", bad.join("\n  "));
        assert!(
            grounds.len() > 16,
            "only {} distinct panel grounds across sixteen themes and two modes",
            grounds.len()
        );
    }
    /// **The two shapes divide the panel between them, and neither is
    /// empty.**
    ///
    /// The cheap way to get this wrong is for a shape to lay out nothing at
    /// all and quietly draw an empty box - `targets()` would still be
    /// walkable, every assertion about "answers in its own shape" would hold
    /// vacuously, and the panel would be blank. So this counts.
    ///
    /// It also pins the split itself: the take tier is in `Take` and NOT in
    /// `Machine`, which is the whole of what the owner asked for. A change
    /// that quietly left the take rows in the cog's panel would pass every
    /// other test in this file.
    #[test]
    fn each_shape_shows_its_own_tier_and_only_its_own() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 720.0));
        let anchor = Rect::from_min_size(Pos2::new(40.0, 480.0), Vec2::new(20.0, 20.0));
        let machine = SetupLayout::new(screen, anchor, SetupShape::Machine);
        let take = SetupLayout::new(screen, anchor, SetupShape::Take);
        let inside = |l: &SetupLayout, r: Rect| l.panel.contains(r.center());

        // The take tier belongs to Take and has left the cog entirely.
        for (what, m, t) in [
            ("count-in", machine.count_in, take.count_in),
            ("signature", machine.time_sig, take.time_sig),
            ("the take's name", machine.name, take.name),
            ("the folder", machine.folder, take.folder),
            ("open when done", machine.open_when_done, take.open_when_done),
            ("count-in in take", machine.count_in_in_take, take.count_in_in_take),
        ] {
            assert!(inside(&take, t), "{what} is missing from the take panel");
            assert!(!inside(&machine, m), "{what} is still in the cog's panel");
        }

        // **And the red button's shape is neither.** It shows the AUDIO
        // tier's count-in and signature - the arrangement's own two - and
        // not one row of the take tier, whose EXPORT is the giveaway that
        // the wrong panel is up.
        let rolling = SetupLayout::new(screen, anchor, SetupShape::Rolling);
        for (what, r) in [
            ("the count-in", rolling.audio_count_in),
            ("the signature", rolling.audio_sig),
        ] {
            assert!(inside(&rolling, r), "{what} is missing from the red button's panel");
        }
        for (what, r) in [
            ("the take's name", rolling.name),
            ("the take's count-in", rolling.count_in),
            ("the export", rolling.export),
            ("the audio row", rolling.audio),
            ("the project row", rolling.project_row),
        ] {
            assert!(!inside(&rolling, r), "{what} is on the red button's panel");
        }
        assert_eq!(
            SetupShape::Rolling.title_word(),
            "RECORDING",
            "the red button's panel is headed as somebody else's"
        );
        assert_eq!(SetupShape::Take.title_word(), "TAKE");

        // And the machine tier belongs to the cog and is not on the take.
        for (what, m, t) in [
            ("the audio row", machine.audio, take.audio),
            ("the project row", machine.project_row, take.project_row),
            ("plugin folders", machine.plugin_folders, take.plugin_folders),
        ] {
            assert!(inside(&machine, m), "{what} is missing from the cog's panel");
            assert!(!inside(&take, t), "{what} leaked onto the take panel");
        }

        // **The take panel is SHORTER**, because it shows fewer rows. A panel
        // that kept the machine's height would be two thirds empty and read
        // as one that failed to load.
        assert!(
            take.panel.height() < machine.panel.height(),
            "the take panel is {} tall against the machine's {}",
            take.panel.height(),
            machine.panel.height()
        );
    }

    /// **Every control answers, in the shape that shows it — and in no
    /// other.**
    ///
    /// The panel has two shapes since the take tier moved onto the capture
    /// button: the cog opens the machine and where things are kept, the
    /// capture button opens the take. This walks BOTH and asserts the pair
    /// of properties that keeps them honest.
    ///
    /// A control inside the panel answers with itself, which is the original
    /// test. And a control belonging to the OTHER shape answers with nothing
    /// at all — that is what proves the tier is really gone from the cog's
    /// panel rather than merely painted over, and it is the half that would
    /// fail if a band were laid out on top of another one.
    #[test]
    fn every_take_settings_control_can_actually_be_clicked() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 720.0));
        let anchor = Rect::from_min_size(Pos2::new(40.0, 480.0), Vec2::new(20.0, 20.0));
        let v = idle();
        let mut seen_in_some_shape = 0usize;
        for shape in [SetupShape::Machine, SetupShape::Take, SetupShape::Rolling] {
            let l = SetupLayout::new(screen, anchor, shape);
            for (rect, want) in l.targets() {
                assert!(rect.is_positive(), "{want:?} has no rectangle at all in {shape:?}");
                let got = setup_hit_test(screen, anchor, &v, rect.center(), shape);
                if l.panel.contains(rect.center()) {
                    assert_eq!(got, Some(want), "{shape:?}: clicking {want:?} landed on {got:?}");
                    seen_in_some_shape += 1;
                } else {
                    // The other shape's tier, laid out where the clip and the
                    // panel test cannot reach it. See `SetupShape`.
                    assert_eq!(
                        got, None,
                        "{shape:?}: {want:?} belongs to the other shape and still answered"
                    );
                }
            }
            // The extras this panel alone reaches, each in its own shape.
            for (want, rect) in [
                (Hit::ToggleCountInInTake, l.count_in_in_take),
                (Hit::ToggleHideElapsed, l.hide_elapsed),
                (Hit::ShowAudioStatus, l.audio),
            ] {
                let got = setup_hit_test(screen, anchor, &v, rect.center(), shape);
                if l.panel.contains(rect.center()) {
                    assert_eq!(got, Some(want), "{shape:?}: {want:?} is not reachable");
                }
            }
        }
        assert!(
            seen_in_some_shape > 0,
            "neither shape showed a single control"
        );
    }

    /// Every row the shape shows, for the two size claims below.
    fn setup_rows_shown(l: &SetupLayout) -> Vec<Rect> {
        [
            l.name, l.dest, l.reveal, l.folder, l.disk, l.count_in, l.time_sig,
            l.export, l.open_when_done, l.click_in_take, l.count_in_in_take,
            l.hide_elapsed, l.audio, l.hear_input, l.audio_count_in, l.audio_sig,
            l.stream_row, l.project_row, l.plugin_folders, l.plugin_rescan,
            l.proj_dest, l.export_dest, l.preset_dest, l.irs_dest,
            l.audio_head, l.take_head, l.project_head,
        ]
        .into_iter()
        .filter(|r| l.panel.contains(r.center()))
        .collect()
    }

    /// **A row is the same size in all three shapes.**
    ///
    /// The height used to be the machine panel's scaled by how many rows a
    /// shape showed, so the red button's ONE row came out at a fraction of
    /// the height of the same control under the cog. The owner's words were
    /// "too small to be legible anyways". A shorter panel is right; shorter
    /// ROWS are not, and nothing in the file said so until this.
    #[test]
    fn a_row_is_the_same_size_in_every_shape_of_the_panel() {
        for w in [900.0_f32, 1280.0, 1920.0] {
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, w * 0.56));
            let anchor = Rect::from_min_size(Pos2::new(40.0, w * 0.3), Vec2::new(20.0, 20.0));
            // One row each shape actually SHOWS. The hidden ones are laid out
            // by a different allocator and would prove nothing.
            let sized = |shape: SetupShape| {
                let l = SetupLayout::new(screen, anchor, shape);
                let row = match shape {
                    SetupShape::Take => l.time_sig,
                    _ => l.audio_sig,
                };
                assert!(
                    l.panel.contains(row.center()),
                    "{shape:?} at {w} does not show the row this measures"
                );
                (row.height(), l.title.height())
            };
            let (row_m, title_m) = sized(SetupShape::Machine);
            for shape in [SetupShape::Take, SetupShape::Rolling] {
                let (row, title) = sized(shape);
                assert!(
                    (row - row_m).abs() < 0.5,
                    "{shape:?} at {w}: a row is {row:.1} against the cog's {row_m:.1}"
                );
                assert!(
                    (title - title_m).abs() < 0.5,
                    "{shape:?} at {w}: the title is {title:.1} against the cog's {title_m:.1}"
                );
            }
            assert!(row_m >= 12.0, "the cog's own rows are {row_m:.1} at {w}");
        }
    }

    /// **No shape leaves a row of nothing under its last control.**
    ///
    /// One row's worth of margin is the design; two is a panel that lost a
    /// tier and kept its height. The take tier left the cog's panel and the
    /// row COUNT did not follow it, which the owner met as "the removal of
    /// the take options from the main settings panel left an undesirable
    /// margin/empty space from the bottom".
    #[test]
    fn no_shape_leaves_a_row_of_nothing_under_its_last_control() {
        for w in [900.0_f32, 1280.0, 1920.0] {
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, w * 0.56));
            let anchor = Rect::from_min_size(Pos2::new(40.0, w * 0.3), Vec2::new(20.0, 20.0));
            for shape in [SetupShape::Machine, SetupShape::Take, SetupShape::Rolling] {
                let l = SetupLayout::new(screen, anchor, shape);
                let rows = setup_rows_shown(&l);
                assert!(!rows.is_empty(), "{shape:?} at {w} shows no rows at all");
                let bottom = rows.iter().map(|r| r.bottom()).fold(f32::MIN, f32::max);
                // A row's pitch, read back from a band: `band` insets its
                // rectangle to 0.10..0.92 of the pitch it stands on.
                let pitch = rows
                    .iter()
                    .map(|r| r.height())
                    .fold(f32::MIN, f32::max)
                    / 0.82;
                let slack = l.panel.bottom() - bottom;
                // **Tight enough to catch half a row.** A heading counted
                // for a shape that does not draw one is 0.55 of a row of
                // nothing, which a looser bound waves through; every shape
                // settles at 1.6 and the margin here is deliberate.
                assert!(
                    slack < pitch * 2.0,
                    "{shape:?} at {w}: {slack:.0}pt under the last control, {:.1} rows",
                    slack / pitch
                );
                assert!(
                    slack > pitch * 0.5,
                    "{shape:?} at {w}: the last control is jammed against the edge"
                );
            }
        }
    }

    const LONG_NODE: &str =
        "alsa_input.usb-Focusrite_Scarlett_Solo_USB_Y771VU50AAEF28-00.pro-input-0";

    /// **The AUDIO SETUP row names a long device at a size somebody can read.**
    ///
    /// On the owner's Linux box the row was an empty box: the caption and the
    /// PipeWire node name shrink together, and 84 characters in a 197-309 pt
    /// row fall under `MIN_TEXT`, where `label_text` draws nothing. The fixture
    /// reproduces that first, so this cannot pass vacuously.
    #[test]
    fn the_audio_setup_row_names_a_long_device_at_a_readable_size() {
        for w in [900.0_f32, 1117.0, 1280.0, 1440.0, 1920.0, 2560.0] {
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, w * 0.56));
            let anchor = Rect::from_min_size(Pos2::new(40.0, w * 0.3), Vec2::new(20.0, 20.0));
            let l = SetupLayout::new(screen, anchor, SetupShape::Machine);
            let inner = label_inner(l.audio).expect("the row has room");
            let want = inner.height() * 0.52;
            if w <= 1440.0 {
                assert!(
                    fit_text(inner, &format!("AUDIO SETUP {LONG_NODE}"), want) < MIN_TEXT,
                    "at {w} the old row would have drawn - the fixture no longer shows the bug"
                );
            }
            let (v, size) = device_row_text(inner, "AUDIO SETUP", LONG_NODE);
            assert!(size >= want * DEVICE_FLOOR - 1e-3, "at {w}pt the device is set at {size:.1}pt");
            assert!(
                v.contains('\u{2026}') && v.starts_with("alsa_input") && v.ends_with("input-0"),
                "at {w} the cut did not keep both ends: {v}"
            );
            assert!(
                (12 + v.chars().count()) as f32 * ADV * size <= inner.width() + 0.01,
                "at {w} what is drawn overruns the row"
            );
            // The tidied name the host now sends is drawn WHOLE.
            let (whole, size) = device_row_text(inner, "AUDIO SETUP", "Focusrite Scarlett Solo USB");
            assert!(matches!(whole, std::borrow::Cow::Borrowed(_)), "at {w} the short name was cut");
            assert!(size >= want * DEVICE_FLOOR - 1e-3);
            // Counted in characters: a trademark sign must not split a char.
            let win = "Microphone Array (Intel\u{ae} Smart Sound Technology for Digital Microphones)";
            let (v, _) = device_row_text(inner, "AUDIO SETUP", win);
            assert!(v.ends_with("Microphones)"), "at {w}: {v}");
        }
    }

    /// And the words really are PAINTED - the check above is arithmetic; this
    /// runs `draw_setup` and reads the text shapes back out of the row.
    #[test]
    fn the_audio_setup_row_paints_words_for_a_long_device_name() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        for w in [1117.0_f32, 1280.0, 1920.0] {
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, w * 0.56));
            let anchor = Rect::from_min_size(Pos2::new(40.0, w * 0.3), Vec2::new(20.0, 20.0));
            let l = SetupLayout::new(screen, anchor, SetupShape::Machine);
            for label in [DeviceLabel::Open(LONG_NODE), DeviceLabel::Missing(LONG_NODE)] {
                let view = RecorderView {
                    audio: label,
                    ..RecorderView::empty()
                };
                let out = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ctx| {
                        let painter = Painter::new(ctx.clone(), egui::LayerId::background(), screen);
                        draw_setup(&painter, screen, anchor, &view, &Settings::default(), SetupShape::Machine);
                    },
                );
                let mut texts: Vec<(String, f32, Rect)> = Vec::new();
                fn walk(shape: &egui::Shape, row: Rect, out: &mut Vec<(String, f32, Rect)>) {
                    match shape {
                        egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, row, out)),
                        egui::Shape::Text(t) => {
                            let r = t.visual_bounding_rect();
                            if row.contains(r.center()) {
                                let size = t.galley.job.sections.first().map_or(0.0, |s| s.format.font_id.size);
                                out.push((t.galley.text().to_owned(), size, r));
                            }
                        }
                        _ => {}
                    }
                }
                for c in &out.shapes {
                    walk(&c.shape, l.audio, &mut texts);
                }
                let cap = texts
                    .iter()
                    .find(|(t, ..)| t == "AUDIO SETUP")
                    .unwrap_or_else(|| panic!("the audio row is an empty box at {w}: {texts:?}"));
                let floor = label_inner(l.audio).unwrap().height() * 0.52 * DEVICE_FLOOR - 1e-3;
                let value = texts
                    .iter()
                    .find(|(t, ..)| t != "AUDIO SETUP" && !t.is_empty())
                    .unwrap_or_else(|| panic!("no device name painted at {w}: {texts:?}"));
                assert!(value.1 >= floor, "at {w} the name is painted at {:.1}pt", value.1);
                assert!(value.0.contains('\u{2026}'), "at {w} the name was not cut: {}", value.0);
                assert!(value.2.right() <= l.audio.right() && value.2.left() >= cap.2.right());
            }
        }
    }

    /// **Every SIG field shows its caret.** The signature is typed into the
    /// TAKE panel's SIG and into the AUDIO tier's, which the cog's panel and
    /// the red button's RECORDING panel show; only the TAKE panel's drew a
    /// caret, so in the other two the arrow keys moved one nobody could see.
    #[test]
    fn every_sig_field_shows_its_caret() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 720.0));
        let anchor = Rect::from_min_size(Pos2::new(40.0, 300.0), Vec2::new(20.0, 20.0));
        let edit = crate::recorder::NumEdit::prefilled(NumField::Meter, "7/8".to_owned());
        let view = RecorderView { editing: Some(&edit), ..RecorderView::empty() };
        let carets_in = |shape: SetupShape, row: fn(&SetupLayout) -> Rect| -> usize {
            let r = row(&SetupLayout::new(screen, anchor, shape));
            let out = ctx.run(
                egui::RawInput { screen_rect: Some(screen), ..Default::default() },
                |ctx| {
                    let painter = Painter::new(ctx.clone(), egui::LayerId::background(), screen);
                    draw_setup(&painter, screen, anchor, &view, &Settings::default(), shape);
                },
            );
            fn count(s: &egui::Shape, row: Rect) -> usize {
                match s {
                    egui::Shape::Vec(v) => v.iter().map(|s| count(s, row)).sum(),
                    egui::Shape::LineSegment { points, .. } => {
                        usize::from(row.contains(points[0].lerp(points[1], 0.5)))
                    }
                    _ => 0,
                }
            }
            out.shapes.iter().map(|c| count(&c.shape, r)).sum()
        };
        assert!(carets_in(SetupShape::Take, |l| l.time_sig) > 0, "the fixture draws no caret even in the TAKE panel");
        assert!(carets_in(SetupShape::Rolling, |l| l.audio_sig) > 0, "the RECORDING panel's SIG takes typing with no caret");
        assert!(carets_in(SetupShape::Machine, |l| l.audio_sig) > 0, "the cog's SIG takes typing with no caret");
    }

    /// **Under a take the rows a press cannot reach look it.**
    /// `setup_hit_test` refuses every row of the panel's `targets()` but DONE
    /// while a take writes, and the cassette's TAKE panel still opens then -
    /// with NAME, FOLDER, COUNT-IN, SIG, EXPORT and the rest drawn live,
    /// swallowing every press. Their words go to the faint ink; DONE and the
    /// two rows that stay live ("Count-in into take", "Hide elapsed time")
    /// keep the panel's own.
    #[test]
    fn under_a_take_the_refused_rows_are_drawn_refused() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 720.0));
        let anchor = Rect::from_min_size(Pos2::new(40.0, 300.0), Vec2::new(20.0, 20.0));
        let s = Settings::default();
        let p = palette(&s);
        let l = SetupLayout::new(screen, anchor, SetupShape::Take);
        // The colour each of these words is painted in, inside its row.
        let inks = |state: RecordState| -> Vec<(&'static str, Option<Color32>)> {
            let view = RecorderView { state, take_name: "nocturne", ..RecorderView::empty() };
            let out = ctx.run(
                egui::RawInput { screen_rect: Some(screen), ..Default::default() },
                |ctx| {
                    let painter = Painter::new(ctx.clone(), egui::LayerId::background(), screen);
                    draw_setup(&painter, screen, anchor, &view, &s, SetupShape::Take);
                },
            );
            fn walk(shape: &egui::Shape, row: Rect, word: &str, out: &mut Option<Color32>) {
                match shape {
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, row, word, out)),
                    egui::Shape::Text(t) if t.galley.text() == word && row.contains(t.visual_bounding_rect().center()) => {
                        *out = t.galley.job.sections.first().map(|s| s.format.color);
                    }
                    _ => {}
                }
            }
            [("nocturne", l.name), ("DONE", l.close), ("Hide elapsed time", l.hide_elapsed)]
                .into_iter()
                .map(|(word, row)| {
                    let mut ink = None;
                    for c in &out.shapes {
                        walk(&c.shape, row, word, &mut ink);
                    }
                    (word, ink)
                })
                .collect()
        };
        for (word, ink) in inks(RecordState::Idle) {
            assert_eq!(ink, Some(p.ink), "at rest {word:?} is not drawn live");
        }
        let rolling = inks(RecordState::Rolling);
        assert_eq!(rolling[0].1, Some(p.faint), "under a take the refused NAME is drawn live");
        for (word, ink) in &rolling[1..] {
            assert_eq!(*ink, Some(p.ink), "under a take {word:?}, which stays live, was dimmed");
        }
    }

    /// **The PLUGINS row still reads after the resync button took its right
    /// end**, at every window size the card is drawn at.
    ///
    /// `label_text` shrinks its words to fit and DRAWS NOTHING at all below
    /// `MIN_TEXT`, so taking eight percent of a row's width is not a free
    /// change: it moves the size at which the row stops existing. The row
    /// carries the longest caption-plus-value pair on the card, which makes
    /// it the one that hits the floor first.
    #[test]
    fn the_plugins_row_still_has_room_for_its_words() {
        for w in [900.0_f32, 1280.0, 1920.0, 2560.0] {
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, w * 0.56));
            let anchor = Rect::from_min_size(Pos2::new(40.0, w * 0.3), Vec2::new(20.0, 20.0));
            let l = SetupLayout::new(screen, anchor, SetupShape::Machine);
            let r = l.plugin_folders;
            let inset = label_inset(r);
            let inner = Rect::from_min_max(
                Pos2::new(r.left() + inset, r.top()),
                Pos2::new(r.right() - inset, r.bottom()),
            );
            let size = fit_text(inner, "PLUGINS Add a folder...", inner.height() * 0.52);
            assert!(
                size >= MIN_TEXT,
                "at {w}pt wide the PLUGINS row draws nothing ({size:.1}pt)"
            );
            // And the button beside it is a button, not a sliver.
            assert!(
                l.plugin_rescan.width() >= 8.0 && l.plugin_rescan.height() >= 8.0,
                "at {w}pt wide the resync button is {:.1}x{:.1}",
                l.plugin_rescan.width(),
                l.plugin_rescan.height()
            );
        }
    }

    /// **AUDIO then MEDIA in the cog, TAKE on its own** - the owner's order,
    /// now split across shapes: in the cog each tier's rows sit under its own
    /// heading, and no tier's rows reach into the next one.
    ///
    /// The TAKE tier used to be the third band of this panel and is the
    /// capture button's whole panel since the owner asked for it there; that
    /// panel, like the red button's, wears its tier's name as its TITLE and
    /// has no heading band, so its rows start right under the title.
    #[test]
    fn the_settings_tiers_read_audio_then_media_then_take() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 720.0));
        let anchor = Rect::from_min_size(Pos2::new(40.0, 300.0), Vec2::new(20.0, 20.0));

        // ── the cog: AUDIO, then MEDIA ──
        let l = SetupLayout::new(screen, anchor, SetupShape::Machine);
        for (name, r) in [("AUDIO heading", l.audio_head), ("MEDIA heading", l.project_head)] {
            assert!(l.panel.contains(r.center()), "{name} is not in the cog's panel");
        }
        assert!(
            l.audio_head.bottom() <= l.audio.top(),
            "the audio row is not under its heading"
        );
        assert!(
            l.audio.bottom() <= l.project_head.top()
                && l.hear_input.bottom() <= l.project_head.top(),
            "the AUDIO tier does not end before MEDIA begins"
        );
        assert!(
            l.project_head.bottom() <= l.proj_dest.top(),
            "the media rows do not start under their heading"
        );
        // And MEDIA is the last tier here now: its rows run to the floor
        // rather than stopping short for a TAKE heading that has left.
        for (what, r) in [
            ("project dest", l.proj_dest),
            ("export dest", l.export_dest),
            ("IRs dest", l.irs_dest),
        ] {
            assert!(l.panel.contains_rect(r), "{what} ran off the cog's panel");
        }

        // ── the capture button: TAKE, alone, and its name is the TITLE ──
        //
        // A one-tier panel headed SETTINGS over a heading reading TAKE wore
        // its own name twice. The word moved up into the title band and the
        // heading went with the rows it used to open; the rows start right
        // under the title now.
        for shape in [SetupShape::Take, SetupShape::Rolling] {
            let t = SetupLayout::new(screen, anchor, shape);
            assert_eq!(shape.title_word(), shape.tier_word());
            assert!(
                !t.panel.contains(t.take_head.center()),
                "{shape:?} still draws a tier heading under its own name"
            );
            assert!(t.title.is_positive(), "{shape:?} has no title band");
            assert!(t.panel.contains_rect(t.title), "{shape:?}'s title ran off its panel");
        }
        let t = SetupLayout::new(screen, anchor, SetupShape::Take);
        assert!(
            t.title.bottom() <= t.name.top(),
            "the take rows do not start under the title"
        );
        assert!(
            t.name.top() >= t.panel.top(),
            "the take rows start above their own panel"
        );
        // The cog keeps SETTINGS, because it is more than one tier.
        assert_eq!(SetupShape::Machine.title_word(), "SETTINGS");
        assert!(!SetupShape::Machine.title_names_its_tier());
        assert!(SetupShape::Take.title_names_its_tier());
        assert!(SetupShape::Rolling.title_names_its_tier());
    }

    /// No two controls in the panel share a pixel.
    ///
    /// Seven rows where there were six: the row that was added is the row most
    /// likely to have been laid over the one above it.
    #[test]
    fn the_take_settings_rows_do_not_overlap() {
        // **Both shapes, and only the rows each one shows.** The panel is
        // two shapes since the take tier moved onto the capture button, and
        // the tier a shape leaves out is laid out far below it - so "every
        // row is inside the panel" is a claim about the rows that shape
        // DRAWS, and the hidden ones are filtered out rather than excused.
        for (w, shape) in [900.0_f32, 1280.0, 1920.0]
            .into_iter()
            .flat_map(|w| {
                [
                    (w, SetupShape::Machine),
                    (w, SetupShape::Take),
                    (w, SetupShape::Rolling),
                ]
            })
        {
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, w * 0.56));
            let anchor = Rect::from_min_size(Pos2::new(40.0, w * 0.3), Vec2::new(20.0, 20.0));
            let l = SetupLayout::new(screen, anchor, shape);
            let all: Vec<(&str, Rect)> = vec![
                ("name", l.name),
                ("project", l.project_row),
                ("plugin folders", l.plugin_folders),
                ("plugin rescan", l.plugin_rescan),
                ("stream", l.stream_row),
                ("audio count-in", l.audio_count_in),
                ("audio sig", l.audio_sig),
                ("dest", l.dest),
                ("reveal", l.reveal),
                ("folder", l.folder),
                ("disk", l.disk),
                ("count-in", l.count_in),
                ("time sig", l.time_sig),
                ("export", l.export),
                ("open when done", l.open_when_done),
                ("click in take", l.click_in_take),
                ("count-in in take", l.count_in_in_take),
                ("hide elapsed", l.hide_elapsed),
                ("hear input", l.hear_input),
                ("audio setup", l.audio),
                ("close", l.close),
            ];
            // What this shape actually shows. A row whose centre is outside
            // the panel belongs to the other one.
            let zones: Vec<(&str, Rect)> = all
                .into_iter()
                .filter(|(_, r)| l.panel.contains(r.center()))
                .collect();
            // The red button's shape is two controls and the way out, and
            // saying so here is cheaper than a third rule about which rows a
            // shape owns.
            let least = if shape == SetupShape::Rolling { 3 } else { 4 };
            assert!(
                zones.len() >= least,
                "{shape:?} at {w} shows almost nothing ({} rows)",
                zones.len()
            );
            for i in 0..zones.len() {
                for j in (i + 1)..zones.len() {
                    if !zones[i].1.is_positive() || !zones[j].1.is_positive() {
                        continue;
                    }
                    assert!(
                        !zones[i].1.intersects(zones[j].1),
                        "{shape:?}: {} and {} overlap at {w}",
                        zones[i].0,
                        zones[j].0
                    );
                }
            }
            // And every one of them is inside the panel, which a seventh row
            // pushing past the bottom edge is exactly how it would not be.
            for (what, z) in &zones {
                if z.is_positive() {
                    assert!(
                        l.panel.contains_rect(*z),
                        "{shape:?}: {what} is outside the panel at {w}"
                    );
                }
            }
        }
    }

    /// **A held tempo says so to the keyboard too.** The knob and TAP stay
    /// drawn and stay a control, but while a take or an export holds the
    /// tempo a press on them is refused; the nodes a screen reader walks must
    /// report them disabled then, and enabled the moment the hold lifts,
    /// or the keys spent on them vanish into a control that ignores them.
    #[test]
    fn a_held_tempo_is_disabled_to_a_screen_reader_and_only_then() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1300.0, band_height(1300.0)));
        for held in [false, true] {
            let mut view = furnished();
            view.tempo_held = held;
            let nodes = a11y_nodes(rect, &view);
            for label in ["Tempo", "Tap tempo"] {
                let n = nodes
                    .iter()
                    .find(|n| n.label == label)
                    .unwrap_or_else(|| panic!("no {label:?} node (held {held})"));
                assert_eq!(n.enabled, !held, "{label:?} with the tempo held = {held}");
            }
            // Only those two: a hold is not a reason to grey out the band.
            let greyed: Vec<&str> =
                nodes.iter().filter(|n| !n.enabled).map(|n| n.label.as_str()).collect();
            let expect: &[&str] = if held { &["Tap tempo", "Tempo"] } else { &[] };
            let mut greyed_sorted = greyed.clone();
            greyed_sorted.sort_unstable();
            assert_eq!(greyed_sorted, expect, "held = {held}");
        }
    }

    /// **Every control in the band has words, and they are the words on it.**
    ///
    /// The band is one `Painter`; before AccessKit it was one empty rectangle
    /// to a screen reader, and to a keyboard it did not exist at all. This
    /// asserts the two things that make the nodes worth having: that the list
    /// covers the controls (drawn from `Layout::targets`, the panel's own
    /// enumeration, so it cannot drift from where the controls actually are),
    /// and that what each one SAYS is a sentence rather than a debug spelling.
    #[test]
    fn every_control_in_the_band_can_be_reached_and_says_what_it_is() {
        let view = furnished();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1300.0, band_height(1300.0)));
        let nodes = a11y_nodes(rect, &view);

        // **Coverage, against the panel's own enumeration.** Every target
        // that is really there is either named or on the surfaces list; a
        // control added to `Layout::targets` and forgotten here fails by
        // name rather than by a count nobody can interpret.
        let l = Layout::new(rect, &view);
        let mut unnamed: Vec<String> = Vec::new();
        for (r, produces) in l.targets() {
            if !r.is_positive() {
                continue;
            }
            let hit = produces.hit(r, r.center());
            if is_surface(&hit) {
                continue;
            }
            if !nodes.iter().any(|n| n.rect == r) {
                unnamed.push(format!("{hit:?}"));
            }
        }
        assert!(
            unnamed.is_empty(),
            "{} control(s) in the band have no words and no reason to be silent: {unnamed:#?}",
            unnamed.len()
        );

        for n in &nodes {
            assert!(!n.label.is_empty(), "a node has no label: {n:?}");
            // No debug spellings, no snake_case, no enum names leaking out.
            assert!(
                !n.label.contains('_') && !n.label.contains("Hit"),
                "{:?} reads like source rather than English",
                n.label
            );
            assert!(
                n.rect.is_positive() && rect.contains_rect(n.rect),
                "{:?} is outside the band it belongs to",
                n.label
            );
        }

        // The transport, the master and all six effect knobs are the controls
        // somebody reaches for; name them explicitly so a rename is a
        // decision rather than an accident.
        let said: Vec<String> = nodes.iter().map(crate::a11y::Node::spoken).collect();
        for want in ["Play", "Record", "Stop", "Master", "Tempo"] {
            assert!(
                said.iter().any(|s| s.starts_with(want)),
                "nothing in the band says {want:?}; it says {said:#?}"
            );
        }

        // A fader reads in dB and a filter in Hz — the number on the panel,
        // not the 0..1 underneath it. A screen reader saying "0.596" has
        // announced the implementation.
        let master = nodes
            .iter()
            .find(|n| n.label == "Master")
            .expect("the master has no node");
        assert!(
            master.spoken().contains("dB") || master.spoken().contains("OFF"),
            "the master reads as {:?} rather than in decibels",
            master.spoken()
        );
        assert!(matches!(master.kind, crate::a11y::Kind::Slider { .. }));

        // Every id is distinct, or focus lands on the wrong control.
        let mut ids: Vec<_> = nodes.iter().map(|n| n.id).collect();
        ids.sort_by_key(|i| format!("{i:?}"));
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "two controls in the band share an id");
    }

    /// The names survive every state the band has, and every size.
    ///
    /// A node built from a rect that collapsed at a small size would put a
    /// tab stop on nothing; one built while rolling must still be the control
    /// that is actually there.
    #[test]
    fn the_bands_names_hold_at_every_size_and_state() {
        for view in [idle(), rolling(), furnished(), furnished_rolling()] {
            for w in [560.0_f32, 900.0, 1300.0, 2600.0] {
                let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, band_height(w)));
                for n in a11y_nodes(rect, &view) {
                    assert!(
                        n.rect.is_positive(),
                        "{:?} collapsed at {w}pt",
                        n.label
                    );
                    assert!(
                        rect.contains_rect(n.rect),
                        "{:?} left the band at {w}pt: {:?} outside {rect:?}",
                        n.label,
                        n.rect
                    );
                }
            }
        }
    }

    /// **Every control in the band is drawn at every width the window can
    /// be, and the settings cog in particular.**
    ///
    /// The owner's report: at 100% on a 1440-point screen "the tangent window
    /// spawns a bit too small and cuts off the last and arguably most
    /// important button -- the settings". It was not cut off; at 900 points
    /// every button shrank to 9.1 and `draw_cog` refused to paint under a
    /// 5-point radius while the click target stayed live. Between 794 and 889
    /// points the whole send grid was refused too, and took the master, the
    /// pitch dial and the tempo knob with it (they are sized from its first
    /// cell) - the default window on a 1280 or 1366 screen. Every point from
    /// the window's floor to 2600, because the failures were a few points
    /// wide and alternated.
    #[test]
    fn every_control_is_drawn_at_every_width_the_window_allows() {
        let mut w = crate::settings::MIN_WINDOW.x;
        while w <= 2600.0 {
            for view in [idle(), rolling()] {
                if let Err(missing) = every_control_drawn(band(w), &view) {
                    panic!("at {w} points ({:?}) the band does not draw {missing:?}", view.state);
                }
            }
            w += 1.0;
        }
    }

    /// And the captions: from the width where the sends carry their words
    /// on, the TEMPO knob carries its own. It is the one knob that is a
    /// transport control, and at 900 points it was the one word missing.
    #[test]
    fn the_tempo_knob_is_named_at_the_100_percent_window_on_a_laptop() {
        for w in [900.0_f32, 918.0, 945.0, 960.0] {
            let l = Layout::new(band(w), &idle());
            let f = knob_face(l.tempo).expect("the tempo knob");
            assert!(f.label_h > 0.0, "at {w} the tempo knob has no caption strip");
            let top = Rect::from_min_max(l.tempo.min, Pos2::new(l.tempo.right(), l.tempo.top() + f.label_h));
            let size = fit_text(top, TEMPO_WORD, f.label_h * 0.9);
            assert!(size >= MIN_TEXT, "at {w} the word TEMPO would be set at {size:.2}");
        }
    }

    /// The cog is never a click target without a picture, in the band OR in
    /// the detached window, which lays the same row out at a different
    /// aspect.
    #[test]
    fn the_cog_is_never_pressable_where_it_is_not_drawn() {
        let mut rects: Vec<Rect> = Vec::new();
        let mut w = crate::settings::MIN_WINDOW.x;
        while w <= 2600.0 {
            rects.push(band(w));
            w += 1.0;
        }
        let mut dw = 400.0_f32;
        while dw <= 1400.0 {
            let mut dh = 200.0_f32;
            while dh <= 800.0 {
                rects.push(Rect::from_min_size(Pos2::ZERO, Vec2::new(dw, dh)));
                dh += 20.0;
            }
            dw += 20.0;
        }
        for r in rects {
            let l = Layout::new(r, &idle());
            assert!(
                !l.setup.is_positive() || cog_fits(l.setup),
                "at {r:?} the cog is pressable at {:?} but too small to draw",
                l.setup
            );
        }
        for d in [DETACHED_DEFAULT, DETACHED_MIN] {
            let r = Rect::from_min_size(Pos2::ZERO, d);
            assert!(cog_fits(Layout::new(r, &idle()).setup), "the detached window at {d:?} has no cog");
        }
    }

    /// An empty rack, which is what the app opens with.
    fn idle() -> RecorderView<'static> {
        RecorderView::empty()
    }

    fn rolling() -> RecorderView<'static> {
        RecorderView {
            state: RecordState::Rolling,
            elapsed_s: 252.0,
            ..RecorderView::empty()
        }
    }

    /// A backing track with a real wave in it, leaked once: the view borrows
    /// its track, and a fixture that built one per call could not return it.
    fn a_track() -> &'static crate::ports::TrackInfo {
        static T: std::sync::OnceLock<crate::ports::TrackInfo> = std::sync::OnceLock::new();
        T.get_or_init(|| crate::ports::TrackInfo {
            name: "song.wav".to_owned(),
            seconds: 200.0,
            wave: (0..600).map(|i| (i % 60) as f32 / 60.0).collect(),
            error: String::new(),
        })
    }

    /// A backing track loaded, which is the state in which the timeline is a
    /// LOCATE rather than an import button — the state most of the
    /// reachability tests need.
    fn furnished() -> RecorderView<'static> {
        RecorderView {
            track: a_track(),
            ..RecorderView::empty()
        }
    }

    fn furnished_rolling() -> RecorderView<'static> {
        RecorderView {
            state: RecordState::Rolling,
            elapsed_s: 252.0,
            ..furnished()
        }
    }

    fn with_rack(state: RecordState) -> RecorderView<'static> {
        RecorderView {
            state,
            ..furnished()
        }
    }

    /// Every state a take passes through, which several tests have to sweep.
    const STATES: [RecordState; 4] = [
        RecordState::Idle,
        RecordState::CountIn { beat: 3, of: 4 },
        RecordState::Rolling,
        RecordState::Finishing,
    ];

    /// **A knob drags up, a fader drags across.** The caller pins the axis a
    /// control does NOT travel on, so an answer of Horizontal for the knobs
    /// would freeze the only axis they move on — the control would be dead
    /// rather than fussy, and nothing about the picture would say so.
    #[test]
    fn each_control_reports_the_axis_it_actually_travels_on() {
        let r = band(1300.0);
        let v = with_rack(RecordState::Idle);
        for (hit, want) in [
            (Hit::SetFx(Fx::Reverb, 0.0), DragAxis::Vertical),
            (Hit::SetFx(Fx::Delay, 0.0), DragAxis::Vertical),
            (Hit::SetMaster(0.0), DragAxis::Vertical),
            (Hit::SetTempo(DEFAULT_BPM), DragAxis::Vertical),
        ] {
            assert_eq!(
                drag_axis(r, &v, hit),
                Some(want),
                "{} travels the other way",
                hit.label()
            );
        }
        // A button is not dragged at all, and a caller that treated one as a
        // fader would set a value every time the pointer moved over it.
        assert_eq!(drag_axis(r, &v, Hit::Record), None);
        assert_eq!(drag_axis(r, &v, Hit::Stop), None);
    }

    /// Both ends of a knob's travel are reachable, and up is more.
    /// **A filter knob reads in hertz, and a send still reads in percent.**
    /// **The tempo drags relatively, like every other knob.**
    ///
    /// It never did: `with_value` had no arm for it, so every frame of a drag
    /// re-applied the hit the PRESS produced — which is absolute. The knob
    /// jumped to wherever it was first touched and then would not move.
    #[test]
    fn the_tempo_knob_travels_with_the_hand() {
        // A drag is `control_value` plus how far the hand has moved, put
        // through `with_value`. Half way up the dial is half way up the range.
        let mid = Hit::SetTempo(0.0).with_value(0.5);
        let Hit::SetTempo(bpm) = mid else {
            panic!("the tempo lost its value: {mid:?}")
        };
        assert!(
            (bpm - (MIN_BPM + (MAX_BPM - MIN_BPM) * 0.5)).abs() < 0.01,
            "half travel is {bpm} BPM"
        );
        // Both ends, and the round trip through the position it reports.
        assert_eq!(Hit::SetTempo(0.0).with_value(0.0), Hit::SetTempo(MIN_BPM));
        assert_eq!(Hit::SetTempo(0.0).with_value(1.0), Hit::SetTempo(MAX_BPM));
        // **The round trip is to the nearest WHOLE beat**, because the dial
        // does not produce decimals any more — see `knob_to_tempo`. A typed
        // 92.5 still reads back as 92.5 on the knob's FACE; it is only the
        // hand that cannot land between two numbers.
        for bpm in [40.0_f64, 92.5, 120.0, 200.0] {
            let back = Hit::SetTempo(0.0).with_value(tempo_knob_position(bpm));
            let Hit::SetTempo(got) = back else { panic!() };
            assert!((got - got.round()).abs() < 1e-9, "{got} is not a whole beat");
            // Within half a beat, not exact: the position is an f32, so a
            // value on the .5 boundary can fall either side of it.
            assert!((got - bpm).abs() <= 0.51, "{bpm} came back as {got}");
        }
        // **And every control that travels goes through it.** The tempo was
        // the one that did not, and nothing said so. Two different values, so
        // this asks whether the value is USED rather than whether it happens
        // to differ from whatever `Hit::ALL` was built with.
        for h in Hit::ALL.into_iter().filter(|h| h.is_draggable()) {
            assert_ne!(
                h.with_value(0.2),
                h.with_value(0.8),
                "{h:?} ignores the value a drag hands it"
            );
        }
    }

    /// **The clip latch is cleared by pressing the meter it is shown on.**
    ///
    /// It was a word in the status line, which is a second place to look for a
    /// fact the VU's own lamp already carries — and on a Mac with no interface
    /// selected the lamp never lit at all, so the word was the only indication
    /// and it was the one that did not work. One indicator, in the place
    /// somebody watching levels is already looking, and pressing it is what
    /// puts it out.
    #[test]
    fn the_clip_latch_is_cleared_by_pressing_the_meter() {
        let r = band(1300.0);
        let v = idle();
        let l = Layout::new(r, &v);
        assert!(l.meter.is_positive(), "no meter to press");
        assert_eq!(hit_test(r, &v, l.meter.center()), Some(Hit::DismissClip));

        // Live as well as idle: a clip during a take is exactly when somebody
        // acknowledges one.
        let rolling = rolling();
        let l = Layout::new(r, &rolling);
        assert!(l.meter.is_positive());
        assert_eq!(hit_test(r, &rolling, l.meter.center()), Some(Hit::DismissClip));
    }

    /// **Pressing the preview chooses the camera** — and only at rest.
    #[test]
    fn the_preview_is_the_camera_picker_until_a_take_starts() {
        let r = band(1300.0);
        let v = idle();
        let l = Layout::new(r, &v);
        assert!(l.preview.is_positive(), "no preview");
        assert_eq!(hit_test(r, &v, l.preview.center()), Some(Hit::PickCamera));

        // Not mid-take: the camera cannot be changed then, and the dialog it
        // opens would be modal over the one gesture that matters, which is
        // Stop.
        let rolling = rolling();
        assert_ne!(
            hit_test(r, &rolling, Layout::new(r, &rolling).preview.center()),
            Some(Hit::PickCamera),
            "the picker is reachable during a take"
        );
    }

    /// **Every knob resets to the position where it is doing nothing** —
    /// except the two that are not effects, which have a resting value of
    /// their own.
    #[test]
    fn a_double_click_puts_every_knob_back() {
        use crate::recorder::fader_to_gain;
        for fx in Fx::ALL {
            // The limiter's dial is a threshold and the LPF's is a cutoff:
            // for both, "not applied" is fully clockwise — the limiter above
            // everything, the filter wide open at 20 kHz. The owner met the
            // old behavior as "double-click slams the LPF shut": a reset
            // that filtered everything OUT of a knob whose shipped state
            // filters nothing.
            // The LPF rests wide open; the LIMITER rests where it ships,
            // which is in circuit — see `recorder::LIMITER_SHIPS_AT`.
            let rest = match fx {
                Fx::Lpf => 1.0,
                Fx::Limiter => crate::recorder::LIMITER_SHIPS_AT,
                _ => 0.0,
            };
            assert_eq!(
                Hit::SetFx(fx, 0.77).reset_to(),
                Some(Hit::SetFx(fx, rest)),
                "{} does not reset to nothing applied",
                fx.title()
            );
        }
        assert_eq!(Hit::SetTempo(180.0).reset_to(), Some(Hit::SetTempo(120.0)));
        // The master goes to UNITY, which is 0 dB, not to the bottom of its
        // travel — a master that reset to silence would be a reset nobody
        // could use.
        let Some(Hit::SetMaster(back)) = Hit::SetMaster(0.1).reset_to() else {
            panic!("the master does not reset")
        };
        assert!(
            (fader_to_gain(back) - 1.0).abs() < 1.0e-4,
            "the master resets to {} dB",
            20.0 * fader_to_gain(back).log10()
        );

        // A button has nothing to put back.
        assert_eq!(Hit::Record.reset_to(), None);
        assert_eq!(Hit::Type(NumField::Tempo).reset_to(), None);
    }

    /// The eight knobs are exactly the controls with the knob gesture set.
    #[test]
    fn the_knobs_are_the_ones_that_look_like_knobs() {
        let knobs: Vec<Hit> = Hit::ALL.into_iter().filter(|h| h.is_knob()).collect();
        // The eight effects, the master, the tempo and the varispeed.
        assert_eq!(knobs.len(), Fx::ALL.len() + 3, "{knobs:?}");
        assert!(knobs.iter().any(|h| h.is_same_control(Hit::SetVarispeed(0.0))));
        for fx in Fx::ALL {
            assert!(knobs.iter().any(|h| h.is_same_control(Hit::SetFx(fx, 0.0))));
        }
        assert!(knobs.iter().any(|h| h.is_same_control(Hit::SetTempo(0.0))));
        assert!(knobs.iter().any(|h| h.is_same_control(Hit::SetMaster(0.0))));
        // And every one of them can be typed into, which is what the
        // right-click does — except the pitch dial, which prints no number
        // to type against.
        for h in knobs {
            if h.is_same_control(Hit::SetVarispeed(0.0)) {
                assert!(h.reset_to().is_some(), "the pitch dial cannot be reset");
                continue;
            }
            assert!(num_field(h).is_some(), "{h:?} cannot be typed into");
            assert!(h.reset_to().is_some(), "{h:?} cannot be reset");
        }
    }

    /// **The master ladder is linear in decibels**, which is the only scale
    /// on which the bottom half of a meter ever moves.
    #[test]
    fn the_master_ladder_is_a_decibel_scale() {
        assert!((master_fraction(1.0) - 1.0).abs() < 1.0e-4, "0 dBFS is the top");
        assert_eq!(master_fraction(0.0), 0.0, "silence is the floor");
        // -60 dBFS is the floor, and half way up is -30.
        let floor = 10f32.powf(MASTER_FLOOR_DB / 20.0);
        assert!(master_fraction(floor) < 1.0e-4);
        let half = 10f32.powf((MASTER_FLOOR_DB / 2.0) / 20.0);
        assert!((master_fraction(half) - 0.5).abs() < 0.01, "-30 dB is not the middle");
        // Past full scale it pins rather than running off the top.
        assert_eq!(master_fraction(4.0), 1.0);

        // The colours change where they say they do, and nowhere else.
        let at = |db: f32| led_colour((db - MASTER_FLOOR_DB) / -MASTER_FLOOR_DB);
        assert_eq!(at(-40.0), LED_GREEN);
        assert_eq!(at(-19.0), LED_GREEN);
        assert_eq!(at(-17.0), LED_AMBER);
        assert_eq!(at(-7.0), LED_AMBER);
        assert_eq!(at(-5.0), LED_RED);
        assert_eq!(at(0.0), LED_RED);
    }

    /// **The master column sits beside the band, not on top of it.**
    ///
    /// Four new rectangles went into a row that was already full. Every one of
    /// them has to clear the knobs to its left and the slots to its right, at
    /// every width — an overlap here is a control that cannot be pressed.
    #[test]
    fn the_master_column_clears_everything_around_it() {
        for w in [500.0_f32, 900.0, 1300.0, 1800.0] {
            let r = band(w);
            for v in [idle(), rolling()] {
                let l = Layout::new(r, &v);
                // **First: it is there at all.** Every assertion below skips a
                // rectangle that is NOTHING, so without this the whole test
                // passes on a layout that simply never placed the master.
                if w >= 900.0 {
                    assert!(l.master_bars.is_positive(), "no master ladder at {w}");
                    assert!(l.master_knob.is_positive(), "no master knob at {w}");
                }
                let master = [
                    ("scale L", l.master_scale[0]),
                    ("scale R", l.master_scale[1]),
                    ("bars", l.master_bars),
                    ("knob", l.master_knob),
                ];
                for (name, m) in master {
                    if !m.is_positive() {
                        continue;
                    }
                    assert!(
                        r.contains_rect(m),
                        "the master {name} is outside the band at {w}"
                    );
                    for (other, o) in Fx::ALL.map(|fx| (fx.title(), l.cell(fx))) {
                        if o.is_positive() {
                            assert!(
                                m.left() >= o.right() - 0.5,
                                "the master {name} overlaps {other} at {w}"
                            );
                        }
                    }
                    if l.meter.is_positive() {
                        assert!(
                            m.left() >= l.meter.right() - 0.5,
                            "the master {name} overlaps the VU at {w}"
                        );
                    }
                }
                // And the knob is an effect knob's size, because it is one.
                if l.master_knob.is_positive() && l.fx[0].is_positive() {
                    assert!(
                        (l.master_knob.height() - l.fx[0].height()).abs() < 0.5,
                        "the master knob is {} high and an effect knob is {} at {w}",
                        l.master_knob.height(),
                        l.fx[0].height()
                    );
                }
            }
        }
    }

    /// The limiter and the master do not wear the same cap.
    ///
    /// They sit two inches apart and mean opposite things — one holds the
    /// output down, the other is the only control that can push it up past
    /// what the first one guaranteed.
    /// **The tempo knob is the same size as the six it sits with.**
    ///
    /// It is sized from an effect cell on purpose — they are the same control
    /// and the eye reads them as a set. Spacing the row by the knob's visible
    /// extent nearly cost that: `knob_face` budgets the radius from the
    /// SHORTER side of its cell, so a cell narrowed below the drawing's own
    /// width feeds back and shrinks the knob. The owner caught it by eye.
    #[test]
    fn the_tempo_knob_is_exactly_the_size_of_an_effect_knob() {
        for w in [900.0_f32, 1300.0, 1800.0] {
            let v = idle();
            let l = Layout::new(band(w), &v);
            let (Some(tempo), Some(fx)) = (knob_face(l.tempo), knob_face(l.fx[0])) else {
                continue;
            };
            assert!(
                (tempo.radius - fx.radius).abs() < 0.01,
                "at {w}pt the tempo knob is {:.2} where the effect knobs are {:.2}",
                tempo.radius,
                fx.radius
            );
        }
    }

    /// **The varispeed dial, and its word, on all sixteen.**
    ///
    /// The owner asked for this one by name — "whole dial must be subject and
    /// tested WITH IT's LABEL TEST LEGIBLE in all 16" — and it is the right
    /// thing to ask for. Every ink failure this app has had was found by
    /// looking at a screenshot rather than by a test, seven times over, and
    /// every one of them was a colour inherited from a role instead of
    /// measured against the surface it actually landed on.
    ///
    /// So this measures each part against the ground it is drawn on, in both
    /// modes, on every shipped theme:
    ///
    /// - the WORD under the dial, which is what the request named — twice,
    ///   because it is drawn faint at rest and in full ink when engaged;
    /// - the READING above it, same two states;
    /// - the SLOT on the dial's face, which is the pointer and the only thing
    ///   that says where the control is set;
    /// - the recess the dial sits in, against the band, so a dial on a pale
    ///   theme is still visibly a dial and not a flat disc.
    #[test]
    fn every_theme_dresses_the_varispeed_dial_legibly() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let p = palette(&set);
                let mode = if dark { "dark" } else { "light" };
                let mut check = |what: &str, fg: Color32, bg: Color32, floor: f32| {
                    let r = contrast_ratio(fg, bg);
                    if r < floor {
                        bad.push(format!("{} {mode}: {what} {r:.2}:1", theme.name));
                    }
                };
                // **The word, both ways it is drawn.** `faint` carries it at
                // rest and `ink` when the dial is engaged; a theme that made
                // either unreadable would be a control nobody can name.
                check("VARI at rest", p.faint, p.bg, 4.5);
                check("VARI engaged", p.ink, p.bg, 4.5);
                // The reading over it, which is the number that says whether
                // the take is about to come out at the wrong pitch.
                check("the reading", p.ink, p.bg, 4.5);
                check("the reading at rest", p.ink.gamma_multiply(0.7), p.bg, 3.0);
                // The recess, against the band it is cut into. A LOWER bar
                // than text on purpose: this is a shape, not a glyph, and
                // WCAG's 3:1 for non-text is the right one to hold it to.
                let well = shade(p.well, if wants_light_ink(p.bg) { 0.82 } else { 0.94 });
                check("the recess", well, p.bg, 1.18);
                // The SLOT, on the dial's own face — measured against that
                // face, never picked from the theme. This is the rule that
                // seven separate ink bugs were each a violation of.
                let dial = surface_of(p.varispeed);
                check("the slot", dial.ink, dial.bg, 4.5);
                // And the face against the recess, so the dial is visible in
                // the hole it sits in.
                check("the face in its recess", dial.bg, well, 1.6);
                // The detent mark on the recess, which is what says where OFF
                // is: the accent at rest, the panel's line when engaged.
                check("the detent at rest", p.accent, well, 1.6);
                // A 1.4-point mark, so WCAG's 3:1 for non-text — and it is
                // held to it rather than to whatever a fixed colour happens
                // to give, which is where `Palette::line` left it at 1.21:1
                // on Centennial.
                check("the detent engaged", surface_of(well).faint, well, 3.0);
                // The − and + beside it.
                check("the ends", p.faint, p.bg, 4.5);
            }
        }
        assert!(bad.is_empty(), "{} unreadable: {bad:#?}", bad.len());
    }

    /// The varispeed dial is not any of the eight, and no theme can paint it
    /// into one.
    ///
    /// It sits between the LIMITER and the MASTER — the two knobs that decide
    /// whether a take clips — and it does something neither of them does. A
    /// theme that landed it on either colour would put three controls that
    /// mean three different things in one livery, in the one corner of the
    /// band where that matters most.
    #[test]
    fn no_theme_paints_the_varispeed_into_a_knob_beside_it() {
        for t in crate::theme::factory() {
            let Some(vs) = t.varispeed else {
                panic!("{} has no varispeed colour", t.name);
            };
            let vs = vs.to_color32();
            let mut set = Settings::default();
            t.apply(&mut set);
            let p = palette(&set);
            // **ΔE and not a contrast ratio.** The question is "can a person
            // tell these two apart", which is perceptual distance; contrast
            // is a LUMINANCE ratio and says two colours of the same
            // brightness and opposite hue are identical. That mistake failed
            // all sixteen themes at 1.5:1 once already.
            for (name, other) in [
                ("the master", p.master),
                ("the limiter", LIMITER_CAP),
                ("the gain", GAIN_CAP),
                ("the effect knobs", p.fx_knob),
            ] {
                let d = delta_e(vs, other);
                assert!(
                    d > 25.0,
                    "{}: the varispeed ({vs:?}) and {name} ({other:?}) are {d:.1} apart",
                    t.name
                );
            }
        }
    }

    #[test]
    fn the_limiter_and_the_master_are_told_apart_by_colour() {
        // **The colour the master actually wears**, asked of the settings
        // rather than of a constant. These two lines used to compare against
        // `MASTER_CAP`, a hardcoded red this panel stopped painting when the
        // knob became `master_knob_color` — so they were holding the effects
        // apart from a colour nothing draws.
        let master = Settings::default().master_knob_color.to_color32();
        assert_ne!(Fx::Limiter.cap(), master);
        for fx in Fx::ALL {
            assert_ne!(fx.cap(), master, "{} wears the master's red", fx.title());
        }
        // **And under every theme, not just the stock colours.** Themes paint
        // the master; if one of them lands on the limiter's red, the two
        // controls that mean opposite things become the same colour on
        // somebody's screen. Held to a real distance, not mere inequality.
        for t in crate::theme::factory() {
            let Some(master) = t.master_knob else { continue };
            let master = master.to_color32();
            let d = |a: Color32, b: Color32| {
                (f32::from(a.r()) - f32::from(b.r())).abs()
                    + (f32::from(a.g()) - f32::from(b.g())).abs()
                    + (f32::from(a.b()) - f32::from(b.b())).abs()
            };
            assert!(
                d(master, LIMITER_CAP) > 60.0,
                "{}: the master ({:?}) and the limiter ({:?}) are the same red",
                t.name,
                master,
                LIMITER_CAP
            );
        }
    }

    #[test]
    fn a_knob_reads_out_in_its_own_unit() {
        assert_eq!(knob_reading(KnobUnit::Percent, 0.42), "42%");
        assert_eq!(knob_reading(KnobUnit::Percent, 0.0), "0%");
        assert_eq!(knob_reading(KnobUnit::Percent, 1.0), "100%");

        // The high-pass, as the host sweeps it.
        let hp = KnobUnit::Hertz {
            low: 20.0,
            high: 1_200.0,
        };
        assert_eq!(knob_reading(hp, 0.0), "20 Hz");
        assert_eq!(knob_reading(hp, 1.0), "1.2 kHz");
        // Half way up an exponential sweep is the GEOMETRIC middle, which is
        // the whole reason a filter dial is logarithmic: sqrt(20 * 1200).
        assert_eq!(knob_reading(hp, 0.5), "155 Hz");

        // The limiter is a threshold, linear in decibels: taking the log of
        // something that already is one would put the useful half of the dial
        // in the last eighth of the travel.
        let th = KnobUnit::Decibels {
            low: 0.0,
            high: -30.0,
        };
        assert_eq!(knob_reading(th, 0.0), "0.0 dB");
        assert_eq!(knob_reading(th, 0.5), "-15.0 dB");
        assert_eq!(knob_reading(th, 1.0), "-30.0 dB");

        // The low-pass ascends like every dial now: the top of the travel
        // is 20 kHz and OUT OF THE WAY.
        let lp = KnobUnit::Hertz {
            low: 200.0,
            high: 20_000.0,
        };
        assert_eq!(knob_reading(lp, 1.0), "20 kHz");
        assert_eq!(knob_reading(lp, 0.0), "200 Hz");
        assert_eq!(knob_reading(lp, 0.5), "2 kHz");
        // And "20k" is how anybody writes the top of it.
        assert_eq!(knob_typed(lp, "20k"), Some(1.0));
        assert_eq!(knob_typed(lp, "200"), Some(0.0));
    }

    /// Typing into a filter is typing a frequency, and it lands there.
    #[test]
    fn a_typed_frequency_lands_on_the_right_part_of_the_dial() {
        let hp = KnobUnit::Hertz {
            low: 20.0,
            high: 1_200.0,
        };
        // Round trip: whatever is typed reads back as itself.
        for text in ["20", "155", "480", "1.2k", "1200 Hz", "800hz"] {
            let t = knob_typed(hp, text).expect("{text} was refused");
            let back = knob_reading(hp, t);
            let want: f32 = text
                .trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != 'k')
                .trim_end_matches('k')
                .parse()
                .unwrap();
            let want = if text.contains('k') { want * 1_000.0 } else { want };
            let got: f32 = back
                .split_whitespace()
                .next()
                .unwrap()
                .parse()
                .unwrap();
            let got = if back.contains('k') { got * 1_000.0 } else { got };
            assert!(
                (got - want).abs() / want < 0.02,
                "{text} came back as {back}"
            );
        }
        // A threshold is typed in decibels, with or without the unit.
        let th = KnobUnit::Decibels {
            low: 0.0,
            high: -30.0,
        };
        for text in ["-6", "-6 dB", "-6dB", " -6 "] {
            let t = knob_typed(th, text).unwrap_or_else(|| panic!("{text} was refused"));
            assert_eq!(knob_reading(th, t), "-6.0 dB", "{text}");
        }
        assert_eq!(knob_typed(th, "0"), Some(0.0));
        // Past either end it pins, and a positive threshold is not a thing
        // this dial has.
        assert_eq!(knob_typed(th, "-90"), Some(1.0));
        assert_eq!(knob_typed(th, "+6"), Some(0.0));
        assert_eq!(knob_typed(th, "loud"), None);

        // Off the ends, a real wish gets the nearest thing this dial has
        // rather than a refusal.
        assert_eq!(knob_typed(hp, "5"), Some(0.0));
        assert_eq!(knob_typed(hp, "9000"), Some(1.0));
        // And nonsense is refused rather than read as silence.
        assert_eq!(knob_typed(hp, "abc"), None);
        assert_eq!(knob_typed(hp, ""), None);
        assert_eq!(knob_typed(hp, "-40"), None);
        assert_eq!(knob_typed(hp, "0"), None);
    }

    #[test]
    fn a_knob_reads_zero_at_the_bottom_and_one_at_the_top() {
        let r = band(1300.0);
        let v = idle();
        // The dial, not the whole cell: the READING below it is the type
        // field, and it moved there when the name and the number swapped.
        let cell = {
            let c = Layout::new(r, &v).fx[Fx::Reverb.index()];
            let label = knob_face(c).map_or(0.0, |f| f.label_h);
            Rect::from_min_max(c.min, Pos2::new(c.right(), c.bottom() - label))
        };
        assert!(cell.is_positive(), "the knob has no rectangle to drag in");
        let at = |y: f32| hit_test(r, &v, Pos2::new(cell.center().x, y));
        // Half a point in from the bottom edge, which is the last point inside
        // the rect — so this is "as low as the pointer can get", not exactly 0.
        let Some(Hit::SetFx(Fx::Reverb, low)) = at(cell.bottom() - 0.5) else {
            panic!("the bottom of the knob is not the knob")
        };
        // **A point of travel, not a fixed 2%.** The knobs are half the height
        // they were now that there are six of them in two rows, so half a point
        // is twice the fraction of the travel it used to be — an absolute
        // tolerance here fails on a layout change rather than on a bug.
        assert!(
            low <= 1.0 / cell.height(),
            "the bottom of the travel reads {low} in a cell {} high",
            cell.height()
        );
        assert_eq!(at(cell.top()), Some(Hit::SetFx(Fx::Reverb, 1.0)));
        // And the middle is the middle, not an end.
        let Some(Hit::SetFx(Fx::Reverb, mid)) = at(cell.center().y) else {
            panic!("the middle of the knob is not the knob")
        };
        assert!((mid - 0.5).abs() < 0.05, "the centre reads {mid}");
    }

    /// **The camera sits in equal margins.** It is the inset a take carries,
    /// so it is a picture rather than a control, and a picture hung with a
    /// different gap under it than beside it reads as an accident. The status
    /// line used to run underneath it, which is what made the bottom margin
    /// twice the others.
    #[test]
    fn the_preview_has_the_same_margin_above_left_and_below_it() {
        for w in [500.0_f32, 900.0, 1300.0, 1800.0] {
            let r = band(w);
            for v in [idle(), rolling()] {
                let l = Layout::new(r, &v);
                assert!(l.preview.is_positive(), "no preview at {w}pt");
                let left = l.preview.left() - r.left();
                let top = l.preview.top() - r.top();
                let bottom = r.bottom() - l.preview.bottom();
                for (name, got) in [("top", top), ("bottom", bottom)] {
                    assert!(
                        (got - left).abs() < 0.5,
                        "at {w}pt the preview has {left:.1} to the left and {got:.1} {name}"
                    );
                }
                // And it really is 16:9, which is what makes it the shape of
                // the file rather than the shape of whatever was left over.
                let aspect = l.preview.width() / l.preview.height();
                assert!(
                    (aspect - PREVIEW_ASPECT).abs() < 0.02 || l.preview.width() < r.width() * 0.24,
                    "the preview is {aspect:.2}:1 at {w}pt"
                );
            }
        }
    }

    /// Nothing else may sit in the preview's column — that is what the equal
    /// margin means. The status line ran under it before.
    #[test]
    fn nothing_shares_the_previews_column() {
        let r = band(1300.0);
        let v = idle();
        let l = Layout::new(r, &v);
        for (name, other) in [
            ("setup", l.setup),
            ("capture", l.capture),
            ("name", l.name),
            ("tempo", l.tempo),
            ("meter", l.meter),
            ("reverb", l.cell(Fx::Reverb)),
            ("limiter", l.cell(Fx::Limiter)),
        ] {
            if !other.is_positive() {
                continue;
            }
            assert!(
                other.left() >= l.preview.right(),
                "{name} overlaps the preview: it starts at {:.1} and the preview ends at {:.1}",
                other.left(),
                l.preview.right()
            );
        }
    }

    /// The controls that stay reachable once a take is live.
    ///
    /// The desk owns the per-channel levels now, so what survives in the BAND
    /// is what leaves the room: the master, the effects, the way of putting
    /// the clip lamp out - and the whole transport, because a take is capture
    /// only and the song under it is still played, stopped and recorded from
    /// these buttons. The pickers and the import do not: each one opens a
    /// dialog over a take that cannot change its devices or its destination.
    const SURVIVORS: [Hit; 29] = [
        Hit::Record,
        Hit::Play,
        Hit::Stop,
        Hit::Pause,
        // The tempo stays too - its knob, its TAP chip and its typed word.
        // During a count-in they refuse (the APP does, see
        // `RecorderState::tempo_held`), but they are still where the hand
        // expects them, so a press on one is never a press on empty band.
        Hit::SetTempo(0.0),
        Hit::TapTempo,
        Hit::Type(NumField::Tempo),
        // The cassette started this take, and pressing it again is how it
        // ends, or how its count-in is cancelled - the one control that
        // reaches the take at all.
        Hit::Capture,
        // The master most of all: it is the level control somebody reaches
        // for at 0:47, and a take is exactly when.
        Hit::SetMaster(0.0),
        // Pressing the VU puts its clip lamp out, and a clip during a take is
        // exactly when somebody does that.
        Hit::DismissClip,
        // **And every number that belongs to something that survives.** A
        // level you can drag mid-take is a level you can type mid-take.
        Hit::Type(NumField::Master),
        Hit::Type(NumField::Fx(Fx::Reverb)),
        Hit::Type(NumField::Fx(Fx::Delay)),
        Hit::Type(NumField::Fx(Fx::Chorus)),
        Hit::Type(NumField::Fx(Fx::Phaser)),
        Hit::Type(NumField::Fx(Fx::Hpf)),
        Hit::Type(NumField::Fx(Fx::Lpf)),
        Hit::Type(NumField::Fx(Fx::Gain)),
        Hit::Type(NumField::Fx(Fx::Limiter)),
        Hit::SetFx(Fx::Reverb, 0.0),
        Hit::SetFx(Fx::Delay, 0.0),
        Hit::SetFx(Fx::Chorus, 0.0),
        Hit::SetFx(Fx::Phaser, 0.0),
        // The filters and the limiter survive a take for the same reason the
        // sends do: they are the sound, and a take is when somebody wants to
        // change it.
        Hit::SetFx(Fx::Hpf, 0.0),
        Hit::SetFx(Fx::Lpf, 0.0),
        Hit::SetFx(Fx::Gain, 0.0),
        Hit::SetFx(Fx::Limiter, 0.0),
        // **The varispeed rides**, which is what a tape operator does with
        // one: it cannot change a frame that is already written, and pulling
        // the speed during a pass is the oldest use the control has.
        Hit::SetVarispeed(0.5),
        // And the loudness ladder, which is a METER. Cycling its window is
        // reading, not changing — and a take is exactly when somebody wants
        // to know whether the programme is landing on target.
        Hit::CycleLufs,
    ];

    /// The whole reason `band_height` takes one argument. A camera that could
    /// change the band's height would resize the piano when somebody plugged in
    /// a different webcam — `docs/RECORDER-PLAN.md` §0's named failure, and the
    /// one thing about this panel that cannot be fixed later without breaking
    /// every geometry test in `app.rs`.
    #[test]
    fn a_four_three_camera_does_not_make_the_band_taller() {
        for w in [400.0_f32, 650.0, 1300.0, 2600.0] {
            let want = band_height(w);
            let r = band(w);
            assert_eq!(r.height(), want);
            for src in [
                Vec2::new(640.0, 480.0),   // 4:3
                Vec2::new(1920.0, 1080.0), // 16:9
                Vec2::new(1080.0, 1920.0), // a phone held upright
                Vec2::new(1000.0, 1000.0),
            ] {
                let l = Layout::new(r, &idle());
                let dst = fit_preview(l.preview, src);
                assert!(
                    l.preview.contains_rect(dst.shrink(0.01)),
                    "a {src:?} source escaped its box at {w}pt"
                );
                assert!(
                    r.contains_rect(l.preview),
                    "the preview box escaped the band at {w}pt"
                );
                // And the height the layout was given is still the height the
                // layout function alone decided.
                assert_eq!(band_height(w), want);
            }
        }
    }

    /// **The damped half of the height law, in numbers.**
    ///
    /// The band is linear in width up to [`BAND_DAMP_FROM`] and shorter than
    /// linear past it, and the owner has now asked for that reduction twice —
    /// so what it comes to at a maximised window is a decision worth writing
    /// down. A change to [`BAND_DAMP_EXP`] that moves the sizes people
    /// actually work at should fail here rather than be noticed on screen.
    #[test]
    fn the_band_is_shorter_than_linear_once_the_window_is_wide() {
        let linear = |w: f64| (190.0 * w / 1300.0).trunc();

        // Nothing at or below the threshold moves. This is the half that must
        // never change: "at 100 percent the app feels perfect visually".
        for w in [800.0_f32, 1300.0, 1599.0, 1600.0] {
            assert_eq!(
                band_height(w),
                linear(f64::from(w)) as f32,
                "the band stopped being linear at {w}pt"
            );
        }

        // And past it, the measured law — see the table in `band_height`.
        for (w, want) in [(1920.0_f32, 245.0), (2560.0, 266.0), (3440.0, 316.0)] {
            assert_eq!(band_height(w), want, "the damped height moved at {w}pt");
        }

        // The shape of it, stated as properties rather than as points: it
        // still GROWS with the window (a band that shrank as the screen got
        // wider would be a bug, not a preference), it is always shorter than
        // linear once damped, and it never gives back more than the floor.
        let mut last = 0.0_f32;
        let mut w = 400.0_f32;
        while w <= 5120.0 {
            let h = band_height(w);
            assert!(h >= last, "the band shrank going from {}pt to {w}pt", w - 20.0);
            let lin = linear(f64::from(w)) as f32;
            assert!(h <= lin, "the band grew past linear at {w}pt");
            assert!(
                h >= (lin * BAND_DAMP_FLOOR as f32).trunc() - 1.0,
                "the band fell through the floor at {w}pt"
            );
            last = h;
            w += 20.0;
        }
    }

    /// State and the presence of a frame are equally powerless over the height.
    #[test]
    fn the_band_height_is_a_function_of_width_and_nothing_else() {
        assert_eq!(band_height(1300.0), 190.0);
        assert_eq!(band_height(650.0), 95.0);
        // Truncated, not rounded, like every other band: 190 * 1000/1300 is
        // 146.1, and a half-pixel band puts every row on a fractional line.
        assert_eq!(band_height(1000.0), 146.0);
        assert_eq!(band_height(0.0), 0.0);

        let pv = Preview {
            texture: egui::TextureId::User(1),
            size: Vec2::new(640.0, 480.0),
        };
        for w in [400.0_f32, 1300.0] {
            let want = band_height(w);
            for state in STATES {
                for preview in [None, Some(pv)] {
                    let v = RecorderView {
                            position_s: 0.0,
                            playing: false,
                        input_monitor: false,
                        preview,
                        ..with_rack(state)
                    };
                    // Nothing in the view can reach the height: the layout is
                    // handed a rect the height already decided.
                    let l = Layout::new(band(w), &v);
                    assert_eq!(band_height(w), want, "{state:?} moved the band height");
                    for (r, k) in l.targets() {
                        assert!(
                            !r.is_positive() || band(w).contains_rect(r),
                            "{} escaped the band at {w}pt in {state:?}",
                            k.control().label()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn fit_preview_letterboxes_whatever_shape_the_camera_is() {
        let box_ = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(320.0, 180.0));
        // Wider than the box: full width, bars top and bottom.
        let wide = fit_preview(box_, Vec2::new(1000.0, 250.0));
        assert!((wide.width() - 320.0).abs() < 0.01);
        assert!(wide.height() < box_.height());
        // Taller than the box: full height, bars left and right.
        let tall = fit_preview(box_, Vec2::new(480.0, 640.0));
        assert!((tall.height() - 180.0).abs() < 0.01);
        assert!(tall.width() < box_.width());
        // Exactly the box's aspect: it fills, and it is not a rounding error
        // away from filling.
        let exact = fit_preview(box_, Vec2::new(1920.0, 1080.0));
        assert!((exact.width() - 320.0).abs() < 0.01, "{exact:?}");
        assert!((exact.height() - 180.0).abs() < 0.01, "{exact:?}");

        for (src, got) in [
            (Vec2::new(1000.0, 250.0), wide),
            (Vec2::new(480.0, 640.0), tall),
            (Vec2::new(1920.0, 1080.0), exact),
        ] {
            assert!(
                box_.contains_rect(got.shrink(0.01)),
                "{src:?} left the box: {got:?}"
            );
            assert!(
                (got.center() - box_.center()).length() < 0.01,
                "{src:?} was not centred"
            );
            assert!(
                ((got.width() / got.height()) - (src.x / src.y)).abs() < 0.001,
                "{src:?} was stretched to {got:?}"
            );
        }
    }

    /// A camera that has not reported a size yet, and a band that has been
    /// dragged to nothing. Both are reachable and neither may divide by zero.
    #[test]
    fn fit_preview_survives_a_zero_sized_source_or_box() {
        let box_ = Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 180.0));
        for src in [
            Vec2::ZERO,
            Vec2::new(0.0, 480.0),
            Vec2::new(640.0, 0.0),
            Vec2::new(-4.0, 3.0),
        ] {
            let r = fit_preview(box_, src);
            assert!(!r.is_positive(), "{src:?} produced something to draw");
        }
        for b in [
            Rect::from_min_size(Pos2::ZERO, Vec2::ZERO),
            Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 0.0)),
            Rect::NOTHING,
        ] {
            let r = fit_preview(b, Vec2::new(1920.0, 1080.0));
            assert!(!r.is_positive());
            assert!(!r.min.x.is_nan() && !r.min.y.is_nan(), "NaN escaped");
        }
    }

    /// Two controls that overlap means one of them silently swallows the
    /// other's clicks, and nothing about the picture says so.
    ///
    /// Swept over every rack as well as every record state, because a slot row's
    /// insides change shape with what is in it: an empty slot's name box takes
    /// the whole row, and a row whose neighbour grew into it is precisely the
    /// bug this catches.
    /// **The two transport buttons are the same size.**
    ///
    /// Stop used to be 0.66 of record and then had its glyph shrunk another
    /// 30% inside that, so the square read as less than half the circle — which
    /// makes one look like the real control and the other like a note about it.
    /// **The band stays readable whatever colour it is given.**
    ///
    /// The ink follows the BACKGROUND, not `dark_mode`. A rule that read the
    /// theme would put near-black text on a dark walnut the moment somebody
    /// picked one in light mode, and the band would be unreadable with nothing
    /// on screen explaining why.
    #[test]
    fn the_ink_follows_the_background_and_not_the_theme() {
        use crate::settings::Rgb;
        // A dark colour chosen while the app is in LIGHT mode.
        let mut s = Settings {
            dark_mode: false,
            ..Settings::default()
        };
        s.recorder_bg_color = Rgb {
            r: 0x2A,
            g: 0x20,
            b: 0x18,
        };
        let p = palette(&s);
        assert!(
            contrast_ratio(p.bg, p.ink) > 4.0,
            "dark band, light theme: ink {:?} on {:?} is unreadable",
            p.ink,
            p.bg
        );

        // And a light colour chosen while the app is in DARK mode.
        let mut s = Settings {
            dark_mode: true,
            ..Settings::default()
        };
        s.recorder_bg_color = Rgb {
            r: 0xE8,
            g: 0xDC,
            b: 0xC0,
        };
        let p = palette(&s);
        assert!(
            contrast_ratio(p.bg, p.ink) > 4.0,
            "light band, dark theme: ink {:?} on {:?} is unreadable",
            p.ink,
            p.bg
        );
    }

    /// Every colour a user could pick leaves legible ink, and a well that is
    /// still the same hue as the band it is cut into.
    #[test]
    fn any_background_gets_readable_ink_and_a_well_of_its_own_hue() {
        use crate::settings::Rgb;
        for (r, g, b) in [
            (0x00, 0x00, 0x00),
            (0xFF, 0xFF, 0xFF),
            (0x4A, 0x3B, 0x2C), // the default walnut
            (0x8B, 0x00, 0x00), // saturated red
            (0x00, 0x00, 0x8B), // saturated blue: dark despite a high channel
            (0x00, 0xFF, 0x00), // and green, which is bright despite one channel
            (0x80, 0x80, 0x80),
        ] {
            let mut s = Settings::default();
            s.recorder_bg_color = Rgb { r, g, b };
            let p = palette(&s);
            assert!(
                contrast_ratio(p.bg, p.ink) > 3.5,
                "#{r:02X}{g:02X}{b:02X}: ink {:?} on {:?}",
                p.ink,
                p.bg
            );
            // The well is a shade of the band, not a grey hole in it: the
            // channel ORDER survives.
            let order = |c: Color32| {
                let mut v = [(c.r(), 0u8), (c.g(), 1), (c.b(), 2)];
                v.sort();
                [v[0].1, v[1].1, v[2].1]
            };
            assert_eq!(
                order(p.well),
                order(p.bg),
                "#{r:02X}{g:02X}{b:02X}: the well lost the band's hue"
            );
        }
    }

    /// **The transport row is the same row under a take: every button, in
    /// the same place, answering the same press.**
    ///
    /// The owner: "when active, the take button simply glows amber -- it is
    /// now the only way to stop the take with a button. why? we need to be
    /// able to record arrangements and or play them back on command ... the
    /// main buttons need to be intact while recording takes." The take row
    /// used to keep only a steady red dot, the stop and the cassette. Every
    /// state, the widths the other layout tests sweep and the two detached
    /// windows; only the cassette's colour says a take is running.
    #[test]
    fn the_transport_row_is_the_same_under_a_take() {
        let mut rects: Vec<Rect> = [320.0_f32, 500.0, 560.0, 640.0, 900.0, 1300.0, 2600.0]
            .iter()
            .map(|w| band(*w))
            .collect();
        rects.push(Rect::from_min_size(Pos2::ZERO, DETACHED_DEFAULT));
        rects.push(Rect::from_min_size(Pos2::ZERO, DETACHED_MIN));
        for r in rects {
            let at_rest = Layout::new(r, &idle());
            for state in STATES {
                let v = RecorderView {
                    state,
                    ..idle()
                };
                let l = Layout::new(r, &v);
                for (name, a, b) in [
                    ("record", at_rest.record, l.record),
                    ("play", at_rest.play, l.play),
                    ("stop", at_rest.stop, l.stop),
                    ("pause", at_rest.pause, l.pause),
                    ("the cassette", at_rest.capture, l.capture),
                    ("the tempo knob", at_rest.tempo, l.tempo),
                    ("TAP", at_rest.tempo_tap, l.tempo_tap),
                ] {
                    assert_eq!(a, b, "{name} moved or went away in {state:?} at {r:?}");
                }
                for (hit, rect) in [
                    (Hit::Record, l.record),
                    (Hit::Play, l.play),
                    (Hit::Stop, l.stop),
                    (Hit::Pause, l.pause),
                    (Hit::Capture, l.capture),
                    (Hit::TapTempo, l.tempo_tap),
                ] {
                    if rect.is_positive() {
                        assert_eq!(
                            hit_test(r, &v, rect.center()),
                            Some(hit),
                            "in {state:?} at {r:?} a press on its centre is not {hit:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn record_and_stop_are_the_same_size() {
        for w in [320.0_f32, 640.0, 1300.0, 2600.0] {
            let r = band(w);
            let l = Layout::new(r, &idle());
            if !l.record.is_positive() || !l.stop.is_positive() {
                continue;
            }
            assert!(
                (l.record.width() - l.stop.width()).abs() < 0.51,
                "at {w}: record is {} wide and stop is {}",
                l.record.width(),
                l.stop.width()
            );
            assert!(
                (l.record.height() - l.stop.height()).abs() < 0.51,
                "at {w}: record is {} tall and stop is {}",
                l.record.height(),
                l.stop.height()
            );
            // And they still do not touch, which is the constraint that made
            // them different sizes in the first place.
            assert!(
                !l.record.intersects(l.stop),
                "at {w}: the transport buttons overlap"
            );
        }
    }

    #[test]
    fn no_two_hit_regions_overlap() {
        for w in [500.0_f32, 900.0, 1300.0, 2600.0] {
            for r in [
                band(w),
                Rect::from_min_size(Pos2::ZERO, DETACHED_DEFAULT),
                Rect::from_min_size(Pos2::ZERO, DETACHED_MIN),
            ] {
                for state in STATES {
                    for hide in [false, true] {
                        let v = RecorderView {
                            position_s: 0.0,
                            playing: false,
                            input_monitor: false,
                            hide_elapsed: hide,
                            ..with_rack(state)
                        };
                        let t = Layout::new(r, &v).targets();
                        for i in 0..t.len() {
                            for j in (i + 1)..t.len() {
                                assert!(
                                    !t[i].0.intersects(t[j].0),
                                    "{} and {} overlap at {w}pt in {state:?}: {:?} {:?}",
                                    t[i].1.control().label(),
                                    t[j].1.control().label(),
                                    t[i].0,
                                    t[j].0
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Every control has to be reachable at the point it is drawn, or it is
    /// decoration. Driven off `Hit::ALL` so a new variant that nobody wired up
    /// fails here rather than shipping as a button that does nothing.
    ///
    /// With a full rack, because six of the twenty-six controls belong to a
    /// loaded instrument and an empty slot deliberately offers none of them.
    /// What an EMPTY slot offers is its own test.
    #[test]
    fn every_control_is_reachable_in_the_idle_layout() {
        for r in [
            band(1300.0),
            band(650.0),
            Rect::from_min_size(Pos2::new(40.0, 90.0), DETACHED_DEFAULT),
            Rect::from_min_size(Pos2::ZERO, DETACHED_MIN),
        ] {
            let v = furnished();
            let l = Layout::new(r, &v);
            for want in Hit::ALL {
                // The one control with no rectangle of its own: whether the
                // click lands in the FILE is set by right-clicking the
                // metronome, because it is set once and was taking a box and a
                // caption in the busiest row of the band. Its reachability is
                // the app's to prove, not the layout's.
                // The take's settings moved to the menu, so they have no
                // rectangle in the band any more. Their reachability is the
                // menu's to prove, not the layout's — the same bargain the
                // in-take toggle already made.
                // Everything the band no longer carries itself. The take
                // NAME joined them when the words column went: it is set once
                // a session, and the band is what your hands are on while a
                // take runs. `SetTempo` is here because the tempo BOX is a
                // control and the value is not — see `Hit::EditTempo`.
                // `PickAudio` joined when the microphone icon left with the
                // input fader: the desk's audio channels pick their inputs
                // now, and the menu keeps the row for the interface itself.
                const IN_THE_MENU: [Hit; 17] = [
                    Hit::ChooseFolder,
                    Hit::RevealFolder,
                    // The PROJECT tier's six, which live in the cog and
                    // nowhere in the band — the same as the take folder's
                    // two above them.
                    Hit::ChooseProjectFolder,
                    Hit::RevealProjectFolder,
                    Hit::ChooseExportFolder,
                    Hit::RevealExportFolder,
                    Hit::ChoosePresetFolder,
                    Hit::RevealPresetFolder,
                    Hit::ChooseIrFolder,
                    Hit::RevealIrFolder,
                    Hit::ToggleOpenWhenDone,
                    Hit::CycleCountIn,
                    Hit::Export,
                    Hit::EditTimeSignature,
                    Hit::NameField,
                    Hit::PickAudio,
                    // The STREAM door lives in the cog's AUDIO tier — the
                    // owner's placement — and nowhere in the band.
                    Hit::StreamToObs,
                ];
                // `DismissClip` is a target only while the warning is on
                // screen, which the idle fixture is not — see
                // `the_clip_warning_can_be_dismissed_only_while_it_is_showing`,
                // which is where that half is proved.
                // A `Type` target is the WORD over a knob or the number
                // beside a fader, and below a certain band height neither is
                // drawn — see `knob_face`. Covered at a readable size by
                // `every_number_can_be_typed_into_where_it_is_written`.
                // A `Type` target is the WORD over a knob or the number
                // beside a fader, and below a certain band height neither is
                // drawn — see `knob_face`. It has a rectangle at readable
                // sizes, so it belongs in neither branch here; it is covered
                // by `every_number_can_be_typed_into_where_it_is_written`.
                if matches!(want, Hit::Type(_)) {
                    continue;
                }
                // The × is a hit-test PRE-PASS, not a target: it sits on the
                // seek surface, whose fraction is measured against the whole
                // timeline, so it cannot be carved out of `targets` without
                // bending the mapping. Reachability is proved directly.
                if want.is_same_control(Hit::ClearTrack) {
                    assert!(
                        l.timeline_close.is_positive(),
                        "the loaded timeline has no × to press"
                    );
                    assert_eq!(
                        hit_test(r, &v, l.timeline_close.center()),
                        Some(Hit::ClearTrack),
                        "the timeline's × is not clickable at its own centre"
                    );
                    continue;
                }
                // **The loudness reset is a SECONDARY click** on a target
                // that already exists, exactly as the timeline's × is a
                // primary one that `hit_test` pre-empts. It has no rectangle
                // of its own by design; what it has is the ladder's, and the
                // app routes the right button. Proved directly rather than
                // through `targets`.
                if want.is_same_control(Hit::ResetLufs) {
                    assert!(
                        l.lufs_bar.is_positive(),
                        "the loudness ladder has nothing to right-click"
                    );
                    assert_eq!(
                        hit_test(r, &v, l.lufs_bar.center()),
                        Some(Hit::CycleLufs),
                        "the ladder is not clickable at its own centre"
                    );
                    continue;
                }
                if IN_THE_MENU.iter().any(|m| m.is_same_control(want))
                    || want == Hit::ToggleMetronomeInTake
                {
                    assert!(
                        l.targets().into_iter().all(|(rect, k)| {
                            !k.control().is_same_control(want) || !rect.is_positive()
                        }),
                        "{} grew a rectangle in the band again",
                        want.label()
                    );
                    continue;
                }
                let (rect, _) = l
                    .targets()
                    .into_iter()
                    .find(|(_, k)| k.control().is_same_control(want))
                    .expect("every variant is in `targets`");
                assert!(
                    rect.is_positive(),
                    "{} has no rect while idle at {:?}",
                    want.label(),
                    r.size()
                );
                let got = hit_test(r, &v, rect.center());
                assert!(
                    got.is_some_and(|h| h.is_same_control(want)),
                    "{} is not clickable at its own centre: got {got:?}",
                    want.label()
                );
            }
            // And a miss is a miss: outside the band nothing is hit.
            assert_eq!(hit_test(r, &v, r.min - Vec2::splat(4.0)), None);
            assert_eq!(hit_test(r, &v, r.max + Vec2::splat(4.0)), None);
        }
    }


    /// **Which controls survive a live take**, asserted by sweeping the whole
    /// band rather than by asking the layout, because the question is what a
    /// user can hit and not what the struct says.
    ///
    /// Stop, the two faders, the three slot knobs, the three editor buttons and
    /// the click switch. Nothing else: the folder, the name and the devices are
    /// all decided at `T0`, and offering them at 0:47 would be promising
    /// something the recorder cannot do. Record is gone too — the button is a
    /// red dot while rolling, and a dead control is worse than no control.
    ///
    /// The instrument PICKERS are the interesting absence. They sit in the group
    /// that survives, inches from the knobs that do, and they still have to go:
    /// loading a VST3 blocks the main thread for seconds, so a picker offered
    /// mid-take is an offer to lose the take. Clearing a slot goes with them for
    /// the same reason.
    ///
    /// The levels and the editor buttons are the exception on purpose. Turning
    /// the click down or a layer up halfway through a take is exactly when you
    /// need to, reaching a preset in the plugin's own window between two passes
    /// is a real thing, and none of them can change what has already been
    /// written. The one that could — "in take", which decides whether the click
    /// reaches the file — leaves with the destination.
    #[test]
    fn the_levels_and_editor_buttons_survive_a_take_and_the_pickers_do_not() {
        for state in [
            RecordState::CountIn { beat: 1, of: 8 },
            RecordState::Rolling,
            RecordState::Finishing,
        ] {
            for hide in [false, true] {
                let v = RecorderView {
                position_s: 0.0,
                playing: false,
                    input_monitor: false,
                    hide_elapsed: hide,
                    ..with_rack(state)
                };
                let r = band(1300.0);
                let mut seen: Vec<Hit> = Vec::new();
                let mut y = r.top();
                while y <= r.bottom() {
                    let mut x = r.left();
                    while x <= r.right() {
                        if let Some(h) = hit_test(r, &v, Pos2::new(x, y)) {
                            assert!(
                                SURVIVORS.iter().any(|s| s.is_same_control(h)),
                                "{} is reachable at ({x}, {y}) during {state:?}",
                                h.label()
                            );
                            if !seen.iter().any(|s| s.is_same_control(h)) {
                                seen.push(h);
                            }
                        }
                        x += 2.0;
                    }
                    y += 2.0;
                }
                for want in SURVIVORS {
                    assert!(
                        seen.iter().any(|s| s.is_same_control(want)),
                        "{} is unreachable during {state:?}",
                        want.label()
                    );
                }
                assert_eq!(seen.len(), SURVIVORS.len());
            }
        }
    }

    /// A control that survives a take and then moves is only half a survivor.
    ///
    /// Everything else in the band jumps when a take starts — that is what the
    /// two layouts are for — so the one group you might reach for at 0:47 has
    /// to be exactly where it was at 0:00, to the point, and not merely still
    /// present somewhere.
    #[test]
    fn the_monitor_is_the_same_rectangle_before_and_during_a_take() {
        for r in [
            band(500.0),
            band(1300.0),
            band(2600.0),
            Rect::from_min_size(Pos2::new(40.0, 90.0), DETACHED_DEFAULT),
            Rect::from_min_size(Pos2::ZERO, DETACHED_MIN),
        ] {
            let at_rest = Layout::new(r, &furnished());
            let live = Layout::new(r, &furnished_rolling());
            // The timeline holds still: the playhead somebody is watching at
            // 0:47 is drawn in the same box it was drawn in at 0:00, and a
            // take no longer stops a click on it locating. (Dormant for now:
            // the LCD owns its box, and both layouts leave it `NOTHING`.)
            assert_eq!(at_rest.timeline, live.timeline, "the timeline moved");
            // So do the master and the effect knobs, which are the levels a
            // hand reaches for mid-take.
            assert_eq!(at_rest.master_knob, live.master_knob, "the master moved");
            for fx in Fx::ALL {
                assert_eq!(at_rest.cell(fx), live.cell(fx), "{fx:?} moved");
            }
            // And the divider that marks the group off does not move either.
            assert_eq!(at_rest.rules[1], live.rules[1]);
        }
    }

    /// **The heart's corner is nobody else's.** It holds the band's
    /// bottom-right corner and must not sit on the master ladder's numbers or
    /// bars.
    #[test]
    fn the_heart_keeps_out_of_the_masters_scale() {
        for w in [900.0_f32, 1300.0, 2600.0] {
            let v = furnished();
            let l = Layout::new(band(w), &v);
            if !l.heart.is_positive() {
                continue;
            }
            for (what, r) in [
                ("the master's right scale", l.master_scale[1]),
                ("the master's bars", l.master_bars),
                ("the master knob", l.master_knob),
            ] {
                assert!(
                    !l.heart.intersects(r),
                    "at {w}: the heart sits on {what}: {:?} vs {r:?}",
                    l.heart
                );
            }
        }
    }

    #[test]
    fn the_lcd_owns_the_box_and_the_scope_is_passive_glass() {
        let r = band(1300.0);
        let v = idle();
        let l = Layout::new(r, &v);
        // The whole box is still the LCD; the old seek surface answers
        // nothing, and the RIGHT half — the oscilloscope now — is glass:
        // no band hit anywhere on it.
        assert!(l.lcd.is_positive(), "the LCD has no box");
        assert!(!l.timeline.is_positive(), "the overview is still laid out");
        for frac in [0.30f32, 0.55, 0.75, 0.95] {
            let p = Pos2::new(
                l.lcd.left() + l.lcd.width() * frac,
                l.lcd.center().y,
            );
            assert_eq!(
                hit_test(r, &v, p),
                None,
                "the LCD answered a press at {frac} of its width"
            );
        }
        // And the REVIVED cluster: every Start/End/Length cell stands in
        // the LEFT half and answers the part pre-pass at its own centre.
        for (row, cells) in lcd_part_cells(l.lcd).iter().enumerate() {
            for (part, cell) in cells.iter().enumerate() {
                assert!(cell.is_positive(), "cell {row}/{part} has no room");
                assert!(
                    cell.center().x < l.lcd.left() + l.lcd.width() * 0.5,
                    "cell {row}/{part} strayed into the scope half"
                );
                assert_eq!(
                    lcd_part_at(r, &v, cell.center()),
                    Some((row as u8, part as u8)),
                    "cell {row}/{part} does not answer at its own centre"
                );
            }
        }
        // **And the PLAYHEAD's own three**, which is where the drag zones had
        // drifted off the digits: they tile their box in order with no gap and
        // no overlap, and each answers as itself.
        let big = lcd_playhead_cells(l.lcd);
        for (part, cell) in big.iter().enumerate() {
            assert!(cell.is_positive(), "playhead cell {part} has no room");
            assert_eq!(
                lcd_part_at(r, &v, cell.center()),
                Some((3, part as u8)),
                "playhead cell {part} does not answer at its own centre"
            );
        }
        for (i, w) in big.windows(2).enumerate() {
            assert!(
                (w[0].right() - w[1].left()).abs() < 0.01,
                "a gap or an overlap between playhead fields {i} and {}",
                i + 1
            );
        }
    }

    /// **The playhead's digits are drawn into the cells they are dragged by.**
    ///
    /// They used to be one centred string measured against three cells cut at
    /// fixed fractions. A centred string moves with its own contents — "9| 1|
    /// 000" and "128| 4| 000" put the beats digit in different places — while
    /// the cut did not, so the zone that rode the beats sat somewhere off to
    /// the right of the beats digit and moved with the bar number. Two
    /// geometries that ought to agree cannot be tuned into agreeing; they have
    /// to be one geometry.
    ///
    /// **A source scan, because what is being asserted is that the drawing and
    /// the hit test share a function** — and a drawing that had drifted apart
    /// again would still paint something perfectly plausible. The scanning
    /// tests in `settings.rs` set the precedent.
    #[test]
    fn the_playhead_digits_are_drawn_into_their_drag_cells() {
        let src = include_str!("recorder_panel.rs");
        let start = src
            .find("fn draw_timecode_cluster")
            .expect("the cluster is still drawn by that name");
        let body = &src[start..start + 6_000];
        assert!(
            body.contains("lcd_playhead_cells(r)"),
            "the timecode cluster no longer draws the playhead through the \
             cells its drags are hit-tested against, so the two can drift"
        );
        assert!(
            !body.contains(r#"format!("{b}| {bt}| {th:03}")"#),
            "the playhead is back to one centred string, which is what put \
             the beats zone to the right of the beats digit"
        );
    }

    /// **The musical clock counts the way a musician does.** Bar and beat
    /// from one, thousandths of a beat, honouring the time signature's
    /// bottom number — and never a beat past the bar.
    #[test]
    fn bars_beats_counts_from_one_and_honours_the_signature() {
        use crate::recorder::TimeSignature;
        let common = TimeSignature { beats: 4, unit: 4 };
        // 120 bpm: a beat is half a second, a bar two seconds.
        assert_eq!(bars_beats(0.0, 120.0, common), "1| 1| 000");
        assert_eq!(bars_beats(0.5, 120.0, common), "1| 2| 000");
        assert_eq!(bars_beats(2.0, 120.0, common), "2| 1| 000");
        assert_eq!(bars_beats(2.25, 120.0, common), "2| 1| 500");
        // 6/8 at 120: the beat is an EIGHTH, a quarter second.
        let six8 = TimeSignature { beats: 6, unit: 8 };
        assert_eq!(bars_beats(0.25, 120.0, six8), "1| 2| 000");
        assert_eq!(bars_beats(1.5, 120.0, six8), "2| 1| 000");
        // Never negative, never a beat out of range.
        assert_eq!(bars_beats(-3.0, 120.0, common), "1| 1| 000");
    }

    /// The clock the setting hides is a CLOCK. The count-in beat is the number
    /// the player is counting on, and a performance setting that swallowed it
    /// would be hiding the wrong thing under the right name.
    #[test]
    fn the_count_in_readout_shows_the_beat_and_hide_elapsed_leaves_it_alone() {
        // Just the number the player is saying out loud. It read "3 OF 4"
        // while the count was a running total against a total; now that it is
        // a beat WITHIN THE BAR — 1 2 3 4 5 6, 1 2 3 4 5 6 in 6/8 — the second
        // half carries nothing the SIG cell does not already say.
        assert_eq!(readout_text(RecordState::CountIn { beat: 3, of: 4 }, 0.0, 0.0), "3");
        assert_eq!(
            readout_text(RecordState::CountIn { beat: 5, of: 6 }, 91.0, 91.0),
            "5",
            "the elapsed clock does not leak into the count"
        );
        assert_eq!(readout_text(RecordState::Finishing, 12.0, 12.0), "FINISHING");
        // **The take's own time, not the playhead.** The song can sit still,
        // or be located back, under a running take now; the clock in the
        // take's slot says how long the camera has been rolling.
        assert_eq!(readout_text(RecordState::Rolling, 30.0, 252.0), "4:12");

        let r = band(1300.0);
        for hide in [false, true] {
            let v = RecorderView {
                position_s: 0.0,
                playing: false,
                input_monitor: false,
                state: RecordState::CountIn { beat: 1, of: 4 },
                hide_elapsed: hide,
                ..RecorderView::empty()
            };
            assert!(
                Layout::new(r, &v).timecode.is_positive(),
                "the beat vanished with hide_elapsed = {hide}"
            );
        }
    }

    /// The setting no competitor offers: after a blinking light, a running
    /// timer is the most-cited performance distraction. It is reached from the
    /// menu now — there is no `CLOCK` box in the band, because the owner could
    /// not tell what one was for — but the readout still obeys it.
    #[test]
    fn the_clock_is_not_drawn_while_rolling_when_it_is_hidden() {
        let r = band(1300.0);
        let hidden = |state| {
            Layout::new(
                r,
                &RecorderView {
                    state,
                    hide_elapsed: true,
                    ..RecorderView::empty()
                },
            )
            .timecode
        };
        assert!(
            !hidden(RecordState::Rolling).is_positive(),
            "the clock is still on screen during a take"
        );
        // Hiding a clock hides a clock - never the tempo beside it.
        let l = Layout::new(
            r,
            &RecorderView {
                state: RecordState::Rolling,
                hide_elapsed: true,
                ..RecorderView::empty()
            },
        );
        assert!(l.tempo.is_positive() && l.tempo_tap.is_positive(), "hiding the clock took the tempo");
        assert!(
            hidden(RecordState::CountIn { beat: 2, of: 4 }).is_positive(),
            "the count-in is the number the user is waiting for"
        );
        assert!(
            hidden(RecordState::Finishing).is_positive(),
            "'files are still being closed' is not a clock"
        );
        // **Nothing at rest, whatever the setting says.** A clock that can
        // only ever read 0:00 is furniture: every state with a number in it —
        // the count-in, rolling, finishing — is `is_active`, and those take
        // the whole column above the transport row.
        assert!(
            !hidden(RecordState::Idle).is_positive(),
            "a clock at rest, with only one thing it could ever say"
        );
        // Without the setting the clock is drawn in every state, in exactly
        // the same box. **Rolling no longer rearranges the band.** The clock
        // used to swell to four times its resting size, which meant the meters
        // narrowed and the buttons moved at the one moment nobody can afford
        // to look away and find things elsewhere. The dot replacing the record
        // button is the only difference between the two layouts now.
        let shown = |state| {
            Layout::new(
                r,
                &RecorderView {
                    state,
                    ..RecorderView::empty()
                },
            )
        };
        let at_rest = shown(RecordState::Idle).timecode;
        let live = shown(RecordState::Rolling).timecode;
        assert!(!at_rest.is_positive() && live.is_positive());

        // The meter, the stop button and the faders are still there either way.
        for state in [RecordState::Idle, RecordState::Rolling] {
            let l = Layout::new(
                r,
                &RecorderView {
                    state,
                    hide_elapsed: true,
                    ..RecorderView::empty()
                },
            );
            assert!(l.meter.is_positive(), "the meter went away in {state:?}");
            assert!(l.stop.is_positive(), "the transport went away in {state:?}");
            assert!(l.lcd.is_positive(), "the LCD went away in {state:?}");
        }
    }

    /// Every rect the band seats the cassette in, rolling (the take's own
    /// row), at every width the window allows, plus the two detached sizes.
    fn capture_rects() -> Vec<Rect> {
        let mut out = Vec::new();
        let mut w = crate::settings::MIN_WINDOW.x;
        while w <= 2600.0 {
            out.push(Layout::new(band(w), &rolling()).capture);
            w += 1.0;
        }
        for d in [DETACHED_DEFAULT, DETACHED_MIN] {
            out.push(Layout::new(Rect::from_min_size(Pos2::ZERO, d), &rolling()).capture);
        }
        out.retain(|r| r.is_positive());
        out
    }

    /// **A hub never eats the body it sits in**, so the amber is there to
    /// light at the smallest size the row draws. At 10.5 points the old hubs
    /// and rim left six or eight pixels of amber, and the owner's screen read
    /// the take button at 108,86,71 at rest against 115,92,72 lit.
    #[test]
    fn a_hub_never_eats_the_body_it_sits_in() {
        let mut rects = capture_rects();
        let mut d = TRANSPORT_MIN_SIDE;
        while d <= 40.0 {
            rects.push(Rect::from_min_size(Pos2::new(100.3, 50.7), Vec2::splat(d)));
            d += 0.25;
        }
        for r in rects {
            let g = cassette(r);
            assert!(
                g.hub <= g.body.height() * 0.5 - g.rim - HUB_CLEAR + 1e-4,
                "at {r:?} a hub leaves less than a point of lit body above and below it"
            );
            assert!(
                (g.hubs[1].x - g.hubs[0].x) - 2.0 * g.hub >= 2.0,
                "at {r:?} the hubs leave no lit body between them"
            );
            assert!(
                g.body.height() - 2.0 * g.rim >= 0.6 * g.body.height(),
                "at {r:?} the rim is most of the body"
            );
            assert!(
                r.expand(1e-3).contains_rect(g.plate.expand(g.feather * 0.5)),
                "at {r:?} the glow leaves its own button"
            );
            if r.width().min(r.height()) >= 20.0 {
                assert!(g.ring >= 0.9, "at {r:?} there is room for the hub rings and none is drawn");
            }
            // Every hub that is drawn has an edge: at rest its hole is the
            // band's colour, 1.5:1 from the body on the light bands.
            if g.hub > 0.5 {
                assert!(g.ring > 0.0, "at {r:?} a hub is drawn with no ring");
                assert!(g.ring <= g.hub, "at {r:?} the ring is wider than its hub");
            }
        }
    }

    /// **The lit cassette paints nothing outside its own button** - the row's
    /// three-point gap to its neighbours stays empty - and it IS lit: one
    /// plate in the glow colour, soft-edged only where there is room.
    #[test]
    fn the_lit_cassette_paints_nothing_outside_its_own_button() {
        let ctx = egui::Context::default();
        let p = palette(&Settings::default());
        let glow = toward(cassette_lit(p.bg), p.bg, CASSETTE_GLOW_T);
        let rects = capture_rects();
        let smallest = *rects
            .iter()
            .min_by(|a, b| a.width().min(a.height()).total_cmp(&b.width().min(b.height())))
            .unwrap();
        let largest = *rects
            .iter()
            .max_by(|a, b| a.width().min(a.height()).total_cmp(&b.width().min(b.height())))
            .unwrap();
        for r in [smallest, largest] {
            for active in [false, true] {
                let out = ctx.run(egui::RawInput::default(), |ctx| {
                    let painter = Painter::new(ctx.clone(), egui::LayerId::background(), Rect::EVERYTHING);
                    draw_capture(&painter, r, active, &p);
                });
                let mut plates = 0;
                for c in &out.shapes {
                    assert!(
                        r.expand(1e-3).contains_rect(c.shape.visual_bounding_rect()),
                        "at {r:?} (active {active}) a shape leaves the button: {:?}",
                        c.shape.visual_bounding_rect()
                    );
                    if let egui::Shape::Rect(rs) = &c.shape {
                        if rs.fill == glow {
                            plates += 1;
                            let soft = rs.blur_width > 0.0;
                            assert_eq!(soft, r == largest, "at {r:?} the plate's edge is soft={soft}");
                        }
                    }
                }
                assert_eq!(plates, usize::from(active), "at {r:?} active {active}: {plates} lit plates");
            }
        }
    }

    /// The stock band, and every dark factory band, keeps the owner's amber
    /// exactly; only the light bands deepen it so "lit" still reads.
    #[test]
    fn the_stock_band_keeps_the_owners_amber() {
        assert_eq!(cassette_lit(palette(&Settings::default()).bg), TAPE_AMBER);
        for t in crate::theme::factory() {
            let mut set = Settings::default();
            t.apply(&mut set);
            let bg = palette(&set).bg;
            let lit = cassette_lit(bg);
            if wants_light_ink(bg) {
                assert_eq!(lit, TAPE_AMBER, "{} is a dark band and its lamp moved", t.name);
            }
            assert!(
                contrast_ratio(lit, bg) >= CASSETTE_LIT_FLOOR - 0.05,
                "{}: the lit cassette stands {:.2}:1 off its band",
                t.name,
                contrast_ratio(lit, bg)
            );
        }
    }

    /// **An export is the exception**: it owns the transport, the host
    /// refuses a record, play or pause into it, so the band does not seat
    /// them. Stop (which ends it early, on purpose) and the cassette stay put.
    #[test]
    fn an_export_seats_only_stop_and_the_cassette() {
        let r = band(1300.0);
        let take = Layout::new(r, &rolling());
        let export = Layout::new(
            r,
            &RecorderView {
                exporting: true,
                ..rolling()
            },
        );
        for (name, rect) in [
            ("record", export.record),
            ("play", export.play),
            ("pause", export.pause),
            ("the tempo knob", export.tempo),
            ("TAP", export.tempo_tap),
        ] {
            assert!(!rect.is_positive(), "{name} is seated over an export the host refuses it for");
        }
        assert!(take.tempo.is_positive(), "an ordinary take lost its tempo knob");
        assert_eq!(export.stop, take.stop, "Stop moved under an export");
        assert_eq!(export.capture, take.capture, "the cassette moved under an export");
    }

    /// **The take's readout lives on the LCD's scope glass and covers no
    /// control.** It used to take the tempo knob's slot in the transport row.
    #[test]
    fn the_take_readout_lives_on_the_scope_glass_and_displaces_nothing() {
        let mut rects: Vec<Rect> = [560.0_f32, 900.0, 1300.0, 2560.0].iter().map(|w| band(*w)).collect();
        rects.push(Rect::from_min_size(Pos2::ZERO, DETACHED_DEFAULT));
        rects.push(Rect::from_min_size(Pos2::ZERO, DETACHED_MIN));
        for r in rects {
            for state in [
                RecordState::CountIn { beat: 2, of: 4 },
                RecordState::Rolling,
                RecordState::Finishing,
            ] {
                let view = with_rack(state);
                let l = Layout::new(r, &view);
                let g = l.timecode;
                assert!(g.is_positive(), "at {r:?} in {state:?} there is no readout");
                assert_eq!(g, lcd_scope_rect(l.lcd), "at {r:?} the readout is not the glass");
                assert!(l.lcd.contains_rect(g));
                assert!(
                    g.left() > l.lcd.left() + l.lcd.width() * lcd_split(l.lcd),
                    "at {r:?} the readout reaches into the LCD's clock cells"
                );
                for (target, _) in l.targets() {
                    if target.is_positive() {
                        assert!(!target.intersects(g), "at {r:?} the readout covers a control at {target:?}");
                    }
                }
                assert_eq!(hit_test(r, &view, g.center()), None, "at {r:?} the glass answers a press");
            }
            assert!(!Layout::new(r, &idle()).timecode.is_positive(), "at {r:?} a readout at rest");
        }
        // A band too small for an LCD has no glass - NOTHING, never NaN; one
        // with a sliver of LCD has a sliver of glass, inside it.
        for tiny in [band(0.0), Rect::from_min_size(Pos2::ZERO, Vec2::new(60.0, 20.0))] {
            let l = Layout::new(tiny, &rolling());
            let t = l.timecode;
            assert!(!t.min.x.is_nan() && !t.max.y.is_nan(), "a tiny band made a NaN readout: {t:?}");
            assert!(!t.is_positive() || l.lcd.contains_rect(t), "a tiny band's readout left its LCD");
        }
    }

    /// **And it reads from the bench**: forty points and more at the 900-point
    /// window where the old slot-0 clock was six and a half - and it does not
    /// jump a size as the digits change.
    #[test]
    fn the_take_readout_reads_from_the_bench() {
        for (w, clock, count) in [(900.0_f32, 30.0, 40.0), (1300.0, 45.0, 60.0)] {
            let g = Layout::new(band(w), &rolling()).timecode;
            assert!(readout_size(g, RecordState::Rolling, "4:12") >= clock, "at {w} the take clock is small");
            assert!(
                readout_size(g, RecordState::CountIn { beat: 3, of: 4 }, "3") >= count,
                "at {w} the count-in digit is small"
            );
            assert!(readout_size(g, RecordState::Finishing, "FINISHING") >= 20.0 * w / 900.0);
            assert_eq!(
                readout_size(g, RecordState::Rolling, "9:59"),
                readout_size(g, RecordState::Rolling, "10:00"),
                "the clock jumps a size at ten minutes"
            );
            assert_eq!(
                readout_size(g, RecordState::CountIn { beat: 1, of: 12 }, "1"),
                readout_size(g, RecordState::CountIn { beat: 12, of: 12 }, "12")
            );
        }
        // On a NARROW glass the width binds, and a size taken from the text
        // itself would jump at every digit that changes the string's length.
        let narrow = Rect::from_min_size(Pos2::ZERO, Vec2::new(40.0, 60.0));
        assert_eq!(
            readout_size(narrow, RecordState::Rolling, "4:12"),
            readout_size(narrow, RecordState::Rolling, "10:00"),
            "on a narrow glass the clock grows and shrinks with its digits"
        );
        assert_eq!(
            readout_size(narrow, RecordState::CountIn { beat: 1, of: 12 }, "1"),
            readout_size(narrow, RecordState::CountIn { beat: 12, of: 12 }, "12")
        );
    }

    /// **The count-in digit is red and readable on every theme's glass.**
    #[test]
    fn the_count_in_is_red_on_every_glass() {
        for t in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = Settings {
                    dark_mode: dark,
                    ..Settings::default()
                };
                t.apply(&mut set);
                let p = palette(&set);
                let c = readout_ink(RecordState::CountIn { beat: 1, of: 4 }, &p);
                assert!(
                    contrast_ratio(c, p.lcd_face) >= 3.0,
                    "{} (dark {dark}): the count-in stands {:.2}:1 off the glass",
                    t.name,
                    contrast_ratio(c, p.lcd_face)
                );
                assert!(c.r() > c.g() && c.r() > c.b(), "{}: the count-in is not red: {c:?}", t.name);
                assert_eq!(readout_ink(RecordState::Rolling, &p), p.lcd);
            }
        }
    }

    /// Every leaf shape the band paints for `view` at `r`.
    fn band_shapes(r: Rect, view: &RecorderView<'_>) -> Vec<egui::Shape> {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(r),
                ..Default::default()
            },
            |ctx| {
                let painter = Painter::new(ctx.clone(), egui::LayerId::background(), r);
                draw(&painter, r, view, &Settings::default());
            },
        );
        fn walk(shape: egui::Shape, out: &mut Vec<egui::Shape>) {
            match shape {
                egui::Shape::Vec(v) => v.into_iter().for_each(|s| walk(s, out)),
                other => out.push(other),
            }
        }
        let mut leaves = Vec::new();
        for c in out.shapes {
            walk(c.shape, &mut leaves);
        }
        leaves
    }

    /// **What `draw` paints for a held tempo, not what a helper returns.** The
    /// knob's cap loses its light and TAP its orange, on the band itself;
    /// with the tempo free both are at full colour.
    #[test]
    fn the_band_paints_a_held_tempo_dim() {
        let p = palette(&Settings::default());
        for w in [900.0_f32, 1300.0] {
            for held in [false, true] {
                let view = RecorderView {
                    state: RecordState::Rolling,
                    tempo_held: held,
                    ..furnished()
                };
                let r = band(w);
                let l = Layout::new(r, &view);
                assert!(l.tempo.is_positive() && l.tempo_tap.is_positive(), "at {w} no tempo pair");
                let shapes = band_shapes(r, &view);
                let caps: Vec<Color32> = shapes
                    .iter()
                    .filter_map(|s| match s {
                        egui::Shape::Circle(c) if l.tempo.contains(c.center) => Some(c.fill),
                        _ => None,
                    })
                    .collect();
                let (lit, dim) = (TEMPO_CAP, TEMPO_CAP.gamma_multiply(0.33));
                assert_eq!(caps.contains(&lit), !held, "at {w} held {held}: caps {caps:?}");
                assert_eq!(caps.contains(&dim), held, "at {w} held {held}: caps {caps:?}");
                let tap = shapes
                    .iter()
                    .find_map(|s| match s {
                        egui::Shape::Text(t) if t.galley.text() == "TAP" => {
                            t.galley.job.sections.first().map(|s| s.format.color)
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("at {w} no TAP painted"));
                let want = if held { toward(TEMPO_CAP, p.bg, 0.55) } else { TEMPO_CAP };
                assert_eq!(tap, want, "at {w} TAP with the tempo held = {held}");
            }
        }
    }

    /// **The glass is the readout's or the scope's, never both and never
    /// neither.** While a take rolls the scope draws nothing under the digits;
    /// with the clock hidden the scope has the glass back; and a glass too
    /// small for the digits keeps its scope rather than going blank.
    #[test]
    fn the_glass_is_the_readouts_or_the_scopes() {
        // Paint on the glass that is not text: the scope's face, its
        // graticule, its trace.
        let scope_on = |r: Rect, view: &RecorderView<'_>| {
            let glass = lcd_scope_rect(Layout::new(r, view).lcd);
            let shapes = band_shapes(r, view);
            let inside = |s: &egui::Shape| glass.expand(0.5).contains_rect(s.visual_bounding_rect());
            let marks = shapes.iter().filter(|s| inside(s) && !matches!(s, egui::Shape::Text(_))).count();
            let words = shapes.iter().filter(|s| inside(s) && matches!(s, egui::Shape::Text(_))).count();
            (marks, words)
        };
        for w in [900.0_f32, 1300.0, 2560.0] {
            let r = band(w);
            let (marks, words) = scope_on(r, &rolling());
            assert_eq!(marks, 0, "at {w} the scope draws under the take's clock");
            assert_eq!(words, 1, "at {w} the take's clock is not on the glass");
            let hidden = RecorderView {
                hide_elapsed: true,
                ..rolling()
            };
            let (marks, words) = scope_on(r, &hidden);
            assert!(marks > 0, "at {w} the clock is hidden and the glass is blank");
            assert_eq!(words, 0, "at {w} a hidden clock is still drawn");
            let (marks, _) = scope_on(r, &idle());
            assert!(marks > 0, "at {w} the scope is gone at rest");
        }
        // A glass that cannot hold the digits: its scope, not an empty face.
        let mut found = false;
        let mut h = 4.0;
        while h < 40.0 {
            let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, h));
            let l = Layout::new(r, &rolling());
            let glass = lcd_scope_rect(l.lcd);
            if glass.is_positive() && !l.timecode.is_positive() {
                found = true;
                let (marks, words) = scope_on(r, &rolling());
                assert!(marks > 0, "at height {h} the glass is blank: no digits, no scope");
                assert_eq!(words, 0);
            }
            h += 1.0;
        }
        assert!(found, "no band small enough to test the empty-glass case");
    }

    /// **A count recorded INTO a take reads its beat, in red, on the glass** -
    /// the session is already Rolling, so only `count_beat` says a count is
    /// on - and hiding the elapsed clock does not hide the beat.
    #[test]
    fn a_count_inside_a_take_paints_its_beat_in_red() {
        let p = palette(&Settings::default());
        for w in [900.0_f32, 1300.0] {
            for hide in [false, true] {
                let view = RecorderView {
                    count_beat: Some(3),
                    hide_elapsed: hide,
                    ..rolling()
                };
                let r = band(w);
                let l = Layout::new(r, &view);
                assert!(l.timecode.is_positive(), "at {w} hide {hide}: the beat has no glass");
                let texts: Vec<(String, Color32)> = band_shapes(r, &view)
                    .iter()
                    .filter_map(|s| match s {
                        egui::Shape::Text(t) if l.timecode.contains(t.visual_bounding_rect().center()) => Some((
                            t.galley.text().to_owned(),
                            t.galley.job.sections.first().map_or(Color32::TRANSPARENT, |s| s.format.color),
                        )),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    texts,
                    vec![("3".to_owned(), count_red(p.rec, p.lcd_face))],
                    "at {w} hide {hide}: the glass reads {texts:?}"
                );
            }
        }
    }

    /// **A Play waiting on a count's downbeat lights the triangle, and the
    /// Pause that takes it back.** Play pressed during a count-in starts the
    /// song on the downbeat, and nothing on the band showed it had been
    /// pressed: the triangle stayed dark over a song about to start.
    #[test]
    fn a_play_waiting_on_the_downbeat_lights_the_play_triangle() {
        for pending in [false, true] {
            let view = RecorderView {
                play_pending: pending,
                ..furnished()
            };
            let r = band(1300.0);
            let l = Layout::new(r, &view);
            assert!(l.play.is_positive() && l.pause.is_positive(), "no transport at this width");
            let shapes = band_shapes(r, &view);
            let triangle = shapes
                .iter()
                .find_map(|s| match s {
                    egui::Shape::Path(path)
                        if path.closed
                            && path.points.len() == 3
                            && path.points.iter().all(|pt| l.play.expand(1.0).contains(*pt)) =>
                    {
                        Some(path.fill)
                    }
                    _ => None,
                })
                .expect("the play triangle is not drawn");
            assert_eq!(triangle == PLAY_GREEN, pending, "pending {pending}: the triangle is {triangle:?}");
            let bars: Vec<Color32> = shapes
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Rect(rs) if l.pause.contains(rs.rect.center()) => Some(rs.fill),
                    _ => None,
                })
                .collect();
            assert_eq!(bars.len(), 2, "the pause bars: {bars:?}");
            assert!(
                bars.iter().all(|c| (*c == PLAY_GREEN) == pending),
                "pending {pending}: the pause bars are {bars:?}"
            );
        }
    }

    /// **The metronome panel's TEMPO row dims with the knob** - the other
    /// road to the same number wears the same rule.
    #[test]
    fn the_metronome_panels_tempo_dims_while_held() {
        let p = palette(&Settings::default());
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1300.0, 900.0));
        let anchor = Rect::from_center_size(Pos2::new(400.0, 200.0), Vec2::splat(1.0));
        let row = MetLayout::new(screen, anchor).rows[0];
        assert!(row.is_positive());
        let track = MetLayout::track(row);
        for held in [false, true] {
            let ctx = egui::Context::default();
            fonts::install(&ctx, fonts::FontChoice::default(), None);
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                let painter = Painter::new(ctx.clone(), egui::LayerId::background(), screen);
                let v = MetView {
                    bpm: 120.0,
                    beats: 4,
                    unit: 4,
                    div: 1,
                    sample: "",
                    count_in_bars: 1,
                    count_in_in_take: false,
                    held,
                };
                draw_met(&painter, screen, anchor, v, &Settings::default());
            });
            let fills: Vec<Color32> = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Rect(r) if row.contains_rect(r.rect) && track.expand(4.0).contains_rect(r.rect) => {
                        Some(r.fill)
                    }
                    _ => None,
                })
                .collect();
            let (fill, handle) = met_tempo_inks(held, &p);
            assert!(fills.contains(&fill) && fills.contains(&handle), "held {held}: the row paints {fills:?}");
            let lit = [p.accent, p.ink];
            assert_eq!(
                fills.iter().any(|f| lit.contains(f)),
                !held,
                "held {held}: the row paints {fills:?}"
            );
        }
        assert_ne!(met_tempo_inks(true, &p), met_tempo_inks(false, &p));
    }

    /// **A held tempo is dim and still a control**: it keeps its place and
    /// its press, so a hand on it is never a hand on empty band (which, in a
    /// borderless window, drags the window).
    #[test]
    fn a_held_tempo_is_dim_and_still_a_control() {
        assert!(tempo_knob(120.0, None, false, true).dim);
        assert!(!tempo_knob(120.0, None, false, false).dim);
        for w in [900.0_f32, 1300.0] {
            let r = band(w);
            let view = RecorderView {
                state: RecordState::CountIn { beat: 1, of: 4 },
                tempo_held: true,
                ..furnished()
            };
            let l = Layout::new(r, &view);
            assert_eq!(l.tempo, Layout::new(r, &furnished()).tempo, "at {w} a held tempo moved");
            let at = knob_centre(l.tempo).expect("the held knob has a face");
            assert!(
                hit_test(r, &view, at).is_some_and(|h| h.is_same_control(Hit::SetTempo(0.0))),
                "at {w} a press on the held knob is not a press on the knob"
            );
            assert_eq!(hit_test(r, &view, l.tempo_tap.center()), Some(Hit::TapTempo));
        }
    }

    /// The record button is the ARRANGEMENT's and stays one under a take;
    /// the take is told apart by the cassette's amber and the window's red
    /// outline, not by the red button turning into a picture of itself.
    #[test]
    fn the_record_button_stays_a_button_under_a_take() {
        let r = band(1300.0);
        let at_rest = Layout::new(r, &idle());
        assert!(at_rest.record.is_positive());
        let live = Layout::new(r, &rolling());
        assert_eq!(live.record, at_rest.record, "the red button moved or went away");
        // **The take's readout is on the LCD's glass**, the height of the
        // LCD, and the LCD itself does not move.
        assert!(
            live.timecode.height() >= live.lcd.height() * 0.8,
            "the take's readout is not on the glass: {:?} in {:?}",
            live.timecode,
            live.lcd
        );
        assert_eq!(live.lcd, at_rest.lcd, "the LCD moved when the take started");

        // **And the preview does NOT collapse.** It used to, back when it was
        // a framing check somebody glanced at before a take. A take records
        // the window now, so this box is the camera inset the recording will
        // carry, and the moment it matters most is the moment it used to
        // shrink.
        assert_eq!(
            live.preview, at_rest.preview,
            "the preview moved when the take started"
        );
        // **The meter no longer has to grow, and asserting that it does would
        // be asserting the old band.** It was tiny at rest when the transport
        // was a quarter of a narrow column, so growing was the only way it
        // could become readable. The transport is half the band now and the
        // meters are large in both layouts; what the rolling one buys is the
        // CLOCK, which takes nearly half the group. So: the meter stays
        // substantial, and the clock is what changes.
        let area = |r: Rect| r.width() * r.height();
        assert!(
            area(live.meter) > area(at_rest.meter) * 0.4,
            "the rolling meter collapsed: {:?} vs {:?}",
            live.meter,
            at_rest.meter
        );
        assert!(
            live.meter.height() >= at_rest.meter.height(),
            "the rolling meter got shorter, which is what its faces are sized by"
        );
    }

    /// **The needle and the scale it is read against cannot disagree.**
    ///
    /// A drawn meter has one failure worse than being ugly: pointing somewhere
    /// the printing says is a different number. Both come out of `VU_MARKS`, so
    /// this test checks the one thing that could still drift — that a level
    /// lands at the fraction its own printed mark is drawn at.
    #[test]
    fn the_vu_needle_points_at_the_mark_the_face_prints() {
        for (vu, frac, _) in VU_MARKS {
            let level = 10_f32.powf((vu + VU_ZERO_DBFS) / 20.0);
            let got = vu_frac(level);
            assert!(
                (got - frac).abs() < 1e-3,
                "{vu} VU draws at {frac} and reads at {got}"
            );
        }
        // 0 VU is the top of the black arc and the start of the red, and it is
        // the same level the band has always drawn its red zone from.
        assert!((vu_frac(CLIP_ZONE) - VU_MARKS[6].1).abs() < 0.02);
        // Monotonic, and pinned at both stops: a needle that goes backwards or
        // walks off the card is worse than no meter.
        let mut last = -1.0;
        for i in 0..=200 {
            let f = vu_frac(i as f32 / 100.0);
            assert!(f >= last, "the needle went backwards at {i}");
            assert!((0.0..=1.0).contains(&f), "{f} is off the face");
            last = f;
        }
        assert_eq!(vu_frac(0.0), 0.0, "silence is not the left stop");
        assert_eq!(vu_frac(4.0), 1.0, "a huge level is not the right stop");
    }

    #[test]
    fn the_count_in_control_cycles_through_every_choice_and_comes_back() {
        let mut seen = Vec::new();
        let mut v = COUNT_IN_CHOICES[0];
        for _ in 0..COUNT_IN_CHOICES.len() {
            seen.push(v);
            v = next_count_in(v);
        }
        assert_eq!(seen, COUNT_IN_CHOICES.to_vec());
        assert_eq!(v, COUNT_IN_CHOICES[0], "the cycle does not come back round");
        // A value a hand-edited settings file could hold lands somewhere real
        // rather than sticking.
        assert_eq!(next_count_in(97), COUNT_IN_CHOICES[0]);
        // And a length the metronome panel steps to, off the list, steps ON
        // to the next choice above it rather than starting over at off.
        assert_eq!(next_count_in(3), 4, "3 bars stepped back to off");
        // And the label says BARS, because that is what the choices are now
        // that there is a time signature to count them in. It said beats, and
        // went on saying beats after the setting became bars — so the cell
        // showed a number the control no longer changed.
        assert_eq!(count_in_text(0), "off");
        assert_eq!(count_in_text(1), "1 bar");
        assert_eq!(count_in_text(2), "2 bars");
    }

    /// **Everything draggable is typeable; not everything typeable is
    /// draggable.**
    ///
    /// It used to be an equality, and that was right while every numeric cell
    /// was a fader: a draggable hit with no field would swallow a tap and do
    /// nothing. The time signature broke the symmetry honestly rather than
    /// accidentally — "6/8" is two numbers and a slash, and there is no
    /// continuum between 4/4 and 7/8 to drag along, so it is typed and only
    /// typed.
    #[test]
    fn every_control_you_can_drag_is_one_you_can_type_into() {
        for hit in Hit::ALL {
            // **The pitch dial is the one exception, deliberately.** It
            // prints no number, so there is nothing for a typed one to
            // agree with — you find the pitch by ear. See `num_field`. Its
            // RANGE is still typed, in the panel a right-click opens.
            if hit.is_same_control(Hit::SetVarispeed(0.0)) {
                assert!(
                    num_field(hit).is_none(),
                    "the pitch dial grew a number field again"
                );
                continue;
            }
            if hit.is_draggable() {
                assert!(
                    num_field(hit).is_some(),
                    "{hit:?} can be dragged and not typed into"
                );
            }
        }
        // And the one that goes the other way is the one it is supposed to be.
        let typed_only: Vec<Hit> = Hit::ALL
            .into_iter()
            .filter(|h| num_field(*h).is_some() && !h.is_draggable())
            .collect();
        // What is typed and never dragged: a time signature, which has no
        // continuum between 4/4 and 7/8 to sweep along, and the `Type` targets
        // themselves — the number at the end of a fader and the word over a
        // knob, which exist to be typed into and nothing else.
        assert!(
            typed_only
                .iter()
                .all(|h| matches!(h, Hit::EditTimeSignature | Hit::Type(_))),
            "{typed_only:?}"
        );
        assert!(typed_only.contains(&Hit::EditTimeSignature));
    }

    /// The four fields are four fields, not one used four times.
    ///
    /// **`Type` is excluded, and deliberately.** A `Type` hit names the same
    /// field as the control it sits on — that is the whole point of it, and
    /// counting both would make every field look shared with itself.
    #[test]
    fn each_control_owns_its_own_field() {
        let fields: Vec<_> = Hit::ALL
            .into_iter()
            .filter(|h| !matches!(h, Hit::Type(_)))
            .filter_map(num_field)
            .collect();
        let mut seen = fields.clone();
        seen.sort_by_key(|f| format!("{f:?}"));
        seen.dedup();
        assert_eq!(
            seen.len(),
            fields.len(),
            "two controls share a field, so typing into one would edit the other"
        );
        assert!(fields.contains(&NumField::Tempo));
        assert!(fields.contains(&NumField::Master));
    }

    #[test]
    fn a_tempo_reads_as_a_round_number_unless_it_is_not_one() {
        assert_eq!(tempo_text(120.0), "120");
        assert_eq!(tempo_text(92.5), "92.5");
        assert_eq!(tempo_text(60.0), "60");
    }

    #[test]
    fn the_export_line_says_what_a_take_will_produce() {
        use crate::recorder::VideoMode;
        assert_eq!(
            export_summary(&ExportSpec {
                video: VideoMode::None,
                ..Default::default()
            }),
            "wav + midi"
        );
        // The shipped default is not that one. A take records the window.
        assert_eq!(export_summary(&ExportSpec::default()), "wav + midi + 1 video");
        assert_eq!(
            export_summary(&ExportSpec {
                video: VideoMode::Composite,
                ..Default::default()
            }),
            "wav + midi + 1 video"
        );
        assert_eq!(
            export_summary(&ExportSpec {
                audio: false,
                video: VideoMode::Both,
                ..Default::default()
            }),
            "midi + 3 videos"
        );
        // `problem()` refuses this at the moment of recording, but the band
        // still has to render it rather than showing an empty line.
        assert_eq!(
            export_summary(&ExportSpec {
                audio: false,
                midi: false,
                video: VideoMode::None,
                ..Default::default()
            }),
            "nothing"
        );
    }

    /// The painter has to survive every state at every size, with and without a
    /// frame. It draws real glyphs, so a bare `Context` with no fonts bound
    /// panics rather than failing an assertion — hence the install.
    #[test]
    fn the_band_paints_in_every_state_at_every_size() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        let pv = Preview {
            texture: egui::TextureId::User(7),
            size: Vec2::new(640.0, 480.0),
        };
        let _loaded_track = crate::ports::TrackInfo {
            name: "backing.mp3".to_owned(),
            seconds: 214.0,
            wave: (0..1000)
                .map(|i| ((i as f32 / 60.0).sin().abs() * 0.9).min(1.0))
                .collect(),
            error: String::new(),
        };
        for dark in [false, true] {
            let s = Settings {
                dark_mode: dark,
                ..Settings::default()
            };
            for w in [0.0_f32, 60.0, 400.0, 1300.0, 2600.0] {
                for state in STATES {
                    for preview in [None, Some(pv)] {
                        // Every control that can be typed into, drawn mid-edit
                        // at every width — including the degenerate ones, where
                        // the caret's own rectangle is what would go negative.
                        for edit in typing_states() {
                            let v = RecorderView {
                                scope: RecorderView::empty().scope,
                                analyzer: RecorderView::empty().analyzer,
                                pointer: None,
                                prefer_flats: false,
                                project_dest: "",
                                export_dest: "",
                                preset_dest: "",
                                irs_dest: "",
                                position_s: 0.0,
                                playing: false,
                                arr_recording: false,
                                exporting: false,
                                tempo_held: matches!(state, RecordState::CountIn { .. }),
                                count_beat: None,
                                play_pending: false,
                                view_window: (0.0, 60.0),
                                input_monitor: false,
                                scrub: crate::recorder::ScrubView::default(),
                                state,
                                elapsed_s: 3725.0,
                                meters: Meters {
                                    left: Level {
                                        peak: 1.4,
                                        rms: 0.3,
                                        hold: 0.9,
                                        vu: 0.3,
                                    },
                                    right: Level::default(),
                                    mono: state == RecordState::Rolling,
                                    clipped: true,
                                },
                                dest: "~/Movies/Tangent",
                                take_name: "nocturne",
                                name_focused: true,
                                editing: edit.as_ref(),
                                folder_preview: "nocturne-2026-08-16-141203",
                                // The master at a different, awkward place on
                                // each pass: unity on one, well below on the
                                // other.
                                gains: Gains {
                                    master: if dark { 1.0 } else { 0.35 },
                                    ..Gains::default()
                                },
                                // Both ends of each knob's travel, so a slot or
                                // a pointer that escapes at one extreme is
                                // caught rather than only being drawn mid-way.
                                fx: crate::recorder::FxSends {
                                    reverb: 0.0,
                                    delay: 1.0,
                                    chorus: 0.5,
                                    phaser: 0.4,
                                    hpf: 0.35,
                                    lpf: 1.0,
                                    gain: 0.5,
                                    limiter: 0.7,
                                },
                                // The filters read in hertz, so the sweep
                                // also covers the longest string a knob can
                                // be asked to fit.
                                // Loud enough to be in the red on one pass
                                // and quiet on the other, with the limiter
                                // visibly working: the sweep has to cover a
                                // meter that is doing something.
                                master: Meters {
                                    left: Level {
                                        peak: if dark { 0.98 } else { 0.06 },
                                        rms: if dark { 0.7 } else { 0.03 },
                                        hold: if dark { 1.0 } else { 0.09 },
                                        vu: if dark { 0.7 } else { 0.03 },
                                    },
                                    right: Level {
                                        peak: if dark { 0.55 } else { 0.02 },
                                        rms: if dark { 0.4 } else { 0.01 },
                                        hold: if dark { 0.6 } else { 0.04 },
                                        vu: if dark { 0.4 } else { 0.01 },
                                    },
                                    mono: false,
                                    clipped: dark,
                                },
                                gr_db: if dark { 7.5 } else { 0.0 },
                                // A loaded track on one pass and none on the
                                // other, so the sweep covers both the row that
                                // is an offer and the row that is a control.
                                track: if dark { a_track() } else { crate::ports::TrackInfo::NONE },
                                varispeed: crate::varispeed::Varispeed::default(),
                                loudness: crate::recorder::Loudness::default(),
                                fx_units: [
                                    KnobUnit::Percent,
                                    KnobUnit::Percent,
                                    KnobUnit::Percent,
                                    KnobUnit::Percent,
                                    KnobUnit::Hertz { low: 20.0, high: 1_200.0 },
                                    KnobUnit::Hertz { low: 200.0, high: 20_000.0 },
                                    KnobUnit::Decibels { low: 0.0, high: 24.0 },
                                    KnobUnit::Decibels { low: -48.0, high: 0.0 },
                                ],
                                // A knob mid-turn, so the sweep also covers a
                                // knob showing a number instead of its name.
                                turning: Some(NumField::Fx(Fx::Reverb)),
                                metronome_on: true,
                                metronome_in_take: dark,
                                tempo_bpm: 92.5,
                                count_in_beats: 8,
                                count_in_bars: 2,
                                time_signature: crate::recorder::TimeSignature { beats: 6, unit: 8 },
                                camera: DeviceLabel::Missing("FaceTime HD Camera"),
                                audio: DeviceLabel::Open("Scarlett 2i2 USB"),
                                preview,
                                disk_minutes: Some(134.0),
                                hide_elapsed: dark,
                                message: Some("recorded 4:12 to nocturne-2026-08-16-141203"),
                                clip_warning: true,
                                limiter_off: false,
                            };
                            let r = band(w);
                            let _ = ctx.run(Default::default(), |ctx| {
                                egui::CentralPanel::default().show(ctx, |ui| {
                                    draw(ui.painter(), r, &v, &s);
                                    draw_top_edge(ui.painter(), r, &s);
                                });
                            });
                            // And the hit test agrees it is looking at the same
                            // rectangle, at every size, without panicking on the
                            // degenerate ones.
                            let _ = hit_test(r, &v, r.center());
                        }
                    }
                }
            }
        }
    }

    /// No edit, plus one per typeable field, plus the two shapes a field passes
    /// through on the way to a number: empty, and a lone minus sign.
    fn typing_states() -> Vec<Option<crate::recorder::NumEdit>> {
        let mut v = vec![None];
        for field in [
            NumField::Master,
            NumField::Fx(Fx::Reverb),
            NumField::Tempo,
        ] {
            for text in ["", "-", "-12.5"] {
                v.push(Some(crate::recorder::NumEdit {
                    field,
                    text: text.to_owned(),
                    caret: 0,
                    seeded: false,
                }));
            }
        }
        v
    }
    /// **The selection rim is visible on every plate a track can wear.**
    ///
    /// It used to be one fixed warm white at low alpha. A painted channel takes
    /// its colour for the WHOLE strip, and the arrange header takes the same
    /// colour for its plate, so on the pale end of the strip palette the rim
    /// was near-white on near-white — and it is the only thing that says which
    /// tracks are in a multiple selection. Three hundred strip colours times
    /// sixteen themes times two modes is a lot of places for that to hide.
    #[test]
    fn the_selection_rim_shows_on_every_plate_a_track_can_wear() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let p = palette(&set);
                let mode = if dark { "dark" } else { "light" };
                // The plates an UNPAINTED track wears, which are the ones
                // that move with the theme.
                for under in [p.bg, p.field, p.well, p.lcd_face] {
                    let r = contrast_ratio(selection_rim(under), under);
                    if r < 1.9 {
                        bad.push(format!(
                            "{} {mode}: rim on {:?} is {r:.2}:1",
                            theme.name, under
                        ));
                    }
                }
            }
        }
        // **And every colour a PAINTED track can take.** These do not move
        // with the theme — the table is fixed — so they are walked once, but
        // they are where the old fixed rim actually disappeared: the pale end
        // of the strip palette.
        for (i, (r, g, b)) in crate::mixer_panel::STRIP_COLORS.iter().enumerate() {
            let under = Color32::from_rgb(*r, *g, *b);
            let ratio = contrast_ratio(selection_rim(under), under);
            if ratio < 1.9 {
                bad.push(format!("strip colour {i} ({r},{g},{b}): {ratio:.2}:1"));
            }
        }
        assert!(
            bad.is_empty(),
            "a selected track cannot be told from an unselected one:\n  {}",
            bad.join("\n  ")
        );
    }

    /// **The supporter heart can be seen, whatever colour is passing through
    /// it.**
    ///
    /// The wave takes its colours from the live palette, and a palette colour
    /// is a SURFACE colour — the whole table is held under a luminance ceiling
    /// so light text printed on a painted channel still clears 4.5:1. Drawn
    /// raw as a small sprite on the band's own dark face, half of them would
    /// be a smudge. Sixteen colours times sixteen palettes times two modes is
    /// a lot of places for one of them to vanish.
    #[test]
    fn the_supporter_heart_shows_on_every_palette_and_theme() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let face = band_face(&set);
                let mode = if dark { "dark" } else { "light" };
                for pal in &crate::mixer_panel::PALETTES {
                    for c in 0..crate::recorder::CHANNELS {
                        let (sr, sg, sb) =
                            crate::mixer_panel::STRIP_COLORS[pal.slot(c)];
                        let tint = heart_shade(Color32::from_rgb(sr, sg, sb), face);
                        let on_face = contrast_ratio(tint, face);
                        // Three, not five — see `heart_shade::ON_BAND`. The
                        // owner asked for every shade to be BRIGHT, and on a
                        // pale band a five-to-one floor is a floor that
                        // darkens whatever brightness put there. The outline
                        // checked below is what carries the shape.
                        if on_face < 3.0 {
                            bad.push(format!(
                                "{} {mode}: {} slot {c} is only {on_face:.2}:1 on the band",
                                theme.name, pal.name
                            ));
                        }
                        // **The outline has to be tellable from the fill it
                        // edges.** Against the BAND it need not be: the fill
                        // already clears that at 3.5:1 above, so the shape is
                        // there whether or not the outline shows on the band.
                        let border = heart_border(tint);
                        let edge = contrast_ratio(border, tint);
                        if edge < 1.5 {
                            bad.push(format!(
                                "{} {mode}: {} slot {c} outline is only \
                                 {edge:.2}:1 on its own fill",
                                theme.name, pal.name
                            ));
                        }
                    }
                }
            }
        }
        assert!(
            bad.is_empty(),
            "the supporter heart disappears:\n  {}",
            bad.join("\n  ")
        );

        // **And the shades are BRIGHTER than the strips they come from**,
        // which is the whole reason `heart_shade` exists rather than the
        // colours being used raw: a strip colour is muted on purpose, and
        // muted on a sprite seven pixels wide is a grey smudge.
        let mut set = Settings::default();
        set.dark_mode = true;
        let face = band_face(&set);
        let mut flat = 0;
        for pal in &crate::mixer_panel::PALETTES {
            for c in 0..crate::recorder::CHANNELS {
                let (r, g, b) = crate::mixer_panel::STRIP_COLORS[pal.slot(c)];
                let raw = Color32::from_rgb(r, g, b);
                if contrast_ratio(heart_shade(raw, face), face) <= contrast_ratio(raw, face) {
                    flat += 1;
                }
            }
        }
        assert_eq!(flat, 0, "{flat} heart shades are no brighter than their strip");
    }

    /// **A dial's scale arcs over the TOP of it, and its gap is at the bottom.**
    ///
    /// It was upside down for many versions and nobody caught it, because the
    /// one thing that could have shown it cannot: the pointer is a slot drawn
    /// from one edge of the cap to the other straight through the centre, so
    /// turning it half a turn draws exactly the same line. The ticks were the
    /// only evidence, hanging under the dial like a smile, and reading a scale
    /// the hand on the knob is covering is the thing every real panel avoids.
    #[test]
    fn a_dials_scale_arcs_over_the_top_of_it() {
        // Straight up, dead centre of the sweep.
        let mid = knob_tick_dir(KNOB_TICKS / 2);
        assert!(
            mid.y < -0.99 && mid.x.abs() < 0.01,
            "the middle tick points {mid:?}, which is not twelve o'clock"
        );
        // Symmetric about it: the first and last mirror each other.
        let (a, b) = (knob_tick_dir(0), knob_tick_dir(KNOB_TICKS - 1));
        assert!(
            (a.x + b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01,
            "the scale is lopsided: {a:?} against {b:?}"
        );
        // And the whole arc leans upward — the missing quarter is the BOTTOM.
        let lean: f32 = (0..KNOB_TICKS).map(|i| knob_tick_dir(i).y).sum();
        assert!(lean < 0.0, "the scale hangs below the dial, sum {lean}");
        // Nothing reaches straight down, which is where the gap belongs.
        for i in 0..KNOB_TICKS {
            assert!(
                knob_tick_dir(i).y < 0.75,
                "tick {i} is drawn under the dial at {:?}",
                knob_tick_dir(i)
            );
        }
    }

    /// **A dial's number is what you click to type into it.**
    ///
    /// The reading and the name swapped places — name on top, field below —
    /// and the typing target stayed at the top, where the name now is. So
    /// clicking the number did nothing and clicking the name opened a number
    /// field. Both halves are asserted, because either one alone would have
    /// passed while the pair was wrong.
    #[test]
    fn the_type_field_is_over_the_number_not_over_the_name() {
        let r = band(1300.0);
        let v = idle();
        let l = Layout::new(r, &v);
        for fx in [Fx::Reverb, Fx::Delay, Fx::Gain, Fx::Limiter] {
            let cell = l.fx[fx.index()];
            let Some(f) = knob_face(cell) else { continue };
            if f.label_h <= 0.0 {
                continue;
            }
            // Just inside the foot of the cell: the reading, and the field.
            let low = Pos2::new(cell.center().x, cell.bottom() - 1.0);
            assert_eq!(
                hit_test(r, &v, low),
                Some(Hit::Type(NumField::Fx(fx))),
                "{fx:?}: the number does not open a field"
            );
            // Just inside the top: the NAME, which drags the dial like the
            // rest of the cell and must not open a field.
            let high = Pos2::new(cell.center().x, cell.top() + 1.0);
            assert!(
                !matches!(hit_test(r, &v, high), Some(Hit::Type(_))),
                "{fx:?}: the name opens a number field"
            );
        }
    }

    /// **The peak lamp answers "did this clip", not "is this loud".**
    ///
    /// It lit at [`CLIP_ZONE`] — six decibels down, where the FACE goes red —
    /// so a mix with a limiter holding a true peak of -1 dBFS lit it solidly
    /// and permanently. The arc is right to go red there; a lamp is read as an
    /// alarm and must not.
    #[test]
    fn the_peak_lamp_does_not_light_on_a_limited_mix() {
        // -1 dBFS, which is what a true-peak limiter is set to hold.
        let limited = 10f32.powf(-1.0 / 20.0);
        assert!(
            limited < CLIP_AT,
            "a mix held at -1 dBFS ({limited}) still lights the lamp"
        );
        // -6 dBFS is where the FACE goes red, and it must stay that way.
        assert!(
            (CLIP_ZONE - 10f32.powf(-6.0 / 20.0)).abs() < 0.005,
            "the red arc moved off -6 dBFS"
        );
        // And a real clip still lights it.
        assert!(1.0 >= CLIP_AT, "full scale does not light the lamp");
    }

    /// **A sine at the reference reads 0 VU**, which is what calibration means.
    ///
    /// The needle was fed raw RMS against a scale whose zero is a PEAK level,
    /// so every signal read 3.01 dB low before crest factor was considered —
    /// the sine correction is what closes that. See `TangentApp::VU_SINE`.
    #[test]
    fn a_sine_at_the_reference_reads_zero_vu() {
        // A sine whose peak is at VU_ZERO_DBFS: its RMS is 3.01 dB lower, and
        // the app multiplies by root two on the way in.
        let peak = 10f32.powf(VU_ZERO_DBFS / 20.0);
        let rms = peak / std::f32::consts::SQRT_2;
        let fed = rms * std::f32::consts::SQRT_2;
        let f = vu_frac(fed);
        // Mark index 6 is 0 VU.
        assert!(
            (f - VU_MARKS[6].1).abs() < 0.01,
            "a reference sine reads {f} where 0 VU is {}",
            VU_MARKS[6].1
        );
    }

    /// **A recess looks the same depth on every theme.**
    ///
    /// Its bevel used to be a fixed multiple of the well, which is a fixed
    /// RATIO and not a fixed amount of shading: barely visible on the dark
    /// palettes and, on Sandstone, Centennial and Porcelain, two plainly drawn
    /// curves on the left of the varispeed dial that appear on no other theme.
    #[test]
    fn the_recess_bevel_is_the_same_depth_on_every_theme() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let p = palette(&set);
                let mode = if dark { "dark" } else { "light" };
                let well = shade(p.well, if wants_light_ink(p.bg) { 0.66 } else { 0.88 });
                let shadow = contrast_ratio(recess_bevel(well, false), well);
                let light = contrast_ratio(recess_bevel(well, true), well);
                // **Never BOLD**, which is the failure that shipped: two
                // clearly drawn curves on the pale palettes.
                for (what, r) in [("shadowed", shadow), ("lit", light)] {
                    if r > 1.30 {
                        bad.push(format!(
                            "{} {mode}: the {what} edge is {r:.3}:1, a drawn shape",
                            theme.name
                        ));
                    }
                }
                // **And SYMMETRIC**, or the rim reads as one arc with nothing
                // opposite it — which is the same failure wearing one edge.
                if (shadow - light).abs() > 0.02 {
                    bad.push(format!(
                        "{} {mode}: the rim is lopsided, {shadow:.3}:1 against \
                         {light:.3}:1",
                        theme.name
                    ));
                }
            }
        }
        assert!(
            bad.is_empty(),
            "the recess is not the same depth everywhere:\n  {}",
            bad.join("\n  ")
        );
    }

    /// **A slot number can be read on every theme, in both views.**
    ///
    /// The arrange view drew an unused lane's number in the themed STRIP
    /// colour dimmed to a third — a colour used as ink, on a ground it has
    /// nothing to do with — so on every theme whose strip is dark the row
    /// numbers were a dark number on a dark ground. The mixer had no numbers
    /// at all. Both now take the ink that is measured against the ground they
    /// actually sit on, which is what the + circle beside each one was already
    /// doing correctly.
    #[test]
    fn a_slot_number_reads_on_every_theme() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let mode = if dark { "dark" } else { "light" };
                // The mixer's unoccupied slot: a ground cut out of this
                // theme's own wood, and ink measured against THAT. The ground
                // used to be one grey for all nineteen themes; see
                // `mixer_panel::empty_ground_for`.
                let face = set.fx_strip_color.to_color32();
                let wood = set.recorder_bg_color.to_color32();
                let ground = crate::mixer_panel::empty_ground_for(wood);
                let ink = crate::mixer_panel::empty_slot_ink_for(face, ground);
                let r = contrast_ratio(ink, ground);
                if r < 3.0 {
                    bad.push(format!("{} {mode}: the mixer slot number is {r:.2}:1", theme.name));
                }
                // **And the hole itself can be seen.** This is the fault the
                // owner reported — "they are all gray, and some are
                // imperceptible because of that" — so it is asserted here
                // rather than left to the eye.
                let seen = contrast_ratio(ground, wood);
                if seen < 1.65 {
                    bad.push(format!(
                        "{} {mode}: the mixer's empty slot is {seen:.2}:1 against the desk",
                        theme.name
                    ));
                }
            }
        }
        assert!(
            bad.is_empty(),
            "a slot number cannot be read:\n  {}",
            bad.join("\n  ")
        );
    }

}

// ── The take-settings popup ─────────────────────────────────────────────────
//
// **The controls that left the band, back as controls.** They spent one
// release as menu rows, and a menu row is a poor home for a folder path you
// want to READ, a device that may say "(not connected)", or three ticks whose
// current state is the question. Every one of them is a box with a caption and
// a value again — the same boxes the band drew, in the same ink, laid out with
// room to breathe rather than squeezed into a column between the transport and
// the instruments.
//
// A popup and not a band, because that is what they are: set once at the start
// of a session, and in the way for the rest of it. It hangs off the cog in the
// band and it is drawn by the same painters as everything else here, so it
// works in a window, in a plugin, and in a recording.

/// How wide the popup is, as a share of the window, and the bounds on that.
/// **Small.** The first cut took a third of the window and gave every row
/// forty points of height, which `labelled` turns into type twice the size of
/// the band's own — a settings panel that shouts. It is a utility panel: it
/// wants to be readable and out of the way, at about the weight of the boxes
/// it was summoned from.
// A third of the screen, and never under 380 points: the owner called the
// old 290-430 card "too small and a pain to use", and the fix is ROOM —
// every row's text sizes itself to its rectangle, so width IS legibility.
const SETUP_W: (f32, f32, f32) = (0.34, 380.0, 580.0);
/// The height over the width of a panel of [`SETUP_ROWS`] rows - the
/// proportion the one-panel design was tuned at. Read only through `ref_h` in
/// `SetupLayout::new`, which scales it to the [`SETUP_REF_ROWS`] reference
/// panel; no shape is drawn at this aspect.
const SETUP_ASPECT: f32 = 1.16;
/// **Not a row count any more.** No shape shows eleven rows; the number
/// survives as the denominator that turns [`SETUP_ASPECT`] into the reference
/// panel's height (`ref_h = w * SETUP_ASPECT * SETUP_REF_ROWS / SETUP_ROWS`),
/// so changing it changes every shape's row pitch. What each shape actually
/// shows is `SETUP_MACHINE_ROWS`, `SETUP_TAKE_ROWS` and `SETUP_ROLLING_ROWS`.
const SETUP_ROWS: usize = 11;

/// **The panel every proportion here was tuned on**: eight rows under two
/// headings. It is a REFERENCE and not a shape - no shape shows eight rows -
/// and it exists so that a row is the same size in all three of them. The
/// height used to be this one scaled by how many rows a shape showed, which
/// is right for the panel and wrong for the row: the red button's single row
/// came out half the height of the same row under the cog, and the owner
/// could not read it.
const SETUP_REF_ROWS: usize = 8;

/// How many rows each shape actually shows. The headings are counted on top
/// of these, and the panel is as tall as its own rows need at the reference
/// pitch.
///
/// **Seven, not eight.** The take tier left this panel and the count did not
/// follow it, so the cog's panel kept a row's worth of nothing under its last
/// control - the owner's "undesirable margin/empty space from the bottom".
const SETUP_MACHINE_ROWS: usize = 7;
const SETUP_TAKE_ROWS: usize = 6;
/// The red button's shape: one row of two controls - the AUDIO tier's
/// count-in and signature - under the panel's own title, with no heading band.
const SETUP_ROLLING_ROWS: usize = 1;

/// **Which tiers of the setup panel a caller wants.**
///
/// One panel, three shapes, rather than three panels. The owner's call -
/// "move the entire take section of the settings menu to summon when you
/// right click the capture take button" - and the cheap way to honour it is
/// not to build more panels with their own copies of the controls. Copies
/// drift: one of them gains a clamp, the other does not, and a year later
/// two places disagree about what a count-in may be.
///
/// So a tier is a SHAPE of the same layout, drawn by the same code and
/// hit-tested by the same table. The bands a shape leaves out are laid out
/// far below the panel, where the clip cannot show them and
/// [`setup_hit_test`]'s own "is it even in the panel" guard cannot reach
/// them.
///
/// **Each shape hangs off the control that opened it**
/// (`TangentApp::setup_anchor`): `Machine` off the cog, or the cassette when
/// the layout has unseated the cog (mid-take, or a band too small to draw
/// it); `Take` off the cassette; `Rolling` off the red button, or the
/// cassette while a realtime export has unseated it. An unseated control's
/// rect is `Rect::NOTHING`, and a panel hung off it is NaN - not drawn,
/// still open, and eating the next press - so a shape whose control can be
/// unseated has to fall back to a seated one or close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SetupShape {
    /// The cog's panel: the machine and where things are kept.
    #[default]
    Machine,
    /// The capture button's panel: the take being made right now.
    Take,
    /// **The record button's panel: the two settings somebody reaches for at
    /// the red button**, and nothing else.
    ///
    /// The owner asked for this shape first, got the whole take tier on the
    /// grounds that a copy of two controls is a copy that drifts, and said
    /// no: "The record button now summmons the menu you put on the takes
    /// button, which should not happen." They are right, and the drift
    /// argument was answered by the wrong thing - the fix is that it is not
    /// a COPY. It is the same layout, the same draw and the same hit test,
    /// showing two of its rows.
    Rolling,
}

impl SetupShape {
    /// **The word for the one tier a single-tier shape shows** - TAKE or
    /// RECORDING - worn as that panel's TITLE (see
    /// [`SetupShape::title_word`]): those two shapes carry no heading band,
    /// because a panel headed TAKE over a heading reading TAKE wore its own
    /// name twice. `Machine` names its AUDIO and MEDIA tiers where they are
    /// drawn, so its answer here is never shown. (`draw_setup` still pairs
    /// this word with `take_head`, which no shape lays out on the panel.)
    pub fn tier_word(self) -> &'static str {
        match self {
            SetupShape::Rolling => "RECORDING",
            _ => "TAKE",
        }
    }

    /// **What the panel is called, at the top of it.**
    ///
    /// The cog's panel is a list of tiers and SETTINGS is the honest name
    /// for all of them. The other two are ONE tier, and a title that said
    /// SETTINGS over a heading that said TAKE was a panel wearing its own
    /// name twice - the owner's "Remove their headers and just put the
    /// RECORDING and TAKE at the top instead". So the tier word IS the
    /// title there, and [`SetupLayout`] lays the heading band out where
    /// nothing can see it.
    pub fn title_word(self) -> &'static str {
        match self {
            SetupShape::Machine => "SETTINGS",
            other => other.tier_word(),
        }
    }

    /// **Whether the panel's own title already names the tier it shows.**
    ///
    /// True for the two single-tier shapes, whose title IS their tier's
    /// name, and false for the cog's, which shows several and names them
    /// where they begin. It is what decides whether the TAKE tier still
    /// gets a heading band: a panel headed TAKE over a heading reading TAKE
    /// wore its own name twice.
    fn title_names_its_tier(self) -> bool {
        self != SetupShape::Machine
    }
}

/// Every rectangle in the popup.
struct SetupLayout {
    panel: Rect,
    title: Rect,
    close: Rect,
    /// What the next take is called. It was a box in the band, at the top of a
    /// column that no longer exists; it is set once a session and belongs with
    /// the rest of what a take is.
    name: Rect,
    dest: Rect,
    reveal: Rect,
    folder: Rect,
    disk: Rect,
    count_in: Rect,
    time_sig: Rect,
    export: Rect,
    open_when_done: Rect,
    click_in_take: Rect,
    count_in_in_take: Rect,
    hide_elapsed: Rect,
    /// Hearing the live input, beside the setup it is about.
    hear_input: Rect,
    /// The way to Setup: the audio system, the devices, the rate, the buffer.
    audio: Rect,
    /// The cog's AUDIO heading. (MEDIA's is `project_head`.)
    audio_head: Rect,
    /// **Laid out OFF the panel in every shape now**: the cog shows no take
    /// tier, and the take and red-button panels wear their tier's name as the
    /// TITLE (`SetupShape::title_names_its_tier`). Kept so a shape can grow a
    /// heading back without re-deriving the band order.
    take_head: Rect,
    /// The AUDIO tier's own count-in and signature — the same settings as
    /// the take's copies, standing where the ARRANGEMENT's rules read as
    /// the arrangement's.
    audio_count_in: Rect,
    audio_sig: Rect,
    /// The AUDIO tier's two doors, on the same row's free half: the
    /// PROJECT dialog and the plugin folders — the owner's cog additions.
    project_row: Rect,
    plugin_folders: Rect,
    plugin_rescan: Rect,
    /// The STREAM door, on the count-in row's free half — the ReaStream send
    /// settings, moved here from the Project menu by the owner's word.
    stream_row: Rect,
    /// The MEDIA tier's heading - the cog's second tier - and its four folder
    /// rows: projects, exports, presets and impulse responses.
    project_head: Rect,
    proj_dest: Rect,
    proj_reveal: Rect,
    export_dest: Rect,
    export_reveal: Rect,
    preset_dest: Rect,
    preset_reveal: Rect,
    irs_dest: Rect,
    irs_reveal: Rect,
}

impl SetupLayout {
    const NONE: Self = Self {
        panel: Rect::NOTHING,
        title: Rect::NOTHING,
        close: Rect::NOTHING,
        name: Rect::NOTHING,
        dest: Rect::NOTHING,
        reveal: Rect::NOTHING,
        folder: Rect::NOTHING,
        disk: Rect::NOTHING,
        count_in: Rect::NOTHING,
        time_sig: Rect::NOTHING,
        export: Rect::NOTHING,
        open_when_done: Rect::NOTHING,
        hide_elapsed: Rect::NOTHING,
        click_in_take: Rect::NOTHING,
        count_in_in_take: Rect::NOTHING,
        hear_input: Rect::NOTHING,
        audio: Rect::NOTHING,
        audio_head: Rect::NOTHING,
        take_head: Rect::NOTHING,
        project_head: Rect::NOTHING,
        proj_dest: Rect::NOTHING,
        proj_reveal: Rect::NOTHING,
        export_dest: Rect::NOTHING,
        export_reveal: Rect::NOTHING,
        preset_dest: Rect::NOTHING,
        preset_reveal: Rect::NOTHING,
        irs_dest: Rect::NOTHING,
        irs_reveal: Rect::NOTHING,
        audio_count_in: Rect::NOTHING,
        audio_sig: Rect::NOTHING,
        project_row: Rect::NOTHING,
        plugin_folders: Rect::NOTHING,
        plugin_rescan: Rect::NOTHING,
        stream_row: Rect::NOTHING,
    };

    /// Hung off `anchor` - the control that opened this shape (see
    /// [`SetupShape`]) - and pulled back onto `screen`.
    ///
    /// Below and left-aligned with the cog when there is room, which is where a
    /// popup belongs relative to the thing that opened it. The clamp is not a
    /// nicety: the cog sits near the left of a band that can be half a screen
    /// wide, and a panel that ran off the bottom would put the ticks where
    /// nobody could reach them.
    fn new(screen: Rect, anchor: Rect, shape: SetupShape) -> Self {
        if !screen.is_positive() {
            return Self::NONE;
        }
        let w = (screen.width() * SETUP_W.0).clamp(SETUP_W.1, SETUP_W.2.min(screen.width()));
        // **Each shape is as tall as the rows it actually shows, AT THE SAME
        // ROW SIZE.** A take panel with the machine's height would be two
        // thirds empty, which reads as a panel that failed to load; a panel
        // whose height alone is scaled shrinks the rows with it, which is how
        // the red button's one row came out illegible. So the pitch is fixed
        // once from [`SETUP_REF_ROWS`] and each shape's height is solved from
        // its own row count against it.
        const HEAD: f32 = 0.55;
        const TITLE_FRAC: f32 = 0.13;
        // A heading is a little over half a row, and the trailing `+ 1.0` is
        // a row's worth of margin under the last control.
        let units_of = |heads: f32, rows: usize| HEAD * heads + rows as f32 + 1.0;
        let (heads, rows_shown) = match shape {
            SetupShape::Machine => (2.0, SETUP_MACHINE_ROWS),
            SetupShape::Take => (0.0, SETUP_TAKE_ROWS),
            SetupShape::Rolling => (0.0, SETUP_ROLLING_ROWS),
        };
        let ref_h = w * SETUP_ASPECT * SETUP_REF_ROWS as f32 / SETUP_ROWS as f32;
        let ref_pad = (ref_h * 0.055).clamp(4.0, 16.0);
        let ref_body = ref_h - 2.0 * ref_pad;
        let ref_title = ref_body * TITLE_FRAC;
        let ref_pitch = (ref_body - ref_title) / units_of(2.0, SETUP_REF_ROWS);
        // The title band is the SAME height in every shape, for the same
        // reason the rows are: it carries a word at a size, not a fraction.
        let h = (ref_title + ref_pitch * units_of(heads, rows_shown) + 2.0 * ref_pad)
            .min(screen.height() * 0.80);
        let mut panel = Rect::from_min_size(
            Pos2::new(anchor.left(), anchor.bottom() + h * 0.04),
            Vec2::new(w, h),
        );
        let dx = (screen.left() - panel.left()).max(0.0) - (panel.right() - screen.right()).max(0.0);
        let dy = (screen.top() - panel.top()).max(0.0) - (panel.bottom() - screen.bottom()).max(0.0);
        panel = panel.translate(Vec2::new(dx, dy));

        let pad = ref_pad;
        let body = panel.shrink(pad);
        if !body.is_positive() {
            return Self {
                panel,
                ..Self::NONE
            };
        }
        // The title keeps a band of its own at the top, with the way out at the
        // right of it. Everything else is rows of one pitch - a heading band
        // takes a little over half of one - because a dialog whose rows are all
        // different sizes reads as a form somebody assembled rather than as a
        // panel.
        // The reference title where there is room for it, and never more than
        // a third of a short panel - the clamp only bites when the screen has
        // squeezed the panel below the height solved for above.
        let title_h = ref_title.clamp(body.height() * TITLE_FRAC, body.height() * 0.45);
        let title = Rect::from_min_max(
            body.min,
            Pos2::new(body.right(), body.top() + title_h),
        );
        // Smaller than the band it sits in — a full-height OK block beside
        // the headline was the second half of the owner's "too loud".
        let close_w = (title.height() * 0.9).min(body.width() * 0.13);
        let close = Rect::from_center_size(
            Pos2::new(title.right() - close_w * 0.5, title.center().y),
            Vec2::new(close_w, title.height() * 0.62),
        );
        let rows = Rect::from_min_max(Pos2::new(body.left(), title.bottom()), body.max);
        // **AUDIO, then MEDIA, under the cog** - the owner's order - with the
        // TAKE tier on its own under the cassette and the red button's one
        // row on its own under it. Walked by a cursor, with a half-height
        // heading band opening each of the cog's two tiers. A real gap between
        // rows: `Rect::contains` is inclusive at both edges, so two rows that
        // merely touch share a line of pixels and every overlap test in this
        // file fails there.
        // Two headings for `Machine` - AUDIO and MEDIA - and none for the
        // other two, whose tier word is the panel's title. Hidden bands go to
        // `gone()` and take no units.
        let units = units_of(heads, rows_shown);
        // Read back from the rectangle rather than used as solved, so a panel
        // the screen squeezed still lays out. Unsqueezed it IS `ref_pitch`.
        let pitch = rows.height() / units;
        let mut y = rows.top();
        // **Where a band goes when this shape does not show it.**
        //
        // Far below the panel, which is what lets every draw call and the
        // whole hit-test table stay unconditional: the painter is clipped to
        // the panel, so nothing there is painted, and `setup_hit_test`
        // returns early for any point outside the panel, so nothing there is
        // reachable. A `Rect::NOTHING` would have done for the hit test and
        // not for the paint - it is an INVERTED rect, and what a painter does
        // with one is its own business rather than nothing.
        // **Each hidden band gets its OWN strip**, stacked downward, rather
        // than all of them sharing one rectangle. Sharing looked simpler and
        // broke the panel's oldest invariant: no two rows overlap. Every
        // hidden band sat exactly on top of every other, so
        // `the_take_settings_rows_do_not_overlap` failed on a pair that is
        // not on screen in either shape - a true failure of a test whose
        // whole job is to catch rows landing on each other.
        let mut y_gone = panel.bottom() + 10_000.0;
        let mut gone = || {
            let r = Rect::from_min_size(
                Pos2::new(rows.left(), y_gone),
                Vec2::new(rows.width(), pitch.max(1.0)),
            );
            y_gone += pitch.max(1.0) * 2.0;
            r
        };
        let mut band = |u: f32| {
            let r = Rect::from_min_max(
                Pos2::new(rows.left(), y + pitch * u * 0.10),
                Pos2::new(rows.right(), y + pitch * u * 0.92),
            );
            y += pitch * u;
            r
        };
        let machine = shape == SetupShape::Machine;
        let take = shape == SetupShape::Take;
        let rolling = shape == SetupShape::Rolling;
        let mut band = |u: f32, mine: bool| if mine { band(u) } else { gone() };
        let audio_head = band(HEAD, machine);
        let r6 = band(1.0, machine);
        let ra2 = band(1.0, machine);
        let ra3 = band(1.0, machine);
        // **The red button's one row.** It carries the AUDIO tier's own
        // count-in and signature - the same two rectangles, the same two
        // hits, moved onto a row of their own and given the whole width -
        // rather than the take tier's, which also carries EXPORT and is a
        // list of things about a take. That is what the owner met as "the
        // take options and NOT THE RECORDING OPTIONS".
        let rolling_row = band(1.0, rolling);
        // **MEDIA, second.** Where documents go and where exports land — two
        // questions that used to have no answer here at all, because both
        // folders were derived from the TAKE folder and moving one moved all
        // three. It sits directly under AUDIO because it answers the same
        // kind of question those rows do — where the app keeps things — and
        // the owner reads the panel top-down as machine, then storage, then
        // the take being made right now.
        let project_head = band(HEAD, machine);
        let rp1 = band(1.0, machine);
        let rp2 = band(1.0, machine);
        let rp3 = band(1.0, machine);
        let rp4 = band(1.0, machine);
        // **`Rolling` shows `rolling_row` alone** - the AUDIO tier's own
        // COUNT-IN and SIG on a row of their own - and no heading: the panel's
        // title already says RECORDING. Everything in the take tier is laid
        // out where nothing can see or reach it, exactly as the other shape's
        // rows are, so the red button's panel is two controls without being
        // a second copy of them.
        // **AUDIO and MEDIA are the cog's headings; the other two shapes
        // wear their tier's name as the panel's TITLE instead.** So this
        // band is laid out where nothing can see it for them - see
        // `SetupShape::title_word` - and the rows move up into the space.
        // The cog shows no take tier either, so the condition below is false
        // for every shape.
        let take_head = band(HEAD, (take || rolling) && !shape.title_names_its_tier());
        let r0 = band(1.0, take);
        let r1 = band(1.0, take);
        let note = {
            let r = band(1.0, take);
            Rect::from_center_size(r.center(), Vec2::new(r.width(), r.height() * 0.60))
        };
        let r3 = band(1.0, take);
        let r4 = band(1.0, take);
        let r5 = band(1.0, take);
        Self {
            panel,
            title,
            close,
            // What the take is called: the first thing about it, and the first
            // row here.
            name: r0,
            // Where takes go, and how to see it.
            //
            // **The folder gets the width the "Default" tick used to have.**
            // That tick set a flag nothing read: the folder was written to
            // settings and reused next launch whether it was on or off, so it
            // was a switch wired to itself, sitting on the one row that needed
            // the room. A path is long and it is the thing being read.
            dest: slice_h(r1, 0.00, 0.79),
            reveal: slice_h(r1, 0.82, 1.00),
            // What the next take will be called, and whether it will fit.
            // Answers rather than questions, so they carry no box — and a
            // HALF-HEIGHT row, because `text_line` sizes itself to whatever it
            // is given and a full row made the one line nobody clicks the
            // biggest thing in the panel.
            folder: slice_h(note, 0.00, 0.60),
            disk: slice_h(note, 0.62, 1.00),
            // How long a bar is, how many of them you get, and what comes out
            // at the end. The three that describe the take itself.
            count_in: slice_h(r3, 0.00, 0.30),
            time_sig: slice_h(r3, 0.33, 0.55),
            export: slice_h(r3, 0.58, 1.00),
            // The four questions with yes-or-no answers, in two rows of two, so
            // a tick is never the only thing on a line.
            open_when_done: slice_h(r4, 0.00, 0.49),
            click_in_take: slice_h(r4, 0.51, 1.00),
            count_in_in_take: slice_h(r5, 0.00, 0.49),
            hide_elapsed: slice_h(r5, 0.51, 1.00),
            // **The AUDIO tier's one row**: the audio system the sound
            // comes through, and whether you hear it. The monitor tick lost
            // its icon when the desk took the inputs, and this is the row
            // that is about the same subject.
            audio: slice_h(r6, 0.00, 0.60),
            hear_input: slice_h(r6, 0.63, 1.00),
            audio_head,
            take_head,
            // **On `ra2` under the cog and on a row of its own at the red
            // button**, where the pair has the width to itself. One pair of
            // rectangles either way, so there is still exactly one count-in
            // control in this panel and one signature control.
            audio_count_in: if rolling {
                slice_h(rolling_row, 0.00, 0.48)
            } else {
                slice_h(ra2, 0.00, 0.30)
            },
            audio_sig: if rolling {
                slice_h(rolling_row, 0.52, 1.00)
            } else {
                slice_h(ra2, 0.33, 0.55)
            },
            // A row of their own: crammed onto the count-in row's free
            // half they were the least legible thing on the card.
            project_row: slice_h(ra3, 0.00, 0.48),
            // Split the way the take's folder row is — a long answer and a
            // small square button off its right end — because it is the same
            // idiom: a thing, and one action on it.
            plugin_folders: slice_h(ra3, 0.52, 0.90),
            plugin_rescan: slice_h(ra3, 0.92, 1.00),
            // The AUDIO count-in row's free half, which the doors above
            // vacated for legibility; one door fits a half-row fine.
            stream_row: slice_h(ra2, 0.58, 1.00),
            project_head,
            // Laid out exactly as the take's folder row is, so the three
            // read as three answers to the same question.
            proj_dest: slice_h(rp1, 0.00, 0.79),
            proj_reveal: slice_h(rp1, 0.82, 1.00),
            export_dest: slice_h(rp2, 0.00, 0.79),
            export_reveal: slice_h(rp2, 0.82, 1.00),
            preset_dest: slice_h(rp3, 0.00, 0.79),
            preset_reveal: slice_h(rp3, 0.82, 1.00),
            irs_dest: slice_h(rp4, 0.00, 0.79),
            irs_reveal: slice_h(rp4, 0.82, 1.00),
        }
    }

    /// Every clickable region and what it means. The popup's [`Layout::targets`].
    fn targets(&self) -> [(Rect, Hit); 25] {
        [
            (self.audio_count_in, Hit::CycleCountIn),
            (self.audio_sig, Hit::EditTimeSignature),
            (self.project_row, Hit::ProjectMenu),
            (self.plugin_folders, Hit::PluginFolders),
            (self.plugin_rescan, Hit::RescanPlugins),
            (self.stream_row, Hit::StreamToObs),
            (self.name, Hit::NameField),
            (self.close, Hit::CloseSetup),
            (self.dest, Hit::ChooseFolder),
            (self.reveal, Hit::RevealFolder),
            (self.proj_dest, Hit::ChooseProjectFolder),
            (self.proj_reveal, Hit::RevealProjectFolder),
            (self.export_dest, Hit::ChooseExportFolder),
            (self.export_reveal, Hit::RevealExportFolder),
            (self.preset_dest, Hit::ChoosePresetFolder),
            (self.preset_reveal, Hit::RevealPresetFolder),
            (self.irs_dest, Hit::ChooseIrFolder),
            (self.irs_reveal, Hit::RevealIrFolder),
            (self.count_in, Hit::CycleCountIn),
            (self.time_sig, Hit::EditTimeSignature),
            (self.export, Hit::Export),
            (self.open_when_done, Hit::ToggleOpenWhenDone),
            (self.click_in_take, Hit::ToggleMetronomeInTake),
            (self.hear_input, Hit::ToggleInputMonitor),
            (self.audio, Hit::ShowAudioStatus),
        ]
    }
}

// ── the effect panels ───────────────────────────────────────────────────────
//
// A right-click on a knob opens the effect behind it. The knob itself is one
// number — how much — because during a take that is the only one anybody
// reaches for; everything that shapes the sound lives here, where it can be
// read and set once and left alone.
//
// One layout for all three, because they are the same shape: a title, four
// rows, a Reset. What differs is what the rows are CALLED and, for the delay's
// first row, that it steps through named divisions rather than sliding.

/// Which effect a panel belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fx {
    Reverb,
    Delay,
    Chorus,
    Phaser,
    Hpf,
    Lpf,
    Gain,
    Limiter,
}

impl Fx {
    /// **In drawn order: the top row, then the bottom row.** Three sends over
    /// three things that shape the whole output. Anything walking all six
    /// walks them in the order a hand finds them.
    pub const ALL: [Fx; 8] = [
        Fx::Reverb,
        Fx::Delay,
        Fx::Chorus,
        Fx::Phaser,
        Fx::Hpf,
        Fx::Lpf,
        Fx::Gain,
        Fx::Limiter,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Fx::Reverb => "REVERB",
            Fx::Delay => "DELAY",
            Fx::Chorus => "CHORUS",
            Fx::Phaser => "PHASER",
            Fx::Hpf => "HPF",
            Fx::Lpf => "LPF",
            Fx::Gain => "GAIN",
            Fx::Limiter => "LIMITER",
        }
    }

    /// A line under the title saying what the thing IS. Two of these name real
    /// hardware, because that is the fastest way to say what a sound is to
    /// somebody who has heard one.
    pub fn subtitle(self) -> &'static str {
        match self {
            Fx::Reverb => "legendary plate, after the EMT-140",
            Fx::Delay => "warbling saturation, after the RE-201 Space Echo",
            Fx::Chorus => "true stereo, after the Boss CE-1",
            Fx::Phaser => "four stages, after the MXR Phase 90",
            Fx::Hpf => "takes the bottom off, after everything",
            Fx::Lpf => "takes the top off - up is darker",
            Fx::Gain => "drive into the limiter - it CAN clip",
            Fx::Limiter => "true peak ceiling - no makeup",
        }
    }

    /// The knob this panel hangs off.
    pub fn hit(self) -> Hit {
        Hit::SetFx(self, 0.0)
    }

    /// Where this knob's own value lives in the settings file.
    ///
    /// The rows in [`Fx::rows`] are the effect's PARAMETERS; this is the knob
    /// itself. Named here for the same reason they are: it is the string the
    /// host answers to, so it cannot be spelled differently in two places.
    pub fn mix_key(self) -> &'static str {
        match self {
            Fx::Reverb => "reverb_mix",
            Fx::Delay => "delay_mix",
            Fx::Chorus => "chorus_mix",
            Fx::Phaser => "phaser_mix",
            Fx::Hpf => "hpf_mix",
            Fx::Lpf => "lpf_mix",
            Fx::Gain => "gain_mix",
            Fx::Limiter => "limiter_mix",
        }
    }

    /// Its position in [`Fx::ALL`], which is its cell in the layout.
    pub fn index(self) -> usize {
        Fx::ALL.iter().position(|&x| x == self).unwrap_or(0)
    }

    /// What the status line says while a hand is on it.
    pub fn describe(self) -> &'static str {
        match self {
            Fx::Reverb => "Reverb  -  shift right-click for its parameters",
            Fx::Delay => "Delay  -  shift right-click for its parameters",
            Fx::Chorus => "Chorus  -  shift right-click for its parameters",
            Fx::Phaser => "Phaser  -  shift right-click for its parameters",
            Fx::Hpf => "High-pass  -  shift right-click for its parameters",
            Fx::Lpf => "Low-pass  -  shift right-click for its parameters",
            Fx::Gain => "Output gain, into the limiter  -  it can clip",
            Fx::Limiter => "Limiter threshold  -  shift right-click for more",
        }
    }

    /// The colour of the knob's cap.
    ///
    /// **The bottom row is not the top row.** Reverb, delay and chorus are
    /// sends — things added to a sound. The filters and the limiter change
    /// what leaves, and the limiter is the one that will alter a take whether
    /// or not anybody is listening for it, so it is the one that is red.
    pub fn cap(self) -> Color32 {
        match self {
            Fx::Reverb | Fx::Delay | Fx::Chorus | Fx::Phaser => KNOB_CAP,
            Fx::Hpf | Fx::Lpf => FILTER_CAP,
            Fx::Gain => GAIN_CAP,
            Fx::Limiter => LIMITER_CAP,
        }
    }

    /// The same cap in the theme's colours, keeping the meaning the three
    /// stock colours carry.
    ///
    /// **The limiter stays out of it, and that is a rule rather than an
    /// oversight** — see `the_limiter_and_the_master_are_told_apart_by_colour`.
    /// A theme drives the sends and the filters; the one control that will
    /// alter a take whether or not anybody is listening keeps its own red, so
    /// no theme can ever paint it into the master's colour by accident.
    pub(crate) fn cap_themed(self, p: &Palette) -> Color32 {
        match self {
            Fx::Reverb | Fx::Delay | Fx::Chorus | Fx::Phaser => p.fx_knob,
            // The filters are the same family a step down, so the two rows
            // still read as two rows under any theme.
            Fx::Hpf | Fx::Lpf => shade(p.fx_knob, 0.72),
            // **The gain keeps its own amber for the same reason the limiter
            // keeps its red.** They are the pair that decides whether a take
            // clips, they sit next to each other, and a theme that painted
            // them alike would put the control that CAUSES clipping and the
            // control that PREVENTS it in one colour.
            Fx::Gain => GAIN_CAP,
            Fx::Limiter => LIMITER_CAP,
        }
    }

    /// The four rows, in order.
    ///
    /// **Keys, not indices.** They are what goes in the settings file and what
    /// the host reads back, so a row that is renamed or reordered here cannot
    /// silently start writing to a different parameter.
    /// `free` is whether the delay's time is on its FREE position, which is
    /// the only one with a rate to set. Every other effect ignores it.
    pub fn rows(self, free: bool) -> [FxRow; FX_ROWS] {
        const fn slide(key: &'static str, label: &'static str) -> FxRow {
            FxRow { key, label, step: false }
        }
        const fn step(key: &'static str, label: &'static str) -> FxRow {
            FxRow { key, label, step: true }
        }
        match self {
            // **The plate, after the EMT 140.** Decay is the machine's own
            // dial and reads in seconds; Dispersion is how much of the
            // steel's chirp is in circuit, which is the single most
            // characteristic thing about the unit and had nowhere to live
            // until the panel grew to six rows.
            Fx::Reverb => [
                // Mode FIRST, because it is what the five rows under it are
                // read against — the same place the echo's selector sits, for
                // the same reason.
                step("reverb_mode", "Mode"),
                slide("reverb_size", "Decay"),
                slide("reverb_damp", "Tone"),
                slide("plate_predelay", "Predelay"),
                slide("plate_dispersion", "Dispersion"),
                slide("reverb_width", "Width"),
            ],
            // **The tape echo, after the RE-201.** Mode is the twelve-way
            // selector — which of the three heads are live and whether the
            // spring is in — and it is the instrument rather than a setting.
            // Time is head ONE's; the other two follow it at 1 : 1.94 : 2.85,
            // which is where the machine's spacing came from.
            Fx::Delay => [
                step("echo_mode", "Mode"),
                step("delay_division", "Time"),
                slide("delay_feedback", "Intensity"),
                slide("delay_tone", "Tone"),
                slide("delay_width", "Spread"),
                // **Only on FREE, and LAST.** Every other position is a
                // musical length and takes its time from the tempo; this is
                // the one synced to nothing, and it was a fixed 375 ms with no
                // way to move it. Last so that appearing and disappearing
                // never moves the five rows above it — a panel that reflows
                // under the hand is a panel you mis-click.
                if free {
                    slide("delay_free_hz", "Rate")
                } else {
                    FxRow::NONE
                },
            ],
            Fx::Chorus => [
                slide("chorus_rate", "Rate"),
                slide("chorus_depth", "Depth"),
                slide("chorus_width", "Width"),
                slide("chorus_tone", "Tone"),
                FxRow::NONE,
                FxRow::NONE,
            ],
            // **The Phase 90 has ONE knob and it is Speed.** The other three
            // are the differences between the pedals in that family rather
            // than controls the original had: the number of stages is a
            // Phase 90 against a Phase 100, the feedback is the script logo
            // against the block, and the range is where in the spectrum the
            // notches sweep. Shipping defaults are the block-logo 90.
            Fx::Phaser => [
                slide("phaser_rate", "Speed"),
                slide("phaser_feedback", "Feedback"),
                step("phaser_stages", "Stages"),
                slide("phaser_range", "Range"),
                FxRow::NONE,
                FxRow::NONE,
            ],
            Fx::Hpf => [
                step("hpf_slope", "Slope"),
                slide("hpf_resonance", "Resonance"),
                FxRow::NONE,
                FxRow::NONE,
                FxRow::NONE,
                FxRow::NONE,
            ],
            Fx::Lpf => [
                step("lpf_slope", "Slope"),
                slide("lpf_resonance", "Resonance"),
                FxRow::NONE,
                FxRow::NONE,
                FxRow::NONE,
                FxRow::NONE,
            ],
            // **Nothing under the gain.** It is a number of decibels and
            // there is no parameter that changes what that means. A panel
            // with a title and a Reset in it would be a panel that punishes
            // the right-click somebody used on the seven knobs beside it.
            Fx::Gain => [FxRow::NONE; FX_ROWS],
            // **The circuit switch first.** The limiter ships OUT of circuit
            // (the owner's ruling, 2026-09-18) and this row is where it goes
            // back in; a dial turned while it is out is drawn dark and
            // applies nothing. See `TangentApp::limiter_disabled`.
            Fx::Limiter => [
                step("limiter_circuit", "Circuit"),
                slide("limiter_release", "Release"),
                slide("limiter_knee", "Knee"),
                FxRow::NONE,
                FxRow::NONE,
                FxRow::NONE,
            ],
        }
    }
}

/// How many parameter rows an effect's panel holds.
///
/// **Six, and it was four.** The plate and the tape echo each want five —
/// the plate's dispersion and the echo's head spread had nowhere to go and
/// sat at their defaults, unreachable. Six costs one array width and one
/// layout test; the alternative was trading a control away, and dispersion
/// is the single most characteristic thing about a plate.
pub const FX_ROWS: usize = 6;

/// One row of an effect's parameter panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FxRow {
    /// The settings key this row writes. Empty means the row is not there.
    pub key: &'static str,
    pub label: &'static str,
    /// Whether it steps through a named list rather than sliding along a
    /// track. There is no position along a track that means "a dotted eighth",
    /// and none that means "24 dB an octave" either.
    pub step: bool,
}

impl FxRow {
    /// A row that is not there. Panels have between two and four.
    pub const NONE: Self = Self {
        key: "",
        label: "",
        step: false,
    };
}

/// A row of an effect panel, and how it is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FxHit {
    /// Drag along the row to set `key` to 0..=1.
    Set { key: &'static str, value: f32 },
    /// A named parameter: open its list, so every value can be seen at once.
    ///
    /// **A popout, not a step.** Stepping was fine for a slope with three
    /// positions and useless for the Space Echo's twelve: finding mode nine
    /// meant pressing eight times past it and round again, reading one word at
    /// a time. A list you can see is a list you can choose from.
    OpenChoice { key: &'static str },
    /// One row of an open list.
    PickChoice { key: &'static str, index: usize },
    /// A press anywhere else while a list is open: put it away, and do
    /// nothing else with the press.
    CloseChoice,
    /// Put this effect back to what it shipped as.
    Reset(Fx),
    Close,
}

const FX_W: (f32, f32, f32) = (0.20, 250.0, 360.0);
const FX_ASPECT: f32 = 0.78;

/// Where everything in an effect panel goes.
struct FxLayout {
    panel: Rect,
    title: Rect,
    close: Rect,
    /// One per row of [`Fx::rows`], empty ones included so the indices line up.
    rows: [Rect; FX_ROWS],
    reset: Rect,
}

impl FxLayout {
    const NONE: Self = Self {
        panel: Rect::NOTHING,
        title: Rect::NOTHING,
        close: Rect::NOTHING,
        rows: [Rect::NOTHING; FX_ROWS],
        reset: Rect::NOTHING,
    };

    /// Hung off the knob that opened it, and clamped onto the screen.
    ///
    /// Below the knob when there is room and above it when there is not: the
    /// knobs sit in the top band, so "below" is nearly always right, and the
    /// clamp is what stops a panel running off the bottom of a short window.
    fn new(screen: Rect, anchor: Rect) -> Self {
        if !screen.is_positive() || !anchor.is_positive() {
            return Self::NONE;
        }
        // The floor bows to the surface — see `MetLayout::new`, where the
        // same `clamp(min, max)` panic is spelled out. It has always been
        // reachable here too.
        let w = (screen.width() * FX_W.0)
            .clamp(FX_W.1.min(screen.width()), FX_W.2.min(screen.width()));
        let h = (w * FX_ASPECT).min(screen.height() * 0.80);
        // Centred on the knob rather than left-aligned to it: a knob is thirty
        // points wide and a panel is three hundred, so hanging one off the
        // left edge of the other puts it visibly off to one side.
        let mut panel = Rect::from_min_size(
            Pos2::new(anchor.center().x - w * 0.5, anchor.bottom() + h * 0.05),
            Vec2::new(w, h),
        );
        let dx = (screen.left() - panel.left()).max(0.0) - (panel.right() - screen.right()).max(0.0);
        let dy = (screen.top() - panel.top()).max(0.0) - (panel.bottom() - screen.bottom()).max(0.0);
        panel = panel.translate(Vec2::new(dx, dy));

        let pad = (h * 0.06).clamp(4.0, 14.0);
        let body = panel.shrink(pad);
        if !body.is_positive() {
            return Self {
                panel,
                ..Self::NONE
            };
        }
        let title_h = body.height() * 0.20;
        let title = Rect::from_min_max(body.min, Pos2::new(body.right(), body.top() + title_h));
        // Square, at the top right of the title band. A word would need three
        // times the width and this panel does not have it — and an X in the
        // corner is the one piece of chrome nobody has to be taught.
        let close_w = (title.height() * 0.52).min(body.width() * 0.16);
        let close = Rect::from_min_size(
            Pos2::new(title.right() - close_w, title.top()),
            Vec2::splat(close_w),
        );
        // **Six rows and the Reset, evenly.** Equal heights because a panel
        // whose rows are all different sizes reads as a form somebody
        // assembled — and six because the plate and the tape echo each carry
        // five parameters; see `FX_ROWS`. An effect with fewer simply leaves
        // the tail of the column empty, which is what `FxRow::NONE` is for.
        let rest = Rect::from_min_max(Pos2::new(body.left(), title.bottom()), body.max);
        let pitch = rest.height() / (FX_ROWS as f32 + 1.0);
        let row = |i: usize| {
            Rect::from_min_size(
                Pos2::new(rest.left(), rest.top() + pitch * i as f32),
                Vec2::new(rest.width(), pitch * 0.86),
            )
        };
        Self {
            panel,
            title,
            close,
            rows: std::array::from_fn(row),
            // Right-aligned and narrow: it is the one destructive thing here
            // and it should not look like another row of settings.
            reset: {
                let r = row(FX_ROWS);
                Rect::from_min_max(
                    Pos2::new(r.right() - r.width() * 0.36, r.top()),
                    r.max,
                )
            },
        }
    }

    /// The draggable track inside a row: the part after the label.
    fn track(row: Rect) -> Rect {
        slice_h(row, FX_TRACK.0, FX_TRACK.1)
    }
}

/// Where a row's track starts and ends, as a share of the row.
///
/// The label takes the left and the reading takes the right, exactly as a
/// fader row does — so the two kinds of control in this app look like each
/// other rather than like two people's work.
const FX_TRACK: (f32, f32) = (0.40, 0.86);

// ── the varispeed range ─────────────────────────────────────────────────────
//
// A small panel, the same chrome as an effect's, with two typed fields in it
// instead of four tracks. **Typed and not dragged**: the ends of a range are
// numbers somebody has in mind — 85 and 115, or 50 and 200 — and there is
// nothing to sweep along between them worth the pixels a track would take.

/// Where everything in the range panel goes.
struct VariLayout {
    panel: Rect,
    title: Rect,
    close: Rect,
    /// Min, then Max.
    rows: [Rect; 2],
    reset: Rect,
}

impl VariLayout {
    const NONE: Self = Self {
        panel: Rect::NOTHING,
        title: Rect::NOTHING,
        close: Rect::NOTHING,
        rows: [Rect::NOTHING; 2],
        reset: Rect::NOTHING,
    };

    fn new(screen: Rect, anchor: Rect) -> Self {
        if !screen.is_positive() || !anchor.is_positive() {
            return Self::NONE;
        }
        // Narrower and shorter than an effect's: two rows, not four, and
        // nothing in them but a word and a number.
        let w = (screen.width() * 0.16).clamp(200.0, 300.0_f32.min(screen.width()));
        let h = (w * 0.62).min(screen.height() * 0.80);
        let mut panel = Rect::from_min_size(
            Pos2::new(anchor.center().x - w * 0.5, anchor.bottom() + h * 0.05),
            Vec2::new(w, h),
        );
        let dx = (screen.left() - panel.left()).max(0.0) - (panel.right() - screen.right()).max(0.0);
        let dy = (screen.top() - panel.top()).max(0.0) - (panel.bottom() - screen.bottom()).max(0.0);
        panel = panel.translate(Vec2::new(dx, dy));
        let pad = (h * 0.08).clamp(4.0, 14.0);
        let body = panel.shrink(pad);
        if !body.is_positive() {
            return Self { panel, ..Self::NONE };
        }
        let title_h = body.height() * 0.30;
        let title = Rect::from_min_max(body.min, Pos2::new(body.right(), body.top() + title_h));
        let close_w = (title.height() * 0.42).min(body.width() * 0.16);
        let close = Rect::from_min_size(
            Pos2::new(title.right() - close_w, title.top()),
            Vec2::splat(close_w),
        );
        let rest = Rect::from_min_max(Pos2::new(body.left(), title.bottom()), body.max);
        let pitch = rest.height() / 3.0;
        let row = |i: usize| {
            Rect::from_min_size(
                Pos2::new(rest.left(), rest.top() + pitch * i as f32),
                Vec2::new(rest.width(), pitch * 0.86),
            )
        };
        Self {
            panel,
            title,
            close,
            rows: [row(0), row(1)],
            reset: {
                let r = row(2);
                Rect::from_min_max(Pos2::new(r.right() - r.width() * 0.40, r.top()), r.max)
            },
        }
    }

    /// Where a row's number sits — the part a press opens for typing.
    fn field(row: Rect) -> Rect {
        slice_h(row, 0.52, 1.0)
    }
}

// ───────────────────────────────────────────────────────────────────────────
// The metronome's panel
// ───────────────────────────────────────────────────────────────────────────

/// The metronome panel's proportions. See [`draw_met`].
const MET_W: (f32, f32, f32) = (0.22, 260.0, 380.0);
/// Taller since count-in arrived: two more rows out of five, over a panel
/// that is mostly rows.
const MET_ASPECT: f32 = 1.34;
/// Where a row's control starts and ends, as a fraction of the row.
const MET_TRACK: (f32, f32) = (0.32, 0.98);
/// TEMPO, BEATS, UNIT, CLICK, SOUND, COUNT-IN, IN-TAKE.
///
/// **Count-in joined the metronome** on the owner's call: "move the relevant
/// timing/met controls (countin and time sig) to the metronome icon's right
/// click menu". The signature was already here as the BEATS and UNIT rows -
/// a time signature IS how many beats and what kind - so what actually
/// moved is the count, which had been living in the Setup panel two tiers
/// away from the thing it counts.
const MET_ROWS: usize = 7;

/// **The metronome's right-click panel, built like the dials' own.**
///
/// It began as an egui dialog and read as a foreign object: egui's stock style
/// under this crate's ink, its own idea of spacing, and a window that centred
/// like a modal rather than hanging off the button — the owner's "it's cut off
/// or looks wonky". The desk already has a language for "a right-click opened
/// a small panel about this control", in `draw_fx` and `draw_vari_range`, and
/// this is that language. Themed by construction, because every colour comes
/// from `palette(s)` and nothing here asks egui for a style.
struct MetLayout {
    panel: Rect,
    title: Rect,
    close: Rect,
    rows: [Rect; MET_ROWS],
}

impl MetLayout {
    const NONE: Self = Self {
        panel: Rect::NOTHING,
        title: Rect::NOTHING,
        close: Rect::NOTHING,
        rows: [Rect::NOTHING; MET_ROWS],
    };

    fn new(screen: Rect, anchor: Rect) -> Self {
        if !screen.is_positive() {
            return Self::NONE;
        }
        // **The floor bows to the surface.** `clamp(min, max)` PANICS when
        // min > max, and a window narrower than the panel's own minimum makes
        // exactly that: 260 against a 30-point screen. Rare, and reachable —
        // a plugin editor's inline surface is small, and the first frame of a
        // resize can be anything.
        let w = (screen.width() * MET_W.0)
            .clamp(MET_W.1.min(screen.width()), MET_W.2.min(screen.width()));
        let h = (w * MET_ASPECT).min(screen.height() * 0.85);
        // **A panel SUMMONED at a point is centred on it; one opened from a
        // button hangs off the button.**
        //
        // Centred on the button horizontally and clear of it below - the
        // same placement the effect panels use - which is right for the
        // metronome chip and wrong for `Cmd+M`, whose anchor is the middle
        // of the surface. That put the panel across the middle and BELOW it:
        // the owner's "horizontally but not vert centered ... needs both to
        // be in the middle of the screen". An anchor with no area is not a
        // thing on screen to hang off, so it is a place to BE.
        let mut panel = if anchor.width() <= 1.0 && anchor.height() <= 1.0 {
            Rect::from_center_size(anchor.center(), Vec2::new(w, h))
        } else {
            Rect::from_min_size(
                Pos2::new(anchor.center().x - w * 0.5, anchor.bottom() + h * 0.04),
                Vec2::new(w, h),
            )
        };
        let dx = (screen.left() - panel.left()).max(0.0) - (panel.right() - screen.right()).max(0.0);
        let dy = (screen.top() - panel.top()).max(0.0) - (panel.bottom() - screen.bottom()).max(0.0);
        panel = panel.translate(Vec2::new(dx, dy));

        let pad = (h * 0.05).clamp(4.0, 14.0);
        let body = panel.shrink(pad);
        if !body.is_positive() {
            return Self { panel, ..Self::NONE };
        }
        let title_h = body.height() * 0.17;
        let title = Rect::from_min_max(body.min, Pos2::new(body.right(), body.top() + title_h));
        let close_w = (title.height() * 0.52).min(body.width() * 0.16);
        let close = Rect::from_min_size(
            Pos2::new(title.right() - close_w, title.top()),
            Vec2::splat(close_w),
        );
        let rest = Rect::from_min_max(Pos2::new(body.left(), title.bottom()), body.max);
        let pitch = rest.height() / MET_ROWS as f32;
        let row = |i: usize| {
            Rect::from_min_size(
                Pos2::new(rest.left(), rest.top() + pitch * i as f32),
                Vec2::new(rest.width(), pitch * 0.84),
            )
        };
        Self {
            panel,
            title,
            close,
            rows: std::array::from_fn(row),
        }
    }

    /// A row's control area: everything right of its label.
    fn track(row: Rect) -> Rect {
        slice_h(row, MET_TRACK.0, MET_TRACK.1)
    }

    /// `n` equal boxes across a row's control area.
    fn cells(row: Rect, n: usize) -> Vec<Rect> {
        let t = Self::track(row);
        if n == 0 || !t.is_positive() {
            return Vec::new();
        }
        let gap = (t.width() * 0.025).min(4.0);
        let each = (t.width() - gap * (n - 1) as f32) / n as f32;
        if each <= 0.0 {
            return Vec::new();
        }
        (0..n)
            .map(|i| {
                Rect::from_min_size(
                    Pos2::new(t.left() + (each + gap) * i as f32, t.top() + t.height() * 0.12),
                    Vec2::new(each, t.height() * 0.76),
                )
            })
            .collect()
    }
}

/// The click divisions the panel offers, and what each is called.
///
/// In beats of the SIGNATURE, because the beat is whatever the signature says:
/// in 6/8 it is an eighth. Zero is once a bar, which is the compound-meter
/// answer — 6/8 is felt in two, and six clicks a bar is unusable in the meter
/// that wants one most.
pub const MET_DIVS: [(i64, &str); 5] =
    [(0, "bar"), (1, "beat"), (2, "1/2"), (3, "1/3"), (4, "1/4")];

/// The bottom numbers a signature may have. Buttons rather than a field,
/// because the bottom number is a power of two and a field would invite 3.
pub const MET_UNITS: [u8; 4] = [2, 4, 8, 16];
/// The metronome panel's TEMPO row inks: the accent fill and the ink handle,
/// or both faded into the band while the tempo is held - the TAP chip's dim.
fn met_tempo_inks(held: bool, p: &Palette) -> (Color32, Color32) {
    if held {
        (toward(p.accent, p.bg, 0.55), toward(p.ink, p.bg, 0.55))
    } else {
        (p.accent, p.ink)
    }
}


/// What the metronome panel is showing.
#[derive(Debug, Clone, Copy)]
pub struct MetView<'a> {
    pub bpm: f64,
    pub beats: u8,
    pub unit: u8,
    pub div: i64,
    /// The chosen click file, or empty for the one that ships.
    pub sample: &'a str,
    /// Count-in length, in BARS at this signature. Zero is none.
    pub count_in_bars: i64,
    /// Whether the count-in keeps clicking once the take has started.
    pub count_in_in_take: bool,
    /// **The tempo is held** (a take's count-in, an export): the TEMPO row is
    /// drawn dim, and the app refuses it - the band's knob's rule, on the
    /// other road to the same number. See `RecorderState::tempo_held`.
    pub held: bool,
}

/// What a press inside the metronome panel means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MetHit {
    /// The tempo track, as a fraction along it. Dragged, like a fader.
    Tempo(f32),
    /// One beat fewer, or one more.
    Beats(i8),
    /// One of [`MET_UNITS`].
    Unit(u8),
    /// One of [`MET_DIVS`].
    Div(i64),
    /// One count-in bar fewer, or one more.
    CountIn(i8),
    /// Whether the count-in keeps clicking into the take.
    CountInInTake,
    /// Choose a click sound.
    SoundPick,
    /// Back to the one that ships.
    SoundReset,
    Close,
}

/// Where the panel goes, for a caller that has to swallow presses.
pub fn met_popup_rect(screen: Rect, anchor: Rect) -> Rect {
    MetLayout::new(screen, anchor).panel
}

/// The tempo row's track, for a DRAG that has already taken its grab and must
/// keep answering from the geometry rather than from what is under the hand.
pub fn met_tempo_track(screen: Rect, anchor: Rect) -> Rect {
    MetLayout::track(MetLayout::new(screen, anchor).rows[0])
}

/// What is under `pos`. `None` is the panel's own chrome, which the caller
/// still swallows — a press inside a panel never falls through to the app.
pub fn met_hit_test(screen: Rect, anchor: Rect, pos: Pos2) -> Option<MetHit> {
    let l = MetLayout::new(screen, anchor);
    if !l.panel.contains(pos) {
        return None;
    }
    if l.close.contains(pos) {
        return Some(MetHit::Close);
    }
    // TEMPO: anywhere along the row, as a fraction of the track. The whole
    // ROW answers, not just the well, so a hand a few points high still moves
    // it — a two-point target is not a control.
    let track = MetLayout::track(l.rows[0]);
    if l.rows[0].contains(pos) && track.is_positive() {
        let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
        return Some(MetHit::Tempo(t));
    }
    // BEATS: minus, the count, plus. The count is a READOUT — pressing it
    // does nothing rather than doing something arbitrary.
    let beats = MetLayout::cells(l.rows[1], 3);
    if beats.first().is_some_and(|c| c.contains(pos)) {
        return Some(MetHit::Beats(-1));
    }
    if beats.get(2).is_some_and(|c| c.contains(pos)) {
        return Some(MetHit::Beats(1));
    }
    for (cell, unit) in MetLayout::cells(l.rows[2], MET_UNITS.len()).iter().zip(MET_UNITS) {
        if cell.contains(pos) {
            return Some(MetHit::Unit(unit));
        }
    }
    for (cell, (d, _)) in MetLayout::cells(l.rows[3], MET_DIVS.len()).iter().zip(MET_DIVS) {
        if cell.contains(pos) {
            return Some(MetHit::Div(d));
        }
    }
    // **DEFAULT first, CUSTOM second**, which is the owner's order and the
    // one that reads. The row used to be the other way round and its two
    // words were "built-in" and "default" — the same thing said twice, so
    // there was no way to tell which one was the click you were on and which
    // was the way back to it.
    let sound = MetLayout::cells(l.rows[4], 2);
    if sound.first().is_some_and(|c| c.contains(pos)) {
        return Some(MetHit::SoundReset);
    }
    if sound.get(1).is_some_and(|c| c.contains(pos)) {
        return Some(MetHit::SoundPick);
    }
    // COUNT-IN: minus, the count, plus — the BEATS row's own shape, because
    // it is the same kind of question and a hand should not have to learn a
    // second way to step a number in one panel.
    let count = MetLayout::cells(l.rows[5], 3);
    if count.first().is_some_and(|c| c.contains(pos)) {
        return Some(MetHit::CountIn(-1));
    }
    if count.get(2).is_some_and(|c| c.contains(pos)) {
        return Some(MetHit::CountIn(1));
    }
    if l.rows[6].contains(pos) {
        return Some(MetHit::CountInInTake);
    }
    None
}

/// Draw the metronome panel. See [`MetLayout`] for why it is painted rather
/// than built out of egui widgets.
pub fn draw_met(
    painter: &Painter,
    screen: Rect,
    anchor: Rect,
    v: MetView<'_>,
    s: &Settings,
) {
    let l = MetLayout::new(screen, anchor);
    if !l.panel.is_positive() {
        return;
    }
    let p = palette(s);
    // The same scrim the effect panels use: it is what says the panel is in
    // FRONT of the window rather than being another band that appeared.
    painter.rect_filled(screen, 0.0, Color32::from_black_alpha(96));
    painter.rect_filled(l.panel, 4.0, p.bg);
    painter.rect_stroke(l.panel, 4.0, Stroke::new(1.0_f32, p.ink), StrokeKind::Inside);

    if l.title.is_positive() {
        let size = fit_text(l.title, "METRONOME", l.title.height() * 0.42);
        if size >= MIN_TEXT {
            painter.text(
                Pos2::new(l.title.left(), l.title.top() + l.title.height() * 0.30),
                Align2::LEFT_CENTER,
                "METRONOME",
                font_light(size),
                p.ink,
            );
        }
        let sub = tempo_text(v.bpm);
        let sub = format!("{sub} bpm, {}/{}", v.beats.max(1), v.unit.max(1));
        let size = fit_text(l.title, &sub, l.title.height() * 0.26);
        if size >= MIN_TEXT {
            painter.text(
                Pos2::new(l.title.left(), l.title.top() + l.title.height() * 0.74),
                Align2::LEFT_CENTER,
                &sub,
                font_light(size),
                p.faint,
            );
        }
        draw_close_x(painter, l.close, &p);
    }

    let label = |row: Rect, text: &str| {
        let size = fit_text(slice_h(row, 0.0, MET_TRACK.0), text, row.height() * 0.46);
        if size >= MIN_TEXT {
            painter.text(
                Pos2::new(row.left(), row.center().y),
                Align2::LEFT_CENTER,
                text,
                font_light(size),
                p.ink,
            );
        }
    };
    // A box with a word in it, lit when it is the one that is set. The lit
    // fill is the accent and the ink goes to the panel's ground, because an
    // accent-on-accent word is the one thing this palette cannot do.
    let cell = |r: Rect, text: &str, on: bool| {
        painter.rect_filled(r, 2.0, if on { p.accent } else { p.field });
        painter.rect_stroke(r, 2.0, Stroke::new(1.0_f32, p.line), StrokeKind::Inside);
        let size = fit_text(r.shrink(r.width() * 0.08), text, r.height() * 0.56);
        if size >= MIN_TEXT {
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                text,
                font_light(size),
                if on { p.bg } else { p.ink },
            );
        }
    };

    // ── TEMPO: a well with the set part filled, exactly as a fader's is ──
    let row = l.rows[0];
    if row.is_positive() {
        label(row, "TEMPO");
        let track = MetLayout::track(row);
        let span = (MAX_BPM - MIN_BPM).max(1.0);
        let t = (((v.bpm - MIN_BPM) / span) as f32).clamp(0.0, 1.0);
        let h = (row.height() * 0.22).max(2.0);
        let well = Rect::from_center_size(track.center(), Vec2::new(track.width(), h));
        painter.rect_filled(well, h * 0.5, p.well);
        let (fill, handle) = met_tempo_inks(v.held, &p);
        if t > 0.0 {
            painter.rect_filled(
                Rect::from_min_size(well.min, Vec2::new(well.width() * t, h)),
                h * 0.5,
                fill,
            );
        }
        let knob_w = (row.height() * 0.20).max(3.0);
        let x = well.left() + well.width() * t;
        painter.rect_filled(
            Rect::from_center_size(
                Pos2::new(x.clamp(well.left() + knob_w * 0.5, well.right() - knob_w * 0.5), well.center().y),
                Vec2::new(knob_w, row.height() * 0.62),
            ),
            1.5,
            handle,
        );
    }

    // ── BEATS: minus, the count, plus ──
    let row = l.rows[1];
    if row.is_positive() {
        label(row, "BEATS");
        let cells = MetLayout::cells(row, 3);
        if let [minus, count, plus] = cells.as_slice() {
            cell(*minus, "-", false);
            cell(*count, &format!("{}", v.beats.max(1)), false);
            cell(*plus, "+", false);
        }
    }

    // ── UNIT: what gets the beat ──
    let row = l.rows[2];
    if row.is_positive() {
        label(row, "NOTE");
        for (r, unit) in MetLayout::cells(row, MET_UNITS.len()).iter().zip(MET_UNITS) {
            cell(*r, &format!("{unit}"), v.unit == unit);
        }
    }

    // ── CLICK: how often it sounds ──
    let row = l.rows[3];
    if row.is_positive() {
        label(row, "CLICK");
        for (r, (d, word)) in MetLayout::cells(row, MET_DIVS.len()).iter().zip(MET_DIVS) {
            cell(*r, word, v.div == d);
        }
    }

    // ── SOUND: a file, and always a way back to the one that ships ──
    let row = l.rows[4];
    if row.is_positive() {
        label(row, "SOUND");
        let cells = MetLayout::cells(row, 2);
        if let [reset, pick] = cells.as_slice() {
            // **Two words that are not the same word.** This row used to read
            // "built-in | default" with nothing chosen — one control naming
            // the click you were on and one naming the click you would go
            // back to, which are the same click, so neither told you what
            // pressing it would do.
            //
            // Default is the one that ships. Custom raises the file panel,
            // and once something is chosen it wears that file's own NAME
            // rather than the word — a panel this wide cannot show a path,
            // a truncated path names nothing, and the name is the only part
            // anybody recognises. The lit one is the click that is playing.
            let name = if v.sample.is_empty() {
                "custom".to_owned()
            } else {
                std::path::Path::new(v.sample)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| v.sample.to_owned())
            };
            cell(*reset, "default", v.sample.is_empty());
            cell(*pick, &name, !v.sample.is_empty());
        }
    }
    // ── COUNT-IN: minus, the count, plus ──
    //
    // **The BEATS row's own shape**, deliberately: it is the same kind of
    // question, and a hand should not have to learn a second way to step a
    // number inside one panel.
    let row = l.rows[5];
    if row.is_positive() {
        label(row, "COUNT");
        let cells = MetLayout::cells(row, 3);
        if let [minus, count, plus] = cells.as_slice() {
            cell(*minus, "-", false);
            // **Bars, and it says so.** "2" alone in a metronome panel could
            // be two of anything; in 6/8 two bars is twelve clicks and the
            // difference matters. Zero is spelled "off" rather than "0 bars",
            // because none is a state and not a quantity.
            let word = match v.count_in_bars.max(0) {
                0 => "off".to_owned(),
                1 => "1 bar".to_owned(),
                n => format!("{n} bars"),
            };
            cell(*count, &word, false);
            cell(*plus, "+", false);
        }
    }

    // ── IN TAKE: is the count-in recorded at the head of the take ──
    let row = l.rows[6];
    if row.is_positive() {
        label(row, "IN TAKE");
        let cells = MetLayout::cells(row, 2);
        if let [off, on] = cells.as_slice() {
            cell(*off, "stops", !v.count_in_in_take);
            cell(*on, "carries on", v.count_in_in_take);
        }
    }

}

/// What a press inside the varispeed range panel means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariHit {
    /// Open one end for typing.
    Type(NumField),
    /// Back to 85 and 115.
    Reset,
    Close,
}

/// Where the range panel goes, for a caller that has to swallow presses.
pub fn vari_popup_rect(screen: Rect, anchor: Rect) -> Rect {
    VariLayout::new(screen, anchor).panel
}

/// What is under `pos` in the range panel. `None` is its own chrome, which
/// the caller still swallows.
pub fn vari_hit_test(screen: Rect, anchor: Rect, pos: Pos2) -> Option<VariHit> {
    let l = VariLayout::new(screen, anchor);
    if !l.panel.contains(pos) {
        return None;
    }
    if l.close.contains(pos) {
        return Some(VariHit::Close);
    }
    if l.reset.contains(pos) {
        return Some(VariHit::Reset);
    }
    for (row, field) in l.rows.into_iter().zip([NumField::VariMin, NumField::VariMax]) {
        if row.contains(pos) {
            return Some(VariHit::Type(field));
        }
    }
    None
}

/// The range panel.
pub fn draw_vari_range(
    painter: &Painter,
    screen: Rect,
    anchor: Rect,
    v: crate::varispeed::Varispeed,
    typing: Option<&crate::recorder::NumEdit>,
    s: &Settings,
) {
    use crate::varispeed::{HARD_MAX, HARD_MIN};
    let l = VariLayout::new(screen, anchor);
    if !l.panel.is_positive() {
        return;
    }
    let p = palette(s);
    painter.rect_filled(screen, 0.0, Color32::from_black_alpha(96));
    painter.rect_filled(l.panel, 4.0, p.bg);
    painter.rect_stroke(l.panel, 4.0, Stroke::new(1.0_f32, p.ink), StrokeKind::Inside);
    if l.title.is_positive() {
        let size = fit_text(l.title, "VARISPEED RANGE", l.title.height() * 0.34);
        if size >= MIN_TEXT {
            painter.text(
                Pos2::new(l.title.left(), l.title.top() + l.title.height() * 0.30),
                Align2::LEFT_CENTER,
                "VARISPEED RANGE",
                font_light(size),
                p.ink,
            );
        }
        // The hard limits, said out loud rather than discovered by typing a
        // number and watching it change to something else.
        let sub = format!("percent of normal speed, {HARD_MIN} to {HARD_MAX}");
        let size = fit_text(l.title, &sub, l.title.height() * 0.22);
        if size >= MIN_TEXT {
            painter.text(
                Pos2::new(l.title.left(), l.title.top() + l.title.height() * 0.74),
                Align2::LEFT_CENTER,
                &sub,
                font_light(size),
                p.faint,
            );
        }
        draw_close_x(painter, l.close, &p);
    }
    for (row, (field, label, value)) in l.rows.into_iter().zip([
        (NumField::VariMin, "Min", v.min()),
        (NumField::VariMax, "Max", v.max()),
    ]) {
        if !row.is_positive() {
            continue;
        }
        let size = fit_text(slice_h(row, 0.0, 0.5), label, row.height() * 0.52);
        if size >= MIN_TEXT {
            painter.text(
                Pos2::new(row.left(), row.center().y),
                Align2::LEFT_CENTER,
                label,
                font_light(size),
                p.ink,
            );
        }
        let field_r = VariLayout::field(row);
        let well = surface_of(p.field);
        painter.rect_filled(field_r, 2.0, well.bg);
        let editing = typing.filter(|e| e.field == field);
        let text = match editing {
            Some(e) => format!("{}|", e.text),
            None => format!("{value}"),
        };
        let size = fit_text(field_r.shrink(4.0), &text, row.height() * 0.56);
        if size >= MIN_TEXT {
            painter.text(
                Pos2::new(field_r.right() - 4.0, field_r.center().y),
                Align2::RIGHT_CENTER,
                &text,
                font_light(size),
                well.ink,
            );
        }
    }
    draw_word_button_sized(painter, l.reset, &["RESET"], 0.34, &p);
}

/// Where an effect panel goes, for a caller that has to swallow presses.
pub fn fx_popup_rect(screen: Rect, anchor: Rect) -> Rect {
    FxLayout::new(screen, anchor).panel
}

/// What a press inside an effect panel means.
///
/// `None` for a press on the panel's own chrome, which the caller still
/// swallows: a panel you can click through is one that closes when you meant
/// to press something in it.
pub fn fx_hit_test(screen: Rect, anchor: Rect, fx: Fx, pos: Pos2,
    // Which list is open, and how long it is. `None` when none is. The key is
    // `'static` because it comes from `Fx::rows`, and the hit it produces is
    // handed straight back to the caller as one of those same keys.
    open: Option<(&'static str, usize)>,
    // Whether the delay's time is on FREE, which is the one position with a
    // rate row under it. See `Fx::rows`.
    free: bool,
) -> Option<FxHit> {
    let l = FxLayout::new(screen, anchor);
    // **An open list is hit-tested BEFORE the panel under it**, and a press
    // outside it closes it rather than falling through to whatever is beneath.
    // The mixer's insert menu works the same way, for the same reason: a menu
    // that lets presses through is a menu that does two things at once.
    if let Some((key, n)) = open {
        for (k, r) in fx_choice_list(screen, anchor, fx, key, n).into_iter().enumerate() {
            if r.contains(pos) {
                return Some(FxHit::PickChoice { key, index: k });
            }
        }
        return Some(FxHit::CloseChoice);
    }
    if !l.panel.contains(pos) {
        return None;
    }
    if l.close.contains(pos) {
        return Some(FxHit::Close);
    }
    if l.reset.contains(pos) {
        return Some(FxHit::Reset(fx));
    }
    for (i, row) in fx.rows(free).into_iter().enumerate() {
        if row.key.is_empty() || !l.rows[i].contains(pos) {
            continue;
        }
        if row.step {
            return Some(FxHit::OpenChoice { key: row.key });
        }
        let track = FxLayout::track(l.rows[i]);
        return Some(FxHit::Set {
            key: row.key,
            value: along(track, pos),
        });
    }
    None
}

/// Where an open choice list stands, and where each of its rows is.
///
/// **One function for the painter and the hit test**, which is the rule this
/// file keeps: a menu drawn in one place and tested in another is a menu whose
/// rows do the wrong thing, and nothing warns.
///
/// It hangs off the row's own box and is clamped inside the panel, flipping
/// above the box when there is not room below — the same rule the band's
/// menus follow.
pub fn fx_choice_list(screen: Rect, anchor: Rect, fx: Fx, key: &str, n: usize) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }
    let l = FxLayout::new(screen, anchor);
    // The superset: the row this looks for is a STEP row, and no step row
    // depends on the free position.
    let Some(i) = fx.rows(true).iter().position(|r| r.key == key && r.step) else {
        return Vec::new();
    };
    let row = l.rows[i];
    if !row.is_positive() || !l.panel.is_positive() {
        return Vec::new();
    }
    let track = FxLayout::track(row);
    let box_r = Rect::from_min_max(
        Pos2::new(track.left(), row.top() + row.height() * 0.14),
        Pos2::new(row.right(), row.bottom() - row.height() * 0.14),
    );
    let h = (row.height() * 0.86).clamp(11.0, 20.0);
    let total = h * n as f32 + 4.0;
    // Below if it fits, above if it does not — and pinned inside the panel
    // either way, because a list that runs off the panel is rows nobody can
    // reach.
    let top = if box_r.bottom() + total <= l.panel.bottom() - 2.0 {
        box_r.bottom() + 2.0
    } else {
        (box_r.top() - 2.0 - total).max(l.panel.top() + 2.0)
    };
    (0..n)
        .map(|k| {
            Rect::from_min_max(
                Pos2::new(box_r.left(), top + 2.0 + h * k as f32),
                Pos2::new(box_r.right(), top + 2.0 + h * (k + 1) as f32),
            )
        })
        .collect()
}

/// Which row of `fx` a point is on, for a caller continuing a drag.
///
/// A drag inside a panel has to keep setting the row it STARTED on, even once
/// the pointer has slid onto the row above — the same reason `is_same_control`
/// exists for the band.
pub fn fx_row_at(screen: Rect, anchor: Rect, fx: Fx, pos: Pos2) -> Option<&'static str> {
    // A stepped row is a row. The question this answers is "which parameter is
    // under the pointer", and answering `None` over the Slope row would say
    // there is nothing there.
    match fx_hit_test(screen, anchor, fx, pos, None, true)? {
        FxHit::Set { key, .. } | FxHit::OpenChoice { key } => Some(key),
        _ => None,
    }
}

/// The value a point along `key`'s row would set, wherever the pointer is.
///
/// Clamped into the track rather than refused, so a hand that slides off the
/// end of a row pins the value instead of dropping the gesture.
pub fn fx_value_at(screen: Rect, anchor: Rect, fx: Fx, key: &str, pos: Pos2) -> Option<f32> {
    let l = FxLayout::new(screen, anchor);
    // The superset — the only row that comes and goes is the LAST one, so
    // every index is the same either way.
    let i = fx.rows(true).iter().position(|r| r.key == key)?;
    let track = FxLayout::track(l.rows[i]);
    track.is_positive().then(|| along(track, pos))
}

/// Where the setup panel goes, given the window, the control it hangs off and
/// the shape.
///
/// [`Rect::NOTHING`] when there is no room for it. The app needs this to know
/// whether a press landed inside the popup or outside it, which is the whole of
/// "click away to dismiss".
pub fn setup_popup_rect(screen: Rect, anchor: Rect, shape: SetupShape) -> Rect {
    SetupLayout::new(screen, anchor, shape).panel
}

/// What a press inside the popup means, or `None` for a press on its chrome.
///
/// A press anywhere in the panel that is not a control returns `None` and is
/// still SWALLOWED by the caller — a dialog you can click through is one that
/// closes when you meant to press something in it.
pub fn setup_hit_test(
    screen: Rect,
    anchor: Rect,
    view: &RecorderView<'_>,
    pos: Pos2,
    shape: SetupShape,
) -> Option<Hit> {
    let l = SetupLayout::new(screen, anchor, shape);
    if !l.panel.contains(pos) {
        return None;
    }
    let rolling = view.state.is_active();
    l.targets()
        .into_iter()
        // Nothing here is set MID-TAKE. Every one of these decides how the
        // file being written right now was going to be written, and a picker
        // that swaps the camera under a running encoder is not a setting, it
        // is a crash with a caption.
        .find(|(r, _)| !rolling && r.contains(pos))
        .map(|(_, h)| h)
        .or_else(|| l.close.contains(pos).then_some(Hit::CloseSetup))
        .or({
            // Extra rows that are only reachable here.
            let extra = [
                (l.count_in_in_take, Hit::ToggleCountInInTake),
                (l.hide_elapsed, Hit::ToggleHideElapsed),
            ];
            extra
                .into_iter()
                .find(|(r, _)| r.contains(pos))
                .map(|(_, h)| h)
        })
}

/// Draw an effect's panel over `screen`, hung off the knob at `anchor`.
///
/// `values` answers what each key is set to, and `division` is the delay's
/// time. Both come from the caller because this crate does not own the
/// settings file — the same reason every other painter here takes a view.
pub fn draw_fx(
    painter: &Painter,
    screen: Rect,
    anchor: Rect,
    fx: Fx,
    values: &dyn Fn(&str) -> f32,
    // `choice`: the label a stepped row shows, for its key. See `FxRow::step`.
    choice: &dyn Fn(&str) -> String,
    // The open list: its key, every label in it, and which one is set. See
    // `fx_choice_list` — the geometry is shared with the hit test.
    open: Option<(&str, &[String], usize)>,
    // See `fx_hit_test`.
    free: bool,
    s: &Settings,
) {
    let l = FxLayout::new(screen, anchor);
    if !l.panel.is_positive() {
        return;
    }
    let p = palette(s);
    // The same scrim the take settings use: it is what says the panel is in
    // FRONT of the window rather than being another band that appeared.
    painter.rect_filled(screen, 0.0, Color32::from_black_alpha(96));
    painter.rect_filled(l.panel, 4.0, p.bg);
    painter.rect_stroke(l.panel, 4.0, Stroke::new(1.0_f32, p.ink), StrokeKind::Inside);

    if l.title.is_positive() {
        let size = fit_text(l.title, fx.title(), l.title.height() * 0.42);
        if size >= MIN_TEXT {
            painter.text(
                Pos2::new(l.title.left(), l.title.top() + l.title.height() * 0.30),
                Align2::LEFT_CENTER,
                fx.title(),
                font_light(size),
                p.ink,
            );
        }
        // What the thing is, under its name. Faint, because it is read once.
        let sub = fit_text(l.title, fx.subtitle(), l.title.height() * 0.26);
        if sub >= MIN_TEXT {
            painter.text(
                Pos2::new(l.title.left(), l.title.top() + l.title.height() * 0.74),
                Align2::LEFT_CENTER,
                fx.subtitle(),
                font_light(sub),
                p.faint,
            );
        }
        draw_close_x(painter, l.close, &p);
    }

    for (i, FxRow { key, label, step }) in fx.rows(free).into_iter().enumerate() {
        let row = l.rows[i];
        if key.is_empty() || !row.is_positive() {
            continue;
        }
        let size = fit_text(slice_h(row, 0.0, FX_TRACK.0), label, row.height() * 0.52);
        if size >= MIN_TEXT {
            painter.text(
                Pos2::new(row.left(), row.center().y),
                Align2::LEFT_CENTER,
                label,
                font_light(size),
                p.ink,
            );
        }
        let track = FxLayout::track(row);
        let reading = Rect::from_min_max(Pos2::new(track.right(), row.top()), row.max);
        if step {
            // A name, in a box, that steps to the next one when pressed. Drawn
            // as a button rather than as a track, because it is one: there is
            // no position along a line that means "a dotted eighth", and none
            // that means "24 dB an octave" either.
            let division = &choice(key);
            let box_r = Rect::from_min_max(
                Pos2::new(track.left(), row.top() + row.height() * 0.14),
                Pos2::new(reading.right(), row.bottom() - row.height() * 0.14),
            );
            painter.rect_filled(box_r, 2.0, p.field);
            let size = fit_text(box_r.shrink(box_r.width() * 0.06), division, box_r.height() * 0.62);
            if size >= MIN_TEXT {
                painter.text(box_r.center(), Align2::CENTER_CENTER, division, font_light(size), p.ink);
            }
            continue;
        }

        let v = values(key).clamp(0.0, 1.0);
        // The track: a well with the set part filled, exactly as a fader's is.
        let h = (row.height() * 0.22).max(2.0);
        let well = Rect::from_center_size(track.center(), Vec2::new(track.width(), h));
        painter.rect_filled(well, h * 0.5, p.well);
        if v > 0.0 {
            let filled = Rect::from_min_size(well.min, Vec2::new(well.width() * v, h));
            painter.rect_filled(filled, h * 0.5, p.accent);
        }
        // And the handle, so it is obviously a thing to be dragged.
        let knob_w = (row.height() * 0.20).max(3.0);
        let x = well.left() + well.width() * v;
        painter.rect_filled(
            Rect::from_center_size(
                Pos2::new(x.clamp(well.left() + knob_w * 0.5, well.right() - knob_w * 0.5), well.center().y),
                Vec2::new(knob_w, row.height() * 0.66),
            ),
            1.5,
            p.ink,
        );
        let text = format!("{:.0}%", v * 100.0);
        let size = fit_text(reading, &text, row.height() * 0.46);
        if size >= MIN_TEXT {
            painter.text(
                Pos2::new(reading.right(), row.center().y),
                Align2::RIGHT_CENTER,
                &text,
                font_light(size),
                p.faint,
            );
        }
    }

    if l.reset.is_positive() {
        let size = fit_text(l.reset, "Reset", l.reset.height() * 0.52);
        if size >= MIN_TEXT {
            painter.rect_stroke(
                l.reset,
                2.0,
                Stroke::new(1.0_f32, p.faint),
                StrokeKind::Inside,
            );
            painter.text(l.reset.center(), Align2::CENTER_CENTER, "Reset", font_light(size), p.ink);
        }
    }

    // **The open list, LAST**, so it is over every row including its own — and
    // through `fx_choice_list`, the same geometry the press is tested against.
    if let Some((key, labels, at)) = open {
        let rows = fx_choice_list(screen, anchor, fx, key, labels.len());
        if let (Some(first), Some(last)) = (rows.first(), rows.last()) {
            let plate = Rect::from_min_max(first.min, last.max).expand(2.0);
            painter.rect_filled(plate, 3.0, p.field);
            painter.rect_stroke(plate, 3.0, Stroke::new(1.0_f32, p.ink), StrokeKind::Inside);
            for (k, (r, text)) in rows.iter().zip(labels).enumerate() {
                // The one that is SET wears the accent behind it, so opening
                // the list answers "where am I" before it answers "what else".
                if k == at {
                    painter.rect_filled(*r, 2.0, p.accent.gamma_multiply(0.35));
                }
                let size = fit_text(r.shrink2(Vec2::new(5.0, 0.0)), text, r.height() * 0.66);
                if size >= MIN_TEXT {
                    painter.text(
                        Pos2::new(r.left() + 4.0, r.center().y),
                        Align2::LEFT_CENTER,
                        text,
                        font_light(size),
                        p.ink,
                    );
                }
            }
        }
    }
}

/// Draw the setup panel in `shape` over `screen`, hung off the control at
/// `anchor` (see [`SetupShape`]).
pub fn draw_setup(
    painter: &Painter,
    screen: Rect,
    anchor: Rect,
    view: &RecorderView<'_>,
    s: &Settings,
    shape: SetupShape,
) {
    let l = SetupLayout::new(screen, anchor, shape);
    if !l.panel.is_positive() {
        return;
    }
    let p = palette(s);
    // **Refused rows look refused.** While a take writes, `setup_hit_test`
    // refuses every row of `targets()` but DONE - each one decides how the
    // file being written was going to be written - and the cassette's TAKE
    // panel opens mid-take anyway, because two of its rows still work there
    // ("Count-in into take", and "Hide elapsed time", which hides the clock
    // live). The refused rows were drawn live and swallowed every press.
    // `rows` is the palette they are drawn in: their words in the faint ink
    // under a take, the panel's own otherwise. DONE and the two live rows
    // keep `p`.
    let refused = Palette { ink: p.faint, ..p };
    let rows = if view.state.is_active() { &refused } else { &p };

    // A scrim over the whole window. It is what says the panel is modal, and
    // it is what makes a dialog drawn in the app's own ink read as being IN
    // FRONT of the window rather than as another band that appeared.
    painter.rect_filled(screen, 0.0, Color32::from_black_alpha(96));
    // **Clipped to the panel from here on**, which is what lets the tier this
    // shape does not show be laid out off-panel and drawn anyway. The scrim
    // above is the one thing that has to reach the whole window, so the clip
    // starts after it. See `SetupShape`.
    let painter = &painter.with_clip_rect(l.panel);
    painter.rect_filled(l.panel, 4.0, p.bg);
    painter.rect_stroke(l.panel, 4.0, Stroke::new(1.0_f32, p.ink), StrokeKind::Inside);

    if l.title.is_positive() {
        // 0.34, was 0.50: at the grown card the headline read as SHOUTING —
        // the owner asked for both the word and the button beside it smaller.
        let word = shape.title_word();
        let size = fit_text(l.title, word, l.title.height() * 0.34);
        if size >= MIN_TEXT {
            painter.text(
                Pos2::new(l.title.left(), l.title.center().y),
                Align2::LEFT_CENTER,
                word,
                font_light(size),
                p.ink,
            );
        }
    }
    // The tier headings: a word and a hairline running out to the right,
    // so AUDIO and MEDIA read as sections rather than as two more rows.
    // (`take_head` is laid out off the panel in every shape, so its word is
    // never seen.)
    for (band, word) in [
        (l.audio_head, "AUDIO"),
        (l.project_head, "MEDIA"),
        // **The red button's tier is not the take tier**, and a heading that
        // said TAKE over it was the whole of why its two rows read as the
        // wrong panel.
        (l.take_head, shape.tier_word()),
    ] {
        if !band.is_positive() {
            continue;
        }
        let size = fit_text(band, word, band.height() * 0.62);
        if size >= MIN_TEXT {
            painter.text(
                Pos2::new(band.left(), band.center().y),
                Align2::LEFT_CENTER,
                word,
                font_light(size),
                p.faint,
            );
            let text_w = size * 0.62 * word.len() as f32;
            let x0 = band.left() + text_w + 8.0;
            if x0 < band.right() - 4.0 {
                painter.line_segment(
                    [
                        Pos2::new(x0, band.center().y),
                        Pos2::new(band.right(), band.center().y),
                    ],
                    Stroke::new(1.0_f32, p.faint.gamma_multiply(0.5)),
                );
            }
        }
    }
    draw_word_button(painter, l.close, &["DONE", "OK"], &p);

    // **The take name lives here now.** It was a box at the top of a column in
    // the band that no longer exists — and it belongs here anyway: it is set
    // once a session, beside everything else that describes a take, and the
    // band is what your hands are on while one is running.
    //
    // Not required and not unique: the timestamp guarantees that. Type
    // "nocturne" once, press record five times, and get five adjacent folders
    // with no overwrite dialog ever.
    control(painter, l.name, rows);
    {
        let empty = view.take_name.is_empty();
        let shown = if empty { "(optional)" } else { view.take_name };
        label_text(
            painter,
            l.name,
            "NAME",
            shown,
            if empty { p.faint } else { rows.ink },
            rows,
        );
        if view.name_focused {
            draw_caption_caret(
                painter,
                l.name,
                "NAME",
                shown,
                view.take_name.chars().count(),
                rows,
            );
        }
    }

    // "Choose..." lives INSIDE the folder box, right-aligned, because the box
    // itself is the target: two adjacent controls that open the same picker is
    // one control too many.
    //
    // **Its width is taken out of the box BEFORE the path is placed.** The
    // band's folder box was wide enough that the two never met; this one is
    // half of a small panel, and the path ran straight under the word. A
    // reserved zone rather than a clip, so the path shrinks to fit the room it
    // really has instead of being cut off mid-directory.
    control(painter, l.dest, rows);
    if l.dest.is_positive() {
        let size = fit_text(l.dest, "Choose...", l.dest.height() * 0.45);
        let gap = if size >= MIN_TEXT {
            let w = painter
                .layout_no_wrap("Choose...".to_owned(), font_light(size), p.faint)
                .rect
                .width();
            painter.text(
                Pos2::new(l.dest.right() - label_inset(l.dest), l.dest.center().y),
                Align2::RIGHT_CENTER,
                "Choose...",
                font_light(size),
                p.faint,
            );
            w + label_inset(l.dest) * 2.0
        } else {
            0.0
        };
        let value = Rect::from_min_max(
            l.dest.min,
            Pos2::new(l.dest.right() - gap, l.dest.bottom()),
        );
        label_text(painter, value, "FOLDER", view.dest, rows.ink, rows);
    }
    // SHOW rather than OPEN, for the reason the tick beside Export has: nothing
    // is opened, a folder is shown.
    draw_word_button(painter, l.reveal, &["SHOW FOLDER", "SHOW"], rows);

    // **The PROJECT tier's three folders**, drawn exactly as the take's is —
    // a box carrying the word and the path, with a Choose... at its right
    // and a SHOW beside it. Four answers to one question, so they read as
    // a set rather than as four different controls.
    for (box_, show, word, path) in [
        (l.proj_dest, l.proj_reveal, "PROJECTS", view.project_dest),
        (l.export_dest, l.export_reveal, "EXPORTS", view.export_dest),
        (l.preset_dest, l.preset_reveal, "PRESETS", view.preset_dest),
        (l.irs_dest, l.irs_reveal, "IRS", view.irs_dest),
    ] {
        control(painter, box_, rows);
        if box_.is_positive() {
            let size = fit_text(box_, "Choose...", box_.height() * 0.45);
            let gap = if size >= MIN_TEXT {
                let w = painter
                    .layout_no_wrap("Choose...".to_owned(), font_light(size), p.faint)
                    .rect
                    .width();
                painter.text(
                    Pos2::new(box_.right() - label_inset(box_), box_.center().y),
                    Align2::RIGHT_CENTER,
                    "Choose...",
                    font_light(size),
                    p.faint,
                );
                w + label_inset(box_) * 2.0
            } else {
                0.0
            };
            let value = Rect::from_min_max(
                box_.min,
                Pos2::new(box_.right() - gap, box_.bottom()),
            );
            label_text(painter, value, word, path, rows.ink, rows);
        }
        draw_word_button(painter, show, &["SHOW FOLDER", "SHOW"], rows);
    }

    text_line(painter, l.folder, view.folder_preview, p.faint, false);
    // Disk as a DURATION. "214 GB free" means nothing to a pianist and "~58
    // min" means everything, which is why the view carries minutes and not
    // bytes in the first place.
    let disk = match view.disk_minutes {
        Some(m) if m >= 90.0 => format!("~{:.0} h left", m / 60.0),
        Some(m) => format!("~{m:.0} min left"),
        None => "measuring free space".to_owned(),
    };
    text_line(painter, l.disk, &disk, p.faint, true);

    labelled(
        painter,
        l.count_in,
        "COUNT-IN",
        &count_in_text(view.count_in_bars),
        rows.ink,
        rows,
    );
    // The AUDIO tier's own pair — the same two settings, standing where
    // the ARRANGEMENT's rules read as the arrangement's: the red button
    // counts in by this, and the signature rules the grid and the click.
    labelled(
        painter,
        l.audio_count_in,
        "COUNT-IN",
        &count_in_text(view.count_in_bars),
        rows.ink,
        rows,
    );
    {
        let typing = typing_for(view, NumField::Meter);
        let shown = typing
            .clone()
            .unwrap_or_else(|| view.time_signature.label());
        labelled(painter, l.audio_sig, "SIG", &shown, rows.ink, rows);
        // The same caret the TAKE tier's SIG draws: this is the copy the
        // cog's panel and the red button's show, and the arrows moved a
        // caret here that nobody could see.
        if let Some(typed) = typing {
            draw_caption_caret(painter, l.audio_sig, "SIG", &shown, typed.chars().count(), rows);
        }
    }
    // The two doors the cog carries now: the Project dialog and the
    // plugin folders — the menu's rows, reachable without the menu.
    labelled(painter, l.project_row, "PROJECT", "New / Open / Save...", rows.ink, rows);
    labelled(painter, l.plugin_folders, "PLUGINS", "Add a folder...", rows.ink, rows);
    draw_resync(painter, l.plugin_rescan, rows);
    // A readout as well as a way in, like AUDIO SETUP: whether the master is
    // going out over ReaStream right now.
    labelled(
        painter,
        l.stream_row,
        "STREAM",
        if s.reastream_enabled { "To OBS: on" } else { "To OBS: off" },
        rows.ink,
        rows,
    );
    {
        let typing = typing_for(view, NumField::Meter);
        let shown = typing
            .clone()
            .unwrap_or_else(|| view.time_signature.label());
        labelled(painter, l.time_sig, "SIG", &shown, rows.ink, rows);
        if let Some(typed) = typing {
            draw_caption_caret(painter, l.time_sig, "SIG", &shown, typed.chars().count(), rows);
        }
    }
    labelled(
        painter,
        l.export,
        "EXPORT",
        &export_summary(&s.record_export),
        rows.ink,
        rows,
    );

    // The four yes-or-no questions. Each is worded as the STATE it is in, not
    // as the action that changes it, because a tick already says which way it
    // is set and a row that says both is a row you have to read twice.
    //
    // Two of them stay live under a take, and keep the panel's ink.
    for (r, cap, on, ink) in [
        (l.open_when_done, "Show when done", s.record_open_when_done, rows),
        (l.click_in_take, "Click into takes", s.metronome_in_take, rows),
        (l.count_in_in_take, "Count-in into take", s.record_count_in_in_take, &p),
        (l.hide_elapsed, "Hide elapsed time", s.record_hide_elapsed, &p),
        // Session state, not a setting: monitoring is off at every launch by
        // construction, which is why it reads from the VIEW.
        (l.hear_input, "Hear the input", view.input_monitor, rows),
    ] {
        draw_tick(painter, r, cap, on, ink);
    }

    // **Setup**, showing the input it is about. The caption is a question the
    // device name answers, so the row is a readout as well as a way in - but
    // **not yet one that says when the interface has gone missing**:
    // `DeviceLabel::text` is the bare name for an open interface and a missing
    // one alike, and both are painted in the same ink, so the panel you would
    // open to find out why nothing is being recorded reads the same either way.
    //
    // Not `labelled`: a device name is as long as its driver makes it, and a
    // long one drew nothing at all there. See `DEVICE_FLOOR`.
    draw_device_row(painter, l.audio, "AUDIO SETUP", view.audio.text(), rows.ink, rows);
}



// ── the backing track's waveform ────────────────────────────────────────────
//
// A right-click on the track's icon opens this. The row itself is a level and
// an import button, because that is all a fifteen-point row can be; where the
// track STARTS and STOPS is a question about a picture, and it needs one.

/// The click's half of the waveform's time axis: an x position on `wave` as
/// seconds into a file `seconds` long.
///
/// `None` off the rect or for an empty file. The exact inverse of
/// [`fraction_of`], and the pair is guard-tested as a round trip — the click
/// and the playhead it produces may never disagree, because the playhead the
/// user sees IS the answer to where they clicked.
pub fn seconds_at(wave: Rect, seconds: f64, x: f32) -> Option<f64> {
    if !wave.is_positive() || seconds <= 0.0 || !(wave.left()..=wave.right()).contains(&x) {
        return None;
    }
    let t = f64::from((x - wave.left()) / wave.width());
    Some((t * seconds).clamp(0.0, seconds))
}

/// The playhead's half: a position in seconds as a fraction along the file.
///
/// `None` for an empty file or a position past its end — a playhead beyond
/// the clip is drawn nowhere rather than pinned to the edge lying about where
/// the transport is.
pub fn fraction_of(seconds: f64, position_s: f64) -> Option<f32> {
    if seconds <= 0.0 || position_s < 0.0 || position_s > seconds {
        return None;
    }
    Some((position_s / seconds) as f32)
}

/// A time in seconds as `m:ss.t`, which is how somebody reads a position.
/// Tenths TRUNCATED, then split — rounding the seconds while truncating
/// the minutes printed the impossible "0:60.0" for the last 50 ms of
/// every minute, a flash on the clock at each boundary the playhead
/// crossed.
pub fn time_text(seconds: f64) -> String {
    let tenths = (seconds.max(0.0) * 10.0).floor() as u64;
    format!("{}:{:02}.{}", tenths / 600, (tenths / 10) % 60, tenths % 10)

}




#[cfg(test)]
mod spacing_probe {
    use super::tests::band;
    use super::*;

    /// Not an assertion — a ruler. Prints the four clusters at the right end
    /// of the band with the gaps between them, so the spacing is measured
    /// rather than argued about.
    #[test]
    #[ignore]
    fn print_the_right_hand_clusters() {
        for w in [1300.0_f32, 975.0, 1625.0] {
            let r = band(w);
            let l = Layout::new(r, &RecorderView::empty());
            let show = |name: &str, x: Rect| {
                if x.is_positive() {
                    println!("  {name:<14} {:8.1} .. {:8.1}  (w {:6.1})", x.left(), x.right(), x.width());
                } else {
                    println!("  {name:<14} —");
                }
            };
            println!("band {w} (rect {:.1}..{:.1})", r.left(), r.right());
            show("knobs/meters", l.meter);
            show("vari(pitch)", l.vari);
            if let Some(f) = vari_face(l.vari, l.vari_label) {
                println!("    vari CELL   y {:.1}..{:.1}", l.vari.top(), l.vari.bottom());
                println!("    vari FACE   y {:.1}..{:.1}  circle y {:.1}..{:.1}",
                    f.face.top(), f.face.bottom(),
                    f.face.center().y - f.radius, f.face.center().y + f.radius);
                println!("    radius {:.1}  label_h {:.1}", f.radius, f.label_h);
                let strip = f.label_h.max(KNOB_LABEL.1);
                let label = Rect::from_min_max(
                    Pos2::new(l.vari_label.left(), l.vari.top()),
                    Pos2::new(l.vari_label.right(), l.vari.top() + strip),
                );
                let dial_label = KNOB_LABEL.1;
                let size = fit_text(label, PITCH_LABEL, dial_label * 3.4)
                    .min(strip * 0.92)
                    .max(MIN_TEXT);
                println!("    PITCH CONT size {size:.1} (word room w {:.1}, cell w {:.1})",
                    label.width(), l.vari.width());
            }
            if let Some(kf) = knob_face(l.vari) {
                let drag = Rect::from_min_max(
                    l.vari.min,
                    Pos2::new(l.vari.right(), l.vari.bottom() - kf.label_h),
                );
                println!("    vari DRAG   y {:.1}..{:.1}", drag.top(), drag.bottom());
            }
            {
                let strip = (knob_face(l.vari).map_or(0.0, |k| k.label_h) * 2.0)
                    .max(KNOB_LABEL.1);
                if let Some(w) = vari_word_ink(l.vari, l.vari_label, strip) {
                    let step = l.master_bars.height() / 9.0;
                    let sz = fit_text(
                        Rect::from_min_size(
                            l.master_scale[0].min,
                            Vec2::new(l.master_scale[0].width() - 3.0, step),
                        ),
                        MASTER_SCALE_FIT,
                        step * 0.86,
                    );
                    let mink = (l.master_scale[0].right() - 2.0 - master_scale_ink_w(sz))
                        .max(l.master_scale[0].left());
                    println!(
                        "    CENTRING word.right {:.1} | lufs {:.1}..{:.1} | master ink {:.1}  => left gap {:.1}, right gap {:.1}",
                        w.right(),
                        l.lufs_bar.left(),
                        l.lufs_ticks.right(),
                        mink,
                        l.lufs_bar.left() - w.right(),
                        mink - l.lufs_ticks.right(),
                    );
                }
            }
            show("lufs_bar", l.lufs_bar);
            show("lufs_ticks", l.lufs_ticks);
            show("master_scale0", l.master_scale[0]);
            show("master_bars", l.master_bars);
            show("master_scale1", l.master_scale[1]);
            show("master_knob", l.master_knob);
            let g = |a: Rect, b: Rect| {
                if a.is_positive() && b.is_positive() { b.left() - a.right() } else { f32::NAN }
            };
            println!("  GAPS: meters->vari {:.1} | vari->lufsbar {:.1} | lufsticks->mscale0 {:.1} | lufsticks->mbars {:.1}",
                g(l.meter, l.vari), g(l.vari, l.lufs_bar), g(l.lufs_ticks, l.master_scale[0]), g(l.lufs_ticks, l.master_bars));
        }
    }
}

#[cfg(test)]
mod claim_probe {
    use super::*;

    #[test]
    #[ignore]
    fn probe_word_overlap() {
        let mut worst_r: f32 = f32::INFINITY;
        let mut worst_l: f32 = f32::INFINITY;
        let mut worst_rw = 0.0_f32;
        let mut worst_lw = 0.0_f32;
        let mut w = 600.0_f32;
        while w <= 3840.0 {
            let r = Rect::from_min_size(Pos2::new(0.0, 350.0), Vec2::new(w, band_height(w)));
            let v = RecorderView::empty();
            let l = Layout::new(r, &v);
            if l.vari.is_positive() && l.lufs_bar.is_positive() {
                let strip =
                    (knob_face(l.vari).map_or(0.0, |k| k.label_h) * 2.0).max(KNOB_LABEL.1);
                if let Some(word) = vari_word_ink(l.vari, l.vari_label, strip) {
                    let gap_r = l.lufs_bar.left() - word.right();
                    let gap_l = word.left() - l.meter.right();
                    if gap_r < worst_r { worst_r = gap_r; worst_rw = w; }
                    if gap_l < worst_l { worst_l = gap_l; worst_lw = w; }
                    if gap_r <= 0.0 || gap_l <= 0.0 {
                        println!("OVERLAP w={w:.0}: gap_r={gap_r:.2} gap_l={gap_l:.2} word={:.1}..{:.1} lufs.left={:.1} meter.right={:.1} vari={:.1}..{:.1} label={:.1}..{:.1}",
                            word.left(), word.right(), l.lufs_bar.left(), l.meter.right(),
                            l.vari.left(), l.vari.right(), l.vari_label.left(), l.vari_label.right());
                    }
                }
            }
            w += 1.0;
        }
        println!("WORST right gap {worst_r:.2} at w={worst_rw:.0}; WORST left gap {worst_l:.2} at w={worst_lw:.0}");
    }

    #[test]
    #[ignore]
    fn probe_word_vs_dial() {
        let mut worst: f32 = f32::INFINITY;
        let mut at = 0.0_f32;
        let mut w = 600.0_f32;
        while w <= 3840.0 {
            let r = Rect::from_min_size(Pos2::new(0.0, 350.0), Vec2::new(w, band_height(w)));
            let v = RecorderView::empty();
            let l = Layout::new(r, &v);
            if let Some(f) = vari_face(l.vari, l.vari_label) {
                let strip =
                    (knob_face(l.vari).map_or(0.0, |k| k.label_h) * 2.0).max(KNOB_LABEL.1);
                let size = vari_word_size(
                    if l.vari_label.is_positive() { l.vari_label } else { l.vari },
                    strip,
                    KNOB_LABEL.1,
                );
                // real ink bottom, assuming the WORD_LINE model
                let ink_bottom = l.vari.top() + size * WORD_LINE;
                let circle_top = f.face.center().y - f.radius * RECESS_R;
                let g = circle_top - ink_bottom;
                if g < worst { worst = g; at = w; }
                if g <= 0.0 {
                    println!("DIAL OVERLAP w={w:.0}: g={g:.2} size={size:.1} ink_bottom={ink_bottom:.1} circle_top={circle_top:.1}");
                }
            }
            w += 1.0;
        }
        println!("WORST word->recess gap {worst:.2} at w={at:.0}");
    }

    /// Sweep: is the LUFS cluster centred between the WORD's ink and the
    /// master scale's ink at every width, in both layouts?
    #[test]
    #[ignore]
    fn probe_centring_sweep() {
        for rolling in [false, true] {
            let mut v = RecorderView::empty();
            if rolling {
                v.state = RecordState::Rolling;
            }
            let mut worst_err = 0.0_f32;
            let mut worst_w = 0.0_f32;
            let mut bail = 0usize;
            let mut touch = 0usize;
            let mut worst_read: f32 = f32::INFINITY;
            let mut worst_read_w = 0.0_f32;
            let mut worst_read_over = 0.0_f32;
            let mut w = 600.0_f32;
            while w <= 3840.0 {
                let r = Rect::from_min_size(Pos2::new(0.0, 350.0), Vec2::new(w, band_height(w)));
                let l = Layout::new(r, &v);
                if !l.lufs_bar.is_positive() || !l.master_bars.is_positive() {
                    w += 1.0;
                    continue;
                }
                let strip =
                    (knob_face(l.vari).map_or(0.0, |k| k.label_h) * 2.0).max(KNOB_LABEL.1);
                let word_right = match vari_word_ink(l.vari, l.vari_label, strip) {
                    Some(x) => x.right(),
                    None => knob_face(l.vari).map_or(l.vari.right(), |f| f.face.right()),
                };
                let step = l.master_bars.height() / 9.0;
                let size = fit_text(
                    Rect::from_min_size(
                        l.master_scale[0].min,
                        Vec2::new(l.master_scale[0].width() - 3.0, step),
                    ),
                    MASTER_SCALE_FIT,
                    step * 0.86,
                );
                let mink = (l.master_scale[0].right() - 2.0 - master_scale_ink_w(size))
                    .max(l.master_scale[0].left());
                let before = l.lufs_bar.left() - word_right;
                let after = mink - l.lufs_ticks.right();
                let err = (before - after).abs();
                if err > worst_err {
                    worst_err = err;
                    worst_w = w;
                }
                if before <= 0.0 || after <= 0.0 {
                    touch += 1;
                    if touch < 6 {
                        println!("TOUCH rolling={rolling} w={w:.0} before={before:.2} after={after:.2}");
                    }
                }
                if err > 1.0 {
                    bail += 1;
                    if bail < 8 {
                        println!("OFF-CENTRE rolling={rolling} w={w:.0} before={before:.2} after={after:.2}");
                    }
                }
                // The reading, drawn centred on the BAR (see draw_lufs).
                if l.lufs_readout.is_positive() {
                    let text = "M -14.2";
                    let rs = fit_text(l.lufs_readout, text, l.lufs_readout.height() * 0.78);
                    if rs >= MIN_TEXT {
                        let ink = text.chars().count() as f32 * ADV * rs;
                        let left = l.lufs_bar.center().x - ink * 0.5;
                        let over = l.lufs_bar.left() - left;
                        let gap = left - word_right;
                        if gap < worst_read {
                            worst_read = gap;
                            worst_read_w = w;
                            worst_read_over = over;
                        }
                    }
                }
                w += 1.0;
            }
            println!("rolling={rolling}: worst |before-after| = {worst_err:.2} at w={worst_w:.0}; off-centre widths {bail}; touching {touch}");
            println!("rolling={rolling}: readout ink nearest the word: gap {worst_read:.2} at w={worst_read_w:.0} (sticks {worst_read_over:.2} left of the bar)");
        }
    }

    #[test]
    #[ignore]
    fn probe_free_aspect() {
        let mut worst_r: f32 = f32::INFINITY;
        let mut worst_at = (0.0_f32, 0.0_f32);
        let mut worst_l: f32 = f32::INFINITY;
        let mut worst_l_at = (0.0_f32, 0.0_f32);
        let mut worst_d: f32 = f32::INFINITY;
        let mut worst_d_at = (0.0_f32, 0.0_f32);
        let mut hits = 0u32;
        let mut w = 640.0_f32;
        while w <= 2600.0 {
            let mut h = 340.0_f32;
            while h <= 1600.0 {
                let r = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(w, h));
                let v = RecorderView::empty();
                let l = Layout::new(r, &v);
                if l.vari.is_positive() && l.lufs_bar.is_positive() {
                    let strip =
                        (knob_face(l.vari).map_or(0.0, |k| k.label_h) * 2.0).max(KNOB_LABEL.1);
                    if let Some(word) = vari_word_ink(l.vari, l.vari_label, strip) {
                        let gap_r = l.lufs_bar.left() - word.right();
                        let gap_l = word.left() - l.meter.right();
                        if gap_r < worst_r { worst_r = gap_r; worst_at = (w, h); }
                        if gap_l < worst_l { worst_l = gap_l; worst_l_at = (w, h); }
                        if gap_r <= 0.0 || gap_l <= 0.0 {
                            hits += 1;
                            if hits < 12 {
                                println!("OVERLAP {w:.0}x{h:.0}: gap_r={gap_r:.2} gap_l={gap_l:.2} word={:.1}..{:.1} lufs.left={:.1} meter.right={:.1} label={:.1}..{:.1} vari={:.1}..{:.1}",
                                    word.left(), word.right(), l.lufs_bar.left(), l.meter.right(),
                                    l.vari_label.left(), l.vari_label.right(), l.vari.left(), l.vari.right());
                            }
                        }
                    }
                    if let Some(f) = vari_face(l.vari, l.vari_label) {
                        let size = vari_word_size(
                            if l.vari_label.is_positive() { l.vari_label } else { l.vari },
                            strip,
                            KNOB_LABEL.1,
                        );
                        let ink_bottom = l.vari.top() + size * WORD_LINE;
                        let circle_top = f.face.center().y - f.radius * RECESS_R;
                        let g = circle_top - ink_bottom;
                        if g < worst_d { worst_d = g; worst_d_at = (w, h); }
                    }
                }
                h += 4.0;
            }
            w += 4.0;
        }
        println!("FREE-ASPECT worst right gap {worst_r:.2} at {:?}; worst left gap {worst_l:.2} at {:?}; worst word->recess {worst_d:.2} at {:?}; overlaps {hits}",
            worst_at, worst_l_at, worst_d_at);
    }

    #[test]
    #[ignore]
    fn probe_font_metrics() {
        use crate::fonts::{self, FontChoice};
        for choice in FontChoice::ALL {
            let ctx = egui::Context::default();
            fonts::install(&ctx, choice, None);
            let _ = ctx.run(Default::default(), |_| {});
            for size in [6.0_f32, 10.0, 14.0, 18.0, 24.0, 30.0] {
                let (w, h, ink_top, ink_bottom) = ctx.fonts_mut(|f| {
                    let g = f.layout_no_wrap(
                        PITCH_LABEL.to_owned(),
                        font_light(size),
                        Color32::WHITE,
                    );
                    let row = &g.rows[0];
                    let mut top = f32::INFINITY;
                    let mut bot = f32::NEG_INFINITY;
                    for gl in &row.glyphs {
                        top = top.min(gl.pos.y + gl.uv_rect.offset.y);
                        bot = bot.max(gl.pos.y + gl.uv_rect.offset.y + gl.uv_rect.size.y);
                    }
                    (g.rect.width(), g.rect.height(), top, bot)
                });
                let n = PITCH_LABEL.chars().count() as f32;
                println!(
                    "{:?} size {size:>4.1}: galley w {w:7.2} (adv/em {:.4}, model {:.4}) h {h:6.2} (h/size {:.3}, WORD_LINE {WORD_LINE:.3}) ink y {ink_top:6.2}..{ink_bottom:6.2} (bottom/size {:.3})",
                    choice, w / (n * size), ADV, h / size, ink_bottom / size
                );
            }
        }
    }

    /// Does the word the PAINTER draws match the word the LAYOUT measured?
    #[test]
    #[ignore]
    fn probe_word_size_agreement() {
        let v = RecorderView::empty();
        let mut worst = 0.0_f32;
        let mut worst_w = 0.0_f32;
        let mut printed = 0usize;
        let mut w = 600.0_f32;
        while w <= 3840.0 {
            let r = Rect::from_min_size(Pos2::new(0.0, 350.0), Vec2::new(w, band_height(w)));
            let l = Layout::new(r, &v);
            if !l.vari.is_positive() || !l.lufs_bar.is_positive() {
                w += 1.0;
                continue;
            }
            // What the painter uses.
            let dial_label = {
                let cell = l.cell(Fx::Limiter);
                knob_face(cell).map_or(KNOB_LABEL.1, |f| {
                    let bottom = Rect::from_min_max(
                        Pos2::new(cell.left(), cell.bottom() - f.label_h),
                        cell.max,
                    );
                    fit_text(bottom, Fx::Limiter.title(), f.label_h * 0.9)
                })
            };
            let strip =
                (knob_face(l.vari).map_or(0.0, |k| k.label_h) * 2.0).max(KNOB_LABEL.1);
            let word = if l.vari_label.is_positive() { l.vari_label } else { l.vari };
            let label = Rect::from_min_max(
                Pos2::new(word.left(), l.vari.top()),
                Pos2::new(word.right(), l.vari.top() + strip),
            );
            let painted = vari_word_size(label, strip, dial_label);
            let assumed = vari_word_size(word, strip, KNOB_LABEL.1);
            let d = painted - assumed;
            if d.abs() > worst {
                worst = d.abs();
                worst_w = w;
            }
            if d.abs() > 0.05 && printed < 8 {
                printed += 1;
                let half_p = PITCH_LABEL.chars().count() as f32 * ADV * painted * 0.5;
                let mid = l.vari.center().x;
                println!(
                    "w={w:.0} dial_label={dial_label:.2} painted={painted:.2} assumed={assumed:.2} \
                     ink_right painted={:.1} assumed={:.1} lufs_bar.left={:.1}",
                    mid + half_p,
                    vari_word_ink(l.vari, l.vari_label, strip).map_or(f32::NAN, |x| x.right()),
                    l.lufs_bar.left(),
                );
            }
            w += 1.0;
        }
        println!("worst painted-vs-assumed word size difference {worst:.2} pt at w={worst_w:.0}");
    }

    /// The reading under the meter, at its real worst-case string.
    #[test]
    #[ignore]
    fn probe_readout_overhang() {
        let v = RecorderView::empty();
        let mut worst_over = 0.0_f32;
        let mut worst_w = 0.0_f32;
        let mut ratio_worst = 1.0_f32;
        let mut ratio_w = 0.0_f32;
        let mut w = 600.0_f32;
        while w <= 3840.0 {
            let r = Rect::from_min_size(Pos2::new(0.0, 350.0), Vec2::new(w, band_height(w)));
            let l = Layout::new(r, &v);
            if !l.lufs_readout.is_positive() || !l.lufs_bar.is_positive() || !l.master_bars.is_positive() {
                w += 1.0;
                continue;
            }
            let strip =
                (knob_face(l.vari).map_or(0.0, |k| k.label_h) * 2.0).max(KNOB_LABEL.1);
            let Some(word) = vari_word_ink(l.vari, l.vari_label, strip) else { w += 1.0; continue };
            let step = l.master_bars.height() / 9.0;
            let size = fit_text(
                Rect::from_min_size(
                    l.master_scale[0].min,
                    Vec2::new(l.master_scale[0].width() - 3.0, step),
                ),
                MASTER_SCALE_FIT,
                step * 0.86,
            );
            let mink = (l.master_scale[0].right() - 2.0 - master_scale_ink_w(size))
                .max(l.master_scale[0].left());
            let text = "M 30.0";
            let rs = fit_text(l.lufs_readout, text, l.lufs_readout.height() * 0.78);
            if rs < MIN_TEXT {
                w += 1.0;
                continue;
            }
            let ink = text.chars().count() as f32 * ADV * rs;
            let left = l.lufs_bar.center().x - ink * 0.5;
            let right = l.lufs_bar.center().x + ink * 0.5;
            let over = l.lufs_bar.left() - left;
            let before = left.min(l.lufs_bar.left()) - word.right();
            let after = mink - right.max(l.lufs_ticks.right());
            if over > worst_over { worst_over = over; worst_w = w; }
            let ratio = if before > 0.0 { after / before } else { f32::INFINITY };
            if ratio > ratio_worst { ratio_worst = ratio; ratio_w = w; }
            if (w - 1300.0).abs() < 0.5 || (w - 890.0).abs() < 0.5 || (w - 2560.0).abs() < 0.5 {
                println!("w={w:.0}: readout size {rs:.2} ink {ink:.1} rect w {:.1} h {:.1} | bar {:.1}..{:.1} | readout ink {left:.1}..{right:.1} | word.right {:.1} | master ink {mink:.1} | before {before:.2} after {after:.2}",
                    l.lufs_readout.width(), l.lufs_readout.height(),
                    l.lufs_bar.left(), l.lufs_bar.right(), word.right());
            }
            w += 1.0;
        }
        println!("worst readout overhang left of the bar {worst_over:.2} at w={worst_w:.0}; worst after/before ratio {ratio_worst:.2} at w={ratio_w:.0}");
    }
}

/// **What a contrast decision costs**, with the sRGB curve looked up against
/// with it computed — the measurement behind `srgb_to_linear_table`.
///
/// Ignored by default; run with `--ignored --nocapture`, and in RELEASE. A
/// debug build reports about ten times the truth on this code and the app
/// ships optimised.
#[cfg(test)]
mod contrast_probe {
    use super::tests::luminance_reference;
    use super::*;
    use std::time::{Duration, Instant};

    /// [`contrast_ratio`] over the curve as it was computed, so the two
    /// roads are timed doing the same work.
    fn contrast_powf(a: Color32, b: Color32) -> f32 {
        let (x, y) = (luminance_reference(a), luminance_reference(b));
        let (hi, lo) = if x > y { (x, y) } else { (y, x) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// [`legible_surface`] over the same, verbatim but for the ratio it asks.
    fn legible_surface_powf(c: Color32) -> Color32 {
        let light = contrast_powf(c, INK_LIGHT);
        let dark = contrast_powf(c, INK_DARK);
        if light.max(dark) >= 4.5 {
            return c;
        }
        let (ink, toward) = if light > dark {
            (INK_LIGHT, Color32::BLACK)
        } else {
            (INK_DARK, Color32::WHITE)
        };
        let mut out = c;
        for step in 1u8..=50 {
            let t = f32::from(step) * 0.02_f32;
            let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
            out = Color32::from_rgb(
                f(c.r(), toward.r()),
                f(c.g(), toward.g()),
                f(c.b(), toward.b()),
            );
            if contrast_powf(out, ink) >= 4.6 {
                break;
            }
        }
        out
    }

    fn timed(n: u32, mut f: impl FnMut()) -> Duration {
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        t.elapsed() / n
    }

    #[test]
    #[ignore]
    fn what_does_a_contrast_decision_cost() {
        // The strips' own swatches, which is what the arrangement measures
        // every frame: one `legible_surface` per visible clip and per header.
        let swatches: Vec<Color32> = crate::mixer_panel::STRIP_COLORS
            .iter()
            .map(|&(r, g, b)| Color32::from_rgb(r, g, b))
            .collect();
        // A colour in the dead zone, where the walk runs its fifty steps and
        // the powf bill is at its worst.
        let deep = swatches
            .iter()
            .copied()
            .filter(|c| (0.12..0.22).contains(&luminance(*c)))
            .max_by(|a, b| luminance(*a).total_cmp(&luminance(*b)))
            .expect("no dead-zone swatch in the table");

        // **The colour goes through `black_box`, not the answer.** Both of
        // these are pure functions of one constant, so a `black_box` on the
        // RESULT lets the optimiser hoist the whole call out of the loop and
        // time an empty one: the first run of this said `legible_surface`
        // cost a nanosecond, which is a bus cycle and not fifty steps of
        // colour arithmetic. Blinding the INPUT is what keeps the work
        // inside the loop.
        // **The colour goes through `black_box`, not the answer.** Both of
        // these are pure functions of one constant, so a `black_box` on the
        // RESULT lets the optimiser hoist the whole call out of the loop and
        // time an empty one: the first run of this said `legible_surface`
        // cost a nanosecond, which is a bus cycle and not fifty steps of
        // colour arithmetic. Blinding the INPUT is what keeps the work
        // inside the loop.
        //
        // And the cheap calls are timed over the whole swatch table rather
        // than one colour, because `Duration` counts in whole nanoseconds
        // and a table lookup does not take one: measured singly, the fast
        // road reports zero and the ratio is unreadable.
        let opaque = || std::hint::black_box(deep);
        println!(
            "contrast [{}]: legible_surface       table {:>9.3?}  powf {:>9.3?}   (one colour in the dead zone, {deep:?}, luminance {:.3})",
            crate::perf_fixtures::build(),
            timed(20_000, || { std::hint::black_box(legible_surface(opaque())); }),
            timed(20_000, || { std::hint::black_box(legible_surface_powf(opaque())); }),
            luminance(deep),
        );
        let sweep = |f: &dyn Fn(Color32)| {
            timed(200, || {
                for c in &swatches {
                    f(std::hint::black_box(*c));
                }
            })
        };
        println!(
            "contrast [{}]: luminance       x{:<5} table {:>9.3?}  powf {:>9.3?}",
            crate::perf_fixtures::build(),
            swatches.len(),
            sweep(&|c| { std::hint::black_box(luminance(c)); }),
            sweep(&|c| { std::hint::black_box(luminance_reference(c)); }),
        );
        println!(
            "contrast [{}]: legible_surface x{:<5} table {:>9.3?}  powf {:>9.3?}",
            crate::perf_fixtures::build(),
            swatches.len(),
            sweep(&|c| { std::hint::black_box(legible_surface(c)); }),
            sweep(&|c| { std::hint::black_box(legible_surface_powf(c)); }),
        );
        // And the answers must not have moved.
        for c in &swatches {
            assert_eq!(legible_surface(*c), legible_surface_powf(*c), "{c:?} moved");
        }
    }
}
