//! The electric piano: a steel tine, a tonebar bolted beside it, and a magnet
//! listening to the tine move.
//!
//! **Why this is not the piano with different numbers.** A piano string is
//! flexible and under enormous tension, so its partials are very nearly
//! whole-number multiples and a delay line models it beautifully. A tine is a
//! stiff steel rod clamped at one end — a tuning fork's arm — and a stiff bar's
//! modes are nowhere near harmonic: the textbook clamped-free ratios run
//! 1 : 6.27 : 17.55. A waveguide cannot make those, because a delay line
//! produces harmonics by construction. So this file is a bank of resonators
//! instead, one per mode, each free to sit wherever the metal actually puts it.
//! That single decision is the difference between an electric piano and a
//! detuned piano with the tone knob down.
//!
//! **Where the sound actually comes from.**
//! - The TINE gives the pitch and the chime. Its second mode — the glassy
//!   partial a light touch is almost entirely made of — dies in well under a
//!   second while the fundamental rings for many, and that ratio IS the sound.
//!   Where the second mode sits comes from the beam ([`super::beam`]): a bar
//!   with the tuning spring's mass on its tip, scheduled across the keyboard.
//!   This file used to carry two more modes at exactly 2·f0 and 3·f0, a
//!   piano's harmonics on a fork's arm; they were there because the old pickup
//!   supplied almost no harmonics of its own — the bench measured the whole
//!   BARK knob moving the second harmonic by 2.3 dB at fortissimo — and they
//!   carried 18 of the 20 dB of second harmonic this instrument had. They are
//!   gone. A clamped bar has no such modes, and the pickup below makes the
//!   harmonics honestly.
//! - The TONEBAR is the fork's other prong: a bar tuned a hair above the tine,
//!   bolted into the same block, exchanging energy with it. It is not a second
//!   oscillator added to the sum — an addition cannot exchange anything — it
//!   is the other half of a two-body coupled pair, solved at the strike. The
//!   beat under a held note and the two-stage decay (a fast prompt sound, a
//!   slow aftersound as the bar hands its energy back) fall out of that
//!   solve; nothing posts them in.
//! - The PICKUP is a magnet, and it is a transducer, not a waveshaper: the
//!   coil reads the time derivative of the flux, the flux is a curve of where
//!   the tine tip is, and the tip rests some way off the pole's axis
//!   ([`super::pickup`]). Everything that used to be written here about
//!   "inverse square of the gap" and `x / (1 − s·x)³` was a saturator with the
//!   right flavour at one swing and no idea what a pickup is; measured against
//!   the real curve it kept the third harmonic 6–14 dB under the second where
//!   the real transducer puts them level. The dirt still depends on how far
//!   the tine is moving, so it arrives when the player hits hard and is
//!   simply absent when they do not — that is why it lives here, on the
//!   displacement, and not in an overdrive after the fact.

use super::beam::{self, BeamRatios};
use super::pickup::{Curve, Transducer};
use super::EpKnobs;

/// The pickup's geometry in the evidence's units: the tine tip rides an arc of
/// six gaps' radius past a pole one gap away. `evidence/pickup.py` measured
/// every table in the brief with exactly this pair, so a displacement handed
/// to the transducer here is in the same gaps those tables are in.
const GAP: f32 = 1.0;
const ARC: f32 = 6.0;

/// The strike's budget at full velocity, in gaps of the coil-referred sum,
/// of which the fundamental's displacement is 0.54 at fortissimo (0.44 gap)
/// and 0.73 feathered — see the normalisation in `strike_based`.
///
/// **A fundamental of 0.44 gap, from the transducer's own table, and not
/// more.** At the
/// reference regulation this is where the real curve's dirt lives — measured
/// on a sinusoid: second harmonic −22 dB and third −13 dB re the fundamental
/// at a swing of 0.5, −16/−9 at 0.6, −2/+0.5 at 1.0. The ceiling is the other
/// end of the BARK knob: with the tine lifted to 0.3 gap from the pole's
/// axis a swing of 0.5 puts the second harmonic at −4.0 dB, 0.6 at −1.5 and
/// 0.7 at +0.8 — the octave overtaking the note, which is the trap the
/// whole pickup file is built around. Half a gap keeps 4 dB of daylight at
/// the dirtiest regulation the knob can reach and still crosses the axis at
/// fortissimo there, which is the two-events-per-cycle growl.
const SWING_GAP: f32 = 0.82;

/// How far the BARK knob moves the tine, in gaps, over each half of its
/// travel — the knob is a length now, the tine-height screw.
///
/// **Toward the pole is more bark; the tables say so.** From the reference
/// 0.5 gap down to 0.3, at a fortissimo swing of half a gap the second
/// harmonic goes −22 → −11 → −4 dB and the third −13 → −8 → −8 dB, and from
/// mezzo-forte up the tip crosses the pole's axis: both harmonics arrive
/// with force, which is what a bark is. The other half of the travel sinks
/// the tine to 0.7 gap, into the tail of the curve where it is monotonic:
/// a gentle second harmonic that barely follows velocity (−19 dB at a
/// mezzo-forte swing, −14 at fortissimo), no third at all, and half a
/// decibel less level — a Rhodes regulated low and warm. Two tenths of a
/// gap either way; the lower end stops well above [`super::pickup::OFFSET_FLOOR`],
/// and the upper end is where a full-velocity swing still leaves the
/// fundamental on top (see [`SWING_GAP`]).
const BARK_REACH: f32 = 0.2;

/// Half the BARK knob's travel in octaves of `EpKnobs::bark`: the knob
/// arrives as `2^(±1.2)`, and this turns it back into a signed fraction of
/// the travel so that [`BARK_REACH`] can be a length.
const BARK_OCTAVES: f32 = 1.2;

/// The tine's taper, root to tip, as the beam solve takes it: width and
/// thickness at the tip as a fraction of the root.
///
/// **One taper for the whole keyboard, chosen for what Euler–Bernoulli
/// cannot see.** A Rhodes tine is a round rod of one diameter, so this is
/// not a description of a file; it stands in for the shear and rotary
/// inertia of a short thick treble tine and for whatever the root's clamp
/// does, all of which pull the second mode DOWN from the textbook 6.27, and
/// which the tapered beam is the honest lever for. This pair puts a bare,
/// spring-free tine at 4.50 — where the top of the keyboard sits (the old
/// linear schedule extrapolated to 4.2 there, a number nobody measured) —
/// and lets the tuning spring's mass carry everything above it.
const TAPER_WIDTH: f32 = 0.5;
const TAPER_THICK: f32 = 0.6;

/// The tuning spring's mass as a fraction of the tine's, at C4 and per key.
///
/// **Why C4 is 5.64, and was already.** The file's old "bell" schedule,
/// `6.6 − 0.030·(k − 28)`, put the second mode at 5.64·f0 on middle C: ten
/// per cent under the uniform beam's 6.27, which is exactly where a lightly
/// tapered bar carrying a light tip mass sits. It was in the right place
/// for a reason the old comment did not state: that partial is not a bell,
/// it is the bar's own second mode. So it stays there, by solving the beam
/// for the spring mass that puts it there — 0.1615 with the taper above,
/// bisected — and the spring shrinks toward the treble, 0.0035 of a tine
/// mass per key, because a bass tine carries a big spring on a long rod and
/// a treble tine a small one on a short rod, and the ratio still climbs
/// toward the bass. The schedule is linear because measured against the old
/// one it IS the old one: A0 6.77 against 6.81, E1 6.58 against 6.60, E4
/// 5.52 against 5.52, C6 4.76 against 4.80; only above C7, where the spring
/// has run out, does the bar's own 4.50 replace the extrapolated 4.2.
const SPRING_C4: f32 = 0.1615;
const SPRING_PER_KEY: f32 = 0.0035;

/// The most samples a strike's force pulse is delivered over.
///
/// **The strike is a pulse, not an impulse, because the pickup is a coil.**
/// A kicked mode's velocity jumps in one sample, and a coil differentiates
/// that jump into a step at the output — measured: a fresh strike's first
/// difference was 22 times the old memoryless pickup's (0.675 against
/// 0.030 at the same peak), a click on every note. A hammer tip is in
/// contact for a millisecond or more, and the tine's velocity rises over
/// that time. So the seed is delivered through a force pulse instead: one
/// that rises from nothing and decays, `t·e^{−t/τ}`, with `τ = contact/2π`
/// — the time constant whose first-order spectrum, `1/√(1 + (f/fc)²)` at
/// `fc = 1/contact`, is exactly the contact roll-off this file has always
/// voiced with. Not a one-sided exponential, which is that spectrum's own
/// pulse: it starts at its maximum, and the coil still read an edge from
/// it (a first sample of 17 % of the velocity at fortissimo, second
/// difference equal to the first); the rising shape's first sample is
/// under 3 %, and it has no spectral nulls to land on a partial the way a
/// raised cosine once did. Each mode's seed is divided by the truncated
/// pulse's exact magnitude at its frequency, so the amplitude it settles
/// at is the voiced one to the bit: the pulse changes when a note starts,
/// not what it is.
///
/// Six τ at the longest contact (2.6 ms feathered, times the softest tip's
/// 1.9) is 4.7 ms, 227 samples at 48 kHz; 256 leaves room for 44.1 and
/// caps 96 kHz at 3.4 τ, where the exact division still lands the
/// amplitude and only the last tenth of the pulse's tail is cut.
const PULSE_MAX: u32 = 256;

/// The damper's time constant as the bench tuned it. Longer than the piano's
/// because a Rhodes damper is a felt pad on a steel bar, not on wire; the
/// RELEASE knob scales this and nothing else.
const DAMP_SECS: f32 = 0.085;

/// Modes per voice: the coupled pair (tine side and bar side), then the
/// bar's second, third and fourth modes. More are inaudible under the
/// pickup's own rolloff.
const MODES: usize = 5;
/// Where each lives in the bank.
const TINE: usize = 0;
const BAR: usize = 1;
const MODE2: usize = 2;

/// What the transducer's output is multiplied by so that the velocity table
/// the bench signed off keeps its level.
///
/// **One gain, measured.** The old pickup put out about one unit per unit of
/// displacement at its positive extreme, with the displacement normalised so
/// a full-velocity strike swung 0.92 in all. The transducer puts out at most
/// one gap per gap of displacement, and a full-velocity fundamental swings
/// half a gap of it. The ratio of those, corrected on the bench so that the
/// E4 velocity ladder lands on the old table — see
/// `the_velocity_table_keeps_its_shape` for the twelve numbers and the
/// worst deviation — is this.
const OUT_GAIN: f32 = 1.22;

/// What separates the two shipped Rhodes before any knob moves: the
/// regulation of the harp. The knobs multiply these, and 0.5 on every knob
/// lands exactly here — two instruments with their own bench signatures,
/// not two presets of one. The amp differences live in `mod.rs` on top.
#[derive(Clone, Copy)]
pub struct TineBase {
    /// Where the tine tip rests below the pole's axis at BARK centre, in
    /// gaps: the tine-height screw as the factory left it.
    pub height: f32,
    /// The tonebar's coupling to the tine at TONEBAR centre, as a fraction
    /// of the tine's own stiffness (`κ / ω₁²`).
    pub coupling: f32,
    /// How far above the tine the bar is tuned, as a fraction of frequency,
    /// before the coupling pushes the two apart.
    pub detune: f32,
    /// How long the bar rings on its own relative to the tine: a heavy bar
    /// in a rubber grommet against a thin rod in a felt-lined clamp.
    pub bar_ring: f32,
}

impl TineBase {
    /// The instrument the bench signed off, and the evidence's reference
    /// regulation: the tine at half a gap, where `evidence/pickup.py`
    /// measured every table.
    ///
    /// **The coupling and the detune together are the old beat.** The old
    /// bar was a free sinusoid 0.18 % above the tine at 0.42 of its level,
    /// which beat at 0.0018·f0 and crossed the tine's decay a second and a
    /// half in. In the coupled pair the beat rate is `f0·√(ε² + c²)` and the
    /// bar side's share of the tine's motion is `sin²θ` with
    /// `tan 2θ = c/ε`; 0.00072 and 0.00165 put the beat at the same
    /// 0.0018·f0 (0.59 Hz on E4) with the bar side at 0.30 of the motion —
    /// the old bar's 0.42 in amplitude, near enough — so a held note at the
    /// centre swells at the rate it always did. The bar rings three tines
    /// long: with the pair's decays mixed by the same angle the aftersound
    /// runs out to about 7 s on E4 against the tine's 3.8, which is a knee a
    /// listener can hear before the note is gone.
    pub const MK1: TineBase = TineBase {
        height: 0.5,
        coupling: 0.001_65,
        detune: 0.000_72,
        bar_ring: 3.0,
    };
    /// The suitcase harp, regulated a hair higher: the tine at 0.45 gap is
    /// the flux curve's inflection (steepest at 0.4675, measured), where the
    /// second harmonic is at its quietest under a light touch — −57 dB
    /// against the Mk.1's −53 — and the second and third arrive together
    /// from mezzo-forte up (−15/−11 dB at a swing of half a gap against
    /// −22/−13): cleaner feathered, dirtier leaned on, which is the
    /// instrument people carry to the funk gig. Its bar is coupled a third
    /// harder, so the beat runs faster and the aftersound is stronger.
    pub const SUITCASE: TineBase = TineBase {
        height: 0.45,
        coupling: 0.002_2,
        detune: 0.000_72,
        bar_ring: 3.0,
    };
}

/// One mode of the metal, as a decaying sinusoid.
///
/// **Kicked, not driven.** The obvious way to build a modal bank is a row of
/// resonant filters with the excitation poured through them, and it is a trap:
/// a two-pole resonator sharp enough to ring for nine seconds has a gain at
/// its own frequency of about a hundred thousand, so the first version of this
/// file put out ninety decibels over full scale. The fix is not to normalise
/// that gain. It is to stop driving the filter at all — a mode of a struck bar
/// IS a decaying sinusoid, so the strike sets its amplitude directly and the
/// recursion below just plays it out.
///
/// Two multiplies and two adds per mode per sample, no input, no gain to get
/// wrong, and — the part that matters for this instrument — the amplitude of
/// every partial is a number the strike chooses rather than the accidental
/// product of a filter's response. The whole velocity design depends on being
/// able to say exactly how loud the chime is relative to the fundamental.
#[derive(Clone, Copy, Default)]
pub(super) struct Mode {
    y1: f32,
    y2: f32,
    /// 2 * r * cos(w)
    b1: f32,
    /// -r^2
    b2: f32,
}

impl Mode {
    /// Start this mode ringing at `amp`, decaying 60 dB in `t60` seconds.
    ///
    /// The seed values are the first two samples of `amp * r^n * sin(w n)`;
    /// the recursion generates every sample after that exactly.
    pub(super) fn kick(&mut self, freq: f32, t60: f32, rate: f32, amp: f32) {
        if freq <= 0.0 || freq >= rate * 0.47 || amp == 0.0 {
            self.b1 = 0.0;
            self.b2 = 0.0;
            self.y1 = 0.0;
            self.y2 = 0.0;
            return;
        }
        let w = std::f32::consts::TAU * freq / rate;
        let r = 10.0_f32
            .powf(-3.0 / (t60.max(0.01) * rate))
            .clamp(0.0, 0.999_99);
        self.b1 = 2.0 * r * w.cos();
        self.b2 = -(r * r);
        self.y2 = 0.0;
        self.y1 = amp * r * w.sin();
    }

    /// `kick`, for a mode that may already be ringing: the strike is ADDED
    /// to the motion, which is what a hammer does to a bar that is still
    /// moving. It does not stop it and start over.
    ///
    /// **The recursion is linear, so the sum has an exact state.** Two
    /// strikes on one mode are two sequences added, and a fresh sequence's
    /// state is `(y2 = 0, y1 = seed)`, so with the coefficients unchanged the
    /// superposition is `y1 += seed` with `y2` untouched — the same output
    /// as running both modes side by side and summing, to the bit. When the
    /// coefficients DO change under a ringing mode (a knob moved between two
    /// strikes of one note) the new pair is installed and the seed added
    /// anyway: the output stays continuous, because the state is whatever
    /// it was and the next sample follows from it, but it is no longer the
    /// exact sum of two strikes. There is no physical answer to a tine whose
    /// stiffness changed mid-ring either. A silent mode takes the plain
    /// `kick` path, so a first strike from silence is `kick` to the bit.
    ///
    /// This replaces `kick` at every strike, and here is why: `kick`
    /// re-seeded a ringing mode — `y1` and `y2` set to the first two samples
    /// of a NEW sinusoid — which is a step from wherever the old one was to
    /// wherever the new one starts. Loudest under the pedal, because that is
    /// when the old one is still loud; the owner heard it as a pop on every
    /// consecutive strike.
    pub(super) fn kick_add(&mut self, freq: f32, t60: f32, rate: f32, amp: f32) {
        if self.silent() {
            self.kick(freq, t60, rate, amp);
            return;
        }
        if freq <= 0.0 || freq >= rate * 0.47 || amp == 0.0 {
            // Nothing to add. A strike that puts no energy into a partial
            // does not silence the partial that was already there.
            return;
        }
        let w = std::f32::consts::TAU * freq / rate;
        let r = 10.0_f32
            .powf(-3.0 / (t60.max(0.01) * rate))
            .clamp(0.0, 0.999_99);
        self.b1 = 2.0 * r * w.cos();
        self.b2 = -(r * r);
        self.y1 += amp * r * w.sin();
    }

    /// Install new coefficients under whatever is ringing, adding nothing:
    /// the mode keeps its motion and from the next sample decays at `t60`.
    ///
    /// **This is what a damper does to metal.** A felt pad landing on a
    /// tine does not turn the tine down, it takes energy out of it faster —
    /// a loss, in the metal. The old file kept the damper as a gain on the
    /// OUTPUT and faded it, which worked with a memoryless pickup and
    /// cannot with a transducer: folding a fading gain back into the motion
    /// at the next strike (the old landing rule) is a step in DISPLACEMENT,
    /// and a coil differentiates a step into one sample of the whole step
    /// over `ω₀` — thirty times the step, a pop on every note re-struck
    /// mid-release. A loss installed under the state steps nothing: the
    /// state is continuous, only the ratio of one sample to the last
    /// changes, and it changes by a few parts in ten thousand. Same
    /// arithmetic `kick_add` already relies on when a knob moves under a
    /// ringing note.
    pub(super) fn set_ring(&mut self, freq: f32, t60: f32, rate: f32) {
        if self.silent() || freq <= 0.0 || freq >= rate * 0.47 {
            return;
        }
        let w = std::f32::consts::TAU * freq / rate;
        let r = 10.0_f32
            .powf(-3.0 / (t60.max(0.01) * rate))
            .clamp(0.0, 0.999_99);
        self.b1 = 2.0 * r * w.cos();
        self.b2 = -(r * r);
    }

    /// Install this mode's coefficients, silent or ringing, adding nothing,
    /// and hand back what one unit of amplitude is in state units — the
    /// `r·sin w` that `kick` would plant in `y1` — so that the caller can
    /// deliver a strike over time through [`Self::nudge`] rather than in
    /// one sample. Zero for a frequency the mode cannot hold, and then the
    /// coefficients are left alone, so a strike with nothing for a partial
    /// does not silence the partial that was already there.
    pub(super) fn prime(&mut self, freq: f32, t60: f32, rate: f32) -> f32 {
        if freq <= 0.0 || freq >= rate * 0.47 {
            return 0.0;
        }
        let w = std::f32::consts::TAU * freq / rate;
        let r = 10.0_f32
            .powf(-3.0 / (t60.max(0.01) * rate))
            .clamp(0.0, 0.999_99);
        self.b1 = 2.0 * r * w.cos();
        self.b2 = -(r * r);
        r * w.sin()
    }

    /// Add to the motion now: one sample's worth of the superposition
    /// `kick_add` does all at once. The recursion is linear, so a seed
    /// delivered in pieces over successive samples is the impulse response
    /// convolved with the pieces — which is how a force PULSE, rather than
    /// an impulse, reaches a mode.
    pub(super) fn nudge(&mut self, dy: f32) {
        self.y1 += dy;
    }

    pub(super) fn silent(&self) -> bool {
        self.y1 == 0.0 && self.y2 == 0.0
    }

    /// The amplitude this mode is ringing at now, read off its two-sample
    /// state: for `A sin(w n + phi)`, `A^2 sin^2 w = y1^2 + y2^2 - 2 y1 y2
    /// cos w`, and `cos w` is `b1 / 2r` — everything the mode already
    /// carries, nothing stored for the purpose. The pickup's calibration
    /// asks this of the pair on a landing strike. Zero for a silent or
    /// degenerate mode.
    pub(super) fn amplitude(&self) -> f32 {
        let r = (-self.b2).max(0.0).sqrt();
        if r <= 1.0e-6 {
            return 0.0;
        }
        let c = (self.b1 / (2.0 * r)).clamp(-1.0, 1.0);
        let s2 = 1.0 - c * c;
        if s2 <= 1.0e-8 {
            return 0.0;
        }
        let a2 = self.y1 * self.y1 + self.y2 * self.y2 - 2.0 * self.y1 * self.y2 * c;
        (a2.max(0.0) / s2).sqrt()
    }

    /// Scale the motion. What a damper has taken out of the metal, made
    /// real in the mode — the reed piano's landing rule; the tine no longer
    /// needs it, see `set_ring`.
    pub(super) fn scale(&mut self, g: f32) {
        self.y1 *= g;
        self.y2 *= g;
    }

    /// The most of a ringing mode ONE landing hammer may take out of it, as
    /// a fraction of the amplitude it found there.
    ///
    /// **A tip is not a damper.** The seed is an instantaneous impulse, and
    /// an impulse landing exactly against the motion cancels it outright —
    /// arithmetic a felt or neoprene tip cannot do, because it is in contact
    /// for a couple of milliseconds, which around middle C is most of a
    /// period: it meets the metal coming at it, is pushed back, and
    /// re-contacts on the way out. What it can do is bleed the mode. HALF
    /// ITS ENERGY is the most any one landing takes, which is this factor on
    /// the amplitude — and half is generous, not stingy: it is the whole of
    /// what the contact could couple if it were perfectly timed against the
    /// motion.
    ///
    /// Without this the ENERGY CAP made the cancellation worse rather than
    /// better. Pinning a repeated note's fundamental at `cap` — a little
    /// over one strike's own throw — parks it on the knife edge where the
    /// next seed, which is one strike's own throw, very nearly annihilates
    /// it: the bench measured a repeated Rhodes note alternating fourteen
    /// decibels up and down, a note that vanishes every other strike, which
    /// is exactly as wrong as the step the cap was added to prevent.
    const LANDING_FLOOR: f32 = std::f32::consts::FRAC_1_SQRT_2;

    /// How much of a `kick_add` of `amp` this ringing mode may take while
    /// staying between `LANDING_FLOOR` of the amplitude it has now and
    /// `cap`, as a fraction of the seed, 0 to 1.
    ///
    /// **Solved on the state, not guessed from a level.** With `s` added to
    /// `y1`, the squared amplitude is a parabola in `s`: `Q(s) = Q0 + 2 s b
    /// + s^2` with `b = y1 - y2 cos w`. The sign of `b` is the phase of the
    /// motion against the hammer: a hammer pushing WITH the motion raises
    /// `Q` and is allowed up to the larger of the cap and what was already
    /// there, and a hammer pushing AGAINST it lowers `Q` — it is taking
    /// energy out, which is what a real hammer does to a tine coming at it
    /// — until it reaches the floor above, at which point the tip has given
    /// what it has. Both bounds are solved for the seed that lands exactly
    /// on them, so neither steps: a tine at the cap struck in phase is
    /// simply not pushed further, and a tine struck dead against its motion
    /// is quietened rather than switched off.
    pub(super) fn seed_within(&self, freq: f32, rate: f32, amp: f32, cap: f32) -> f32 {
        if self.silent() || amp <= 0.0 || cap <= 0.0 {
            return 1.0;
        }
        let w = std::f32::consts::TAU * freq / rate;
        let (sw, cw) = w.sin_cos();
        let r = (-self.b2).max(0.0).sqrt();
        if sw <= 1.0e-6 || r <= 1.0e-6 {
            return 1.0;
        }
        let s = amp * r * sw;
        let q0 = self.y1 * self.y1 + self.y2 * self.y2 - 2.0 * self.y1 * self.y2 * cw;
        let b = self.y1 - self.y2 * cw;
        let bound = q0.max(cap * cap * sw * sw);
        let floor = q0 * Self::LANDING_FLOOR * Self::LANDING_FLOOR;
        // Only the seed the strike actually asks for is judged, not the road
        // it took to get there: a hammer big enough to carry the mode THROUGH
        // the cancellation and out the far side is a hammer that landed, and
        // clamping it because the parabola dipped on the way would gut every
        // hard strike on a nearly-dead note.
        let q = q0 + 2.0 * s * b + s * s;
        if q > bound {
            let s_hi = -b + (b * b - q0 + bound).max(0.0).sqrt();
            return (s_hi / s).clamp(0.0, 1.0);
        }
        if q < floor {
            // The floor bites on the way DOWN, so it is the SMALLER root —
            // the last seed still above it. Reachable only with `b < 0`, a
            // hammer against the motion, which is the only way `q` can be
            // under `q0` at all. Both sides of the clamped band land on
            // `floor` exactly, so the amplitude this returns is continuous
            // in the strike: no velocity has a cliff on either side of it.
            let s_lo = -b - (b * b - q0 + floor).max(0.0).sqrt();
            return (s_lo / s).clamp(0.0, 1.0);
        }
        1.0
    }

    pub(super) fn tick(&mut self) -> f32 {
        let y = self.b1 * self.y1 + self.b2 * self.y2;
        let y = if y.is_finite() { y } else { 0.0 };
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub(super) fn clear(&mut self) {
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// The tine and the tonebar as one system, solved: the two eigenmodes of
/// the coupled pair, each as the tine's share of it. Index 0 is the mode the
/// tine mostly owns and the note is tuned to; index 1 the one the bar mostly
/// owns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pair {
    /// Eigenfrequencies, Hz, tuned so that `freq[0]` is the note.
    pub freq: [f32; 2],
    /// Eigen-decays, seconds to −60 dB.
    pub t60: [f32; 2],
    /// Each mode's share of the tine's motion after a hammer impulse on the
    /// tine alone. They sum to one, so the strike's throw is the strike's
    /// throw whatever the coupling.
    pub share: [f32; 2],
}

/// Solve the two-prong fork.
///
/// ```text
/// ẍ₁ + γ₁ẋ₁ + ω₁²x₁ = κ(x₂ − x₁)      the tine, hammer-driven
/// ẍ₂ + γ₂ẋ₂ + ω₂²x₂ = κ(x₁ − x₂)      the bar, driven only through κ
/// ```
///
/// The stiffness matrix `[[ω₁²+κ, −κ], [−κ, ω₂²+κ]]` is symmetric, so its
/// eigenvectors are orthonormal and closed-form: with `m = (a+b)/2`,
/// `h = (a−b)/2`, `d = √(h²+κ²)` the eigenvalues are `m ∓ d` and the
/// eigenvectors `(κ, h ± d)`, normalised. A hammer impulse on the tine
/// alone lands on each eigenmode in proportion to that mode's tine
/// component, and the tine's own motion is read back through the same
/// component, so each mode's share of what the pickup sees is that
/// component SQUARED — which is why the shares sum to one and the pickup's
/// calibration can trust the strike's throw.
///
/// **The damping is projected, not solved, and here is the size of that.**
/// With damping the exact problem is quartic; the light-damping answer is
/// each eigenmode decaying at its own components' mix of the two decay
/// rates, which is what is returned. What it neglects is the off-diagonal
/// term `(γ₁−γ₂)·sinθ·cosθ` coupling the two eigenmodes through damping,
/// and that matters in proportion to itself over the frequency split
/// `Ω₂−Ω₁`. On E4 at the Mk.1 centre the split is 3.7 rad/s and the
/// neglected term is 0.4 s⁻¹ — a tenth, entering the decay rates at second
/// order, so under one per cent on a t60. That is well inside what the
/// bench's Schroeder t60 resolves and far inside what a beat obscures.
///
/// **Tuned after the solve.** The coupling raises BOTH eigenfrequencies by
/// about `κ/2ω₁²` and pushes them apart by `√(ε²+c²)`, so a tine tuned to
/// f0 in isolation would play `c/2` sharp with the bar attached — a couple
/// of cents that would follow the TONEBAR knob. A Rhodes is tuned with its
/// bar on, so the mode the tine mostly owns is set to exactly f0 and the
/// other keeps its solved distance above it.
///
/// A coupling of zero is a tine on dead wood: the solve is degenerate there
/// (both eigenvector components vanish), and the answer is the uncoupled
/// tine at full share with the bar at none, which is also the limit.
pub(super) fn couple(f0: f32, t60_tine: f32, base: &TineBase, coupling: f32) -> Pair {
    let w1 = f64::from(std::f32::consts::TAU * f0);
    let w2 = w1 * (1.0 + f64::from(base.detune));
    let t60_bar = t60_tine * base.bar_ring;
    let kappa = f64::from(coupling.max(0.0)) * w1 * w1;
    if kappa <= 1.0e-12 * w1 * w1 {
        return Pair {
            freq: [f0, (w2 / f64::from(std::f32::consts::TAU)) as f32],
            t60: [t60_tine, t60_bar],
            share: [1.0, 0.0],
        };
    }
    let a = w1 * w1 + kappa;
    let b = w2 * w2 + kappa;
    let mean = 0.5 * (a + b);
    let half = 0.5 * (a - b);
    let disc = (half * half + kappa * kappa).sqrt();
    // Eigenvector of `mean ∓ disc` is `(κ, half ± disc)`; its tine
    // component squared, normalised, is the share.
    let share_of = |e: f64| kappa * kappa / (kappa * kappa + e * e);
    let lo = (mean - disc, share_of(half + disc));
    let hi = (mean + disc, share_of(half - disc));
    // The tine's mode is the one it mostly owns; the note is tuned to it.
    let (tine, bar) = if lo.1 >= hi.1 { (lo, hi) } else { (hi, lo) };
    let tune = w1 / tine.0.sqrt();
    let freq = |lam: f64| (lam.sqrt() * tune / f64::from(std::f32::consts::TAU)) as f32;
    // Modal damping: each eigenmode decays at its components' mix of the
    // two rates, and a 60 dB rate is the reciprocal of a t60.
    let t60_of = |share: f64| {
        let rate = share / f64::from(t60_tine) + (1.0 - share) / f64::from(t60_bar);
        (1.0 / rate) as f32
    };
    Pair {
        freq: [freq(tine.0), freq(bar.0)],
        t60: [t60_of(tine.1), t60_of(bar.1)],
        share: [tine.1 as f32, bar.1 as f32],
    }
}

/// One key of the electric piano.
pub struct EpVoice {
    rate: f32,
    modes: [Mode; MODES],
    /// Each mode's frequency as last struck, so the damper can re-time a
    /// mode without being told again where it is.
    freqs: [f32; MODES],
    /// The magnet and coil. Regulated by BARK at the strike, calibrated
    /// for the swing the strike is going to produce, cleared only when the
    /// voice is silent — read `pickup::Transducer::clear` before touching
    /// that last rule.
    pickup: Transducer,
    /// The note's angular frequency per sample, which the coil's derivative
    /// is referred to; see `pickup::Transducer::tick` for why.
    omega0: f32,
    /// Where the tine rests now and where the last strike asked it to rest.
    /// A strike from silence sets both at once. A strike landing on a
    /// ringing note whose BARK moved between the two strikes GLIDES: the
    /// transducer is regulated to the new height outright, and the
    /// difference is handed to it as displacement, shrinking over a few
    /// milliseconds — `f(x₀ₙₑw + x + (x₀ₙₒw − x₀ₙₑw))` is `f(x₀ₒₗd + x)` at
    /// the first sample, so the flux the coil differentiates is continuous
    /// and the screw turns instead of jumping. A jump of a tenth of a gap
    /// would differentiate to about three, six times full scale.
    x0_now: f32,
    x0_to: f32,
    x0_slew: f32,
    /// The damper's time constant in seconds, set at strike time from the
    /// RELEASE knob.
    damp_secs: f32,
    /// Whether the pad is on the metal: a released note is not thumped
    /// twice, and a strike lifts the pad by re-timing the modes anyway.
    damped: bool,
    env: f32,
    done: bool,
    /// Random sign per strike, so repeated notes are not bit-identical.
    seed: u32,
    /// The damper pad landing: a soft dud that plays when the key is let go.
    thump: Mode,
    /// The passive pickup and cable: a gentle top rolloff, one pole. A real
    /// harp's coil into a real lead has nothing left above the bar's third
    /// mode, and the raw modal sum was the one place this instrument still
    /// sounded like arithmetic.
    pickup_lp: f32,
    pickup_a: f32,
    /// The strike being delivered: each mode's seed in state units, the
    /// pulse's ratio per sample, its normalisation, the running `q^k` and
    /// `k`, and how many samples remain. See `PULSE_MAX`.
    pend: [f32; MODES],
    pulse_q: f32,
    pulse_scale: f32,
    pulse_qk: f32,
    pulse_k: u32,
    pulse_left: u32,
    /// The last beam solve, keyed on the spring mass that asked for it: a
    /// repeated note on one voice is the common case, and ten microseconds
    /// on the audio thread is worth not spending twice. NaN until the first
    /// strike, which matches nothing.
    beam_beta: f32,
    beam_ratios: BeamRatios,
}

/// `|Σ_{k=1}^{n} k·q^k·e^{−jwk}|`: the strike pulse's magnitude at one
/// frequency, before normalisation. Complex arithmetic by hand, because it
/// is four multiplies and the audio thread does not want a crate for it.
fn pulse_magnitude(q: f32, w: f32, n: u32) -> f32 {
    let (sw, cw) = w.sin_cos();
    let z = (q * cw, -q * sw);
    let mul = |a: (f32, f32), b: (f32, f32)| (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0);
    let (snw, cnw) = (w * n as f32).sin_cos();
    let qn = q.powi(n as i32);
    let zn = (qn * cnw, -qn * snw);
    let zn1 = mul(zn, z);
    let nf = n as f32;
    let inner = (1.0 - (nf + 1.0) * zn.0 + nf * zn1.0, -(nf + 1.0) * zn.1 + nf * zn1.1);
    let num = mul(z, inner);
    let one_minus_z = (1.0 - z.0, -z.1);
    let den2 = one_minus_z.0 * one_minus_z.0 + one_minus_z.1 * one_minus_z.1;
    (num.0 * num.0 + num.1 * num.1).sqrt() / den2.max(1.0e-12)
}

impl EpVoice {
    pub fn new(sample_rate: f32) -> Self {
        EpVoice {
            rate: sample_rate,
            modes: [Mode::default(); MODES],
            freqs: [0.0; MODES],
            pickup: Transducer::new(Curve::Magnetic { gap: GAP, arc: ARC }),
            omega0: 0.0,
            x0_now: TineBase::MK1.height,
            x0_to: TineBase::MK1.height,
            // Five milliseconds: under the shortest contact, over any one
            // cycle of the note, so the glide is neither an attack nor a
            // wobble.
            x0_slew: 1.0 - (-1.0 / (0.005 * sample_rate.max(8_000.0))).exp(),
            damp_secs: DAMP_SECS,
            damped: false,
            env: 0.0,
            done: true,
            seed: 0x9E37_79B9,
            thump: Mode::default(),
            pickup_lp: 0.0,
            // Corner near 7.5 kHz: high enough to leave the bar's second and
            // third modes alone, low enough to shave the synthetic edge off
            // the top.
            pickup_a: 1.0 - (-std::f32::consts::TAU * 7_500.0
                / sample_rate.max(8_000.0))
                .exp(),
            pend: [0.0; MODES],
            pulse_q: 0.0,
            pulse_scale: 0.0,
            pulse_qk: 1.0,
            pulse_k: 0,
            pulse_left: 0,
            beam_beta: f32::NAN,
            beam_ratios: BeamRatios {
                f2: 0.0,
                f3: 0.0,
                f4: 0.0,
            },
        }
    }

    pub fn strike(&mut self, pitch: u8, velocity: f32) {
        self.strike_knobs(pitch, velocity, &EpKnobs::default());
    }

    /// `strike` with the whole panel: every knob this mechanism answers to,
    /// on the Mk.1 harp. `strike_based` picks the harp.
    pub fn strike_knobs(&mut self, pitch: u8, velocity: f32, k: &EpKnobs) {
        self.strike_based(pitch, velocity, k, &TineBase::MK1);
    }

    /// `strike` with the panel's VOICING knob: it scales the hammer tip's
    /// contact time, 1.3x at full mellow to 0.77x at full bright, the same
    /// lever velocity itself pulls. The exponent form lands 0.5 on exactly
    /// 1.0, so the shipped default is this file's tuning untouched.
    pub fn strike_voiced(&mut self, pitch: u8, velocity: f32, voicing: f32) {
        let mut k = EpKnobs::default();
        k.voicing = voicing;
        self.strike_based(pitch, velocity, &k, &TineBase::MK1);
    }

    /// The clamped-free bar this key's tine is, solved once per new spring
    /// mass and remembered.
    fn beam_ratios(&mut self, beta: f32) -> BeamRatios {
        if beta != self.beam_beta {
            self.beam_beta = beta;
            self.beam_ratios = beam::clamped_free(beta, TAPER_WIDTH, TAPER_THICK);
        }
        self.beam_ratios
    }

    /// Where the tine rests for this knob and harp, in gaps below the axis.
    fn rest_offset(knobs: &EpKnobs, base: &TineBase) -> f32 {
        // `bark` arrives as 2^(±1.2); its log is the knob's signed travel.
        let travel = knobs.bark.max(1.0e-3).log2() / BARK_OCTAVES;
        base.height - BARK_REACH * travel.clamp(-1.0, 1.0)
    }

    /// `strike` with the whole panel and a harp.
    pub fn strike_based(&mut self, pitch: u8, velocity: f32, knobs: &EpKnobs, base: &TineBase) {
        let voicing = knobs.voicing;
        let f0 = 440.0 * 2.0_f32.powf((f32::from(pitch) - 69.0) / 12.0);
        let v = velocity.clamp(0.0, 1.0);
        let k = f32::from(pitch);
        // The tip's compound, worn soft to fresh hard, as a scale on the
        // contact time. The 1.9 span is measured, not felt-out: at the old
        // 1.3 the chime moved under 3 dB across the knob's whole travel,
        // because the contact rolloff only starts biting the second mode
        // when the width can actually double. 0.5 still lands on exactly 1.
        let felt = 1.9_f32.powf(1.0 - 2.0 * voicing.clamp(0.0, 1.0));

        // **The bar's modes, from the beam.** The tuning spring's mass
        // shrinks toward the treble (see `SPRING_C4`), so the second mode
        // sits around the sixth partial low down and collapses toward the
        // fourth and a half at the top, and the third and fourth modes keep
        // the ratios the beam gives them to the second. CHIME slides the
        // second mode up or down the series and the others with it — the
        // same instrument with a longer or shorter tine, not a different
        // tuning — and the clamp is the range a tine can be, not a limiter.
        let beta = (SPRING_C4 - SPRING_PER_KEY * (k - 60.0)).max(0.0);
        let ratios = self.beam_ratios(beta);
        let mode2 = (ratios.f2 + knobs.chime).clamp(2.2, 9.0);
        let stretch = mode2 / ratios.f2;
        let series = [1.0, 1.0, mode2, ratios.f3 * stretch, ratios.f4 * stretch];

        // Decay. The fundamental rings for many seconds; the second mode is
        // gone in under one, and shorter still in the treble. If those two
        // numbers are the same the instrument sounds like an organ.
        let t60_f = (9.0 * (-(k - 28.0) / 42.0).exp()).clamp(0.9, 11.0) * knobs.decay;
        let t60_f = t60_f.clamp(0.1, 40.0);
        // The chime follows DECAY too, but at its own much shorter scale: if
        // it did not, a long-decay setting would leave the chime ringing at
        // the same length while everything under it grew, which is a bell,
        // not a Rhodes.
        // 1.05 base with the ceiling at 1.2: the earlier 0.75/0.9 pair left
        // the chime at a third of a second in the middle of the keyboard,
        // which reads as a tick riding the note rather than glass over it —
        // the reed piano's lone bar overtone was outliving it, and the two
        // instruments were trading characters. A chime that hangs on for
        // half a second is still gone twenty times faster than the
        // fundamental, which is the ratio the sound IS.
        let t60_bell =
            ((1.05 * (-(k - 28.0) / 46.0).exp()).clamp(0.2, 1.2) * knobs.decay).clamp(0.03, 4.0);

        // **Velocity, mechanism one: contact time.** A hard strike is short and
        // therefore wide-band, so far more of the energy lands in the high
        // modes. The excitation's spectrum is the transform of its contact
        // pulse, which for a raised cosine of length T rolls off from about
        // 1/T — so rather than filter anything, each mode simply takes the
        // amount of energy that pulse actually contains at its frequency.
        // Contact runs about 2.6 ms feathered down to 0.7 ms hard.
        let contact = (2.2 - 1.5 * v.powf(0.7)) * 0.001 * felt;
        let fc = 1.0 / contact;

        // The second mode's own weight on top of that, because the tine's
        // high mode is also driven harder by a stiffer, more compressed tip.
        // Between the two, the chime sits about thirty decibels below the
        // fundamental at pianissimo and level with it at fortissimo, which
        // is the entire reason this instrument can whisper and shout.
        // BELL scales that weight: 0 is a tine with no chime at all, 2 is a
        // glass bell over the note.
        // The 0.16 floor is what keeps the promise above honest at the soft
        // end: at the old 0.045, the contact rolloff and the chime's own
        // decay stacked up to fifty-six decibels under the fundamental by the
        // bench's measure — not a soft chime, no chime — and a feathered
        // Rhodes note is supposed to be almost nothing but glass. The floor
        // cannot buy the whole distance back (a 2 ms contact genuinely has
        // little to give a 1.5 kHz mode — that part is physics, not
        // voicing); this lands pianissimo in the high minus-thirties re the
        // fundamental, which is a chime you hear, from a bench column that
        // used to say there was none.
        let bell_w = (0.16 + 0.89 * v.powf(2.0)) * knobs.bell;
        let weights = [1.0, 0.0, bell_w, bell_w * 0.34, bell_w * 0.11];

        // Each mode's share of the strike, as DISPLACEMENT: its own weight,
        // shaped by what the contact pulse has at that frequency, and then
        // divided by where it sits on the series.
        //
        // **That division is the coil's tilt, taken back out.** The weights
        // above were tuned by ear at the OUTPUT, through a pickup that
        // read displacement; the coil reads velocity, which is 6 dB per
        // octave up the partials of one note — the second mode at 5.6·f0
        // would come out 15 dB louder than the bench signed it, the fourth
        // 30 dB. Physics says the tilt is real; the bench says the chime is
        // voiced. Both are kept by expressing the voiced weights as the
        // displacement that produces them at the coil, which is the weight
        // over the ratio. The tilt still applies to everything the transducer
        // makes on its own, which is where it belongs.
        let mut amps = [0.0_f32; MODES];
        let mut norm = 0.0_f32;
        for i in 0..MODES {
            if i == BAR {
                continue;
            }
            let f = f0 * series[i];
            // First order in AMPLITUDE. Squaring it here (the obvious
            // 1/(1 + (f/fc)^2)) is a twelve-decibel-per-octave weighting, and
            // the bench showed what that costs: at pianissimo it put the
            // chime fifty-two decibels under the fundamental, which is not a
            // soft chime, it is no chime. A Rhodes played gently is still
            // glassy. This is the spectrum of the force pulse the strike is
            // actually delivered through, below — see `PULSE_MAX`.
            let roll = 1.0 / (1.0 + (f / fc) * (f / fc)).sqrt();
            let voiced = weights[i] * roll;
            norm += voiced.abs();
            amps[i] = voiced / series[i];
        }
        // Normalise the strike's budget where the weights were voiced — at
        // the coil — so that the fundamental's share of the throw FALLS as
        // the chime takes more of it: a harder, shorter contact puts the
        // same energy higher up the bar. That is the velocity curve the
        // bench signed (the fundamental at 0.73 of the budget feathered,
        // 0.54 at fortissimo); normalising the displacement instead
        // steepened the ladder by 3.5 dB, measured. The displacement the
        // sum then reaches is what the pickup below is calibrated for.
        let scale = if norm > 1e-9 { 1.0 / norm } else { 0.0 };
        // **The overall swing is what velocity buys.** The exponent is the
        // neoprene tip: rubber gets stiffer as it is compressed, so the
        // amplitude curve is steeper than the velocity that caused it.
        let swing = SWING_GAP * v.powf(1.25);

        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        let jitter = 1.0 + ((self.seed >> 8) as f32 / 16_777_216.0 - 0.5) * 0.03;
        let throw = swing * jitter;

        // **The fork, solved.** TONEBAR is the coupling: 0 is a tine screwed
        // to dead wood — no beat, no aftersound, the note dies at the tine's
        // own rate — and 2 is a bar coupled twice as hard as the factory's,
        // beating twice as fast with the aftersound at four tenths of the
        // motion. (The knob spent one release wired to the PIANO's
        // soundboard send, which the electric instruments hand a hard zero:
        // a placebo. This is the real bar.)
        let pair = couple(f0, t60_f, base, base.coupling * knobs.body);
        let t60s = [
            pair.t60[0],
            pair.t60[1],
            t60_bell,
            t60_bell * 0.68,
            t60_bell * 0.45,
        ];
        let freqs = [
            pair.freq[0],
            pair.freq[1],
            f0 * series[MODE2],
            f0 * series[3],
            f0 * series[4],
        ];
        amps[TINE] *= pair.share[0];
        amps[BAR] = amps[TINE] / pair.share[0].max(1.0e-6) * pair.share[1];

        // **A hammer landing on a tine that is still moving ADDS to the
        // motion.** It does not stop the metal and start it again — that
        // was this function's old behaviour, a re-seed of every mode, and
        // the step from the old motion to the new seed was the pop the owner
        // heard on every consecutive strike, worst under the pedal because
        // that is when the old motion is still loud. Now a strike on a
        // ringing voice is a superposition: `Mode::kick_add`, plus the one
        // thing the linear sum cannot do on its own, below.
        let landing = !self.done;

        // **The energy cap, on the pair.** Superposition stacks: eight
        // strikes at sixty milliseconds, landing in phase, would throw the
        // tine several times further than one hammer can, and into the
        // magnet. A real hammer cannot do that — it cannot overtake a tine
        // already moving faster than it is — so the seed for the tine's own
        // mode is limited to what brings it to `cap`: a little over this
        // strike's own full throw from rest, solved exactly on the mode's
        // state so nothing steps, and the bar's mode takes the same
        // fraction because the two are one impulse. The chime family is
        // left alone: attack colour, gone twenty times faster than the
        // note, and the articulation of a restrike is made of it.
        let full = amps[TINE] * scale * throw;
        let allowed = if landing {
            self.modes[TINE].seed_within(freqs[TINE], self.rate, full, full * 1.25)
        } else {
            1.0
        };

        // What the pair is swinging before this strike lands, for the
        // pickup's calibration below: the excursion it can reach is that
        // plus the strike's own throw.
        let ringing = if landing {
            self.modes[TINE].amplitude() + self.modes[BAR].amplitude()
        } else {
            0.0
        };

        // **The pulse** (see `PULSE_MAX`): `c·k·q^k` for k = 1..len, in
        // closed form for the normalisation and for its magnitude at each
        // mode, `|Σ k z^k| = |z(1 − (n+1)z^n + n z^(n+1))| / |1 − z|²` with
        // `z = q·e^(−jw)`. A strike landing inside another's pulse simply
        // takes it over: what the first had left to deliver is not
        // delivered, and nothing that is ringing is touched.
        let tau = contact / std::f32::consts::TAU;
        let q = (-1.0 / (tau * self.rate)).exp();
        let len = ((6.0 * tau * self.rate).ceil() as u32).clamp(2, PULSE_MAX);
        let n = len as f32;
        let qn = q.powi(len as i32);
        let sum = q * (1.0 - (n + 1.0) * qn + n * qn * q) / ((1.0 - q) * (1.0 - q));
        let c0 = 1.0 / sum.max(1.0e-9);
        for i in 0..MODES {
            let held = if i <= BAR { allowed } else { 1.0 };
            self.freqs[i] = freqs[i];
            let unit = self.modes[i].prime(freqs[i], t60s[i], self.rate);
            let w = std::f32::consts::TAU * freqs[i] / self.rate;
            let gain = c0 * pulse_magnitude(q, w, len);
            let a = amps[i] * scale * throw * held;
            self.pend[i] = if gain > 1.0e-6 { a * unit / gain } else { 0.0 };
        }
        self.pulse_q = q;
        self.pulse_scale = c0;
        self.pulse_qk = 1.0;
        self.pulse_k = 0;
        self.pulse_left = len;

        // **The pickup, regulated and calibrated.** BARK is the tine-height
        // screw (see `BARK_REACH`), and the transducer is normalised for
        // the swing this strike can reach — what the pair was already
        // swinging plus the strike's own throw — so that BARK and velocity
        // add dirt, never level; `pickup::Transducer::calibrate` records
        // the two times that went wrong. A strike from silence regulates
        // outright; a landing glides, see `x0_now`.
        let x0 = Self::rest_offset(knobs, base);
        let excursion = ringing + throw;
        self.pickup.calibrate(excursion);
        self.pickup.set_offset(x0);
        self.x0_to = x0;
        if !landing {
            self.x0_now = x0;
        }
        self.omega0 = std::f32::consts::TAU * f0 / self.rate;

        // The pad comes off the metal: `kick_add` has just re-timed every
        // mode to its undamped ring.
        self.damped = false;
        self.damp_secs = (DAMP_SECS * knobs.release).clamp(0.004, 1.5);
        // The follower keeps whichever is louder: it is the stealing
        // policy's view of the voice, and a soft strike on a loud note has
        // not made the note soft. The figure is the old displacement swing,
        // kept as the priority number it always was.
        let priority = 0.92 * v.powf(1.25);
        self.env = if landing { self.env.max(priority) } else { priority };
        self.done = false;
    }

    /// Silence now: every state cleared, `finished` true. What the frame
    /// wants from a voice it has moved out of the pool and faded, so that
    /// the next strike on it is a first strike from silence exactly.
    pub fn silence(&mut self) {
        for m in &mut self.modes {
            m.clear();
        }
        self.thump.clear();
        self.pickup.clear();
        self.pickup_lp = 0.0;
        self.pulse_left = 0;
        self.damped = false;
        self.env = 0.0;
        self.done = true;
    }

    pub fn release(&mut self, pedal: bool) {
        if !pedal {
            self.land_damper();
        }
    }

    pub fn damp(&mut self) {
        self.land_damper();
    }

    /// The pad lands: a loss installed under every mode, and the dud of
    /// the felt itself, scaled by how loudly the tine was still swinging —
    /// a note that has already died gets no thump, because the pad has
    /// nothing to catch.
    ///
    /// The damped ring is the old output fade's time constant expressed as
    /// a t60 — `ln(1000)` of them — combined with the mode's own loss, so
    /// the note stops at the rate the RELEASE knob always meant.
    fn land_damper(&mut self) {
        if self.done || self.damped {
            return;
        }
        self.damped = true;
        self.thump.kick(88.0, 0.045, self.rate, self.env * 0.35);
        let t60_pad = 6.907_755 * self.damp_secs;
        for (m, &f) in self.modes.iter_mut().zip(self.freqs.iter()) {
            let own = (-m.b2).max(0.0).sqrt();
            // The mode's own t60 from its radius, `r = 10^(−3/(t60·rate))`,
            // then the pad's loss added as rates add.
            let own_t60 = if own > 0.0 && own < 1.0 {
                -3.0 / (own.log10() * self.rate)
            } else {
                40.0
            };
            let t60 = 1.0 / (1.0 / own_t60 + 1.0 / t60_pad);
            m.set_ring(f, t60, self.rate);
        }
    }

    pub fn finished(&self) -> bool {
        self.done
    }

    pub fn level(&self) -> f32 {
        self.env
    }

    pub fn tick(&mut self) -> f32 {
        if self.done {
            return 0.0;
        }
        // The strike arriving: the force pulse, one sample of it.
        if self.pulse_left > 0 {
            self.pulse_k += 1;
            self.pulse_qk *= self.pulse_q;
            let c = self.pulse_scale * self.pulse_k as f32 * self.pulse_qk;
            for (m, &p) in self.modes.iter_mut().zip(self.pend.iter()) {
                m.nudge(p * c);
            }
            self.pulse_left -= 1;
        }
        let mut disp = 0.0;
        for m in &mut self.modes {
            disp += m.tick();
        }

        // **Velocity, mechanism two: the pickup.** The tine's displacement,
        // in gaps, goes through the transducer: the flux curve at the
        // regulated rest position, differentiated, referred to the note's
        // own frequency. Every harmonic this instrument has above the bar's
        // modes is made here, from how far the tine is swinging — and so it
        // arrives with force and is absent without it.
        // The glide is the BARK screw turning under a ringing note; from
        // silence, or with the knob still, it hands over exactly zero.
        self.x0_now += (self.x0_to - self.x0_now) * self.x0_slew;
        let x = disp + (self.x0_now - self.x0_to);
        let out = self.pickup.tick(x, self.omega0) * OUT_GAIN;

        // **Headroom for chords, not for one note.** A single note at full
        // velocity peaks near -7 dBFS, which looks quiet on a meter and is
        // not: five voices of a comped voicing sum, and at a trim that made
        // one note hit 0 dBFS the bench put a four-note chord plus a bass
        // note four decibels INTO the ceiling. An instrument that clips its
        // own output on an ordinary voicing is broken; the channel fader is
        // downstream and cannot undo it.
        // The pickup's own rolloff, then the damper's thump: the pad landing
        // is the damper, and the damper is a loss in the metal now, so there
        // is nothing here for the thump to be outside of.
        self.pickup_lp += self.pickup_a * (out - self.pickup_lp);
        let out = (self.pickup_lp + self.thump.tick()) * 0.45;
        let out = if out.is_finite() { out.clamp(-4.0, 4.0) } else { 0.0 };

        let a = out.abs();
        self.env += (a - self.env) * if a > self.env { 0.4 } else { 0.0006 };
        if self.env < 2.0e-5 {
            self.done = true;
            for m in &mut self.modes {
                m.clear();
            }
            self.thump.clear();
            self.pickup.clear();
            self.pickup_lp = 0.0;
            self.pulse_left = 0;
            self.damped = false;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const RATE: f32 = 48_000.0;
    const E4: u8 = 64;
    const F_E4: f32 = 329.627_56;

    fn knobs_with(f: impl FnOnce(&mut EpKnobs)) -> EpKnobs {
        let mut k = EpKnobs::default();
        f(&mut k);
        k
    }

    /// One voice, struck once, ticked for `secs` — no pool, no pan, no body.
    fn render(pitch: u8, vel: f32, k: &EpKnobs, base: &TineBase, secs: f32) -> Vec<f32> {
        let mut v = EpVoice::new(RATE);
        v.strike_based(pitch, vel, k, base);
        (0..(secs * RATE) as usize).map(|_| v.tick()).collect()
    }

    /// Magnitude at one frequency through a Hann window, in f64: the
    /// bench's measuring stick. A rectangle's leakage floor would pin
    /// noise instead — the lesson this module paid for once already.
    fn goertzel_hann(x: &[f32], freq: f32) -> f64 {
        let n = x.len();
        let w = TAU * f64::from(freq) / f64::from(RATE);
        let (mut re, mut im) = (0.0_f64, 0.0_f64);
        for (i, &s) in x.iter().enumerate() {
            let win = 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos();
            let v = f64::from(s) * win;
            re += v * (w * i as f64).cos();
            im += v * (w * i as f64).sin();
        }
        re.hypot(im) * 4.0 / n as f64
    }

    fn db(v: f64) -> f64 {
        20.0 * v.max(1e-30).log10()
    }

    /// `harm.py`'s measurement: harmonics 1..=8 of `f0` over 0.12–0.52 s
    /// after the strike, dB re the fundamental, with the fundamental in dBFS.
    fn harmonics(x: &[f32], f0: f32) -> (f64, [f64; 8]) {
        let a = (0.12 * RATE) as usize;
        let b = (0.52 * RATE) as usize;
        let seg = &x[a..b.min(x.len())];
        let mut amps = [0.0_f64; 8];
        for (h, a) in amps.iter_mut().enumerate() {
            *a = goertzel_hann(seg, f0 * (h + 1) as f32);
        }
        let h1 = amps[0].max(1e-30);
        let mut rel = [0.0; 8];
        for h in 0..8 {
            rel[h] = db(amps[h] / h1);
        }
        (db(h1), rel)
    }

    /// The bench's velocity ladder: MIDI 8, 19, … 127.
    fn ladder() -> [u8; 12] {
        let mut v = [0u8; 12];
        for (i, s) in v.iter_mut().enumerate() {
            *s = (8 + i * 11).min(127) as u8;
        }
        v
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0_f32, |m, s| m.max(s.abs()))
    }

    /// Where the loudest thing within ±5 % of `around` actually is.
    fn peak_near(x: &[f32], around: f32) -> (f32, f64) {
        let mut best = (0.0_f32, 0.0_f64);
        let mut f = around * 0.95;
        while f < around * 1.05 {
            let g = goertzel_hann(x, f);
            if g > best.1 {
                best = (f, g);
            }
            f += around * 0.001;
        }
        best
    }

    /// The fundamental's envelope in dB, Hann frames of `frame` seconds
    /// every `hop` seconds, as (frame centre time, dB).
    fn envelope(x: &[f32], f0: f32, frame: f32, hop: f32) -> Vec<(f32, f64)> {
        let n = (frame * RATE) as usize;
        let h = (hop * RATE) as usize;
        let mut out = Vec::new();
        let mut i = 0;
        while i + n <= x.len() {
            out.push(((i + n / 2) as f32 / RATE, db(goertzel_hann(&x[i..i + n], f0))));
            i += h;
        }
        out
    }

    /// Dips in an envelope: local minima of the envelope's residual from
    /// its own line, at least 1 dB under the residual half a second either
    /// side. Detrended, because a note falling 15 dB/s hides a shallow dip
    /// from any test on neighbouring frames; a straight exponential leaves
    /// no residual and so no dip.
    fn dips(env: &[(f32, f64)], t0: f32, t1: f32) -> usize {
        let (m, c, _) = linfit(env, t0, t1);
        let res: Vec<(f32, f64)> = env.iter().map(|(t, d)| (*t, d - m * f64::from(*t) - c)).collect();
        let at = |t: f32| res.iter().min_by(|a, b| (a.0 - t).abs().partial_cmp(&(b.0 - t).abs()).unwrap()).unwrap().1;
        let mut n = 0;
        for i in 0..res.len() {
            let (t, r) = res[i];
            if t < t0 + 0.5 || t > t1 - 0.5 {
                continue;
            }
            let local_min = res.iter().filter(|(u, _)| (*u - t).abs() <= 0.5).all(|(_, v)| *v >= r);
            if local_min && r < at(t - 0.5) - 1.0 && r < at(t + 0.5) - 1.0 {
                n += 1;
            }
        }
        n
    }

    /// The two-body fit: `A²e^{−2α₁t} + B²e^{−2α₂t} + 2AB·e^{−(α₁+α₂)t}·cos(2πΔf·t)`
    /// against the fundamental's squared envelope, with `Δf` given (from
    /// the phase slope), by a grid over the two decay rates with the three
    /// amplitude products solved linearly at each point, weighted by
    /// relative error so the tail counts as much as the attack. Returns
    /// `(A, α₁, B, α₂)` with the rates in nepers per second; the crossover
    /// — the knee — is `ln(A/B)/(α₁−α₂)` and a t60 is `ln 1000 / α`.
    fn fit_pair(env: &[(f32, f64)], split_hz: f64, t0: f32, t1: f32) -> (f64, f64, f64, f64) {
        let pts: Vec<(f64, f64)> = env
            .iter()
            .filter(|(t, _)| *t >= t0 && *t < t1)
            .map(|(t, d)| (f64::from(*t), 10f64.powf(d / 10.0)))
            .collect();
        let dw = TAU * split_hz;
        let mut best = (f64::MAX, 0.0, 0.0, [0.0; 3]);
        let mut a1 = 0.2_f64;
        while a1 <= 3.0 {
            let mut a2 = 0.2_f64;
            while a2 <= 3.0 {
                if (a1 - a2).abs() > 1e-9 {
                    // Normal equations for (p, q, r) = (A², B², AB).
                    let mut m = [[0.0_f64; 3]; 3];
                    let mut v = [0.0_f64; 3];
                    for &(t, y) in &pts {
                        let b = [(-2.0 * a1 * t).exp(), (-2.0 * a2 * t).exp(), 2.0 * (-(a1 + a2) * t).exp() * (dw * t).cos()];
                        let wgt = 1.0 / (y * y);
                        for i in 0..3 {
                            v[i] += wgt * b[i] * y;
                            for j in 0..3 {
                                m[i][j] += wgt * b[i] * b[j];
                            }
                        }
                    }
                    // Cramer's rule, three by three.
                    let det = |m: &[[f64; 3]; 3]| {
                        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
                    };
                    let d = det(&m);
                    if d.abs() > 1e-30 {
                        let mut sol = [0.0; 3];
                        for k in 0..3 {
                            let mut mk = m;
                            for i in 0..3 {
                                mk[i][k] = v[i];
                            }
                            sol[k] = det(&mk) / d;
                        }
                        if sol[0] > 0.0 && sol[1] > 0.0 {
                            let sse: f64 = pts
                                .iter()
                                .map(|&(t, y)| {
                                    let f = sol[0] * (-2.0 * a1 * t).exp() + sol[1] * (-2.0 * a2 * t).exp() + sol[2] * 2.0 * (-(a1 + a2) * t).exp() * (dw * t).cos();
                                    ((f - y) / y).powi(2)
                                })
                                .sum();
                            if sse < best.0 {
                                best = (sse, a1, a2, sol);
                            }
                        }
                    }
                }
                a2 += 0.02;
            }
            a1 += 0.02;
        }
        let (_, a1, a2, sol) = best;
        // The model is symmetric in its two components; the tine side is
        // the one that decays faster.
        if a1 >= a2 {
            (sol[0].sqrt(), a1, sol[1].sqrt(), a2)
        } else {
            (sol[1].sqrt(), a2, sol[0].sqrt(), a1)
        }
    }

    /// The fundamental's phase against a pure `f0` in absolute time,
    /// unwrapped, in cycles: a component exactly at `f0` holds still, one
    /// at `f0 + Δf` advances `Δf` cycles a second, and the sum of two
    /// advances at the dominant one's rate on average, whatever the other
    /// does to it on the way. Hann frames of `frame` seconds every `hop`.
    fn phase_track(x: &[f32], f0: f32, frame: f32, hop: f32) -> Vec<(f32, f64)> {
        let n = (frame * RATE) as usize;
        let h = (hop * RATE) as usize;
        let w = TAU * f64::from(f0) / f64::from(RATE);
        let mut out = Vec::new();
        let (mut i, mut prev, mut acc) = (0usize, 0.0_f64, 0.0_f64);
        while i + n <= x.len() {
            let (mut re, mut im) = (0.0_f64, 0.0_f64);
            for j in 0..n {
                let win = 0.5 - 0.5 * (TAU * j as f64 / n as f64).cos();
                let v = f64::from(x[i + j]) * win;
                let ph = w * (i + j) as f64;
                re += v * ph.cos();
                im -= v * ph.sin();
            }
            let phase = im.atan2(re) / TAU;
            if !out.is_empty() {
                let mut d = phase - prev;
                d -= d.round();
                acc += d;
            }
            prev = phase;
            out.push((i as f32 / RATE, acc));
            i += h;
        }
        out
    }

    /// Least squares line through the points in `[t0, t1)`: slope,
    /// intercept, and the sum of squared residuals.
    fn linfit(pts: &[(f32, f64)], t0: f32, t1: f32) -> (f64, f64, f64) {
        let p: Vec<(f64, f64)> = pts
            .iter()
            .filter(|(t, _)| *t >= t0 && *t < t1)
            .map(|(t, d)| (f64::from(*t), *d))
            .collect();
        let n = p.len() as f64;
        let mx = p.iter().map(|q| q.0).sum::<f64>() / n;
        let my = p.iter().map(|q| q.1).sum::<f64>() / n;
        let sxy: f64 = p.iter().map(|q| (q.0 - mx) * (q.1 - my)).sum();
        let sxx: f64 = p.iter().map(|q| (q.0 - mx) * (q.0 - mx)).sum();
        let m = sxy / sxx;
        let c = my - m * mx;
        let sse: f64 = p.iter().map(|q| (q.1 - m * q.0 - c).powi(2)).sum();
        (m, c, sse)
    }

    /// **The second mode at C4 is 5.64, from the beam, as the file always
    /// had it.** Measured on the voice, not read off the schedule: strike
    /// C4 at forte and find the loudest thing within five per cent of
    /// 5.64·f0. The tolerance is a fifth of a semitone.
    ///
    /// If this goes red the spring schedule or the taper moved: `SPRING_C4`
    /// was bisected for exactly this number.
    #[test]
    fn the_second_mode_at_c4_is_where_the_file_always_put_it() {
        let f0 = 261.6256_f32;
        let out = render(60, 1.0, &EpKnobs::default(), &TineBase::MK1, 0.25);
        let (f, _) = peak_near(&out, f0 * 5.64);
        let ratio = f / f0;
        let cents = 1200.0 * (ratio / 5.64).log2();
        println!("second mode at C4: {ratio:.3} x f0 ({cents:+.0} cents from 5.64)");
        assert!(cents.abs() < 20.0, "the second mode sits at {ratio:.3} x f0, not 5.64");
        // And it is the beam's number for the schedule's spring at C4.
        let r = beam::clamped_free(SPRING_C4, TAPER_WIDTH, TAPER_THICK);
        assert!((r.f2 - 5.64).abs() < 0.01, "the beam puts the spring-loaded tine at {}", r.f2);
    }

    /// **The series is the beam's and falls toward the treble**, and the
    /// old linear schedule was that series to within a few hundredths from
    /// A0 to C6 — printed side by side so the claim in `SPRING_C4`'s comment
    /// is on the record every run. Above C7 the spring has run out and the
    /// bar's own 4.50 stands where the old line extrapolated to 4.2.
    #[test]
    fn the_series_is_the_beams_and_falls_toward_the_treble() {
        println!(" key   beta    f2      old    f3/f2  f4/f2");
        let mut prev = f32::MAX;
        for k in [21u8, 28, 40, 52, 60, 64, 76, 88, 100] {
            let beta = (SPRING_C4 - SPRING_PER_KEY * (f32::from(k) - 60.0)).max(0.0);
            let r = beam::clamped_free(beta, TAPER_WIDTH, TAPER_THICK);
            let old = 6.6 - 0.030 * (f32::from(k) - 28.0);
            println!(" {k:3}   {beta:.4}  {:.3}   {old:.3}  {:.3}  {:.3}", r.f2, r.f3 / r.f2, r.f4 / r.f2);
            assert!(r.f2 < prev, "the second mode must fall toward the treble");
            assert!((r.f2 - old).abs() < 0.06, "key {k}: beam {} against the old schedule {old}", r.f2);
            // The beam's third and fourth, not the old 1.62 and 2.71.
            assert!(r.f3 / r.f2 > 2.5 && r.f4 / r.f2 > 4.8, "{r:?} is not a clamped-free series");
            prev = r.f2;
        }
    }

    /// The pair's solve, checked where it can be checked by hand.
    ///
    /// - At zero coupling: the tine at f0 with all the motion, the bar at
    ///   its detune with none.
    /// - The shares sum to one at every coupling, so the throw is the throw.
    /// - The tine's mode is tuned to f0 exactly at every coupling.
    /// - The split equals `f0·√(ε² + c²)` to half a per cent — the first
    ///   order closed form of the 2×2, whose next term is `(ε + c)/2` of
    ///   itself, 0.1 % here — which at `c → 0` is `|ω₁ − ω₂| / 2π`, and it
    ///   grows with `c`.
    /// - The bar side's decay is longer than the tine side's: the aftersound.
    #[test]
    fn the_pair_solve_is_the_closed_form() {
        let base = TineBase::MK1;
        let p0 = couple(F_E4, 3.8, &base, 0.0);
        assert_eq!(p0.share, [1.0, 0.0]);
        assert_eq!(p0.freq[0], F_E4);
        assert!((p0.freq[1] / F_E4 - 1.0 - base.detune).abs() < 1e-6);
        let mut last_split = 0.0_f32;
        for body in [0.05_f32, 0.25, 0.5, 1.0, 2.0] {
            let c = base.coupling * body;
            let p = couple(F_E4, 3.8, &base, c);
            let split = p.freq[1] - p.freq[0];
            let want = F_E4 * (base.detune * base.detune + c * c).sqrt();
            println!(
                "body {body:4.2}: split {split:.4} Hz (closed form {want:.4}), shares {:.3}/{:.3}, t60 {:.2}/{:.2} s",
                p.share[0], p.share[1], p.t60[0], p.t60[1]
            );
            assert!((p.share[0] + p.share[1] - 1.0).abs() < 1e-5, "shares {:?}", p.share);
            assert!((p.freq[0] - F_E4).abs() < 1e-3, "the tine's mode is {} not f0", p.freq[0]);
            assert!((split / want - 1.0).abs() < 5e-3, "split {split} against {want}");
            assert!(split > last_split, "the split must widen with the coupling");
            assert!(p.t60[1] > p.t60[0], "the bar side must outlast the tine side");
            last_split = split;
        }
        // c → 0: the split tends to the bare detune.
        let p = couple(F_E4, 3.8, &base, 1e-6);
        assert!(((p.freq[1] - p.freq[0]) / (F_E4 * base.detune) - 1.0).abs() < 1e-3);
    }

    /// **Acceptance C, the beat.** A held E4's fundamental beats at the
    /// pair's split, and the split is measured off the audio, not read
    /// off the solve: once the bar side owns the motion, the fundamental's
    /// phase against a pure f0 advances at exactly `f_bar − f_tine` cycles
    /// a second (the tine side is tuned to f0 and holds still), so the
    /// slope of that phase over the last three and a half seconds IS the
    /// beat rate. Held to 5 % of the solved split at three couplings, it
    /// must quicken as TONEBAR rises, and the envelope must actually dip —
    /// a beat nobody can hear is not a beat. A null's TIME is not used: the
    /// minimum of two decaying phasors sits at anti-phase only when they
    /// are equal there, and at a slow beat the decay drags it late by a
    /// sixth (measured 2.12 s against 1.83).
    ///
    /// At the knob's bottom (κ = 0) the bar is not in the motion, the
    /// envelope has no dip, and the phase holds still: the limit the split
    /// formula reaches, `|ω₁ − ω₂| / 2π` on a bar nobody can hear. Break
    /// `couple` — shares `[1.0, 0.0]` always — and every coupled row reads
    /// the dead-wood case.
    #[test]
    fn the_beat_is_the_pairs_split_and_quickens_with_the_tonebar() {
        let mut last_rate = 0.0_f64;
        for body in [0.25_f32, 1.0, 2.0] {
            let k = knobs_with(|k| k.body = body);
            let out = render(E4, 0.8, &k, &TineBase::MK1, 8.0);
            let track = phase_track(&out, F_E4, 0.1, 0.05);
            let (rate, _, _) = linfit(&track, 4.0, 7.5);
            let p = couple(F_E4, 3.8, &TineBase::MK1, TineBase::MK1.coupling * body);
            let want = f64::from(p.freq[1] - p.freq[0]);
            let env = envelope(&out, F_E4, 0.1, 0.02);
            let dips = dips(&env, 0.1, 6.0);
            println!("body {body}: phase advances {rate:.4} Hz, solved split {want:.4} Hz, {dips} dip(s) in the envelope");
            assert!((rate / want - 1.0).abs() < 0.05, "body {body}: beat {rate:.4} Hz against the split's {want:.4}");
            assert!(dips >= 1, "body {body}: the envelope never dipped");
            assert!(rate > last_rate, "the beat must quicken as TONEBAR rises");
            last_rate = rate;
        }
        // Dead wood: no dip, and the phase holds still.
        let out = render(E4, 0.8, &knobs_with(|k| k.body = 0.0), &TineBase::MK1, 6.0);
        let env = envelope(&out, F_E4, 0.1, 0.02);
        let dips = dips(&env, 0.1, 5.5);
        assert_eq!(dips, 0, "with the bar on dead wood the envelope still dipped {dips} times");
        // **Fitted earlier than the coupled rows, and for a reason that is
        // the physics of this very case.** With the bar out of the motion
        // there is nothing handing energy back, so the tine alone decays;
        // the coupled rows still have a fundamental worth measuring at
        // seven seconds and this one does not. Past about three and a half
        // seconds the phase slope here is reading the noise the mode has
        // fallen into, not the mode: measured -0.0063 Hz over every window
        // from 0.3 s to 3.5 s, steady to three digits, and -0.134 Hz over
        // 3.5-5.5 s from the same render. A rectangular Goertzel lying
        // about a decay is the same family of mistake this module has paid
        // for twice, so the window moved and the tolerance did not.
        let track = phase_track(&out, F_E4, 0.1, 0.05);
        let (rate, _, _) = linfit(&track, 0.5, 3.5);
        // And the proof that the window has signal in it: the fundamental
        // at the far end is still well clear of where that reading went bad.
        let env = envelope(&out, F_E4, 0.1, 0.02);
        let at = |t: f32| {
            env.iter()
                .min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs()))
                .map_or(f64::NEG_INFINITY, |p| p.1)
        };
        // And the proof that the window has a mode in it and not a floor:
        // at the far end the fundamental has fallen 49.5 dB from its
        // attack, to -67.5 dBFS. Two seconds later it is into the region
        // where the phase slope read -0.134 Hz, which is what this guard
        // exists to keep the fit out of.
        let fell = at(3.5) - at(0.3);
        assert!(
            fell > -55.0,
            "the dead-wood fit ends {fell:.1} dB under the attack, past where a phase slope means anything"
        );
        assert!(rate.abs() < 0.01, "with the bar on dead wood the fundamental drifts at {rate:.4} Hz");
    }

    /// **Acceptance C, the knee.** A held E4's fundamental is two decays,
    /// not one, and the two are measured off the audio: the two-body model
    /// (see `fit_pair`) fitted to the fundamental's envelope over the first
    /// five and a half seconds, with the beat rate from the phase slope,
    /// gives each side's amplitude and decay. The tine side must decay
    /// faster than the bar side — the fast prompt sound and the slow
    /// aftersound — and the knee, where the bar side overtakes
    /// (`ln(A/B)/(α₁−α₂)`), must come earlier as TONEBAR rises from a
    /// quarter to centre to full: a larger share for the bar side, so the
    /// tail takes over sooner.
    ///
    /// The same fit checks the solve: the bar side's share and both t60s
    /// off the audio against the eigen-solve's, which is the damping
    /// projection in `couple` held to what it claims. The knob's upper half
    /// moves the knee less than its lower half; that is the two-body solve
    /// (the knee tends to `2/(α₁−α₂)` as the mixing angle opens) and not a
    /// fault, so the comparison stops at centre-to-full. Break `couple` —
    /// shares `[1.0, 0.0]` — and the fit finds one decay with nothing to
    /// cross it.
    #[test]
    fn a_held_e4_has_a_knee_that_the_tonebar_moves_earlier() {
        let mut last_knee = f64::MAX;
        for body in [0.25_f32, 0.5, 1.0] {
            let k = knobs_with(|k| k.body = body);
            let out = render(E4, 0.8, &k, &TineBase::MK1, 6.0);
            let p = couple(F_E4, 3.8, &TineBase::MK1, TineBase::MK1.coupling * body);
            let split = f64::from(p.freq[1] - p.freq[0]);
            let env = envelope(&out, F_E4, 0.1, 0.02);
            let (a, a1, b, a2) = fit_pair(&env, split, 0.05, 5.5);
            let (t60_1, t60_2) = (1000f64.ln() / a1, 1000f64.ln() / a2);
            let share = b / (a + b);
            let knee = (a / b).ln() / (a1 - a2);
            println!(
                "body {body}: tine side {a:.3} at t60 {t60_1:.2} s, bar side {b:.3} at t60 {t60_2:.2} s (solve: {:.2}/{:.2} s, bar share {:.3} vs fitted {share:.3}); knee at {knee:.2} s",
                p.t60[0], p.t60[1], p.share[1]
            );
            assert!(t60_2 > t60_1 * 1.3, "body {body}: the aftersound ({t60_2:.2} s) does not outlast the prompt sound ({t60_1:.2} s)");
            assert!((t60_1 / f64::from(p.t60[0]) - 1.0).abs() < 0.15, "body {body}: tine side t60 {t60_1:.2} s off the audio against the solve's {:.2}", p.t60[0]);
            assert!((t60_2 / f64::from(p.t60[1]) - 1.0).abs() < 0.15, "body {body}: bar side t60 {t60_2:.2} s off the audio against the solve's {:.2}", p.t60[1]);
            assert!((share - f64::from(p.share[1])).abs() < 0.05, "body {body}: bar share {share:.3} off the audio against the solve's {:.3}", p.share[1]);
            assert!(knee < last_knee, "the knee must come earlier as TONEBAR rises");
            last_knee = knee;
        }
    }

    /// **Acceptance D(1).** At BARK centre a forte E4 has its third harmonic
    /// with its second: at least within 6 dB below it. The old pickup kept
    /// H3 6–14 dB under H2 at every velocity (−29.0 against −15.5 at 127,
    /// `evidence/harmonics-today.txt`); the transducer at the reference
    /// regulation puts H3 ABOVE H2, because 0.5 gap is almost exactly the
    /// flux curve's inflection (0.4675, measured), where the curve's second
    /// derivative — the second harmonic's source at small swing — is nearly
    /// zero and the third derivative is the first term to arrive. Both
    /// numbers are printed; the gap between them is the doubt recorded in
    /// MERGE-ME.
    ///
    /// Break the transducer — hand it a hundredth of the displacement — and
    /// H3 falls to the noise.
    #[test]
    fn at_bark_centre_a_forte_e4_puts_the_third_harmonic_with_the_second() {
        for vel in [107u8, 127] {
            let out = render(E4, f32::from(vel) / 127.0, &EpKnobs::default(), &TineBase::MK1, 0.6);
            let (h1, h) = harmonics(&out, F_E4);
            println!("E4 vel {vel}: H1 {h1:.1} dBFS, H2 {:.1}, H3 {:.1} dB re H1 (H3 − H2 = {:+.1})", h[1], h[2], h[2] - h[1]);
            assert!(h[2] >= h[1] - 6.0, "vel {vel}: H3 at {:.1} dB sits more than 6 dB under H2 at {:.1}", h[2], h[1]);
        }
    }

    /// **Acceptance D(2).** Peak output rises monotonically with velocity at
    /// BARK min, centre and max — the check that caught the inverse-volume
    /// knob twice. The transducer's calibration is what holds it: normalise
    /// by anything but the largest slope over the real excursion and the
    /// level knob runs backwards or away.
    ///
    /// Revert check: refer the derivative to `omega0 * v` instead of
    /// `omega0` (a velocity-dependent reference) and the ladder folds.
    #[test]
    fn peak_output_rises_with_velocity_at_every_bark() {
        for bark in [0.43_f32, 1.0, 2.3] {
            let k = knobs_with(|k| k.bark = bark);
            let mut last = 0.0_f32;
            let mut row = String::new();
            for vel in ladder() {
                let out = render(E4, f32::from(vel) / 127.0, &k, &TineBase::MK1, 0.26);
                let p = peak(&out[..(0.25 * RATE) as usize]);
                row.push_str(&format!(" {:+5.1}", db(f64::from(p))));
                assert!(p > last, "bark {bark}: peak fell from {last} to {p} at velocity {vel}");
                last = p;
            }
            println!("bark {bark:4.2} peak dB over the ladder:{row}");
        }
    }

    /// **Acceptance D(3), Trap 1.** The second harmonic never exceeds the
    /// fundamental at any BARK, any velocity, on three keys — the octave
    /// that lives on the pole's axis stays off every regulation the knob
    /// can reach. Measured in `harm.py`'s window and in the attack too.
    /// Worst case is BARK max at fortissimo, where `SWING_GAP` leaves 4 dB.
    ///
    /// Break `rest_offset` — return the floor, 0.2 — and BARK max plays the
    /// octave at fortissimo.
    #[test]
    fn the_second_harmonic_never_exceeds_the_fundamental() {
        let mut worst = f64::MIN;
        for bark in [0.43_f32, 0.7, 1.0, 1.5, 2.3] {
            for pitch in [40u8, 64, 76] {
                let f0 = 440.0 * 2.0_f32.powf((f32::from(pitch) - 69.0) / 12.0);
                for vel in [0.3_f32, 0.6, 0.85, 1.0] {
                    let k = knobs_with(|k| k.bark = bark);
                    let out = render(pitch, vel, &k, &TineBase::MK1, 0.6);
                    let (_, h) = harmonics(&out, f0);
                    let attack = &out[..(0.25 * RATE) as usize];
                    let a2 = db(goertzel_hann(attack, 2.0 * f0) / goertzel_hann(attack, f0).max(1e-30));
                    worst = worst.max(h[1]).max(a2);
                    assert!(h[1] < 0.0, "bark {bark} key {pitch} vel {vel}: H2 at {:+.1} dB over the fundamental", h[1]);
                    assert!(a2 < 0.0, "bark {bark} key {pitch} vel {vel}: H2 at {a2:+.1} dB in the attack");
                }
            }
        }
        println!("closest the octave came to the fundamental: {worst:+.1} dB");
    }

    /// **Acceptance D(4).** `harmonics-today.txt`'s E4 table, re-measured on
    /// the voice at this HEAD — printed for MERGE-ME §5 — and the one thing
    /// in it that is a promise: a feathered note is clean. At velocity 8
    /// the second and third harmonics both sit under −40 dB; the old
    /// pickup's hard-coded modes put H2 at −21.5 there.
    #[test]
    fn the_e4_table_and_a_feathered_note_is_clean() {
        println!("  vel    H1 dBFS |  H2     H3     H4     H5     H6     H7     H8   (dB re H1)");
        for vel in [8u8, 41, 74, 107, 127] {
            let out = render(E4, f32::from(vel) / 127.0, &EpKnobs::default(), &TineBase::MK1, 0.6);
            let (h1, h) = harmonics(&out, F_E4);
            println!(
                "  {vel:3}   {h1:8.1} | {:5.1}  {:5.1}  {:5.1}  {:5.1}  {:5.1}  {:5.1}  {:5.1}",
                h[1], h[2], h[3], h[4], h[5], h[6], h[7]
            );
            if vel == 8 {
                assert!(h[1] < -40.0 && h[2] < -40.0, "a feathered note carries H2 {:.1} / H3 {:.1} dB", h[1], h[2]);
            }
        }
    }

    /// **The level contract.** The E4 velocity ladder's peaks against the
    /// bench-signed table at 898cbfb (`evidence/baseline-report.txt`,
    /// through `Keys` with its pan), after one gain: the deviation from a
    /// single offset must stay within 2 dB at every rung. The offset is the
    /// pan and `OUT_GAIN`; the SHAPE is the instrument.
    #[test]
    fn the_velocity_table_keeps_its_shape() {
        const OLD: [f64; 12] = [
            -36.6, -27.1, -22.3, -19.1, -16.5, -14.5, -13.1, -11.7, -10.8, -10.0, -9.6, -8.9,
        ];
        let mut got = [0.0_f64; 12];
        for (i, vel) in ladder().iter().enumerate() {
            let out = render(E4, f32::from(*vel) / 127.0, &EpKnobs::default(), &TineBase::MK1, 0.26);
            got[i] = db(f64::from(peak(&out[..(0.25 * RATE) as usize])));
        }
        let offset = (0..12).map(|i| got[i] - OLD[i]).sum::<f64>() / 12.0;
        let mut worst = 0.0_f64;
        println!("  vel   old dB   new dB   new − offset − old");
        for (i, vel) in ladder().iter().enumerate() {
            let dev = got[i] - offset - OLD[i];
            worst = worst.max(dev.abs());
            println!("  {vel:3}   {:+6.1}   {:+6.1}   {dev:+5.2}", OLD[i], got[i]);
        }
        println!("  offset {offset:+.2} dB, worst deviation {worst:.2} dB");
        assert!(worst <= 2.0, "the velocity ladder's shape moved by {worst:.2} dB");
    }

    /// The chime's level at the coil is the bench-signed one: at C4 forte
    /// the second mode within 4 dB of the old report's bell column (−17.9 at
    /// E4 127, −18 at C4 by the same measure). The weights carry `1/ratio`
    /// for the coil's tilt; drop that division and the chime comes out
    /// 15 dB hot.
    #[test]
    fn the_chime_is_voiced_at_the_coil_not_the_metal() {
        let out = render(E4, 1.0, &EpKnobs::default(), &TineBase::MK1, 0.26);
        let w = &out[(0.004 * RATE) as usize..(0.254 * RATE) as usize];
        let fund = goertzel_hann(w, F_E4);
        let r = beam::clamped_free((SPRING_C4 - SPRING_PER_KEY * 4.0).max(0.0), TAPER_WIDTH, TAPER_THICK);
        let (_, bell) = peak_near(w, F_E4 * r.f2);
        let rel = db(bell / fund.max(1e-30));
        println!("E4 127: second mode {rel:+.1} dB re the fundamental (old report: −17.9)");
        assert!((rel - -17.9).abs() < 4.0, "the chime sits at {rel:+.1} dB, the bench signed −17.9");
    }

    /// The damper is a loss in the metal and steps nothing: the largest
    /// second difference in the twenty milliseconds around a release is
    /// within three times the note's own a moment before it. Three, not
    /// one, because the pad's dud is IN that window and starts at its
    /// second sample (`Mode::kick` seeds two), a designed onset of
    /// `2·amp·w` — 2.4 times the note's second difference at this level,
    /// −55 dBFS, the dud it is meant to be. The thing this guards against
    /// measures two hundred: fold the damper's gain into the motion instead
    /// (`m.scale(0.5)` in `land_damper`) and the coil differentiates the
    /// step. And the note does stop: −40 dB within a second of the key
    /// coming up at RELEASE centre.
    #[test]
    fn a_release_neither_steps_nor_lingers() {
        let mut v = EpVoice::new(RATE);
        v.strike_based(E4, 0.9, &EpKnobs::default(), &TineBase::MK1);
        let mut out = Vec::new();
        for _ in 0..(0.4 * RATE) as usize {
            out.push(v.tick());
        }
        let at = out.len();
        v.release(false);
        for _ in 0..(1.2 * RATE) as usize {
            out.push(v.tick());
        }
        let d2 = |a: usize, b: usize| (a.max(2)..b).fold(0.0_f32, |m, n| m.max((out[n] - 2.0 * out[n - 1] + out[n - 2]).abs()));
        let before = d2(at - 960, at);
        let around = d2(at - 10, at + 960);
        println!("d2 before release {before:.2e}, around it {around:.2e}");
        assert!(around <= before * 3.0, "the release stepped: d2 {around:.2e} against {before:.2e} before it");
        let late = peak(&out[at + (1.0 * RATE) as usize..]);
        let early = peak(&out[at - 4800..at]);
        assert!(db(f64::from(late / early)) < -40.0, "a second after release the note is only {:.1} dB down", db(f64::from(late / early)));
    }

    /// **The strike is a pulse at the coil, not a step.** The output's
    /// largest one-sample difference in the first two milliseconds of a
    /// fortissimo E4, against its largest anywhere in the first 20 ms of
    /// steady ringing: with the seed delivered in one sample the coil put
    /// out a step of the whole velocity, measured at 4.4 times the note's
    /// own difference; through the contact pulse it is under 1.5. And the
    /// pulse changes nothing about what the note settles to: each mode's
    /// amplitude read 30 ms in is the voiced one, within a tenth of a dB
    /// of the same strike delivered as a `kick_add` impulse.
    ///
    /// Break the division by the pulse's magnitude — `self.pend[i] = a *
    /// unit` — and the chime family comes out under by the roll-off,
    /// 3 dB at the second mode; break the delivery — `len = 1` — and the
    /// step is back.
    #[test]
    fn a_strike_rises_through_the_contact_and_settles_where_it_was_voiced() {
        let out = render(E4, 1.0, &EpKnobs::default(), &TineBase::MK1, 0.1);
        let d1 = |x: &[f32]| x.windows(2).fold(0.0_f32, |m, w| m.max((w[1] - w[0]).abs()));
        let onset = d1(&out[..(0.002 * RATE) as usize]);
        let ringing = d1(&out[(0.02 * RATE) as usize..(0.04 * RATE) as usize]);
        println!("largest step at the onset {onset:.4}, while ringing {ringing:.4} ({:.2}x)", onset / ringing);
        assert!(onset < ringing * 1.5, "the strike steps: {onset:.4} against {ringing:.4} ringing");

        // Same strike as an impulse, mode by mode, for the amplitudes.
        let mut v = EpVoice::new(RATE);
        v.strike_based(E4, 1.0, &EpKnobs::default(), &TineBase::MK1);
        // The copy carries the strike's own coefficients; only the seed
        // is delivered differently — all at once, times the pulse gain
        // `strike_based` divided it by.
        let mut imp = v.modes;
        for (i, m) in imp.iter_mut().enumerate() {
            let w = std::f32::consts::TAU * v.freqs[i] / RATE;
            let gain = v.pulse_scale * pulse_magnitude(v.pulse_q, w, v.pulse_left);
            m.clear();
            m.nudge(v.pend[i] * gain);
        }
        for _ in 0..(0.03 * RATE) as usize {
            v.tick();
            for m in imp.iter_mut() {
                m.tick();
            }
        }
        for i in 0..MODES {
            let (p, k) = (v.modes[i].amplitude(), imp[i].amplitude());
            if k < 1e-6 {
                continue;
            }
            let off = 20.0 * (p / k).log10();
            println!("mode {i} at {:.0} Hz: pulsed {p:.5}, impulse {k:.5} ({off:+.2} dB)", v.freqs[i]);
            assert!(off.abs() < 0.1, "mode {i}: the pulse moved the amplitude by {off:+.2} dB");
        }
    }

    #[test]
    fn zz_mode_quantisation() {
        for (f, t60) in [(F_E4, 3.82_f32), (F_E4 * 1.0018, 11.0), (261.6256, 4.2), (55.0, 9.0), (1046.5, 1.5)] {
            let mut m = Mode::default();
            m.kick(f, t60, RATE, 0.3);
            let x: Vec<f32> = (0..(6.0 * RATE) as usize).map(|_| m.tick()).collect();
            let (rate, _, _) = linfit(&phase_track(&x, f, 0.1, 0.05), 0.5, 5.5);
            println!("bare Mode at {f:.3} Hz, t60 {t60}: actual frequency off by {rate:+.4} Hz");
        }
    }

    /// A BARK move between two strikes of one ringing note glides: no
    /// sample around the second strike differs from its neighbours by more
    /// than the strike itself does from silence.
    #[test]
    fn a_bark_change_under_a_ringing_note_does_not_spike() {
        let fresh = render(E4, 0.8, &knobs_with(|k| k.bark = 2.3), &TineBase::MK1, 0.05);
        let d1 = |x: &[f32]| x.windows(2).fold(0.0_f32, |m, w| m.max((w[1] - w[0]).abs()));
        let fresh_d1 = d1(&fresh);
        let mut v = EpVoice::new(RATE);
        v.strike_based(E4, 0.8, &knobs_with(|k| k.bark = 0.43), &TineBase::MK1);
        for _ in 0..(0.3 * RATE) as usize {
            v.tick();
        }
        v.strike_based(E4, 0.8, &knobs_with(|k| k.bark = 2.3), &TineBase::MK1);
        let after: Vec<f32> = (0..(0.05 * RATE) as usize).map(|_| v.tick()).collect();
        let landing_d1 = d1(&after);
        println!("largest step: fresh strike {fresh_d1:.3}, landing with BARK swept end to end {landing_d1:.3}");
        assert!(landing_d1 < fresh_d1 * 2.0, "the BARK move spiked: {landing_d1} against a fresh strike's {fresh_d1}");
    }

    /// The suitcase harp is its own instrument: cleaner feathered and
    /// dirtier leaned on than the Mk.1, and its bar beats faster.
    #[test]
    fn the_suitcase_harp_differs_from_the_mk1_the_way_its_base_says() {
        let (_, mk1_pp) = harmonics(&render(E4, 8.0 / 127.0, &EpKnobs::default(), &TineBase::MK1, 0.6), F_E4);
        let (_, sc_pp) = harmonics(&render(E4, 8.0 / 127.0, &EpKnobs::default(), &TineBase::SUITCASE, 0.6), F_E4);
        let (_, mk1_ff) = harmonics(&render(E4, 1.0, &EpKnobs::default(), &TineBase::MK1, 0.6), F_E4);
        let (_, sc_ff) = harmonics(&render(E4, 1.0, &EpKnobs::default(), &TineBase::SUITCASE, 0.6), F_E4);
        println!("pp H2: mk1 {:.1} suitcase {:.1}; ff H2: mk1 {:.1} suitcase {:.1}", mk1_pp[1], sc_pp[1], mk1_ff[1], sc_ff[1]);
        assert!(sc_pp[1] < mk1_pp[1], "the suitcase should be cleaner feathered");
        assert!(sc_ff[1] > mk1_ff[1], "the suitcase should bark harder at fortissimo");
        let p1 = couple(F_E4, 3.8, &TineBase::MK1, TineBase::MK1.coupling);
        let p2 = couple(F_E4, 3.8, &TineBase::SUITCASE, TineBase::SUITCASE.coupling);
        assert!(p2.freq[1] - p2.freq[0] > p1.freq[1] - p1.freq[0]);
    }

    /// What a strike costs, in release, with the beam solve in it — printed
    /// so the figure is on the record, gated at a tenth of a millisecond:
    /// the callback has 2.7 ms per block and an eight-note chord in one
    /// block must not spend a third of it striking.
    ///
    /// **The gate is RELEASE-only, and it was not, which is why it went red
    /// for a week over nothing.**
    ///
    /// The budget above is a release budget — 8 x 100 us is a third of a
    /// 2.7 ms block, and release is what ships — but `cargo test` builds this
    /// crate unoptimised, and there is no `[profile.test]` raising it. So the
    /// number being compared was a DEBUG figure held against a release bar,
    /// and it passed for months only because debug happened to land just
    /// under it. Measured on one machine on 2026-09-07: 10.3 us in release
    /// and 104-110 us in debug, a factor of ten. Nothing in `keys/` had
    /// changed; the machine had merely got slower, and a bar that was never
    /// measuring the right build finally noticed.
    ///
    /// A red test nobody can act on is worse than no test — it is what
    /// teaches people to skim past a suite, which is how a dead overload
    /// counter shipped green earlier the same week. So the figure is still
    /// PRINTED in both builds, because it is worth having on the record and
    /// worth eyeballing, and the assertion is made where its number means
    /// something.
    #[test]
    fn a_strike_costs_well_under_a_tenth_of_a_millisecond() {
        use std::hint::black_box;
        use std::time::Instant;
        let mut v = EpVoice::new(RATE);
        let k = EpKnobs::default();
        let mut best = f64::MAX;
        for _ in 0..5 {
            let t0 = Instant::now();
            for i in 0..200u32 {
                // A different key every time, so the beam cache never hits.
                v.strike_based(black_box(36 + (i % 60) as u8), 0.7, &k, &TineBase::MK1);
                black_box(v.tick());
            }
            best = best.min(t0.elapsed().as_secs_f64() * 1e6 / 200.0);
        }
        println!("strike: {best:.1} us (beam solve, pair solve, two calibrations)");
        // Debug is roughly ten times release here and has no budget of its
        // own to answer to; the block it has to fit in is a release block.
        #[cfg(not(debug_assertions))]
        assert!(best < 100.0, "a strike costs {best:.0} us");
    }
}
