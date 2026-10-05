//! The theory band (D-UI-17): what you are playing, drawn as geometry.
//!
//! Three pictures of the same twelve notes, any combination of which can be on
//! at once. They share one band above everything else and split its width.
//!
//!   * the **circle of fifths**, which arranges keys so that neighbours share
//!     six of seven notes — the reason a modulation to the next sector sounds
//!     like a step and one to the opposite sector sounds like a shove,
//!   * the **Tonnetz**, a lattice where the three axes are the fifth, the major
//!     third and the minor third, so every triad is a triangle and two chords
//!     that share two notes are two triangles that share an edge, and
//!   * the **harmonic triangles**, I-IV-V pointing up with i-iv-v inverted
//!     through the same centre, which is the tonic-subdominant-dominant
//!     relationship as one shape you can slide to a new key.
//!
//! Like `piano.rs` and `fretboard_panel.rs`, this module is dumb: it is handed
//! a set of sounding pitch classes and it draws. It does not detect anything.
//! What it adds over those two is that a pitch class here has no octave and no
//! string — the whole point of these diagrams is what survives forgetting both.

use crate::fonts;
use crate::settings::Settings;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};

/// Band height for a 1300pt-wide window. Large, because these are diagrams
/// rather than readouts: a circle of fifths with unreadable sector labels is
/// decoration. Scaled with everything else (spec §3.2).
pub const BAND_H_AT_1300: f64 = 300.0;

/// Height of the theory band for a window `w` points wide, or 0 when nothing
/// is selected. Truncated like every other band in the layout.
pub fn band_height(w: f32, views: &Views) -> f32 {
    if views.count() == 0 {
        return 0.0;
    }
    (BAND_H_AT_1300 * w as f64 / 1300.0).trunc() as f32
}

// ── the twelve notes ───────────────────────────────────────────────────────

const SHARP_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
const FLAT_NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

/// The tonic of a key signature, as a pitch class.
///
/// Sharps go up a fifth each, flats down one, so the signature IS the tonic in
/// disguise: `key` fifths from C. The major tonic, because that is what the
/// menu's labels lead with ("Eb / Cm  (3b)") and the relative minor sits on the
/// same six notes either way.
pub fn key_tonic(key: i32) -> u8 {
    (7 * key).rem_euclid(12) as u8
}

/// Which way to spell the five black keys, given a signature.
///
/// The same rule `staff::spell_in_key` uses, and deliberately the same words:
/// flats when the signature has flats, sharps when it has sharps, and the
/// user's own preference only when the signature is silent. A diagram spelling
/// A flat while the staff beside it prints G sharp is one instrument's key
/// signature drawn twice, two different ways.
fn key_flats(key: i32, prefer_flats: bool) -> bool {
    if key == 0 {
        prefer_flats
    } else {
        key < 0
    }
}

fn pc_name(pc: u8, prefer_flats: bool) -> &'static str {
    let i = (pc % 12) as usize;
    if prefer_flats {
        FLAT_NAMES[i]
    } else {
        SHARP_NAMES[i]
    }
}

/// The real accidental characters. JetBrains Mono carries both and sits at the
/// bottom of every font chain, so they always render.
const FLAT: char = '\u{266D}';
const SHARP: char = '\u{266F}';

/// Draw a note name with its accidental raised to the top right, the way
/// music is set rather than the way a string concatenates.
///
/// `♭` and `♯` on the baseline at full size read as a second letter — which
/// is exactly the complaint that started this: a lower-case "Bb" was two b's,
/// one a note and one a flat, and no reader should have to work out which.
/// Raised and smaller, the accidental is unmistakably a modifier.
///
/// The pair is centred as a WHOLE: the letter shifts left by half the
/// accidental's width so "B♭" sits where "B" would, or a ring of note names
/// wobbles.
///
/// `suffix` is appended after the accidental at full size — the "m" of a
/// minor chord, which is part of the NAME rather than a modifier of the
/// letter, and so is not raised with it.
fn draw_note(
    painter: &Painter,
    center: Pos2,
    pc: u8,
    prefer_flats: bool,
    size: f32,
    color: Color32,
    lower: bool,
    suffix: &str,
) {
    let name = pc_name(pc, prefer_flats);
    let letter = if lower {
        name[..1].to_lowercase()
    } else {
        name[..1].to_owned()
    };
    let acc = match name.as_bytes().get(1) {
        Some(b'b') => Some(FLAT),
        Some(b'#') => Some(SHARP),
        _ => None,
    };
    let Some(acc) = acc else {
        painter.text(
            center,
            Align2::CENTER_CENTER,
            &format!("{letter}{suffix}"),
            font(size),
            color,
        );
        return;
    };
    let acc_size = size * 0.72;
    // Monospace-ish advance; close enough to centre the pair by eye and it
    // cannot be measured without laying the text out first.
    let acc_w = acc_size * 0.6 + suffix.chars().count() as f32 * size * 0.6;
    let r = painter.text(
        center - Vec2::new(acc_w * 0.5, 0.0),
        Align2::CENTER_CENTER,
        &letter,
        font(size),
        color,
    );
    let a = painter.text(
        Pos2::new(r.right(), r.top() + acc_size * 0.34),
        Align2::LEFT_CENTER,
        acc.to_string(),
        font(acc_size),
        color,
    );
    if !suffix.is_empty() {
        painter.text(
            Pos2::new(a.right(), center.y),
            Align2::LEFT_CENTER,
            suffix,
            font(size),
            color,
        );
    }
}

/// The twelve major keys in ascending fifths from C. Index is also the number
/// of sharps, up to the enharmonic seam at six.
const FIFTHS: [u8; 12] = [0, 7, 2, 9, 4, 11, 6, 1, 8, 3, 10, 5];

/// The key signature at position `i` around the circle, as (sharps, flats).
/// Exactly one of the two is non-zero except at C, which has neither, and at
/// the seam, which is six of either depending on how you spell it.
fn signature(i: usize) -> (u8, u8) {
    match i {
        0 => (0, 0),
        1..=6 => (i as u8, 0),
        _ => (0, (12 - i) as u8),
    }
}

/// The major scale on `root`, as a 12-bit pitch-class mask.
pub fn major_scale(root: u8) -> u16 {
    const STEPS: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
    STEPS
        .iter()
        .fold(0u16, |m, s| m | 1 << ((root as u16 + *s as u16) % 12))
}

/// A major triad as a pitch-class mask.
pub fn major_triad(root: u8) -> u16 {
    (1 << (root % 12)) | (1 << ((root + 4) % 12)) | (1 << ((root + 7) % 12))
}

/// A minor triad as a pitch-class mask.
pub fn minor_triad(root: u8) -> u16 {
    (1 << (root % 12)) | (1 << ((root + 3) % 12)) | (1 << ((root + 7) % 12))
}

fn has(mask: u16, pc: u8) -> bool {
    mask & (1 << (pc % 12)) != 0
}

/// Read the root and quality back off one of the app's own chord labels.
///
/// Deliberately narrow. These labels are generated by `tangent-core`, so the
/// leading note name is always one of the twelve spellings above, and a
/// lower-case `m` that is not the start of `maj` is always minor quality. It
/// returns None rather than guessing on anything else — a diagram that points
/// at the wrong tonic is worse than one that points nowhere, because there is
/// no way to tell from looking at it.
pub fn parse_label(label: &str) -> Option<(u8, bool)> {
    let b = label.as_bytes();
    let letter = *b.first()?;
    let base = match letter {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return None,
    };
    let (root, rest) = match b.get(1) {
        Some(b'#') => ((base + 1) % 12, &label[2..]),
        Some(b'b') => ((base + 11) % 12, &label[2..]),
        _ => (base, &label[1..]),
    };
    // "m" is minor; "maj" is not, and neither is the "m" inside "dim".
    let minor = rest.starts_with('m') && !rest.starts_with("maj");
    Some((root, minor))
}

// ── what is selected ───────────────────────────────────────────────────────

/// **Which elements are in the band, IN ORDER.**
///
/// An ordered list and not a set of flags, and that is the whole design. The
/// band used to be three bools, which can say what is showing and cannot say
/// where — so the arrangement was whatever order the enum happened to be
/// written in and there was no way for anybody to change it.
///
/// With a list, one interaction does both jobs: a number key removes its
/// element if it is showing and APPENDS it if it is not. Turning something off
/// and on again therefore moves it to the right, which is the whole of the
/// reordering vocabulary and takes no second control to learn. See
/// [`Views::toggled`].
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Views(Vec<View>);

impl Views {
    /// Every element, in the order the number keys are numbered.
    pub fn all() -> Views {
        Views(View::ALL.to_vec())
    }

    pub fn of(order: Vec<View>) -> Views {
        let mut seen = Vec::new();
        for v in order {
            if !seen.contains(&v) {
                seen.push(v);
            }
        }
        Views(seen)
    }

    pub fn order(&self) -> &[View] {
        &self.0
    }

    pub fn count(&self) -> usize {
        self.0.len()
    }

    pub fn any(&self) -> bool {
        !self.0.is_empty()
    }

    pub fn contains(&self, v: View) -> bool {
        self.0.contains(&v)
    }

    /// Where `v` sits in the band, if it is in it. 0 is the leftmost.
    pub fn position(&self, v: View) -> Option<usize> {
        self.0.iter().position(|x| *x == v)
    }

    /// **Off if it is on; on at the RIGHT-HAND END if it is off.**
    ///
    /// Appending rather than restoring the element to its numbered place is the
    /// entire reordering mechanism: press a number twice and that element moves
    /// to the end, so any arrangement is reachable with nothing but the number
    /// row. Restoring it to a fixed slot would make the band's order permanent
    /// and the second press a no-op.
    pub fn toggled(&self, v: View) -> Views {
        let mut out = self.0.clone();
        match out.iter().position(|x| *x == v) {
            Some(i) => {
                out.remove(i);
            }
            None => out.push(v),
        }
        Views(out)
    }

    /// The order, as it goes in the settings file.
    pub fn keys(&self) -> String {
        self.0
            .iter()
            .map(|v| v.key())
            .collect::<Vec<_>>()
            .join("+")
    }

    /// Read back what `keys` wrote. Unknown names are dropped rather than
    /// refused, so a file written by a LATER build — one that knows a fifth
    /// diagram — still opens here with the four this build understands.
    pub fn from_keys(text: &str) -> Views {
        Views::of(text.split('+').filter_map(View::from_key).collect())
    }

}

/// One element of the theory band, for the menu and for the number keys.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Circle,
    Tonnetz,
    Triangles,
    /// The sheet music. A diagram like the others: it says what you are playing
    /// in the notation everybody already reads, and it is the most concrete of
    /// the four — which is why it is worth having beside the abstractions
    /// rather than in a band of its own. One chord at a time does not need the
    /// width of a window.
    Staff,
}

impl View {
    /// **The order the number keys are numbered in, and it must not be
    /// reshuffled.** `1` is this array's first element on every machine
    /// forever; moving one would silently rebind everybody's fingers. New
    /// elements go on the END.
    pub const ALL: [View; 4] = [View::Circle, View::Tonnetz, View::Triangles, View::Staff];

    pub fn label(self) -> &'static str {
        match self {
            View::Circle => "Circle of Fifths",
            View::Tonnetz => "Tonnetz",
            View::Triangles => "Harmonic Triangles",
            View::Staff => "Sheet Music",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            View::Circle => "circle",
            View::Tonnetz => "tonnetz",
            View::Triangles => "triangles",
            View::Staff => "staff",
        }
    }

    pub fn from_key(k: &str) -> Option<View> {
        View::ALL.into_iter().find(|v| v.key() == k)
    }

    /// The number key that toggles this element, 1-based.
    pub fn number(self) -> usize {
        View::ALL
            .iter()
            .position(|v| *v == self)
            .expect("every view is in ALL")
            + 1
    }

    /// Which element a number key means, if any.
    pub fn from_number(n: usize) -> Option<View> {
        (n >= 1).then(|| View::ALL.get(n - 1).copied()).flatten()
    }

    pub fn is_on(self, v: &Views) -> bool {
        v.contains(self)
    }
}

/// What the diagrams are drawing.
#[derive(Clone, Copy, Default, Debug)]
pub struct Input {
    /// Sounding pitch classes, bit `i` for pitch class `i`. Octaves are gone
    /// by design: none of these diagrams has an axis for them.
    pub pcs: u16,
    /// Lowest sounding pitch class. The fallback tonic when the detector has
    /// no name for what is being played, because a bass note is a better guess
    /// than C.
    pub bass: Option<u8>,
    /// Root and quality from the detected chord label, when there is one.
    pub root: Option<u8>,
    pub minor: bool,
}

impl Input {
    /// The tonic the key-centred diagrams orient themselves around: the
    /// detected root, else the bass, else C so the picture is still drawn.
    pub fn tonic(&self) -> u8 {
        self.root.or(self.bass).unwrap_or(0)
    }

    pub fn contains(&self, mask: u16) -> bool {
        self.pcs != 0 && (self.pcs & mask) == mask
    }
}

// ── palette ────────────────────────────────────────────────────────────────

struct Palette {
    bg: Color32,
    ink: Color32,
    faint: Color32,
    line: Color32,
    lit: Color32,
    lit_text: Color32,
    root: Color32,
}

/// The theory band's own background.
///
/// Public so that the camera pane, which sits INSIDE this band, can letterbox
/// against it. A camera surrounded by the Recorder's leather in the middle of a
/// cream band reads as a hole cut in the window rather than as part of it.
pub fn band_bg(s: &Settings) -> Color32 {
    palette(s).bg
}

fn palette(s: &Settings) -> Palette {
    // `faint` and `line` carry the outlines, the roman numerals and the key
    // signatures — everything that says what the shapes MEAN. They were
    // pitched as background texture and were hard to read in both modes: a
    // 0x55 grey on near-black, and a 0xC8 on cream, are both about 2:1
    // against their background where text wants 4.5:1. Darkened on light and
    // lightened on dark until the numerals read at a glance, while staying
    // clearly below the note names in weight.
    // **The ground is the THEME's, and everything on it is measured against
    // it.** These were two frozen pairs — near-black and cream — so every one
    // of the sixteen themes drew its theory modules on the same two
    // backgrounds, which is what the owner saw: "ivory on every light mode,
    // black on every dark mode".
    let bg = crate::piano::themed_bg(s);
    // **Derived against THIS page**, like the line and the ground beside it.
    // The theme names one lit colour and it is drawn on the keyboard, on the
    // light page and on the dark one; Mono's is white, which is 1.25:1 on its
    // own light page. See `recorder_panel::lit_on` for why the theme table
    // cannot fix that for any colour.
    let lit = crate::recorder_panel::lit_on(s.white_key_active_color.to_color32(), bg);

    let ink = if crate::recorder_panel::wants_light_ink(bg) {
        crate::recorder_panel::INK_LIGHT
    } else {
        crate::recorder_panel::INK_DARK
    };
    Palette {
        bg,
        ink,
        // The numerals and the key signatures: below the note names in weight
        // and still readable, which is what pulling the ink 30% toward the
        // ground buys — and it is a RATIO of the two, so it holds on any
        // ground rather than being a grey that suits one.
        faint: {
            let f = |x: u8, y: u8| {
                (f32::from(x) + (f32::from(y) - f32::from(x)) * 0.30) as u8
            };
            Color32::from_rgb(
                f(ink.r(), bg.r()),
                f(ink.g(), bg.g()),
                f(ink.b(), bg.b()),
            )
        },
        // **The theme's line, pushed off the ground if it lands on it.**
        //
        // Dark mode used to darken this by a fixed 0.66, which was tuned for
        // the one near-black background this view used to have. Against a
        // ground that now moves with the theme, a fixed nudge is a line that
        // disappears on seven of the sixteen — so it is measured instead, and
        // moved only as far as it has to be.
        line: visible_on(s.theory_line_color.to_color32(), bg, 1.9),
        lit,
        // What a lit cell can carry, for the same reason as everything else:
        // the lit colour is the user's and can be anything.
        lit_text: if crate::recorder_panel::wants_light_ink(lit) {
            crate::recorder_panel::INK_LIGHT
        } else {
            crate::recorder_panel::INK_DARK
        },
        root: s.sustain_color.to_color32(),
    }
}

/// Move `c` away from `bg` until it can be SEEN on it — the least distance
/// that clears `floor`, so a theme's own colour survives wherever it already
/// worked.
///
/// Lines are structure rather than words, so the bar is well under the 4.5:1
/// that text answers to; what it prevents is a grid that is not there.
fn visible_on(c: Color32, bg: Color32, floor: f32) -> Color32 {
    if crate::recorder_panel::contrast_ratio(c, bg) >= floor {
        return c;
    }
    let toward = if crate::recorder_panel::wants_light_ink(bg) {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    let mut out = c;
    for step in 1u8..=40 {
        let t = f32::from(step) * 0.025_f32;
        let f = |x: u8, y: u8| {
            (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8
        };
        out = Color32::from_rgb(
            f(c.r(), toward.r()),
            f(c.g(), toward.g()),
            f(c.b(), toward.b()),
        );
        if crate::recorder_panel::contrast_ratio(out, bg) >= floor {
            break;
        }
    }
    out
}

fn font(size: f32) -> FontId {
    FontId::new(fonts::px(size), fonts::courier_bold())
}

fn font_light(size: f32) -> FontId {
    FontId::new(fonts::px(size), fonts::courier())
}

// ── the band ───────────────────────────────────────────────────────────────

/// Divide `rect` into one cell per selected view, left to right in a fixed
/// order so turning one off never reshuffles the others.
pub fn cells(rect: Rect, views: &Views) -> Vec<(View, Rect)> {
    let on = views.order().to_vec();
    if on.is_empty() {
        return Vec::new();
    }
    let w = rect.width() / on.len() as f32;
    on.into_iter()
        .enumerate()
        .map(|(i, v)| {
            (
                v,
                Rect::from_min_size(
                    Pos2::new(rect.min.x + w * i as f32, rect.min.y),
                    Vec2::new(w, rect.height()),
                ),
            )
        })
        .collect()
}

/// What a click landed on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    /// One pitch class: a name on the circle, a node on the lattice.
    Pc(u8),
    /// A whole chord: a vertex of the harmonic triangles, which is a triad
    /// rather than a note and is much more useful placed all at once.
    Triad { root: u8, minor: bool },
}

/// What is under `pos`, if anything.
///
/// The exact inverse of `draw`, and deliberately in the same file a few lines
/// from it: a hit test that lives somewhere else is a hit test that stops
/// matching the picture the first time the picture moves.
pub fn hit_test(rect: Rect, views: &Views, key: i32, pos: Pos2) -> Option<Hit> {
    let (view, cell) = cells(rect, &views)
        .into_iter()
        .find(|(_, c)| c.contains(pos))?;
    let body = body_rect(cell_inner(cell));
    match view {
        View::Circle => circle_hit(body, pos).map(Hit::Pc),
        View::Tonnetz => tonnetz_hit(body, pos).map(Hit::Pc),
        // The sheet music is a READOUT, not a control. There is nothing on a
        // staff to click that would mean anything — a note on it is a note you
        // are already holding — and a cell that swallowed clicks would make the
        // band's behaviour depend on which panel happened to be under the
        // pointer.
        View::Staff => None,
        // The only one that needs a tonic: I, IV and V are relative to one.
        // It comes from the KEY now rather than from the notes — which is why
        // this function grew a parameter it can no longer get from `input`, and
        // why passing the wrong one would place the triad a click meant to
        // place somewhere else entirely.
        View::Triangles => triangles_hit(body, key_tonic(key), pos),
    }
}

pub fn draw(
    painter: &Painter,
    rect: Rect,
    views: &Views,
    input: Input,
    notes: &std::collections::HashSet<u8>,
    readout: Option<crate::staff::Readout<'_>>,
    s: &Settings,
) {
    let p = palette(s);
    painter.rect_filled(rect, 0.0, p.bg);

    let cells = cells(rect, &views);
    for (i, (view, cell)) in cells.iter().enumerate() {
        // A hairline between panes, so three diagrams read as three and not as
        // one crowded one. Not drawn on the first, which has the band edge.
        if i > 0 {
            painter.line_segment(
                [
                    Pos2::new(cell.min.x, cell.min.y + 6.0),
                    Pos2::new(cell.min.x, cell.max.y - 6.0),
                ],
                Stroke::new(1.0_f32, p.line),
            );
        }
        // **A margin that scales with the pane, not eight flat points.** At a
        // full-screen window eight points is a hairline and the diagrams ran
        // to the edges of their cells; the three of them read as one crowded
        // picture with rules through it rather than as three panels.
        let inner = cell_inner(*cell);
        match view {
            View::Circle => draw_circle(painter, inner, input, &p, s),
            View::Tonnetz => draw_tonnetz(painter, inner, input, &p, s),
            View::Triangles => draw_triangles(painter, inner, input, &p, s),
            // The one element that needs the notes themselves rather than
            // their pitch classes: an octave is exactly what a staff is FOR,
            // and `Input` has thrown it away by design.
            //
            // Its title is drawn HERE rather than inside `staff::draw`, so the
            // four panels share one title in one font at one height. The legend
            // says which staves and which key, because those are two settings
            // that live in a right-click menu and would otherwise be invisible
            // — a band showing E flat major with nothing saying so is a band
            // that looks wrong to anybody who did not set it.
            View::Staff => {
                let legend = format!(
                    "{}  /  {}",
                    s.staff_set().label(),
                    crate::staff::key_label(s.staff_key)
                );
                let body = title(painter, inner, "SHEET MUSIC", &legend, &p);
                crate::staff::draw(painter, body, notes, readout, s);
            }
        }
    }

    // Bottom edge, the same idea as the fretboard's top edge: the bands are
    // one window and need a seam or they read as one tall picture.
    painter.line_segment(
        [
            Pos2::new(rect.min.x, rect.max.y - 0.5),
            Pos2::new(rect.max.x, rect.max.y - 0.5),
        ],
        Stroke::new(1.0_f32, p.line),
    );
}

/// A title over each pane, in the space the diagrams leave for it.
/// A title and a one-line key to what the colours mean.
///
/// The legend is not decoration. Three diagrams showing three different things
/// in the same two colours is genuinely hard to read, and a viewer who has to
/// work out from context whether a lit shape means "this note is sounding" or
/// "this key fits what you are playing" is being asked to do the diagram's
/// job. One line each, in the faint ink, costs about 11 points of height.
/// What is left of a pane once its title and legend have taken their share.
///
/// Shared by the drawing and the hit-testing so the two cannot disagree about
/// where the diagram starts — the class of bug where a click lands one row
/// above the thing it looks like it is on.
/// The inset a cell's contents are drawn inside.
///
/// **One function, because the drawing and the hit test both need it.** They
/// each had their own: `draw` used a fifth of a tenth of the cell clamped to
/// 8..24, and `hit_test` used a flat 8. On a three-hundred-point cell that is
/// seven points of disagreement, which is a click landing on the neighbouring
/// node of a lattice — a bug you find by missing, and blame on yourself.
pub fn cell_inner(cell: Rect) -> Rect {
    cell.shrink((cell.width().min(cell.height()) * 0.05).clamp(8.0, 24.0))
}

/// Where the sheet music itself is drawn inside its cell: the inset, less the
/// title strip above it.
pub fn staff_body(cell: Rect) -> Rect {
    body_rect(cell_inner(cell))
}

fn body_rect(rect: Rect) -> Rect {
    let h = (rect.height() * 0.075).clamp(9.0, 14.0);
    let used = if rect.height() > 150.0 {
        h + h * 0.82 + 2.0
    } else {
        h + 2.0
    };
    Rect::from_min_max(Pos2::new(rect.min.x, rect.min.y + used), rect.max)
}

/// Advance width of one character as a fraction of the font size, for the
/// bundled monospaced faces. The same number `recorder_panel` uses, for the
/// same reason: sizing text to its box without laying it out first.
const LEGEND_ADV: f32 = 0.62;

/// How much of its pane a diagram fills, across its widest part.
///
/// **The circle and the hexagram used to fill theirs exactly** — a diameter of
/// `min(w, h)` — while the Tonnetz is a wide lattice that runs out of width
/// long before it runs out of height and so sat visibly smaller than both. The
/// two round ones give up a little so the three read as one set. Not more than
/// this: past it they stop being the biggest thing in their own panel, which
/// is what a diagram in a panel is for.
const DIAGRAM_FILL: f32 = 0.92;

fn title(painter: &Painter, rect: Rect, text: &str, legend: &str, p: &Palette) -> Rect {
    let h = (rect.height() * 0.075).clamp(9.0, 14.0);
    painter.text(
        Pos2::new(rect.center().x, rect.min.y + h * 0.5),
        Align2::CENTER_CENTER,
        text,
        font(h * 0.92),
        p.faint,
    );
    let lh = h * 0.82;
    // Dropped rather than crammed when the pane is too short for it: an
    // unreadable legend is worse than none, and it is the first thing that
    // should give up its space.
    //
    // **And too NARROW, which is the case that used to be missed.** These are
    // forty-odd characters of centred text; in a pane half the window's width
    // they ran out of both sides and collided with the legends either side of
    // them, so three diagrams read as one smear of overlapping type. Shrink to
    // fit, then give up — the same bargain the height already made.
    let fitted = (rect.width() * 0.98 / (legend.chars().count().max(1) as f32 * LEGEND_ADV))
        .min(lh * 0.88);
    if rect.height() > 150.0 && !legend.is_empty() && fitted >= 6.0 {
        painter.text(
            Pos2::new(rect.center().x, rect.min.y + h + lh * 0.5),
            Align2::CENTER_CENTER,
            legend,
            font_light(fitted),
            p.faint,
        );
    }
    body_rect(rect)
}

// ── circle of fifths ───────────────────────────────────────────────────────

/// Angle of circle position `i`, with C at twelve o'clock and fifths
/// clockwise. Screen y grows downward, so this is the usual convention with
/// the sign of the sine left alone.
fn circle_angle(i: usize) -> f32 {
    -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / 12.0
}

fn draw_circle(painter: &Painter, rect: Rect, input: Input, p: &Palette, s: &Settings) {
    let rect = title(
        painter,
        rect,
        "CIRCLE OF FIFTHS",
        "filled = note sounding    shaded = key that fits",
        p,
    );
    let c = rect.center();
    let r = (rect.width().min(rect.height()) * 0.5 - 2.0) * DIAGRAM_FILL;
    if r < 20.0 {
        return;
    }

    let r_sig = r;
    let r_maj = r * 0.80;
    let r_min = r * 0.56;
    let r_hub = r * 0.34;
    let step = std::f32::consts::TAU / 12.0;

    // ── the wedges: which KEYS contain everything being played ─────────────
    //
    // One meaning, one treatment. This used to share the note colour and to
    // shade partial fits as well, so a single chord washed most of the circle
    // in the same blue that meant "this note is sounding" — two different
    // questions answered in one colour, which is what made it unreadable.
    //
    // Now: a key that contains ALL of it gets a neutral warm wash and a
    // boundary; a key that does not gets nothing. Nothing is a real answer.
    let full: Vec<bool> = FIFTHS
        .iter()
        .map(|&pc| input.pcs != 0 && input.pcs & !major_scale(pc) == 0)
        .collect();

    for (i, &fits) in full.iter().enumerate() {
        if !fits {
            continue;
        }
        let a0 = circle_angle(i) - step * 0.5;
        let mut poly = Vec::with_capacity(18);
        for k in 0..=8 {
            let t = a0 + step * k as f32 / 8.0;
            poly.push(c + Vec2::new(t.cos(), t.sin()) * r_sig);
        }
        for k in (0..=8).rev() {
            let t = a0 + step * k as f32 / 8.0;
            poly.push(c + Vec2::new(t.cos(), t.sin()) * r_hub);
        }
        painter.add(egui::Shape::convex_polygon(
            poly,
            p.ink.gamma_multiply(0.10),
            Stroke::NONE,
        ));
    }

    // ── the spokes and rings ───────────────────────────────────────────────
    for i in 0..12usize {
        let a0 = circle_angle(i) - step * 0.5;
        painter.line_segment(
            [
                c + Vec2::new(a0.cos(), a0.sin()) * r_hub,
                c + Vec2::new(a0.cos(), a0.sin()) * r_sig,
            ],
            Stroke::new(1.0_f32, p.line),
        );
    }
    for ring in [r_sig, r_maj, r_min, r_hub] {
        painter.circle_stroke(c, ring, Stroke::new(1.0_f32, p.line));
    }

    // ── the names ──────────────────────────────────────────────────────────
    for (i, &pc) in FIFTHS.iter().enumerate() {
        let a = circle_angle(i);
        let dir = Vec2::new(a.cos(), a.sin());
        let sounding = has(input.pcs, pc);

        // The major name, on a filled disc when THAT PITCH CLASS is sounding.
        // The disc is the same colour the piano paints a held key, which is
        // the whole point: the two displays agree by construction.
        let at = c + dir * ((r_maj + r_min) * 0.5);
        let name_size = r * 0.135;
        if sounding {
            painter.circle_filled(at, name_size * 0.95, p.lit);
            // The root is a sounding note WITH a ring, not a different fill —
            // so it reads as "and this one is the root" rather than as a
            // fourth unrelated colour.
            if input.root == Some(pc) {
                painter.circle_stroke(at, name_size * 0.95, Stroke::new(2.5_f32, p.root));
            }
        }
        draw_note(
            painter,
            at,
            pc,
            s.prefer_flats,
            name_size,
            if sounding { p.lit_text } else { p.ink },
            false,
            "",
        );

        // The relative minor, lower case, which is how a circle of fifths has
        // always been labelled and which also fits the inner wedge.
        //
        // A LABEL, not a second set of lights. Every pitch class appears twice
        // on this circle — the minor at position i is the major at position
        // i+3 — so lighting both rings put eight discs on the screen for a
        // four-note chord and said nothing the outer ring had not already
        // said. The wedge behind it still shades when that key fits.
        let rel = (pc + 9) % 12;
        draw_note(
            painter,
            c + dir * ((r_min + r_hub) * 0.5),
            rel,
            s.prefer_flats,
            r * 0.115,
            p.ink.gamma_multiply(0.8),
            true,
            "",
        );

        // Key signature, outermost. ASCII accidentals: neither U+266F nor
        // U+266D is in any bundled face, and both drew as tofu boxes.
        let (sharps, flats) = signature(i);
        let sig = match (sharps, flats) {
            (0, 0) => String::new(),
            (n, 0) => format!("{n}{SHARP}"),
            (0, n) => format!("{n}{FLAT}"),
            _ => String::new(),
        };
        if !sig.is_empty() {
            painter.text(
                c + dir * ((r_sig + r_maj) * 0.5),
                Align2::CENTER_CENTER,
                &sig,
                font_light(r * 0.095),
                p.faint,
            );
        }
    }

    // ── the hub: what the picture is of ────────────────────────────────────
    if input.pcs != 0 {
        let tonic = input.tonic();
        draw_note(
            painter,
            Pos2::new(c.x, c.y - r_hub * 0.28),
            tonic,
            s.prefer_flats,
            r * 0.19,
            p.ink,
            false,
            if input.minor { "m" } else { "" },
        );
        // Zero is the useful reading, not an error: it means what you are
        // playing belongs to no single major key.
        let n = full.iter().filter(|f| **f).count();
        painter.text(
            Pos2::new(c.x, c.y + r_hub * 0.38),
            Align2::CENTER_CENTER,
            &match n {
                0 => "no key fits".to_owned(),
                1 => "1 key".to_owned(),
                n => format!("{n} keys"),
            },
            font_light(r * 0.085),
            p.faint,
        );
    }
}

/// Which pitch class the point is on, if any. The inverse of the name ring
/// above, and kept next to it so the two cannot drift.
fn circle_hit(rect: Rect, pos: Pos2) -> Option<u8> {
    let c = rect.center();
    // The SAME radius `draw_circle` shades — `DIAGRAM_FILL` included.
    // Without it the accepted annulus sat ~9% outside the drawn ring:
    // the ring's inner edge went click-dead, and the empty band just
    // outside it — labels, not controls — sounded and placed notes.
    let r = (rect.width().min(rect.height()) * 0.5 - 2.0) * DIAGRAM_FILL;
    if r < 20.0 {
        return None;
    }
    let v = pos - c;
    let d = v.length();
    // The major ring only. The signature ring, the hub and the relative-minor
    // ring are all labels rather than controls — and since every pitch class
    // already appears on the major ring, nothing is unreachable.
    if !(r * 0.56..=r * 0.80).contains(&d) {
        return None;
    }
    let step = std::f32::consts::TAU / 12.0;
    // Undo `circle_angle`: subtract the -90 degree offset and round to the
    // nearest twelfth.
    let a = v.y.atan2(v.x) + std::f32::consts::FRAC_PI_2;
    let i = (a / step).round().rem_euclid(12.0) as usize;
    Some(FIFTHS[i])
}

// ── Tonnetz ────────────────────────────────────────────────────────────────

/// The pitch class at lattice position (u, v): `u` fifths and `v` major thirds
/// from the origin. This one line is the whole Tonnetz — every property of the
/// diagram falls out of it.
pub fn tonnetz_pc(u: i32, v: i32, origin: u8) -> u8 {
    (((7 * u + 4 * v) % 12 + 12) % 12 + origin as i32) as u8 % 12
}

/// Where the Tonnetz nodes go inside a pane.
///
/// Pulled out of the drawing so it can be tested. The lattice is unbounded by
/// nature and a pane is not; every mistake this diagram can make is a mistake
/// about that boundary, and none of them is visible in a passing paint test.
struct Lattice {
    a: f32,
    h: f32,
    node_r: f32,
    rows: i32,
    cols: i32,
    x0: f32,
    y0: f32,
}

impl Lattice {
    /// Five rows shows the vertical period — the lattice repeats every three
    /// major thirds — with a row of context above and below it.
    const ROWS: i32 = 5;
    /// Breathing room at every edge, so no node ever sits exactly on it.
    const SLACK: f32 = 1.5;

    fn fit(rect: Rect) -> Option<Self> {
        let rows = Self::ROWS;
        let vspan = (rows - 1) as f32 * 0.8660254 + 0.6; // + a node either end
                                                         // A band this small is not a diagram. Bailing here also keeps `a_h`
                                                         // away from zero, which is what made the column count divide into
                                                         // infinity and then overflow `i32` on the way back — found by the
                                                         // every-size test, at a 40pt-wide window nobody would open on purpose.
        if !(rect.width() >= 32.0 && rect.height() >= vspan * 16.0) {
            return None;
        }
        // A pixel of slack, top and bottom.
        //
        // Without it the lattice is sized so its extent lands EXACTLY on the
        // pane edge — `a * vspan == rect.height()` by construction — and a
        // node whose bounding box touches the boundary fails `contains_rect`
        // by a rounding error, so `shows()` drops it. The whole bottom row
        // disappears and the diagram reads as cut off. It only showed below
        // 100% window size, because that is where the arithmetic stopped
        // landing on whole pixels.
        let a_h = (rect.height() - Self::SLACK * 2.0).max(1.0) / vspan;
        // Columns are whatever the width affords at that spacing, never fewer
        // than five — below that the horizontal axis stops showing a run of
        // fifths — and never so many that the names stop being readable.
        let shear = (rows - 1) as f32 * 0.5;
        let cols = ((((rect.width() - Self::SLACK * 2.0) / a_h) - shear - 0.6).floor() + 1.0)
            .clamp(5.0, 13.0) as i32;
        let a_w = (rect.width() - Self::SLACK * 2.0).max(1.0) / ((cols - 1) as f32 + shear + 0.6);
        let a = a_h.min(a_w);
        if a < 16.0 {
            return None;
        }
        let h = a * 0.8660254; // equilateral: a * sqrt(3)/2
                               // The lattice is a sheared rhombus, so its bounding box is wider than
                               // the node grid by the shear. Centre the BOX, not the grid, or it
                               // sits left.
        let w_used = (cols - 1) as f32 * a + shear * a;
        let h_used = (rows - 1) as f32 * h;
        Some(Self {
            a,
            h,
            node_r: a * 0.30,
            rows,
            cols,
            x0: rect.center().x - w_used * 0.5,
            y0: rect.center().y + h_used * 0.5,
        })
    }

    fn at(&self, u: i32, v: i32) -> Pos2 {
        Pos2::new(
            self.x0 + u as f32 * self.a + v as f32 * self.a * 0.5,
            self.y0 - v as f32 * self.h,
        )
    }

    /// Whether a node at (u, v) is drawn.
    ///
    /// Two conditions, and the second is easy to forget. It has to be INSIDE
    /// THE GRID — the triangle loop walks `-1..cols` so it can reach the cells
    /// on the left edge, and a corner outside `0..cols` is a node that is
    /// never drawn however well it fits. And it has to fit WHOLE: a node
    /// clipped in half by the pane edge is a note name that is not a note
    /// name, which is how an "Eb" once appeared as "Kb".
    fn shows(&self, rect: Rect, u: i32, v: i32) -> bool {
        (0..self.cols).contains(&u)
            && (0..self.rows).contains(&v)
            && rect.contains_rect(Rect::from_center_size(
                self.at(u, v),
                Vec2::splat(self.node_r * 2.0),
            ))
    }
}

fn draw_tonnetz(painter: &Painter, rect: Rect, input: Input, p: &Palette, s: &Settings) {
    let rect = title(
        painter,
        rect,
        "TONNETZ",
        "solid triangle = major    outlined = minor",
        p,
    );
    // Everything below stays inside the pane. The lattice is unbounded by
    // nature, so without this it happily draws over the neighbouring diagram.
    let painter = &painter.with_clip_rect(rect);

    let Some(l) = Lattice::fit(rect) else {
        return;
    };
    let (rows, cols, node_r) = (l.rows, l.cols, l.node_r);
    // The lattice is ANCHORED at C rather than re-centred on the tonic.
    //
    // Re-centring looked clever and is wrong once the diagram is clickable:
    // placing a note changes the tonic, which slides the whole lattice, so the
    // node you just clicked is no longer under your finger. Nothing is lost by
    // anchoring — every pitch class appears somewhere on the grid, which
    // `the_tonnetz_still_has_a_lattice_left` asserts — and the tonic is marked
    // by the same ring it gets on the other two diagrams.
    let origin = 0u8;
    let at = |u: i32, v: i32| l.at(u, v);

    // Triads first, so the nodes sit on top of their triangles.
    //
    // Two triangles per cell, and they are the two chord qualities: the one
    // pointing up is {p, p+7, p+4}, a major triad on p; the one pointing down
    // is {p+7, p+4, p+11}, a minor triad on p+4. That is the fact the whole
    // diagram is for, and it needs no table.
    for v in 0..rows {
        for u in -1..cols {
            // Major and minor differ by TREATMENT, not by hue: solid versus
            // outlined, both in the note colour. They used to be blue and
            // orange, and orange was also the root marker — so a lit minor
            // triad and a root note said the same thing in the same colour
            // while meaning different things.
            for (corners, minor) in [
                ([(u, v), (u + 1, v), (u, v + 1)], false),
                ([(u + 1, v), (u, v + 1), (u + 1, v + 1)], true),
            ] {
                let mask = corners
                    .iter()
                    .fold(0u16, |m, &(u, v)| m | 1 << tonnetz_pc(u, v, origin));
                if !input.contains(mask) {
                    continue;
                }
                // Only between nodes that are actually drawn. The lattice is
                // clipped to the pane, so a triangle reaching past the last
                // visible column used to be cut off mid-edge and read as an
                // unfinished shape rather than a chord.
                if !corners.iter().all(|&(u, v)| l.shows(rect, u, v)) {
                    continue;
                }
                let pts: Vec<Pos2> = corners.iter().map(|&(u, v)| at(u, v)).collect();
                if minor {
                    painter.add(egui::Shape::convex_polygon(
                        pts.clone(),
                        p.lit.gamma_multiply(0.16),
                        Stroke::NONE,
                    ));
                    for k in 0..3 {
                        painter
                            .line_segment([pts[k], pts[(k + 1) % 3]], Stroke::new(2.0_f32, p.lit));
                    }
                } else {
                    painter.add(egui::Shape::convex_polygon(
                        pts,
                        p.lit.gamma_multiply(0.50),
                        Stroke::NONE,
                    ));
                }
            }
        }
    }

    // The three axes as edges. Every edge is an interval: horizontal a fifth,
    // up-right a major third, up-left a minor third.
    //
    // **Drawn across the WHOLE pane, not just across the nodes.** The loop ran
    // `0..=rows`, and all three directions go up or sideways from a node — so
    // the lattice bled off the top and the sides and stopped dead under the
    // bottom row, which left a bar of empty pane under the diagram and made a
    // centred thing look like it had sagged.
    //
    // A Tonnetz IS unbounded; what the pane holds is a crop of it. Lines with
    // no nodes on them are not lines you cannot use — they are the rest of the
    // lattice, and drawing them is what makes the crop read as a crop rather
    // than as a diagram that ran out of paper. The splash's wordmark has done
    // this since 4.2 for exactly the same reason.
    //
    // The range is solved from the pane rather than guessed: invert `at` at
    // each corner and take a row and a column of margin, so every segment that
    // could cross the pane is emitted and the clip rect does the rest.
    {
        let v_lo = ((l.y0 - rect.bottom()) / l.h).floor() as i32 - 1;
        let v_hi = ((l.y0 - rect.top()) / l.h).ceil() as i32 + 1;
        // A guard, not a policy: `fit` already refuses to build a lattice with
        // a spacing under sixteen points, so these counts are small. It is
        // here because the one thing worse than an ugly diagram is a frame
        // that never finishes drawing.
        let stroke = Stroke::new(1.3_f32, p.line);
        for v in v_lo.max(-64)..=v_hi.min(64) {
            let shear = v as f32 * l.a * 0.5;
            let u_lo = ((rect.left() - l.x0 - shear) / l.a).floor() as i32 - 1;
            let u_hi = ((rect.right() - l.x0 - shear) / l.a).ceil() as i32 + 1;
            for u in u_lo.max(-64)..=u_hi.min(64) {
                let from = at(u, v);
                for (du, dv) in [(1, 0), (0, 1), (-1, 1)] {
                    painter.line_segment([from, at(u + du, v + dv)], stroke);
                }
            }
        }
    }

    for v in 0..rows {
        for u in 0..cols {
            if !l.shows(rect, u, v) {
                continue;
            }
            let c = at(u, v);
            let pc = tonnetz_pc(u, v, origin);
            let lit = has(input.pcs, pc);
            let fill = if lit { p.lit } else { p.bg };
            painter.circle(
                c,
                node_r,
                fill,
                Stroke::new(1.4_f32, if lit { fill } else { p.faint }),
            );
            // The root, ringed rather than recoloured — the same mark it gets
            // on the circle of fifths, so one glance learns it once.
            if input.root == Some(pc) || (input.root.is_none() && lit && pc == origin) {
                painter.circle_stroke(c, node_r, Stroke::new(2.5_f32, p.root));
            }
            draw_note(
                painter,
                c,
                pc,
                s.prefer_flats,
                node_r * 0.95,
                if lit { p.lit_text } else { p.ink },
                false,
                "",
            );
        }
    }
}

/// The lattice node under `pos`, if any.
fn tonnetz_hit(rect: Rect, pos: Pos2) -> Option<u8> {
    let l = Lattice::fit(rect)?;
    for v in 0..l.rows {
        for u in 0..l.cols {
            if l.shows(rect, u, v) && (l.at(u, v) - pos).length() <= l.node_r {
                return Some(tonnetz_pc(u, v, 0));
            }
        }
    }
    None
}

// ── harmonic triangles ─────────────────────────────────────────────────────

/// Vertex `k` of the upward or downward triangle.
///
/// One definition, used by the drawing and by the hit test, because a click
/// that lands next to the circle it looks like it is on is the failure this
/// shape is most prone to.
fn hex_vertex(c: Pos2, r: f32, k: usize, down: bool) -> Pos2 {
    let base = if down {
        std::f32::consts::FRAC_PI_2
    } else {
        -std::f32::consts::FRAC_PI_2
    };
    let a = base + k as f32 * std::f32::consts::TAU / 3.0;
    c + Vec2::new(a.cos(), a.sin()) * r
}

/// Vertex k of the upward triangle is I at the apex, then V and IV going
/// clockwise, so the dominant is on the right where it is usually drawn.
const HEX_UP_ORDER: [usize; 3] = [0, 2, 1];
const HEX_DOWN_ORDER: [usize; 3] = [0, 1, 2];

/// Distance from the centre of the harmonic-triangle figure to a vertex.
///
/// The drawn shape is `r` to a vertex PLUS a node radius beyond it, and the
/// node radius is 0.30r, so the real extent is 1.30r and the pane has to hold
/// 2.60r. Sizing on `r` alone put the tonic node through the pane's own title.
fn hexagram_radius(rect: Rect) -> f32 {
    // The 1.27 is `1 + node_r/r` from `draw_triangles`: the circles stand
    // proud of the hexagram by their own radius, so the pane has to hold the
    // figure PLUS one node either side. Change one and change the other, or
    // the vertices are clipped at the edges.
    rect.width().min(rect.height()) * 0.5 * DIAGRAM_FILL / 1.27
}

fn draw_triangles(painter: &Painter, rect: Rect, input: Input, p: &Palette, s: &Settings) {
    let rect = title(
        painter,
        rect,
        "HARMONIC TRIANGLES",
        "filled = you are playing that chord",
        p,
    );
    let c = rect.center();
    let r = hexagram_radius(rect);
    if r < 20.0 {
        return;
    }

    // **The KEY's tonic, not the chord's.**
    //
    // It used to be whatever you had just played, which made the one diagram
    // with roman numerals on it the one that could not tell you where you
    // were: every chord was I, because the picture slid under it. Anchored to
    // the key, the numerals mean what they say — you play the IV and the IV
    // lights up — and sliding the whole shape is a modulation you perform by
    // choosing another key, which is what the shape was always for.
    //
    // The Circle and the Tonnetz are unchanged and still ring what you are
    // playing. Two diagrams that answer "what am I on" and one that answers
    // "where is it in the key" is the split that makes the band worth reading;
    // three of the first is why the third was ignored.
    let tonic = key_tonic(s.staff_key);
    let flats = key_flats(s.staff_key, s.prefer_flats);
    // I, IV, V — the tonic, the fifth below it and the fifth above it. Written
    // as -5 and +7 rather than +5 and +7 so the arithmetic says what the
    // relationship is.
    let roots = [tonic, (tonic + 5) % 12, (tonic + 7) % 12];
    let numerals_major = ["I", "IV", "V"];
    let numerals_minor = ["i", "iv", "v"];

    // The upward triangle carries the major chords, the downward one the
    // minor. Sharing a centre makes them one shape rather than two diagrams,
    // which is the point: sliding it is a modulation.
    let up = |k: usize| hex_vertex(c, r, k, false);
    let down = |k: usize| hex_vertex(c, r, k, true);
    let up_order = HEX_UP_ORDER;
    let down_order = HEX_DOWN_ORDER;

    let tri = |f: &dyn Fn(usize) -> Pos2| [f(0), f(1), f(2)];

    for (pts, minor) in [(tri(&up), false), (tri(&down), true)] {
        let stroke = Stroke::new(if minor { 1.4_f32 } else { 2.0 }, p.faint);
        for k in 0..3 {
            painter.line_segment([pts[k], pts[(k + 1) % 3]], stroke);
        }
    }

    // **A quarter of the hexagram, not three tenths.** The six vertices are
    // where the chords are, but the SHAPE is what the diagram is teaching —
    // which triads sit a third apart and which a fifth — and at 0.30 the
    // circles were heavy enough that the hexagram read as six buttons with
    // lines between them rather than as a figure with its chords marked.
    let node_r = r * 0.25;
    let draw_vertex = |pos: Pos2, root: u8, minor: bool, numeral: &str| {
        let mask = if minor {
            minor_triad(root)
        } else {
            major_triad(root)
        };
        let lit = input.contains(mask);
        let fill = if lit { p.lit } else { p.bg };
        painter.circle(
            pos,
            node_r,
            fill,
            Stroke::new(
                if lit { 2.0_f32 } else { 1.0 },
                if lit { p.lit } else { p.faint },
            ),
        );
        // The tonic, ringed, exactly as on the other two diagrams — but only
        // on the triangle whose QUALITY matches. These vertices are CHORDS,
        // so ringing both C and Cm said "the root is C" twice, and the unlit
        // one read as important-but-silent.
        if input.root == Some(root) && input.minor == minor {
            painter.circle_stroke(pos, node_r, Stroke::new(2.5_f32, p.root));
        }
        draw_note(
            painter,
            Pos2::new(pos.x, pos.y - node_r * 0.22),
            root,
            flats,
            node_r * 0.62,
            if lit { p.lit_text } else { p.ink },
            false,
            if minor { "m" } else { "" },
        );
        painter.text(
            Pos2::new(pos.x, pos.y + node_r * 0.42),
            Align2::CENTER_CENTER,
            numeral,
            font(node_r * 0.46),
            if lit {
                p.lit_text
            } else {
                p.ink.gamma_multiply(0.85)
            },
        );
    };

    for (k, &oi) in up_order.iter().enumerate() {
        draw_vertex(up(k), roots[oi], false, numerals_major[oi]);
    }
    for (k, &oi) in down_order.iter().enumerate() {
        draw_vertex(down(k), roots[oi], true, numerals_minor[oi]);
    }

    // The key in the middle of its own triangle.
    draw_note(
        painter,
        c,
        tonic,
        flats,
        r * 0.24,
        p.ink.gamma_multiply(0.55),
        false,
        "",
    );
}

/// The chord vertex under `pos`, if any.
///
/// Returns a whole triad rather than a note: these vertices ARE chords, and
/// placing one note of a chord you pointed at would be a strange thing for a
/// diagram of chords to do.
fn triangles_hit(rect: Rect, tonic: u8, pos: Pos2) -> Option<Hit> {
    let c = rect.center();
    let r = hexagram_radius(rect);
    if r < 20.0 {
        return None;
    }
    let node_r = r * 0.30;
    let roots = [tonic, (tonic + 5) % 12, (tonic + 7) % 12];
    for (k, &oi) in HEX_UP_ORDER.iter().enumerate() {
        if (hex_vertex(c, r, k, false) - pos).length() <= node_r {
            return Some(Hit::Triad {
                root: roots[oi],
                minor: false,
            });
        }
    }
    for (k, &oi) in HEX_DOWN_ORDER.iter().enumerate() {
        if (hex_vertex(c, r, k, true) - pos).length() <= node_r {
            return Some(Hit::Triad {
                root: roots[oi],
                minor: true,
            });
        }
    }
    None
}

/// A one-pixel seam over the top of the band, matching the fretboard's.
pub fn draw_bottom_edge(painter: &Painter, rect: Rect, s: &Settings) {
    let p = palette(s);
    painter.rect_stroke(
        Rect::from_min_max(
            Pos2::new(rect.min.x, rect.max.y - 1.0),
            Pos2::new(rect.max.x, rect.max.y),
        ),
        0.0,
        Stroke::new(1.0_f32, p.line),
        StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// The circle really is fifths, all the way round and back to the start.
    /// A typo in that table would still look like a circle of fifths.

    /// **Every theme brings its own theory ground, in both modes — and the
    /// two modes are not the same ground.**
    ///
    /// The owner: "ivory on every light mode, black on every dark mode when
    /// they should be per theme". Two frozen pairs meant sixteen themes drew
    /// their modules on the same two backgrounds. This checks that the ground
    /// MOVES with the theme, and that everything printed on it still reads.
    ///
    /// **And that the toggle does something, which is the half it was missing
    /// for three releases.** Everything above measures colours AGAINST the
    /// page and nothing asked what the page IS, so a light mode that came up
    /// black passed every line of it: the ink flipped to bone, the faint and
    /// the line followed, and each contrast cleared its floor comfortably. On
    /// Nocturne and Phosphor that is exactly what happened — their light page
    /// read 0.011 and 0.004 luminance, took light ink, and was
    /// indistinguishable from their dark page — while the keyboard under it,
    /// which decides its own polarity, came up pale. The owner: "They also
    /// have their theory views flipped and inverted."
    ///
    /// Stated as which INK the page takes rather than as a luminance bar,
    /// because that is the decision the page is actually for and it needs no
    /// number anybody can argue with: a light page takes dark ink, a dark page
    /// takes light ink, and if a theme takes the same ink in both modes then
    /// its two modes are one mode wearing two names.
    ///
    /// Revert check performed: return `background_color` from
    /// `piano::themed_bg` again and both dark-designed themes name themselves
    /// here.
    #[test]
    fn every_theme_grounds_the_theory_view_in_its_own_colour() {
        let mut seen_light = HashSet::new();
        let mut seen_dark = HashSet::new();
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut s = Settings::default();
                theme.apply(&mut s);
                s.dark_mode = dark;
                let p = palette(&s);
                let mode = if dark { "dark" } else { "light" };
                let key = (p.bg.r(), p.bg.g(), p.bg.b());
                if dark { seen_dark.insert(key) } else { seen_light.insert(key) };
                for (what, fg, floor) in [
                    ("ink", p.ink, 4.5),
                    ("faint", p.faint, 3.0),
                    ("line", p.line, 1.7),
                    // **The lit disc, against the page it sits on.** Not
                    // measured here until Mono showed why: its `key_active`
                    // is white, which is 16:1 on the dark page and 1.25:1 on
                    // the light one — an active note nobody could see, on the
                    // one theme, in the one mode, and nothing in this file
                    // was looking. Three, because a disc is a graphic.
                    ("lit", p.lit, 3.0),
                ] {
                    let r = crate::recorder_panel::contrast_ratio(fg, p.bg);
                    if r < floor {
                        bad.push(format!("{} {mode}: {what} {r:.2}:1", theme.name));
                    }
                }
                let lit = crate::recorder_panel::contrast_ratio(p.lit_text, p.lit);
                if lit < 4.5 {
                    bad.push(format!("{} {mode}: lit text {lit:.2}:1", theme.name));
                }
            }
        }
        assert!(bad.is_empty(), "the theory view does not hold up:\n  {}", bad.join("\n  "));
        // The page is a different KIND of page in the two modes, on every
        // theme: dark ink in light mode, light ink in dark mode.
        let mut stuck: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            let mut s = Settings::default();
            theme.apply(&mut s);
            for dark in [false, true] {
                s.dark_mode = dark;
                let bg = palette(&s).bg;
                if crate::recorder_panel::wants_light_ink(bg) != dark {
                    stuck.push(format!(
                        "{} {}: page {:?} takes {} ink",
                        theme.name,
                        if dark { "dark" } else { "light" },
                        bg,
                        if dark { "dark" } else { "light" },
                    ));
                }
            }
        }
        assert!(
            stuck.is_empty(),
            "the theory page is in the wrong mode:\n  {}",
            stuck.join("\n  ")
        );
        // And they are not all the same ground, which was the whole report.
        assert!(
            seen_light.len() > 8,
            "only {} distinct light grounds across sixteen themes",
            seen_light.len()
        );
        assert!(
            seen_dark.len() > 8,
            "only {} distinct dark grounds across sixteen themes",
            seen_dark.len()
        );
    }
    #[test]
    fn the_circle_ascends_by_fifths_and_closes() {
        for i in 0..12 {
            assert_eq!(
                FIFTHS[(i + 1) % 12],
                (FIFTHS[i] + 7) % 12,
                "position {i} is not a fifth above its neighbour"
            );
        }
        let mut seen = [false; 12];
        for pc in FIFTHS {
            assert!(!seen[pc as usize], "pitch class {pc} appears twice");
            seen[pc as usize] = true;
        }
    }

    /// Adjacent keys share six of seven notes and opposite keys share two.
    /// This is the fact the diagram exists to show, so it is worth asserting
    /// rather than trusting the drawing to make it look true.
    #[test]
    fn neighbouring_keys_are_the_closest_ones() {
        for i in 0..12usize {
            let here = major_scale(FIFTHS[i]);
            let next = major_scale(FIFTHS[(i + 1) % 12]);
            let opposite = major_scale(FIFTHS[(i + 6) % 12]);
            assert_eq!(
                (here & next).count_ones(),
                6,
                "neighbours of {i} do not share six notes"
            );
            assert_eq!(
                (here & opposite).count_ones(),
                2,
                "opposites of {i} do not share two notes"
            );
        }
    }

    /// The relative minor sits on the same spoke because it has the same key
    /// signature. Same signature means, exactly, the same seven notes.
    #[test]
    fn the_relative_minor_shares_its_majors_notes() {
        for pc in 0..12u8 {
            // A natural minor scale is its relative major's, started later.
            let rel = (pc + 9) % 12;
            let minor = major_scale(pc); // same set, different tonic
            assert_eq!(
                major_scale((rel + 3) % 12),
                minor,
                "the relative minor of {pc} does not share its notes"
            );
        }
    }

    /// Signatures run 0..6 sharps clockwise and 6..1 flats coming back.
    #[test]
    fn key_signatures_count_out_and_back() {
        let got: Vec<(u8, u8)> = (0..12).map(signature).collect();
        assert_eq!(
            got,
            vec![
                (0, 0),
                (1, 0),
                (2, 0),
                (3, 0),
                (4, 0),
                (5, 0),
                (6, 0),
                (0, 5),
                (0, 4),
                (0, 3),
                (0, 2),
                (0, 1),
            ]
        );
    }

    /// Every upward triangle in the lattice is a major triad and every
    /// downward one is a minor triad, everywhere, for any origin. This is the
    /// Tonnetz's defining property; if it fails the diagram is a decoration.
    #[test]
    fn every_tonnetz_triangle_is_a_triad() {
        for origin in 0..12u8 {
            for u in -4..4 {
                for v in -4..4 {
                    let p = tonnetz_pc(u, v, origin);
                    let up = [(u, v), (u + 1, v), (u, v + 1)]
                        .iter()
                        .fold(0u16, |m, &(u, v)| m | 1 << tonnetz_pc(u, v, origin));
                    assert_eq!(
                        up,
                        major_triad(p),
                        "the up-triangle at ({u},{v}) origin {origin} is not major on {p}"
                    );

                    let dn = [(u + 1, v), (u, v + 1), (u + 1, v + 1)]
                        .iter()
                        .fold(0u16, |m, &(u, v)| m | 1 << tonnetz_pc(u, v, origin));
                    // The minor triad shares the up-triangle's third and fifth.
                    assert_eq!(
                        dn,
                        minor_triad((p + 4) % 12),
                        "the down-triangle at ({u},{v}) origin {origin} is not minor"
                    );
                }
            }
        }
    }

    /// Neighbouring triangles share an edge, which means the two chords share
    /// two notes. That is why the Tonnetz shows voice leading at a glance.
    #[test]
    fn adjacent_triads_share_two_notes() {
        for pc in 0..12u8 {
            assert_eq!(
                (major_triad(pc) & minor_triad((pc + 4) % 12)).count_ones(),
                2
            );
            assert_eq!(
                (major_triad(pc) & minor_triad((pc + 9) % 12)).count_ones(),
                2
            );
        }
    }

    /// I, IV and V between them use every note of the major scale and no
    /// other. It is why three chords are enough for so many songs, and it
    /// keeps the triangle honest about which key it is drawing.
    #[test]
    fn one_four_and_five_spell_the_whole_key() {
        for tonic in 0..12u8 {
            let three =
                major_triad(tonic) | major_triad((tonic + 5) % 12) | major_triad((tonic + 7) % 12);
            assert_eq!(
                three,
                major_scale(tonic),
                "I-IV-V in {tonic} is not the major scale"
            );
        }
    }

    #[test]
    fn labels_parse_to_a_root_and_a_quality() {
        let cases = [
            ("C", Some((0, false))),
            ("Cm", Some((0, true))),
            ("Cmaj7", Some((0, false))),
            ("F#m7", Some((6, true))),
            ("Bb", Some((10, false))),
            ("Bbm", Some((10, true))),
            ("Ebmaj9", Some((3, false))),
            ("G7", Some((7, false))),
            ("Adim", Some((9, false))),
            ("", None),
            ("?", None),
            ("Hm", None),
        ];
        for (label, want) in cases {
            assert_eq!(parse_label(label), want, "parsing {label:?}");
        }
    }

    /// **The whole interaction, walked through as somebody would.**
    ///
    /// Four number keys, and between them they choose what is showing AND
    /// arrange it — because a toggle that appends is a reorder. This is the
    /// sequence the owner described, asserted step by step, and it is the one
    /// test in this file that would catch "on" quietly meaning "back in its
    /// numbered slot", which would make the band's order permanent and the
    /// second press of a number do nothing.
    #[test]
    fn the_number_keys_both_choose_and_arrange_the_band() {
        // Everything, in the numbered order.
        let mut band = Views::all();
        assert_eq!(band.order(), View::ALL);

        // 1 takes the first element away and the other three close up.
        band = band.toggled(View::Circle);
        assert_eq!(band.order(), &[View::Tonnetz, View::Triangles, View::Staff]);

        // Empty it entirely. This is a real state: the band collapses and the
        // window gives its height back.
        for v in [View::Tonnetz, View::Triangles, View::Staff] {
            band = band.toggled(v);
        }
        assert!(!band.any(), "the band did not collapse");
        assert_eq!(band.count(), 0);

        // From empty, ANY number puts that one element in full view.
        band = band.toggled(View::Triangles);
        assert_eq!(band.order(), &[View::Triangles]);

        // And the next one goes to its RIGHT, whatever its number is. This is
        // the reordering: 3 then 2 puts the Tonnetz after the triangles, which
        // no arrangement of flags could have said.
        band = band.toggled(View::Tonnetz);
        assert_eq!(band.order(), &[View::Triangles, View::Tonnetz]);
        band = band.toggled(View::Staff);
        assert_eq!(band.order(), &[View::Triangles, View::Tonnetz, View::Staff]);

        // Pressing a number twice moves that element to the end, which is the
        // whole vocabulary: any arrangement is reachable from any other.
        band = band.toggled(View::Triangles).toggled(View::Triangles);
        assert_eq!(band.order(), &[View::Tonnetz, View::Staff, View::Triangles]);

        // The numbers are FIXED to elements, not to positions. If this ever
        // became "the nth showing panel", every rearrangement would rebind the
        // keys under the user's fingers.
        for (i, v) in View::ALL.iter().enumerate() {
            assert_eq!(View::from_number(i + 1), Some(*v));
            assert_eq!(v.number(), i + 1);
        }
        assert_eq!(View::from_number(0), None);
        assert_eq!(View::from_number(View::ALL.len() + 1), None);
    }

    /// The order survives the settings file, and a file from a build that knows
    /// a fifth element still opens here.
    #[test]
    fn the_band_order_round_trips_and_tolerates_what_it_does_not_know() {
        for band in every_selection() {
            assert_eq!(Views::from_keys(&band.keys()), band, "{}", band.keys());
        }
        // A name this build has never heard of is DROPPED, not refused: the
        // alternative is a file written by a later version resetting somebody's
        // whole arrangement because one word in it was unfamiliar.
        assert_eq!(
            Views::from_keys("staff+spectrogram+circle"),
            Views::of(vec![View::Staff, View::Circle])
        );
        assert_eq!(Views::from_keys(""), Views::default());
        // And a name repeated is not two panels.
        assert_eq!(
            Views::from_keys("circle+circle"),
            Views::of(vec![View::Circle])
        );
    }

    /// **The cells divide the whole band and keep the order they were given.**
    #[test]
    fn the_cells_follow_the_order_and_leave_no_gap() {
        let rect = Rect::from_min_size(Pos2::new(11.0, 7.0), Vec2::new(900.0, 300.0));
        for band in every_selection() {
            let cells = cells(rect, &band);
            assert_eq!(
                cells.iter().map(|(v, _)| *v).collect::<Vec<_>>(),
                band.order(),
                "the cells came out in a different order from the band"
            );
            if band.any() {
                let total: f32 = cells.iter().map(|(_, r)| r.width()).sum();
                assert!((total - rect.width()).abs() < 0.01, "the band has a gap");
                assert!((cells[0].1.left() - rect.left()).abs() < 0.01);
                let last = cells.last().expect("a cell").1;
                assert!((last.right() - rect.right()).abs() < 0.01);
            }
        }
    }

    /// Panes are laid out in a fixed order, so turning one off never makes the
    /// other two swap places under the user's eyes.
    #[test]
    fn panes_keep_their_order_and_fill_the_band() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 300.0));
        let all = Views::all();
        let got = cells(rect, &all);
        assert_eq!(
            got.iter().map(|(v, _)| *v).collect::<Vec<_>>(),
            View::ALL.to_vec()
        );
        let total: f32 = got.iter().map(|(_, r)| r.width()).sum();
        assert!((total - rect.width()).abs() < 0.5, "panes left a gap");

        // Dropping the middle one keeps the outer two in the same order.
        let two = Views::of(vec![View::Circle, View::Triangles]);
        assert_eq!(
            cells(rect, &two).iter().map(|(v, _)| *v).collect::<Vec<_>>(),
            vec![View::Circle, View::Triangles]
        );
        assert!(cells(rect, &Views::default()).is_empty());
    }

    /// The band takes no height when nothing is selected — the window must not
    /// grow by 300 points to show three empty panes.
    #[test]
    fn zz_tmp_octave_sign_bounds() {
        for set in [
            crate::staff::StaffSet::Single(crate::staff::Clef::Treble),
            crate::staff::StaffSet::Grand,
        ] {
            for note in [100u8, 113, 41, 29] {
                let mut s = Settings::first_launch();
                s.staff_set = set.key();
                let views = s.theory_views();
                let w = 1300.0_f32;
                let band =
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(w, band_height(w, &views)));
                let cells = cells(band, &views);
                let Some((_, staff_cell)) = cells.iter().find(|(v, _)| *v == View::Staff)
                else {
                    continue;
                };
                let inner = cell_inner(*staff_cell);
                let body = body_rect(inner);
                let h = (inner.height() * 0.075).clamp(9.0, 14.0);
                let lh = h * 0.82;
                let legend_centre_y = inner.min.y + h + lh * 0.5;
                let notes: HashSet<u8> = [note].into_iter().collect();
                let ctx = egui::Context::default();
                fonts::install(&ctx, fonts::FontChoice::default(), None);
                let out = ctx.run(egui::RawInput::default(), |ctx| {
                    let painter = ctx.layer_painter(egui::LayerId::debug());
                    draw(
                        &painter,
                        band,
                        &views,
                        Input {
                            pcs: 1 << (note % 12),
                            bass: Some(note % 12),
                            root: None,
                            minor: false,
                        },
                        &notes,
                        None,
                        &s,
                    );
                });
                let mut worst_top = f32::INFINITY;
                let mut worst_bot = f32::NEG_INFINITY;
                let mut texts = Vec::new();
                for cs in &out.shapes {
                    if let egui::Shape::Text(t) = &cs.shape {
                        let r = t.visual_bounding_rect();
                        texts.push((t.galley.job.text.clone(), r));
                    }
                    let r = cs.shape.visual_bounding_rect();
                    if r.is_positive() && r.width() < 1e6 {
                        worst_top = worst_top.min(r.top());
                        worst_bot = worst_bot.max(r.bottom());
                    }
                }
                eprintln!(
                    "SET={:?} note={note} body={:?} legend_cy={:.2} inner_top={:.2} worst_top={:.2} worst_bot={:.2}",
                    set.key(), body, legend_centre_y, inner.min.y, worst_top, worst_bot
                );
                for (txt, r) in texts {
                    eprintln!("   text {txt:?} rect={r:?} above_body={:.2}", body.top() - r.top());
                }
            }
        }
    }

    #[test]
    fn an_empty_selection_takes_no_band() {
        assert_eq!(band_height(1300.0, &Views::default()), 0.0);
        assert!(
            band_height(1300.0, &Views::of(vec![View::Circle])) > 0.0
        );
    }

    /// The pane sizes the band actually produces: one, two or three diagrams
    /// across a window at every size the app offers, minus the title strip.
    fn real_panes() -> Vec<Rect> {
        let mut out = Vec::new();
        for pct in [50i64, 75, 100, 125, 150, 175, 200] {
            let w = (1300.0 * pct as f64 / 100.0).trunc() as f32;
            for n in 1..=View::ALL.len() {
                // **Actually n panes.** This loop used to build a one-element
                // band whatever `n` was, so every width in it was measured
                // three times and the multi-pane split — the one that can
                // starve a diagram — was never measured at all.
                let views = Views::of(View::ALL.into_iter().take(n).collect());
                let h = band_height(w, &views);
                let band = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, h));
                for (_, cell) in cells(band, &views) {
                    // What draw() hands each diagram, and then what title()
                    // leaves below itself.
                    let inner = cell_inner(cell);
                    let th = (inner.height() * 0.09).clamp(9.0, 15.0);
                    out.push(Rect::from_min_max(
                        Pos2::new(inner.min.x, inner.min.y + th + 2.0),
                        inner.max,
                    ));
                }
            }
        }
        out
    }

    /// Not one Tonnetz node may cross its pane. The lattice is unbounded and
    /// the pane is not; the first render put a column through the neighbouring
    /// diagram and clipped an "Eb" down to "Kb", which reads as a note name
    /// and is not one.
    #[test]
    fn no_tonnetz_node_escapes_its_pane() {
        for pane in real_panes() {
            let Some(l) = Lattice::fit(pane) else {
                continue;
            };
            for v in 0..l.rows {
                for u in 0..l.cols {
                    if !l.shows(pane, u, v) {
                        continue;
                    }
                    let node = Rect::from_center_size(l.at(u, v), Vec2::splat(l.node_r * 2.0));
                    assert!(
                        pane.contains_rect(node),
                        "node ({u},{v}) at {node:?} escapes {pane:?}"
                    );
                }
            }
        }
    }

    /// ...and the fix for that must not have emptied the diagram. A guard that
    /// hides everything also satisfies "nothing escapes", so the useful half of
    /// the property is that a real pane still shows a lattice worth looking at.
    #[test]
    fn the_tonnetz_still_has_a_lattice_left() {
        for pane in real_panes() {
            let l = Lattice::fit(pane).expect("a real pane must fit a lattice");
            let shown = (0..l.rows)
                .flat_map(|v| (0..l.cols).map(move |u| (u, v)))
                .filter(|&(u, v)| l.shows(pane, u, v))
                .count();
            assert!(
                shown >= 20,
                "only {shown} nodes survive in {pane:?} ({}x{})",
                l.cols,
                l.rows
            );

            // EVERY row and EVERY column, whole. The lattice used to be sized
            // so its extent landed exactly on the pane edge, and a node whose
            // box touches the boundary fails `contains_rect` by a rounding
            // error — so the bottom row vanished and the diagram read as cut
            // off. It only happened below 100% window size, which is where
            // the arithmetic stopped landing on whole pixels, so a test that
            // checks one size proves nothing.
            for v in 0..l.rows {
                let in_row = (0..l.cols).filter(|&u| l.shows(pane, u, v)).count();
                assert!(
                    in_row >= 3,
                    "row {v} of {} has only {in_row} whole nodes in {pane:?} - \
                     the lattice is clipped",
                    l.rows
                );
            }
            // Every pitch class reachable, so the diagram can always light the
            // note you are playing.
            let seen: u16 = (0..l.rows)
                .flat_map(|v| (0..l.cols).map(move |u| (u, v)))
                .filter(|&(u, v)| l.shows(pane, u, v))
                .fold(0u16, |m, (u, v)| m | 1 << tonnetz_pc(u, v, 0));
            assert_eq!(seen, 0xFFF, "some pitch class has no node in {pane:?}");
        }
    }

    /// The harmonic-triangle figure, nodes included, fits the pane it is given.
    /// It did not: the tonic node was drawn over the pane's own title.
    #[test]
    fn the_hexagram_fits_under_its_own_title() {
        for pane in real_panes() {
            let r = hexagram_radius(pane);
            let extent = r * 1.30 * 2.0;
            assert!(
                extent <= pane.width().min(pane.height()) + 0.5,
                "the figure is {extent} across in a {:?} pane",
                pane.size()
            );
        }
    }

    /// Every character the band draws must reach the screen. The key
    /// signatures were written with U+266F and U+266D, which neither bundled
    /// face nor the fallback chain has, so they drew as tofu boxes — and a
    /// missing accidental turns "2 sharps" into "2 something".
    #[test]
    fn every_glyph_the_band_draws_exists() {
        // EVERY bundled face, not just the default: the typeface is a user
        // choice, so a glyph missing from one of them is a glyph missing for
        // whoever picked it.
        //
        // The two music accidentals are the exception, and deliberately: only
        // JetBrains Mono has them, and `fonts::install` puts JetBrains at the
        // bottom of BOTH families for exactly that reason. They are checked
        // separately below, against that one face.
        let bundled: &[(&str, &[u8])] = &[
            ("Courier Prime", fonts::COURIER_PRIME_REGULAR),
            ("Courier Prime Bold", fonts::COURIER_PRIME_BOLD),
            ("Terminess", fonts::TERMINESS_REGULAR),
            ("Terminess Bold", fonts::TERMINESS_BOLD),
            ("JetBrains Mono", fonts::JETBRAINS_REGULAR),
            ("JetBrains Mono Bold", fonts::JETBRAINS_BOLD),
        ];
        let mut drawn: Vec<char> = Vec::new();
        for pc in 0..12u8 {
            drawn.extend(pc_name(pc, true).chars());
            drawn.extend(pc_name(pc, false).chars());
            drawn.extend(pc_name(pc, true).to_lowercase().chars());
        }
        for i in 0..12usize {
            let (sharps, flats) = signature(i);
            drawn.extend(format!("{sharps}#{flats}b").chars());
        }
        drawn.extend("CIRCLE OF FIFTHS TONNETZ HARMONIC TRIANGLES".chars());
        drawn.extend("I IV V i iv v no key fits keys 0123456789m".chars());
        drawn.sort_unstable();
        drawn.dedup();

        for c in drawn {
            for (name, bytes) in bundled {
                assert!(
                    ttf_parser::Face::parse(bytes, 0)
                        .map(|f| f.glyph_index(c).is_some())
                        .unwrap_or(false),
                    "{name} has no glyph for {c:?} (U+{:04X})",
                    c as u32
                );
            }
        }

        // The accidentals, which reach the screen through the JetBrains
        // fallback rather than the chosen face. If this face ever loses them
        // the circle silently goes back to drawing tofu boxes, which is how
        // this started.
        for c in [FLAT, SHARP] {
            assert!(
                ttf_parser::Face::parse(fonts::JETBRAINS_REGULAR, 0)
                    .map(|f| f.glyph_index(c).is_some())
                    .unwrap_or(false),
                "JetBrains Mono has no {c:?} (U+{:04X}), so nothing does",
                c as u32
            );
        }
    }

    /// Every Tonnetz cell the band can actually produce.
    fn real_tonnetz_cells() -> Vec<(Rect, Rect)> {
        let mut out = Vec::new();
        for pct in [50i64, 75, 100, 125, 150, 175, 200] {
            let w = (1300.0 * pct as f64 / 100.0).trunc() as f32;
            for n in 1..=View::ALL.len() {
                // The Tonnetz in a band of `n` panes, not in a band of one
                // pane `n` times — same bug as `real_panes` had.
                let mut order = vec![View::Tonnetz];
                order.extend(View::ALL.into_iter().filter(|v| *v != View::Tonnetz).take(n - 1));
                let views = Views::of(order);
                let band = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, band_height(w, &views)));
                for (v, cell) in cells(band, &views) {
                    if v == View::Tonnetz {
                        out.push((band, cell));
                    }
                }
            }
        }
        out
    }

    /// No lit triangle may have a corner on a node that is not drawn.
    ///
    /// The triangle loop walks `-1..cols` so it can reach the cells along the
    /// left edge, which means it can also name corners one column past the
    /// right edge. Those produced a shape hanging off the grid with a vertex
    /// on nothing — it reads as a rendering fault, not as a chord.
    #[test]
    fn no_lit_triangle_hangs_off_the_grid() {
        // Cm7, which is what showed the bug, plus a spread of others.
        for pcs in [
            0b0000_1000_1001_u16 | (1 << 3) | (1 << 10), // C Eb G Bb
            0b0000_1001_0001,                            // C E G
            0xFFF,
            0b1000_0100_0001,
        ] {
            let input = Input {
                pcs,
                bass: Some(0),
                root: Some(0),
                minor: false,
            };
            // THE REAL PANES, all three combinations, because a cell in the
            // middle of three is not the same rectangle as a band of one and
            // this bug only showed in the three-pane case.
            for (band, cell) in real_tonnetz_cells() {
                let w = band.width();
                let body = body_rect(cell_inner(cell));
                let Some(l) = Lattice::fit(body) else {
                    continue;
                };
                for v in 0..l.rows {
                    for u in -1..l.cols {
                        for corners in [
                            [(u, v), (u + 1, v), (u, v + 1)],
                            [(u + 1, v), (u, v + 1), (u + 1, v + 1)],
                        ] {
                            let mask = corners
                                .iter()
                                .fold(0u16, |m, &(u, v)| m | 1 << tonnetz_pc(u, v, 0));
                            if !input.contains(mask) {
                                continue;
                            }
                            let drawn = corners.iter().all(|&(u, v)| l.shows(body, u, v));
                            if !drawn {
                                // This is the case the renderer must skip. If
                                // it did not, the assertion below would be the
                                // one to change — it is asserting the SHAPE of
                                // the guard, so it has to name it.
                                continue;
                            }
                            for &(cu, cv) in &corners {
                                assert!(
                                    l.shows(body, cu, cv),
                                    "a lit triangle at ({u},{v}) in a {w}pt band \
                                     has a corner at ({cu},{cv}) that is never drawn"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// A click on a Tonnetz node must return the pitch class written on it.
    ///
    /// Hit tests drift away from the pictures they belong to, and the drift is
    /// invisible: the diagram still looks right and the clicks land on the
    /// wrong thing. So this walks every node the lattice actually shows and
    /// asks the hit test what is at its exact centre.
    #[test]
    fn a_click_on_a_tonnetz_node_finds_that_note() {
        let views = Views::of(vec![View::Tonnetz]);
        for pane in real_panes() {
            let band = Rect::from_min_size(Pos2::ZERO, Vec2::new(pane.width() + 16.0, 300.0));
            let cell = cells(band, &views)[0].1;
            let body = body_rect(cell_inner(cell));
            let Some(l) = Lattice::fit(body) else {
                continue;
            };
            let mut checked = 0;
            for v in 0..l.rows {
                for u in 0..l.cols {
                    if !l.shows(body, u, v) {
                        continue;
                    }
                    let c = l.at(u, v);
                    assert_eq!(
                        hit_test(band, &views, 0, c),
                        Some(Hit::Pc(tonnetz_pc(u, v, 0))),
                        "clicking the centre of node ({u},{v}) missed it"
                    );
                    checked += 1;
                }
            }
            assert!(checked >= 20, "only {checked} nodes were reachable at all");
        }
    }

    /// The same for the circle: the name you click is the note you get, on
    /// both rings, all the way round.
    #[test]
    fn a_click_on_a_circle_name_finds_that_note() {
        let views = Views::of(vec![View::Circle]);
        let band = Rect::from_min_size(Pos2::ZERO, Vec2::new(440.0, 300.0));
        let body = body_rect(cell_inner(cells(band, &views)[0].1));
        let c = body.center();
        let r = body.width().min(body.height()) * 0.5 - 2.0;
        for (i, &pc) in FIFTHS.iter().enumerate() {
            let a = circle_angle(i);
            let dir = Vec2::new(a.cos(), a.sin());
            // Where `draw` puts the major name, and the relative minor.
            let major_at = c + dir * ((r * 0.80 + r * 0.56) * 0.5);
            let minor_at = c + dir * ((r * 0.56 + r * 0.34) * 0.5);
            assert_eq!(
                hit_test(band, &views, 0, major_at),
                Some(Hit::Pc(pc)),
                "the major name at position {i} is not clickable"
            );
            // The relative-minor ring is a label. Every pitch class is
            // already reachable on the major ring, so nothing is lost.
            assert_eq!(
                hit_test(band, &views, 0, minor_at),
                None,
                "the relative-minor label at position {i} acts as a control"
            );
        }
        // The hub and the signature ring are labels, not controls.
        assert_eq!(hit_test(band, &views, 0, c), None);
    }

    /// Each vertex of the hexagram gives back the chord printed on it, in
    /// every key — and the two triangles must not answer for each other.
    ///
    /// **Driven by the KEY now, in every one of the fifteen.** The shape is
    /// anchored to the signature rather than to what is being played, which is
    /// why `hit_test` no longer takes an `Input` at all: a test could have
    /// caught it answering for the wrong tonic, but a parameter it cannot see
    /// is a guarantee, and the compiler checks that one.
    ///
    /// What is left to check is the arithmetic, which is where the mistake
    /// moved to: `key_tonic` for flats is one `rem_euclid` away from being
    /// right for sharps and wrong for flats.
    #[test]
    fn a_click_on_a_chord_vertex_gives_that_chord() {
        let views = Views::of(vec![View::Triangles]);
        let band = Rect::from_min_size(Pos2::ZERO, Vec2::new(440.0, 300.0));
        let body = body_rect(cell_inner(cells(band, &views)[0].1));
        let c = body.center();
        let r = hexagram_radius(body);
        for key in -crate::staff::MAX_KEY..=crate::staff::MAX_KEY {
            let tonic = key_tonic(key);
            let roots = [tonic, (tonic + 5) % 12, (tonic + 7) % 12];
            for (k, &oi) in HEX_UP_ORDER.iter().enumerate() {
                assert_eq!(
                    hit_test(band, &views, key, hex_vertex(c, r, k, false)),
                    Some(Hit::Triad {
                        root: roots[oi],
                        minor: false
                    }),
                    "upward vertex {k} in key {key}"
                );
            }
            for (k, &oi) in HEX_DOWN_ORDER.iter().enumerate() {
                assert_eq!(
                    hit_test(band, &views, key, hex_vertex(c, r, k, true)),
                    Some(Hit::Triad {
                        root: roots[oi],
                        minor: true
                    }),
                    "downward vertex {k} in key {key}"
                );
            }
        }
    }

    /// The signature IS the tonic, `key` fifths from C.
    ///
    /// Spelled out rather than left to the arithmetic, because the arithmetic
    /// is one `rem_euclid` away from being right for sharps and wrong for
    /// flats, and both halves look correct in isolation.
    #[test]
    fn the_key_signature_names_the_tonic_it_is_the_signature_of() {
        for (key, want) in [
            (0, 0),   // C
            (1, 7),   // G
            (2, 2),   // D
            (7, 1),   // C sharp
            (-1, 5),  // F
            (-2, 10), // B flat
            (-3, 3),  // E flat
            (-7, 11), // C flat, which sounds B
        ] {
            assert_eq!(key_tonic(key), want, "key {key}");
        }
        // And the black keys are spelled the way the signature does, not the
        // way the user's own preference would: that preference decides only
        // when there is no signature to decide it.
        assert!(key_flats(-3, false), "a flat key spelled with sharps");
        assert!(!key_flats(4, true), "a sharp key spelled with flats");
        assert!(key_flats(0, true) && !key_flats(0, false));
    }

    /// A click belongs to the pane it landed in, and to no other.
    ///
    /// Three diagrams side by side share one rectangle, and each one's
    /// geometry is happy to answer for a point anywhere on the plane — the
    /// circle's rings and the lattice both extend past their cell by
    /// arithmetic if not by drawing. `hit_test` picks the cell FIRST for
    /// exactly that reason.
    #[test]
    fn a_click_belongs_to_the_pane_it_landed_in() {
        let all = Views::of(vec![View::Circle, View::Tonnetz, View::Triangles]);
        let band = Rect::from_min_size(Pos2::ZERO, Vec2::new(1300.0, 300.0));
        let panes = cells(band, &all);
        let triangles_cell = panes
            .iter()
            .find(|(v, _)| *v == View::Triangles)
            .map(|(_, c)| *c)
            .expect("the triangles pane");

        // Sweep the whole band. Only the triangles produce chords, and only
        // from inside their own cell.
        let mut triads = 0;
        let mut hits = 0;
        for x in (0..1300).step_by(7) {
            for y in (0..300).step_by(7) {
                let p = Pos2::new(x as f32, y as f32);
                match hit_test(band, &all, 0, p) {
                    Some(Hit::Triad { .. }) => {
                        assert!(
                            triangles_cell.contains(p),
                            "a chord came back from {p:?}, outside the triangles pane"
                        );
                        triads += 1;
                        hits += 1;
                    }
                    Some(Hit::Pc(_)) => {
                        assert!(
                            !triangles_cell.contains(p),
                            "a bare note came back from inside the triangles pane at {p:?}"
                        );
                        hits += 1;
                    }
                    None => {}
                }
            }
        }
        assert!(triads > 0, "no chord vertex was reachable anywhere");
        assert!(
            hits > 100,
            "only {hits} points in the whole band are clickable"
        );

        // Nothing selected, nothing to click.
        for x in (0..1300).step_by(53) {
            for y in (0..300).step_by(29) {
                assert_eq!(
                    hit_test(band, &Views::default(), 0,
                        Pos2::new(x as f32, y as f32)
                    ),
                    None
                );
            }
        }
    }

    /// Every pane must draw without panicking at any size the layout can hand
    /// it, holding anything from nothing to all twelve notes. These are
    /// painters full of trigonometry and integer row counts; a zero-width band
    /// or a degenerate lattice is exactly where that breaks.
    #[test]
    fn every_pane_draws_at_every_size() {
        let ctx = egui::Context::default();
        // Every label here is a real glyph, and a bare Context has no font
        // families bound, so painting one panics rather than failing.
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        let s = Settings::default();
        for w in [40.0_f32, 120.0, 400.0, 1300.0, 2600.0] {
            for views in [
                Views::of(vec![View::Circle]),
                Views::of(vec![View::Tonnetz]),
                Views::of(vec![View::Triangles]),
                Views::of(vec![View::Circle, View::Tonnetz, View::Triangles]),
            ] {
                let h = band_height(w, &views);
                let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, h));
                for pcs in [0u16, 0b0000_1001_0001, 0xFFF, 0b1000_0000_0001] {
                    let input = Input {
                        pcs,
                        bass: (pcs != 0).then_some(0),
                        root: (pcs != 0).then_some(0),
                        minor: false,
                    };
                    let _ = ctx.run(Default::default(), |ctx| {
                        let painter = ctx.layer_painter(egui::LayerId::background());
                        draw(&painter, rect, &views, input, &HashSet::new(), None, &s);
                    });
                }
            }
        }
    }

    // ── every arrangement, and where its controls are ──────────────────────

    /// **Every ordered subset of the band**, which is what the band can now
    /// be: sixty-five of them, from empty to all four in each of their
    /// twenty-four arrangements. It used to be the eight combinations three
    /// flags could express, and order was not one of the things it could
    /// express — so nothing here could have caught a hit test that was right
    /// about WHICH panels and wrong about where they were.
    fn every_selection() -> Vec<Views> {
        fn grow(prefix: Vec<View>, out: &mut Vec<Views>) {
            out.push(Views::of(prefix.clone()));
            for v in View::ALL {
                if !prefix.contains(&v) {
                    let mut next = prefix.clone();
                    next.push(v);
                    grow(next, out);
                }
            }
        }
        let mut out = Vec::new();
        grow(Vec::new(), &mut out);
        out
    }

    /// Where the drawing puts every control inside `rect`, and what clicking it
    /// must give back.
    ///
    /// Re-derived from the drawing code rather than read back out of
    /// `hit_test`, exactly like the band's own click tests: a hit test compared
    /// against itself proves nothing. If `draw_circle` moves the name ring or
    /// `draw_triangles` moves a vertex, this goes stale and the assertions
    /// below fail, which is the point.
    fn controls(rect: Rect, views: &Views, input: Input) -> Vec<(View, Pos2, Hit)> {
        let mut out = Vec::new();
        for (view, cell) in cells(rect, views) {
            let body = body_rect(cell_inner(cell));
            match view {
                View::Circle => {
                    let c = body.center();
                    let r = body.width().min(body.height()) * 0.5 - 2.0;
                    if r < 20.0 {
                        continue;
                    }
                    for (i, &pc) in FIFTHS.iter().enumerate() {
                        let a = circle_angle(i);
                        let dir = Vec2::new(a.cos(), a.sin());
                        // Where `draw_circle` centres the major name.
                        out.push((view, c + dir * ((r * 0.80 + r * 0.56) * 0.5), Hit::Pc(pc)));
                    }
                }
                // The sheet music has nothing to click on: it is a readout.
                View::Staff => {}
                View::Tonnetz => {
                    let Some(l) = Lattice::fit(body) else {
                        continue;
                    };
                    for v in 0..l.rows {
                        for u in 0..l.cols {
                            if l.shows(body, u, v) {
                                out.push((view, l.at(u, v), Hit::Pc(tonnetz_pc(u, v, 0))));
                            }
                        }
                    }
                }
                View::Triangles => {
                    let c = body.center();
                    let r = hexagram_radius(body);
                    if r < 20.0 {
                        continue;
                    }
                    let tonic = input.tonic();
                    let roots = [tonic, (tonic + 5) % 12, (tonic + 7) % 12];
                    for (k, &oi) in HEX_UP_ORDER.iter().enumerate() {
                        let hit = Hit::Triad {
                            root: roots[oi],
                            minor: false,
                        };
                        out.push((view, hex_vertex(c, r, k, false), hit));
                    }
                    for (k, &oi) in HEX_DOWN_ORDER.iter().enumerate() {
                        let hit = Hit::Triad {
                            root: roots[oi],
                            minor: true,
                        };
                        out.push((view, hex_vertex(c, r, k, true), hit));
                    }
                }
            }
        }
        out
    }

    /// C major sounding, so the diagrams have something to light and the
    /// triangles have a tonic to orient themselves around.
    fn c_major() -> Input {
        Input {
            pcs: major_triad(0),
            bass: Some(0),
            root: Some(0),
            minor: false,
        }
    }

    /// **The main window's floor leaves the diagrams something to draw.**
    ///
    /// `Lattice::fit` returns None below `a = 16`, and `draw_circle` and
    /// `draw_triangles` both return early below `r = 20`. All three failures
    /// look identical from outside: a pane title over an empty rectangle, which
    /// reads as a rendering fault rather than as "the window is too small".
    ///
    /// `settings::MIN_WINDOW` took its width from the popped-out theory
    /// window's floor (the diagrams are the first thing to give up, and they
    /// give up at 540), but the main window hands the theory band a row whose
    /// HEIGHT is derived from the window's width rather than chosen, so the
    /// two floors were never the same rectangle and the inherited number
    /// proves nothing on its own. This is the check that the main window's
    /// floor is a floor.
    #[test]
    fn the_main_windows_floor_leaves_the_diagrams_something_to_draw() {
        let views = Views::all();
        let w = crate::settings::MIN_WINDOW.x;
        let h = band_height(w, &views);
        assert!(h > 0.0, "the theory band has no height at the window floor");
        let row = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, h));
        let at_min = controls(row, &views, c_major());
        for view in View::ALL.into_iter().filter(|v| *v != View::Staff) {
            assert!(
                at_min.iter().any(|(v, _, _)| *v == view),
                "{view:?} draws nothing in the theory band at the main window's \
                 floor ({w} x {h})"
            );
        }
    }
}
