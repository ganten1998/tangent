//! The nylon guitar: six plucked strings on a hollow body, heard through two
//! close microphones.
//!
//! **Why modal, and not the waveguide the clav and the old piano use.** A
//! plucked string is the one excitation a mode bank can take EXACTLY: the
//! finger holds the string in a known shape and lets go, so every mode starts
//! from its share of that shape and zero velocity — an initial condition, not
//! a force pulse to be integrated. The reference instrument's losses are a law
//! of absolute frequency with the body's admittance bumps in it and a string
//! of its own per partial; a per-mode decay rate places each of those exactly
//! where a loop filter can only approximate them (and a loop's
//! linear-interpolation read would cap the treble's second partial at about a
//! second where the reference rings two). Each mode's two complex output taps
//! are the two microphones, the per-mode damper is the left hand lifting, and
//! the bridge drive feeds the open strings and the body. So this file is
//! `modal::Bank` driven the way a guitar is played.
//!
//! **The parts, and where each number came from.** Every constant below was
//! fitted to the reference's own renders by the scripts in the lane's
//! scratch (`lossfit3.py`, `radfit.py`, `fleshfit.py`), and the tests pin
//! each one beside its measurement:
//!
//! 1. *The fingerboard.* Six strings at E2 A2 D3 G3 B3 E4, first position
//!    (frets 0-4, then the top string to its 19th fret), and a string selector
//!    the frame owns (`Fretboard`): one note per string, so a second that
//!    would share a string moves to the next string down that reaches it.
//! 2. *The pluck.* A triangle with its apex at a fixed DISTANCE from the bridge
//!    (0.197 of the open string, so the comb's notches move with the fret),
//!    released from rest; its spectrum shaped by how the string leaves the
//!    finger — the flesh's slow release, the nail's sharp one, blended by
//!    velocity; and a contact phase before the release in which the finger
//!    rubs the string.
//! 3. *The losses.* The string material's own loss, rising steeply with
//!    frequency, plus what the bridge takes: tension over speaking length
//!    times the body's conductance at the partial's absolute frequency, a
//!    curve with the air mode and the top's first resonances in it, seen a
//!    little differently from each string's place on the saddle.
//! 4. *The radiation.* What the reference's microphones hear per newton at
//!    the bridge, recovered from its own attack spectra, with the two
//!    microphones hearing a partial in step at low frequency and apart above
//!    a few hundred hertz.
//! 5. *The open strings and the air.* `Fretboard` rings the six strings
//!    nobody is holding from the bridge's motion, and the hollow body's air
//!    mode from the same drive.
//!
//! **Real-time contract.** `GuitarVoice::new` and `Fretboard::new` allocate
//! everything; `strike`, `render` and the fretboard's calls allocate nothing
//! (`a_pluck_and_a_block_allocate_nothing` counts it).

use super::modal::{Bank, ModeSpec, BLOCK, C64, CULL_FLOOR};
use super::MAX_PARAMS;

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, LN_10, LN_2, PI, SQRT_2, TAU};

// ── the fingerboard ──────────────────────────────────────────────────────

/// Six strings. Index 0 is string 1, the top E; index 5 the bass E.
pub const STRINGS: usize = 6;

/// Each string's open pitch, MIDI, string 1 to string 6: the standard tuning.
pub const OPEN: [u8; STRINGS] = [64, 59, 55, 50, 45, 40];

/// The compass: the bass E open to the 19th fret of the top E — a classical
/// guitar's whole fingerboard, which is also everything the reference plays
/// (its scores reach B5 and stop). Keys outside it are silent.
pub const LOWEST: u8 = 40;
pub const HIGHEST: u8 = 83;

/// The last fret.
const FRETS: u8 = 19;

/// How far up another string the selector moves a note that would share a
/// string: within twelve frets, the reach of one hand position plus a shift.
pub const REACH: u8 = 12;

/// Strings 4, 5 and 6 are wound (a nylon core under a metal winding);
/// strings 1 to 3 are plain nylon. The two take a fingertip differently
/// (`flesh`).
const WOUND: [bool; STRINGS] = [false, false, false, true, true, true];

/// The scale: an open string's speaking length, metres. 650 mm, the
/// classical standard.
const SCALE_M: f64 = 0.650;

/// String tensions, newtons: a normal-tension nylon set (about 5.3 to 7.3
/// kilogram-force per string). They set how hard each string pulls on the
/// bridge for a given shape, so they are each string's loudness; the bridge
/// LOSS is fitted per string directly (`bridge_loss`) and does not read them.
const TENSION: [f64; STRINGS] = [70.0, 53.0, 52.0, 63.0, 69.0, 72.0];

/// Where a note is played: a string (0 = string 1) and a fret.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Place {
    pub string: usize,
    pub fret: u8,
}

impl Place {
    /// First position: the highest string whose open pitch is at or under
    /// the note, which in standard tuning is always within frets 0-4, and the
    /// top string up to its 19th fret above E4. This is where the reference's
    /// measured pluck combs and stiffnesses put 17 of its 19 notes (the
    /// analysis's beta and B tables). `None` outside the compass.
    pub fn home(pitch: u8) -> Option<Place> {
        if !(LOWEST..=HIGHEST).contains(&pitch) {
            return None;
        }
        OPEN.iter().position(|&open| pitch >= open).map(|s| Place {
            string: s,
            fret: pitch - OPEN[s],
        })
    }

    /// The note on string `s`, if that string reaches it.
    pub fn on(s: usize, pitch: u8) -> Option<Place> {
        let open = *OPEN.get(s)?;
        if pitch >= open && pitch - open <= FRETS {
            Some(Place {
                string: s,
                fret: pitch - open,
            })
        } else {
            None
        }
    }

    /// The speaking length, metres: the scale shortened by the fret.
    pub fn length(self) -> f64 {
        SCALE_M * 2.0_f64.powf(-f64::from(self.fret) / 12.0)
    }
}

/// Equal temperament, A4 = 440. A guitar is fretted, so every note sits
/// where its fret puts it.
fn f_equal(pitch: u8) -> f64 {
    440.0 * 2.0_f64.powf((f64::from(pitch) - 69.0) / 12.0)
}

// ── the pluck ────────────────────────────────────────────────────────────

/// Where the finger plucks, as a fraction of the OPEN string from the bridge.
///
/// **A distance, not a fraction.** The reference's pluck combs (the analysis,
/// G1 corrected: two estimators within 0.005) put the notch of every
/// first-position note at 0.197 x 2^(fret/12) of its speaking length: the
/// right hand stays where it is, 128 mm from the saddle, while the left hand
/// shortens the string under it. So an open string notches at n = 5, 10, 15
/// and the 12th fret at n = 2.5. POSITION scales the distance.
pub const BETA_OPEN: f64 = 0.197;

/// **Never nearer the fretting finger than the reference's own hand.** A
/// distance fixed from the bridge is a growing fraction of what speaks as
/// the fret climbs: at the 19th it is 0.586 of it, the reference's own last
/// pluck point (0.414 of the speaking length from the fret). POSITION scales
/// the distance, and at its top (twice the distance) the first draft plucked
/// the 16th fret and above ON the fret (clamped at 0.98): a string that
/// barely moves, G#5 to B5 20 dB under the default (the lane verifier's
/// chromatic scan). So the fraction stops where the reference's neck stops
/// it, which every default note already respects (the 19th fret lands on it
/// exactly), and a pluck point and its mirror about the middle give the same
/// modes, so the cap is no colour of its own.
fn beta_max() -> f64 {
    BETA_OPEN * 2.0_f64.powf(f64::from(FRETS) / 12.0)
}

/// The finger's force at full velocity, newtons: what a pluck IS. A finger
/// pushes; how far the string goes under it follows from the string —
/// `h = F beta (1 - beta) L / T`, the triangle whose tension balances the
/// push — so a short, stiff fretted string is drawn less far than an open
/// one by the same finger, and the bridge feels the same `F (1 - beta)` step
/// wherever on the neck it is played. 1.75 N draws the open bass E 2.5 mm.
/// (The first draft fixed the DEPTH instead: the 19th fret then sounded 11
/// dB over the open strings by the radiation fit's per-note offsets, and its
/// strain three times larger made phantoms that pulled B5's measured
/// stiffness to a third.)
const PLUCK_N: f64 = 1.75;

/// **A finger is not a point.** The string leaves the fingertip across its
/// width, so the comb `sin(n pi beta)` never quite reaches zero: no partial
/// of a plucked string is left with exactly nothing. The reference's open
/// strings notch at n5 and n10 about 13-16 dB under the comb's own envelope
/// once its radiation is taken out (A2: n5 -25, n10 -33 dB re n1), where an
/// ideal point at its 0.197 leaves n5 22 dB under; a floor of 0.06 on
/// `|sin|` (-24 dB) keeps every notch a notch and none a hole. The first
/// draft had none: A4's 19th partial sits on a null (19 x 0.263 = 4.997),
/// was left out entirely, and the comb estimator took that one -171 dB hole
/// for a notch at n = 19 (beta 0.053 against 0.263).
const NOTCH_FLOOR: f64 = 0.06;

/// Amplitude as velocity to this power: the reference's fundamental rises
/// 23.7 to 26.3 dB from MIDI 10 to 127 on A2/A3/A4 (`gvelramp`), which is
/// v^1.07 to v^1.19, and its A3's 300 ms RMS 26.7 dB (the critic's
/// per-instrument law: 26.8). 1.11 is 24.5 dB on the fundamental (read as
/// 24.2-24.4: at MIDI 127 the glide, `GLIDE`, carries the attack's partials
/// past the analyser's 0.4 % search); with the finger's brightness carried on
/// to MIDI 127 (`BLEND`) the A3 300 ms RMS then spans 27.6 dB (lane render,
/// `velsweep.py`), where 1.15 made it 29.5 and 1.08 left the fundamental
/// 23.6.
const VELOCITY_EXP: f64 = 1.11;

/// The finger: how much of the nail's quick release a pluck has, by MIDI
/// velocity. The reference blends its plucking brightness through three
/// points, at MIDI velocity 41, 70 and 98 (its manual, section 4.3), and
/// they are the knots here, with both ends of the velocity range.
///
/// **The weights are its measured brightness, not its preset's numbers.**
/// The first draft put the preset's 0.3 / 0.8 / 1.0 on the knots as nail
/// weights and held them flat outside: brightness (mean n5..n10 re n1) then
/// jumped 7.5 dB between MIDI 41 and 70 and stopped dead under 41 and over 98,
/// where the reference's rises steadily from 40 to 127 (A2 +8.2 to +12.2 dB
/// over 100-127, +1.4 by 40; the lane verifier, finding 7). Fitted to the
/// reference's own curve (`blendfit.py`: `m_velcurve.py` on its velocity
/// score, the mean of A2/A3/A4, re MIDI 10: 0 +0.7 +0.9 +3.4 +3.5 +7.2 +9.1
/// +10.6 +11.9 dB at 10..127), with the MIDI-80 weight held where it was
/// (0.871: VOICING's cut there, -20..-23 dB, is the reference's hardness-0
/// one): 0.33 dB rms. That curve is the three notes' mean, and its A3 is the
/// brightest of them where the reference's A3 is the dullest (+8.3 against
/// +12.2 and +15.1): at full excursion the lane's A3 brightened +12 and its
/// 300 ms RMS spanned 30.2 dB against the critic's 26.8. So each knot's
/// distance from the MIDI-80 weight is three quarters of the fit's: the
/// spans become +11.0 / +8.9 / +8.2 dB (lane render, `analyze.py`; the
/// brief's +8..+15), still rising over 100-127 and 10-40 as the reference's
/// does. Above 1 the release is sharper than the stock nail's — a harder
/// push lets the string go faster — and the knots' slopes carry on past
/// both ends.
const BLEND: [(f64, f64); 5] = [(10.0, 0.511), (41.0, 0.570), (70.0, 0.714), (98.0, 1.154), (127.0, 1.532)];

/// The velocity the radiation table was measured at: the reference's
/// decay score plays every note at MIDI 80, so `RADIATION` is what the
/// microphones hear of a MIDI-80 pluck, and the finger's spectrum is applied
/// RELATIVE to that one (`excitation`).
const REFERENCE_VELOCITY: f64 = 80.0;

/// The flesh's release, as the cut it makes against a nail pluck.
///
/// **Measured, per kind of string** (`fleshfit.py` on the reference's
/// hardness-0 render against its shipped one, rms residual 3.7 dB over 85
/// partials): on the wound strings the flesh leaves everything under 319 Hz
/// alone and cuts above it, steeply (a 13th-order knee: A2 n3 -6, n4 -15, n5
/// -20 dB); on the plain nylon strings the cut sits just above the
/// fundamental, at 1.66 f0, as a wall (A3 n2 -24, E4 n2 -17, B5 n2 -23 dB). A
/// slow release through a soft pad, 0.5 ms on the bass strings — inside the
/// brief's 0.4-0.6 — and about two-thirds of a period on the treble ones,
/// whose thinner nylon slips off the pad as soon as it is drawn. Under the
/// knee a floor of -24 dB is left: the edge of the pad, which is never
/// perfectly soft.
const FLESH_WOUND_HZ: f64 = 319.0;
const FLESH_WOUND_ORDER: f64 = 13.0;
const FLESH_PLAIN_F0: f64 = 1.66;
const FLESH_PLAIN_ORDER: f64 = 40.0;
const FLESH_FLOOR_DB: f64 = -24.0;

/// The nail's own release, about 0.027 ms: a one-pole knee at 6 kHz, the
/// brief's 0.02-0.03 ms.
const NAIL_HZ: f64 = 6000.0;

/// A hardness-independent part of the release above about 5 kHz: the
/// reference's hardness-0 render recovers to within 2-10 dB of its shipped
/// one at 7-10 kHz (E5 n12 -10, n16 -7; B5 n8 -2 dB) where its mid band is
/// cut 20-30. The string scraping off whatever touched it last, flesh or
/// nail, which does not care which it was. Its share of the nail pluck at
/// the top, and where it comes up.
const SCRAPE_SHARE: f64 = 0.5;
const SCRAPE_HZ: f64 = 7000.0;

/// How fast the finger drinks a string's motion while it touches it, nepers
/// per second for a mode with an antinode under it: what a restrike leaves
/// of the note it lands on (`GuitarVoice::strike`). A player's fingertip is a
/// heavy, lossy load; at the shipped contact (8 ms at mezzo) it keeps a
/// third of the fundamental, which is why repeated notes on one string do
/// not swell (`eight_pedalled_repeats_neither_stack_nor_leave_a_hole` holds
/// every patch to 4 dB).
const FINGER_NP: f64 = 450.0;

/// The contact phase: how long the finger holds the string before it lets
/// go, as the reference plays it, milliseconds at MIDI velocity m:
/// `37 - 0.1453 (m - 10)`, which is its measured 37 ms at MIDI 10, 26 ms at
/// 80 and 20 ms at 127 (the analysis's verifier, missed gap 1).
const CONTACT_MS_V10: f64 = 37.0;
const CONTACT_MS_PER_VEL: f64 = 0.1453;

/// Tangent plays it shorter: a keyboard player feels 26 ms as latency, so
/// PICK's centre is this fraction of the reference's law — 8.3 ms at MIDI 80,
/// 6.2 at 127, 11.4 at 10 — the top of the knob is the reference's own
/// timing, and zero lets go at the note-on (`GuitarKnobs::pick`).
const PICK_DEFAULT: f64 = 0.308;

/// How the knob reaches that: contact = law x PICK_DEFAULT x (2 knob)^1.7,
/// so the centre is the default exactly and the top (2^1.7 x 0.308 = 1.000)
/// is the reference's timing exactly.
const PICK_CURVE: f64 = 1.7;

/// The finger's contact noise: the reference's pre-release segments (2 ms
/// to its release, the `gx` score's seven MIDI-80 notes with its open strings
/// off, each on a silent background; `contact2.py`, both channels) sit -23.3
/// dB under the 14-36 ms after their release (-20.6 in 0.25-4 kHz; its first
/// note alone, the analysis's figure, -28.4), and their summed octave bands
/// are -3 0 0 -3 -8 -16 -20 -24 dB from 63 Hz up: skin on a string, most of
/// it under 500 Hz, falling 5-8 dB an octave above a kilohertz. A high-pass
/// at 100 Hz and low-passes at 120 Hz and 2.4 kHz (`noisefit.py`: -2 0 -1 -4
/// -7 -12 -19 -27, 1.7 dB rms from the reference's), at the brief's -29 dB
/// (lane render, PICK at the reference's own contact time: -27.8 broadband,
/// -26.3 in 0.25-4 kHz). The first draft's two low-passes at 1.6 kHz put
/// the noise's peak at 1-2 kHz, 10-14 dB too bright above 2 kHz (-5 0 -2 -5
/// -2 -7 -11 -19; the lane verifier, finding 12).
const NOISE_DB: f64 = -29.0;
const NOISE_HP_HZ: f64 = 100.0;
const NOISE_LP1_HZ: f64 = 120.0;
const NOISE_LP2_HZ: f64 = 2400.0;

/// **The onset, and why the plateau is taken back.** A plucked string's
/// bridge force does not step when the finger lets go: the finger had been
/// holding the string in its triangle for the whole contact, so the bridge
/// was already carrying the triangle's static pull, and that pull is where
/// the force STARTS when the string swings. The microphones never heard
/// that pull (a body does not radiate a constant force), but a mode bank
/// started at the release instant from its modes' shares of the triangle
/// says the opposite — every mode's reading starts at its share, and their
/// sum, the plateau, appears in one sample. So the voice subtracts what a
/// body's high-pass would have done to that plateau: the plateau times the
/// step response of a critically damped second-order high-pass at
/// `ONSET_HZ` (the body radiates nothing under its air mode), `(1 - w t)
/// e^(-w t)`. It starts at the plateau, so the output starts at zero; its
/// integral is zero, so it adds no thump; the string's own steps — the kinks
/// arriving at the bridge, the sound of a pluck — are untouched. Measured
/// against the reference's onsets: its largest one-sample step after a
/// release is 4 to 12 % of the next 60 ms' peak (`onset_look.py`).
const ONSET_HZ: f64 = 100.0;

/// How much of the onset correction the bridge DRIVE takes. The air mode
/// and the open strings hear the bridge, not the microphones, and the
/// bridge does feel the string start to swing.
const DRIVE_ONSET: f32 = 1.0;

/// How long the onset correction runs: 16 ms is 10 time constants at 100 Hz,
/// where `(1 - w t) e^(-w t)` is 50 dB down.
const ONSET_SECS: f64 = 0.016;

// ── the losses ───────────────────────────────────────────────────────────

/// The string material's own loss, dB per second at 1 kHz, and how it rises
/// with frequency: 9.2 (f / 1 kHz)^1.46 — the analyst's fit of the
/// reference's high bands, which its verifier reproduced band by band (1.08
/// s at 2.8-3.2 kHz, 0.68 s at 4-4.5 kHz, 3.13 s at 1.1-1.26 kHz).
const INTERNAL_DB_1K: f64 = 9.2;
const INTERNAL_EXP: f64 = 1.46;

/// The bridge's conductance as each string sees it, as a bridge loss in dB
/// per second for an OPEN string, scaled by 2^(fret/12) for a fretted one —
/// the loss of a string mode to a resistive termination is the tension over
/// the speaking length times the conductance (Gough, Acustica 49, 1981;
/// Weinreich, JASA 62, 1977), and the tension stays when the fret shortens
/// the length.
///
/// **Fitted to the reference with its open strings OFF** (`lossfit3.py`:
/// 145 partials of 14 notes from its `Sympathetic Resonance = 0` renders,
/// the brief's protocol, plus its decay score's partials above 1.5 kHz,
/// where the open strings change nothing): a curve in log-frequency on
/// Gaussian bumps a third of an octave apart from 80 Hz, `exp(sum c_j
/// phi_j(f))`, and per string a regularised deviation of it, because the
/// saddle is a rocking beam and the bass and treble ends of it see the top's
/// modes differently. With the material's own loss fixed at
/// `INTERNAL_DB_1K`, the fit lands every fundamental within 19 % of its
/// harp-off T60 (A3 the worst, 1.80 s against 1.51) and 88 % of all partials
/// within x1.5; the bump the corrected brief names is in it — 3.0 to 4.8
/// dB/s at 202-254 Hz against 1.1-1.7 at 101-160 Hz, a ratio of about three.
const G_CEN0: f64 = 80.0;
const G_STEP: f64 = 1.0 / 3.0;
const G_WIDTH: f64 = 1.0 / 3.0;
const G_SHARED: [f64; 22] = [
    1.2342, 0.0827, 0.3443, 0.5583, 1.1011, 1.5724, 0.7670, 0.52348, 1.1461, 1.0557, 0.6853, 0.8032, 0.9553,
    0.8229, 0.6430, 0.7700, 0.8198, 0.5698, 0.4122, 0.3825, 0.3805, 0.3804,
];
/// The per-string deviations, string 1 (top E) to string 6, as fitted. (The
/// first draft carried one hand edit, +0.35 on string 3 at 254 Hz, which held
/// A3's fundamental down to its measurement once the second polarisation's
/// slow twin had been added under it; the twins no longer reach 220 Hz
/// (`POLAR_LO_HZ`), and the fit as it stands puts A3 at 1.78 s and G3 at 3.56
/// against 1.51 and 4.0, where the edit now left 1.34 and 2.98.)
const G_STRING: [[f64; 22]; STRINGS] = [
    [
        -0.0006, -0.0012, -0.0031, -0.0080, -0.0193, -0.0306, 0.0137, 0.1496, 0.2537, 0.2811, 0.2855, 0.2430,
        0.1544, 0.1260, 0.2204, 0.3077, 0.2270, 0.0770, 0.0113, 0.0004, -0.0001, -0.0001,
    ],
    [
        -0.0262, -0.0524, -0.1305, -0.3177, -0.5040, -0.2061, 0.3369, 0.2667, 0.1201, 0.1341, 0.1329, 0.1467,
        0.1493, 0.0698, -0.0308, -0.0658, -0.0413, -0.0175, -0.0067, -0.0026, -0.0010, -0.0005,
    ],
    [
        -0.0412, -0.0814, -0.1765, -0.2058, 0.1726, 0.5312, 0.3006, 0.0729, 0.0726, 0.1191, 0.1347, 0.1431,
        0.2023, 0.2342, 0.1572, 0.0709, 0.0276, 0.0105, 0.0040, 0.0015, 0.0006, 0.0003,
    ],
    [
        -0.0014, -0.0056, -0.0266, -0.0480, 0.0117, 0.0962, 0.0014, -0.1923, -0.2068, -0.2015, -0.2920, -0.3553,
        -0.3551, -0.2455, -0.1206, -0.0664, -0.0409, -0.0201, -0.0082, -0.0032, -0.0013, -0.0006,
    ],
    [
        -0.2634, -0.1385, 0.2989, 0.4240, 0.1610, -0.2268, -0.3494, -0.1797, -0.0985, -0.2599, -0.2494, -0.0579,
        -0.0769, -0.3023, -0.3746, -0.2423, -0.1085, -0.0427, -0.0164, -0.0063, -0.0025, -0.0013,
    ],
    [
        0.4648, 0.1682, 0.0432, 0.1308, 0.1906, -0.0219, -0.2670, -0.1750, -0.0738, -0.0454, -0.0798, -0.1542,
        -0.0719, 0.0857, 0.0879, 0.0074, -0.0174, -0.0100, -0.0040, -0.0015, -0.0006, -0.0003,
    ],
];

/// A sum of the Gaussian basis the two fitted curves (`G_*`, `RADIATION`)
/// are written on: `sum_j c_j exp(-x_j^2 / 2)`, `x_j = log2(f / c_j) / width`,
/// only the terms within five widths of `f` (the rest are under e^-12).
fn bumps(coef: &[f64], cen0: f64, step: f64, width: f64, f: f64) -> f64 {
    let pos = (f.max(1.0) / cen0).log2() / step;
    let reach = 5.0 * width / step;
    let lo = (pos - reach).floor().max(0.0) as usize;
    let hi = ((pos + reach).ceil().max(0.0) as usize).min(coef.len().saturating_sub(1));
    let mut sum = 0.0;
    for (j, c) in coef.iter().enumerate().take(hi + 1).skip(lo) {
        let x = (pos - j as f64) * step / width;
        sum += c * (-0.5 * x * x).exp();
    }
    sum
}

/// The string material's own loss at `f`, dB per second.
fn internal_loss(f: f64) -> f64 {
    INTERNAL_DB_1K * (f / 1000.0).powf(INTERNAL_EXP)
}

/// What the bridge takes from a mode of string `s` at `f` Hz, dB per
/// second, for an open string.
fn bridge_loss(s: usize, f: f64) -> f64 {
    let shared = bumps(&G_SHARED, G_CEN0, G_STEP, G_WIDTH, f);
    let own = bumps(&G_STRING[s], G_CEN0, G_STEP, G_WIDTH, f);
    (shared + own).exp()
}

/// A mode's free decay, nepers per second: the material's loss plus the
/// bridge's, the bridge's scaled by how much shorter than open the string is.
fn mode_sigma(place: Place, f: f64) -> f64 {
    let db = internal_loss(f) + 2.0_f64.powf(f64::from(place.fret) / 12.0) * bridge_loss(place.string, f);
    db * LN_10 / 20.0
}

// ── the stiffness ────────────────────────────────────────────────────────

/// Inharmonicity B where the reference was measured, per string as (fret,
/// B): its 19 decay-score notes, NLS-fitted on 9 to 30 partials each with a
/// median residual under a cent (the analysis's verifier, `inharm_t.py`).
/// Between and beyond those points the stopped-string law carries it:
/// `B = pi^2 E I / (T L^2)` at a fixed tension grows as 1/L^2, that is as
/// 4^(fret/12) — so what is interpolated is the open-equivalent `B /
/// 4^(fret/12)`, log-linearly in frets, and held past the last measured fret.
/// The measured top string does not follow the law (B falls from the 7th fret
/// to the 15th and rises again to the 19th); the table keeps what was
/// measured.
const B_MEASURED: [&[(u8, f64)]; STRINGS] = [
    &[
        (0, 3.13e-5),
        (3, 5.26e-5),
        (5, 7.6e-5),
        (7, 1.02e-4),
        (10, 6.73e-5),
        (12, 6.0e-5),
        (15, 5.46e-5),
        (17, 9.6e-5),
        (19, 2.1e-4),
    ],
    &[(0, 1.66e-5), (3, 2.42e-5)],
    &[(0, 5.98e-5), (2, 8.48e-5)],
    &[(0, 2.09e-5), (2, 3.11e-5)],
    &[(0, 7.23e-6), (2, 1.08e-5)],
    &[(0, 2.04e-5), (3, 4.36e-5)],
];

/// The inharmonicity of a note at `place`.
pub fn inharmonicity(place: Place) -> f64 {
    let pts = B_MEASURED[place.string];
    let law = |fret: f64| 4.0_f64.powf(fret / 12.0);
    let k = |i: usize| pts[i].1 / law(f64::from(pts[i].0));
    let fret = f64::from(place.fret);
    let open_equiv = if fret <= f64::from(pts[0].0) {
        k(0)
    } else if fret >= f64::from(pts[pts.len() - 1].0) {
        k(pts.len() - 1)
    } else {
        let mut v = k(0);
        for i in 0..pts.len() - 1 {
            let (f0, f1) = (f64::from(pts[i].0), f64::from(pts[i + 1].0));
            if fret >= f0 && fret <= f1 {
                let t = (fret - f0) / (f1 - f0);
                v = (k(i).ln() * (1.0 - t) + k(i + 1).ln() * t).exp();
                break;
            }
        }
        v
    };
    open_equiv * law(fret)
}

// ── the radiation ────────────────────────────────────────────────────────

/// What the microphones hear of a partial per newton of bridge force at a
/// MIDI-80 pluck, dB, on the same Gaussian basis from 70 Hz.
///
/// **Recovered from the reference's own attack spectra** (`radfit.py`: 465
/// partials of its 19 decay-score notes, 35-235 ms, the two channels' power
/// average, each corrected for its own decay to the window's centre, minus
/// the plucked string's force comb `|sin(n pi beta)| / n` at the fixed-distance
/// beta; median residual 3.8 dB). It rises 17 dB from 82 to 220 Hz — the
/// body radiates little under its air mode, which is why the bass E's
/// fundamental sits 19 dB under its own note — and falls about 4.7 dB per
/// octave from there to 3 kHz, which with the comb's 6 is the 12.5 dB per
/// octave above 1 kHz the analyst measured, pluck included.
const RAD_CEN0: f64 = 70.0;
const RADIATION: [f64; 25] = [
    -4.515, -1.122, 1.947, 4.187, 5.583, 6.064, 5.423, 4.304, 3.518, 3.371, 3.412, 3.027, 2.004, 1.145, 0.468, -0.432,
    -1.383, -2.306, -2.281, -0.913, -0.042, -1.239, -2.887, -4.476, -6.034,
];

/// The dip between the air mode and the top's first resonance, where the
/// two radiate against each other: the reference's D3 and E3 fundamentals
/// (146.8 and 164.8 Hz) are 10.1 and 7.4 dB under their notes where B2 and
/// G3 either side sit at -6 and -1 (the analysis's matched-window n1 shares),
/// a narrower trough than the third-of-an-octave curve above can draw.
/// Centre, depth in dB, width in octaves; with it D3 and E3 measure -9.3 and
/// -7.6 where they were -8.7 and -3.5.
const DIP_HZ: f64 = 152.0;
const DIP_DB: f64 = -8.0;
const DIP_OCT: f64 = 0.16;

/// And the air mode's own radiation: partials near the Helmholtz frequency
/// are carried out of the soundhole as well as off the top, and under it the
/// two cancel and the radiation falls away faster than the fitted curve's
/// third-octave resolution can draw. A bump at 104 Hz and a trough at 80,
/// set on the measured n1 shares of G2, A2, B2 and E2 (-10.6, -5.8, -6.1,
/// -18.9 dB; lane renders g19).
const HELM_HZ: f64 = 104.0;
const HELM_DB: f64 = 0.0;
const HELM_OCT: f64 = 0.20;
const SUB_HZ: f64 = 80.0;
const SUB_DB: f64 = -3.5;
const SUB_OCT: f64 = 0.12;

/// The radiation at `f`, linear, before the overall gain.
fn radiation(f: f64) -> f64 {
    let db = bumps(&RADIATION, RAD_CEN0, 1.0 / 3.0, 1.0 / 3.0, f);
    let x = (f / DIP_HZ).log2() / DIP_OCT;
    let h = (f / HELM_HZ).log2() / HELM_OCT;
    let u = (f / SUB_HZ).log2() / SUB_OCT;
    let extra = DIP_DB * (-0.5 * x * x).exp() + HELM_DB * (-0.5 * h * h).exp() + SUB_DB * (-0.5 * u * u).exp();
    10.0_f64.powf((db + extra) / 20.0)
}

/// Output units per newton of bridge force at 0 dB of `RADIATION`.
///
/// **Set by the loudest thing a keyboard can ask of a guitar:** all six
/// strings struck at MIDI 127 in one instant, one note on every string.
/// Seven such shapes (`strum2.events`: E major, the open strings, G major,
/// A major, and E major with the top string at its 12th and 19th frets)
/// peak at -3.6 dBFS at worst with the voicing below, and chords of 10 to 44
/// keys at once no higher (only the last note on each string plucks:
/// `GuitarVoice::choke`): the brief's -3 dBFS ceiling with half a decibel to
/// spare. A strum spread over 30-45 ms, as hands play one, peaks 3-6 dB
/// lower. A single note at MIDI 127 then peaks under -13 dBFS and a mezzo one
/// sits near -31 dBFS RMS over its first 300 ms, 7-8 dB under the
/// reference's renders. (0.0235 before the finger's brightness was carried
/// on past MIDI 98: G major then peaked -2.9.)
const GAIN: f64 = 0.0215;

/// **The register's balance, as the reference is voiced.** Plucked with
/// one force everywhere, the strings come out of the physics uneven against
/// the reference: its 300 ms RMS at MIDI 80 is level from E2 to B5 within
/// 6 dB, where the bare model's bass E sits 3 dB and its top string's 19th
/// fret 4 dB under their neighbours, and its G#2, on the air mode, 4 dB over.
///
/// **A curve in pitch, fitted to every key.** The first draft voiced a level
/// per string and a rise up the neck, least-squares over the 19 notes the
/// reference had been measured on: it was a step at every string crossing,
/// and the crossings had not been measured — G#2 4.3 dB over A2 where the
/// reference's two are level, B3 and D3 2.3 dB under the notes below them
/// (the lane verifier, finding 4). Here the reference's own chromatic scan
/// (`chromq`, every key E2-B5 at MIDI 80, one offline render) against the
/// bare model's (`voicefit.py`): straight lines in pitch between knots at
/// the open strings, the last fret before each, A4's 7th fret and the top,
/// a light curvature penalty, zero on average over the 44 keys. What is left
/// is 0.88 dB RMS (the reference's own note-to-note scatter, A#5 2.4 dB
/// under its neighbours, the worst), and each crossing steps as the
/// reference's does to 0.7 dB (G#2-A2 -0.6 against -0.1, C#3-D3 -1.5 / -1.5,
/// F#3-G3 +0.2 / +0.4, A#3-B3 -0.6 / -1.2, D#4-E4 +1.5 / +1.6). Applied to
/// what the microphones and the bridge hear of the voice, not to the
/// string's motion, so its sum tones and its open strings keep their share
/// of it; by pitch, so a note the selector moves keeps its key's level.
const VOICING: [(u8, f64); 13] = [
    (40, 2.27),
    (44, -0.32),
    (45, -1.84),
    (49, -3.40),
    (50, -4.16),
    (54, -2.48),
    (55, 0.01),
    (58, -0.20),
    (59, -2.83),
    (63, -3.58),
    (64, -1.44),
    (71, 2.00),
    (83, 4.31),
];

/// The voicing in dB at `pitch`: `VOICING`'s straight lines, held past its
/// ends.
fn voicing_db(pitch: u8) -> f64 {
    let x = f64::from(pitch);
    let (first, last) = (VOICING[0], VOICING[VOICING.len() - 1]);
    if x <= f64::from(first.0) {
        return first.1;
    }
    if x >= f64::from(last.0) {
        return last.1;
    }
    for w in VOICING.windows(2) {
        let ((p0, d0), (p1, d1)) = (w[0], w[1]);
        if x <= f64::from(p1) {
            return d0 + (d1 - d0) * (x - f64::from(p0)) / f64::from(p1 - p0);
        }
    }
    last.1
}

/// The voicing gain at `pitch`, a factor.
fn level_trim(pitch: u8) -> f64 {
    10.0_f64.powf(voicing_db(pitch) / 20.0)
}

/// The two close microphones: the phase difference with which they hear a
/// partial at `f`, as `pi/2 (1 - exp(-(f / f_c)^q))` — in step at the bottom,
/// where both hear the whole top move as one, and a quarter turn apart at
/// the top, where each hears its own patch of a many-lobed mode. Fitted to
/// the reference's octave-band correlations (0.92 / 0.68 / 0.56 / 0.21 /
/// 0.40 / 0.02 / 0.03 from 63 Hz up, `stereo_v.py`). Never past a quarter
/// turn: a partial's mono sum then keeps at least half its power, so no note
/// loses more than 3 dB folded to mono. The crossover sits where both of
/// the middle bands land inside the brief's 0.55-0.7 with the second
/// polarisation in (gdecay, lane renders): 205 Hz read 0.68 / 0.54, 212 Hz
/// 0.689 / 0.548, 218 Hz 0.696 / 0.555, 220 Hz 0.698 / 0.558; mono loss at
/// most 2.1 dB at each.
const MIC_HZ: f64 = 218.0;
const MIC_ORDER: f64 = 0.6;
/// The level difference between the two, at most, in radians of the
/// equal-power angle: 0.12 is about +-2 dB.
const MIC_LEVEL: f64 = 0.12;

/// **Which microphone leads is the body's, at each frequency.** The phase
/// difference's sign and the level difference wander with frequency as the
/// top's mode patterns do — `tanh(3 sin(2 pi f / MIC_SIGN_HZ))`, nearly a
/// square wave that changes sign every 48 Hz, and a sine every 131 Hz for
/// the level — never with which string happens to drive that frequency. The
/// first draft drew them per string and partial; a partial, its twin and the
/// open string answering it a hertz away then took opposite signs, and their
/// sum was heard alike by both microphones (500-1000 Hz correlation 0.49
/// where the law alone gave 0.3).
const MIC_SIGN_HZ: f64 = 97.0;
const MIC_LEVEL_HZ: f64 = 131.0;

/// A deterministic number in [-1, 1] per string, fret, partial and salt: the
/// microphones' view of a partial is the same every time that partial of
/// that string at that fret is heard.
fn hash_unit(s: usize, fret: u8, n: u32, salt: u32) -> f64 {
    let mut z = ((s as u64) << 56) ^ (u64::from(fret) << 40) ^ (u64::from(n) << 8) ^ u64::from(salt);
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 52) as f64 - 1.0
}

/// The two microphones' complex weights for a mode heard at `amp` (output
/// units per unit of the mode's coordinate) at `f` Hz.
fn mics(amp: f64, f: f64, width: f64) -> (C64, C64) {
    let spread = FRAC_PI_2 * (1.0 - (-(f / MIC_HZ).powf(MIC_ORDER)).exp());
    let sign = (3.0 * (TAU * f / MIC_SIGN_HZ + 0.7).sin()).tanh();
    let psi = (spread * width).min(FRAC_PI_2) * sign;
    let lean = (TAU * f / MIC_LEVEL_HZ + 1.9).sin();
    let alpha = (FRAC_PI_4 + MIC_LEVEL * width.min(1.5) * lean).clamp(0.1, FRAC_PI_2 - 0.1);
    (C64::polar(amp * SQRT_2 * alpha.cos(), 0.5 * psi), C64::polar(amp * SQRT_2 * alpha.sin(), -0.5 * psi))
}

// ── the release ──────────────────────────────────────────────────────────

/// The damper: the hand that stops a string, lying along it — a lifted
/// left-hand finger relaxing onto it, the right hand's palm on an open one.
///
/// **What the reference does** (`damp_v.py`, its open strings off, extra
/// decay 15-75 ms after key-off): -179 to -481 dB/s on the fundamentals,
/// median -343, and a median over its twelve notes rising with the partial
/// NUMBER — -416, -485, -518, -553 on n2 to n5, -650 to -700 by n8 — with no
/// trend in pitch and no comb: a slow partial here and there (E3 n3 -86, B2 n4
/// -94, E2 n2 and n6 -110), all of them at about 494 Hz, a body resonance's,
/// not a node's. A law in absolute frequency (the first draft's first try,
/// 420 dB/s at 200 Hz rising as f^0.25) damped E4 and A4's strings alone 6 and
/// 11 dB too far by 100 ms (-32 and -33 against -26 and -22, `postoff.py`).
///
/// **A pad, not a point.** The first draft pressed a point at exactly a
/// third, so every third partial — a node there — kept only `DAMP_FLOOR` of
/// the rate: n3, n6, n9 at -112..-153 dB/s beside neighbours at -410..-700,
/// and 60-160 ms after a key-off the twelfth stood 34 to 44 dB over its
/// neighbours, a natural harmonic on every release (on E3 louder than the
/// fundamental; the lane verifier, finding 6). A hand touches a stretch of
/// string, and a mode feels the pad's average of `sin^2(n pi x)` over it:
/// here from a sixth of the string to its middle, centred on the third. With
/// the floor the weights of n1..n8 lie within a factor of 1.4 of each other,
/// and the rate rises as `n^0.35` (`dampfit.py`: the reference's medians to
/// 0.17 in log rate, the point at a third 0.79). Here the fundamental's rate,
/// the slope in n, the pad's centre and width, and the floor.
const DAMP_DB_N1: f64 = 350.0;
const DAMP_SLOPE: f64 = 0.35;
const DAMP_AT: f64 = 1.0 / 3.0;
const DAMP_WIDTH: f64 = 1.0 / 3.0;
const DAMP_FLOOR: f64 = 0.25;

/// The pad's hold on mode n: `sin^2(n pi x)` averaged over `DAMP_WIDTH`
/// about `DAMP_AT`.
fn pad(n: usize) -> f64 {
    let a = PI * n as f64 * DAMP_WIDTH;
    0.5 * (1.0 - (2.0 * PI * n as f64 * DAMP_AT).cos() * a.sin() / a)
}

/// How long the finger takes to come down, seconds at RELEASE centre, and
/// how fast a choke (another note taking the string) is.
const FALL_SECS: f32 = 0.010;
const CHOKE_SECS: f32 = 0.004;
/// A choke damps this many times harder than a release: the next note's
/// finger is already pressing the string. The modes carry the choke's rate
/// (`Bank::set_damper` cannot go past 1), and a release presses the damper
/// to `1 / CHOKE_RATE` of it.
const CHOKE_RATE: f64 = 2.0;

/// A mode's damping under a fully pressed damper, nepers per second: the
/// fundamental's `DAMP_DB_N1`, the pad's hold on mode n relative to its hold
/// on the fundamental, and the rise in n.
fn damper_rate(n: usize) -> f64 {
    let hold = (pad(n) + DAMP_FLOOR) / (pad(1) + DAMP_FLOOR);
    DAMP_DB_N1 * (n as f64).powf(DAMP_SLOPE) * hold * LN_10 / 20.0
}

/// Where a release presses the damper to, of the modes' (choke) rate.
const RELEASE_PRESS: f32 = (1.0 / CHOKE_RATE) as f32;

// ── phantoms ──────────────────────────────────────────────────────────

/// The string's tension modulation into the bridge: every low partial's
/// SQUARE, a line at exactly `2 f_k` — sum tones only, the reference's
/// quadratic-effect energy below 1.5 f0 being -75 dB on E2 v127 (the critic,
/// contradiction 0).
///
/// **Squares, not every pair.** The tension a string's stretch adds is
/// `(EA / 2L) int (dy/dx)^2`, whose products of two DIFFERENT partials fall
/// between `2 f_k` and the stretched `f_2k` — and the reference's gap there
/// is clean: its phantom at `2 f_k` stands 47-57 dB over the mid-gap floor
/// (E2, `phantom_v2.py`). The first draft rang every pair `j + l = m` as one
/// mode at their weighted frequency, which put it IN the gap: 23-48 dB over
/// a floor it was itself making. So each partial k gets one phantom at
/// `2 f_k`, seeded with its own coordinate squared (phase-locked to it),
/// decaying at twice its rate, `q k^2 z_k^2 / 2` — plus the two nearest
/// pairs either side, `z_(k-d) z_(k+d)` for d = 1..3, which the string's
/// longitudinal modes `p = 2d` carry (`grand.rs`) and which land within 0.1-1
/// Hz of `2 f_k` on these strings: squares alone left E2's k15 13 dB over the
/// gap where the reference's is 53, because partial 15 sits in the pluck's
/// third comb notch and its square sits in it twice.
///
/// **Heard a quarter turn round.** The phantom is a force ALONG the string;
/// the bridge passes it to the top by rocking, a different path from the
/// transverse pull, so a phantom does not add to the partial it sits on with
/// a fixed sign. Heard in the transverse phase it did, and the comb's sign
/// decided which way: A2's sixth partial cancelled its own k3 phantom at MIDI
/// 127 (-39 dB re n1 against -26 at MIDI 10; the reference's -12.7). A
/// quarter turn makes every unresolvable one (low k, where `2 f_k` and
/// `f_2k` are a fraction of a hertz apart) add in power.
const PHANTOM_PARTIALS: usize = 20;
const PHANTOM_REACH: usize = 3;

/// The first longitudinal mode of an open wound string, Hz: its core
/// carries the stretch at `sqrt(E A_core / mu)`, the winding about three
/// times the core's mass, some 1.5 km/s over twice 650 mm. Fretting raises
/// it as 2^(fret/12). And its Q. Set on E2 at MIDI 127 against the
/// reference's sum tones over the mid-gap floor (`phsweep.py`, open strings
/// off, k11-k15: +39 / +54 / +66 / +47 / +56 dB against its +48 / +51 / +56 /
/// +55 / +53, three of five in the brief's +47..+56): the first draft's 1050
/// Hz put the pairs' resonance on k12-k13 (+41 / +57 / +69 / +42 / +49 once
/// the glide was in), 1250 left k14 at +45.
const LONGITUDINAL_HZ: f64 = 1150.0;
const LONGITUDINAL_Q: f64 = 8.0;

fn longitudinal_mode_gain(f: f64, f_p: f64) -> f64 {
    let x = f / f_p;
    C64::new(1.0 - x * x, x / LONGITUDINAL_Q).recip().abs()
}

/// The coupling: the longitudinal force per metre squared of transverse
/// motion, `EA pi^2 / (8 L^2)` with a nylon string's EA of about 2 kN, times
/// this calibration, times velocity squared — the reference's phantom-minus-
/// partial ratio on E2's clean k11 and k12 rises 9 and 11 dB from MIDI 80
/// to 127, where a constant coupling (amplitude squared alone) gives 4.3 and
/// one in velocity 8.6. Set on E2 v127 against the reference's k11-k15 over
/// the mid-gap floor (`phantom_v2.py`: +47 +51 +56 +56 +53 dB) with A2's
/// MIDI-127 attack kept where the reference has it (n2 +5.9, n4 -5.5 dB re
/// n1): the low partials' squares are the loudest phantoms there are, so the
/// base coupling is small and the longitudinal resonance does the lifting
/// near 2 kHz.
const PHANTOM: f64 = 20.0;
const EA_NYLON: f64 = 2000.0;

/// Only the wound strings' first position has phantoms: the register the
/// reference shows them in, and where its gap between `2 f_k` and `f_2k` is
/// wide enough to resolve them. Higher, they did harm here: on B5 the m = 5
/// cluster outweighed a partial sitting in the pluck's comb notch and pulled
/// the measured stiffness to a third, and on A3 they grew the attack's upper
/// half 16.5 dB from MIDI 10 to 127 against the reference's 8.1.
const PHANTOM_UNTIL: u8 = 54;

/// **The pitch glide: a string plucked hard is stretched, and sharp.** Its
/// tension grows with the slope it is bent to, `dT = (EA / 8) sum (n pi /
/// L)^2 |z_n|^2` over its partials (time-averaged), and every partial rises
/// by `dT / 2T` of itself, 866 cents per unit, falling back as the string
/// rings down. The reference's bass E at MIDI 127 with its open strings off
/// is +5.9 c at 0.12 s, +3.7 at 0.2, +2.4 at 0.3, +1.3 at 0.5, +0.7 at 0.8,
/// and at MIDI 80 +1.1 c at 0.12 s (its analysis's verifier: coherent
/// Hilbert phase over n2-n12, `glide_v2.py`). That law as written, with a
/// nylon string's EA, gives +0.35 c at 0.12 s (lane render, `glidesweep.py`)
/// and falls with the low partials' slow decay (13 times it: +6.1, +3.9 at
/// 0.3 s, against the reference's 2.4). The reference's glide falls at 43
/// dB/s over 0.12-0.3 s, the rate of the bass E's upper partials, so the sum
/// is weighted `n^GLIDE_EXP` over the first `GLIDE_PARTIALS`: with the
/// fourth power every partial of the plucked triangle counts alike and the
/// attack carries the glide (squares 0.12 / 0.3 s ratio 0.64, the reference
/// 0.41, the fourth power 0.41). That sum already rises with the finger's
/// brightness, which grows with velocity on its own (`BLEND`): divided by
/// velocity (`GLIDE_VEL`), the MIDI-80 glide is the reference's, where
/// without it it was half. `GLIDE` is the calibration on the bass E at MIDI
/// 127: +5.98 / +4.03 / +2.45 / +1.02 / +0.27 c at 0.12 / 0.2 / 0.3 / 0.5 /
/// 0.8 s, and at MIDI 80 +1.07 / +0.79 / +0.46 / +0.15 (the brief's +6 +-1.5,
/// under 2.5 by 0.3 s, about +1 at MIDI 80). Each block the voice measures
/// the sum and, once the pitch it calls for has moved `GLIDE_STEP_CENTS` from
/// the one the bank plays, rebuilds every mode but the sum tones at the new
/// ratio from where it is (`rewrite`: its coordinate carried over exactly, no
/// step), the microphones' weights held at the untuned frequency. The first
/// draft had no glide (-0.15 c).
const GLIDE: f64 = 0.38;
const GLIDE_EXP: f64 = 4.0;
const GLIDE_VEL: f64 = -1.0;
const GLIDE_PARTIALS: usize = 32;
const GLIDE_STEP_CENTS: f64 = 0.1;

// ── the knobs ────────────────────────────────────────────────────────────

/// `2^((v - 0.5) 2 octaves)`: the frame's own centre-is-exact mapping
/// (`mod.rs` keeps its copy private).
fn bipolar(v: f32, octaves: f32) -> f32 {
    2.0_f32.powf((v.clamp(0.0, 1.0) - 0.5) * 2.0 * octaves)
}

/// The width at the top of knob 1, the frame's own `WIDTH_FULL`.
const WIDTH_FULL: f32 = 1.5;

/// Every knob at the instrument the bench signed off: the frame's
/// `GUITAR_PARAMS` reads its defaults from here. WIDTH is 1/1.5 of its knob
/// (the stock pair) as on every patch; the rest are centred.
pub const DEFAULTS: [f32; MAX_PARAMS] = [0.5, 1.0 / WIDTH_FULL, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];

/// The guitar's eight knobs in the units the voice and the fretboard use.
/// Each one's centre is the tuned constant exactly
/// (`every_knob_at_its_default_is_the_tuned_instrument`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuitarKnobs {
    /// 0 all flesh, 1 all nail; 0.5 is the reference's velocity blend.
    pub voicing: f32,
    /// How far apart the two microphones hear it: 1.0 the stock pair, 0 one
    /// microphone, 1.5 the top of the knob.
    pub width: f32,
    /// Multiplier on every string's T60, 0.38 to 2.6.
    pub decay: f32,
    /// Multiplier on the plucking distance from the bridge, 0.5 (near the
    /// bridge, the reference's Strike Point 1/7.6) to 2 (over the soundhole).
    pub position: f32,
    /// How much the hollow body's air speaks, 0 (a solid body) to 2.
    pub body: f32,
    /// The finger's contact time as a fraction of the reference's law.
    pub pick: f32,
    /// Multiplier on the damper's time and its rate's inverse: bigger is
    /// slower.
    pub release: f32,
    /// How hard the open strings answer, 0 (none) to 2.
    pub strings: f32,
}

impl GuitarKnobs {
    /// The knobs from the frame's eight values. A value that is not a number
    /// is the knob at its default: `clamp` passes a NaN through, and one NaN
    /// WIDTH or DECAY reaching the modes would put a NaN in every sample they
    /// add (the modal lane's final bank refuses such a mode; the grand reads
    /// its knobs the same way, lane/modal cc701c73).
    pub fn from_params(p: &[f32; MAX_PARAMS]) -> Self {
        let p: [f32; MAX_PARAMS] = std::array::from_fn(|i| if p[i].is_finite() { p[i] } else { DEFAULTS[i] });
        GuitarKnobs {
            voicing: p[0].clamp(0.0, 1.0),
            width: (p[1] * WIDTH_FULL).clamp(0.0, WIDTH_FULL),
            decay: bipolar(p[2], 1.4),
            position: bipolar(p[3], 1.0),
            body: (p[4] * 2.0).clamp(0.0, 2.0),
            pick: (PICK_DEFAULT * f64::from((p[5] * 2.0).clamp(0.0, 2.0)).powf(PICK_CURVE)) as f32,
            release: bipolar(p[6], 1.7),
            strings: (p[7] * 2.0).clamp(0.0, 2.0),
        }
    }

    /// The nail's weight in the pluck at MIDI velocity `midi`: the
    /// reference's blend, scaled by VOICING (0.5 is the blend itself, 0 all
    /// flesh, 1 the blend's sharpest, its MIDI-127 nail, at every velocity).
    fn nail(&self, midi: f64) -> f64 {
        let b = blend(midi);
        let s = f64::from(self.voicing) * 2.0;
        if s <= 1.0 {
            b * s
        } else {
            let top = BLEND[BLEND.len() - 1].1;
            b + (s - 1.0) * (top - b).max(0.0)
        }
    }
}

/// The reference's plucking-brightness blend at MIDI velocity `midi`:
/// straight lines between the knots, carried on past both ends (never under
/// zero, all flesh).
fn blend(midi: f64) -> f64 {
    let k = BLEND.len();
    let seg = if midi <= BLEND[1].0 {
        0
    } else if midi >= BLEND[k - 2].0 {
        k - 2
    } else {
        (1..k - 2).find(|&i| midi <= BLEND[i + 1].0).unwrap_or(k - 2)
    };
    let ((v0, b0), (v1, b1)) = (BLEND[seg], BLEND[seg + 1]);
    (b0 + (b1 - b0) * (midi - v0) / (v1 - v0)).max(0.0)
}

/// The flesh's release spectrum at `f` for a note of fundamental `f0` on a
/// wound or plain string, linear (1 under the knee, the floor far above it).
fn flesh(f: f64, f0: f64, wound: bool) -> f64 {
    let eps = 10.0_f64.powf(FLESH_FLOOR_DB / 20.0);
    let (knee, order) = if wound {
        (FLESH_WOUND_HZ, FLESH_WOUND_ORDER)
    } else {
        (FLESH_PLAIN_F0 * f0, FLESH_PLAIN_ORDER)
    };
    // (f/knee)^order overflows nowhere f64 can reach here: order 40 at 20 kHz
    // over a 98 Hz knee is 10^92.
    eps + (1.0 - eps) / (1.0 + (f / knee).powf(order)).sqrt()
}

/// The finger's release spectrum with nail weight `w`.
fn release_spectrum(f: f64, f0: f64, wound: bool, w: f64) -> f64 {
    let nail = 1.0 / (1.0 + (f / NAIL_HZ).powi(2)).sqrt();
    let x = (f / SCRAPE_HZ).powi(4);
    nail * (w + (1.0 - w) * flesh(f, f0, wound)) + SCRAPE_SHARE * x / (1.0 + x)
}

/// What the finger does to partial `f` against a MIDI-80 pluck with the
/// default finger: the factor on `RADIATION`'s spectrum.
fn excitation(f: f64, f0: f64, wound: bool, w: f64) -> f64 {
    let w80 = blend(REFERENCE_VELOCITY);
    release_spectrum(f, f0, wound, w) / release_spectrum(f, f0, wound, w80)
}

/// The reference's contact time at MIDI velocity `midi`, seconds.
fn contact_law(midi: f64) -> f64 {
    (CONTACT_MS_V10 - CONTACT_MS_PER_VEL * (midi - 10.0)).clamp(15.0, 40.0) * 1.0e-3
}

// ── the second polarisation ──────────────────────────────────────────────

/// **The string also swings along the top, not only across it.** A finger
/// never pulls exactly perpendicular to the soundboard, so a partial has a
/// twin in the other polarisation; the bridge is much stiffer along the top
/// than across it, so the twin barely moves it — it radiates less, loses far
/// less to the bridge, and sits a fraction of a hertz away, pulled by a
/// different reactance. Where a partial is drained fast by the bridge, the
/// twin outlasts it and the partial decays in two stages. The reference does
/// this with its open strings off on 16-21 % of its partials (its verifier's
/// early/late slope test), and its engine carries two polarisation data sets.
///
/// **Where the reference's twins are.** With its open strings off its
/// two-stage partials lie at 290-990 Hz, two in three on the wound strings
/// (A2 n3 n4 n5 n7, B2 n3 n4 n5 n7, D3 n2 n3, E3 n2, E2 n6 n8, G2 n6; on the
/// plain ones A3 n2, E4 n1, G3 n1, B3 n4, D4 n1; `two.py` on its `gx` and
/// `vlow` renders), with second components 5-25 dB under the first and
/// decaying at little more than the string's own loss (`twoexp.py`: T60s
/// of 8-20 s behind 1-5 s); none on the top string up the neck, none in the
/// bass, none over a kilohertz. The first draft gave every partial of every
/// note a twin at a quarter of the bridge loss: its two-stage partials sat in
/// the bump at 196-247 Hz and up the top string (A4, E5, B5), and those twins
/// held the treble's tails up with the open strings off — A4 -37 dB at 2.5 s
/// against the reference's -62, E5 -53 against -99.5, B5 -64 against -136
/// (the lane verifier, finding 5). So the twin is driven only between
/// `POLAR_LO_HZ` and `POLAR_HI_HZ` (smooth edges `POLAR_EDGE_OCT` wide), in
/// first position (to the 4th fret, gone by the 5th), at `POLAR_AD` on the A
/// and D strings, `POLAR_E6` on the low E and `POLAR_PLAIN` on the plain
/// three; at half the main's amplitude, heard at 0.8 of it (-8 dB on a wound
/// string), losing a twentieth of the bridge loss. Set on the four shares
/// together with the open strings' coupling (`sweep.py`, lane renders, see
/// `COUPLE` and `OPEN_DECAY`): `gx` 40 % and 15 % without the open strings,
/// `vlow` 32 % and 20 % (the reference's 36/16 and 39/21), with A4's tail at
/// 2.5 s -27.6 dB with them and -48.0 without, E5 -33.8 and gone. A plain
/// share of 0.15 left `vlow` at 27 %, 0.25 took `vlow` off to 21 %; the low E
/// at full strength put five two-stage partials on G2 alone (`vlow` off 25
/// %). The A and D strings' twins are as strong as the main's half: their
/// partials and twins beat rather than knee within the test's 2.9 s (B2 n3
/// to n7), where the reference's knee 12-25 dB down; weaker (0.3-0.6) took
/// `gx`'s share without the open strings to 10-11 % and gave B2 no knee
/// either (its twins then cross the main after 2.7-3.8 s; `sweep.py`
/// FLAGS=1, the lane fix pass).
const POLAR_PARTIALS: usize = 24;
const POLAR_EXCITE: f64 = 0.5;
const POLAR_RADIATE: f64 = 0.8;
const POLAR_LOSS: f64 = 0.05;
const POLAR_HZ_MIN: f64 = 0.03;
const POLAR_HZ_MAX: f64 = 0.2;
const POLAR_LO_HZ: f64 = 290.0;
const POLAR_HI_HZ: f64 = 900.0;
const POLAR_EDGE_OCT: f64 = 0.4;
const POLAR_FRETS: f64 = 4.0;
const POLAR_FRET_FADE: f64 = 1.0;
const POLAR_PLAIN: f64 = 0.18;
const POLAR_E6: f64 = 0.35;
const POLAR_AD: f64 = 1.0;

/// How much of `POLAR_EXCITE` partial `f` of a note at `place` gets.
fn twin_share(f: f64, place: Place) -> f64 {
    let fret = place.fret;
    let string = match place.string {
        5 => POLAR_E6,
        s if WOUND[s] => POLAR_AD,
        _ => POLAR_PLAIN,
    };
    let edge = |x: f64| -> f64 {
        let t = x.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let lo = edge(1.0 + (f / POLAR_LO_HZ).log2() / POLAR_EDGE_OCT);
    let hi = edge(1.0 - (f / POLAR_HI_HZ).log2() / POLAR_EDGE_OCT);
    let fr = edge(1.0 - (f64::from(fret) - POLAR_FRETS) / POLAR_FRET_FADE);
    lo * hi * fr * string
}

// ── the voice ────────────────────────────────────────────────────────────

/// The most partials a string is given. The bass E at 20 kHz is 242.
pub const MAX_PARTIALS: usize = 256;

/// The pending pluck: every partial and the twins of the low ones.
const MAX_PENDING: usize = MAX_PARTIALS + POLAR_PARTIALS;

/// No partial above this, Hz (and never above 0.45 fs).
const TOP_HZ: f64 = 20_000.0;

/// **No cull during the attack.** Every partial is added at the pluck,
/// however quiet, and the bank is not culled for this long after it: a
/// treble note's partials above 10 kHz start under the cull floor, and
/// culling them at once left holes in the attack spectrum where the
/// reference has a smooth floor (B4 n24 and n26 at -127 and -161 dB against
/// its -109 and -94), which the comb estimator took for notches (B4's beta
/// read 0.116 against 0.295). By 150 ms a 12 kHz partial has decayed 45 dB on
/// its own, and the cull takes it then.
const CULL_HOLD_SECS: f64 = 0.150;

/// **The cull floor, as far under the guitar as the grand's is under the
/// grand.** `CULL_FLOOR` (-120 dBFS) is where the grand lets a mode go; the
/// guitar plays 11.3 dB quieter at mezzo (gdecay MIDI 80, median rms300
/// -32.5 dBFS against Piano 1's -21.1, lane renders), because its gain is set
/// by a six-string strum. At the grand's floor every culled mode's last step
/// sat 35 dB under E2's 26th partial at MIDI 127, in the gap where the
/// reference's sum tone is measured (its gap is 47 dB down): the floor that
/// metric read was the cull's, not the instrument's.
const CULL: f32 = CULL_FLOOR * 0.25;

const KIND_STRING: u32 = 0;
const KIND_POLAR: u32 = 1;
const KIND_PHANTOM: u32 = 2;
const KIND_REMNANT: u32 = 3;
const KIND_REMNANT_TWIN: u32 = 4;

/// **What a release leaves ringing.** The reference's sound after a key-off
/// is its sympathetic side's (off, the string alone is gone by 300 ms), and
/// on a fretted note it is mostly the note's OWN partials, a little sharp:
/// C4's tail 0.3-0.95 s after the key-off is 262.8 Hz, 522 and 526 Hz, 789
/// Hz (C4 is 261.6), the same 30 dB under the note whether it was held 0.12
/// s or 2 s (`rel_v.py`: -40/-41 dB re pre-off at 0.3-0.6 s, -52 at
/// 0.6-0.95, -70/-71 at 1.0-1.5), decaying at about 33 dB/s. The key's own
/// string, freed of the finger and not yet still. So a release hands this
/// share of each low partial to an undamped twin this many cents sharp,
/// decaying at this rate — a split, not an addition, so the sum does not
/// step — scaled by STRINGS, which is what owns it in the reference.
///
/// **The treble's tail decays slower.** C5's (the same score, held 1 s) is
/// -43, -49 and -63 dB at 0.3-0.6, 0.6-0.95 and 1.0-1.5 s, falling about
/// 22 dB/s where C4's falls 33: the share and the rate run between the two
/// measured keys and hold beyond them (`by_key`).
///
/// **The whole string, both its polarisations.** The split takes the
/// partial's twin too: a held note's sound at its key-off is partly its
/// twins', and splitting only the partials made the tail's share of the
/// note fall the longer it was held (C4 held 2 s 5.4-5.8 dB under the
/// reference at 0.1-0.6 s). And it is measured per key, as the rate is: with
/// the twins gone from the top string up the neck (`POLAR_FRETS`), C5's
/// sound at 1 s is its partials alone and a twentieth of them read its
/// 0.1-0.3 s window 7 dB over the reference's. Set on the release score
/// (`relsweep.py`, both channels' power as WAVE2 asks, lane minus the
/// reference): C4 0.042 at 33 dB/s, C5 0.027 at 22 dB/s put all 20 windows
/// within 5 dB (worst +4.7, C4 held 2 s at 50-100 ms; the left channel alone
/// 19 of 20, that window +5.1); C4 at 0.05 left that window +5.2, C5 at 0.05
/// and 25 dB/s its 0.1-0.3 s window +7.1, C5 at 20 dB/s its last window +3.9,
/// C4 at 30 dB/s its last window +5.5.
const REMNANT_PARTIALS: usize = 8;
const REMNANT_CENTS: f64 = 8.0;
const REMNANT_KEYS: [u8; 2] = [60, 72];
const REMNANT_SHARE_C4: f64 = 0.042;
const REMNANT_SHARE_C5: f64 = 0.027;
const REMNANT_DB_C4: f64 = 33.0;
const REMNANT_DB_C5: f64 = 22.0;

/// `(at C4, at C5)` for `pitch`: straight between the two keys the
/// reference's release score plays, held outside them.
fn by_key(pitch: u8, c4: f64, c5: f64) -> f64 {
    let [p0, p1] = REMNANT_KEYS;
    let t = ((f64::from(pitch) - f64::from(p0)) / (f64::from(p1) - f64::from(p0))).clamp(0.0, 1.0);
    c4 + (c5 - c4) * t
}

/// The remnant's decay for `pitch`, dB per second.
fn remnant_db(pitch: u8) -> f64 {
    by_key(pitch, REMNANT_DB_C4, REMNANT_DB_C5)
}

/// The share of each low partial and twin a release leaves for `pitch`.
fn remnant_share(pitch: u8) -> f64 {
    by_key(pitch, REMNANT_SHARE_C4, REMNANT_SHARE_C5)
}

fn tag_of(kind: u32, n: usize) -> u32 {
    (kind << 24) | ((n as u32) << 8)
}

/// What a strike must find to LAND on a ringing string rather than replace
/// it: the same note on the same string at the same fret — what `lands`
/// compares. Every knob re-voices what rings when the finger lets go
/// (`revoice`), so a re-pluck after a knob move is still a re-pluck.
///
/// **Why the knobs left the identity.** The first draft compared DECAY and
/// WIDTH too, as the grand does, so a re-strike after either had moved was a
/// new voice, and the frame buried the old one among its corpses — three of
/// them, each faded over 4 ms. A six-string chord re-struck after a knob move
/// buried six at once, and the fourth to the sixth silenced corpses whose fade
/// had not begun: three ringing strings cut dead (the lane verifier, finding
/// 2: energy above 6 kHz in the first 3 ms -57 dBFS against -81 with the
/// knobs still). A guitar has six strings and one note on each, so a
/// re-strike that keeps its string never needs a corpse.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Design {
    place: Place,
    pitch: u8,
}

/// The knobs a string's modes were built with beyond its place: what a
/// re-pluck finds changed and re-voices (`revoice`). POSITION, VOICING and
/// PICK shape only the pluck itself; STRINGS only what a release leaves.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Voicing {
    decay: u32,
    width: u32,
    release: u32,
}

impl Voicing {
    fn of(k: &GuitarKnobs) -> Voicing {
        Voicing {
            decay: k.decay.to_bits(),
            width: k.width.to_bits(),
            release: k.release.to_bits(),
        }
    }
}

/// The pluck a strike designed, waiting for the finger to let go: per
/// partial (the first `n_partials` entries) and per twin (after them), the
/// numbers its mode is built from. Plain numbers rather than whole
/// `ModeSpec`s — the microphones' weights are formed from `heard` when the
/// mode is added — which is 17 KB a voice where the first draft's specs took
/// 42 (the lane verifier, finding 8: every voice slot owns a guitar voice).
struct Plan {
    freq: [f64; MAX_PENDING],
    sigma: [f64; MAX_PENDING],
    damp: [f64; MAX_PENDING],
    /// The coordinate the pluck gives the mode (real: released from rest).
    start: [f64; MAX_PENDING],
    /// What a restrike's finger leaves of the mode it lands on.
    keep: [f64; MAX_PENDING],
    /// Output units per unit of coordinate, before the microphones split it.
    heard: [f64; MAX_PENDING],
    drive: [f64; MAX_PENDING],
    tag: [u32; MAX_PENDING],
}

impl Plan {
    fn new() -> Box<Plan> {
        Box::new(Plan {
            freq: [0.0; MAX_PENDING],
            sigma: [0.0; MAX_PENDING],
            damp: [0.0; MAX_PENDING],
            start: [0.0; MAX_PENDING],
            keep: [0.0; MAX_PENDING],
            heard: [0.0; MAX_PENDING],
            drive: [0.0; MAX_PENDING],
            tag: [0; MAX_PENDING],
        })
    }

    /// Entry `i` as a mode the microphones hear at `width`, its frequency
    /// times `ratio` (the glide), from `start`.
    fn spec(&self, i: usize, width: f64, ratio: f64, start: C64) -> ModeSpec {
        let (left, right) = mics(self.heard[i], self.freq[i], width);
        ModeSpec {
            freq: self.freq[i] * ratio,
            sigma: self.sigma[i],
            force: C64::ZERO,
            probe: C64::ZERO,
            left,
            right,
            drive: self.drive[i],
            start,
            damp: self.damp[i],
            tag: self.tag[i],
        }
    }
}

/// The three readings a mode with these weights gives of coordinate `z`:
/// the two microphones, and the drive (the bank reads it along the mid
/// weight's phase, `modal.rs`).
fn readings(left: C64, right: C64, drive: f64, z: C64) -> [f64; 3] {
    let mid = (left + right).scale(0.5);
    let d = if mid.abs() > 0.0 { drive * (mid * z).re / mid.abs() } else { 0.0 };
    [(left * z).re, (right * z).re, d]
}

/// What a rebuild's loop culls with afterwards: anything heard under 1e-15
/// (300 dB down). Since `rewrite` is `Bank::respec` it empties no mode, so
/// this removes only modes that have rung out to nothing; kept because the
/// mode order, and so every sum, is what the lane signed off.
const SWAP_FLOOR: f32 = 1.0e-15;

/// **Re-make ringing mode `slot` as `spec`, its motion carried over**: at
/// this sample it is where it was and moving as it was, and only its future
/// follows the new frequency, decay, damping and microphones. Every live
/// rebuild in this file goes through here: the glide's retune (`glide`), a
/// re-pluck's re-voicing (`revoice`) and a choke reaching a release's
/// remnant (`damp_remnants`). The bank does it in place (`Bank::respec`):
/// the slot keeps its coordinate and takes the new coefficients, so no free
/// slot is needed, and a spec the bank refuses leaves the mode as it was.
/// (Before the bank had `respec` this added a copy at the slot's coordinate
/// and zeroed the original for the caller's `SWAP_FLOOR` cull; the renders
/// are bit-identical either way.)
fn rewrite(bank: &mut Bank, slot: usize, spec: &ModeSpec) -> bool {
    bank.respec(slot, spec)
}

/// The finger's contact noise: xorshift through a high-pass and two
/// low-passes. Real-time trivial.
#[derive(Clone, Copy)]
struct Noise {
    seed: u32,
    hp: f32,
    hp_prev: f32,
    lp1: f32,
    lp2: f32,
    a_hp: f32,
    a_lp1: f32,
    a_lp2: f32,
    /// What the filters leave of white noise's RMS, from their impulse
    /// response in `new`.
    rms: f32,
}

impl Noise {
    fn new(rate: f32) -> Noise {
        let a_hp = (-TAU as f32 * NOISE_HP_HZ as f32 / rate).exp();
        let a_lp1 = 1.0 - (-TAU as f32 * NOISE_LP1_HZ as f32 / rate).exp();
        let a_lp2 = 1.0 - (-TAU as f32 * NOISE_LP2_HZ as f32 / rate).exp();
        let mut n = Noise {
            seed: 1,
            hp: 0.0,
            hp_prev: 0.0,
            lp1: 0.0,
            lp2: 0.0,
            a_hp,
            a_lp1,
            a_lp2,
            rms: 1.0,
        };
        // The filters' power gain is the energy of their impulse response;
        // white noise uniform on [-1, 1) has a variance of 1/3. (The first
        // draft measured 20 000 noise samples per voice, which was most of
        // what `Keys::new` spent on the guitar: 3.4 ms over 67 voices.)
        let mut e = 0.0_f64;
        let mut x = 1.0_f32;
        // 50 ms: the 100 Hz high-pass, the slowest, is 270 dB down by then.
        let len = (0.05 * rate) as usize;
        for _ in 0..len {
            let y = n.filter(x);
            e += f64::from(y) * f64::from(y);
            x = 0.0;
        }
        n.rms = ((e / 3.0).sqrt() as f32).max(1.0e-9);
        n.reset(1);
        n
    }

    /// Start over from `seed` (never zero: xorshift's one fixed point).
    fn reset(&mut self, seed: u32) {
        self.seed = seed.max(1);
        self.hp = 0.0;
        self.hp_prev = 0.0;
        self.lp1 = 0.0;
        self.lp2 = 0.0;
    }

    fn filter(&mut self, white: f32) -> f32 {
        // One-pole high-pass, then two one-pole low-passes.
        self.hp = self.a_hp * (self.hp + white - self.hp_prev);
        self.hp_prev = white;
        self.lp1 += self.a_lp1 * (self.hp - self.lp1);
        self.lp2 += self.a_lp2 * (self.lp1 - self.lp2);
        self.lp2
    }

    fn next(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        let white = (self.seed as f32 / u32::MAX as f32) * 2.0 - 1.0;
        self.filter(white)
    }
}

/// Each string's skin noise its own: six fingers on six strings rub
/// independently, so a chord's contact noises add in power, not in step (one
/// seed for every voice made a six-string strum's noise 7.8 dB louder than
/// six independent ones). Deterministic per string and note.
fn noise_seed(place: Place, pitch: u8) -> u32 {
    let mut z = (place.string as u32) << 16 | u32::from(pitch) << 8 | u32::from(place.fret);
    z = z.wrapping_mul(0x9e37_79b9) ^ 0x2545_f491;
    z ^= z >> 15;
    z = z.wrapping_mul(0x85eb_ca6b);
    z ^ (z >> 13)
}

/// One control block of the guitar voice: how long it is, how far the voice
/// has rendered into it, how much of that was past the finger's contact,
/// and whether the damper's place for the block has been set. The damper
/// moves once a block, to where the block will leave it, and the bank is
/// culled and the voice judged at a block's end; a host's call may end
/// inside a block and the next goes on with it, so these run on a grid of
/// the voice's own, every `BLOCK` frames from the last strike or damper.
/// Once a call, a release rendered in calls of 1, 16 or 37 frames was -26
/// to -31 dB re the signal off the 128-frame take. (The glide keeps its
/// own grid from the finger's release: `glide_due`.)
#[derive(Clone, Copy, Default)]
struct Ctl {
    len: usize,
    done: usize,
    post: usize,
    damper_set: bool,
}

/// One string, plucked: the note a key plays on the guitar.
pub struct GuitarVoice {
    rate: f32,
    bank: Bank,
    plan: Box<Plan>,
    n_pending: usize,
    /// How many of the pending entries are partials (the rest are twins).
    n_partials: usize,
    design: Option<Design>,
    /// The knobs the ringing modes were built with, and the width their
    /// microphone weights were formed at (`revoice`).
    built: Option<Voicing>,
    built_width: f64,
    /// This strike's knobs, and whether it landed on what rings.
    voicing: Voicing,
    landing: bool,
    place: Place,
    pitch: u8,
    /// The phantom coupling for this strike, newtons per metre squared.
    phantom_q: f64,
    /// The glide (see `GLIDE`): the frequency ratio the bank's modes play at,
    /// the cents it stands for, and this strike's cents per unit of the
    /// weighted slope energy (zero: no glide).
    ratio: f64,
    glide_cents: f64,
    glide_k: f64,
    /// Frames rendered since the release or the glide's last step (it steps
    /// every `BLOCK` of them, `render`), and each low partial's weight
    /// `n^GLIDE_EXP` in it, tabled once (`glide`).
    glide_due: usize,
    glide_w: [f64; GLIDE_PARTIALS + 1],
    width: f64,
    /// `GAIN` with this place's voicing (`level_trim`).
    gain: f64,
    /// Samples of contact left before the finger lets go; `None` once it has.
    contact: Option<usize>,
    contact_total: usize,
    /// The string was taken before the finger let go (`choke`): nothing will
    /// be plucked.
    abandoned: bool,
    noise: Noise,
    noise_gain: f32,
    /// What the pluck will sound like, for the stealing policy while the
    /// finger is still on the string.
    seed: f32,
    /// The onset correction: the plateau each output took on at the release
    /// (left, right, drive), and how far into its decay it is.
    plateau: [f32; 3],
    onset_t: usize,
    onset_len: usize,
    onset_w: f32,
    /// Samples of the attack left in which the bank is not culled.
    hold: usize,
    /// The damper: falling, how far through its fall, its fall time, where
    /// it falls from and to, and a key-up that arrived while the finger was
    /// still on the string (it waits for the pluck).
    falling: bool,
    fall_t: f32,
    fall_secs: f32,
    fall_from: f32,
    fall_to: f32,
    pending_damp: Option<(f32, f32)>,
    release_secs: f32,
    /// The share of each low partial a release leaves ringing, and whether
    /// this strike has left it yet.
    remnant: f64,
    remnant_left: bool,
    done: bool,
    /// The control block being rendered (see [`Ctl`]).
    ctl: Ctl,
}

impl GuitarVoice {
    pub fn new(sample_rate: f32) -> Self {
        let rate = if sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        let zero = Voicing {
            decay: 0,
            width: 0,
            release: 0,
        };
        GuitarVoice {
            rate,
            bank: Bank::new(rate),
            plan: Plan::new(),
            n_pending: 0,
            n_partials: 0,
            design: None,
            built: None,
            built_width: 1.0,
            voicing: zero,
            landing: false,
            place: Place { string: 0, fret: 0 },
            pitch: 64,
            phantom_q: 0.0,
            ratio: 1.0,
            glide_cents: 0.0,
            glide_k: 0.0,
            glide_due: 0,
            glide_w: std::array::from_fn(|n| (n as f64).powf(GLIDE_EXP)),
            width: 1.0,
            gain: GAIN,
            contact: None,
            contact_total: 0,
            abandoned: false,
            noise: Noise::new(rate),
            noise_gain: 0.0,
            seed: 0.0,
            plateau: [0.0; 3],
            onset_t: 0,
            onset_len: 0,
            onset_w: (TAU * ONSET_HZ / f64::from(rate)) as f32,
            hold: 0,
            falling: false,
            fall_t: 0.0,
            fall_secs: FALL_SECS,
            fall_from: 0.0,
            fall_to: 0.0,
            pending_damp: None,
            release_secs: FALL_SECS,
            remnant: REMNANT_SHARE_C4,
            remnant_left: false,
            done: true,
            ctl: Ctl::default(),
        }
    }

    /// Silence now.
    pub fn silence(&mut self) {
        self.bank.clear();
        self.design = None;
        self.built = None;
        self.n_pending = 0;
        self.n_partials = 0;
        self.contact = None;
        self.abandoned = false;
        self.falling = false;
        self.fall_t = 0.0;
        self.pending_damp = None;
        self.onset_t = self.onset_len;
        self.seed = 0.0;
        self.ratio = 1.0;
        self.glide_cents = 0.0;
        self.glide_due = 0;
        self.done = true;
        self.ctl = Ctl::default();
    }

    /// Whether a strike of `pitch` at `place` lands on what is ringing (see
    /// `Design`): the same note on the same string and fret. The knobs are
    /// taken for the frame's sake and change nothing here — a re-pluck under
    /// moved knobs re-voices the string instead (`revoice`).
    pub fn lands(&self, place: Place, pitch: u8, _knobs: &GuitarKnobs) -> bool {
        !self.done && self.design == Some(Design { place, pitch })
    }

    pub fn finished(&self) -> bool {
        self.done
    }

    /// The string this voice is on.
    pub fn string(&self) -> usize {
        self.place.string
    }

    /// Loudness for the stealing policy: the bank's own level (measured every
    /// block, the attack's included: `render`), or what the pluck is about to
    /// be while the finger is on the string.
    pub fn level(&self) -> f32 {
        if self.contact.is_some() {
            self.bank.amplitude().max(self.seed)
        } else {
            self.bank.amplitude()
        }
    }

    /// Key up. Under the pedal the string rings on.
    pub fn release(&mut self, pedal: bool) {
        if !pedal {
            self.damp();
        }
    }

    /// The finger comes down on the string: per mode, a third of the way
    /// along, never a step (`Bank::set_damper`).
    pub fn damp(&mut self) {
        self.close_for_damper();
        self.start_damper(self.release_secs, RELEASE_PRESS);
    }

    /// A damper about to start from this frame: the control block it falls
    /// in ends here (see `Ctl`). One that will wait for the pluck (the
    /// finger still on the string) starts at the release, inside a block,
    /// as the release itself does.
    fn close_for_damper(&mut self) {
        if !self.done && self.contact.is_none() {
            self.close_block();
        }
    }

    /// Another note took this string: the next finger is already on it.
    ///
    /// **A finger that has not let go never plucks.** The frame chokes AFTER
    /// the newcomer's strike, so a chord of more keys than there are strings
    /// chokes notes still in their contact phase. The first draft queued the
    /// damper behind the pluck as it does a key-up, and every such note
    /// plucked in full before it was stopped: ghost attacks that grew with
    /// the chord (44 keys at MIDI 127 peaked at +6.7 dBFS, 20 at +0.1; the
    /// lane verifier's `sim` scores, finding 1). Its pluck is dropped instead
    /// (`abandon`), and whatever still rang from before (a re-pluck) is damped
    /// as any choke damps it.
    pub fn choke(&mut self) {
        if self.done {
            return;
        }
        if self.contact.is_some() {
            self.abandon();
        }
        self.close_for_damper();
        self.start_damper(CHOKE_SECS, 1.0);
    }

    /// The pluck that will not happen: the planned modes, their phantoms and
    /// the level they promised go, and the skin's noise fades out over a
    /// millisecond from where it is (at once if it had not begun to sound).
    fn abandon(&mut self) {
        let Some(left) = self.contact else {
            return;
        };
        self.abandoned = true;
        self.n_pending = 0;
        self.phantom_q = 0.0;
        self.glide_k = 0.0;
        self.seed = 0.0;
        let sounded = self.contact_total - left;
        let fade = ((0.001 * self.rate) as usize).max(1);
        let end = if sounded == 0 { 0 } else { left.min(fade) };
        // The contact's length moves with its end, so the noise envelope
        // (`render`) runs on from where it is.
        self.contact_total -= left - end;
        self.contact = Some(end);
    }

    fn start_damper(&mut self, secs: f32, to: f32) {
        if self.done {
            return;
        }
        if self.contact.is_some() {
            // The finger has not let go yet: the damper waits for the pluck.
            // The harder of two presses wins, with its own speed.
            self.pending_damp = match self.pending_damp {
                Some((s, t)) if t >= to => Some((s, t)),
                _ => Some((secs, to)),
            };
            return;
        }
        if to <= RELEASE_PRESS && !self.falling && !self.remnant_left {
            self.leave_remnant();
        }
        if to > RELEASE_PRESS {
            self.damp_remnants();
        }
        // From wherever it is, never back up: a choke during a release
        // presses harder, a release during a choke changes nothing.
        if !self.falling || to > self.fall_to {
            self.fall_from = self.bank.damper();
            self.fall_to = to.max(self.fall_from);
            self.fall_secs = secs.max(1.0e-3);
            self.fall_t = 0.0;
            self.falling = true;
        }
    }

    /// The plan entry behind a mode of `kind` and number `n` that a release
    /// splits or left (a partial and its remnant, a twin and its), if it is
    /// one the release splits.
    fn remnant_source(&self, kind: u32, n: usize) -> Option<(usize, u32)> {
        let low = REMNANT_PARTIALS.min(self.n_partials);
        match kind {
            KIND_STRING | KIND_REMNANT if (1..=low).contains(&n) => Some((n - 1, KIND_REMNANT)),
            KIND_POLAR | KIND_REMNANT_TWIN if (1..=low.min(POLAR_PARTIALS)).contains(&n) => Some((self.n_partials + n - 1, KIND_REMNANT_TWIN)),
            _ => None,
        }
    }

    /// The remnant of plan entry `i` (see `KIND_REMNANT`), tagged `kind` and
    /// `n`: `REMNANT_CENTS` sharp of it, decaying at `remnant_db`, heard as
    /// the entry is, from `start`, with `damp` under a pressed damper. `None`
    /// above 0.45 fs.
    fn remnant_spec(&self, i: usize, kind: u32, n: usize, start: C64, damp: f64) -> Option<ModeSpec> {
        let freq = self.plan.freq[i] * 2.0_f64.powf(REMNANT_CENTS / 1200.0) * self.ratio;
        if freq >= 0.45 * f64::from(self.rate) {
            return None;
        }
        let (left, right) = mics(self.plan.heard[i], self.plan.freq[i], self.width);
        Some(ModeSpec {
            freq,
            sigma: remnant_db(self.pitch) * LN_10 / 20.0,
            force: C64::ZERO,
            probe: C64::ZERO,
            left,
            right,
            drive: self.plan.drive[i],
            start,
            damp,
            tag: tag_of(kind, n),
        })
    }

    /// The release's split: `remnant_share` of each low partial's and twin's
    /// coordinate moves to an undamped copy `REMNANT_CENTS` sharp (see
    /// `KIND_REMNANT`).
    ///
    /// **One remnant per partial, however fast the key repeats.** A re-pluck
    /// lands on what rings, the remnant of the last release included, and the
    /// next release's share goes INTO that mode (the same partial's copy at
    /// the same frequency and decay, so the sum is exactly the two modes
    /// superposed). The first draft added a new pair of modes at every
    /// release: E2 plucked and let go ten times a second (a tremolo) grew by
    /// 14 modes a pluck, and because the attack's no-cull hold (150 ms)
    /// restarts at every pluck, nothing was culled: the bank was full at 768
    /// modes after 40 plucks, a re-pluck's partials then refused (the lane
    /// fix pass; `repeated_plucks_never_pile_up_modes`).
    fn leave_remnant(&mut self) {
        self.remnant_left = true;
        if self.remnant <= 0.0 {
            return;
        }
        let count = self.bank.count();
        // Where an earlier release's remnant of each low partial and twin is.
        let mut rem_of = [[u16::MAX; REMNANT_PARTIALS + 1]; 2];
        for k in 0..count {
            let t = self.bank.tag(k);
            let n = ((t >> 8) & 0xffff) as usize;
            if n <= REMNANT_PARTIALS {
                match t >> 24 {
                    KIND_REMNANT => rem_of[0][n] = k as u16,
                    KIND_REMNANT_TWIN => rem_of[1][n] = k as u16,
                    _ => {}
                }
            }
        }
        for slot in 0..count {
            let t = self.bank.tag(slot);
            let n = ((t >> 8) & 0xffff) as usize;
            if t >> 24 != KIND_STRING && t >> 24 != KIND_POLAR {
                continue;
            }
            let Some((i, kind)) = self.remnant_source(t >> 24, n) else {
                continue;
            };
            let z = self.bank.coordinate(slot);
            let held = rem_of[usize::from(kind == KIND_REMNANT_TWIN)][n];
            if held != u16::MAX {
                let r = usize::from(held);
                let zr = self.bank.coordinate(r);
                self.bank.reseed(r, zr + z.scale(self.remnant));
                self.bank.reseed(slot, z.scale(1.0 - self.remnant));
                continue;
            }
            let Some(spec) = self.remnant_spec(i, kind, n, z.scale(self.remnant), 0.0) else {
                continue;
            };
            if self.bank.add(&spec) {
                self.bank.reseed(slot, z.scale(1.0 - self.remnant));
            }
        }
    }

    /// **A choke stops the whole string**, the part a release left ringing
    /// too: the remnant is added undamped so that the release does not stop
    /// it, and a mode's damping is fixed when it is added, so each remnant
    /// mode is rebuilt where it is with its partial's damping, and the choke's
    /// press reaches it (the first draft's remnant rang on under the next
    /// note's finger: C5 released, D5 taking its string 0.1 s later, C5's tail
    /// identical with and without D5; the lane verifier, finding 9).
    fn damp_remnants(&mut self) {
        let count = self.bank.count();
        let mut moved = false;
        for slot in 0..count {
            let t = self.bank.tag(slot);
            let n = ((t >> 8) & 0xffff) as usize;
            if t >> 24 != KIND_REMNANT && t >> 24 != KIND_REMNANT_TWIN {
                continue;
            }
            let Some((i, kind)) = self.remnant_source(t >> 24, n) else {
                continue;
            };
            let Some(spec) = self.remnant_spec(i, kind, n, C64::ZERO, self.plan.damp[i]) else {
                continue;
            };
            moved |= rewrite(&mut self.bank, slot, &spec);
        }
        if moved {
            self.bank.cull(SWAP_FLOOR);
        }
    }

    /// Pluck `pitch` at `place` with `velocity` (MIDI / 127).
    pub fn strike(&mut self, place: Place, pitch: u8, velocity: f32, knobs: &GuitarKnobs) {
        // The finger lands on the string as this frame finds it: the control
        // block it falls in ends here (see `Ctl`).
        self.close_block();
        let design = Design { place, pitch };
        let landing = !self.done && self.design == Some(design);
        if !landing {
            self.bank.clear();
            self.built = None;
            self.ratio = 1.0;
            self.glide_cents = 0.0;
            self.glide_due = 0;
        }
        // A finger re-plucking lifts the damper with it, from where it is.
        self.bank.set_damper(0.0);
        self.falling = false;
        self.fall_t = 0.0;
        self.pending_damp = None;
        self.design = Some(design);
        self.voicing = Voicing::of(knobs);
        self.landing = landing;
        self.place = place;
        self.pitch = pitch;
        self.width = f64::from(knobs.width);
        self.gain = GAIN * level_trim(pitch);
        self.release_secs = (FALL_SECS * knobs.release).clamp(0.002, 0.2);
        self.remnant = remnant_share(pitch) * f64::from(knobs.strings);
        self.remnant_left = false;
        self.abandoned = false;
        // A NaN velocity is a finger that touches and does not pull (silent,
        // as the grand's is no strike); an infinite one is a fortissimo.
        let velocity = if velocity.is_nan() { 0.0 } else { velocity.clamp(0.0, 1.0) };
        let midi = f64::from(velocity) * 127.0;
        let contact_secs = contact_law(midi) * f64::from(knobs.pick);
        self.contact_total = (contact_secs * f64::from(self.rate)).round() as usize;
        self.design_pluck(midi, knobs, landing, contact_secs);
        self.contact = Some(self.contact_total);
        self.noise.reset(noise_seed(place, pitch));
        self.done = false;
    }

    /// The pluck's modes, computed at the strike and applied at the release.
    fn design_pluck(&mut self, midi: f64, knobs: &GuitarKnobs, landing: bool, contact_secs: f64) {
        let fs = f64::from(self.rate);
        let place = self.place;
        let s = place.string;
        let fret = f64::from(place.fret);
        let wound = WOUND[s];
        let f0 = f_equal(self.pitch);
        let b = inharmonicity(place);
        let big_f = f0 / (1.0 + b).sqrt();
        let len = place.length();
        let beta = (BETA_OPEN * 2.0_f64.powf(fret / 12.0) * f64::from(knobs.position)).clamp(0.02, beta_max());
        let tension = TENSION[s];
        let push = PLUCK_N * (midi / 127.0).max(0.0).powf(VELOCITY_EXP);
        let depth = push * beta * (1.0 - beta) * len / tension;
        let nail = knobs.nail(midi);
        let decay = f64::from(knobs.decay);
        let release = f64::from(knobs.release);
        let top = TOP_HZ.min(0.45 * fs);
        let x1 = excitation(f0, f0, wound, nail);
        let mut energy = 0.0_f64;
        self.n_pending = 0;
        self.n_partials = 0;
        for n in 1..=MAX_PARTIALS {
            let nf = n as f64;
            let f = nf * big_f * (1.0 + b * nf * nf).sqrt();
            if f >= top {
                break;
            }
            // The triangle's share in mode n, released from rest:
            // 2 h sin(n pi beta) / (n^2 pi^2 beta (1 - beta)), the comb
            // floored at `NOTCH_FLOOR`.
            let comb = (nf * PI * beta).sin();
            let comb = comb.signum() * (comb * comb + NOTCH_FLOOR * NOTCH_FLOOR).sqrt();
            let shape = 2.0 * comb / (nf * nf * PI * PI * beta * (1.0 - beta));
            let q = depth * shape * excitation(f, f0, wound, nail) / x1;
            // The string's pull on the bridge per metre of mode n: T n pi / L.
            let pull = tension * nf * PI / len;
            let heard = self.gain * radiation(f) * pull;
            energy += 0.5 * (heard * q).powi(2);
            // What a restrike's finger leaves of mode n after its contact.
            let sb = (nf * PI * beta).sin();
            let i = n - 1;
            self.plan.freq[i] = f;
            self.plan.sigma[i] = mode_sigma(place, f) / decay;
            self.plan.damp[i] = CHOKE_RATE * damper_rate(n) / release;
            self.plan.start[i] = q;
            self.plan.keep[i] = if landing { (-FINGER_NP * contact_secs * sb * sb).exp() } else { 0.0 };
            self.plan.heard[i] = heard;
            self.plan.drive[i] = self.gain * pull;
            self.plan.tag[i] = tag_of(KIND_STRING, n);
            self.n_partials = n;
        }
        self.n_pending = self.n_partials;
        // The twins, after the partials so the partials keep their indices.
        for n in 1..=self.n_partials.min(POLAR_PARTIALS) {
            let f = self.plan.freq[n - 1];
            let u = 0.5 * (1.0 + hash_unit(s, place.fret, n as u32, 7));
            let detune = POLAR_HZ_MIN + (POLAR_HZ_MAX - POLAR_HZ_MIN) * u;
            let pull = tension * n as f64 * PI / len;
            let k = self.n_pending;
            self.plan.freq[k] = f + detune;
            self.plan.sigma[k] = (internal_loss(f) + POLAR_LOSS * 2.0_f64.powf(fret / 12.0) * bridge_loss(s, f)) * LN_10 / 20.0 / decay;
            self.plan.damp[k] = self.plan.damp[n - 1];
            self.plan.start[k] = self.plan.start[n - 1] * POLAR_EXCITE * twin_share(f, place);
            self.plan.keep[k] = self.plan.keep[n - 1];
            self.plan.heard[k] = self.gain * radiation(f) * pull * POLAR_RADIATE;
            self.plan.drive[k] = self.gain * pull * POLAR_RADIATE;
            self.plan.tag[k] = tag_of(KIND_POLAR, n);
            self.n_pending = k + 1;
        }
        self.seed = energy.sqrt() as f32;
        self.noise_gain = (f64::from(self.seed) * 10.0_f64.powf(NOISE_DB / 20.0)) as f32 / self.noise.rms;
        let v = midi / 127.0;
        self.phantom_q = if self.pitch <= PHANTOM_UNTIL {
            PHANTOM * EA_NYLON * PI * PI / (8.0 * len * len) * v * v
        } else {
            0.0
        };
        // cents = (1200 / ln 2) dT / 2T, dT = (EA / 8) (pi / L)^2 sum n^2 |z_n|^2.
        // **The velocity it scales by is a MIDI one, 1 or over.** At a silent
        // press `v^GLIDE_VEL` was infinite, its product with the silent bank's
        // zero energy NaN (`glide`), and so was the ratio a landing on that
        // string then rebuilt its modes' readings at (`let_go`): two silent
        // presses of one key, or one landing on a ringing string at 1e-9, put
        // NaN in the onset correction, the string's bus and from there the
        // open strings and the air (verifier, 0d20e3d1 as well:
        // `a_silent_press_on_a_ringing_string_stays_finite`).
        self.glide_k = GLIDE * 1200.0 / LN_2 * EA_NYLON / (16.0 * tension) * (PI / len).powi(2) * v.max(1.0 / 127.0).powf(GLIDE_VEL);
    }

    /// The glide's step at the end of a block (see `GLIDE`): the weighted
    /// slope energy of the low partials, the pitch it calls for, and, once
    /// that has moved `GLIDE_STEP_CENTS` from the pitch the bank plays, every
    /// mode rebuilt at the new ratio where it is.
    fn glide(&mut self) {
        let mut e = 0.0_f64;
        for slot in 0..self.bank.count() {
            let t = self.bank.tag(slot);
            let n = ((t >> 8) & 0xffff) as usize;
            if t >> 24 == KIND_STRING && (1..=GLIDE_PARTIALS).contains(&n) {
                e += self.glide_w[n] * self.bank.coordinate(slot).norm2();
            }
        }
        let cents = self.glide_k * e;
        if (cents - self.glide_cents).abs() < GLIDE_STEP_CENTS {
            return;
        }
        self.glide_cents = cents;
        self.ratio = 2.0_f64.powf(cents / 1200.0);
        let count = self.bank.count();
        let mut moved = false;
        for slot in 0..count {
            let tag = self.bank.tag(slot);
            // The sum tones stay where they are (`phantom_spec`): their spec
            // does not move with the ratio, so there is nothing to rebuild.
            if tag >> 24 == KIND_PHANTOM {
                continue;
            }
            let Some((spec, _, _)) = self.planned_spec(tag, self.width) else {
                continue;
            };
            moved |= rewrite(&mut self.bank, slot, &spec);
        }
        if moved {
            self.bank.cull(SWAP_FLOOR);
        }
    }

    /// The phantom of partial `k` (see `PHANTOM_PARTIALS`) as a mode heard
    /// at `width`, from `start`. `None` above 0.45 fs.
    fn phantom_spec(&self, k: usize, width: f64, start: C64) -> Option<ModeSpec> {
        // At the untuned 2 f_k: the glide (`GLIDE`) does not carry the sum
        // tones. Carried with their partials, they swept across the gaps the
        // reference keeps clean while its own partials glide (E2 at MIDI 127,
        // k11-k15 33.8-42.8 dB over the mid-gap against its 47.5-56.3; held
        // here, 40.8 / 57.4 / 69.0 / 41.7 / 49.2).
        let base = 2.0 * self.plan.freq[k - 1];
        let freq = base;
        if freq >= 0.45 * f64::from(self.rate) {
            return None;
        }
        let (l0, r0) = mics(self.gain * radiation(base), base, width);
        let quarter = C64::new(0.0, 1.0);
        Some(ModeSpec {
            freq,
            sigma: 2.0 * self.plan.sigma[k - 1],
            force: C64::ZERO,
            probe: C64::ZERO,
            left: l0 * quarter,
            right: r0 * quarter,
            drive: self.gain,
            start,
            damp: 2.0 * self.plan.damp[k - 1],
            tag: tag_of(KIND_PHANTOM, k),
        })
    }

    /// The spec a ringing mode tagged `tag` gets from this strike's plan (its
    /// start zero), and the microphone weights it had at `old_width`.
    fn planned_spec(&self, tag: u32, old_width: f64) -> Option<(ModeSpec, C64, C64)> {
        let n = ((tag >> 8) & 0xffff) as usize;
        let (spec, i) = match tag >> 24 {
            KIND_STRING if (1..=self.n_partials).contains(&n) => (self.plan.spec(n - 1, self.width, self.ratio, C64::ZERO), n - 1),
            KIND_POLAR if (1..=self.n_partials.min(POLAR_PARTIALS)).contains(&n) => {
                let i = self.n_partials + n - 1;
                (self.plan.spec(i, self.width, self.ratio, C64::ZERO), i)
            }
            KIND_PHANTOM if (1..=self.n_partials.min(PHANTOM_PARTIALS)).contains(&n) => {
                let spec = self.phantom_spec(n, self.width, C64::ZERO)?;
                let old = self.phantom_spec(n, old_width, C64::ZERO)?;
                return Some((spec, old.left, old.right));
            }
            KIND_REMNANT | KIND_REMNANT_TWIN => {
                let (i, kind) = self.remnant_source(tag >> 24, n)?;
                (self.remnant_spec(i, kind, n, C64::ZERO, 0.0)?, i)
            }
            _ => return None,
        };
        let (l, r) = mics(self.plan.heard[i], self.plan.freq[i], old_width);
        Some((spec, l, r))
    }

    /// **A re-pluck re-voices what rings.** A strike that lands finds the
    /// string's modes built with the knobs of an earlier one; when DECAY,
    /// WIDTH or RELEASE have moved since, every ringing mode is rebuilt from
    /// this strike's plan, continuing from where it is (its coordinate is
    /// carried over exactly, so a decay or damping change is clickless) —
    /// what the frame's corpses did by burying the voice (see `Design`). A
    /// width change moves the microphones, which steps their readings; that
    /// step goes into the onset correction with the pluck's own (returned:
    /// left, right, drive), so the outputs stay continuous.
    fn revoice(&mut self, old_width: f64) -> [f64; 3] {
        let fs = f64::from(self.rate);
        let mut jump = [0.0_f64; 3];
        let count = self.bank.count();
        let mut moved = false;
        for slot in 0..count {
            let Some((spec, old_l, old_r)) = self.planned_spec(self.bank.tag(slot), old_width) else {
                continue;
            };
            let z = self.bank.coordinate(slot);
            if !rewrite(&mut self.bank, slot, &spec) {
                continue;
            }
            moved = true;
            let zs = z * C64::polar((-spec.sigma / fs).exp(), TAU * spec.freq / fs);
            let now = readings(spec.left, spec.right, spec.drive, zs);
            let was = readings(old_l, old_r, spec.drive, zs);
            for c in 0..3 {
                jump[c] += now[c] - was[c];
            }
        }
        if moved {
            self.bank.cull(SWAP_FLOOR);
        }
        jump
    }

    /// The finger lets go: every pending mode starts from its share of the
    /// triangle (added to what a restrike's finger left of it), the phantoms
    /// are seeded from the new amplitudes, and the plateau each output steps
    /// to is recorded for the onset correction.
    fn let_go(&mut self) {
        if self.abandoned {
            // Nothing is plucked (`abandon`): no plateau, no hold, and the
            // damper that took the string now reaches what rang before.
            self.plateau = [0.0; 3];
            self.onset_t = 0;
            self.onset_len = 0;
            self.hold = 0;
            if let Some((secs, to)) = self.pending_damp.take() {
                self.start_damper(secs, to);
            }
            return;
        }
        let mut jump = [0.0_f64; 3];
        if self.landing && self.built.is_some_and(|b| b != self.voicing) {
            jump = self.revoice(self.built_width);
        }
        self.built = Some(self.voicing);
        self.built_width = self.width;
        // Where each partial and twin already rings (a restrike).
        let mut slot_of = [u16::MAX; MAX_PARTIALS + 1];
        let mut twin_of = [u16::MAX; POLAR_PARTIALS + 1];
        for k in 0..self.bank.count() {
            let t = self.bank.tag(k);
            let n = ((t >> 8) & 0xffff) as usize;
            match t >> 24 {
                KIND_STRING if (1..=MAX_PARTIALS).contains(&n) => slot_of[n] = k as u16,
                KIND_POLAR if (1..=POLAR_PARTIALS).contains(&n) => twin_of[n] = k as u16,
                _ => {}
            }
        }
        let fs = f64::from(self.rate);
        let mut amp = [C64::ZERO; PHANTOM_PARTIALS + 1];
        for i in 0..self.n_pending {
            let tag = self.plan.tag[i];
            let n = ((tag >> 8) & 0xffff) as usize;
            let twin = tag >> 24 == KIND_POLAR;
            let slot = if twin { twin_of[n] } else { slot_of[n] };
            let start = C64::new(self.plan.start[i], 0.0);
            let spec = self.plan.spec(i, self.width, self.ratio, start);
            let (z_new, dz) = if slot != u16::MAX {
                let z_old = self.bank.coordinate(usize::from(slot));
                let z_new = z_old.scale(self.plan.keep[i]) + start;
                self.bank.reseed(usize::from(slot), z_new);
                (z_new, z_new - z_old)
            } else {
                // A twin the finger does not drive (`twin_share`) is no mode.
                if (twin && self.plan.start[i] == 0.0) || !self.bank.add(&spec) {
                    continue;
                }
                (start, start)
            };
            if !twin && (1..=PHANTOM_PARTIALS).contains(&n) {
                amp[n] = z_new;
            }
            // The first output sample after the release is one step on.
            let step = C64::polar((-spec.sigma / fs).exp(), TAU * spec.freq / fs);
            let moved = readings(spec.left, spec.right, spec.drive, dz * step);
            for c in 0..3 {
                jump[c] += moved[c];
            }
        }
        let ph = self.add_phantoms(&amp);
        for c in 0..3 {
            jump[c] += ph[c];
        }
        self.plateau = [jump[0] as f32, jump[1] as f32, jump[2] as f32];
        self.onset_t = 0;
        self.onset_len = (ONSET_SECS * fs) as usize;
        self.hold = (CULL_HOLD_SECS * fs) as usize;
        self.glide_due = 0;
        if let Some((secs, to)) = self.pending_damp.take() {
            self.start_damper(secs, to);
        }
    }

    /// The phantom partials: one per low partial, its square (see
    /// `PHANTOM_PARTIALS`). Returns their plateau contribution.
    fn add_phantoms(&mut self, amp: &[C64; PHANTOM_PARTIALS + 1]) -> [f64; 3] {
        let mut jump = [0.0; 3];
        if self.phantom_q == 0.0 {
            return jump;
        }
        let parts = self.n_partials.min(PHANTOM_PARTIALS);
        let fs = f64::from(self.rate);
        let f_long = LONGITUDINAL_HZ * 2.0_f64.powf(f64::from(self.place.fret) / 12.0);
        for k in 1..=parts {
            let freq = 2.0 * self.plan.freq[k - 1];
            let mut z = (amp[k] * amp[k]).scale(0.5 * self.phantom_q * (k * k) as f64);
            for d in 1..=PHANTOM_REACH.min(k - 1) {
                if k + d > parts {
                    break;
                }
                // An off-diagonal pair appears twice in (sum y)^2 and keeps
                // all of its product, through the longitudinal mode p = 2d.
                let p = 2 * d;
                let gain = longitudinal_mode_gain(freq, p as f64 * f_long) / p as f64;
                let c = self.phantom_q * ((k - d) * (k + d)) as f64 * gain;
                z = z + (amp[k - d] * amp[k + d]).scale(c);
            }
            if z.abs() == 0.0 {
                continue;
            }
            let Some(spec) = self.phantom_spec(k, self.width, z) else {
                break;
            };
            let tag = spec.tag;
            let mut slot = None;
            for s in 0..self.bank.count() {
                if self.bank.tag(s) == tag {
                    slot = Some(s);
                    break;
                }
            }
            let dz = match slot {
                Some(s) => {
                    let old = self.bank.coordinate(s);
                    self.bank.reseed(s, z);
                    z - old
                }
                None => {
                    if !self.bank.add(&spec) {
                        continue;
                    }
                    z
                }
            };
            let step = C64::polar((-spec.sigma / fs).exp(), TAU * spec.freq / fs);
            let moved = readings(spec.left, spec.right, spec.drive, dz * step);
            for c in 0..3 {
                jump[c] += moved[c];
            }
        }
        jump
    }

    /// Render one block (at most `modal::BLOCK` frames) and ADD it into the
    /// two microphones and the bridge drive. Returns how many of its frames
    /// the voice sounded: all of them, or as many as it took to finish, so
    /// that the frame hands its string back from there
    /// (`Fretboard::voice_ended`).
    pub fn render(&mut self, l: &mut [f32], r: &mut [f32], drive: &mut [f32]) -> usize {
        let n = l.len().min(r.len()).min(drive.len()).min(BLOCK);
        let mut at = 0;
        while at < n && !self.done {
            if self.ctl.done == self.ctl.len {
                self.ctl = Ctl { len: BLOCK, ..Ctl::default() };
            }
            let m = (self.ctl.len - self.ctl.done).min(n - at);
            self.run(&mut l[at..at + m], &mut r[at..at + m], &mut drive[at..at + m]);
            self.ctl.done += m;
            at += m;
            if self.ctl.done == self.ctl.len {
                self.end_block();
            }
        }
        at
    }

    /// A control block ends, full or cut short by an event: the damper's
    /// clock moves by what it ran past the contact, the bank is culled (or
    /// in the attack only measured) and the voice judged. A block that was
    /// all contact has none of that.
    fn end_block(&mut self) {
        let post = self.ctl.post;
        self.ctl = Ctl::default();
        if post == 0 {
            return;
        }
        if self.falling {
            self.fall_t += post as f32 / self.rate;
        }
        if self.hold > 0 {
            // No cull in the attack (`CULL_HOLD_SECS`), but the level is
            // measured all the same: a floor of zero removes nothing, and
            // the stealing policy reads a pluck's level from its first
            // block on (it read the empty bank's zero for 150 ms; the
            // lane verifier, finding 13).
            self.hold = self.hold.saturating_sub(post);
            self.bank.cull(0.0);
        } else {
            self.bank.cull(CULL);
        }
        if self.bank.count() == 0 && self.onset_t >= self.onset_len {
            self.done = true;
            self.design = None;
            self.built = None;
        }
    }

    /// An event lands inside a control block: the block ends where it
    /// stands, and the next one starts at the event.
    fn close_block(&mut self) {
        if self.ctl.done > 0 && self.ctl.done < self.ctl.len {
            self.end_block();
        }
    }

    /// The next `l.len()` frames of the control block, at most what is left
    /// of it: the finger's contact up to its release, then the string.
    fn run(&mut self, l: &mut [f32], r: &mut [f32], drive: &mut [f32]) {
        let n = l.len();
        let mut at = 0;
        if let Some(left) = self.contact {
            // The finger on the string: what still rings (a restrike) and
            // the skin's noise, up to the release.
            let k = left.min(n);
            if k > 0 {
                self.bank.render(&mut l[..k], &mut r[..k], &mut drive[..k]);
                let fade = (0.001 * self.rate).max(1.0);
                for i in 0..k {
                    let since = (self.contact_total - left + i) as f32;
                    let until = (left - i) as f32;
                    let env = (since / fade).min(1.0).min(until / fade);
                    let v = self.noise.next() * self.noise_gain * env;
                    l[i] += v;
                    r[i] += v;
                    drive[i] += v * 0.2;
                }
            }
            if left <= n {
                self.contact = None;
                self.let_go();
                at = left;
            } else {
                self.contact = Some(left - n);
                at = n;
            }
        }
        if at < n {
            if self.falling && !self.ctl.damper_set {
                // Once per block, a smoothstep from where the fall began, so
                // it neither starts nor lands with a corner (`grand.rs`): set
                // where the block will leave it, at its first frame past the
                // contact; its clock moves at the block's end.
                let rest = (self.ctl.len - self.ctl.done - at) as f32;
                let x = ((self.fall_t + rest / self.rate) / self.fall_secs).min(1.0);
                let a = self.fall_from + (self.fall_to - self.fall_from) * x * x * (3.0 - 2.0 * x);
                self.bank.set_damper(a);
                self.ctl.damper_set = true;
            }
            // **The glide steps every `BLOCK` frames after the release,
            // wherever the host's blocks fall** (`GLIDE`): the bank runs to the
            // step, takes it, and runs on. Measured once per call instead, the
            // pitch's staircase followed the host's blocks, and two block sizes
            // of one performance parted at -36..-46 dB RMS (the lane fix pass,
            // vbench with sample-accurate events; -67..-83 before the glide,
            // the lane verifier), and every call paid for a measurement: 2.2
            // times the 128-frame load at 16-frame blocks (the verifier's
            // `run30`, 30 plucks a second at MIDI 110: 18x real time against
            // 44).
            let start = at;
            let mut from = start;
            while from < n {
                let to = if self.glide_k > 0.0 { n.min(from + (BLOCK - self.glide_due)) } else { n };
                self.bank.render(&mut l[from..to], &mut r[from..to], &mut drive[from..to]);
                if self.glide_k > 0.0 {
                    self.glide_due += to - from;
                    if self.glide_due >= BLOCK {
                        self.glide_due = 0;
                        self.glide();
                    }
                }
                from = to;
            }
            if self.onset_t < self.onset_len {
                let w = self.onset_w;
                for i in start..n {
                    if self.onset_t >= self.onset_len {
                        break;
                    }
                    let t = self.onset_t as f32;
                    let g = (1.0 - w * t) * (-w * t).exp();
                    l[i] -= self.plateau[0] * g;
                    r[i] -= self.plateau[1] * g;
                    drive[i] -= self.plateau[2] * g * DRIVE_ONSET;
                    self.onset_t += 1;
                }
            }
            self.ctl.post += n - start;
        }
    }

    /// The prompt frequency of partial n (1-based) of the last strike, Hz.
    #[cfg(test)]
    fn partial_freq(&self, n: usize) -> f64 {
        if n >= 1 && n <= self.n_partials {
            self.plan.freq[n - 1]
        } else {
            0.0
        }
    }
}

// ── the fretboard: the selector, the open strings and the air ────────────

/// Lanes per resonator group, as in `modal.rs`.
const LANES: usize = 8;

/// The most open-string partials one string rings.
const OPEN_PARTIALS: usize = 24;
/// And never above this, Hz: the sympathetic answer lives in the first
/// handful of partials, and an open string's partials above 3 kHz die in a
/// second.
const OPEN_TOP_HZ: f64 = 3000.0;

/// The open strings' coupling: the rate, nepers per second, at which a
/// partial at resonance builds an open string's mode n, as `COUPLE n`.
///
/// **Why it grows with n.** A string mode hears the bridge through its slope
/// at the bridge end, `n pi / L`, and speaks back through the same slope, so
/// the higher an open string's partial, the harder a coincident partial of
/// another string drives it. Measured against the reference's sympathetic
/// tails (`symp_tail.py`: each `gx` note's fundamental 2.5 s after the pluck,
/// re 0.1 s): a rate following each open mode's own bridge loss (the first
/// draft) put A4 9 dB and A3 17 dB too loud and B5 10 dB too quiet; `n`
/// lands all six within a factor of 2.3. One way: the open string hears the
/// bridge and the voice is not drained, so the rate is capped at
/// `COUPLE_MAX`, where an open partial driven at exact resonance by a
/// decaying one can no longer outgrow it. Set with `OPEN_DECAY`, below, and
/// re-set with the second polarisation's twins confined to where the
/// reference has them (`POLAR_LO_HZ`): at 0.08 n, with `OPEN_DECAY` at 0.58,
/// the four shares sit in the brief's bands (`sweep.py`: `gx` 40 % and 15 %
/// without the open strings, `vlow` 32 % and 20 %), A4's tail at 2.5 s -27.6
/// dB and E5's -33.8 (the reference's -29.7 and -35.1); 0.07 left `vlow` with
/// them at 27-30 %. Twice it (0.16) took `gx` to 48 % and A4's tail to -22.3
/// dB; four times (0.32) left E3's early fundamental T60 at 13.5 s against
/// the reference's 23 with A4's tail at -17.0; eight times (0.64) brought E3
/// and the 222-249 Hz band (9.8 s against 10.4) to the reference only with
/// A4's tail at -12.1 dB and E4's fundamental growing for its first half
/// second: no one-way coupling reaches the early decays (see `OPEN_DECAY`).
const COUPLE: f64 = 0.08;
const COUPLE_MAX: f64 = 2.0;

/// The open strings' own decay, as a fraction of the same string's decay
/// when it is played.
///
/// **The reference's open strings outlast its played ones.** B2's second
/// partial, coincident with the open B string's fundamental, reads a 37.9 s
/// early T60 with the open strings on and 5.9 s with them off, where the B
/// string played open lasts 4.9 s; B5's tail decays at 7.7 dB/s, slower
/// than any of the three open partials under it decays when that string is
/// played (13.5-20 dB/s). An open string answering keeps its whole length
/// free, with no finger, no fret and no plucking hand on it. At the played
/// strings' own decay no coupling reached the reference's two-stage share
/// with the open strings on (18 % against its 36 %, whatever the coupling:
/// a louder harp made louder tails, not more knees); at 0.58 of it the four
/// shares sit inside the brief's bands (`COUPLE`; the lane fix pass,
/// `sweep.py`: 0.65 left `vlow` with the open strings at 29 %, a partial
/// short of 30, 0.60 at 30, 0.55 took `gx` to 44 % beside the reference's
/// 36) and B5's tail comes 2.7 dB nearer the reference's (-41.4 against
/// -38.6; -44.1 at 0.65). Lower (0.3-0.5) rang the treble's tails nearer the
/// reference (E5 -31.5 to -32.8, B5 -36.6 to -39.7) but took `gx`'s share
/// with them to 48-61 %, and did not reach what the reference's open strings
/// do to the early decay of the partials they answer (E3's fundamental 9.3 s
/// against its 23, the 222-249 Hz band 5.0 s against 10.4): a one-way drive
/// builds the open string's answer from nothing, so it cannot slow a played
/// partial's first half second (`sweep.py`, `GD=1`). That takes the two
/// strings' modes solved together, the played partial's energy shared with
/// the open string's rather than copied into it (MERGE-ME, gaps).
const OPEN_DECAY: f64 = 0.58;

/// How fast a string being taken by a voice stops ringing open: the
/// fingertip's 55 ms, the same felt `symp.rs` closes its loops with.
const CLOSE_SECS: f64 = 0.055;

/// **Hands off the strings: they ring. Hands back on: they stop.** While no
/// key is held and the pedal is up, every open string is damped at this
/// rate, dB per second: the reference's open strings, which carry its whole
/// sound after a key-off, fall at 50-100 dB/s once the key is up (`postoff.py`
/// on the base-minus-sympathetic-off difference: E4 -3, -6, -23, -50 dB
/// re pre-off at 0-50, 50-100, 100-300, 300-600 ms; A4 -2, -11, -35, -58),
/// where an open string left alone rings for seconds. A player's hand
/// coming to rest. Lifted the moment any key goes down.
const REST_DB: f64 = 100.0;

/// The hollow body's air: the Helmholtz mode at 104 Hz and its companion
/// at 93.5 Hz (the reference's hollow-minus-solid difference, two-mode fits:
/// 103.8-105.1 Hz, T60 0.23-0.79 s, median about 0.33; the companion 90-94
/// Hz at -11..+11 dB). (Hz, T60 s, gain re the first, the phase its drive
/// arrives at re the first's, radians.)
///
/// **The companion starts nearly against the main mode.** The two beat at
/// 10.5 Hz, so how they start decides where their sum peaks: the reference's
/// peaks 15-25 ms after its release (38-49 ms after the note-on). Measured
/// on the hollow-minus-solid difference of the `gx` score (`air_v3.py`'s
/// envelope peak, less the 1.33 ms tbench lands the note early and the
/// 8.25 ms release at MIDI 80): a companion 0.7 of a half turn ahead peaked
/// 13.7-15.8 ms after the release (E3 short of the band), 0.8 at 14.7-16.8,
/// and 0.9 at 15.1-18.1 with the first 100 ms at -15.3..-17.6 dB re the
/// note (the reference's -12..-20).
const AIR: [(f64, f64, f64, f64); 2] = [(104.0, 0.33, 1.0, 0.0), (93.5, 0.35, 0.4, 0.95 * PI)];
/// The air mode's drive, per unit of the radiated sound.
///
/// **The air hears the body, not the bridge.** Driven by the bare bridge
/// force (the first draft), the air took the bass strings' fundamentals at
/// full strength where the microphones hear them 19 dB down, and rang under
/// E2 and G2 until their fundamentals' share of the note was -10 and -3 dB
/// against -19 and -11 — while the reference's air changes E2's fundamental
/// by 0.0 dB and A2's by 1.6 (hollow minus solid). Driven by what the
/// microphones hear, the air answers each pluck's onset, which is
/// broadband, and the partials near its own frequency, which the reference
/// shows too. Set against the reference's first-100-ms share of that
/// difference (-12..-20 dB re the note's 300 ms RMS).
const AIR_GAIN: f64 = 0.17;

/// The air mode's phase difference between the two microphones, radians
/// (cos 0.45 = 0.90, the reference's 0.92 in its air's octave), and the
/// right one's level against the left, both at the stock pair. Scaled by
/// WIDTH like every other microphone difference: at zero the two are one
/// microphone and the instrument is exactly mono (the first draft's air kept
/// its spread there: the difference at -27.7 dB re the sum; the lane
/// verifier, finding 10).
const AIR_MIC_RAD: f64 = 0.45;
const AIR_MIC_DROP: f64 = 0.05;

/// The exact two-tap reading of `Re(w z)` from a resonator's state, with the
/// state the real part of the coordinate (see `modal.rs`'s `taps`, whose
/// derivation this is, written out again because that one is private).
fn taps(w: C64, cos_w: f64, sin_w: f64, one_minus_r: f64, r: f64) -> (f64, f64) {
    let half = 0.5 * sin_w.atan2(cos_w);
    let one_minus_cos = 2.0 * half.sin() * half.sin();
    let cos_minus_r = one_minus_r - one_minus_cos;
    // h0 on y and h1 on v: Re(w) + Im(w) (cos w - r) / sin w and
    // Im(w) r / sin w, the small difference computed directly.
    (w.re + w.im * cos_minus_r / sin_w, w.im * r / sin_w)
}

/// One resonator of a `Resonators` bank: frequency (Hz), decay (Np/s), the
/// input's gain (dz/dt per unit of input, complex) and the three readings'
/// complex weights.
#[derive(Clone, Copy)]
struct Lane {
    freq: f64,
    sigma: f64,
    input: C64,
    left: C64,
    right: C64,
    drive: C64,
}

/// A small bank of resonators driven by one input signal: the open strings
/// and the air. The difference form of `modal.rs` (`v += b y - d2 v + ...;
/// y += v`), with the input entering through its zero-order-hold taps; and
/// what it has put out since it was last judged (`acc`, the sum of squares
/// of its two microphone readings), for the sleep test.
struct Resonators<const G: usize> {
    count: usize,
    y: [[f32; LANES]; G],
    v: [[f32; LANES]; G],
    b: [[f32; LANES]; G],
    d2: [[f32; LANES]; G],
    g0: [[f32; LANES]; G],
    g1: [[f32; LANES]; G],
    hl0: [[f32; LANES]; G],
    hl1: [[f32; LANES]; G],
    hr0: [[f32; LANES]; G],
    hr1: [[f32; LANES]; G],
    hd0: [[f32; LANES]; G],
    hd1: [[f32; LANES]; G],
    x_prev: f32,
    acc: f32,
}

impl<const G: usize> Resonators<G> {
    fn new() -> Self {
        let z = [[0.0_f32; LANES]; G];
        Resonators {
            count: 0,
            y: z,
            v: z,
            b: z,
            d2: z,
            g0: z,
            g1: z,
            hl0: z,
            hl1: z,
            hr0: z,
            hr1: z,
            hd0: z,
            hd1: z,
            x_prev: 0.0,
            acc: 0.0,
        }
    }

    /// Set lane `k` from `lane` at sample rate `rate`. Keeps the state when
    /// `keep` is set (a re-design under a ringing string: the decay change
    /// continues the trajectory as `Bank::set_sigma` does).
    fn set(&mut self, k: usize, rate: f64, lane: &Lane, keep: bool) {
        let Lane {
            freq,
            sigma,
            input,
            left: wl,
            right: wr,
            drive: wd,
        } = *lane;
        let (gi, li) = (k / LANES, k % LANES);
        let w = TAU * freq / rate;
        let sigma = sigma.max(0.0);
        let one_minus_r = -(-sigma / rate).exp_m1();
        let r = 1.0 - one_minus_r;
        let (sin_w, cos_w) = w.sin_cos();
        let p = C64::polar(r, w);
        let half = 0.5 * w;
        let one_minus_rc = one_minus_r + r * 2.0 * half.sin() * half.sin();
        let b = -(one_minus_rc * one_minus_rc + (r * sin_w) * (r * sin_w));
        let d2 = one_minus_r * (1.0 + r);
        if keep {
            // The same complex amplitude at this sample on the new decay:
            // y[n-1] rescaled by r_old / r_new (modal.rs `set_sigma`).
            let r_old = (1.0 - f64::from(self.d2[gi][li])).max(0.0).sqrt();
            if r_old > 0.0 {
                let rho = r_old / r;
                let (y, v) = (f64::from(self.y[gi][li]), f64::from(self.v[gi][li]));
                self.v[gi][li] = ((1.0 - rho) * y + rho * v) as f32;
            }
        } else {
            self.y[gi][li] = 0.0;
            self.v[gi][li] = 0.0;
        }
        let lambda = C64::new(-sigma, w * rate);
        let g = input * (p - C64::ONE) / lambda;
        self.b[gi][li] = b as f32;
        self.d2[gi][li] = d2 as f32;
        self.g0[gi][li] = g.re as f32;
        self.g1[gi][li] = -(g * p.conj()).re as f32;
        let reading = |wt: C64| -> (f32, f32) {
            let (h0, h1) = taps(wt, cos_w, sin_w, one_minus_r, r);
            (h0 as f32, h1 as f32)
        };
        let (a, c) = reading(wl);
        self.hl0[gi][li] = a;
        self.hl1[gi][li] = c;
        let (a, c) = reading(wr);
        self.hr0[gi][li] = a;
        self.hr1[gi][li] = c;
        let (a, c) = reading(wd);
        self.hd0[gi][li] = a;
        self.hd1[gi][li] = c;
        self.count = self.count.max(k + 1);
    }

    /// Every state to zero.
    fn clear(&mut self) {
        for g in 0..G {
            self.y[g] = [0.0; LANES];
            self.v[g] = [0.0; LANES];
        }
        self.x_prev = 0.0;
        self.acc = 0.0;
    }

    /// Every lane whose state has fallen under 1e-20 to zero, where it
    /// would only ever decay through the denormals. Once a control block
    /// (`BoardCtl`), not once a render: flushed at the end of each call, a
    /// lane's last 1e-20 left at a moment the host's call size chose.
    fn flush(&mut self) {
        let groups = self.count.div_ceil(LANES);
        for g in 0..groups {
            for j in 0..LANES {
                if self.y[g][j].abs() < 1.0e-20 && self.v[g][j].abs() < 1.0e-20 {
                    self.y[g][j] = 0.0;
                    self.v[g][j] = 0.0;
                }
            }
        }
    }

    /// Drive with `x` and ADD the three readings. `x`, `l`, `r` and `d` are
    /// one block long (at most `BLOCK`).
    fn render(&mut self, x: &[f32], l: &mut [f32], r: &mut [f32], d: &mut [f32]) {
        let n = x.len().min(l.len()).min(r.len()).min(d.len()).min(BLOCK);
        if n == 0 || self.count == 0 {
            return;
        }
        let mut al = [[0.0_f32; LANES]; BLOCK];
        let mut ar = [[0.0_f32; LANES]; BLOCK];
        let mut ad = [[0.0_f32; LANES]; BLOCK];
        let groups = self.count.div_ceil(LANES);
        for g in 0..groups {
            let (b, d2, g0, g1) = (self.b[g], self.d2[g], self.g0[g], self.g1[g]);
            let (hl0, hl1, hr0, hr1, hd0, hd1) = (self.hl0[g], self.hl1[g], self.hr0[g], self.hr1[g], self.hd0[g], self.hd1[g]);
            let (mut y, mut v) = (self.y[g], self.v[g]);
            let mut xp = self.x_prev;
            for s in 0..n {
                let xs = x[s];
                for j in 0..LANES {
                    v[j] += b[j] * y[j] - d2[j] * v[j] + g0[j] * xs + g1[j] * xp;
                    y[j] += v[j];
                }
                for j in 0..LANES {
                    al[s][j] += hl0[j] * y[j] + hl1[j] * v[j];
                    ar[s][j] += hr0[j] * y[j] + hr1[j] * v[j];
                    ad[s][j] += hd0[j] * y[j] + hd1[j] * v[j];
                }
                xp = xs;
            }
            self.y[g] = y;
            self.v[g] = v;
        }
        self.x_prev = x[n - 1];
        for s in 0..n {
            let (a, b) = (al[s].iter().sum::<f32>(), ar[s].iter().sum::<f32>());
            l[s] += a;
            r[s] += b;
            d[s] += ad[s].iter().sum::<f32>();
            self.acc += a * a + b * b;
        }
    }
}

/// Groups per open string (24 partials) and for the air (2 modes).
const OPEN_GROUPS: usize = OPEN_PARTIALS.div_ceil(LANES);

/// **Under this RMS an open string, or the air, is put to sleep and
/// cleared: -146 dBFS**, the harp's floor (`symp.rs`'s `FLOOR_RMS`, whose
/// doc says why: clearing a ring that quiet is not a step anyone can hear).
/// Judged on what it puts out, both microphones, over `QUIET_SECS`. They
/// slept when their state fell under 1e-9, which put a closing string to
/// sleep at -129 to -143 dBFS RMS and, after all notes off, the air at -128
/// (the block's mean square as it slept, the pedalled six-note chord): up to
/// 18 dB over this.
const FLOOR_RMS: f32 = 5.0e-8;

/// How long an open string's or the air's output must stay under
/// `FLOOR_RMS` for it to sleep: more than a period of the lowest thing it
/// rings (E2's 12.1 ms, the air's 10.7), so that a low ring is not judged
/// quiet between its peaks, which a control block's 2.7 ms can fall
/// between.
const QUIET_SECS: f64 = 0.025;

/// One control block of the fretboard: how long it is, how far the board
/// has rendered into it, whether any string's bus drove it, and whether the
/// air ran in it.
///
/// **The board's control runs on a grid of its own**, every `BLOCK` frames
/// from its last event (a key, the pedal, a string handed back at a strike,
/// all notes off), as each voice's does (`Ctl`): the hand's rest and a
/// taken string's closing move once a block, set at its first frame to
/// where the block will leave them; a string is judged asleep, and the
/// lanes flushed, at its end; and a string whose voice has ended answers
/// from the first block that starts on or after the frame it ended on
/// (`voice_ended`). A host's call may end inside a block and the next goes
/// on with it. Run once a call, all three followed the host's calls: three
/// keys let up within ten frames of one another, the hand then coming to
/// rest a staircase a call at a time, were -56 dB re peak off the 128-frame
/// take at 1- to 7-frame calls (frame-2 verifier, finding 2; -83 with the
/// rest taken out).
///
/// A block an event cuts short has had the whole block's rest or closing,
/// set at its first frame, as a voice's damper leads its block: at most
/// 0.27 dB of rest or 0.42 dB of closing more than the frames it ran, at
/// 48 kHz.
#[derive(Clone, Copy, Default)]
struct BoardCtl {
    len: usize,
    done: usize,
    driven: bool,
    air: bool,
}

/// The instrument's shared side: which string each sounding note is on,
/// the six strings nobody is holding, and the air in the body. One per
/// instrument, owned by the frame, as `symp::Harp` is for the piano.
pub struct Fretboard {
    rate: f32,
    /// Which pitch each string is playing, whether its key is down, and
    /// whether the pedal is holding it after its key came up.
    owner: [Option<u8>; STRINGS],
    held: [bool; STRINGS],
    sustained: [bool; STRINGS],
    pedal: bool,
    open: Box<[Resonators<OPEN_GROUPS>; STRINGS]>,
    /// Whether each open string is answering (no voice on it), and whether
    /// it has anything in it worth running.
    answering: [bool; STRINGS],
    awake: [bool; STRINGS],
    /// Whether the hand is resting on the strings (`REST_DB`), and which
    /// keys are down (bit p for MIDI pitch p): the hand is busy while any
    /// is, whether or not its note still sounds.
    resting: bool,
    keys: u128,
    /// The control block being rendered (see [`BoardCtl`]); for each string
    /// whose voice has ended and that answers from the next block, how many
    /// frames into the block being rendered the voice ended; and the hand's
    /// rest and a taken string's closing over one block, worked out once.
    ctl: BoardCtl,
    freeing: [Option<usize>; STRINGS],
    rest_g: f32,
    close_g: f32,
    /// The open strings were hushed with no key down (`hush`), or the board
    /// is new: until a key goes down they hear nothing.
    deaf: bool,
    /// For how many frames each open string's output, and the air's, has
    /// been under `FLOOR_RMS`, and how many make it asleep (`QUIET_SECS`).
    quiet: [usize; STRINGS],
    air_quiet: usize,
    quiet_for: usize,
    /// The knobs the open strings were last designed with, and the width
    /// the air's microphones were.
    designed: Option<(u32, u32, u32)>,
    air_width: f64,
    /// The air, and whether it may still hold anything (`end_block`).
    air: Box<Resonators<1>>,
    air_awake: bool,
    body: f32,
    /// One block of each string's bridge drive, as the voices on it render
    /// into it, and scratch.
    bus: Box<[[f32; BLOCK]; STRINGS]>,
    total: Box<[f32; BLOCK]>,
    x: Box<[f32; BLOCK]>,
    sym_d: Box<[f32; BLOCK]>,
}

impl Fretboard {
    pub fn new(sample_rate: f32) -> Self {
        let rate = if sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        let mut f = Fretboard {
            rate,
            air_width: f64::NAN,
            owner: [None; STRINGS],
            held: [false; STRINGS],
            sustained: [false; STRINGS],
            pedal: false,
            open: Box::new(std::array::from_fn(|_| Resonators::new())),
            answering: [true; STRINGS],
            awake: [false; STRINGS],
            resting: true,
            keys: 0,
            ctl: BoardCtl::default(),
            freeing: [None; STRINGS],
            // Once per block: a staircase at `REST_DB` (0.27 dB a 128-frame
            // block at 48 kHz) and at the fingertip's `CLOSE_SECS`, the same
            // exact scaling of both states.
            rest_g: (-(REST_DB * LN_10 / 20.0) * BLOCK as f64 / f64::from(rate)).exp() as f32,
            close_g: (-(BLOCK as f64) / (CLOSE_SECS * f64::from(rate))).exp() as f32,
            deaf: true,
            quiet: [0; STRINGS],
            air_quiet: 0,
            quiet_for: (QUIET_SECS * f64::from(rate)).ceil() as usize,
            designed: None,
            air: Box::new(Resonators::new()),
            air_awake: false,
            body: 1.0,
            bus: Box::new([[0.0; BLOCK]; STRINGS]),
            total: Box::new([0.0; BLOCK]),
            x: Box::new([0.0; BLOCK]),
            sym_d: Box::new([0.0; BLOCK]),
        };
        f.set_knobs(&GuitarKnobs::from_params(&DEFAULTS));
        f
    }

    /// Everything quiet and every string free: a patch change.
    pub fn clear(&mut self) {
        self.owner = [None; STRINGS];
        self.release_all();
        self.resting = true;
        self.hush();
    }

    /// Every key up and no string held by a pedal, each still its note's
    /// until the frame finds that voice done (`voice_ended`): what the
    /// frame's all notes off leaves of the strings it has just choked. It
    /// used to `clear` them, forgetting every owner while the choked voices
    /// still rang (review).
    pub fn release_all(&mut self) {
        self.close_block();
        self.held = [false; STRINGS];
        self.sustained = [false; STRINGS];
        self.answering = [true; STRINGS];
        self.keys = 0;
    }

    /// The open strings and the air silenced, who is on which string kept,
    /// and the next control block starting with the next frame.
    ///
    /// **With no key down the strings then hear nothing until one goes
    /// down** (`deaf`), as the harp's sleep until a key (`symp::Harp::hush`)
    /// and as a new board's do. The voices the frame's all notes off choked
    /// play on for a tenth of a second after its clear, and strings that
    /// answered them again took that up and rang it out under the pedal for
    /// seven seconds, at -104 dBFS RMS half a second on and -124 four
    /// seconds on (`all_notes_off_under_the_pedal_sleeps_under_the_floor`).
    pub fn hush(&mut self) {
        self.hand_back();
        self.ctl = BoardCtl::default();
        self.deaf = self.keys == 0;
        self.awake = [false; STRINGS];
        self.quiet = [0; STRINGS];
        self.air_quiet = 0;
        for o in self.open.iter_mut() {
            o.clear();
        }
        self.air.clear();
        self.air_awake = false;
    }

    fn busy(&self, s: usize) -> bool {
        self.owner[s].is_some() && (self.held[s] || self.sustained[s])
    }

    /// **The string selector.** Where `pitch` is played, given what the
    /// strings are already doing: its own string if it still has one (a
    /// re-pluck); else its first-position string if no held note is on it;
    /// else the next string down that reaches it within `REACH` frets and is
    /// free; else its first-position string anyway, and the note there is
    /// choked (`take` says which). One note per string, as on a guitar —
    /// the reference's manual says the same of its own (seconds and minor
    /// thirds cannot share a string; an automatic selector moves them). A
    /// string a released note is still ringing on is free: the new note takes
    /// it and that note stops. `None` outside the compass.
    pub fn place(&self, pitch: u8) -> Option<Place> {
        let home = Place::home(pitch)?;
        if let Some(s) = self.owner.iter().position(|&o| o == Some(pitch)) {
            return Place::on(s, pitch);
        }
        if !self.busy(home.string) {
            return Some(home);
        }
        for s in home.string + 1..STRINGS {
            if let Some(p) = Place::on(s, pitch) {
                if p.fret <= REACH && !self.busy(s) {
                    return Some(p);
                }
            }
        }
        Some(home)
    }

    /// Give `place`'s string to `pitch`, key down. Returns the other pitch
    /// that was on it, which the frame chokes.
    pub fn take(&mut self, place: Place, pitch: u8) -> Option<u8> {
        self.close_block();
        self.deaf = false;
        let s = place.string;
        let old = self.owner[s].filter(|&p| p != pitch);
        self.owner[s] = Some(pitch);
        self.held[s] = true;
        self.keys |= 1u128 << (pitch & 127);
        self.sustained[s] = false;
        self.answering[s] = false;
        old
    }

    /// `pitch`'s key came up. Without the pedal its string is open again
    /// and may ring in sympathy; under it, the note holds the string.
    pub fn key_up(&mut self, pitch: u8) {
        self.close_block();
        self.keys &= !(1u128 << (pitch & 127));
        for s in 0..STRINGS {
            if self.owner[s] == Some(pitch) {
                self.held[s] = false;
                self.sustained[s] = self.pedal;
                if !self.pedal {
                    self.answering[s] = true;
                }
            }
        }
    }

    /// The sustain pedal: lifting it frees every string a released note was
    /// holding.
    pub fn set_pedal(&mut self, down: bool) {
        self.close_block();
        self.pedal = down;
        if !down {
            for s in 0..STRINGS {
                if self.sustained[s] {
                    self.sustained[s] = false;
                    self.answering[s] = true;
                }
            }
        }
    }

    /// The frame gave `pitch`'s voice to another key at a strike (stolen,
    /// or finished): its string is free and open from this frame.
    pub fn voice_done(&mut self, pitch: u8) {
        self.close_block();
        for s in 0..STRINGS {
            if self.owner[s] == Some(pitch) {
                self.owner[s] = None;
                self.held[s] = false;
                self.sustained[s] = false;
                self.answering[s] = true;
            }
        }
    }

    /// `pitch`'s voice ended `at` frames into the block about to be
    /// rendered: its string is free now, and open from the first control
    /// block that starts at or after that frame, or the next event, on the
    /// board's grid (`BoardCtl`). The frame used to tell the board at the
    /// end of the host's call the voice ended in, and the string closed and
    /// answered from wherever that call ended.
    pub fn voice_ended(&mut self, pitch: u8, at: usize) {
        for s in 0..STRINGS {
            if self.owner[s] == Some(pitch) {
                self.owner[s] = None;
                self.held[s] = false;
                self.sustained[s] = false;
                if !self.answering[s] {
                    self.freeing[s] = Some(at);
                }
            }
        }
    }

    /// The knobs the shared side reads: STRINGS, DECAY and WIDTH design the
    /// open strings (re-designed only when they move, continuing what
    /// rings), WIDTH the air's microphones, BODY scales the air.
    pub fn set_knobs(&mut self, k: &GuitarKnobs) {
        self.body = k.body;
        if f64::from(k.width) != self.air_width {
            let keep = !self.air_width.is_nan();
            self.design_air(f64::from(k.width), keep);
        }
        let key = (k.strings.to_bits(), k.decay.to_bits(), k.width.to_bits());
        if self.designed == Some(key) {
            return;
        }
        let keep = self.designed.is_some();
        self.designed = Some(key);
        let rate = f64::from(self.rate);
        for s in 0..STRINGS {
            let place = Place { string: s, fret: 0 };
            let f0 = f_equal(OPEN[s]);
            let b = inharmonicity(place);
            let big_f = f0 / (1.0 + b).sqrt();
            let len = place.length();
            for k_ in 0..OPEN_PARTIALS {
                let n = k_ + 1;
                let nf = n as f64;
                let f = nf * big_f * (1.0 + b * nf * nf).sqrt();
                let o = &mut self.open[s];
                if f >= OPEN_TOP_HZ.min(0.45 * rate) {
                    break;
                }
                let sigma = OPEN_DECAY * mode_sigma(place, f) / f64::from(k.decay);
                let rate_n = (COUPLE * nf).min(COUPLE_MAX) * f64::from(k.strings);
                let pull = TENSION[s] * nf * PI / len;
                let heard = GAIN * radiation(f) * pull;
                let (wl, wr) = mics(heard, f, f64::from(k.width));
                // dz/dt per unit of drive: the coupling rate over what turns
                // this coordinate back into drive units (the pull, scaled as
                // the voices scale theirs).
                let input = C64::new(rate_n / (GAIN * pull), 0.0);
                let lane = Lane {
                    freq: f,
                    sigma,
                    input,
                    left: wl,
                    right: wr,
                    drive: C64::new(GAIN * pull, 0.0),
                };
                o.set(k_, rate, &lane, keep);
            }
        }
    }

    fn design_air(&mut self, width: f64, keep: bool) {
        self.air_width = width;
        let rate = f64::from(self.rate);
        for (k, &(f, t60, g, phase)) in AIR.iter().enumerate() {
            let sigma = 1000.0_f64.ln() / t60;
            let w = TAU * f;
            // Heard as the air plug's velocity, `Re(i w z)`: a band-pass,
            // nothing at DC, so a slow push of the top never reaches it. The
            // two microphones hear the soundhole nearly alike — the
            // reference's 63-125 Hz band correlates 0.92, not 1 — so the
            // right one a little later (`AIR_MIC_RAD`).
            let wl = C64::new(0.0, w * g);
            let wr = C64::new(0.0, w * g * (1.0 - AIR_MIC_DROP * width)) * C64::polar(1.0, AIR_MIC_RAD * width);
            let lane = Lane {
                freq: f,
                sigma,
                input: C64::polar(AIR_GAIN, phase),
                left: wl,
                right: wr,
                drive: C64::ZERO,
            };
            self.air.set(k, rate, &lane, keep);
        }
    }

    /// Zero the drive buses for the next `m` frames.
    pub fn begin(&mut self, m: usize) {
        let m = m.min(BLOCK);
        for b in self.bus.iter_mut() {
            b[..m].fill(0.0);
        }
    }

    /// String `s`'s drive bus, for a voice on it to render into.
    pub fn bus(&mut self, s: usize, m: usize) -> &mut [f32] {
        &mut self.bus[s.min(STRINGS - 1)][..m.min(BLOCK)]
    }

    /// Run the open strings and the air on this block's buses and ADD them
    /// into the two microphones: what is left of the board's control block,
    /// then the next (see [`BoardCtl`]).
    pub fn render(&mut self, l: &mut [f32], r: &mut [f32], m: usize) {
        let m = m.min(BLOCK).min(l.len()).min(r.len());
        if m == 0 {
            return;
        }
        self.total[..m].fill(0.0);
        for b in self.bus.iter() {
            for (t, x) in self.total[..m].iter_mut().zip(&b[..m]) {
                *t += *x;
            }
        }
        let mut at = 0;
        while at < m {
            if self.ctl.done == self.ctl.len {
                self.begin_block(at);
            }
            let k = (self.ctl.len - self.ctl.done).min(m - at);
            self.run(&mut l[at..at + k], &mut r[at..at + k], at);
            self.ctl.done += k;
            at += k;
            if self.ctl.done == self.ctl.len {
                self.end_block();
            }
        }
        // A string still waiting for its block has its voice behind it now:
        // it answers from the next block's first frame.
        for f in self.freeing.iter_mut().flatten() {
            *f = 0;
        }
    }

    /// A control block starts `at` frames into the block being rendered:
    /// a string whose voice ended by then answers, and the hand's rest and a
    /// taken string's closing move once, for the whole block.
    fn begin_block(&mut self, at: usize) {
        self.ctl = BoardCtl { len: BLOCK, ..BoardCtl::default() };
        for s in 0..STRINGS {
            if self.freeing[s].is_some_and(|f| f <= at) {
                self.freeing[s] = None;
                self.answering[s] = true;
            }
        }
        // **The hand rests only when it is off the keys.** The first draft
        // read the strings' `held`, which a note's voice clears when it has
        // died away: a held B5 (its string silent 2.6 s on) let the hand
        // fall on the six open strings while its key was still down, and the
        // instrument went to sleep under a held key.
        self.resting = !self.pedal && self.keys == 0 && !self.sustained.iter().any(|h| *h);
        for s in 0..STRINGS {
            // A voice is this string: it closes the way a fingertip would.
            // Or the hand is on it.
            let g = if !self.answering[s] {
                self.close_g
            } else if self.resting {
                self.rest_g
            } else {
                1.0
            };
            if !self.awake[s] || g == 1.0 {
                continue;
            }
            let o = &mut self.open[s];
            for gi in 0..OPEN_GROUPS {
                for j in 0..LANES {
                    o.y[gi][j] *= g;
                    o.v[gi][j] *= g;
                }
            }
        }
    }

    /// The next `l.len()` frames of the control block, `at` frames into the
    /// block being rendered: the open strings, then the air.
    fn run(&mut self, l: &mut [f32], r: &mut [f32], at: usize) {
        let k = l.len();
        let driven = self.total[at..at + k].iter().any(|v| *v != 0.0);
        self.ctl.driven |= driven;
        self.sym_d[..k].fill(0.0);
        for s in 0..STRINGS {
            if !self.answering[s] {
                // Closing (`begin_block`): it rings out, and hears nothing.
                if self.awake[s] {
                    self.x[..k].fill(0.0);
                    self.open[s].render(&self.x[..k], l, r, &mut self.sym_d[..k]);
                }
                continue;
            }
            if (!driven || self.deaf) && !self.awake[s] {
                continue;
            }
            // Every string but this one drives it: a string does not ring in
            // sympathy with itself.
            for i in 0..k {
                self.x[i] = self.total[at + i] - self.bus[s][at + i];
            }
            self.open[s].render(&self.x[..k], l, r, &mut self.sym_d[..k]);
            self.awake[s] = true;
        }
        if self.body > 0.0 {
            // The air hears what the body radiates, not the bare bridge
            // force (see `AIR_GAIN`).
            for i in 0..k {
                self.x[i] = 0.5 * (l[i] + r[i]);
            }
            let mut al = [0.0_f32; BLOCK];
            let mut ar = [0.0_f32; BLOCK];
            let mut ad = [0.0_f32; BLOCK];
            self.air.render(&self.x[..k], &mut al[..k], &mut ar[..k], &mut ad[..k]);
            for i in 0..k {
                l[i] += al[i] * self.body;
                r[i] += ar[i] * self.body;
            }
            self.ctl.air = true;
            self.air_awake |= self.x[..k].iter().any(|v| *v != 0.0);
        }
    }

    /// A control block ends, full or cut short by an event. An open string
    /// whose output has stayed under `FLOOR_RMS` for `QUIET_SECS` is put to
    /// sleep and cleared, a taken one once it has closed and an answering one
    /// once no bus drives it; and the air with the board, once no bus drives
    /// it and no string is awake, so that the frame stops rendering it on
    /// nothing at all (judged at the end of the host's call, the air left
    /// whatever that call had made of its last 1e-9 of state, and the next
    /// note started on it: -136 dB re peak off the 128-frame take across a
    /// gap). With BODY at zero it did not run and is out of the sound:
    /// cleared, where it held its last state and kept the instrument awake
    /// while the knob stayed down. What ran and rings on is flushed.
    fn end_block(&mut self) {
        let BoardCtl { done, driven, air, .. } = self.ctl;
        self.ctl = BoardCtl::default();
        // The floor's energy over the block, both microphones. **A reading
        // that is not a number is no sound to stay awake for**: it is under
        // the floor, as the state test's largest magnitude was (`f32::max`
        // passes the number over a NaN), so a string or the air a voice's NaN
        // once reached is cleared once nothing drives it. Read as over it, two
        // silent presses of one key (`GuitarVoice::let_go`'s NaN plateau) left
        // the guitar non-finite and awake for good
        // (`a_string_that_is_not_a_number_falls_asleep`).
        let floor = FLOOR_RMS * FLOOR_RMS * (2 * done) as f32;
        let under = |acc: f32| acc < floor || !acc.is_finite();
        for s in 0..STRINGS {
            if !self.awake[s] {
                continue;
            }
            let o = &mut self.open[s];
            self.quiet[s] = if under(o.acc) { self.quiet[s] + done } else { 0 };
            o.acc = 0.0;
            if (!self.answering[s] || !driven) && self.quiet[s] >= self.quiet_for {
                o.clear();
                self.awake[s] = false;
                self.quiet[s] = 0;
            } else {
                o.flush();
            }
        }
        // The air is heard through BODY (`run`).
        self.air_quiet = if under(self.air.acc * self.body * self.body) { self.air_quiet + done } else { 0 };
        self.air.acc = 0.0;
        if !air || (!driven && !self.awake.iter().any(|a| *a) && self.air_quiet >= self.quiet_for) {
            self.air.clear();
            self.air_awake = false;
            self.air_quiet = 0;
        } else {
            self.air.flush();
        }
    }

    /// An event lands inside a control block: the block ends where it
    /// stands, the next starts at the event, and a string whose voice has
    /// ended answers from here.
    fn close_block(&mut self) {
        if self.ctl.done > 0 && self.ctl.done < self.ctl.len {
            self.end_block();
        }
        self.hand_back();
    }

    /// Every string waiting for its block to answer answers now.
    fn hand_back(&mut self) {
        for s in 0..STRINGS {
            if self.freeing[s].take().is_some() {
                self.answering[s] = true;
            }
        }
    }

    /// Anything still ringing on the shared side.
    pub fn active(&self) -> bool {
        self.awake.iter().any(|a| *a) || self.air_awake
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{Keys, Patch};

    const RATE: f32 = 48_000.0;

    fn knobs() -> GuitarKnobs {
        GuitarKnobs::from_params(&DEFAULTS)
    }

    fn knobs_with(i: usize, v: f32) -> GuitarKnobs {
        let mut p = DEFAULTS;
        p[i] = v;
        GuitarKnobs::from_params(&p)
    }

    /// One voice, plucked once, rendered in blocks: (left, right, drive, voice).
    /// No fretboard: the string alone, as the open strings switched off hear it.
    fn play(pitch: u8, vel: f32, secs: f32, k: &GuitarKnobs, release_at: Option<f32>) -> (Vec<f32>, Vec<f32>, Vec<f32>, GuitarVoice) {
        let mut v = GuitarVoice::new(RATE);
        v.strike(Place::home(pitch).unwrap(), pitch, vel, k);
        let n = (secs * RATE) as usize / BLOCK * BLOCK;
        let (mut l, mut r, mut d) = (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
        let mut at = 0;
        while at < n {
            if release_at.is_some_and(|t| at == (t * RATE) as usize / BLOCK * BLOCK) {
                v.release(false);
            }
            v.render(&mut l[at..at + BLOCK], &mut r[at..at + BLOCK], &mut d[at..at + BLOCK]);
            at += BLOCK;
        }
        (l, r, d, v)
    }

    /// The whole instrument: (seconds, event) through `Keys`, interleaved out.
    fn perform(events: &[(f32, Ev)], secs: f32, tweak: &[(usize, f32)]) -> (Vec<f32>, Vec<f32>, Keys) {
        let mut k = Keys::new(RATE);
        k.set_patch(Patch::Guitar);
        for &(i, v) in tweak {
            k.set_param(i, v);
        }
        let n = (secs * RATE) as usize / BLOCK * BLOCK;
        let mut out = vec![0.0_f32; 2 * BLOCK];
        let (mut l, mut r) = (Vec::with_capacity(n), Vec::with_capacity(n));
        let mut next = 0;
        let mut at = 0;
        while at < n {
            while next < events.len() && ((events[next].0 * RATE) as usize) < at + BLOCK {
                match events[next].1 {
                    Ev::On(p, v) => k.note_on(p, v),
                    Ev::Off(p) => k.note_off(p),
                    Ev::Pedal(d) => k.set_pedal(d),
                    Ev::Sostenuto(d) => k.set_pedals([if k.pedal { 127 } else { 0 }, if d { 127 } else { 0 }, 0]),
                }
                next += 1;
            }
            out.fill(0.0);
            k.render(&mut out, BLOCK, 2);
            for &[a, b] in out.as_chunks::<2>().0 {
                l.push(a);
                r.push(b);
            }
            at += BLOCK;
        }
        (l, r, k)
    }

    #[derive(Clone, Copy)]
    enum Ev {
        On(u8, f32),
        Off(u8),
        Pedal(bool),
        /// The sostenuto (CC 66) down or up, the sustain as it is.
        Sostenuto(bool),
    }

    /// Hann-windowed magnitude at f.
    fn tone(x: &[f32], f: f64) -> f64 {
        let n = x.len();
        let (mut re, mut im, mut ws) = (0.0, 0.0, 0.0);
        let w = TAU * f / f64::from(RATE);
        for (i, &s) in x.iter().enumerate() {
            let win = 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos();
            re += f64::from(s) * win * (w * i as f64).cos();
            im -= f64::from(s) * win * (w * i as f64).sin();
            ws += win;
        }
        2.0 * (re * re + im * im).sqrt() / ws
    }

    /// The strongest Hann magnitude within 0.3 % of f (searched, never pinned)
    /// and where it is.
    fn peak_near(x: &[f32], f: f64) -> (f64, f64) {
        (0..25)
            .map(|i| {
                let g = f * (0.997 + 0.00025 * f64::from(i));
                (tone(x, g), g)
            })
            .fold((0.0, f), |a, b| if b.0 > a.0 { b } else { a })
    }

    /// Level of a partial over a window, both channels' power, dB.
    fn partial_db(l: &[f32], r: &[f32], f: f64, from: f32, len: f32) -> f64 {
        let span = (from * RATE) as usize..((from + len) * RATE) as usize;
        let (a, b) = (peak_near(&l[span.clone()], f).0, peak_near(&r[span], f).0);
        10.0 * ((a * a + b * b) * 0.5).max(1e-30).log10()
    }

    #[test]
    fn first_position_puts_every_note_on_its_string() {
        // (pitch, string number 1-6, fret)
        for &(p, s, f) in &[
            (40u8, 6usize, 0u8),
            (43, 6, 3),
            (44, 6, 4),
            (45, 5, 0),
            (47, 5, 2),
            (50, 4, 0),
            (52, 4, 2),
            (55, 3, 0),
            (57, 3, 2),
            (58, 3, 3),
            (59, 2, 0),
            (62, 2, 3),
            (63, 2, 4),
            (64, 1, 0),
            (69, 1, 5),
            (76, 1, 12),
            (83, 1, 19),
        ] {
            let place = Place::home(p).unwrap();
            assert_eq!((place.string + 1, place.fret), (s, f), "pitch {p}");
        }
        assert_eq!(Place::home(LOWEST - 1), None);
        assert_eq!(Place::home(HIGHEST + 1), None);
    }

    /// **One note per string, and a second moves rather than chokes.** E3
    /// held on the D string (fret 2): F3, whose own place is that string's
    /// third fret, goes to the A string at fret 8. C5 held (top string, fret
    /// 8): E5 has nowhere within `REACH` frets (the B string would need its
    /// 17th), so it takes the top string and C5 is the note to choke. A
    /// released note's string is free; under the pedal it is not; a re-pluck
    /// finds its own string; a silent voice gives its string back.
    #[test]
    fn the_selector_keeps_one_note_per_string() {
        let mut fb = Fretboard::new(RATE);
        let e3 = fb.place(52).unwrap();
        assert_eq!((e3.string, e3.fret), (3, 2));
        assert_eq!(fb.take(e3, 52), None);
        let f3 = fb.place(53).unwrap();
        assert_eq!((f3.string, f3.fret), (4, 8), "F3 beside a held E3");
        assert_eq!(fb.take(f3, 53), None);
        assert_eq!(fb.place(52), Some(e3), "a re-pluck finds its own string");
        assert_eq!(fb.place(53), Some(f3), "wherever that string is");
        // A third note that wants the A string and finds it busy: G#2's own
        // place is the low E string (fret 4), which is free.
        assert_eq!(fb.place(44).map(|p| (p.string, p.fret)), Some((5, 4)));
        // Nowhere to move: the older note is choked.
        let c5 = fb.place(72).unwrap();
        fb.take(c5, 72);
        let e5 = fb.place(76).unwrap();
        assert_eq!((e5.string, e5.fret), (0, 12));
        assert_eq!(fb.take(e5, 76), Some(72), "E5 takes the top string from C5");
        // Released, E3's string is free again: F3 (its old voice gone) goes
        // home, and the still-ringing E3 is the note to stop.
        fb.key_up(52);
        fb.voice_done(53);
        let home = fb.place(53).unwrap();
        assert_eq!((home.string, home.fret), (3, 3), "F3 home once E3 is released");
        assert_eq!(fb.take(home, 53), Some(52));
        // Under the pedal a released note keeps its string.
        let mut fb = Fretboard::new(RATE);
        fb.set_pedal(true);
        let e3 = fb.place(52).unwrap();
        fb.take(e3, 52);
        fb.key_up(52);
        assert_eq!(fb.place(53).map(|p| p.string), Some(4), "the pedal holds E3's string");
        fb.set_pedal(false);
        assert_eq!(fb.place(53).map(|p| p.string), Some(3), "lifting the pedal frees it");
    }

    /// **E3 and F3 held together through the frame**: two voices, on the D
    /// and A strings, both sounding a second later, each fundamental within
    /// a few decibels of the other — not one choking the other. Played one
    /// after the other with E3 let go, F3 takes the D string and E3 stops.
    #[test]
    fn a_held_second_sounds_on_two_strings() {
        let (l, r, k) = perform(&[(0.0, Ev::On(52, 0.6)), (0.05, Ev::On(53, 0.6))], 1.2, &[]);
        let mut strings: Vec<usize> = k
            .notes
            .iter()
            .filter(|n| n.pitch.is_some() && !n.mech.finished())
            .map(|n| n.mech.guitar.string())
            .collect();
        strings.sort_unstable();
        assert_eq!(strings, vec![3, 4], "E3 on the D string, F3 on the A string");
        let e3 = partial_db(&l, &r, f_equal(52), 0.6, 0.5);
        let f3 = partial_db(&l, &r, f_equal(53), 0.6, 0.5);
        println!("E3 + F3 held: E3 n1 {e3:.1} dB, F3 n1 {f3:.1} dB");
        assert!((e3 - f3).abs() < 8.0, "one of the two was choked: {e3:.1} / {f3:.1}");
        // In turn, E3 released first: F3 takes its string and E3 stops.
        let (l2, r2, _) = perform(&[(0.0, Ev::On(52, 0.6)), (0.5, Ev::Off(52)), (0.6, Ev::On(53, 0.6))], 1.4, &[]);
        let e3_after = partial_db(&l2, &r2, f_equal(52), 1.0, 0.3);
        let e3_before = partial_db(&l2, &r2, f_equal(52), 0.2, 0.3);
        assert!(e3_after < e3_before - 30.0, "E3 rang on under F3: {e3_before:.1} -> {e3_after:.1} dB");
    }

    /// **The sostenuto holds a caught note's string as the sustain does.**
    /// E4 caught by the sostenuto and let go, then F4: F4 moves to the B
    /// string and E4 rings on, its fundamental over 0.7-1.3 s within a
    /// decibel of the same played under the sustain pedal. The fretboard was
    /// told the key was up, and F4 took E4's string and choked it (-66.9
    /// against -37.6 dB; review, F4 and alloc 4). The sustain lifted under
    /// the sostenuto frees nothing the sostenuto holds (marking the caught
    /// string as the sustain's, as one proposed fix did, let that lift free
    /// it: E4 at -64.8 dB under F4); the sostenuto's own lift lets it go, E4
    /// damped and F4 home on its string. Measured: printed.
    #[test]
    fn the_sostenuto_holds_a_caught_string_as_the_sustain_does() {
        let v = 90.0 / 127.0;
        let e4 = |evs: &[(f32, Ev)]| {
            let (l, r, k) = perform(evs, 1.4, &[]);
            let on = |p: u8| k.notes.iter().find(|n| n.pitch == Some(p) && !n.mech.finished()).map(|n| n.mech.guitar.string());
            (partial_db(&l, &r, f_equal(64), 0.7, 0.6), on(64), on(65))
        };
        let caught = [(0.05, Ev::On(64, v)), (0.2, Ev::Sostenuto(true)), (0.3, Ev::Off(64))];
        let sustained = e4(&[(0.05, Ev::On(64, v)), (0.2, Ev::Pedal(true)), (0.3, Ev::Off(64)), (0.6, Ev::On(65, v))]);
        let mut evs = caught.to_vec();
        evs.push((0.6, Ev::On(65, v)));
        let held = e4(&evs);
        let mut evs = vec![(0.0, Ev::Pedal(true))];
        evs.extend(caught);
        evs.extend([(0.5, Ev::Pedal(false)), (0.6, Ev::On(65, v))]);
        let lifted = e4(&evs);
        let mut evs = caught.to_vec();
        evs.extend([(0.5, Ev::Sostenuto(false)), (0.6, Ev::On(65, v))]);
        let released = e4(&evs);
        println!(
            "E4 n1 0.7-1.3 s and (E4, F4) strings: sustained {:.1} dB {:?}, caught {:.1} dB {:?}, the sustain lifted under the catch {:.1} dB {:?}, the sostenuto lifted {:.1} dB {:?}",
            sustained.0,
            (sustained.1, sustained.2),
            held.0,
            (held.1, held.2),
            lifted.0,
            (lifted.1, lifted.2),
            released.0,
            (released.1, released.2)
        );
        for (what, run) in [("caught", held), ("the sustain lifted under the catch", lifted)] {
            assert_eq!((run.1, run.2), (Some(0), Some(1)), "{what}: E4 and F4 are not on the top two strings");
            assert!((run.0 - sustained.0).abs() <= 1.0, "{what}: E4 at {:.1} dB, sustained {:.1}", run.0, sustained.0);
        }
        // What is left near E4 once F4 has choked it is the open strings'
        // (the low E's fourth partial and the A's third sit within 0.1 %).
        assert_eq!((released.1, released.2), (None, Some(0)), "the sostenuto came up and F4 did not take E4's string");
        assert!(released.0 < sustained.0 - 20.0, "the sostenuto came up and E4 rang on ({:.1} dB)", released.0);
    }

    /// **A silent press on a ringing string stays finite.** C4 struck at
    /// 0.8, or not at all, then pressed again with no velocity, with a NaN
    /// one or with 1e-9, twice, 100 and 150 ms on, and once struck again at
    /// 0.8 after a silent press: every sample of the next second is finite.
    /// The glide scaled by the inverse velocity, infinite at a silent press,
    /// and the landing after it read a NaN ratio: the take was non-finite
    /// from 12 ms after the second press on, for good (verifier; 0d20e3d1
    /// the same).
    #[test]
    fn a_silent_press_on_a_ringing_string_stays_finite() {
        for (first, then, last) in [(0.8_f32, 0.0_f32, 0.0_f32), (0.8, f32::NAN, 0.0), (0.0, 0.0, 0.0), (0.8, 1.0e-9, 1.0e-9), (0.8, 0.0, 0.8)] {
            let mut k = Keys::new(RATE);
            k.set_patch(Patch::Guitar);
            let mut out = vec![0.0_f32; 2 * BLOCK];
            let mut bad = None;
            for b in 0..(1.2 * RATE) as usize / BLOCK {
                match b {
                    0 => k.note_on(60, first),
                    37 => k.note_on(60, then),
                    56 => k.note_on(60, last),
                    _ => {}
                }
                out.fill(0.0);
                k.render(&mut out, BLOCK, 2);
                if bad.is_none() && out.iter().any(|v| !v.is_finite()) {
                    bad = Some(b);
                }
            }
            assert!(bad.is_none(), "C4 at {first}, then {then}, then {last}: non-finite from block {bad:?}");
        }
    }

    /// **All notes off chokes the strings; it does not lift a finger.** E3
    /// at MIDI 120 with all notes off one block after the strike, the finger
    /// still on the string: the pluck never lands, the next second's peak
    /// under -50 dBFS, where the release's press waited for the pluck and let
    /// it land at -20.2 dBFS (review, alloc 3). And E4 ringing, stopped by it
    /// at 0.4 s: nothing of it is left 0.1-0.3 s on (under -90 dB), where a
    /// note-off leaves the release's remnant ringing (-64.6 dB; the panic
    /// left it too). Measured: printed.
    #[test]
    fn all_notes_off_chokes_the_strings() {
        let mut k = Keys::new(RATE);
        k.set_patch(Patch::Guitar);
        k.note_on(52, 120.0 / 127.0);
        let mut out = vec![0.0_f32; 2 * BLOCK];
        k.render(&mut out, BLOCK, 2);
        k.all_notes_off();
        let mut pluck = 0.0_f32;
        for _ in 0..(RATE as usize / BLOCK) {
            out.fill(0.0);
            k.render(&mut out, BLOCK, 2);
            pluck = out.iter().fold(pluck, |m, s| m.max(s.abs()));
        }
        let pluck = 20.0 * pluck.max(1e-15).log10();
        let stopped = |panic: bool| {
            let mut k = Keys::new(RATE);
            k.set_patch(Patch::Guitar);
            k.note_on(64, 110.0 / 127.0);
            let mut out = vec![0.0_f32; 2 * BLOCK];
            let (mut l, mut r) = (Vec::new(), Vec::new());
            for b in 0..(0.8 * RATE) as usize / BLOCK {
                if b == (0.4 * RATE) as usize / BLOCK {
                    if panic {
                        k.all_notes_off();
                    } else {
                        k.note_off(64);
                    }
                }
                out.fill(0.0);
                k.render(&mut out, BLOCK, 2);
                for &[a, c] in out.as_chunks::<2>().0 {
                    l.push(a);
                    r.push(c);
                }
            }
            partial_db(&l, &r, f_equal(64), 0.5, 0.2)
        };
        let (released, choked) = (stopped(false), stopped(true));
        println!("E3 MIDI 120 stopped in contact: peak {pluck:.1} dBFS; E4 0.1-0.3 s after: note-off {released:.1} dB, all notes off {choked:.1} dB");
        assert!(pluck < -50.0, "all notes off let a pluck land at {pluck:.1} dBFS");
        assert!(choked < -90.0, "all notes off left E4 ringing at {choked:.1} dB");
    }

    /// **And the fretboard still knows who is on which string.** A chord of
    /// three under the sustain, caught by the sostenuto, then all notes off:
    /// every voice still sounding names its own string, and no key, no pedal
    /// holds any; the sostenuto's lift after it holds nothing either, and
    /// once the choked voices are done every string is free. All notes off
    /// forgot every owner while their voices still rang (review, alloc 3).
    #[test]
    fn all_notes_off_keeps_who_is_on_which_string() {
        let mut k = Keys::new(RATE);
        k.set_patch(Patch::Guitar);
        k.set_pedals([127, 0, 0]);
        for p in [52, 55, 59] {
            k.note_on(p, 90.0 / 127.0);
        }
        k.set_pedals([127, 127, 0]);
        let mut out = vec![0.0_f32; 2 * BLOCK];
        for _ in 0..40 {
            out.fill(0.0);
            k.render(&mut out, BLOCK, 2);
        }
        k.all_notes_off();
        k.set_pedals([127, 0, 0]);
        let live: Vec<(u8, usize)> = k.notes.iter().filter(|n| !n.mech.finished()).filter_map(|n| n.pitch.map(|p| (p, n.mech.guitar.string()))).collect();
        assert_eq!(live.len(), 3, "the choked voices were silenced at once");
        for (p, s) in live {
            assert_eq!(k.fretboard.owner[s], Some(p), "string {s} no longer knows its choked {p}");
        }
        assert!(k.fretboard.keys == 0 && !k.fretboard.held.iter().any(|h| *h) && !k.fretboard.sustained.iter().any(|h| *h), "a string is held after all notes off");
        for _ in 0..(RATE as usize / BLOCK) {
            out.fill(0.0);
            k.render(&mut out, BLOCK, 2);
        }
        assert!(k.fretboard.owner.iter().all(Option::is_none), "a string is still owned after its voice was done: {:?}", k.fretboard.owner);
    }

    /// **A voice that ends at an event gives its string back.** B5 at MIDI
    /// 6, held, and its key let up, all notes off, or (let up under the
    /// pedal) the pedal lifted, a few frames before it would have died: the
    /// event closes the voice's control block where it falls and the cull
    /// there empties its bank, so the voice is done before the frame renders
    /// it again, and the end `render_blocks` hands back from was never seen.
    /// Its string stayed owned by nobody until another key took it, and the
    /// next B5 went back on it wherever its home was (verifier: the voice
    /// ended at the event on the last 77 of its 128 frames, with each of the
    /// three, and every time its string stayed owned).
    #[test]
    fn a_voice_that_ends_at_an_event_gives_its_string_back() {
        let pitch = 83;
        let start = |pedal: bool| {
            let mut k = Keys::new(RATE);
            k.set_patch(Patch::Guitar);
            if pedal {
                k.set_pedals([127, 0, 0]);
            }
            k.note_on(pitch, 0.05);
            k
        };
        let up = (0.05 * RATE) as usize;
        for kind in ["its key let up", "all notes off", "the pedal lifted"] {
            let pedal = kind == "the pedal lifted";
            // Where the voice dies untouched: its key held, or let up under
            // the pedal.
            let end = {
                let mut k = start(pedal);
                let mut out = [0.0_f32; 2];
                let mut f = 0;
                while k.notes.iter().any(|n| n.pitch == Some(pitch)) && f < (5.0 * RATE) as usize {
                    if pedal && f == up {
                        k.note_off(pitch);
                    }
                    k.render(&mut out, 1, 2);
                    f += 1;
                }
                f
            };
            let mut ended = 0;
            for back in [1, 2, 4, 8, 16, 32] {
                let mut k = start(pedal);
                let mut out = vec![0.0_f32; 2 * BLOCK];
                let mut f = 0;
                while f < end - back {
                    if pedal && f == up {
                        k.note_off(pitch);
                    }
                    let to = if pedal && f < up { up } else { end - back };
                    let n = (to - f).min(BLOCK);
                    k.render(&mut out[..2 * n], n, 2);
                    f += n;
                }
                match kind {
                    "its key let up" => k.note_off(pitch),
                    "all notes off" => k.all_notes_off(),
                    _ => k.set_pedals([0, 0, 0]),
                }
                if !k.notes.iter().any(|n| n.pitch == Some(pitch) && n.mech.finished()) {
                    continue;
                }
                ended += 1;
                k.render(&mut out, BLOCK, 2);
                assert!(k.fretboard.owner.iter().all(|o| *o != Some(pitch)), "{kind} {back} frames before B5 died: its voice ended there and its string is still owned: {:?}", k.fretboard.owner);
            }
            assert!(ended > 0, "{kind}: B5 never ended at the event, and the handback went untested");
        }
    }

    /// **After all notes off under the pedal the guitar falls asleep as the
    /// other patches do, and clears nothing over the house's floor.** The
    /// pedalled six-note chord (E2 B2 E3 G#3 B3 E4 at MIDI 100), and three
    /// of it (E3 G#3 B3), all notes off at 1.2 s with the pedal held: the
    /// guitar is asleep within a second, as the grands are (measured 0.507
    /// and 0.485 s; Piano 1 0.54, Piano 2 0.90), and the last `QUIET_SECS`
    /// before it sleeps are under `FLOOR_RMS` (-150.2 and -152.9 dBFS RMS).
    /// At 0d20e3d1 the open strings, answering again after the hush's clear,
    /// took up what the choked voices played into them for their last tenth
    /// of a second and rang it out under the pedal (-104 and -102 dBFS RMS
    /// 0.5 to 1 s on, -124 and -127 at 4 to 7 s): awake 7.141 and 7.813 s,
    /// and put to sleep from -129.9 and -137.8 dBFS RMS (`Fretboard::hush`,
    /// `FLOOR_RMS`).
    #[test]
    fn all_notes_off_under_the_pedal_sleeps_under_the_floor() {
        for keys in [&[40u8, 47, 52, 56, 59, 64][..], &[52u8, 56, 59][..]] {
            let mut k = Keys::new(RATE);
            k.set_patch(Patch::Guitar);
            k.set_pedals([127, 0, 0]);
            let stop = (1.2 * RATE) as usize / BLOCK * BLOCK;
            let end = stop + (12.0 * RATE) as usize;
            let mut out = vec![0.0_f32; 2 * BLOCK];
            let mut x: Vec<f32> = Vec::new();
            let (mut at, mut struck, mut asleep) = (0, 0, None);
            while at < end && asleep.is_none() {
                while struck < keys.len() && ((0.01 + 0.012 * struck as f32) * RATE) as usize / BLOCK * BLOCK <= at {
                    k.note_on(keys[struck], 100.0 / 127.0);
                    struck += 1;
                }
                if at == stop {
                    k.all_notes_off();
                }
                out.fill(0.0);
                k.render(&mut out, BLOCK, 2);
                x.extend_from_slice(&out);
                at += BLOCK;
                if at > stop && !k.active() {
                    asleep = Some(at);
                }
            }
            // What the last QUIET_SECS before it slept played, both channels.
            let a = asleep.unwrap_or(at);
            let n = (QUIET_SECS * f64::from(RATE)) as usize;
            let e: f64 = x[2 * (a - n)..2 * a].iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / (2 * n) as f64;
            let last = 10.0 * e.max(1e-40).log10();
            let secs = (a - stop) as f32 / RATE;
            println!("{keys:?} under the pedal, all notes off: asleep {secs:.3} s on{}, the last {:.0} ms before it {last:+.1} dBFS RMS", if asleep.is_none() { " (still awake)" } else { "" }, QUIET_SECS * 1e3);
            assert!(asleep.is_some() && secs <= 1.0, "{keys:?}: still awake {secs:.3} s after all notes off");
            assert!(last <= 20.0 * f64::from(FLOOR_RMS).log10(), "{keys:?}: put to sleep from {last:+.1} dBFS RMS");
        }
    }

    /// The loudest of `shapes`, every key of each struck at MIDI 127 in one
    /// instant, through the frame: (peak, which).
    fn loudest(shapes: &[&[u8]]) -> (f32, usize) {
        let mut best = (0.0_f32, 0);
        for (i, keys) in shapes.iter().enumerate() {
            let evs: Vec<(f32, Ev)> = keys.iter().map(|&p| (0.0, Ev::On(p, 1.0))).collect();
            let (l, r, _) = perform(&evs, 0.6, &[]);
            let pk = l.iter().chain(&r).fold(0.0_f32, |m, s| m.max(s.abs()));
            if pk > best.0 {
                best = (pk, i);
            }
        }
        best
    }

    /// **A note that loses its string before its finger lets go never
    /// sounds** (`choke`, `abandon`). One voice choked in its contact phase
    /// puts out nothing louder than the skin's noise before the choke and is
    /// finished within the block. Through the frame, chords of more keys than
    /// strings, all at MIDI 127 in one instant, peak no higher than the
    /// loudest six-string shape, because only the last note on each string
    /// plucks — the lane verifier's 10-key chord minus its six survivors is
    /// now exactly zero (its `sim44` peaked at +6.7 dBFS, `sim20` at +0.1;
    /// tbench after the fix: -4.2 and -4.9, a six-string G major -3.5).
    #[test]
    fn a_note_choked_before_it_lets_go_never_plucks() {
        let mut v = GuitarVoice::new(RATE);
        v.strike(Place::home(64).unwrap(), 64, 1.0, &knobs());
        let (mut l, mut r, mut d) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK], [0.0_f32; BLOCK]);
        v.render(&mut l, &mut r, &mut d);
        assert!(v.contact.is_some(), "the finger let go inside the first block");
        let before = l.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        v.choke();
        let mut after = 0.0_f32;
        let mut blocks = 0;
        while !v.finished() && blocks < 400 {
            l.fill(0.0);
            r.fill(0.0);
            d.fill(0.0);
            v.render(&mut l, &mut r, &mut d);
            after = l.iter().chain(&r).fold(after, |m, s| m.max(s.abs()));
            blocks += 1;
        }
        println!("choked in its contact: noise peak {before:.2e} before, {after:.2e} after, finished in {blocks} block(s)");
        assert_eq!(blocks, 1, "the choked voice took {blocks} blocks to finish");
        assert!(after <= before, "the choked note sounded: {after:.2e} after against {before:.2e} before");
        let six: [&[u8]; 3] = [&[40, 47, 52, 56, 59, 64], &[43, 47, 50, 55, 59, 67], &[40, 47, 52, 56, 59, 83]];
        let (six_pk, _) = loudest(&six);
        let ten: &[u8] = &[40, 47, 52, 56, 59, 64, 68, 71, 76, 83];
        let twenty: &[u8] = &[40, 42, 44, 46, 48, 51, 53, 55, 57, 59, 62, 64, 66, 68, 70, 73, 75, 77, 79, 81];
        let all: Vec<u8> = (LOWEST..=HIGHEST).collect();
        for keys in [ten, twenty, &all[..]] {
            let (pk, _) = loudest(&[keys]);
            println!("{} keys at MIDI 127 at once: {:.1} dBFS (six strings: {:.1})", keys.len(), 20.0 * pk.log10(), 20.0 * six_pk.log10());
            assert!(pk <= six_pk * 1.012, "{} keys peaked at {:.1} dBFS over the six-string {:.1}", keys.len(), 20.0 * pk.log10(), 20.0 * six_pk.log10());
        }
        // Exactly the six survivors: the ten-key chord is its last note on
        // each string and nothing else.
        let evs = |keys: &[u8]| -> Vec<(f32, Ev)> { keys.iter().map(|&p| (0.0, Ev::On(p, 1.0))).collect() };
        let (a, _, _) = perform(&evs(ten), 0.4, &[]);
        let (b, _, _) = perform(&evs(&[40, 47, 52, 56, 59, 83]), 0.4, &[]);
        assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()), "the four choked notes left something");
    }

    /// **A re-pluck after a knob move is still a re-pluck** (`Design`,
    /// `revoice`). A held six-string chord re-struck softly after DECAY and
    /// WIDTH have moved lands on its six voices — the frame buries none — and
    /// what comes out around the re-strike carries no more high-frequency
    /// energy than with the knobs still (the first draft buried six into
    /// three corpses: above 6 kHz in the first 3 ms -57 dBFS against -81; the
    /// lane's vbench after the fix, Width automated: -92.9 against -92.9).
    /// And the re-plucked strings ARE re-voiced: at WIDTH zero the two
    /// microphones agree exactly once the onset has settled, and at DECAY's
    /// top the chord rings on louder.
    #[test]
    fn a_restrike_after_a_knob_move_lands_without_a_cut() {
        let chord = [40u8, 47, 52, 56, 59, 64];
        let run = |moved: bool| -> (Vec<f32>, Vec<f32>, usize) {
            let mut k = Keys::new(RATE);
            k.set_patch(Patch::Guitar);
            let n = (2.6 * RATE) as usize / BLOCK * BLOCK;
            let at = (1.5 * RATE) as usize / BLOCK * BLOCK;
            let mut out = vec![0.0_f32; 2 * BLOCK];
            let (mut l, mut r) = (Vec::with_capacity(n), Vec::with_capacity(n));
            let mut buried = 0;
            for p in chord {
                k.note_on(p, 100.0 / 127.0);
            }
            let mut f = 0;
            while f < n {
                if f == at {
                    if moved {
                        k.set_param(1, 0.0);
                        k.set_param(2, 1.0);
                    }
                    for p in chord {
                        k.note_on(p, 30.0 / 127.0);
                    }
                    buried = k.corpses.iter().filter(|c| c.left > 0).count();
                }
                out.fill(0.0);
                k.render(&mut out, BLOCK, 2);
                for &[a, b] in out.as_chunks::<2>().0 {
                    l.push(a);
                    r.push(b);
                }
                f += BLOCK;
            }
            (l, r, buried)
        };
        let (l0, _, b0) = run(false);
        let (l1, r1, b1) = run(true);
        assert_eq!((b0, b1), (0, 0), "a re-strike was buried");
        // Second differences: +12 dB an octave, so a cut or a step shows.
        let hf = |x: &[f32], a: f32, b: f32| -> f64 {
            let (i, j) = ((a * RATE) as usize, (b * RATE) as usize);
            let e = (i..j).map(|n| f64::from(x[n] - 2.0 * x[n - 1] + x[n - 2]).powi(2)).sum::<f64>();
            10.0 * (e / (j - i) as f64).log10()
        };
        let t = (1.5 * RATE) as usize / BLOCK * BLOCK;
        let t = t as f32 / RATE;
        // Before the finger lets go (the verifier's 3 ms) nothing may differ;
        // across the release the re-voiced image and the longer decay are
        // allowed their measured +1.0 dB (a buried chord's cut was +24).
        for (a, b, room) in [(0.0, 0.003, 0.5), (0.003, 0.02, 2.0)] {
            let (still, moved) = (hf(&l0, t + a, t + b), hf(&l1, t + a, t + b));
            println!("{:.0}-{:.0} ms after the re-strike: second-difference energy {still:.1} dB knobs still, {moved:.1} moved", a * 1e3, b * 1e3);
            assert!(moved <= still + room, "{:.0}-{:.0} ms: {moved:.1} dB against {still:.1}", a * 1e3, b * 1e3);
        }
        let i = ((t + 0.1) * RATE) as usize;
        let side = (i..i + (0.3 * RATE) as usize).map(|n| (l1[n] - r1[n]).abs()).fold(0.0_f32, f32::max);
        assert_eq!(side, 0.0, "WIDTH zero did not reach the re-plucked strings");
        let rms = |x: &[f32], a: f32, b: f32| -> f64 {
            let (i, j) = ((a * RATE) as usize, (b * RATE) as usize);
            10.0 * (x[i..j].iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / (j - i) as f64).log10()
        };
        let longer = rms(&l1, t + 0.8, t + 1.05) - rms(&l0, t + 0.8, t + 1.05);
        println!("DECAY at its top: the re-plucked chord 0.8-1.05 s on is {longer:+.1} dB");
        assert!(longer > 3.0, "DECAY did not reach the re-plucked strings ({longer:+.1} dB)");
    }

    /// **The top of POSITION leaves no hole up the neck** (`beta_max`): the
    /// 300 ms level of every key from the bass E open to the 19th fret at
    /// POSITION's top within -3..+4 dB of the default (the lane's chromatic
    /// scan, tbench: -2.4..+3.4; the first draft's top four frets were 20 dB
    /// under, plucked on the fret).
    #[test]
    fn the_top_of_position_leaves_no_hole_up_the_neck() {
        let level = |p: u8, pos: f32| -> f64 {
            let (l, r, _) = perform(&[(0.0, Ev::On(p, 80.0 / 127.0))], 0.32, &[(3, pos)]);
            let n = (0.3 * RATE) as usize;
            10.0 * ((0..n).map(|i| f64::from(l[i]).powi(2) + f64::from(r[i]).powi(2)).sum::<f64>() / (2 * n) as f64).log10()
        };
        let mut line = String::new();
        for p in [40u8, 52, 64, 71, 76, 78, 79, 80, 81, 82, 83] {
            let d = level(p, 1.0) - level(p, 0.5);
            line.push_str(&format!(" {p}:{d:+.1}"));
            assert!((-3.0..=4.0).contains(&d), "pitch {p}: {d:+.1} dB at POSITION's top");
        }
        println!("rms300 at POSITION's top re its centre:{line}");
    }

    /// **A choke stops the whole string**, what its release left ringing too
    /// (`damp_remnants`): C5 let go, then D5 taking the top string 0.1 s
    /// later, leaves C5's remnant (8 cents sharp of it) 35 dB and more under
    /// where it rings alone (the first draft: identical with and without D5;
    /// this test's render: -83 against -43 dB re the pre-off level at
    /// 0.25-0.55 s).
    #[test]
    fn a_choke_stops_what_a_release_left() {
        let f = f_equal(72) * 2.0_f64.powf(REMNANT_CENTS / 1200.0);
        let tail = |with_d5: bool| -> f64 {
            let mut evs = vec![(0.0, Ev::On(72, 80.0 / 127.0)), (0.5, Ev::Off(72))];
            if with_d5 {
                evs.push((0.6, Ev::On(74, 80.0 / 127.0)));
            }
            let (l, r, _) = perform(&evs, 1.2, &[]);
            partial_db(&l, &r, f, 0.75, 0.3) - partial_db(&l, &r, f_equal(72), 0.3, 0.2)
        };
        let (alone, choked) = (tail(false), tail(true));
        println!("C5's remnant 0.25-0.55 s after its key-off: {alone:.1} dB alone, {choked:.1} with D5 on its string");
        assert!(choked < alone - 35.0, "the remnant rang on under D5: {choked:.1} against {alone:.1} dB");
    }

    /// **WIDTH at zero is one microphone**: the whole instrument — strings,
    /// twins, phantoms, open strings, the air, a release's remnant — exactly
    /// mono.
    #[test]
    fn width_zero_is_one_microphone() {
        let evs = [(0.0, Ev::On(40, 1.0)), (0.0, Ev::On(57, 0.6)), (0.3, Ev::On(76, 0.8)), (0.6, Ev::Off(57)), (0.7, Ev::Off(76))];
        let (l, r, _) = perform(&evs, 1.2, &[(1, 0.0)]);
        assert!(l.iter().any(|v| v.abs() > 1e-3));
        assert!(l.iter().zip(&r).all(|(a, b)| a.to_bits() == b.to_bits()), "the microphones differ at WIDTH zero");
    }

    /// **A knob or a velocity that is not a number breaks nothing**: a NaN
    /// knob is that knob at its default (`GuitarKnobs::from_params`), a NaN
    /// velocity a silent touch; through the frame, every sample stays finite,
    /// and a NaN WIDTH plays exactly the default's sound.
    #[test]
    fn a_nan_knob_or_velocity_is_never_a_nan_sample() {
        for i in 0..MAX_PARAMS {
            let mut p = DEFAULTS;
            p[i] = f32::NAN;
            assert_eq!(GuitarKnobs::from_params(&p), knobs(), "a NaN knob {i} is not its default");
        }
        let mut v = GuitarVoice::new(RATE);
        v.strike(Place::home(52).unwrap(), 52, f32::NAN, &knobs());
        let (mut l, mut r, mut d) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK], [0.0_f32; BLOCK]);
        for _ in 0..40 {
            v.render(&mut l, &mut r, &mut d);
            assert!(l.iter().chain(&r).chain(&d).all(|s| *s == 0.0), "a NaN velocity sounded");
        }
        let evs = [(0.0, Ev::On(40, 1.0)), (0.0, Ev::On(57, f32::NAN)), (0.2, Ev::On(76, 0.8)), (0.5, Ev::Off(76))];
        let (l, r, _) = perform(&evs, 0.8, &[(1, f32::NAN), (2, f32::NAN)]);
        assert!(l.iter().chain(&r).all(|s| s.is_finite()), "a NaN knob reached a sample");
        let (l0, _, _) = perform(&evs, 0.8, &[]);
        assert!(l.iter().zip(&l0).all(|(a, b)| a.to_bits() == b.to_bits()), "a NaN WIDTH and DECAY are not the defaults");
    }

    /// **The pluck point is a fixed distance from the bridge.** Every
    /// partial's share of the pluck follows `sin(n pi beta)` (floored at
    /// `NOTCH_FLOOR`) with beta = 0.197 x 2^(fret/12) — the reference's
    /// measured law, `beta_v.py` within 0.005 of it to the 7th fret —
    /// and POSITION's bottom (0) halves the distance, its top doubles it.
    #[test]
    fn the_pluck_point_is_a_fixed_distance_from_the_bridge() {
        for &(pitch, pos) in &[(40u8, 0.5_f32), (43, 0.5), (57, 0.5), (69, 0.5), (71, 0.5), (76, 0.5), (83, 0.5), (45, 0.0), (64, 1.0), (79, 1.0)] {
            let k = knobs_with(3, pos);
            let place = Place::home(pitch).unwrap();
            // The 19th fret's default lands exactly on the cap; G5 at the top
            // of POSITION (0.94 asked) is held there (`beta_max`).
            let beta = (BETA_OPEN * 2.0_f64.powf(f64::from(place.fret) / 12.0) * f64::from(k.position)).min(beta_max());
            let mut v = GuitarVoice::new(RATE);
            v.strike(place, pitch, 80.0 / 127.0, &k);
            // The same velocity's spectral shape divided out: what is left
            // is the comb over n^2 (the triangle).
            let f0 = f_equal(pitch);
            let wound = WOUND[place.string];
            let nail = k.nail(80.0);
            let x1 = excitation(f0, f0, wound, nail);
            let q1 = v.plan.start[0];
            let c1 = (PI * beta).sin();
            let c1 = c1.signum() * (c1 * c1 + NOTCH_FLOOR * NOTCH_FLOOR).sqrt();
            // Up to the 20th, or the last under 20 kHz (B5 has 19).
            for n in 2..=v.n_partials.min(20) {
                let x = excitation(v.plan.freq[n - 1], f0, wound, nail) / x1;
                let got = v.plan.start[n - 1] / q1 * (n * n) as f64 / x;
                let c = (n as f64 * PI * beta).sin();
                let want = c.signum() * (c * c + NOTCH_FLOOR * NOTCH_FLOOR).sqrt() / c1;
                // 1e-6: rounding through the excitation's steep knees (2e-9
                // seen at E2 n6).
                assert!((got - want).abs() < 1e-6 * want.abs().max(1.0), "pitch {pitch} position {pos} n{n}: {got} vs {want}");
            }
        }
        assert_eq!(knobs_with(3, 0.0).position, 0.5);
        assert_eq!(knobs_with(3, 1.0).position, 2.0);
    }

    /// **The stiffness the reference measured, on the 19 notes it was
    /// measured on, and the stopped-string law between them.** A fretted note
    /// between two measured frets of one string has the open-equivalent B
    /// `B / 4^(fret/12)` interpolated log-linearly; past the last measured
    /// fret it is held. And a render puts E2's 5th, 10th and 15th partials on
    /// `n F sqrt(1 + B n^2)` to a fraction of a cent.
    #[test]
    fn the_stiffness_is_the_measured_one() {
        let measured: [(u8, f64); 19] = [
            (40, 2.04e-5),
            (43, 4.36e-5),
            (45, 7.23e-6),
            (47, 1.08e-5),
            (50, 2.09e-5),
            (52, 3.11e-5),
            (55, 5.98e-5),
            (57, 8.48e-5),
            (59, 1.66e-5),
            (62, 2.42e-5),
            (64, 3.13e-5),
            (67, 5.26e-5),
            (69, 7.6e-5),
            (71, 1.02e-4),
            (74, 6.73e-5),
            (76, 6.0e-5),
            (79, 5.46e-5),
            (81, 9.6e-5),
            (83, 2.1e-4),
        ];
        for &(p, b) in &measured {
            let got = inharmonicity(Place::home(p).unwrap());
            assert!((got / b - 1.0).abs() < 1e-12, "pitch {p}: {got:e} against {b:e}");
        }
        // The stopped-string law between the top string's 3rd and 5th frets.
        let k3 = 5.26e-5 / 4.0_f64.powf(3.0 / 12.0);
        let k5 = 7.6e-5 / 4.0_f64.powf(5.0 / 12.0);
        let want = (0.5 * (k3.ln() + k5.ln())).exp() * 4.0_f64.powf(4.0 / 12.0);
        assert!((inharmonicity(Place { string: 0, fret: 4 }) / want - 1.0).abs() < 1e-12);
        // Held past the last measured fret: the A string's 8th fret.
        let k2 = 1.08e-5 / 4.0_f64.powf(2.0 / 12.0);
        let got = inharmonicity(Place { string: 4, fret: 8 });
        assert!((got / (k2 * 4.0_f64.powf(8.0 / 12.0)) - 1.0).abs() < 1e-12);
        // Rendered: E2's partials where the formula puts them (B = 0 would
        // put the 15th 3.95 c flat of it), at MIDI 64, where the glide
        // (`GLIDE`) is a few tenths of a cent by 0.3 s.
        let (l, _, _, v) = play(40, 0.5, 1.6, &knobs(), None);
        let b: f64 = 2.04e-5;
        let big_f = f_equal(40) / (1.0 + b).sqrt();
        for n in [5usize, 10, 15] {
            let want = n as f64 * big_f * (1.0 + b * (n * n) as f64).sqrt();
            let (_, at) = peak_near(&l[(0.3 * RATE) as usize..(1.5 * RATE) as usize], want);
            let cents = 1200.0 * (at / want).log2();
            println!("E2 n{n}: {at:.3} Hz against {want:.3} ({cents:+.2} c); design {:.3}", v.partial_freq(n));
            assert!(cents.abs() < 0.5, "E2 n{n} {cents:+.2} c off the stretched series");
        }
    }

    /// **Each fundamental decays at its harp-off T60.** The design (the
    /// material's loss plus the bridge's, `lossfit3.py`) against the
    /// reference's harp-off fits (`slopes.py` 0.06-0.5 s on its `gx` and
    /// `vlow` renders with Sympathetic Resonance 0), within the acceptance
    /// row's 30 %. The worst is G3: designed 2.98 s, rendered 3.23 s (the
    /// twin, which has no bridge bump, lifts it) against 4.0 s; every other
    /// rendered fit within 11 % (`report.py g23`, renders g23-gx-s0 and
    /// g23-vlow-s0). Then a render of three of them, measured on their own
    /// envelope.
    #[test]
    fn each_fundamental_decays_at_its_harp_off_t60() {
        let targets: [(u8, f64); 14] = [
            (40, 8.1),
            (43, 9.0),
            (45, 18.1),
            (47, 10.8),
            (50, 13.8),
            (52, 8.5),
            (55, 4.0),
            (57, 1.5),
            (59, 4.9),
            (62, 2.9),
            (64, 4.6),
            (69, 2.9),
            (76, 1.35),
            (83, 1.2),
        ];
        let mut line = Vec::new();
        for &(p, t) in &targets {
            let place = Place::home(p).unwrap();
            let f1 = f_equal(p);
            let t60 = 1000.0_f64.ln() / mode_sigma(place, f1);
            line.push(format!("{p}:{t60:.2}/{t}"));
            assert!((t60 / t - 1.0).abs() < 0.30, "pitch {p}: designed T60 {t60:.2} s against {t} s");
        }
        println!("designed n1 T60 / reference harp-off: {}", line.join(" "));
        for &(p, t) in &[(40u8, 8.1), (57, 1.5), (64, 4.6)] {
            let (l, r, _, _) = play(p, 0.63, 0.7, &knobs(), None);
            let f1 = f_equal(p);
            let a = partial_db(&l, &r, f1, 0.06, 0.15);
            let b = partial_db(&l, &r, f1, 0.45, 0.15);
            let t60 = 60.0 * 0.39 / (a - b);
            println!("pitch {p}: rendered n1 T60 {t60:.2} s (target {t})");
            assert!((t60 / t - 1.0).abs() < 0.30, "pitch {p}: rendered T60 {t60:.2} s against {t}");
        }
    }

    /// **The hollow body's air rings at 104 Hz for a third of a second**
    /// (the reference's hollow-minus-solid fits: 103.8-105.1 Hz, T60 median
    /// about 0.33 s): an impulse through the fretboard's air, its frequency
    /// from the analytic phase and its T60 from two windows. BODY at zero is
    /// a solid body: no air at all.
    #[test]
    fn the_air_rings_at_104_hz() {
        let run = |body: f32| -> Vec<f32> {
            let mut fb = Fretboard::new(RATE);
            fb.set_knobs(&knobs_with(4, body));
            let n = (0.6 * RATE) as usize / BLOCK * BLOCK;
            let mut out = vec![0.0_f32; n];
            let mut at = 0;
            while at < n {
                fb.begin(BLOCK);
                let mut l = [0.0_f32; BLOCK];
                let mut r = [0.0_f32; BLOCK];
                if at == 0 {
                    l[0] = 1.0;
                    r[0] = 1.0;
                }
                fb.render(&mut l, &mut r, BLOCK);
                if at == 0 {
                    l[0] -= 1.0;
                }
                out[at..at + BLOCK].copy_from_slice(&l);
                at += BLOCK;
            }
            out
        };
        let x = run(0.5);
        // The two modes beat at 10.5 Hz: frequency and decay from windows a
        // whole number of beats long.
        let (m1, f) = peak_near(&x[(0.05 * RATE) as usize..(0.05 * RATE) as usize + 4571 * 2], 104.0);
        let m2 = tone(&x[(0.3 * RATE) as usize..(0.3 * RATE) as usize + 4571 * 2], f);
        let t60 = 60.0 * 0.25 / (20.0 * (m1 / m2).log10());
        println!("air: {f:.2} Hz, T60 {t60:.3} s");
        assert!((f - 104.0).abs() < 1.5, "air at {f}");
        assert!((0.28..0.42).contains(&t60), "air T60 {t60}");
        let solid = run(0.0);
        assert!(solid.iter().all(|s| *s == 0.0), "BODY 0 still rang the air");
    }

    /// **Every knob at its default IS the tuned instrument**, each mapping
    /// exactly on its constant, and a pluck with the defaults bit-identical
    /// to one with those constants written out by hand.
    #[test]
    fn every_knob_at_its_default_is_the_tuned_instrument() {
        let k = knobs();
        assert_eq!(k.voicing, 0.5);
        assert_eq!(k.nail(80.0), blend(80.0), "VOICING's centre is the reference's blend");
        assert_eq!(k.width, 1.0);
        assert_eq!(k.decay, 1.0);
        assert_eq!(k.position, 1.0);
        assert_eq!(k.body, 1.0);
        assert_eq!(f64::from(k.pick), (PICK_DEFAULT as f32) as f64);
        assert_eq!(k.release, 1.0);
        assert_eq!(k.strings, 1.0);
        let by_hand = GuitarKnobs {
            voicing: 0.5,
            width: 1.0,
            decay: 1.0,
            position: 1.0,
            body: 1.0,
            pick: PICK_DEFAULT as f32,
            release: 1.0,
            strings: 1.0,
        };
        let (a, _, _, _) = play(57, 0.7, 0.3, &k, None);
        let (b, _, _, _) = play(57, 0.7, 0.3, &by_hand, None);
        assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
        // And the top of PICK is the reference's own contact timing.
        let top = knobs_with(5, 1.0);
        assert!((f64::from(top.pick) - 1.0).abs() < 2e-3, "PICK's top is {:.4} of the reference", top.pick);
    }

    /// **Velocity is the finger's force, and the finger's brightness is
    /// separate from it.** The fundamental's share of a MIDI-127 pluck over a
    /// MIDI-10 one is (127/10)^1.10 exactly, 24.3 dB (the reference's A2/A3/A4
    /// +23.7/+26.3/+25.6, `gvelramp`); the upper half of the attack rises a
    /// further 8-15 dB with velocity (the reference's +13.2/+8.1/+15.1); and
    /// VOICING at zero (all flesh) leaves the fundamental and cuts n >= 3 by
    /// 20-30 dB on a plain string (the reference's hardness-0 A3: n2 -24,
    /// n3+ -29..-31).
    #[test]
    fn velocity_sets_the_level_and_the_finger_the_brightness() {
        for p in [45u8, 57, 69] {
            let place = Place::home(p).unwrap();
            let start = |vel: f32, k: &GuitarKnobs| -> Vec<f64> {
                let mut v = GuitarVoice::new(RATE);
                v.strike(place, p, vel, k);
                (0..12).map(|i| v.plan.start[i].abs()).collect()
            };
            let lo = start(10.0 / 127.0, &knobs());
            let hi = start(1.0, &knobs());
            let n1 = 20.0 * (hi[0] / lo[0]).log10();
            assert!((n1 - 20.0 * VELOCITY_EXP * 12.7_f64.log10()).abs() < 1e-6, "pitch {p}: n1 span {n1:.2} dB");
            let bright = (4..10).map(|i| 20.0 * ((hi[i] / hi[0]) / (lo[i] / lo[0])).log10()).sum::<f64>() / 6.0;
            println!("pitch {p}: n1 +{n1:.1} dB, n5..n10 re n1 +{bright:.1} dB from MIDI 10 to 127");
            assert!((8.0..=15.0).contains(&bright), "pitch {p}: brightness span {bright:.1} dB");
        }
        let place = Place::home(57).unwrap();
        let start = |k: &GuitarKnobs| -> Vec<f64> {
            let mut v = GuitarVoice::new(RATE);
            v.strike(place, 57, 80.0 / 127.0, k);
            (0..12).map(|i| v.plan.start[i].abs()).collect()
        };
        let base = start(&knobs());
        let flesh = start(&knobs_with(0, 0.0));
        assert!((flesh[0] / base[0] - 1.0).abs() < 1e-12, "VOICING moved the fundamental");
        for n in 3..=10 {
            let cut = 20.0 * (flesh[n - 1] / base[n - 1]).log10();
            assert!((-32.0..=-18.0).contains(&cut), "A3 n{n}: flesh cut {cut:.1} dB");
        }
    }

    /// **The finger holds the string, then lets go.** At MIDI 101.6 (the
    /// frame's test velocity) the release comes 7.3 ms after the note-on,
    /// PICK's centre times the reference's contact law, with only the
    /// contact noise (about -29 dB) before it; at PICK zero it comes at once.
    #[test]
    fn the_finger_holds_the_string_for_the_contact_time() {
        let onset = |k: &GuitarKnobs| -> f64 {
            let (l, _, _, _) = play(57, 0.8, 0.1, k, None);
            let pk = l.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
            l.iter().position(|s| s.abs() > 0.1 * pk).unwrap() as f64 / f64::from(RATE)
        };
        let want = contact_law(0.8 * 127.0) * PICK_DEFAULT;
        let got = onset(&knobs());
        println!("release at {:.2} ms (contact law {:.2} ms)", got * 1e3, want * 1e3);
        assert!((got - want).abs() < 1.0e-3, "released at {got} against {want}");
        assert!(onset(&knobs_with(5, 0.0)) < 1.0e-3, "PICK 0 held the string");
        // The contact before the release is the finger's noise, 20-40 dB
        // under the plucked note.
        let (l, _, _, _) = play(57, 0.8, 0.1, &knobs(), None);
        let rms = |a: usize, b: usize| (l[a..b].iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / (b - a) as f64).sqrt();
        let pre = rms(48, (want * 48_000.0) as usize - 48);
        let post = rms((want * 48_000.0) as usize + 15 * 48, (want * 48_000.0) as usize + 37 * 48);
        let rel = 20.0 * (pre / post).log10();
        println!("contact noise {rel:.1} dB re the pluck");
        assert!((-40.0..=-20.0).contains(&rel), "contact noise at {rel:.1} dB");
    }

    /// **Repeated plucks never pile up modes** (`leave_remnant`): the bass E
    /// plucked at MIDI 100 and let go ten times a second, forty times, holds
    /// no more modes than one pluck and its release do (the first draft: 768,
    /// the bank full, after 40), and the remnant still carries the tail.
    #[test]
    fn repeated_plucks_never_pile_up_modes() {
        let mut v = GuitarVoice::new(RATE);
        let (mut l, mut r, mut d) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK], [0.0_f32; BLOCK]);
        let per = (0.1 * RATE) as usize / BLOCK;
        let mut once = 0;
        let mut most = 0;
        for i in 0..40 {
            v.strike(Place::home(40).unwrap(), 40, 100.0 / 127.0, &knobs());
            for b in 0..per {
                if b == per / 2 {
                    v.release(false);
                }
                v.render(&mut l, &mut r, &mut d);
                most = most.max(v.bank.count());
                if i == 0 {
                    once = once.max(v.bank.count());
                }
            }
        }
        let remnants = (0..v.bank.count()).filter(|&s| v.bank.tag(s) >> 24 >= KIND_REMNANT).count();
        println!("E2 ten plucks a second: at most {most} modes over 40 plucks, {once} over the first; {remnants} remnant modes");
        assert!(most <= once, "the modes piled up: {most} against one pluck's {once}");
        assert!((1..=2 * REMNANT_PARTIALS).contains(&remnants), "{remnants} remnant modes");
    }

    /// **The stealing policy hears a pluck from its first block** (`level`):
    /// the bank's level is measured through the attack's no-cull hold, so a
    /// voice 10 ms into its note reads within 6 dB of what it reads at
    /// 200 ms, where the first draft read the empty bank's zero for 150 ms
    /// (the lane verifier, finding 13).
    #[test]
    fn a_pluck_is_heard_by_the_stealing_policy_from_its_first_block() {
        let mut v = GuitarVoice::new(RATE);
        v.strike(Place::home(45).unwrap(), 45, 80.0 / 127.0, &knobs());
        let (mut l, mut r, mut d) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK], [0.0_f32; BLOCK]);
        let mut levels = Vec::new();
        for _ in 0..(0.2 * RATE) as usize / BLOCK {
            v.render(&mut l, &mut r, &mut d);
            levels.push(v.level());
        }
        let early = levels[(0.010 * RATE) as usize / BLOCK];
        let late = *levels.last().unwrap();
        println!("A2 level at 10 ms {early:.3e}, at 200 ms {late:.3e}");
        assert!(early > 0.0 && (20.0 * (early / late).log10()).abs() < 6.0, "level {early:.3e} at 10 ms against {late:.3e} at 200 ms");
    }

    /// **The left hand stops the string without a step**: the first
    /// difference across the release no larger than before it, and the
    /// fundamental falling at the reference's 180-480 dB/s over 15-75 ms
    /// (`damp_v.py`, its open strings off).
    #[test]
    fn a_release_damps_the_string_without_a_step() {
        let k = knobs_with(7, 0.0);
        let off = 0.6_f32;
        let (l, r, _, _) = play(57, 0.6, 1.0, &k, Some(off));
        let i = (off * RATE) as usize / BLOCK * BLOCK;
        let d1 = |a: usize, b: usize| (a..b).map(|n| (l[n] - l[n - 1]).abs()).fold(0.0_f32, f32::max);
        assert!(d1(i, i + 2000) <= d1(i - 2000, i) * 1.05, "the damper stepped");
        let t = i as f32 / RATE;
        let f1 = f_equal(57);
        let a = partial_db(&l, &r, f1, t + 0.010, 0.020);
        let b = partial_db(&l, &r, f1, t + 0.060, 0.020);
        let rate = (a - b) / 0.050;
        println!("A3 released: n1 falls {rate:.0} dB/s over 15-75 ms");
        assert!((180.0..=600.0).contains(&rate), "release rate {rate:.0} dB/s");
    }

    /// **The open strings answer a coincident partial, and carry the tail.**
    /// A4 (whose fundamental is the open A string's fourth partial) held
    /// three seconds: with STRINGS at its centre its fundamental 2.5 s on
    /// reads within 6 dB of the reference's -29.7 dB re its 0.1 s level, and
    /// at zero well under it. And after a C4 key-off the tail rings on for a
    /// second (the reference's -71 dB at 1.0-1.5 s re the 0.1 s before).
    #[test]
    fn the_open_strings_answer_and_carry_the_tail() {
        let evs = [(0.0, Ev::On(69, 80.0 / 127.0)), (3.0, Ev::Off(69))];
        let level = |strings: f32| -> f64 {
            let (l, r, _) = perform(&evs, 2.8, &[(7, strings)]);
            let f = f_equal(69);
            partial_db(&l, &r, f, 2.3, 0.4) - partial_db(&l, &r, f, 0.045, 0.15)
        };
        let (on, off) = (level(0.5), level(0.0));
        println!("A4 n1 at 2.5 s re 0.1 s: {on:.1} dB with the open strings, {off:.1} without");
        assert!((on + 29.7).abs() < 6.0, "A4's sympathetic tail {on:.1} dB");
        assert!(on > off + 6.0, "the open strings added {:.1} dB", on - off);
        let (l, r, _) = perform(&[(0.0, Ev::On(60, 80.0 / 127.0)), (0.5, Ev::Off(60))], 2.1, &[]);
        let rms = |a: f32, b: f32| -> f64 {
            let (i, j) = ((a * RATE) as usize, (b * RATE) as usize);
            ((i..j).map(|n| f64::from(l[n]).powi(2) + f64::from(r[n]).powi(2)).sum::<f64>() / (2 * (j - i)) as f64).sqrt()
        };
        let tail = 20.0 * (rms(1.5, 2.0) / rms(0.4, 0.5)).log10();
        println!("C4 tail 1.0-1.5 s after the key-off: {tail:.1} dB re the 0.1 s before");
        assert!((-80.0..=-55.0).contains(&tail), "C4's tail at {tail:.1} dB");
    }

    /// **What a release leaves rings longer up the neck.** The remnant's
    /// decay is the reference's C4 rate (33 dB/s) to C4, its C5 rate (22)
    /// from C5, and runs between them, as its share does (0.042 to 0.027);
    /// and through the frame, C5 held a second leaves its tail 1.0-1.5 s
    /// after the key-off within 5 dB of the reference's -63.1 dB re the 0.1 s
    /// before, both channels' power (`rel_v.py` on its release score; the
    /// lane's tbench render reads -60.8).
    #[test]
    fn the_treble_tail_decays_slower() {
        assert_eq!(remnant_db(40), 33.0);
        assert_eq!(remnant_db(60), 33.0);
        assert_eq!(remnant_db(66), 27.5);
        assert_eq!(remnant_db(72), 22.0);
        assert_eq!(remnant_db(83), 22.0);
        assert_eq!(remnant_share(60), REMNANT_SHARE_C4);
        assert_eq!(remnant_share(83), REMNANT_SHARE_C5);
        let (l, r, _) = perform(&[(0.0, Ev::On(72, 80.0 / 127.0)), (1.0, Ev::Off(72))], 2.6, &[]);
        // Both channels' power (WAVE2, trap 2).
        let rms = |a: f32, b: f32| -> f64 {
            let (i, j) = ((a * RATE) as usize, (b * RATE) as usize);
            ((i..j).map(|n| f64::from(l[n]).powi(2) + f64::from(r[n]).powi(2)).sum::<f64>() / (2 * (j - i)) as f64).sqrt()
        };
        let tail = 20.0 * (rms(2.0, 2.5) / rms(0.9, 1.0)).log10();
        println!("C5 tail 1.0-1.5 s after the key-off: {tail:.1} dB re the 0.1 s before");
        assert!((tail + 63.1).abs() < 5.0, "C5's tail at {tail:.1} dB");
    }

    /// **Phantoms are sum tones only, at twice their partials.** E2 at MIDI
    /// 127: every phantom mode sits at exactly `2 f_k`, none under 1.5 f0
    /// (the critic: the reference's quadratic energy there is -75 dB), and
    /// the one at `2 f_13` stands clear of the gap beside it in a render.
    #[test]
    fn phantoms_are_sum_tones_at_twice_their_partials() {
        // Read the bank just after the release, before the cull thins it.
        let (_, _, _, v) = play(40, 1.0, 0.05, &knobs(), None);
        let mut seen = 0;
        for s in 0..v.bank.count() {
            let t = v.bank.tag(s);
            if t >> 24 == KIND_PHANTOM {
                let k = ((t >> 8) & 0xffff) as usize;
                let (f, _) = v.bank.freq_sigma(s);
                // At twice its partial, untuned (`phantom_spec`).
                assert!((f64::from(f) / (2.0 * v.partial_freq(k)) - 1.0).abs() < 1e-5, "phantom k{k} at {f} Hz");
                assert!(f64::from(f) > 1.5 * f_equal(40));
                seen += 1;
            }
        }
        assert_eq!(seen, PHANTOM_PARTIALS, "one phantom per partial up to the 20th");
        let (l, _, _, v) = play(40, 1.0, 3.0, &knobs(), None);
        let w = &l[(0.1 * RATE) as usize..(2.9 * RATE) as usize];
        let f13 = v.partial_freq(13);
        let f26 = v.partial_freq(26);
        let phantom = tone(w, 2.0 * f13);
        let gap = tone(w, 2.0 * f13 + 0.5 * (f26 - 2.0 * f13));
        let lift = 20.0 * (phantom / gap).log10();
        println!("E2 v127 k13: phantom {lift:.1} dB over the mid-gap");
        assert!(lift > 30.0, "the phantom at 2 f_13 is {lift:.1} dB over the gap");
        // None for a treble string.
        let (_, _, _, t) = play(69, 1.0, 0.1, &knobs(), None);
        assert!((0..t.bank.count()).all(|s| t.bank.tag(s) >> 24 != KIND_PHANTOM));
    }

    /// **The loudest thing a keyboard can ask of it does not clip**: all six
    /// strings at MIDI 127 in one instant, under the brief's -3 dBFS ceiling
    /// (`GAIN` was set by these: G major -3.5 dBFS in the lane's tbench).
    #[test]
    fn the_loudest_strum_stays_under_minus_3_dbfs() {
        for (name, shape) in [("E major", [40u8, 47, 52, 56, 59, 64]), ("G major", [43, 47, 50, 55, 59, 67]), ("E major, B5 on top", [40, 47, 52, 56, 59, 83])] {
            let evs: Vec<(f32, Ev)> = shape.iter().map(|&p| (0.0, Ev::On(p, 1.0))).collect();
            let (l, r, _) = perform(&evs, 1.0, &[]);
            let pk = l.iter().chain(&r).fold(0.0_f32, |m, s| m.max(s.abs()));
            println!("{name} at MIDI 127, all at once: {:.1} dBFS", 20.0 * pk.log10());
            assert!(pk < 0.708, "{name} peaked at {pk}");
        }
    }

    /// **The register is level the way the reference's is, key by key.**
    /// Every key from E2 to B5 alone through the frame at MIDI 80: its first
    /// 300 ms RMS (both microphones) less the mean over the 44, against the
    /// same for the reference's chromatic scan (`chromq`, `chrom.py`: -22.5
    /// to -28.6 dBFS), within 3 dB on every key, and every step across a
    /// string crossing within 1.5 dB of the reference's step there (the
    /// lane's tbench render: 0.88 dB RMS, crossings within 0.7; the first
    /// draft's per-string voicing 1.18 dB RMS and G#2-A2 -4.3 dB against the
    /// reference's -0.1). And the voicing averages zero over the 44.
    #[test]
    fn the_register_is_level_the_way_the_reference_is() {
        let reference: [f64; 44] = [
            -22.7, -24.7, -24.1, -22.8, -24.2, -24.2, -23.3, -23.6, -25.3, -24.8, -26.3, -27.2, -25.3, -24.8, -23.6, -23.2, -22.5, -23.3, -24.4, -25.6, -26.5, -25.8,
            -25.7, -27.5, -25.9, -24.9, -25.1, -23.8, -25.8, -23.8, -25.7, -24.5, -24.7, -25.0, -26.6, -24.9, -26.5, -23.8, -23.6, -25.8, -25.7, -24.2, -28.6, -26.2,
        ];
        let keys: Vec<u8> = (LOWEST..=HIGHEST).collect();
        let mean_trim = keys.iter().map(|&p| voicing_db(p)).sum::<f64>() / keys.len() as f64;
        assert!(mean_trim.abs() < 0.01, "the voicing averages {mean_trim:.3} dB");
        let got: Vec<f64> = keys
            .iter()
            .map(|&p| {
                let (l, r, _) = perform(&[(0.0, Ev::On(p, 80.0 / 127.0))], 0.35, &[]);
                let n = (0.3 * RATE) as usize;
                10.0 * ((0..n).map(|i| f64::from(l[i]).powi(2) + f64::from(r[i]).powi(2)).sum::<f64>() / (2 * n) as f64).log10()
            })
            .collect();
        let off = got.iter().zip(&reference).map(|(g, r)| g - r).sum::<f64>() / 44.0;
        let mut line = String::new();
        let mut sq = 0.0;
        for (i, (&p, (g, r))) in keys.iter().zip(got.iter().zip(&reference)).enumerate() {
            let d = g - r - off;
            sq += d * d;
            line.push_str(&format!(" {p}:{d:+.1}"));
            assert!(d.abs() < 3.0, "pitch {p}: {d:+.1} dB off the reference's balance");
            // The last fret before a crossing: G#2, C#3, F#3, A#3, D#4.
            if i > 0 && Place::home(p).unwrap().string != Place::home(p - 1).unwrap().string {
                let (step, want) = (g - got[i - 1], r - reference[i - 1]);
                println!("crossing into {p}: {step:+.1} dB against the reference's {want:+.1}");
                assert!((step - want).abs() < 1.5, "crossing into {p}: {step:+.1} dB against {want:+.1}");
            }
        }
        println!("rms300 re the reference's balance (mean {off:+.1} dB, {:.2} dB RMS):{line}", (sq / 44.0).sqrt());
    }

    /// **Every note, every dynamic: finite, bounded, audible.** E2 to B5 at
    /// MIDI 1, 64 and 127, a single note never above -10 dBFS.
    #[test]
    fn every_note_renders_finite_bounded_and_audible() {
        for p in LOWEST..=HIGHEST {
            for midi in [1u8, 64, 127] {
                let (l, r, d, _) = play(p, f32::from(midi) / 127.0, 0.3, &knobs(), None);
                let mut pk = 0.0_f32;
                for i in 0..l.len() {
                    assert!(l[i].is_finite() && r[i].is_finite() && d[i].is_finite(), "pitch {p} v{midi}");
                    pk = pk.max(l[i].abs()).max(r[i].abs());
                }
                assert!(pk < 0.316, "pitch {p} v{midi} peaked at {pk}");
                assert!(pk > 1.0e-6, "pitch {p} v{midi} is silent");
            }
        }
    }

    /// What allocates, spelled as it would be.
    const ALLOCATING: [&str; 10] = ["Vec", "vec!", "Box::new", "String", "format!", "to_string", "to_vec", "collect", "with_capacity", ".push("];

    /// Every line of `src` outside its test module and outside the bodies of
    /// functions named `new` that spells an allocation: every function a
    /// strike, a block, a key-up or the pedal can reach, the helpers they call
    /// included (the first draft read 22 named functions and none of their
    /// helpers; the lane verifier, finding 13). A `new` body runs from its
    /// `fn` line to the line closing the brace it opened.
    fn rt_allocations(src: &str) -> Vec<String> {
        let body = &src[..src.find("#[cfg(test)]\nmod tests").unwrap_or(src.len())];
        let mut found = Vec::new();
        let mut depth = 0i32;
        let mut in_new: Option<i32> = None;
        for (i, line) in body.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if in_new.is_none() && (code.contains("fn new(") || code.contains("fn new<")) {
                in_new = Some(depth);
            }
            if in_new.is_none() && ALLOCATING.iter().any(|a| code.contains(a)) {
                found.push(format!("line {}: {}", i + 1, line.trim()));
            }
            let closes = code.matches('}').count() as i32;
            depth += code.matches('{').count() as i32 - closes;
            if in_new.is_some_and(|d| depth <= d && closes > 0) {
                in_new = None;
            }
        }
        found
    }

    /// **Nothing a strike, a block, a key-up or the pedal reaches allocates**
    /// (the module's real-time contract): every line of this file outside its
    /// constructors read for an allocation, the way the engine's lane-clamp
    /// scan reads `instrument.rs`. The app binary counts allocations at run
    /// time (`rt_alloc`); this one cannot install its own counter beside it,
    /// so the guitar is read here and counted on the lane's scratch rig
    /// (MERGE-ME), which is the proof: 0 allocations across its run.
    #[test]
    fn a_pluck_and_a_block_allocate_nothing() {
        let found = rt_allocations(include_str!("guitar.rs"));
        assert!(found.is_empty(), "code outside a constructor allocates:\n{}", found.join("\n"));
        // The scan bites: a planted allocation is found in a named function
        // and in a helper alike, a comment that quotes one is not, and `new`
        // may allocate while the function after it may not.
        let planted = "fn render(&mut self) {\n    let v = Vec::new();\n}\n";
        assert_eq!(rt_allocations(planted).len(), 1, "the scan missed a planted Vec");
        let helper = "fn bumps(c: &[f64]) -> f64 {\n    let s = String::new();\n    0.0\n}\n";
        assert_eq!(rt_allocations(helper).len(), 1, "the scan missed a helper's String");
        let quoted = "fn render(&mut self) {\n    // never a Vec here\n}\n";
        assert!(rt_allocations(quoted).is_empty(), "a comment was flagged");
        let in_new = "fn new() -> Self {\n    let v = vec![0.0; 4];\n    if v.is_empty() {\n    }\n}\nfn after() {\n    let w = vec![1];\n}\n";
        assert_eq!(rt_allocations(in_new).len(), 1, "new() was flagged, or the function after it was not");
    }

    /// Timing, not a test: what a pluck costs on the audio thread, and what
    /// one sounding voice costs per second of audio.
    #[test]
    #[ignore]
    fn timing_of_a_pluck_and_a_voice() {
        for p in [40u8, 52, 64, 83] {
            let mut best = f64::MAX;
            let mut v = GuitarVoice::new(RATE);
            for _ in 0..20 {
                v.silence();
                let t0 = std::time::Instant::now();
                v.strike(Place::home(p).unwrap(), p, 0.8, &knobs());
                let (mut l, mut r, mut d) = ([0.0; BLOCK], [0.0; BLOCK], [0.0; BLOCK]);
                // The release happens inside one of the first blocks.
                for _ in 0..4 {
                    v.render(&mut l, &mut r, &mut d);
                }
                best = best.min(t0.elapsed().as_secs_f64());
            }
            let mut best_r = f64::MAX;
            for _ in 0..5 {
                let mut v = GuitarVoice::new(RATE);
                v.strike(Place::home(p).unwrap(), p, 0.8, &knobs());
                let (mut l, mut r, mut d) = ([0.0; BLOCK], [0.0; BLOCK], [0.0; BLOCK]);
                let t0 = std::time::Instant::now();
                for _ in 0..(2.0 * RATE) as usize / BLOCK {
                    v.render(&mut l, &mut r, &mut d);
                }
                best_r = best_r.min(t0.elapsed().as_secs_f64());
                std::hint::black_box(&l);
            }
            println!("pitch {p}: pluck + 4 blocks {:.1} us; one voice {:.2} ms per 2 s ({:.0}x real time)", best * 1e6, best_r * 1e3, 2.0 / best_r);
        }
    }

    /// Diagnostic, not a test: the glide a pluck calls for, block by block
    /// (`GLIDE`), and how many times the bank was rebuilt for it.
    #[test]
    #[ignore]
    fn print_the_glide() {
        for (p, vel) in [(40u8, 1.0_f32), (40, 80.0 / 127.0), (45, 1.0), (64, 1.0), (76, 1.0)] {
            let mut v = GuitarVoice::new(RATE);
            v.strike(Place::home(p).unwrap(), p, vel, &knobs());
            let (mut l, mut r, mut d) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK], [0.0_f32; BLOCK]);
            let (mut line, mut steps, mut last) = (String::new(), 0, 0.0);
            for b in 0..(1.6 * RATE) as usize / BLOCK {
                v.render(&mut l, &mut r, &mut d);
                if v.glide_cents != last {
                    steps += 1;
                    last = v.glide_cents;
                }
                let t = (b + 1) as f32 * BLOCK as f32 / RATE;
                if [2usize, 5, 10, 20, 45, 75, 112, 187, 300, 562].contains(&b) {
                    line.push_str(&format!(" {:.3}s:{:+.2}", t, v.glide_cents));
                }
            }
            println!("pitch {p} MIDI {:.0}: glide{line}; {steps} rebuilds in 1.6 s", vel * 127.0);
        }
    }

    /// Diagnostic, not a test: the bench's `stress` score (keys 30..93 at
    /// MIDI 96, 20 ms apart, pedal down) through the frame, counting the
    /// guitar voices alive and the modes they hold.
    #[test]
    #[ignore]
    fn print_stress_voices() {
        let mut evs = vec![(0.0_f32, Ev::Pedal(true))];
        let mut t = 0.05;
        for p in 30u8..94 {
            evs.push((t, Ev::On(p, 96.0 / 127.0)));
            evs.push((t + 0.10, Ev::Off(p)));
            t += 0.02;
        }
        let mut k = Keys::new(RATE);
        k.set_patch(Patch::Guitar);
        let n = (7.4 * RATE) as usize / BLOCK * BLOCK;
        let mut out = vec![0.0_f32; 2 * BLOCK];
        let (mut next, mut at) = (0, 0);
        let (mut most, mut modes_most, mut sum) = (0usize, 0usize, 0usize);
        let t0 = std::time::Instant::now();
        while at < n {
            while next < evs.len() && ((evs[next].0 * RATE) as usize) < at + BLOCK {
                match evs[next].1 {
                    Ev::On(p, v) => k.note_on(p, v),
                    Ev::Off(p) => k.note_off(p),
                    Ev::Pedal(d) => k.set_pedal(d),
                    Ev::Sostenuto(d) => k.set_pedals([if k.pedal { 127 } else { 0 }, if d { 127 } else { 0 }, 0]),
                }
                next += 1;
            }
            out.fill(0.0);
            k.render(&mut out, BLOCK, 2);
            let alive = k.notes.iter().filter(|n| n.pitch.is_some() && !n.mech.finished()).count();
            let modes: usize = k.notes.iter().map(|n| n.mech.guitar.bank.count()).sum();
            most = most.max(alive);
            modes_most = modes_most.max(modes);
            sum += alive;
            at += BLOCK;
        }
        let secs = t0.elapsed().as_secs_f64();
        println!(
            "stress: at most {most} guitar voices alive ({:.1} on average), at most {modes_most} modes; {:.0}x real time in the test build",
            sum as f64 / (n / BLOCK) as f64,
            7.4 / secs
        );
    }
    /// **A cleared fretboard is a new one, and a hushed one keeps who is on
    /// which string and nothing else.** Twelve histories: strings taken,
    /// let go, freed and held by the pedal at random under random knobs,
    /// every block's buses driven at a random level. Then either `clear` and
    /// the pedal re-sent (what `Keys::set_patch` does), against a new board
    /// given the same knobs and pedal; or the frame's all notes off
    /// (`release_all`), a string taken while the hush fades, then `hush`,
    /// against a new board given the same knobs and the ownership the hush
    /// keeps on purpose (owners, holds, the keys down and the pedal, and
    /// with the keys whether the strings listen: `deaf`). From there the same
    /// strings and drive render the two to the bit: the open strings and the
    /// air carry nothing of the history.
    #[test]
    fn a_cleared_fretboard_is_a_new_one() {
        let mut seed = 0x6b8d_2e47_u32;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32
        };
        let render = |f: &mut Fretboard, k: &GuitarKnobs, drive: &dyn Fn(usize, usize) -> f32, out: &mut [f32; 2 * BLOCK]| {
            f.set_knobs(k);
            f.begin(BLOCK);
            for s in 0..STRINGS {
                if f.owner[s].is_some() {
                    for (i, b) in f.bus(s, BLOCK).iter_mut().enumerate() {
                        *b += drive(s, i);
                    }
                }
            }
            let (l, r) = out.split_at_mut(BLOCK);
            f.render(l, r, BLOCK);
        };
        for trial in 0..12 {
            let mut p = DEFAULTS;
            p[2] = rnd();
            p[3] = rnd();
            p[7] = rnd();
            let k = GuitarKnobs::from_params(&p);
            let mut used = Fretboard::new(RATE);
            let level = 10.0_f32.powf(-2.0 * rnd());
            let mut out = [0.0_f32; 2 * BLOCK];
            for _ in 0..(20 + (rnd() * 200.0) as usize) {
                let pitch = LOWEST + (rnd() * f32::from(HIGHEST - LOWEST)) as u8;
                match (rnd() * 6.0) as u32 {
                    0 | 1 => {
                        if let Some(pl) = used.place(pitch) {
                            used.take(pl, pitch);
                        }
                    }
                    2 => used.key_up(pitch),
                    3 => used.voice_done(pitch),
                    4 => used.set_pedal(rnd() > 0.5),
                    _ => {}
                }
                let noise: Vec<f32> = (0..STRINGS * BLOCK).map(|_| level * (2.0 * rnd() - 1.0)).collect();
                render(&mut used, &k, &|s, i| noise[s * BLOCK + i], &mut out);
            }
            let mut fresh = Fretboard::new(RATE);
            fresh.set_knobs(&k);
            let hushing = trial % 2 == 1;
            if hushing {
                used.release_all();
                if let Some(pl) = used.place(64) {
                    used.take(pl, 64);
                }
                render(&mut used, &k, &|_, _| 0.0, &mut out);
                used.hush();
                fresh.owner = used.owner;
                fresh.held = used.held;
                fresh.sustained = used.sustained;
                fresh.answering = used.answering;
                fresh.keys = used.keys;
                fresh.pedal = used.pedal;
                fresh.deaf = used.deaf;
            } else {
                used.clear();
                used.set_pedal(true);
                fresh.set_pedal(true);
            }
            let tone = |s: usize, i: usize| 0.05 * (std::f32::consts::TAU * (110.0 + 37.0 * s as f32) * i as f32 / RATE).sin();
            let (mut oa, mut ob) = ([0.0_f32; 2 * BLOCK], [0.0_f32; 2 * BLOCK]);
            for b in 0..200 {
                for f in [&mut used, &mut fresh] {
                    match b {
                        2 => {
                            if let Some(pl) = f.place(52) {
                                f.take(pl, 52);
                            }
                        }
                        60 => f.key_up(52),
                        90 => f.set_pedal(false),
                        _ => {}
                    }
                }
                oa.fill(0.0);
                ob.fill(0.0);
                let drive = |s: usize, i: usize| if b < 120 { tone(s, i + b * BLOCK) } else { 0.0 };
                render(&mut used, &k, &drive, &mut oa);
                render(&mut fresh, &k, &drive, &mut ob);
                for i in 0..2 * BLOCK {
                    assert!(oa[i].to_bits() == ob[i].to_bits(), "trial {trial} ({}): the fretboard differs from a new one at block {b}, sample {i} ({:e} against {:e})", if hushing { "hushed" } else { "cleared" }, oa[i], ob[i]);
                }
                assert_eq!(used.active(), fresh.active(), "trial {trial}: one board is awake and the other asleep at block {b}");
            }
        }
    }

    /// **The fretboard renders the same however its frames are split.**
    /// Twelve histories of strings taken, let go, held by the pedal and
    /// handed back by voices ending part way into a block (`voice_ended`),
    /// every owned string's bus driven, the hand off the keys for long
    /// stretches: rendered in blocks of 128 frames, and again in blocks of
    /// random length, every event on its own frame in both. The two agree to
    /// the bit, sample by sample, and on whether the board is awake at every
    /// event. Run once a call, the hand's rest, a taken string's closing and
    /// the handback all followed the blocks (`BoardCtl`).
    #[test]
    fn the_fretboard_renders_the_same_however_its_frames_are_split() {
        let mut seed = 0x51f3_9a27_u32;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32
        };
        // A voice ending is the voice of whichever key holds that string.
        #[derive(Clone, Copy)]
        enum Ev {
            Take(u8),
            Up(u8),
            Pedal(bool),
            Ended(usize),
        }
        // Each string's drive a function of the frame alone, so that every
        // split hands the board the same buses.
        let drive = |s: usize, f: usize, level: f32| {
            let mut z = (f as u32).wrapping_mul(0x9e37_79b9) ^ (s as u32).wrapping_mul(0x85eb_ca6b);
            z ^= z >> 15;
            z = z.wrapping_mul(0x2c1b_3c6d);
            z ^= z >> 12;
            level * (z as f32 / u32::MAX as f32 - 0.5)
        };
        for trial in 0..12 {
            let frames = 48_000;
            let level = 10.0_f32.powf(-1.5 * rnd());
            // Events on frames of their own, in order; a voice ends on a
            // frame where its key may be held or the pedal holding it.
            let mut events: Vec<(usize, Ev)> = Vec::new();
            let mut f = 0;
            while f < frames {
                f += 1 + (rnd() * 3000.0) as usize;
                let pitch = LOWEST + (rnd() * f32::from(HIGHEST - LOWEST)) as u8;
                let e = match (rnd() * 8.0) as u32 {
                    0..=2 => Ev::Take(pitch),
                    3 | 4 => Ev::Up(pitch),
                    5 => Ev::Pedal(rnd() > 0.5),
                    _ => Ev::Ended((rnd() * STRINGS as f32) as usize % STRINGS),
                };
                events.push((f, e));
            }
            let k = knobs();
            let render = |sizes: &mut dyn FnMut() -> usize| {
                let mut board = Fretboard::new(RATE);
                let (mut l, mut r) = (vec![0.0_f32; frames], vec![0.0_f32; frames]);
                let mut awake = Vec::new();
                let (mut at, mut next) = (0, 0);
                while at < frames {
                    // A voice's end is told before the block it falls in,
                    // with how far into that block it is; every other event
                    // lands between blocks, on its frame.
                    while next < events.len() && events[next].0 <= at {
                        match events[next].1 {
                            Ev::Take(p) => {
                                if let Some(pl) = board.place(p) {
                                    board.take(pl, p);
                                }
                            }
                            Ev::Up(p) => board.key_up(p),
                            Ev::Pedal(d) => board.set_pedal(d),
                            Ev::Ended(_) => {}
                        }
                        awake.push(board.active());
                        next += 1;
                    }
                    let mut m = sizes().clamp(1, BLOCK).min(frames - at);
                    let mut ends = [usize::MAX; STRINGS];
                    let mut i = next;
                    while i < events.len() && events[i].0 <= at + m {
                        match events[i].1 {
                            Ev::Ended(s) => ends[s] = ends[s].min(events[i].0 - at),
                            _ => {
                                m = m.min(events[i].0 - at);
                                break;
                            }
                        }
                        i += 1;
                    }
                    while next < events.len() && matches!(events[next].1, Ev::Ended(_)) && events[next].0 <= at + m {
                        next += 1;
                    }
                    // Each held string's voice drives its bus up to the
                    // frame it ends on, and is told of its end after.
                    board.set_knobs(&k);
                    board.begin(m);
                    for s in 0..STRINGS {
                        if board.owner[s].is_some() {
                            for (j, b) in board.bus(s, m).iter_mut().enumerate().take(ends[s]) {
                                *b += drive(s, at + j, level);
                            }
                        }
                    }
                    for (s, &end) in ends.iter().enumerate() {
                        if let (true, Some(p)) = (end != usize::MAX, board.owner[s]) {
                            board.voice_ended(p, end);
                        }
                    }
                    board.render(&mut l[at..at + m], &mut r[at..at + m], m);
                    at += m;
                }
                (l, r, awake)
            };
            let whole = render(&mut || BLOCK);
            let split = render(&mut || 1 + (rnd() * 1.2 * BLOCK as f32) as usize);
            for i in 0..frames {
                assert!(whole.0[i].to_bits() == split.0[i].to_bits() && whole.1[i].to_bits() == split.1[i].to_bits(), "trial {trial}: the split take parts from the 128-frame one at frame {i} ({:e} against {:e})", split.0[i], whole.0[i]);
            }
            assert_eq!(whole.2, split.2, "trial {trial}: the board was awake at an event in one take and asleep in the other");
        }
    }

    /// **A string that is not a number falls asleep.** One block of NaN on
    /// a held string's bus, what a voice's NaN onset puts there, and then
    /// nothing: the open strings and the air have all taken it up, and once
    /// no bus drives them they are put to sleep and cleared as any string
    /// under the floor is (measured 24 ms after the drive stopped, the NaN's
    /// own block counted as quiet: printed), and the next key renders
    /// finite. Read as over the floor, the NaN kept the board awake and
    /// non-finite for good (verifier: the guitar after two silent presses of
    /// one key, non-finite and awake 10 s on; 0d20e3d1's state test let such
    /// a string sleep).
    #[test]
    fn a_string_that_is_not_a_number_falls_asleep() {
        let mut board = Fretboard::new(RATE);
        board.set_knobs(&knobs());
        let (mut l, mut r) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK]);
        let held = board.place(52).expect("E3 is on the fingerboard");
        board.take(held, 52);
        board.begin(BLOCK);
        board.bus(held.string, BLOCK).fill(f32::NAN);
        board.render(&mut l, &mut r, BLOCK);
        assert!(l.iter().chain(&r).any(|v| !v.is_finite()), "the NaN never reached the strings");
        board.voice_ended(52, BLOCK);
        board.key_up(52);
        let mut blocks = 0;
        while board.active() && blocks < (RATE as usize) / BLOCK {
            l.fill(0.0);
            r.fill(0.0);
            board.begin(BLOCK);
            board.render(&mut l, &mut r, BLOCK);
            blocks += 1;
        }
        let secs = (blocks * BLOCK) as f64 / f64::from(RATE);
        println!("a NaN on the strings: the board asleep {secs:.4} s after its drive stopped");
        assert!(!board.active(), "a NaN kept the board awake {secs:.3} s with nothing driving it");
        assert!(secs <= 2.0 * QUIET_SECS + 0.01, "the board slept {secs:.3} s after its drive stopped");
        let next = board.place(57).expect("A3 is on the fingerboard");
        board.take(next, 57);
        for b in 0..20 {
            l.fill(0.0);
            r.fill(0.0);
            board.begin(BLOCK);
            for (i, v) in board.bus(next.string, BLOCK).iter_mut().enumerate() {
                *v = 1.0e-3 * ((b * BLOCK + i) as f32 * 0.05).sin();
            }
            board.render(&mut l, &mut r, BLOCK);
            assert!(l.iter().chain(&r).all(|v| v.is_finite()), "the next key rendered non-finite (block {b})");
        }
    }
}
