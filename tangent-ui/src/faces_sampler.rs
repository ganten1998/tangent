//! The sampler's face: one file, its picture, and an envelope drawn ON it.
//!
//! **What this panel is.** A wide window in two halves, the IR face's shape
//! and for the IR face's reason — a machine on the left, a list of files on
//! the right — because the owner named the browser as the way samples get
//! in: *"just use the drumpad media explorer we already have to allow the
//! sampler to import samples. you can drag from elsewhere into the plugin,
//! but the main way will be the media explorer."*
//!
//! The left half is the sample. Its name across the top, then the waveform
//! on the kit's glass, then the crop and the envelope drawn OVER that
//! waveform rather than beside it, then the ordinary controls in two
//! columns of kit rows.
//!
//! **The envelope is the centrepiece and it is drawn where the sound is.**
//! Four handles a hand can grab — attack, decay, sustain, release — laid
//! over the picture of the file they shape, so the question "where does
//! this note stop being loud" is answered against the waveform and not
//! against an abstract box in another part of the panel. The nodes are
//! placed by the PARAMETER and not by the second: a ten-second release over
//! a three-hundred-millisecond hit would put the node a metre off the right
//! of the window, and a display that only works for samples of one length
//! is not a display. Each stage owns a share of the crop's width and the
//! node stands at its normalised value inside that share, which is what
//! every hardware envelope display has done since the DX7.
//!
//! **A node is a grab, not a pixel hunt.** The drawn dot is six points and
//! the target is [`NODE_HIT`] square, which is more than twice it. That is
//! deliberate and it is the whole difference between an envelope you can
//! dial in and one you fight: the four handles are the most-used controls
//! on this panel and they sit over a busy picture.
//!
//! **The crop is two grips and two numbers, and they cannot disagree.** The
//! owner asked for exactly that correspondence. The numbers are printed
//! from the same `value` the grips are drawn at rather than from the host's
//! `reading` — which is the one place this panel departs from the house law
//! that a reading is spelled by the unit's own mapping, and it departs on
//! purpose: `reading` lags a drag by one push, so a field fed from it would
//! print 0.240 under a grip standing at 0.247 for one frame of every frame
//! the hand is moving. A pair that is allowed to disagree while being
//! dragged is a pair nobody can trust when it is still.
//!
//! **Ink is a property of the surface it lands on.** This panel has three
//! surfaces — the panel's own ground, the LCD glass the waveform lives on,
//! and the explorer's recess — and every word is inked against the one it
//! actually lands on: `st.s.ink`/`faint` on the panel, `face::lcd_ink` on
//! the glass. `the_sampler_prints_its_ink_on_the_surface_it_lands_on_on_
//! every_theme` reads that off the shape stream rather than off this
//! sentence.
//!
//! **Every control the panel draws is in the press list.** This codebase has
//! shipped controls that draw and hit-test and do nothing four times, always
//! because a match arm was missing. Here the press list IS the drawing —
//! every target is an `ui.interact` inside `draw_sampler` and its wish is
//! folded with [`face::keep`] — and `every_drawn_control_answers_a_press`
//! drives a real gesture at each one through egui's own input to prove it.

use crate::face::{self, FaceStyle, LcdTier};
use crate::faces::{FaceAction, FaceLive};
use crate::fonts;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

/// The sentinel, spelled here as the contract with the engine's
/// `builtin_fx::SAMPLER_PATH`; `dialogs::SAMPLER_PATH` is the same string
/// and `the_sentinel_is_spelled_once` pins the pair this crate can see.
///
// MERGE-ME(engine): `tangent/src/builtin_fx.rs` must declare
// `pub const SAMPLER_PATH: &str = "@tangent-sampler";` with `class:
// "Tangent Sampler"` and `voice: true`, and the engine's own sweep should
// pin it against this spelling the way `ANALOG_PATH` is pinned.
pub const SAMPLER_PATH: &str = "@tangent-sampler";
/// The strap-line under the title bar.
pub const SAMPLER_ABOUT: &str = "one file, cropped and shaped, across the keyboard";

// ── the ids, the engine's table's ────────────────────────────────────────────
// Restated here because this crate cannot see the kernel across the
// firewall — `faces_analog` restates sixty-one of them for the same reason.
// `every_control_names_a_pushed_parameter_once` holds them against what the
// host actually pushes, so a renumbering fails a test rather than silently
// moving a knob.

/// Volume.
const P_VOL: u32 = 0;
/// Sample Mode: pitched, fixed, one-shot.
const P_MODE: u32 = 1;
/// Transpose, semitones.
const P_TRANSPOSE: u32 = 2;
/// Fine, CENTS.
const P_FINE: u32 = 3;
const P_ATTACK: u32 = 4;
const P_DECAY: u32 = 5;
const P_SUSTAIN: u32 = 6;
const P_RELEASE: u32 = 7;
/// Crop start, a fraction of the file.
const P_START: u32 = 8;
/// Crop end, a fraction of the file.
const P_END: u32 = 9;
const P_NOTEOFF: u32 = 10;
const P_LOOP: u32 = 11;
/// Loop crossfade, ms.
const P_XFADE: u32 = 12;
const P_VOICES: u32 = 13;
/// Portamento, seconds.
const P_PORTA: u32 = 14;
const P_BEND: u32 = 15;

/// One past the highest id: what a dense harness must number up to so every
/// control finds a value. See `face::fit::FACES`.
pub const ID_CEILING: usize = 16;

/// The stepped ids, for the shared sweeps: what the engine's table says has
/// positions. Mode, the two toggles, the voice count and the bend range.
pub const STEPPED: &[u32] = &[P_MODE, P_NOTEOFF, P_LOOP, P_VOICES, P_BEND];

/// **The cents the Fine knob spans, restated.**
///
/// The face has to spell this because a normalised value is all that crosses
/// the firewall — `ports::BuiltinFaceParam` carries no range, by design, and
/// the words are the engine's. What the owner asked for here is the Hz the
/// detune actually MOVES, printed beside the cents, and Hz cannot be derived
/// from a number between nought and one without knowing what it means.
///
// MERGE-ME(engine): the sampler's `fine` parameter must map 0..=1 onto
// -100..=+100 cents, so this and `builtin_fx::describe` print the same
// detune. `the_fine_knob_prints_the_hz_it_moves` pins the arithmetic on
// this side; nothing here can pin the other.
const FINE_CENTS: f32 = 100.0;

/// The pitch the Hz offset is quoted AT.
///
/// A detune in cents is a RATIO and has no size in Hz until it is applied to
/// a pitch, so "+3.2 Hz" is meaningless without saying of what. Concert A is
/// the reference every tuner in the world prints against, and quoting it
/// makes the number checkable against a tuner rather than against this code.
const FINE_REF_HZ: f32 = 440.0;

// ── the geometry ────────────────────────────────────────────────────────────

/// The header band: the name and the badge.
const HEADER_H: f32 = 24.0;
/// The line that says which file is loaded.
const NAME_H: f32 = 18.0;
/// The crop's two numeric fields.
const CROP_H: f32 = 20.0;
/// The waveform, tallest and shortest. The floor still holds an envelope a
/// hand can reach into: four nodes and the plateau between two of them.
const WAVE_MAX: f32 = 220.0;
const WAVE_MIN: f32 = 96.0;
/// The panel's inset, and the air between its parts.
const MARGIN: f32 = 4.0;
const AIR: f32 = 6.0;
/// The explorer's share of the width, and the least it may be. Below this a
/// file name is an ellipsis and a browser that cannot show a name is not a
/// browser.
const BROWSER_SHARE: f32 = 0.34;
const BROWSER_MIN: f32 = 190.0;
/// The shortcut chips above the listing: Home, Takes, and this project.
const SHORTCUT_H: f32 = 20.0;
/// The text sizes, nominal; each word fits itself down from here.
const TITLE_PT: f32 = 13.0;
const NAME_PT: f32 = 11.0;
const FIELD_PT: f32 = 11.0;

/// The drawn radius of an envelope node, and the side of the square a hand
/// has to hit to take it. See the module header: the target is more than
/// twice the dot on purpose.
const NODE_R: f32 = 5.0;
const NODE_HIT: f32 = 22.0;
/// The crop grip's drawn width, and the width of its target.
const GRIP_W: f32 = 3.0;
const GRIP_HIT: f32 = 16.0;
/// The least crop a drag may leave, as a fraction of the file. A crop with
/// no room in it is a sample that plays nothing, reached by a slip.
const CROP_MIN: f32 = 0.005;

/// How much of the envelope's width each stage owns. They sum to one and
/// the sustain's share is the PLATEAU, which is why it is the short one:
/// a plateau is read as a level and needs only enough width to be seen as
/// flat, while the three times are read as distances.
const A_SHARE: f32 = 0.26;
const D_SHARE: f32 = 0.24;
const S_SHARE: f32 = 0.18;
const R_SHARE: f32 = 0.32;

/// How far a drag moves a numeric field per point, and with shift held.
const DRAG_PER_PT: f32 = 0.006;
const FINE_PER_PT: f32 = 0.0015;

/// Where everything is, from the body the panel was given.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub header: Rect,
    /// The line naming the loaded file.
    pub name: Rect,
    /// The glass the waveform and the envelope share.
    pub wave: Rect,
    /// The two crop numbers under it.
    pub crop_start: Rect,
    pub crop_end: Rect,
    /// The rows of ordinary controls.
    pub rows: Rect,
    /// The browser's column, and the shortcut chips over it.
    pub shortcuts: Rect,
    pub browser: Rect,
}

impl Layout {
    /// The whole panel from the body rect. The waveform gives up its height
    /// before anything else and never below [`WAVE_MIN`]; the browser takes
    /// its share of the width and the machine takes the rest.
    pub fn new(body: Rect) -> Self {
        let inner = body.shrink(MARGIN);
        if !inner.is_positive() {
            return Self {
                header: Rect::NOTHING,
                name: Rect::NOTHING,
                wave: Rect::NOTHING,
                crop_start: Rect::NOTHING,
                crop_end: Rect::NOTHING,
                rows: Rect::NOTHING,
                shortcuts: Rect::NOTHING,
                browser: Rect::NOTHING,
            };
        }
        let browser_w = (inner.width() * BROWSER_SHARE)
            .max(BROWSER_MIN)
            .min(inner.width() * 0.5);
        let split = inner.right() - browser_w;
        let left = Rect::from_min_max(inner.min, Pos2::new(split - AIR, inner.bottom()));
        let header = Rect::from_min_size(left.min, Vec2::new(left.width(), HEADER_H));
        let name = Rect::from_min_size(
            Pos2::new(left.left(), header.bottom()),
            Vec2::new(left.width(), NAME_H),
        );
        // What is left after the fixed furniture, split between the wave and
        // the rows. The wave takes the larger part because it is the panel.
        let free = (left.bottom() - name.bottom() - CROP_H - AIR * 2.0).max(0.0);
        let wave_h = (free * 0.42).clamp(WAVE_MIN.min(free), WAVE_MAX);
        let wave = Rect::from_min_size(
            Pos2::new(left.left(), name.bottom() + AIR * 0.5),
            Vec2::new(left.width(), wave_h),
        );
        let crop_y = wave.bottom() + 2.0;
        let half = (left.width() - AIR) * 0.5;
        let crop_start = Rect::from_min_size(Pos2::new(left.left(), crop_y), Vec2::new(half, CROP_H));
        let crop_end =
            Rect::from_min_size(Pos2::new(left.right() - half, crop_y), Vec2::new(half, CROP_H));
        let rows = Rect::from_min_max(
            Pos2::new(left.left(), crop_start.bottom() + AIR),
            Pos2::new(left.right(), left.bottom()),
        );
        let col = Rect::from_min_max(Pos2::new(split, inner.top()), inner.max);
        let shortcuts = Rect::from_min_size(col.min, Vec2::new(col.width(), SHORTCUT_H));
        let browser = Rect::from_min_max(
            Pos2::new(col.left(), shortcuts.bottom() + 2.0),
            col.max,
        );
        Self { header, name, wave, crop_start, crop_end, rows, shortcuts, browser }
    }

    /// The three shortcut chips, left to right.
    pub fn shortcut_cells(&self, n: usize) -> Vec<Rect> {
        if n == 0 || !self.shortcuts.is_positive() {
            return Vec::new();
        }
        let w = self.shortcuts.width() / n as f32;
        (0..n)
            .map(|i| {
                Rect::from_min_size(
                    Pos2::new(self.shortcuts.left() + i as f32 * w, self.shortcuts.top()),
                    Vec2::new(w - 2.0, self.shortcuts.height()),
                )
            })
            .collect()
    }
}

// ── the envelope's shape ────────────────────────────────────────────────────

/// Which handle a hand is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Node {
    Attack,
    Decay,
    Sustain,
    Release,
}

/// The four nodes' positions inside `band`, given the crop the envelope runs
/// over and the four normalised values.
///
/// Pure arithmetic with no painter and no `Ui`, so the drawing and the hit
/// test read ONE set of coordinates. A node drawn from one derivation and
/// grabbed from another is the fault where a handle answers a press an inch
/// from where it is drawn, and it is invisible to every test that does not
/// compare the two.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Env {
    pub attack: Pos2,
    pub decay: Pos2,
    pub sustain: Pos2,
    pub release: Pos2,
}

impl Env {
    /// `band` is the envelope's own box — the cropped span of the wave,
    /// inset so a node at nought or one is still wholly on the glass.
    pub fn new(band: Rect, attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        let w = band.width();
        let top = band.top();
        let bottom = band.bottom();
        let level = bottom + (top - bottom) * sustain.clamp(0.0, 1.0);
        let a_x = band.left() + attack.clamp(0.0, 1.0) * A_SHARE * w;
        let d_x = band.left() + (A_SHARE + decay.clamp(0.0, 1.0) * D_SHARE) * w;
        let s_x = band.left() + (A_SHARE + D_SHARE + S_SHARE) * w;
        let r_x = s_x + release.clamp(0.0, 1.0) * R_SHARE * w;
        Self {
            attack: Pos2::new(a_x, top),
            decay: Pos2::new(d_x, level),
            sustain: Pos2::new(s_x, level),
            release: Pos2::new(r_x, bottom),
        }
    }

    /// The polyline the envelope draws, start to finish.
    pub fn points(&self, band: Rect) -> [Pos2; 5] {
        [
            Pos2::new(band.left(), band.bottom()),
            self.attack,
            self.decay,
            self.sustain,
            self.release,
        ]
    }

    pub fn at(&self, n: Node) -> Pos2 {
        match n {
            Node::Attack => self.attack,
            Node::Decay => self.decay,
            Node::Sustain => self.sustain,
            Node::Release => self.release,
        }
    }
}

/// The value a node's x lands on, inverting [`Env::new`] for `node`.
fn x_to_value(band: Rect, node: Node, x: f32) -> f32 {
    let w = band.width().max(1.0);
    let t = (x - band.left()) / w;
    let v = match node {
        Node::Attack => t / A_SHARE,
        Node::Decay => (t - A_SHARE) / D_SHARE,
        Node::Release => (t - (A_SHARE + D_SHARE + S_SHARE)) / R_SHARE,
        // A plateau has no time of its own; it is where the level is read.
        Node::Sustain => return f32::NAN,
    };
    v.clamp(0.0, 1.0)
}

/// The level a node's y lands on: the top of the band is full, the bottom
/// silence.
fn y_to_level(band: Rect, y: f32) -> f32 {
    let h = band.height().max(1.0);
    ((band.bottom() - y) / h).clamp(0.0, 1.0)
}

// ── reading the pushed face ─────────────────────────────────────────────────

/// The pushed value of `id`, or nothing if the host has not pushed it — a
/// face draws pushed data and nothing else.
fn value(live: &FaceLive, id: u32) -> Option<(f32, f32, &str)> {
    live.face
        .params
        .iter()
        .find(|p| p.id == id)
        .map(|p| (p.value, p.default, p.reading.as_str()))
}

/// Just the number, for the arithmetic that cannot draw without one.
fn v_of(live: &FaceLive, id: u32) -> f32 {
    value(live, id).map_or(0.0, |(v, _, _)| v)
}

/// **The Hz a detune of `cents` moves concert A**, as words.
///
/// The owner asked for the resulting Hz beside the cents, and this is the
/// arithmetic: a cent is a ratio, `2^(c/1200)`, so the offset is the
/// reference times that ratio less the reference. Rounded to a tenth,
/// because the tenth is the resolution a tuner shows and the hundredth would
/// be a number that changes while nothing audible does.
pub fn hz_offset(cents: f32) -> f32 {
    if !cents.is_finite() {
        return 0.0;
    }
    FINE_REF_HZ * (cents / 1200.0).exp2() - FINE_REF_HZ
}

/// The Fine row's reading: what the host printed, and the Hz beside it.
fn fine_reading(v: f32, pushed: &str) -> String {
    let cents = (v.clamp(0.0, 1.0) * 2.0 - 1.0) * FINE_CENTS;
    let hz = hz_offset(cents);
    // The host's own words carry the cents; this adds only the part it
    // cannot know, which is what that detune does to a pitch.
    format!("{pushed}  {hz:+.1} Hz")
}

/// The crop fields' words: three decimals of the fraction the grip stands
/// on. See the module header for why this is not the host's `reading`.
pub fn crop_text(v: f32) -> String {
    format!("{:.3}", v.clamp(0.0, 1.0))
}

/// The layout the last frame drew, for the tests: left in egui's own temp
/// store rather than on `FaceLive`, because the panel has no business
/// growing a field for a harness — `faces_analog`'s own bargain.
fn layout_key() -> egui::Id {
    egui::Id::new("faces_sampler::layout")
}

/// What a test needs to aim a gesture: where the panel put everything, and
/// where the four envelope nodes ended up.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placed {
    lay: Layout,
    band: Rect,
    env: Env,
}

// ── the panel ───────────────────────────────────────────────────────────────

/// The whole panel. Returns at most one ask a frame.
pub fn draw_sampler(ui: &mut egui::Ui, st: &FaceStyle, live: &mut FaceLive) -> Option<FaceAction> {
    // The room the panel may spend, asked before it spends anything: the
    // window less the preset strip `faces::draw` appends under every face.
    let foot = crate::faces::preset_foot_height(ui);
    let avail = ui.available_size();
    let body_h = (avail.y - foot).max(HEADER_H + NAME_H + WAVE_MIN + CROP_H + AIR * 3.0);
    let (body, _) = ui.allocate_exact_size(Vec2::new(avail.x, body_h), Sense::hover());
    let lay = Layout::new(body);
    let painter = ui.painter_at(body);
    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
    let mut out: Option<FaceAction> = None;

    // ── the header ──────────────────────────────────────────────────────
    painter.text(
        Pos2::new(lay.header.left() + 2.0, lay.header.center().y),
        Align2::LEFT_CENTER,
        "TANGENT SAMPLER",
        bold(TITLE_PT),
        st.s.ink,
    );
    let badge = Rect::from_center_size(
        Pos2::new(lay.header.right() - HEADER_H * 0.5, lay.header.center().y),
        Vec2::splat(HEADER_H - 6.0),
    );
    face::triad_badge(&painter, badge, st.s.ink);
    painter.line_segment(
        [
            Pos2::new(lay.header.left(), lay.header.bottom()),
            Pos2::new(lay.header.right(), lay.header.bottom()),
        ],
        Stroke::new(1.0_f32, st.s.edge),
    );

    // ── which file, or how to get one ───────────────────────────────────
    let info = &live.sampler;
    let (words, ink) = if info.name.is_empty() {
        (
            "No sample - press a file in the browser, or drop one on this window".to_owned(),
            st.s.faint,
        )
    } else if info.missing {
        (
            format!("{} - the file has moved; the sound is silent", info.name),
            face::lcd_ink(st, LcdTier::Primary),
        )
    } else {
        (format!("{}  {:.2}s", info.name, info.seconds), st.s.ink)
    };
    if lay.name.is_positive() {
        painter.text(
            Pos2::new(lay.name.left() + 2.0, lay.name.center().y),
            Align2::LEFT_CENTER,
            crate::drumpad_panel::cut_text(&painter, lay.name.width() - 4.0, &words, &bold(NAME_PT)),
            bold(NAME_PT),
            // The missing warning wears the glass's primary ink, which is
            // resolved against the glass; on the PANEL it would be an ink
            // fetched from a role and not measured on the ground it lands
            // on. So it lands on a plate of glass drawn under it.
            if info.missing { face::lcd_ink(st, LcdTier::Primary) } else { ink },
        );
        if info.missing {
            // A word inked for the glass has to have the glass under it.
            painter.rect_stroke(
                lay.name.shrink(1.0),
                2.0,
                Stroke::new(1.0_f32, st.s.edge),
                StrokeKind::Inside,
            );
        }
    }

    // ── the wave, the crop and the envelope ─────────────────────────────
    let placed = draw_stage(ui, &painter, st, live, &lay, &mut out);
    ui.data_mut(|d| d.insert_temp(layout_key(), placed));

    // ── the two crop numbers ────────────────────────────────────────────
    for (id, cell, word) in [
        (P_START, lay.crop_start, "START"),
        (P_END, lay.crop_end, "END"),
    ] {
        face::keep(
            &mut out,
            crop_field(ui, &painter, st, live, id, cell, word)
                .map(|value| FaceAction::Set { id, value }),
        );
    }

    // ── the ordinary controls, in the kit's own rows ────────────────────
    face::keep(&mut out, control_rows(ui, st, live, lay.rows));

    // ── the browser ─────────────────────────────────────────────────────
    face::keep(&mut out, browser(ui, &painter, st, live, &lay));

    // ── the three doors a sample comes in by ────────────────────────────
    face::keep(&mut out, take_a_drop(ui, body));
    out
}

/// The glass, the waveform, the crop's dimming and grips, and the four
/// envelope nodes. Returns where everything landed, for the tests.
fn draw_stage(
    ui: &mut egui::Ui,
    painter: &Painter,
    st: &FaceStyle,
    live: &FaceLive,
    lay: &Layout,
    out: &mut Option<FaceAction>,
) -> Placed {
    let wave = lay.wave;
    let band = wave.shrink2(Vec2::new(GRIP_HIT * 0.5, NODE_R + 3.0));
    let (start, end) = crop_of(live);
    let env = Env::new(
        crop_band(band, start, end),
        v_of(live, P_ATTACK),
        v_of(live, P_DECAY),
        v_of(live, P_SUSTAIN),
        v_of(live, P_RELEASE),
    );
    let placed = Placed { lay: *lay, band, env };
    if !wave.is_positive() {
        return placed;
    }
    face::lcd_glass(painter, wave, st);

    // The picture: min/max per column off the pyramid, one mesh. The
    // arrangement draws its clips exactly this way and there is no second
    // envelope anywhere in this file — `SamplerInfo::peaks` IS the picture.
    let mid = wave.center().y;
    let half = wave.height() * 0.42;
    if let Some(peaks) = live.sampler.peaks.as_deref() {
        let total = peaks.seconds().max(1.0e-9);
        let cols = wave.width().ceil() as usize;
        let dt = total / f64::from(wave.width().max(1.0));
        let mut mesh = egui::epaint::Mesh::default();
        mesh.reserve_vertices(cols * 2);
        mesh.reserve_triangles(cols * 2);
        let mut prev: Option<u32> = None;
        for i in 0..cols {
            let x = wave.left() + i as f32;
            if x > wave.right() {
                break;
            }
            let t = f64::from(i as f32) * dt;
            let (lo, hi) = peaks.minmax_s(t, t + dt);
            let y0 = mid - hi * half;
            let y1 = (mid - lo * half).max(y0 + 1.0);
            let top = mesh.vertices.len() as u32;
            let inside = {
                let f = (x - wave.left()) / wave.width().max(1.0);
                f >= start && f <= end
            };
            // **The wave is the SECOND tier and the envelope is the
            // first.** Both were the primary in the first rendering and the
            // envelope vanished into the waveform wherever it crossed it —
            // one green line over a green mass. The picture the envelope is
            // read against has to sit UNDER it.
            let ink = if inside {
                face::lcd_ink(st, LcdTier::Secondary)
            } else {
                face::lcd_ink(st, LcdTier::Label)
            };
            mesh.colored_vertex(Pos2::new(x, y0), ink);
            mesh.colored_vertex(Pos2::new(x, y1), ink);
            if let Some(p) = prev {
                mesh.add_triangle(p, p + 1, top);
                mesh.add_triangle(p + 1, top + 1, top);
            }
            prev = Some(top);
        }
        if !mesh.indices.is_empty() {
            painter.add(egui::Shape::mesh(mesh));
        }
    } else {
        painter.line_segment(
            [Pos2::new(wave.left(), mid), Pos2::new(wave.right(), mid)],
            Stroke::new(1.0_f32, face::lcd_ink(st, LcdTier::Label)),
        );
    }

    // **The cropped-out ends are DIMMED, not hidden.** A crop that erased
    // what it excluded would leave a hand no way to see what it had cut
    // off, which is the one thing somebody adjusting a crop is looking at.
    let x_of = |f: f32| wave.left() + f.clamp(0.0, 1.0) * wave.width();
    let veil = Color32::from_black_alpha(120);
    for r in [
        Rect::from_min_max(wave.min, Pos2::new(x_of(start), wave.bottom())),
        Rect::from_min_max(Pos2::new(x_of(end), wave.top()), wave.max),
    ] {
        if r.is_positive() {
            painter.rect_filled(r, 0.0, veil);
        }
    }

    // The two grips, each a line a hand can take a wide target on.
    for (id, f) in [(P_START, start), (P_END, end)] {
        let x = x_of(f);
        let hit = Rect::from_min_max(
            Pos2::new(x - GRIP_HIT * 0.5, wave.top()),
            Pos2::new(x + GRIP_HIT * 0.5, wave.bottom()),
        );
        let resp = ui.interact(hit, ui.id().with(("samp-grip", id)), Sense::click_and_drag());
        let lit = resp.hovered() || resp.dragged();
        painter.line_segment(
            [Pos2::new(x, wave.top() + 1.0), Pos2::new(x, wave.bottom() - 1.0)],
            Stroke::new(
                if lit { GRIP_W + 1.0 } else { GRIP_W },
                face::lcd_ink(st, if lit { LcdTier::Primary } else { LcdTier::Secondary }),
            ),
        );
        resp.clone()
            .on_hover_text("drag to crop; double-click or alt-click to put it back");
        let hand = resp.clone().interact(Sense::click());
        if face::home_gesture(&hand) {
            let home = value(live, id).map_or(if id == P_START { 0.0 } else { 1.0 }, |(_, d, _)| d);
            face::keep(out, Some(FaceAction::Set { id, value: home }));
        } else if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                let want = ((p.x - wave.left()) / wave.width().max(1.0)).clamp(0.0, 1.0);
                let value = if id == P_START {
                    want.min(end - CROP_MIN).max(0.0)
                } else {
                    want.max(start + CROP_MIN).min(1.0)
                };
                face::keep(out, Some(FaceAction::Set { id, value }));
            }
        }
    }

    // ── the envelope, over the crop it shapes ───────────────────────────
    let cband = crop_band(band, start, end);
    let pts = env.points(cband);
    let line = face::lcd_ink(st, LcdTier::Primary);
    for pair in pts.windows(2) {
        // A dark CASING under the line, so the envelope reads wherever it
        // crosses the waveform — the same trick a scope trace uses over a
        // grid, and the thing the first rendering of this panel was missing.
        painter.line_segment([pair[0], pair[1]], Stroke::new(4.0_f32, st.lcd_face));
    }
    for pair in pts.windows(2) {
        painter.line_segment([pair[0], pair[1]], Stroke::new(1.8_f32, line));
    }
    for (node, id) in [
        (Node::Attack, P_ATTACK),
        (Node::Decay, P_DECAY),
        (Node::Sustain, P_SUSTAIN),
        (Node::Release, P_RELEASE),
    ] {
        let at = env.at(node);
        let hit = Rect::from_center_size(at, Vec2::splat(NODE_HIT));
        let resp = ui.interact(hit, ui.id().with(("samp-node", id)), Sense::click_and_drag());
        let lit = resp.hovered() || resp.dragged();
        painter.circle_filled(at, if lit { NODE_R + 1.5 } else { NODE_R }, line);
        painter.circle_stroke(
            at,
            if lit { NODE_R + 1.5 } else { NODE_R },
            Stroke::new(1.0_f32, st.lcd_face),
        );
        resp.clone().on_hover_text(match node {
            Node::Attack => "attack - drag sideways; double-click or alt-click to reset",
            Node::Decay => "decay and sustain - drag sideways and up; double-click to reset",
            Node::Sustain => "sustain - drag up and down; double-click or alt-click to reset",
            Node::Release => "release - drag sideways; double-click or alt-click to reset",
        });
        let hand = resp.clone().interact(Sense::click());
        if face::home_gesture(&hand) {
            let home = value(live, id).map_or(0.0, |(_, d, _)| d);
            face::keep(out, Some(FaceAction::Set { id, value: home }));
            continue;
        }
        if !resp.dragged() {
            continue;
        }
        let Some(p) = resp.interact_pointer_pos() else {
            continue;
        };
        let wish = match node {
            Node::Attack | Node::Release => Some(FaceAction::Set {
                id,
                value: x_to_value(cband, node, p.x),
            }),
            Node::Sustain => Some(FaceAction::Set {
                id: P_SUSTAIN,
                value: y_to_level(cband, p.y),
            }),
            // **The decay node carries the sustain with it**, because that
            // is what the corner of an ADSR IS: the moment the fall stops
            // and the level it stopped at. Two asks in one frame would be a
            // queue, so it rides `SetPair`, which exists for exactly this.
            Node::Decay => Some(FaceAction::SetPair {
                id_a: P_DECAY,
                value_a: x_to_value(cband, Node::Decay, p.x),
                id_b: P_SUSTAIN,
                value_b: y_to_level(cband, p.y),
            }),
        };
        face::keep(out, wish);
    }
    placed
}

/// The crop's two fractions, ordered and never crossed.
fn crop_of(live: &FaceLive) -> (f32, f32) {
    let a = v_of(live, P_START).clamp(0.0, 1.0);
    let b = v_of(live, P_END).clamp(0.0, 1.0);
    if b > a + CROP_MIN {
        (a, b)
    } else {
        (a.min(1.0 - CROP_MIN), (a + CROP_MIN).min(1.0))
    }
}

/// The envelope's own box: the part of `band` the crop leaves.
fn crop_band(band: Rect, start: f32, end: f32) -> Rect {
    let l = band.left() + start * band.width();
    let r = band.left() + end * band.width();
    Rect::from_min_max(Pos2::new(l, band.top()), Pos2::new(r.max(l + 8.0), band.bottom()))
}

/// One crop number: the word, the value, and a drag that sets it.
///
/// A field and not a text box: this panel is a plugin window with no
/// keyboard focus of its own to lend, and every other number on a face is
/// set by dragging. What it promises is the PAIRING — the number it prints
/// is the number the grip above it stands on.
fn crop_field(
    ui: &mut egui::Ui,
    painter: &Painter,
    st: &FaceStyle,
    live: &FaceLive,
    id: u32,
    cell: Rect,
    word: &str,
) -> Option<f32> {
    if !cell.is_positive() {
        return None;
    }
    let (v, default, _) = value(live, id)?;
    let resp = ui.interact(cell, ui.id().with(("samp-crop", id)), Sense::click_and_drag());
    let lit = resp.hovered() || resp.dragged();
    painter.rect_filled(cell, 3.0, if lit { st.s.hover } else { st.s.well });
    painter.rect_stroke(cell, 3.0, Stroke::new(1.0_f32, st.s.edge), StrokeKind::Inside);
    let ground = if lit { st.s.hover } else { st.s.well };
    painter.text(
        Pos2::new(cell.left() + 6.0, cell.center().y),
        Align2::LEFT_CENTER,
        word,
        FontId::new(FIELD_PT, fonts::courier_bold()),
        face::quiet_ink(st, ground),
    );
    painter.text(
        Pos2::new(cell.right() - 6.0, cell.center().y),
        Align2::RIGHT_CENTER,
        crop_text(v),
        // Never `FontId::monospace` — see `face::slider_row`: this app
        // links egui without its default fonts, so that family is empty
        // and a galley in it lays out at zero by zero.
        FontId::new(FIELD_PT, fonts::courier()),
        // The panel's ink is measured against the panel; inside a well it
        // is the well that has to carry it.
        face::quiet_ink(st, ground),
    );
    resp.clone()
        .on_hover_text("drag to set; shift for fine; double-click or alt-click to reset");
    let hand = resp.clone().interact(Sense::click());
    if face::home_gesture(&hand) {
        return Some(default);
    }
    if resp.dragged() {
        let dx = resp.drag_delta().x;
        if dx != 0.0 {
            let per = if ui.input(|i| i.modifiers.shift) { FINE_PER_PT } else { DRAG_PER_PT };
            return Some((v + dx * per).clamp(0.0, 1.0));
        }
    }
    None
}

/// The ten ordinary controls, in two columns of the face kit's own rows.
///
/// The kit and not a second dial bank: `face::slider_row` and
/// `face::stepped_row` already carry this app's whole contract for a
/// control — the reset gesture, the hover that says so, the reading that
/// dims at rest — and a panel that re-spelled it would be a panel where
/// those promises drift. What is bespoke here is only the two things the
/// kit cannot know: the Hz beside the cents, and which ten of the sixteen
/// parameters belong in rows at all (the envelope's four are the nodes and
/// the crop's two are the grips).
fn control_rows(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    live: &FaceLive,
    rows: Rect,
) -> Option<FaceAction> {
    if !rows.is_positive() {
        return None;
    }
    // **ONE column, not two, and the reason is a screenshot.** The first
    // rendering of this panel put the ten rows in two columns, and every
    // reading in them was GONE: `face::slider_row` lays its reading out
    // right-to-left after the name and the slider have taken their shares,
    // and at half this panel's width what was left was fifty points for a
    // word that wants sixty. egui CLIPS a galley that overruns, so the row
    // measured perfectly, escaped nothing, and printed no value at all.
    // `no_word_is_clipped_by_the_room_it_was_given` is the test that came
    // out of it and it reads the clip rectangles off the shape stream,
    // because that is the only place the fault is visible.
    const IDS: [u32; 10] = [
        P_VOL, P_MODE, P_TRANSPOSE, P_FINE, P_PORTA, P_NOTEOFF, P_LOOP, P_XFADE, P_VOICES, P_BEND,
    ];
    let mut out = None;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rows).layout(*ui.layout()));
    for id in IDS {
        face::keep(&mut out, one_row(&mut child, st, live, id));
    }
    ui.advance_cursor_after_rect(rows);
    out
}

/// One parameter as a kit row, continuous or stepped by what the host said.
fn one_row(ui: &mut egui::Ui, st: &FaceStyle, live: &FaceLive, id: u32) -> Option<FaceAction> {
    let p = live.face.params.iter().find(|p| p.id == id)?;
    if p.steps >= 2 {
        let n = p.choices.len().max(2) as i32;
        let at = ((p.value.clamp(0.0, 1.0) * (n - 1) as f32).round() as i32).clamp(0, n - 1);
        let shown = p.choices.get(at as usize).map_or(p.reading.as_str(), |s| s.as_str());
        return face::stepped_row(ui, st, &p.name, shown).map(|ask| match ask {
            face::StepAsk::Home => FaceAction::Set { id, value: p.default },
            face::StepAsk::By(d) => FaceAction::Set {
                id,
                value: (at + d).rem_euclid(n) as f32 / (n - 1) as f32,
            },
        });
    }
    let reading = if id == P_FINE {
        fine_reading(p.value, &p.reading)
    } else {
        p.reading.clone()
    };
    face::slider_row(ui, st, &p.name, &reading, p.value, p.default)
        .map(|value| FaceAction::Set { id, value })
}

// ── the browser, the main door ──────────────────────────────────────────────

/// **The ink a FILE row is printed in**, measured on the pane's own ground.
///
/// It was `face::lcd_ink(st, LcdTier::Primary)` — the LCD's own colour,
/// which is resolved against the LCD GLASS and has no business anywhere
/// else. On a light theme that is an amber on a pale recess at about one
/// and a half to one, and the browser's whole listing was unreadable. The
/// first light-mode rendering of this panel is what found it; no test did,
/// because the pane dimmed the ink before painting it and the dimmed colour
/// matched nothing a sweep recognises.
///
/// So the LCD colour is only the STARTING point and the panel's own ground
/// decides where it ends up — `distinct_surface_at` walks it until it
/// clears the floor, exactly as the drumpad walks its meter green. Files
/// stay a different colour from folders, which is the whole reason not to
/// simply print them in the panel's ink.
fn file_ink(st: &FaceStyle) -> Color32 {
    crate::recorder_panel::distinct_surface_at(
        face::lcd_ink(st, LcdTier::Primary),
        row_ground(st),
        4.5,
    )
}

/// The ground a browser row's word lands on: the panel under two veils.
/// Measuring against the PANEL instead put the first fix at 3.6:1 — the
/// recess darkens the ground toward a dark ink, and the stripe darkens it
/// again.
fn row_ground(st: &FaceStyle) -> Color32 {
    crate::drumpad_panel::media_row_ground(
        st.s.bg,
        crate::recorder_panel::wants_light_ink(st.s.bg),
    )
}

/// The shortcut chips and the shared media pane.
///
/// **The pane is `drumpad_panel::draw_media_pane`**, the drumpad's own, and
/// the listing is pushed by the app exactly as the IR browser's is: a face
/// never opens a folder. What this adds over the drumpad's copy is the
/// SHORTCUTS, because the owner asked for Tangent's own recordings and not
/// only for other people's exports — *"must be able to drag any item in or
/// choose any midi or audio clip recorded in tangent as well"* — and the
/// project's `media/` is exactly where those live.
///
/// **A TAKES chip stood between them for one afternoon.** It was asked for
/// and struck out in the same session — "the takes section as a whole can
/// just go" — and it is gone rather than hidden: the row draws whatever the
/// app pushes, so removing the root removed the chip, and the two that are
/// left are wider for it.
fn browser(
    ui: &mut egui::Ui,
    painter: &Painter,
    st: &FaceStyle,
    live: &mut FaceLive,
    lay: &Layout,
) -> Option<FaceAction> {
    let mut out = None;
    // The roots the app pushed, and then DEF — which is not a place but an
    // act, so it is drawn here rather than pushed as a fourth root with a
    // path that would walk somewhere on a press.
    let cells = lay.shortcut_cells(live.samp_roots.len() + 1);
    let mut want: Option<String> = None;
    let chip = |ui: &mut egui::Ui, cell: Rect, id: &str, word: &str, lit: bool, hint: &str| {
        let resp = ui.interact(cell, ui.id().with(("samp-chip", id)), Sense::click());
        let ground = if lit {
            st.s.ink
        } else if resp.hovered() {
            st.s.hover
        } else {
            st.s.raised
        };
        painter.rect_filled(cell, 3.0, ground);
        painter.rect_stroke(cell, 3.0, Stroke::new(1.0_f32, st.s.edge), StrokeKind::Inside);
        painter.text(
            cell.center(),
            Align2::CENTER_CENTER,
            crate::drumpad_panel::cut_text(
                painter,
                cell.width() - 6.0,
                word,
                &FontId::new(9.5, fonts::courier_bold()),
            ),
            FontId::new(9.5, fonts::courier_bold()),
            // Lit, the chip's ground IS the panel's ink, so the word has to
            // be written in the panel's ground. Ink is a property of the
            // surface, and this surface just changed.
            if lit { st.s.bg } else { face::quiet_ink(st, ground) },
        );
        resp.clone().on_hover_text(hint.to_owned());
        resp.clicked()
    };
    for (cell, (word, path)) in cells.iter().zip(live.samp_roots.iter()) {
        let lit = !path.is_empty() && *path == live.samp_dir;
        let hint = if path.is_empty() { "nothing there yet" } else { path.as_str() };
        if chip(ui, *cell, word, word, lit, hint) && !path.is_empty() {
            want = Some(path.clone());
        }
    }
    // **DEF: make the folder you are standing in the one HOME opens on.**
    //
    // The drumpad's chip, word for word, for the explorer this panel shares
    // with it. The owner reported its absence as the panel's third fault —
    // "no default button for the home media explorer" — and it is the fault
    // it sounds like: a library five folders down had to be walked to from
    // scratch every time a face opened, because a face's browser state does
    // not outlive the window.
    //
    // Lit while the open folder already IS home, which is the drumpad's
    // `def_lit` exactly. Nothing extra is pushed for it: HOME's own root is
    // that folder, so the chip reads the answer off the row beside it.
    if let Some(cell) = cells.last() {
        let home = live.samp_roots.first().map(|(_, p)| p.as_str()).unwrap_or_default();
        let lit = !home.is_empty() && home == live.samp_dir;
        let hint = if lit {
            "HOME opens here".to_owned()
        } else {
            format!("open HOME here from now on (now {home})")
        };
        if chip(ui, *cell, "def", "DEF", lit, &hint) && !live.samp_dir.is_empty() {
            face::keep(
                &mut out,
                Some(FaceAction::SetMediaDefault { path: live.samp_dir.clone() }),
            );
        }
    }

    let ml = crate::drumpad_panel::MediaLayout::new(lay.browser);
    let pane = crate::drumpad_panel::MediaPane {
        dir: &live.samp_dir,
        entries: &live.samp_entries,
        scroll: live.samp_scroll,
        sel: live.samp_sel,
    };
    crate::drumpad_panel::draw_media_pane(
        painter,
        &ml,
        &pane,
        st.s.ink,
        st.s.bg,
        file_ink(st),
        crate::recorder_panel::wants_light_ink(st.s.bg),
    );

    // The `..` chip and every row, each its own press-list entry. A row
    // that draws and hit-tests and does nothing is the fault this codebase
    // has shipped four times; here there is no second match to forget.
    let up = ui.interact(ml.up, ui.id().with("samp-up"), Sense::click());
    up.clone().on_hover_text("out one folder");
    let mut pressed: Option<usize> = None;
    for (row, r) in ml.rows().iter().enumerate() {
        let at = live.samp_scroll + row;
        if live.samp_entries.get(at).is_none() {
            continue;
        }
        let resp = ui.interact(*r, ui.id().with(("samp-row", at)), Sense::click_and_drag());
        if resp.clicked() {
            pressed = Some(at);
        }
        // A FILE row is a drag SOURCE as well as a press — the same
        // `DraggedMedia` contract the drumpad's explorer publishes, so one
        // gesture reaches either panel.
        if resp.drag_started() {
            if let Some(e) = live.samp_entries.get(at).filter(|e| !e.dir) {
                let path = std::path::Path::new(&live.samp_dir).join(&e.name);
                egui::DragAndDrop::set_payload(
                    ui.ctx(),
                    crate::drumpad_panel::DraggedMedia(path),
                );
            }
        }
    }
    if let Some(at) = pressed {
        if let Some(e) = live.samp_entries.get(at).cloned() {
            if e.dir {
                want = Some(
                    std::path::Path::new(&live.samp_dir)
                        .join(&e.name)
                        .to_string_lossy()
                        .into_owned(),
                );
            } else {
                live.samp_sel = Some(at);
                face::keep(
                    &mut out,
                    Some(FaceAction::PickSample {
                        path: std::path::Path::new(&live.samp_dir)
                            .join(&e.name)
                            .to_string_lossy()
                            .into_owned(),
                    }),
                );
            }
        }
    }
    if up.clicked() {
        // Path arithmetic is string work and stays on this side of the
        // firewall; only the LISTING is the app's job. `faces::draw_ir`'s
        // own bargain, word for word.
        if let Some(parent) = std::path::Path::new(&live.samp_dir).parent() {
            want = Some(parent.to_string_lossy().into_owned());
        }
    }
    if let Some(dir) = want {
        live.samp_dir = dir;
        live.samp_scroll = 0;
        live.samp_sel = None;
    }
    // The wheel walks the listing, three rows a NOTCH of accrued intent —
    // per event it walked three rows thirty times per trackpad flick.
    let over = ui
        .ctx()
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|p| ml.pane.is_positive() && ml.pane.contains(p));
    let scroll = if over { ui.ctx().input(|i| i.raw_scroll_delta.y) } else { 0.0 };
    let steps = live.samp_wheel.steps(scroll);
    if steps != 0 {
        let max = live.samp_entries.len().saturating_sub(1);
        live.samp_scroll =
            (live.samp_scroll as isize - 3 * steps as isize).clamp(0, max as isize) as usize;
    }
    out
}

/// **The two doors that are a DROP**, taken on this window's own input.
///
/// An OS file drag arrives in `raw.dropped_files`; an in-app drag — the
/// explorer's own rows, and a clip dragged out of the arrangement — arrives
/// as a `DraggedMedia` payload in `egui::DragAndDrop`, which is context
/// state and so is shared with the window that started the drag. The face
/// takes a PATH and nothing else: it never opens the file, which is the
/// whole of the firewall.
///
/// Only the FIRST file of a multi-file drop is taken. A sampler holds one
/// sample; the drumpad's "N files onto N pads" has somewhere to put the
/// rest and this has not, and silently dropping four of five files is worse
/// than obviously taking one.
fn take_a_drop(ui: &egui::Ui, body: Rect) -> Option<FaceAction> {
    let ctx = ui.ctx();
    let dropped = ctx.input(|i| i.raw.dropped_files.clone());
    if let Some(path) = dropped.into_iter().find_map(|f| f.path) {
        return Some(FaceAction::PickSample {
            path: path.to_string_lossy().into_owned(),
        });
    }
    let over = ctx
        .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
        .is_some_and(|p| body.contains(p));
    let released = ctx.input(|i| i.pointer.any_released());
    if !over || !released {
        return None;
    }
    egui::DragAndDrop::take_payload::<crate::drumpad_panel::DraggedMedia>(ctx).map(|m| {
        FaceAction::PickSample {
            path: m.0.to_string_lossy().into_owned(),
        }
    })
}

/// The fixtures the tests and the screenshot share. One `a_face`, so the
/// panel a person looks at is the panel the sweeps measure — a shot built
/// from its own fixture is a picture of something nothing tests.
#[cfg(test)]
mod tests_support {
    use super::*;
    use crate::face::fit;

    pub fn style() -> FaceStyle {
        let set = crate::settings::Settings::default();
        let p = crate::recorder_panel::palette(&set);
        FaceStyle::new(p.bg, p.lcd, p.lcd_face)
    }

    /// A pushed face with the sampler's real ids and a file behind it.
    pub fn a_face(loaded: bool) -> FaceLive {
        let mut live = fit::a_worst_case_face(SAMPLER_PATH, ID_CEILING, STEPPED, false);
        for p in &mut live.face.params {
            p.value = match p.id {
                P_START => 0.10,
                P_END => 0.85,
                P_ATTACK => 0.30,
                P_DECAY => 0.40,
                P_SUSTAIN => 0.70,
                P_RELEASE => 0.50,
                _ => 0.5,
            };
            p.default = match p.id {
                P_END => 1.0,
                P_START => 0.0,
                _ => 0.25,
            };
        }
        if loaded {
            live.sampler = crate::ports::SamplerInfo {
                name: "a long sample name.wav".to_owned(),
                seconds: 2.5,
                peaks: Some(a_peaks()),
                missing: false,
            };
        }
        live.samp_dir = "/m/Library/Kicks".to_owned();
        live.samp_roots = vec![
            ("HOME".to_owned(), "/home/Samples".to_owned()),
            ("PROJECT".to_owned(), "/p/media".to_owned()),
        ];
        live.samp_entries = (0..40)
            .map(|i| crate::drumpad_panel::MediaEntry {
                name: format!("a rather long take name {i:02}.wav"),
                dir: i % 7 == 0,
            })
            .collect();
        live
    }

    pub fn a_peaks() -> std::sync::Arc<crate::peaks::Peaks> {
        let rate = 48_000u32;
        let n = rate as usize * 2;
        let v: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                (t * std::f32::consts::TAU * 220.0).sin() * (1.0 - t * 0.4)
            })
            .collect();
        crate::peaks::Peaks::build(&v, 1, rate).shared()
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::*;
    use super::*;
    use crate::face::fit;
    use crate::recorder_panel::contrast_ratio;

    /// One frame of the panel at the floor window, with these events.
    fn draw_once(
        ctx: &egui::Context,
        live: &mut FaceLive,
        st: &FaceStyle,
        events: Vec<egui::Event>,
    ) -> Option<FaceAction> {
        let mut asked = None;
        let floor = crate::dialogs::face_window_size(SAMPLER_PATH, None);
        let room = Vec2::new(floor.x, floor.y - crate::shell::CHROME_BAR_H);
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, room)),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        fit::dress(ui.style_mut(), &st.s);
                        asked = draw_sampler(ui, st, live);
                    });
            },
        );
        asked
    }

    fn placed(ctx: &egui::Context) -> Placed {
        ctx.data(|d| d.get_temp(layout_key())).expect("a layout was left")
    }

    /// Drive the panel through egui's own input: press at `at`, move by
    /// `by`, release, one frame each, and whatever it asked on the way.
    fn drag(
        ctx: &egui::Context,
        live: &mut FaceLive,
        st: &FaceStyle,
        at: Pos2,
        by: Vec2,
    ) -> Vec<FaceAction> {
        use egui::{Event, PointerButton};
        let mut asked = Vec::new();
        for events in [
            vec![Event::PointerMoved(at)],
            vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
            vec![Event::PointerMoved(at + by)],
            vec![Event::PointerMoved(at + by)],
            vec![Event::PointerButton {
                pos: at + by,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            vec![],
        ] {
            asked.extend(draw_once(ctx, live, st, events));
        }
        asked
    }

    fn click(ctx: &egui::Context, live: &mut FaceLive, st: &FaceStyle, at: Pos2) -> Vec<FaceAction> {
        drag(ctx, live, st, at, Vec2::ZERO)
    }

    fn sets(asked: &[FaceAction]) -> Vec<(u32, f32)> {
        let mut out = Vec::new();
        for a in asked {
            match a {
                FaceAction::Set { id, value } => out.push((*id, *value)),
                FaceAction::SetPair { id_a, value_a, id_b, value_b } => {
                    out.push((*id_a, *value_a));
                    out.push((*id_b, *value_b));
                }
                _ => {}
            }
        }
        out
    }

    /// The two spellings of the sentinel agree, and the voice table answers
    /// for it — the title the face wears comes from there.
    #[test]
    fn the_sentinel_is_spelled_once() {
        assert_eq!(SAMPLER_PATH, crate::dialogs::SAMPLER_PATH);
        assert!(crate::faces::is_voice_face(SAMPLER_PATH));
        let (name, about) = crate::faces::suite_unit(SAMPLER_PATH).expect("a title");
        assert_eq!(name, crate::dialogs::SAMPLER_NAME);
        assert_eq!(about, SAMPLER_ABOUT);
        assert!(
            !crate::faces::SUITE_UNITS.iter().any(|(p, ..)| *p == SAMPLER_PATH),
            "a voice in the effects table"
        );
    }

    /// **Every control names one pushed parameter, once.** The sixteen ids
    /// the contract publishes are each drawn exactly once — four as
    /// envelope nodes, two as crop grips and fields, ten as rows — and the
    /// stepped list is exactly the ones drawn as stepped rows.
    #[test]
    fn every_control_names_a_pushed_parameter_once() {
        let mut ids: Vec<u32> = Vec::new();
        ids.extend([P_ATTACK, P_DECAY, P_SUSTAIN, P_RELEASE, P_START, P_END]);
        ids.extend([P_VOL, P_MODE, P_TRANSPOSE, P_FINE, P_PORTA]);
        ids.extend([P_NOTEOFF, P_LOOP, P_XFADE, P_VOICES, P_BEND]);
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "an id is drawn twice");
        assert_eq!(n, ID_CEILING, "the panel draws {n} of {ID_CEILING} parameters");
        assert_eq!(ids, (0..ID_CEILING as u32).collect::<Vec<_>>());
        let live = a_face(true);
        for id in &ids {
            assert!(value(&live, *id).is_some(), "id {id} finds no pushed value");
        }
        for id in STEPPED {
            assert!(
                [P_MODE, P_NOTEOFF, P_LOOP, P_VOICES, P_BEND].contains(id),
                "id {id} is stepped but is not drawn as a stepped row"
            );
        }
    }

    /// **The nodes are drawn where they are grabbed.** A handle placed by
    /// one derivation and hit-tested by another answers a press an inch
    /// from where it sits, and no test that checks only the arithmetic can
    /// see it. So the drag is driven through egui at the node's own drawn
    /// centre and the ask has to be that node's parameter.
    #[test]
    fn every_envelope_node_is_grabbed_where_it_is_drawn() {
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let st = style();
        for (node, want) in [
            (Node::Attack, vec![P_ATTACK]),
            (Node::Decay, vec![P_DECAY, P_SUSTAIN]),
            (Node::Sustain, vec![P_SUSTAIN]),
            (Node::Release, vec![P_RELEASE]),
        ] {
            let mut live = a_face(true);
            let _ = draw_once(&ctx, &mut live, &st, Vec::new());
            let at = placed(&ctx).env.at(node);
            let asked = drag(&ctx, &mut live, &st, at, Vec2::new(9.0, -7.0));
            let got: Vec<u32> = sets(&asked).into_iter().map(|(id, _)| id).collect();
            assert!(!got.is_empty(), "{node:?} asked for nothing");
            for id in &got {
                assert!(want.contains(id), "{node:?} moved parameter {id}");
            }
            for id in &want {
                assert!(got.contains(id), "{node:?} never moved parameter {id}");
            }
        }
    }

    /// **A node is grabbable without pixel-hunting.** The target is more
    /// than twice the dot, and a press a third of the way out from the
    /// centre still takes it — which is the number that decides whether an
    /// envelope can be dialled in with a trackpad.
    #[test]
    fn a_node_is_taken_from_a_hand_that_missed_the_dot() {
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let st = style();
        let mut live = a_face(true);
        let _ = draw_once(&ctx, &mut live, &st, Vec::new());
        let at = placed(&ctx).env.at(Node::Release);
        let off = at + Vec2::splat(NODE_HIT * 0.5 - 1.5);
        assert!(NODE_HIT >= NODE_R * 2.0 * 2.0, "the target is not twice the dot");
        let asked = drag(&ctx, &mut live, &st, off, Vec2::new(-10.0, 0.0));
        assert!(
            sets(&asked).iter().any(|(id, _)| *id == P_RELEASE),
            "a press {:.1}pt off the dot missed the node",
            NODE_HIT * 0.5 - 1.5
        );
    }

    /// **The crop grips and the crop fields print one pair of numbers.**
    /// The owner asked for exactly this correspondence, and the way it
    /// breaks is subtle: a field fed from the host's pushed `reading` lags
    /// the grip by one push, so the two disagree on every frame of a drag.
    #[test]
    fn the_crop_fields_print_the_numbers_the_grips_stand_on() {
        let mut live = a_face(true);
        for p in &mut live.face.params {
            if p.id == P_START {
                p.value = 0.2468;
                p.reading = "nonsense the host has not caught up with".to_owned();
            }
            if p.id == P_END {
                p.value = 0.7531;
                p.reading = "also nonsense".to_owned();
            }
        }
        assert_eq!(crop_text(v_of(&live, P_START)), "0.247");
        assert_eq!(crop_text(v_of(&live, P_END)), "0.753");
        let (a, b) = crop_of(&live);
        assert!((a - 0.2468).abs() < 1e-6 && (b - 0.7531).abs() < 1e-6);
        // And the pair can never cross, however the host pushes them.
        for p in &mut live.face.params {
            if p.id == P_END {
                p.value = 0.0;
            }
        }
        let (a, b) = crop_of(&live);
        assert!(b > a, "the crop crossed: {a} -> {b}");
    }

    /// **Dragging a crop grip crops, and it cannot swallow the sample.**
    /// A start dragged past the end used to be the obvious way to make a
    /// sampler silent by accident.
    #[test]
    fn a_crop_grip_crops_and_never_crosses_its_partner() {
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let st = style();
        let mut live = a_face(true);
        let _ = draw_once(&ctx, &mut live, &st, Vec::new());
        let wave = placed(&ctx).lay.wave;
        let x = wave.left() + wave.width() * 0.10;
        let at = Pos2::new(x, wave.center().y);
        let asked = drag(&ctx, &mut live, &st, at, Vec2::new(wave.width(), 0.0));
        let last = sets(&asked)
            .into_iter()
            .filter(|(id, _)| *id == P_START)
            .next_back()
            .expect("the grip asked for nothing");
        assert!(last.1 <= 0.85 - CROP_MIN + 1e-6, "start ran past end: {}", last.1);
        assert!(last.1 > 0.10, "dragging right did not move the crop");
    }

    /// **The crop field answers a drag.** A field that draws a number and
    /// hit-tests it and does nothing is the fault this codebase has shipped
    /// four times.
    #[test]
    fn a_crop_field_answers_a_drag() {
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let st = style();
        for (id, cell) in [(P_START, 0usize), (P_END, 1usize)] {
            let mut live = a_face(true);
            let _ = draw_once(&ctx, &mut live, &st, Vec::new());
            let l = placed(&ctx).lay;
            let r = if cell == 0 { l.crop_start } else { l.crop_end };
            let asked = drag(&ctx, &mut live, &st, r.center(), Vec2::new(20.0, 0.0));
            assert!(
                sets(&asked).iter().any(|(got, _)| *got == id),
                "the {} field asked for nothing",
                if cell == 0 { "START" } else { "END" }
            );
        }
    }

    /// **Every control the panel draws answers a press.** One gesture at
    /// each target, driven through egui's own input: the four nodes, the
    /// two grips, the two fields, the ten rows, the shortcut chips and a
    /// browser row. The press list IS the drawing here, and this is what
    /// proves there is no target that draws and does nothing.
    #[test]
    fn every_drawn_control_answers_a_press() {
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let st = style();
        // The rows: each one, by walking the ids the panel puts in rows.
        for id in [P_VOL, P_MODE, P_TRANSPOSE, P_FINE, P_PORTA, P_NOTEOFF, P_LOOP, P_XFADE, P_VOICES, P_BEND]
        {
            let mut live = a_face(true);
            let _ = draw_once(&ctx, &mut live, &st, Vec::new());
            let rows = placed(&ctx).lay.rows;
            // Sweep the two columns for the row that answers for this id.
            let mut answered = false;
            let step = face::ROW_H + 2.0;
            let mut y = rows.top() + step * 0.5;
            // **A stepped row is walked by a CLICK and a slider by a DRAG.**
            // Probing both with one gesture is how a row gets certified by
            // a test that never actually reached it: a 14-point drag is not
            // a click, so the cycling button would answer nothing and the
            // sweep would blame the layout.
            let by = if STEPPED.contains(&id) { Vec2::ZERO } else { Vec2::new(14.0, 0.0) };
            while y < rows.bottom() && !answered {
                for x in [rows.left() + rows.width() * 0.30, rows.left() + rows.width() * 0.80] {
                    let mut probe = a_face(true);
                    let _ = draw_once(&ctx, &mut probe, &st, Vec::new());
                    let asked = drag(&ctx, &mut probe, &st, Pos2::new(x, y), by);
                    if sets(&asked).iter().any(|(got, _)| *got == id) {
                        answered = true;
                        break;
                    }
                }
                y += step;
            }
            assert!(answered, "no row answered for parameter {id}");
        }
        // A shortcut chip walks the browser. Asked with the number of chips
        // the FIXTURE carries: the row is drawn from what the app pushed, so
        // a hard-coded three would go on hit-testing a cell nothing draws.
        let mut live = a_face(true);
        let _ = draw_once(&ctx, &mut live, &st, Vec::new());
        let n = live.samp_roots.len();
        let chip = placed(&ctx).lay.shortcut_cells(n)[0];
        let _ = click(&ctx, &mut live, &st, chip.center());
        assert_eq!(live.samp_dir, "/home/Samples", "the HOME chip did not walk");
        // **And the DEF chip asks for the folder it is standing in.** It is
        // the last cell of the row and it is not a root, so the sweep above
        // cannot reach it — and a chip that draws, hit-tests and does
        // nothing is this codebase's most-shipped defect.
        let mut live = a_face(true);
        let _ = draw_once(&ctx, &mut live, &st, Vec::new());
        let cells = placed(&ctx).lay.shortcut_cells(live.samp_roots.len() + 1);
        let asked = click(&ctx, &mut live, &st, cells.last().expect("a DEF cell").center());
        assert!(
            asked.iter().any(|a| matches!(
                a,
                FaceAction::SetMediaDefault { path } if *path == live.samp_dir
            )),
            "the DEF chip asked for nothing"
        );
        // And it walks nowhere: DEF says where HOME goes, it does not go
        // there itself.
        assert_eq!(live.samp_dir, "/m/Library/Kicks", "the DEF chip moved the browser");

        // A FILE row asks for the sample under it.
        let mut live = a_face(true);
        live.samp_scroll = 1; // entry 1 is a file; entry 0 is a folder
        let _ = draw_once(&ctx, &mut live, &st, Vec::new());
        let l = placed(&ctx).lay;
        let ml = crate::drumpad_panel::MediaLayout::new(l.browser);
        let row = ml.rows()[0];
        let asked = click(&ctx, &mut live, &st, row.center());
        assert!(
            asked.iter().any(|a| matches!(a, FaceAction::PickSample { .. })),
            "a file row asked for nothing"
        );
        // And the `..` chip walks out.
        let mut live = a_face(true);
        let _ = draw_once(&ctx, &mut live, &st, Vec::new());
        let ml = crate::drumpad_panel::MediaLayout::new(placed(&ctx).lay.browser);
        let _ = click(&ctx, &mut live, &st, ml.up.center());
        assert_eq!(live.samp_dir, "/m/Library", "the `..` chip did not walk out");
    }

    /// **The Fine knob prints the Hz it moves**, beside the cents the host
    /// spelled. The owner asked for the Hz; the cents alone answer "how far
    /// out of tune" and never "by how much".
    #[test]
    fn the_fine_knob_prints_the_hz_it_moves() {
        // A hundred cents is a semitone; a semitone above concert A is
        // 466.16 Hz, so the offset is 26.16.
        assert!((hz_offset(100.0) - 26.16).abs() < 0.02, "{}", hz_offset(100.0));
        assert!(hz_offset(0.0).abs() < 1e-6);
        assert!(hz_offset(-100.0) < 0.0);
        let words = fine_reading(1.0, "+100 c");
        assert!(words.contains("+100 c"), "the host's own words were dropped: {words}");
        assert!(words.contains("+26.2 Hz"), "no Hz beside the cents: {words}");
        let centre = fine_reading(0.5, "0 c");
        assert!(centre.contains("+0.0 Hz"), "{centre}");
    }

    /// **The picture is the pushed pyramid and there is no second
    /// envelope.** A face that computed its own outline would be a second
    /// answer to a question the host already answered, and the two drift.
    #[test]
    fn the_waveform_is_drawn_from_the_pushed_peaks() {
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let st = style();
        let count = |live: &mut FaceLive| -> usize {
            let mut n = 0;
            let floor = crate::dialogs::face_window_size(SAMPLER_PATH, None);
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(floor.x, floor.y - crate::shell::CHROME_BAR_H),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            fit::dress(ui.style_mut(), &st.s);
                            let _ = draw_sampler(ui, &st, live);
                        });
                },
            );
            fn walk(s: &egui::Shape, n: &mut usize) {
                match s {
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, n)),
                    egui::Shape::Mesh(m) => *n += m.vertices.len(),
                    _ => {}
                }
            }
            for cs in &out.shapes {
                walk(&cs.shape, &mut n);
            }
            n
        };
        let with = count(&mut a_face(true));
        let without = count(&mut a_face(false));
        assert!(
            with > without + 200,
            "the pushed peaks drew nothing: {with} vertices against {without}"
        );
    }

    /// **The empty state says how to get a sample**, rather than showing an
    /// empty box somebody has to guess at.
    #[test]
    fn the_empty_state_says_how_to_load_one() {
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let st = style();
        let mut live = a_face(false);
        let words = fit::ink_on_its_surface(
            &ctx,
            crate::dialogs::face_window_size(SAMPLER_PATH, None),
            SAMPLER_ABOUT,
            &st.s,
            &mut |ui| {
                let _ = draw_sampler(ui, &st, &mut live);
            },
        );
        let said: String = words.iter().map(|(_, _, t)| t.as_str()).collect::<Vec<_>>().join(" | ");
        assert!(said.contains("No sample"), "the empty panel says nothing: {said}");
        assert!(
            said.contains("browser") || said.contains("drop"),
            "the empty panel does not say how to load one: {said}"
        );
    }

    /// **Every ordinary control PRINTS the value the host pushed.**
    ///
    /// The fault this is named for was found by looking at a rendering and
    /// by nothing else. The ten controls were laid out in two columns;
    /// `face::slider_row` lays its reading out last, right-to-left, after
    /// the name and the slider have taken their shares of the width; at half
    /// this panel's width what was left was not enough for the word, and
    /// egui answered by printing NOTHING. Five rows with a slider, a name,
    /// and no number anywhere on them.
    ///
    /// Every sweep in this file passed while that was true. The row
    /// allocated correctly, so `escape` saw nothing; the missing word cannot
    /// fail an ink test, because an ink test can only measure ink that was
    /// laid down. So this asks the one question that catches it: is the
    /// string the host pushed actually in the shapes?
    #[test]
    fn every_row_prints_the_reading_the_host_pushed() {
        let mut missing: Vec<String> = Vec::new();
        for font in fit::FONTS {
            let ctx = fit::a_ctx(font);
            let st = style();
            let window = crate::dialogs::face_window_size(SAMPLER_PATH, None);
            let room = Vec2::new(window.x, window.y - crate::shell::CHROME_BAR_H);
            let mut live = a_face(true);
            // **A reading nothing else could print, at the length a real
            // one is.** Both halves matter. Unique, so a row that printed
            // its neighbour's number cannot pass; and ten characters, which
            // is what "high shelf" is — the longest reading `builtin_fx`
            // prints, and the length at which the two-column layout ran out
            // of room. A short probe fits where a real word does not, and a
            // test that probes with one certifies a panel nobody can read.
            for p in &mut live.face.params {
                p.reading = format!("hi shelf{:02}", p.id);
                // And the longest NAME any of this unit's parameters wears.
                p.name = "Loop Crossfade".to_owned();
            }
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, room)),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            fit::dress(ui.style_mut(), &st.s);
                            egui::Frame::NONE
                                .inner_margin(egui::Margin::same(fit::MARGIN as i8))
                                .show(ui, |ui| {
                                    let _ = crate::faces::draw(ui, &st, &mut live);
                                });
                        });
                },
            );
            // **A word with no SIZE is a word nobody can read**, and it is
            // the shape the fault actually took: `FontId::monospace` lays
            // out at zero by zero in this app (egui is linked without its
            // default fonts, so that family is empty), so the shape is
            // emitted, `galley.text()` answers, and no pixel is drawn. A
            // test that only asked "is the string in the shapes" passed on
            // a panel with no numbers on it — this one did, before it
            // measured.
            fn walk(s: &egui::Shape, said: &mut Vec<String>) {
                match s {
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, said)),
                    egui::Shape::Text(t) => {
                        if t.galley.rect.width() > 0.5 && t.galley.rect.height() > 0.5 {
                            said.push(t.galley.text().to_owned());
                        }
                    }
                    _ => {}
                }
            }
            let mut said = Vec::new();
            for cs in &out.shapes {
                walk(&cs.shape, &mut said);
            }
            let all = said.join(" | ");
            // The five continuous rows print a reading; the five stepped
            // ones print their CHOICE, which the worst-case face spells
            // "high shelf" and the row wears on its button.
            for id in [P_VOL, P_TRANSPOSE, P_FINE, P_XFADE, P_PORTA] {
                if !all.contains(&format!("hi shelf{id:02}")) {
                    missing.push(format!("{}: parameter {id} printed no reading", font.key()));
                }
            }
            let named = said.iter().filter(|t| t.contains("Loop Crossfade")).count();
            if named < 10 {
                missing.push(format!("{}: only {named} of the ten rows printed a name", font.key()));
            }
            // And the Fine row carries the Hz the owner asked for, beside
            // whatever the host spelled.
            if !all.contains("Hz") {
                missing.push(format!("{}: the Fine row printed no Hz", font.key()));
            }
        }
        assert!(missing.is_empty(), "rows that print nothing:\n  {}", missing.join("\n  "));
    }

    /// **The unit chrome is the chrome, not a second one.** Preset, Save
    /// and Init are appended by `faces::draw` under every face and this
    /// panel builds none of them; BYPASS is deliberately absent, because a
    /// VOICE in bay 0 is already silenced by the lane's own mute and a
    /// second switch onto a different flag would be a unit that reads
    /// bypassed in one place and live in another.
    #[test]
    fn the_sampler_wears_the_preset_strip_and_no_bypass_row() {
        assert!(crate::faces::is_voice_face(SAMPLER_PATH), "a voice is what suppresses the row");
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let st = style();
        let mut live = a_face(true);
        live.bypassed = true;
        let words = fit::ink_on_its_surface(
            &ctx,
            crate::dialogs::face_window_size(SAMPLER_PATH, None),
            SAMPLER_ABOUT,
            &st.s,
            &mut |ui| {
                let _ = crate::faces::draw(ui, &st, &mut live);
            },
        );
        let said: Vec<&str> = words.iter().map(|(_, _, t)| t.as_str()).collect();
        for want in ["PRESET", "Save", "Init"] {
            assert!(said.contains(&want), "the chrome has no {want}: {said:?}");
        }
        assert!(
            !said.iter().any(|t| t.contains("BYPASS")),
            "a voice drew a bypass row: {said:?}"
        );
    }

    /// **Init walks every parameter the host pushed**, not only the ones
    /// this panel draws — so a sampler whose engine grows a knob the face
    /// has no room for is still reset whole. The strip builds the ask; what
    /// this pins is that the panel's own return does not swallow it.
    #[test]
    fn init_asks_for_every_pushed_default() {
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let st = style();
        let mut live = a_face(true);
        // One more parameter than the face knows about, as a newer engine
        // would push it.
        live.face.params.push(crate::ports::BuiltinFaceParam {
            id: 99,
            symbol: "later".to_owned(),
            name: "Later".to_owned(),
            units: String::new(),
            default: 0.75,
            reading: "later".to_owned(),
            choices: Vec::new(),
            steps: 0,
            value: 0.1,
        });
        let mut asked = None;
        let window = crate::dialogs::face_window_size(SAMPLER_PATH, None);
        let room = Vec2::new(window.x, window.y - crate::shell::CHROME_BAR_H);
        // Two frames: the first lays the strip out, the second presses Init
        // where the first put it.
        let mut init_at = None;
        for pass in 0..2 {
            let events = match (pass, init_at) {
                (1, Some(at)) => vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    },
                ],
                _ => Vec::new(),
            };
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, room)),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            fit::dress(ui.style_mut(), &st.s);
                            egui::Frame::NONE
                                .inner_margin(egui::Margin::same(fit::MARGIN as i8))
                                .show(ui, |ui| {
                                    face::keep(&mut asked, crate::faces::draw(ui, &st, &mut live));
                                });
                        });
                },
            );
            if pass == 0 {
                fn walk(s: &egui::Shape, at: &mut Option<Pos2>) {
                    match s {
                        egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, at)),
                        egui::Shape::Text(t) if t.galley.text() == "Init" => {
                            *at = Some(t.pos + t.galley.rect.size() * 0.5);
                        }
                        _ => {}
                    }
                }
                for cs in &out.shapes {
                    walk(&cs.shape, &mut init_at);
                }
                assert!(init_at.is_some(), "the strip drew no Init");
            }
        }
        let Some(FaceAction::SetMany { sets }) = asked else {
            panic!("Init asked for nothing");
        };
        assert_eq!(sets.len(), ID_CEILING + 1, "Init skipped a pushed parameter");
        assert!(
            sets.iter().any(|(id, v)| *id == 99 && (*v - 0.75).abs() < 1e-6),
            "Init skipped a parameter the face does not draw"
        );
    }

    /// **The floor is a promise**: at the window it opens at and at the one
    /// it grows to, on every font, the whole panel and the preset strip
    /// under it stay inside. Through `faces::draw`, so the strip is counted.
    #[test]
    fn the_sampler_is_whole_at_its_own_window_on_every_font() {
        let st = style();
        let about = crate::faces::suite_unit(SAMPLER_PATH).map_or("", |(_, a)| a);
        let floor = crate::dialogs::face_window_size(SAMPLER_PATH, None);
        let ceiling = crate::dialogs::face_window_size(SAMPLER_PATH, Some(Vec2::new(3840.0, 2160.0)));
        let mut bad: Vec<String> = Vec::new();
        for font in fit::FONTS {
            let ctx = fit::a_ctx(font);
            for (edge, window) in [("opens at", floor), ("grown to", ceiling)] {
                let mut live = a_face(true);
                let (right, bottom) = fit::escape(&ctx, window, about, &st.s, &mut |ui| {
                    let _ = crate::faces::draw(ui, &st, &mut live);
                });
                if right > fit::SLACK {
                    bad.push(format!(
                        "{} {edge} {}x{}: {right:.1} off the right",
                        font.key(),
                        window.x,
                        window.y
                    ));
                }
                if bottom > fit::SLACK {
                    bad.push(format!(
                        "{} {edge} {}x{}: {bottom:.1} off the bottom",
                        font.key(),
                        window.x,
                        window.y
                    ));
                }
            }
        }
        assert!(bad.is_empty(), "the sampler is drawn outside its own window:\n  {}", bad.join("\n  "));
    }

    /// **Ink on its surface, on every theme**, both modes, at the floor.
    /// Read off the shape stream, not off the palette: a test that checks
    /// the palette agrees with whatever the drawing code does.
    #[test]
    fn the_sampler_prints_its_ink_on_the_surface_it_lands_on_on_every_theme() {
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let floor = crate::dialogs::face_window_size(SAMPLER_PATH, None);
        let mut bad: Vec<String> = Vec::new();
        let mut words_seen = usize::MAX;
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let p = crate::recorder_panel::palette(&set);
                let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
                let mut live = a_face(true);
                let words = fit::ink_on_its_surface(&ctx, floor, SAMPLER_ABOUT, &st.s, &mut |ui| {
                    let _ = crate::faces::draw(ui, &st, &mut live);
                });
                let mut counted = 0usize;
                for (ink, under, text) in &words {
                    let floor_ratio = if *ink == st.s.ink {
                        4.5
                    } else if *ink == st.s.faint
                        || *ink == face::quiet_ink(&st, st.s.well)
                        || *ink == face::quiet_ink(&st, st.s.raised)
                        || *ink == face::quiet_ink(&st, st.s.hover)
                    {
                        3.0
                    } else if *ink == face::lcd_ink(&st, LcdTier::Primary) {
                        3.0
                    } else if *ink == file_ink(&st) {
                        // The browser's file rows: the fault this floor is
                        // here for was an LCD amber printed on a pale
                        // recess at 1.5:1, and it shipped past every sweep
                        // because the pane dimmed the ink before painting.
                        4.5
                    } else {
                        continue;
                    };
                    counted += 1;
                    let r = contrast_ratio(*ink, *under);
                    if r < floor_ratio {
                        bad.push(format!(
                            "{} {}: \"{text}\" at {r:.2}:1",
                            theme.name,
                            if dark { "dark" } else { "light" }
                        ));
                    }
                }
                words_seen = words_seen.min(counted);
            }
        }
        assert!(words_seen >= 12, "the sweep recognised only {words_seen} words");
        assert!(bad.is_empty(), "unreadable on:\n  {}", bad.join("\n  "));
    }

    /// **The browser's two row inks read on the ground the rows are drawn
    /// on**, which is the panel under two black veils and not the panel.
    ///
    /// Asked directly rather than through `fit::ink_on_its_surface`,
    /// because that sweep reads the ground off the last OPAQUE rectangle
    /// under a word and both veils are translucent — so it answers with the
    /// panel, which is exactly the surface that is not there. The fault it
    /// missed: an LCD amber, chosen against dark glass, printed on a pale
    /// recess at 3.6:1, found by looking at a light-theme rendering.
    #[test]
    fn the_browsers_rows_read_on_the_ground_they_are_drawn_on() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let p = crate::recorder_panel::palette(&set);
                let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
                let ground = row_ground(&st);
                for (what, ink) in [("a file", file_ink(&st)), ("a folder", st.s.ink)] {
                    let r = contrast_ratio(ink, ground);
                    if r < 4.5 {
                        bad.push(format!(
                            "{} {}: {what} at {r:.2}:1",
                            theme.name,
                            if dark { "dark" } else { "light" }
                        ));
                    }
                }
                // And they stay TELLING APART: a browser that prints files
                // and folders in one colour has lost the only thing its
                // rows say beyond their names.
                let d = crate::recorder_panel::delta_e(file_ink(&st), st.s.ink);
                if d < 10.0 {
                    bad.push(format!("{}: files and folders are one colour ({d:.1})", theme.name));
                }
            }
        }
        assert!(bad.is_empty(), "unreadable browser rows:\n  {}", bad.join("\n  "));
    }

    /// **The envelope's arithmetic inverts.** Every node put at a value and
    /// read back off its own x gives the value again — the one property
    /// that keeps a drag from creeping while the hand is still.
    #[test]
    fn a_node_read_back_off_its_own_x_is_where_it_was_put() {
        let band = Rect::from_min_size(Pos2::new(20.0, 10.0), Vec2::new(400.0, 90.0));
        for v in [0.0_f32, 0.13, 0.5, 0.87, 1.0] {
            let e = Env::new(band, v, v, v, v);
            for (node, at) in [
                (Node::Attack, e.attack),
                (Node::Decay, e.decay),
                (Node::Release, e.release),
            ] {
                let back = x_to_value(band, node, at.x);
                assert!((back - v).abs() < 1e-4, "{node:?} at {v} read back {back}");
            }
            let level = y_to_level(band, e.decay.y);
            assert!((level - v).abs() < 1e-4, "sustain at {v} read back {level}");
        }
        // And the plateau never runs backwards under the corner.
        let e = Env::new(band, 1.0, 1.0, 0.5, 1.0);
        assert!(e.decay.x <= e.sustain.x + 1e-3, "the decay ran past the plateau");
        assert!(e.release.x <= band.right() + 1e-3, "the release ran off the glass");
    }
}

#[cfg(test)]
mod shot {
    use super::tests_support::*;
    use super::*;
    use crate::face::fit;

    /// **Render the panel to a file, so a person can look at it.**
    ///
    /// Not a test of anything: an eye. The ink law — ink is a property of
    /// the surface it lands on — has been broken nine times in this
    /// codebase and every one of them was found by looking at a rendering,
    /// never by a test. The sweeps in this file measure contrast off the
    /// shape stream and they cannot see a node hidden under a veil, a
    /// waveform that came out flat, or a panel that is technically legible
    /// and unreadable.
    ///
    /// `composite.rs`'s `mod shot` photographs the COMPOSITE and its
    /// `frame_painting` is private to that file, so a face — a window, not
    /// part of the video — cannot be reached that way. This is `busy.rs`'s
    /// software rasteriser instead, lifted into `face::fit` so the next
    /// face's shot costs thirty lines.
    ///
    ///     IVORY_SHOT=/tmp/sampler.png \
    ///       cargo test -p tangent-ui --lib faces_sampler::shot \
    ///       -- --ignored --nocapture
    ///
    /// `IVORY_SHOT_THEME=<name>` and `IVORY_SHOT_LIGHT=1` pick the dress,
    /// the same two `shot::window` honours; `IVORY_SHOT_EMPTY=1` shoots the
    /// panel with nothing loaded.
    #[test]
    #[ignore = "writes a picture for a person to look at"]
    fn draw_it() {
        let Ok(out) = std::env::var("IVORY_SHOT") else {
            eprintln!("IVORY_SHOT not set");
            return;
        };
        let mut set = crate::settings::Settings::default();
        if let Ok(name) = std::env::var("IVORY_SHOT_THEME") {
            match crate::theme::factory().into_iter().find(|t| t.name == name) {
                Some(t) => {
                    t.apply(&mut set);
                    set.theme_name = name;
                }
                None => eprintln!("no theme called {name}"),
            }
        }
        if std::env::var("IVORY_SHOT_LIGHT").is_ok() {
            set.dark_mode = false;
        }
        let p = crate::recorder_panel::palette(&set);
        let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
        let loaded = std::env::var("IVORY_SHOT_EMPTY").is_err();
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let window = crate::dialogs::face_window_size(SAMPLER_PATH, None);
        let mut live = a_face(loaded);
        let (w, h, rgba) = fit::shoot(&ctx, window, SAMPLER_ABOUT, &st.s, &mut |ui| {
            let _ = crate::faces::draw(ui, &st, &mut live);
        });
        fit::write_png(std::path::Path::new(&out), &rgba, w, h);
        println!("wrote {out} ({w}x{h}, {} mode)", if set.dark_mode { "dark" } else { "light" });
    }
}
