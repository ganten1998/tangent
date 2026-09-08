//! The piano: a struck string, and the box it is strung on.
//!
//! **The model, in one paragraph.** A string is a delay line with a filter in
//! the feedback loop. Sound travels along it, reflects off the bridge and the
//! agraffe, and comes back slightly quieter and slightly slower at the top —
//! which is the whole of why a piano note is bright at the start and mellow at
//! the end. The delay's LENGTH is the pitch. The filter's shape is the decay,
//! and it has to be frequency-dependent, because a real string loses its top
//! octave in a second and its fundamental in half a minute. An extra all-pass
//! in the loop makes the high partials travel FASTER than the low ones, which
//! is stiffness, which is why a piano's partials are stretched sharp and why
//! two pianos tuned to the same fundamental still beat against each other.
//!
//! **What the hammer is.** Not a click, and no longer a pulse this file
//! draws. Felt compresses, and the harder it is driven the stiffer it gets,
//! so a loud note is not a quiet note turned up: the contact lasts a couple
//! of milliseconds at pianissimo and under one at fortissimo, and a shorter
//! contact injects energy further up the series. That is the single most
//! important thing in this file, because it is what makes the instrument
//! respond to a player rather than to a volume knob. It used to be a
//! Gaussian whose width was a hand-fitted function of velocity; it is now
//! `hammer::Hammer`, a mass on a felt spring that reads the string under it
//! every sample and stays exactly as long as the string lets it. See
//! `LOOP_UNITS_PER_MPS` for which quantity the loop carries and
//! `PianoVoice::tick` for where, in a single delay line, the string under
//! the hammer is read.
//!
//! **What the body is.** A plate with modes (`board.rs`), driven through the
//! bridge, with the old feedback delay network kept behind it for the
//! diffuse tail. See `Body`.

use super::{board, hammer, Patch, PianoKnobs};

/// What separates one grand from another before any knob moves: the base
/// numbers `strike_full` voices itself around. The knobs multiply THESE, and
/// 0.5 on every knob lands exactly here — so two patches sharing this
/// mechanism are two instruments with their own bench signatures, not two
/// presets of one.
#[derive(Clone, Copy)]
pub struct PianoBase {
    /// Multiplier on the whole t60 curve. A nine-footer's bass wire really
    /// does ring longer than a seven-footer's.
    pub t60: f32,
    /// The reference partial's t60 share at BRILLIANCE centre — how long the
    /// top of the series outlasts its own fundamental.
    pub brill: f32,
    /// Multiplier on the measured inharmonicity schedule. Longer strings are
    /// less stiff FOR THEIR PITCH, which is most of why a big grand's treble
    /// sounds sweeter than a baby's.
    pub stiff: f32,
    /// The hammer: what it weighs and what its felt is made of. The three
    /// numbers the base used to carry for the contact — a width at the soft
    /// end, how much of it velocity took away, and a felt multiplier — were
    /// the RESULT a hammer produces, written down as its cause. They are
    /// gone; the contact is now what this hammer does against the string
    /// (`hammer.rs`), and the width comes out of the arithmetic.
    pub hammer: hammer::HammerBase,
    /// The plate this instrument is strung over. See `board.rs`.
    pub plate: board::Plate,
    /// The action's knock — hammer shank, key bed, case — as a level scale.
    pub knock: f32,
}

impl PianoBase {
    /// The instrument the bench signed off first: mid-forward, woody,
    /// voiced for a trio room. Every field 1.0-shaped so `strike_based`
    /// through this base is bit-identical to the pre-base build.
    pub const JAZZ: PianoBase = PianoBase {
        t60: 1.0,
        brill: 0.28,
        stiff: 1.0,
        hammer: hammer::HammerBase::JAZZ,
        plate: board::Plate::Jazz,
        knock: 1.0,
    };
    /// The concert instrument: longer wire, longer ring, a hair less
    /// stiffness for the pitch, a heavier hammer on harder felt — the wider
    /// sweep from whisper to bark a hall instrument is regulated for — a
    /// bigger plate under it, and a quieter action: regulated for
    /// projection rather than for a microphone two feet over the hammers.
    pub const CONCERT: PianoBase = PianoBase {
        t60: 1.35,
        brill: 0.31,
        stiff: 0.85,
        hammer: hammer::HammerBase::CONCERT,
        plate: board::Plate::Concert,
        knock: 0.75,
    };
}

/// The knock: what the ACTION puts into the case the instant the hammer
/// throws. It does not track the note — it is the same wood every time —
/// which is exactly why it reads as a piano being played rather than a tone
/// starting. Two fixed low modes, dead in a few tens of milliseconds.
#[derive(Clone, Copy, Default)]
struct Knock {
    y1: [f32; 2],
    y2: [f32; 2],
    b1: [f32; 2],
    b2: [f32; 2],
}

impl Knock {
    /// Throw the action. Lands on whatever the last throw left ringing —
    /// the same superposition as `tine::Mode::kick_add`, for the same
    /// reason: re-seeding a mode that is still moving is a step. A silent
    /// mode is seeded exactly as it always was.
    fn kick(&mut self, rate: f32, amp: f32) {
        // The case thud and the shank tap. Fixed frequencies, fast decays:
        // by 50 ms this is gone and the string owns the note.
        const FREQ: [f32; 2] = [96.0, 218.0];
        const T60: [f32; 2] = [0.055, 0.030];
        const MIX: [f32; 2] = [1.0, 0.45];
        for i in 0..2 {
            let w = std::f32::consts::TAU * FREQ[i] / rate;
            let r = 10.0_f32.powf(-3.0 / (T60[i] * rate)).clamp(0.0, 0.999_9);
            self.b1[i] = 2.0 * r * w.cos();
            self.b2[i] = -(r * r);
            let seed = amp * MIX[i] * r * w.sin();
            if self.y1[i] == 0.0 && self.y2[i] == 0.0 {
                self.y1[i] = seed;
            } else {
                self.y1[i] += seed;
            }
        }
    }

    fn tick(&mut self) -> f32 {
        let mut out = 0.0;
        for i in 0..2 {
            let y = self.b1[i] * self.y1[i] + self.b2[i] * self.y2[i];
            let y = if y.is_finite() { y } else { 0.0 };
            self.y2[i] = self.y1[i];
            self.y1[i] = y;
            out += y;
        }
        out
    }

    fn clear(&mut self) {
        self.y1 = [0.0; 2];
        self.y2 = [0.0; 2];
    }
}

/// Longest delay any string needs: A0 at 27.5 Hz, at 96 kHz, with room for the
/// fractional part and the tuning correction. Fixed and allocated once, so no
/// note can ever cause an allocation.
const MAX_DELAY: usize = 4096;

/// The damper's time constant as the bench tuned it. Felt takes tens of
/// milliseconds to stop a string and a hard cut sounds like an edit; the
/// RELEASE knob scales this and nothing else.
const DAMP_SECS: f32 = 0.055;

/// How many strings a unison has. Real pianos use one in the low bass, two
/// through the tenor and three above it; three everywhere costs little and the
/// bass ones are simply detuned less.
const UNISON: usize = 3;

/// **What the loop carries, and the one scale between it and the world.**
///
/// The loop is a VELOCITY waveguide. Nothing in `Str` says so — a lossless
/// delay loop is the same equation for displacement and for velocity — but
/// the excitation decides it. The pour that voiced this instrument put a
/// force-shaped pulse into the line directly, and a point force on a
/// string is a velocity jump of `F / (2 Z)` on each rail: launched the
/// instant it acts, gone the instant it stops, which is what a pulse poured
/// in as itself is. Poured into a DISPLACEMENT loop the same force would
/// have had to enter as its own running integral — a step that never comes
/// back down — and through the tap pair that leaves the loop holding a
/// one-sided bump with a mean: a DC, which is the spurious zeroth mode a
/// folded loop has and a string does not. So the loop is velocity, the
/// pour is `F / (2 Z)` per rail, and the displacement the hammer needs is
/// the running integral of the two rails at the strike point, which
/// `PianoVoice::disp` keeps while the hammer is off its check.
///
/// This constant is the single scale: how many loop units one metre per
/// second of string velocity is. It is a LEVEL calibration and nothing
/// else — the physical loop, the string in metres per second and the felt
/// in newtons, is identical at any value of it, because the hammer reads
/// `disp / U` and the strings take `F * U / (2 Z)`. Set on the bench so
/// that the twelve rows of `keysrig piano report`'s velocity table average
/// the level the instrument was signed off at — the one overall gain the
/// re-derivation allows, and it is 0.00 dB
/// (`the_velocity_table_keeps_its_shape` prints it). With that fixed, the
/// C4 mezzo table lands 1.4 dB under its old fundamental and within 1.5 dB
/// on every partial to the seventh
/// (`the_partials_at_mezzo_stay_where_they_were_signed_off`): the felt
/// puts a shade less of a mezzo hammer's energy into C4 than the Gaussian
/// did and a shade more into E4, and the calibration sits between. Read
/// the other way it is a sanity check on everything above: the loop peaks
/// near 0.30 at fortissimo, which at this scale is a string running at
/// 1.8 m/s under a hammer that arrived at 6 — the order every measurement
/// of a struck string gives.
const LOOP_UNITS_PER_MPS: f32 = 0.166;

/// The wave impedance the hammer works against: three C4 strings in
/// parallel, 2.33 N s/m each — 1.08 mm plain wire at the tension that
/// pitches it; `hammer::tests` derives the figure and the felt is fitted
/// against it. ONE number for the whole keyboard, on purpose. A bass
/// string is three times this and a treble string two thirds of it, but a
/// bass hammer is three times the mass too: hammers are scaled to their
/// strings, so the ratios that set the contact, `Z / M` and `K / M`, run
/// nearly flat across a real instrument — and the hammer's equations are
/// invariant under scaling `M`, `K` and `Z` together, the same compression
/// and the same string velocity to the last digit. All a pitch-dependent
/// `Z` alone would change is the level balance across the keyboard, which
/// the pulse it replaces never varied and the chromatic bench signed off
/// as it is. So one impedance and one hammer; the pitch dependence of the
/// contact — long in the bass, a bounce off the agraffe segment in the
/// treble — comes from where the loop's own reflection returns, which is
/// the physics and costs nothing.
const Z_TOTAL: f32 = 3.0 * 2.33;

/// Newtons to loop units, per string: each of the unison takes a third of
/// the force over its own impedance, `(F / 3) / (2 Z)`, which is
/// `F / (2 Z_TOTAL)`, and the loop's level scale on top.
const POUR: f32 = LOOP_UNITS_PER_MPS / (2.0 * Z_TOTAL);

/// The VOICING knob's travel, as a multiplier on the felt's stiffness `K`
/// at either end: needled soft at 0, lacquered hard at 1, the shipped felt
/// at 0.5 EXACTLY (the exponent form lands 0.5 on 1.0 to the last bit).
/// With the felt the whole spring a contact runs as `K^(-1/4)`, so the
/// knob it replaces — 1.35x the contact width at full mellow — would be a
/// `1.35^4 = 3.3x` softer felt; on the loop the contact is only partly the
/// felt's, and 3.3x moved the sound two thirds as far as the old knob did.
/// Five is the travel that lands it: measured at MIDI 63 in
/// `the_voicing_knob_moves_the_felt_not_a_curve`, the bright partials
/// against the fundamental go -19.0 dB at full mellow and +12.3 at full
/// bright, where the old knob went -18.7 and +10.6; at MIDI 127 the bright
/// end sits at +0.1 dB re f0 against the old knob's +0.2.
const VOICING_STIFFNESS: f32 = 5.0;

/// What the level follower is told a strike will reach before the hammer
/// has said anything. The follower is the stealing policy's view of the
/// voice, and a hammer takes a few samples to develop any force at all,
/// so a fresh voice that read as silent for that long could be taken back
/// before it spoke. The seed is the hammer's free peak force
/// (`HammerBase::free_peak_force`, all of its energy into the felt) as a
/// loop level, scaled by what a string that gives way lets the felt
/// actually reach: `the_follower_is_seeded_near_what_the_strike_reaches`
/// measures the true peak against the free one across the velocity range,
/// 0.64 at MIDI 8 and 0.73 to 0.77 everywhere above, and this fraction
/// puts the seed within 3% of the truth from MIDI 30 up and 18% over it
/// at 8. A seed a little high is harmless — the follower falls at 0.0006 a
/// sample and the truth arrives in the next few — a seed of nothing is a
/// note the pool can drop.
const SEED_OF_FREE_PEAK: f32 = 0.75;

/// A one-pole lowpass, which is the loss in the loop.
#[derive(Clone, Copy, Default)]
pub(super) struct Lp {
    pub(super) z: f32,
    pub(super) a: f32,
}

impl Lp {
    pub(super) fn tick(&mut self, x: f32) -> f32 {
        self.z += self.a * (x - self.z);
        self.z
    }
}

/// A first-order all-pass. Cascade them and you have dispersion: a frequency
/// dependent delay, which is stiffness.
#[derive(Clone, Copy, Default)]
pub(super) struct Ap {
    pub(super) a: f32,
    pub(super) x1: f32,
    pub(super) y1: f32,
}

impl Ap {
    pub(super) fn tick(&mut self, x: f32) -> f32 {
        let y = self.a * (x - self.y1) + self.x1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// One string of a unison — and, `pub(super)`, one string of anything
/// else strung on a bridge: the clav borrows this whole loop rather than
/// growing its own, because a struck-and-held string is a struck-and-held
/// string whichever cabinet it lives in.
pub(super) struct Str {
    buf: [f32; MAX_DELAY],
    write: usize,
    /// Delay in samples, fractional.
    pub(super) len: f32,
    pub(super) loss: Lp,
    pub(super) disp: [Ap; 4],
    /// Loop gain, just under one.
    pub(super) gain: f32,
}

impl Str {
    pub(super) fn new() -> Self {
        Str {
            buf: [0.0; MAX_DELAY],
            write: 0,
            len: 100.0,
            loss: Lp::default(),
            disp: [Ap::default(); 4],
            gain: 0.999,
        }
    }

    pub(super) fn clear(&mut self) {
        self.buf.fill(0.0);
        self.loss = Lp { z: 0.0, a: self.loss.a };
        for d in &mut self.disp {
            d.x1 = 0.0;
            d.y1 = 0.0;
        }
    }

    /// Scale everything circulating — the line and the filters' memory —
    /// by `g`. What the damper had taken out of the string, made real in
    /// the string: see `PianoVoice::strike_full`. Four thousand multiplies,
    /// which is what `clear` costs in writes, at a strike and never in the
    /// tick.
    pub(super) fn scale(&mut self, g: f32) {
        for v in &mut self.buf {
            *v *= g;
        }
        self.loss.z *= g;
        for d in &mut self.disp {
            d.x1 *= g;
            d.y1 *= g;
        }
    }

    /// Read the delay line `d` samples back, linearly interpolated.
    pub(super) fn read(&self, d: f32) -> f32 {
        let d = d.clamp(1.0, (MAX_DELAY - 2) as f32);
        let i = d.floor();
        let frac = d - i;
        let back = i as usize;
        let a = self.buf[(self.write + MAX_DELAY - back) % MAX_DELAY];
        let b = self.buf[(self.write + MAX_DELAY - back - 1) % MAX_DELAY];
        a + (b - a) * frac
    }

    /// Advance one sample. `inject` is this sample of the hammer pulse, and
    /// it is a PARAMETER for a reason that cost a day of measurement: it is
    /// superposed onto the value being written at the head. The first
    /// version poured it in through `excite(0, ..)` — into the very slot
    /// this function then overwrites — so the positive half of the hammer
    /// pair never entered the string at all. What circulated was the lone
    /// negative tap: a one-sided blob — waveform asymmetry 1.000 at low
    /// velocity on the bench, a thump for a soft note, and no strike comb
    /// anywhere because the comb IS the pair.
    pub(super) fn tick(&mut self, inject: f32) -> f32 {
        let out = self.read(self.len);
        let mut v = self.loss.tick(out) * self.gain;
        for d in &mut self.disp {
            v = d.tick(v);
        }
        // Guard the loop. A filter coefficient that drifts, a denormal that
        // stalls, a NaN from anywhere — all of them live forever in a feedback
        // path, and a NaN escaping here would poison the master bus for the
        // rest of the session rather than for the rest of the note.
        if !v.is_finite() {
            v = 0.0;
        }
        self.buf[self.write] = v + inject;
        self.write = (self.write + 1) % MAX_DELAY;
        out
    }

    /// Put energy in `at` samples behind the write head — the hammer pair's
    /// far tap. Adds, never assigns: the slot already holds circulating
    /// string, and a hammer lands ON a string, it does not replace it.
    pub(super) fn excite(&mut self, at: usize, v: f32) {
        let i = (self.write + MAX_DELAY - at.min(MAX_DELAY - 1)) % MAX_DELAY;
        self.buf[i] += v;
    }
}

/// Phase delay, in samples, of everything in the loop besides the delay line
/// itself: the loss one-pole (pole `p`) and the four dispersion all-passes
/// (coefficient `ap`), evaluated AT the note's own frequency `w`.
///
/// The first version used the DC formulas — (1-a)/(1+a) and so on — which
/// happened to land within a cent at C4 and would have drifted further up
/// the keyboard, and worse, would have quietly broken every time the loss
/// design below changes its pole. Evaluating the actual phase at the actual
/// frequency keeps "0 cents at the fundamental" true by construction rather
/// than by luck.
pub(super) fn loop_phase_delay(w: f32, p: f32, ap: f32) -> f32 {
    let (sw, cw) = w.sin_cos();
    // One-pole H = g(1-p)/(1 - p e^{-jw}): real numerator, so the phase is
    // minus the denominator's argument.
    let lp = (p * sw).atan2(1.0 - p * cw);
    // All-pass H = (a + e^{-jw})/(1 + a e^{-jw}).
    let ap_ph = (-sw).atan2(ap + cw) - (-ap * sw).atan2(1.0 + ap * cw);
    (lp - 4.0 * ap_ph) / w.max(1e-6)
}

/// Solve the loop's loss filter so the decay is a designed quantity.
///
/// Two targets, two knobs. The loop's per-pass magnitude at the fundamental
/// sets the note's t60; its magnitude at one reference frequency up the
/// series sets how much faster the top dies. A one-pole with a gain in
/// front has exactly two degrees of freedom, so both are hit exactly. The
/// first version set the coefficient from a hand schedule instead, and once
/// the bench's leakage bug was fixed the schedule stood revealed: C4's
/// third partial gone in 1.0 s, the fourth in 0.72 s, nothing above the
/// fourth measurable at ANY velocity — which is why the velocity table was
/// flat. A string that cannot sustain its top cannot brighten.
///
/// For |H(w)| = g(1-p)/|1 - p e^{-jw}| with per-pass targets rho1 at w1 and
/// rho2 at w2, the ratio eliminates g and leaves a quadratic in the pole p;
/// g then follows from the first target. Between and beyond the two points
/// the one-pole's magnitude falls monotonically with frequency, so every
/// partial's t60 decreases strictly with n — the property the whole fix was
/// for — without being individually placed.
pub(super) fn design_loss(w1: f32, w2: f32, rho1: f32, rho2: f32) -> (f32, f32) {
    let (c1, c2) = (w1.cos(), w2.cos());
    let r = (rho1 / rho2) * (rho1 / rho2);
    let q = (c2 - r * c1) / (1.0 - r);
    let p = if r > 1.0 && q > 1.0 {
        q - (q * q - 1.0).sqrt()
    } else {
        // A degenerate request — flat or rising t60 with frequency — gets a
        // nearly transparent filter rather than a wrong one. Reachable only
        // at the extreme treble where both targets clamp.
        0.02
    }
    .clamp(0.02, 0.95);
    let g = rho1 * (1.0 - 2.0 * p * c1 + p * p).sqrt() / (1.0 - p);
    (p, g.min(0.999_9))
}

/// What a strike decides about the loop before it touches a string. See
/// `PianoVoice::design`.
struct Design {
    len: [f32; UNISON],
    gain: [f32; UNISON],
    loss_a: f32,
    disp_a: f32,
    strike: usize,
}

/// One key of the piano.
pub struct PianoVoice {
    rate: f32,
    strings: [Str; UNISON],
    /// The hammer, on its backcheck between strikes and against the
    /// strings during one. Allocated with the voice; `strike_full` launches
    /// it, `tick` runs it while it is off the check.
    hammer: hammer::Hammer,
    /// The strings' displacement under the hammer since it left the check:
    /// the two rails at the strike point, summed over the unison and
    /// integrated, in loop units times seconds. Zero is where the hammer
    /// found the string, which is all the felt needs — compression is
    /// relative to first touch — and it is reset only when a strike finds
    /// the hammer idle, so a strike landing mid-contact keeps its continuity.
    disp: f32,
    /// Where along the string the hammer lands, in samples — the comb notch.
    strike: usize,
    /// Damper: how fast the note is being killed. 1.0 is free.
    damp: f32,
    damp_to: f32,
    /// The damper's time constant in seconds, set at strike time from the
    /// RELEASE knob. Felt takes tens of milliseconds to stop a string;
    /// 0.055 s is the tuned figure and the knob scales it.
    damp_secs: f32,
    /// A peak follower, for the stealing policy and for `finished`.
    env: f32,
    done: bool,
    /// The action's thud, running alongside the strings.
    knock: Knock,
}

impl PianoVoice {
    pub fn new(sample_rate: f32) -> Self {
        PianoVoice {
            rate: sample_rate,
            strings: [Str::new(), Str::new(), Str::new()],
            hammer: hammer::Hammer::new(hammer::HammerBase::JAZZ),
            disp: 0.0,
            strike: 8,
            damp: 1.0,
            damp_to: 1.0,
            damp_secs: DAMP_SECS,
            env: 0.0,
            done: true,
            knock: Knock::default(),
        }
    }

    /// The shipped jazz grand at its default knobs.
    ///
    /// A rung of the strike ladder the instrument does not climb: `mod.rs`
    /// calls the base-picking `strike_based`. What strikes this one is
    /// `symp.rs`'s tests, which need a real piano string beside the harp.
    #[allow(dead_code)]
    pub fn strike(&mut self, pitch: u8, velocity: f32) {
        self.strike_full(pitch, velocity, &PianoKnobs::default(), &PianoBase::JAZZ);
    }

    /// The whole panel on a chosen base — the patch's instrument rather than
    /// the shipped jazz grand. Through `PianoBase::JAZZ` it is `strike` with
    /// the knobs spelled out.
    pub fn strike_based(
        &mut self,
        pitch: u8,
        velocity: f32,
        k: &PianoKnobs,
        base: &PianoBase,
    ) {
        self.strike_full(pitch, velocity, k, base);
    }

    /// `strike`, with the unison detune scaled: 1.0 is the instrument, 0.0
    /// collapses the three strings onto one frequency.
    ///
    /// **Exists for the bench, and only for the bench.** Three live detuned
    /// strings start in phase and drift apart, and the drift sculpts the
    /// ensemble envelope by 15 dB either way on a period of seconds — the
    /// two-stage decay and wandering sustain a real unison has, and exactly
    /// what the detune is FOR. But a t60 fitted to that curve measures the
    /// beat geometry, not the loss filter: the bench's per-partial table once
    /// showed n2 outliving n1 through nothing but a null in n1's beat. The
    /// t60 column is therefore measured with the unison collapsed, where the
    /// decay is the design and nothing else. `mod.rs` never calls this.
    #[allow(dead_code)]
    pub fn strike_spread(&mut self, pitch: u8, velocity: f32, spread: f32) {
        let mut k = PianoKnobs::default();
        k.unison = spread;
        self.strike_full(pitch, velocity, &k, &PianoBase::JAZZ);
    }

    /// Whether a strike of `pitch` with these knobs would land on the loop
    /// that is ringing now, rather than have to replace it. The frame asks
    /// this before a restrike so that the rare case — a knob moved the
    /// string's length under a note still sounding — can be retired
    /// through a fade instead of being cut. See `strike_full`.
    pub fn lands(&self, pitch: u8, knobs: &PianoKnobs, base: &PianoBase) -> bool {
        !self.done && self.fits(&self.design(pitch, knobs, base))
    }

    /// The design of `d` is the loop already ringing, to within a sample of
    /// delay per string. A strike may land on such a loop: its filters can
    /// be re-coefficiented under the circulating wave without a step, and a
    /// read tap that moves by less than a sample moves the output by less
    /// than one sample's worth of the wave's own slope. A tap that moved
    /// further would jump to a different part of the wave, which is a pitch
    /// change mid-ring, which is a pop.
    fn fits(&self, d: &Design) -> bool {
        self.strings
            .iter()
            .zip(&d.len)
            .all(|(s, &len)| (s.len - len).abs() <= 1.0)
    }

    /// Everything a strike decides about the LOOP — length, gain, loss,
    /// dispersion, strike position — for this pitch, these knobs and this
    /// base. Split from the strike itself so a restrike can ask `fits`
    /// before it touches anything. Pure: reads the rate, writes nothing.
    fn design(&self, pitch: u8, knobs: &PianoKnobs, base: &PianoBase) -> Design {
        // `k` is the MIDI key number a few lines down and has been since this
        // file was written; the knobs are `knobs`.
        let spread = knobs.unison;
        let f0 = 440.0 * 2.0_f32.powf((f32::from(pitch) - 69.0) / 12.0);
        let total = (self.rate / f0).clamp(4.0, (MAX_DELAY - 4) as f32);

        // **Inharmonicity.** B is the stiffness coefficient in
        // f_n = n*f0*sqrt(1 + B*n^2). It is smallest in the tenor and rises
        // both ways: short thick bass strings and very short treble ones are
        // both stiff for their length. These are the shape of a measured
        // grand, not a constant.
        let k = f32::from(pitch);
        let b = if k < 48.0 {
            // Bass: climbs steeply downward from about 2e-4 to 4e-3.
            2.0e-4 * 2.0_f32.powf((48.0 - k) / 9.0)
        } else {
            // Tenor up: from 2e-4 to about 1e-3 at the top.
            2.0e-4 * 2.0_f32.powf((k - 48.0) / 22.0)
        };
        // STRETCH scales the measured curve rather than replacing it: the
        // shape across the keyboard is the grand's, and the knob says how
        // stiff this instrument's wire is. Clamped where the all-pass fit
        // stops being a fit.
        let b = (b * knobs.stretch * base.stiff).clamp(1.0e-6, 8.0e-3);

        // Decay, as two designed numbers instead of one guessed coefficient.
        // The fundamental's t60 runs from ~18 s at A0 down to about a second
        // at the top — a solo voicing: singing, not a concert hall's half
        // minute. The reference point up the series (partial 6, or as high as
        // the sample rate allows) gets just over a quarter of that, and
        // `design_loss` hands every partial in between a strictly decreasing
        // t60. Both numbers were tuned against the bench's per-partial table;
        // this pair puts C4 at n1 4.8 s falling steadily to n7 0.8 s, which
        // decays like felt-damped wire rather than either an organ (flat
        // t60) or a clavinet (no top at all).
        // DECAY multiplies the whole curve, after the clamp that shapes it,
        // so the keyboard keeps its slope and the knob only says how long
        // this instrument rings. BRILLIANCE is the reference partial's share
        // of that — the tuned 0.28 at centre.
        let t60_f = (18.0 * (-(k - 21.0) / 30.0).exp()).clamp(1.0, 18.0);
        let t60_f = (t60_f * knobs.decay * base.t60).clamp(0.12, 60.0);
        let f_hi = (6.0 * f0).min(0.40 * self.rate);
        // The knob carries the JAZZ centre baked in by `from_params`; the
        // base re-centres it without moving the knob's travel.
        let brill = knobs.brilliance * (base.brill / PianoBase::JAZZ.brill);
        let t60_h = (t60_f * brill.clamp(0.05, 0.9)).max(0.05).min(t60_f);
        let w1 = std::f32::consts::TAU * f0 / self.rate;
        let w2 = std::f32::consts::TAU * f_hi / self.rate;
        // Per-pass magnitudes: the loop is traversed f0 times a second, so
        // 60 dB in t60 seconds means 10^(-3/(t60*f0)) per pass.
        let rho1 = 10.0_f32.powf(-3.0 / (t60_f * f0));
        let rho2 = 10.0_f32.powf(-3.0 / (t60_h * f0));
        let (p, gain) = design_loss(w1, w2, rho1, rho2);

        // Dispersion, as a cascade of first-order all-passes. Each one adds a
        // delay that shrinks with frequency, and how much extra delay the low
        // partials get over the high ones is the stretch. Designed in DELAY
        // rather than in the coefficient: the per-section delay at DC that
        // delivers the sqrt(1 + B n^2) sharpening scales as sqrt(B) times the
        // period — stiffer wire needs more, higher notes need less of the
        // loop given over to it — and the constant is tuned on the bench so
        // the signed-off C4 default keeps its partials to the cent.
        //
        // The previous map here was INVERTED against B — it fed the sqrt with
        // (1 - b·k), so a stiffer string got a weaker all-pass: the STRETCH
        // knob worked backwards, the concert grand's softer wire came out
        // MORE metallic than the jazz one's, and the clav (which borrows this
        // loop with half the piano's B) landed a long-ringing upper series a
        // quarter tone sharp — a bell it never had. Caught by the bench, not
        // by ear-in-passing: knob 0 measured n6 at +154 cents, knob 1 at +2.
        //
        // Two caps on the per-section delay: 19 samples where |a| would pass
        // 0.9 and stop being trustworthy f32 feedback arithmetic, and 14% of
        // the loop so the shortest treble strings keep enough line to stay in
        // tune. The floor of one sample is the coefficient's zero — a
        // first-order all-pass at a = 0 IS one sample of plain delay.
        let disp_tau = (1.8625 * b.sqrt() * total)
            .clamp(1.0, (0.14 * total).min(19.0).max(1.0));
        let disp_a = (1.0 - disp_tau) / (1.0 + disp_tau);
        // The loop must be SHORTENED by the delay the filters already
        // contribute, or every note plays flat — the single most common bug
        // in a waveguide, and it worsens as pitch rises because the filters'
        // delay is a fixed number of samples while the period shrinks.
        // Evaluated at f0 itself, not approximated at DC: see
        // `loop_phase_delay` for what the DC shortcut would have cost.
        let phase_delay = loop_phase_delay(w1, p, disp_a);

        // The unison. Two or three strings a few cents apart is where the
        // beating comes from, and the beating is why a real piano's sustain
        // wanders instead of sitting still. Only the delay LINE detunes; the
        // filters' phase delay is common to all three.
        const CENTS: [f32; UNISON] = [0.0, 0.9, -1.1];
        // **The double decay.** A real unison's strings do not die together:
        // the coupled motion sheds energy into the bridge fast at first, and
        // what survives is the slow anti-phase tail — the prompt sound and
        // the aftersound. Uncoupled strings cannot do the physics, but giving
        // the off-strings shorter lives makes the SUM do the same thing: the
        // ensemble drops quickly while all three speak, then rides string one
        // out. Scaling the per-pass magnitude by rho1^(1/m - 1) scales that
        // string's whole t60 curve by m without touching its spectrum shape.
        const PAIR_T60: [f32; UNISON] = [1.0, 0.84, 0.74];
        let mut len = [0.0_f32; UNISON];
        let mut gains = [0.0_f32; UNISON];
        for i in 0..UNISON {
            let det = 2.0_f32.powf(CENTS[i] * spread / 1200.0);
            len[i] = (total / det - phase_delay).max(4.0);
            gains[i] = (gain * rho1.powf(1.0 / PAIR_T60[i] - 1.0)).min(0.999_9);
        }

        // Strike position: an eighth of the way along through the mid
        // keyboard, a seventh in the treble. The pair of hammer taps a
        // `strike` apart is a comb, |2 sin(pi f strike/rate)|, with a notch
        // wherever the strike divides the string evenly — the reason a piano
        // has no audible 8th partial and no minor-seventh clash from its own
        // spectrum. ROUNDED, not truncated: C4's loop is 183.5 samples, and
        // truncation drags the notch a third of the way toward n9, leaving
        // n8 dented instead of gone.
        let ratio = if k < 84.0 { 8.0 } else { 7.0 };
        let strike = ((total / ratio).round() as usize).clamp(1, MAX_DELAY - 2);

        Design {
            len,
            gain: gains,
            loss_a: 1.0 - p,
            disp_a,
            strike,
        }
    }

    fn strike_full(&mut self, pitch: u8, velocity: f32, knobs: &PianoKnobs, base: &PianoBase) {
        let voicing = knobs.voicing;
        let k = f32::from(pitch);
        let d = self.design(pitch, knobs, base);

        // **A hammer landing on a string that is still moving adds to the
        // motion.** It does not stop the wire and start again — that was
        // this function's old behaviour, `clear` on every string of a
        // ringing loop, and the step from the circulating wave to zero was
        // the pop the owner heard on every consecutive strike, worst under
        // the pedal because that is when the wave is still loud. Now a
        // strike on a ringing loop of the same design lands on it: the
        // filters take their new coefficients under the wave, the hammer
        // pours in through `excite` and `tick`, which already ADD, and the
        // wave keeps circulating. A loop of a different design — another
        // pitch, or a knob that moved the length — cannot be landed on, and
        // is cleared as before; the frame keeps that case from ever being
        // audible by retiring such a voice through a fade first.
        let landing = !self.done && self.fits(&d);
        if landing && self.damp < 1.0 {
            // The damper's take, made real. `damp` is a gain on the OUTPUT,
            // not a loss in the loop, so a note mid-release still holds its
            // undamped wave behind a fading gain, and putting the gain back
            // to one for the new hammer would reveal all of it at once.
            // Scale the wave by what the felt had actually taken, and then
            // the felt is off the string. Exact, because everything from
            // the loop to the bus is linear.
            for s in &mut self.strings {
                s.scale(self.damp);
            }
        }
        for (i, s) in self.strings.iter_mut().enumerate() {
            for ap in &mut s.disp {
                ap.a = d.disp_a;
            }
            s.loss.a = d.loss_a;
            s.len = d.len[i];
            s.gain = d.gain[i];
            if !landing {
                s.clear();
            }
        }
        if !landing {
            // A loop started from silence gets a hammer from the check.
            // Either the voice was finished, and the hammer is idle already,
            // or the frame is restarting a loop it could not land on, in
            // which case nothing audible is riding on the felt (the frame
            // buries an audible voice rather than reaching here; see
            // `Keys::note_on`) and a hammer left mid-contact over strings
            // just zeroed would be pressing on a string that vanished.
            self.hammer.clear();
            self.disp = 0.0;
        }
        self.strike = d.strike;

        // **The hammer.** A mass on a felt spring, launched at the string
        // at the speed the action gives it (`hammer::velocity_of_midi`) and
        // left to work the contact out for itself, sample by sample, in
        // `tick`. Nothing here decides how long it stays or what shape the
        // force takes: the felt's stiffness, the hammer's mass and the
        // string's own motion decide that, and the width that comes out is
        // the velocity curve this file used to draw by hand — 2.1 ms at
        // MIDI 8 falling to 1.0 at 127 — with the string's reply built in
        // (`contact_comes_out_of_the_hammer_on_the_real_string`). Two
        // shapes failed on the bench before the Gaussian this replaces: a
        // raised cosine, whose spectral nulls at multiples of 2/contact
        // landed on partials 5-6 of the middle keys, and a gamma, whose
        // rise was seven times steeper than its fall and thumped on every
        // quiet note. A felt pulse is neither. At pianissimo the felt is
        // the whole spring and the hump is nearly symmetric; at fortissimo
        // the string segment between hammer and agraffe is the spring, the
        // peak comes early and the reflection rides out as a shoulder
        // (`hammer::tests::loud_pulse_peaks_early`) — and the partial table
        // was re-measured against that shape, not assumed: see
        // `the_partials_at_mezzo_stay_where_they_were_signed_off`.
        let v = velocity.clamp(0.0, 1.0);
        // The VOICING knob is the felt: needled soft to lacquered hard, as
        // a stiffness (`VOICING_STIFFNESS`). The base's own felt sits under
        // it, so the concert hammer at the same knob is still the harder.
        let felt = hammer::HammerBase {
            stiffness: base.hammer.stiffness
                * VOICING_STIFFNESS.powf(2.0 * voicing.clamp(0.0, 1.0) - 1.0),
            ..base.hammer
        };
        let speed = hammer::velocity_of_midi(v);
        // A hammer on its check starts this strike from wherever the string
        // is; one still busy with the last strike keeps its position and
        // its felt and takes only the new speed (`Hammer::strike_with`).
        // The displacement integral follows the same rule — reset only
        // when there is no contact to keep continuous — which is what
        // retires the `LANDING_FLOOR` heuristic the modal banks fake this
        // with: a string moving toward the hammer meets a real felt and
        // hands energy back through it
        // (`a_hammer_meeting_a_string_moving_toward_it_adds_less`).
        if self.hammer.idle() {
            self.disp = 0.0;
        }
        self.hammer.strike_with(felt, speed);
        let seed = felt.free_peak_force(speed) * POUR * SEED_OF_FREE_PEAK;
        // The knock scales harder than the tone: leaning into a key is what
        // makes the action audible, and a feathered note carries almost none.
        // Heavier down the keyboard, where the hammers and the drop are big.
        let bass = 1.0 + ((60.0 - k).max(0.0) / 60.0) * 0.8;
        self.knock.kick(self.rate, base.knock * bass * 0.012 * v.powf(1.9));
        self.damp = 1.0;
        self.damp_to = 1.0;
        self.damp_secs = (DAMP_SECS * knobs.release).clamp(0.004, 1.5);
        // The follower keeps whichever is louder: it is the stealing
        // policy's view of the voice, and a soft strike on a loud note has
        // not made the note soft.
        self.env = if landing { self.env.max(seed) } else { seed };
        self.done = false;
    }

    /// Silence now: every state cleared, `finished` true — the voice a
    /// first strike can land on as if from `new`. The frame calls this on a
    /// voice it has moved out of the pool and faded; the clearing happens
    /// there, once per retired voice, instead of at every strike.
    pub fn silence(&mut self) {
        for s in &mut self.strings {
            s.clear();
        }
        self.knock.clear();
        self.hammer.clear();
        self.disp = 0.0;
        self.damp = 1.0;
        self.damp_to = 1.0;
        self.env = 0.0;
        self.done = true;
    }

    pub fn release(&mut self, pedal: bool) {
        if !pedal {
            self.damp_to = 0.0;
        }
    }

    pub fn damp(&mut self) {
        self.damp_to = 0.0;
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
        // **The hammer and the loop, one sample.** While the hammer is off
        // its check it reads the string under it and pushes back. The
        // string under it is the two rails at the strike point, and in a
        // single folded loop those are two places in one line: the sample
        // written last (delay 1) is the wave that left the strike point
        // toward the near end — the agraffe — and the sample `strike` back
        // is that same wave returned: at the strike point again, heading
        // for the bridge, carried with its agraffe inversion folded away,
        // so it is read with the sign turned back. Their difference is the
        // string's velocity under the hammer, and `disp` integrates it
        // (the loop is velocity; see `LOOP_UNITS_PER_MPS`). Read BEFORE
        // this sample's force goes in, so the force lands a sample late —
        // the explicit scheme the hammer is fitted with. The pour is the
        // tap pair the loop has always had: `+` at the head for the
        // agraffe-bound rail, `-` a strike back for the bridge-bound one,
        // each the velocity jump `F / (2 Z)`. Outside contact all of this
        // is one comparison.
        let mut inject = 0.0_f32;
        if !self.hammer.idle() {
            let at = self.strike as f32;
            let mut v = 0.0_f32;
            for s in &self.strings {
                v += s.read(1.0) - s.read(at);
            }
            let dt = 1.0 / self.rate;
            self.disp += v * (dt / UNISON as f32);
            let force = self.hammer.tick(self.disp / LOOP_UNITS_PER_MPS, dt);
            inject = force * POUR;
        }

        let mut sum = 0.0;
        for s in &mut self.strings {
            // Two injection points, opposite signs, one strike position
            // apart: their difference IS the strike comb, and — because the
            // pair sums to zero — it is also what keeps the excitation
            // DC-free. The positive half rides in through `tick` so it lands
            // in the head slot ADDITIVELY; pushing it through `excite(0, ..)`
            // put it in a slot the loop overwrote the same sample, which
            // silently deleted it and left the one-sided thump the bench
            // measured as asymmetry 1.000.
            if inject != 0.0 {
                s.excite(self.strike, -inject);
            }
            sum += s.tick(inject);
        }
        sum *= 1.0 / UNISON as f32;

        // The damper is a fast fade, not a gate: felt takes tens of
        // milliseconds to stop a string and a hard cut sounds like an edit.
        let coeff = 1.0 - (-1.0 / (self.damp_secs * self.rate)).exp();
        self.damp += (self.damp_to - self.damp) * coeff;
        // The knock rides OUTSIDE the damper: wood does not care whether the
        // dampers are down, and a staccato note keeps its thud.
        let out = sum * self.damp + self.knock.tick();

        let a = out.abs();
        self.env += (a - self.env) * if a > self.env { 0.4 } else { 0.0006 };
        // The felt's first samples are under the follower's floor, and a
        // voice declared finished there would be skipped by the frame and
        // never speak; what carries it across is the seed `strike_full`
        // gives the follower (`SEED_OF_FREE_PEAK`, and
        // `a_soft_strike_is_not_finished_before_the_hammer_speaks` is the
        // test that goes red without it). By the time a follower seeded
        // above a hundredth can fall to the floor, the hammer has been on
        // its check for a hundred milliseconds — so the clear below never
        // cuts a contact short, and a voice this quiet is the one place a
        // hammer may be cleared at all: nothing audible is riding on it,
        // which is the rule `pickup::Transducer::clear` states for the
        // pickups.
        if self.env < 2.0e-5 {
            self.done = true;
            for s in &mut self.strings {
                s.clear();
            }
            self.knock.clear();
            self.hammer.clear();
            self.disp = 0.0;
        }
        out
    }
}

/// The soundboard, and the strings nobody struck.
///
/// **One per instrument, because there is one soundboard.** A string on its own
/// moves almost no air; what is heard is the board it is coupled to through the
/// bridge. Two halves, summed. The plate's resolved modes — the few dozen
/// resonances below a couple of kilohertz the ear hears one at a time, the
/// ones that make a note landing on a mode bloom where its neighbour does
/// not — are `board::Board`, the plate chosen per instrument in `set_patch`.
/// Above that the modes crowd into a smooth colouration, and for that the
/// short feedback delay network that was the whole body until now — a
/// handful of delays cross-mixed so their echoes never line up — stays
/// exactly as it was.
///
/// **The split.** The brief measured the whole body 17.2 dB under the
/// strings at the shipped knob, and the total is held there: the network's
/// output is scaled by `FDN_MIX` and the plate is driven at `BOARD_SEND` so
/// that the body's level under `keysrig piano comp` lands within a decibel
/// of what it was, with the two halves at equal power
/// (`the_body_keeps_its_level_with_the_plate_in` measures both). Equal,
/// because they are two bands of one radiator and neither is the ambience
/// of the other: the modes carry the bottom octaves of the board's colour,
/// the network everything denser, and a plate whispering under its own
/// reverb would be the board the brief measured — present, and not an
/// instrument.
///
/// With the pedal down the network does a second job. Every damper is off
/// the strings, so the whole harp is free to answer whatever is being
/// played; that is the wash under a pedalled chord, and until the harp is
/// strings of its own the network's send rises to stand in for it. The
/// plate's send does not move with the pedal: dampers touch strings, not
/// spruce.
pub struct Body {
    lines: [Vec<f32>; 4],
    idx: [usize; 4],
    fb: f32,
    damp: [Lp; 4],
    pedal: bool,
    energy: f32,
    patch: Patch,
    /// The plate: the resolved modes. See `board.rs`.
    board: board::Board,
    /// Test-only: run the network alone, so a test can measure what the
    /// plate adds against the body it was added to.
    #[cfg(test)]
    board_bypass: bool,
}

/// The network's share of the body, as an output scale, and the bridge
/// sum into the plate. Set together, on the bench, so that under `piano
/// comp` the two halves put out the same RMS (-59.0 dBFS each at root two
/// down) and their sum lands on the body the brief measured. Not quite
/// root two down: the halves are driven by one bridge sum and are not
/// independent, so an equal-power pair summed 1.3 dB over the old body,
/// and both are set that much lower again — the measurement is
/// `the_body_keeps_its_level_with_the_plate_in`, which holds the sum
/// within a decibel of where it was and the split within 3 dB of equal.
const FDN_MIX: f32 = 0.61;
const BOARD_SEND: f32 = 0.0206;

impl Body {
    pub fn new(rate: f32) -> Self {
        // Mutually prime lengths, so no two echoes coincide and the board has
        // no ring of its own.
        let ms = [19.7_f32, 27.3, 34.1, 41.9];
        let lines = ms.map(|m| vec![0.0; ((m * 0.001 * rate) as usize).max(16)]);
        Body {
            lines,
            idx: [0; 4],
            fb: 0.34,
            damp: [Lp { z: 0.0, a: 0.42 }; 4],
            pedal: false,
            energy: 0.0,
            patch: Patch::Piano1,
            board: board::Board::new(rate, PianoBase::JAZZ.plate),
            #[cfg(test)]
            board_bypass: false,
        }
    }

    /// Become another instrument's body. The plate follows the piano base
    /// the patch is played through; the electric patches, which `tick`
    /// silences before it reaches either half, keep whatever plate was
    /// fitted last. Real-time safe: `Board::set_plate` recomputes its
    /// coefficients in place and allocates nothing.
    pub fn set_patch(&mut self, patch: Patch) {
        self.patch = patch;
        match patch {
            Patch::Piano1 => self.board.set_plate(PianoBase::JAZZ.plate),
            Patch::Piano2 => self.board.set_plate(PianoBase::CONCERT.plate),
            _ => {}
        }
        self.reset();
    }

    pub fn set_pedal(&mut self, down: bool) {
        self.pedal = down;
    }

    pub fn reset(&mut self) {
        for l in &mut self.lines {
            l.fill(0.0);
        }
        self.energy = 0.0;
        self.board.clear();
    }

    pub fn ringing(&self) -> bool {
        self.energy > 1.0e-5 || self.board.ringing()
    }

    /// Run the network without the plate. For measuring the plate's
    /// contribution; never reached by the instrument.
    #[cfg(test)]
    pub(super) fn bypass_board(&mut self, bypass: bool) {
        self.board_bypass = bypass;
    }

    /// Drive the board and take its two outputs.
    pub fn tick(&mut self, drive: f32) -> (f32, f32) {
        // Only the pianos have a board. The electric instruments' pickups
        // are electromagnetic or capacitive and their cases do nothing —
        // which is exactly why a Rhodes taken dry is so dry, and why the
        // clav and the reed piano get their character from an amp instead.
        if !matches!(self.patch, Patch::Piano1 | Patch::Piano2) {
            return (0.0, 0.0);
        }
        // Sympathetic strings only answer when the dampers are off.
        let send = if self.pedal { 0.22 } else { 0.075 };
        let x = drive * send;
        let mut outs = [0.0_f32; 4];
        for i in 0..4 {
            let n = self.lines[i].len();
            let v = self.lines[i][self.idx[i]];
            outs[i] = v;
            let _ = n;
        }
        // A Hadamard mix: every line feeds every other, which is what stops
        // the network sounding like four separate echoes.
        let m = [
            outs[0] + outs[1] + outs[2] + outs[3],
            outs[0] - outs[1] + outs[2] - outs[3],
            outs[0] + outs[1] - outs[2] - outs[3],
            outs[0] - outs[1] - outs[2] + outs[3],
        ];
        let mut e = 0.0;
        for i in 0..4 {
            let mut v = x + m[i] * 0.5 * self.fb;
            v = self.damp[i].tick(v);
            if !v.is_finite() {
                v = 0.0;
            }
            let n = self.lines[i].len();
            self.lines[i][self.idx[i]] = v;
            self.idx[i] = (self.idx[i] + 1) % n;
            e += v.abs();
        }
        self.energy += (e * 0.25 - self.energy) * 0.001;
        let l = (outs[0] + outs[2]) * 0.5 * FDN_MIX;
        let r = (outs[1] + outs[3]) * 0.5 * FDN_MIX;
        #[cfg(test)]
        if self.board_bypass {
            return (l, r);
        }
        // The plate, from the same bridge sum, at its own send.
        let (bl, br) = self.board.tick(drive * BOARD_SEND);
        (l + bl, r + br)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Keys, Patch};
    use super::*;

    const RATE: f32 = 48_000.0;
    const BLOCK: usize = 128;

    /// Magnitude at one frequency through a Hann window — the bench's
    /// `analyze::goertzel_hann`, repeated here because `keys/` cannot see
    /// the bench, and for the reason the bench has it: a rectangle's
    /// sidelobes fall as 1/f and bleed a floor under every neighbour.
    fn goertzel_hann(x: &[f32], freq: f32, rate: f32) -> f32 {
        let w = std::f32::consts::TAU * freq / rate;
        let c = 2.0 * w.cos();
        let n = x.len() as f32;
        let step = std::f32::consts::TAU / n;
        let (mut s1, mut s2) = (0.0_f32, 0.0_f32);
        for (i, &v) in x.iter().enumerate() {
            let win = 0.5 - 0.5 * (step * i as f32).cos();
            let s0 = v * win + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / (n * 0.25)
    }

    fn db(v: f32) -> f32 {
        20.0 * v.max(1e-12).log10()
    }

    /// The strongest magnitude within 5% of `around`: dispersion moves a
    /// real partial sharp of the ideal series, and a probe pinned to the
    /// ideal frequency measures the space between.
    fn search_peak(x: &[f32], around: f32) -> f32 {
        let mut best = 0.0_f32;
        let mut f = around * 0.95;
        while f < around * 1.05 {
            best = best.max(goertzel_hann(x, f, RATE));
            f += around * 0.001;
        }
        best
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0_f32, |m, s| m.max(s.abs()))
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / x.len().max(1) as f64).sqrt()
            as f32
    }

    #[derive(Clone, Copy)]
    enum Ev {
        On(u8, u8),
        Off(u8),
    }

    /// Render a performance through the whole instrument, the way the
    /// bench does — block by block, events on the block they fall in — and
    /// return the mono sum. `bypass` runs the body without its plate.
    fn render(patch: Patch, knobs: &[(usize, f32)], events: &[(f32, Ev)], secs: f32, bypass: bool) -> Vec<f32> {
        let mut k = Keys::new(RATE);
        k.set_patch(patch);
        for &(i, v) in knobs {
            k.set_param(i, v);
        }
        k.body.bypass_board(bypass);
        let total = ((secs * RATE) as usize / BLOCK + 1) * BLOCK;
        let mut out = vec![0.0_f32; total];
        let mut scratch = vec![0.0_f32; BLOCK * 2];
        let mut next = 0;
        let mut frame = 0;
        while frame < total {
            let end = (frame + BLOCK) as f32 / RATE;
            while next < events.len() && events[next].0 < end {
                match events[next].1 {
                    Ev::On(p, v) => k.note_on(p, f32::from(v) / 127.0),
                    Ev::Off(p) => k.note_off(p),
                }
                next += 1;
            }
            scratch.fill(0.0);
            k.render(&mut scratch, BLOCK, 2);
            for i in 0..BLOCK {
                out[frame + i] = 0.5 * (scratch[2 * i] + scratch[2 * i + 1]);
            }
            frame += BLOCK;
        }
        out
    }

    fn window(x: &[f32], from: f32, len: f32) -> &[f32] {
        let a = ((from * RATE) as usize).min(x.len());
        let b = (((from + len) * RATE) as usize).min(x.len());
        &x[a..b]
    }

    /// One voice, straight off the mechanism: no body, no pan.
    fn run_voice(v: &mut PianoVoice, secs: f32) -> Vec<f32> {
        (0..(secs * RATE) as usize).map(|_| v.tick()).collect()
    }

    fn f0_of(pitch: u8) -> f32 {
        440.0 * 2.0_f32.powf((f32::from(pitch) - 69.0) / 12.0)
    }

    /// The bench's `report_velocity` on E4: (peak dB, bright partials 5-10
    /// re the fundamental, waveform asymmetry) per row, MIDI 8..127.
    fn velocity_table(x: &[f32]) -> Vec<(f32, f32, f32)> {
        let f0 = f0_of(64);
        let bb = 2.0e-4 * 2.0_f32.powf((64.0 - 48.0) / 22.0);
        let bright: Vec<f32> = (5..=10)
            .map(|n| n as f32 * f0 * (1.0 + bb * (n * n) as f32).sqrt())
            .collect();
        (0..12)
            .map(|i| {
                let t = i as f32 * 1.7;
                let w = window(x, t + 0.004, 0.25);
                let fund = goertzel_hann(w, f0, RATE);
                let hi: f32 = bright.iter().map(|&f| search_peak(w, f)).sum();
                let (mut hi_s, mut lo_s) = (0.0_f32, 0.0_f32);
                for &s in w {
                    hi_s = hi_s.max(s);
                    lo_s = lo_s.min(s);
                }
                let asym = (hi_s - lo_s.abs()).abs() / (hi_s + lo_s.abs()).max(1e-12);
                (db(peak(w)), db(hi / fund.max(1e-12)), asym)
            })
            .collect()
    }

    fn velramp_events() -> Vec<(f32, Ev)> {
        let mut ev = Vec::new();
        for i in 0..12 {
            let v = (8 + i * 11).min(127) as u8;
            let t = i as f32 * 1.7;
            ev.push((t, Ev::On(64, v)));
            ev.push((t + 1.4, Ev::Off(64)));
        }
        ev
    }

    /// The bench's `report_partials` levels at C4: partials 1..=n of a
    /// held MIDI-90 note, 2 s from 20 ms in, each searched a semitone
    /// either side of the stretched series.
    fn partial_levels(x: &[f32], n_max: usize) -> Vec<f32> {
        partial_levels_of(x, n_max, 1.0)
    }

    /// `partial_levels` for a base whose wire is `stiff` times the jazz
    /// schedule's — the concert grand's 0.85.
    fn partial_levels_of(x: &[f32], n_max: usize, stiff: f32) -> Vec<f32> {
        let f0 = f0_of(60);
        let b = stiff * 2.0e-4 * 2.0_f32.powf((60.0 - 48.0) / 22.0);
        let w = window(x, 0.02, 2.0);
        (1..=n_max)
            .map(|n| {
                let ideal = n as f32 * f0 * (1.0 + b * (n * n) as f32).sqrt();
                let mut best = 0.0_f32;
                let mut f = ideal * 0.94;
                while f < ideal * 1.10 {
                    best = best.max(goertzel_hann(w, f, RATE));
                    f += ideal * 0.0008;
                }
                db(best)
            })
            .collect()
    }

    fn contact_ms_on(base: &PianoBase, knobs: &PianoKnobs, midi: u8) -> f32 {
        let mut v = PianoVoice::new(RATE);
        v.strike_based(60, f32::from(midi) / 127.0, knobs, base);
        for _ in 0..(0.05 * RATE) as usize {
            v.tick();
            if v.hammer.idle() {
                break;
            }
        }
        v.hammer.contact_secs() * 1e3
    }

    fn contact_ms(midi: u8) -> f32 {
        contact_ms_on(&PianoBase::JAZZ, &PianoKnobs::default(), midi)
    }

    /// The bench's tables at base 898cbfb, `keysrig piano report`: what the
    /// instrument was signed off sounding like, and what every test below
    /// that says "stays" is measured against.
    const BASE_PEAK: [f32; 12] = [
        -44.4, -33.9, -28.1, -24.1, -21.0, -18.4, -16.1, -14.2, -12.5, -11.0, -9.7, -8.7,
    ];
    const BASE_BELL: [f32; 12] = [
        -37.7, -32.9, -28.7, -25.1, -21.6, -18.6, -15.7, -13.2, -10.8, -8.5, -6.3, -4.8,
    ];
    const BASE_PARTIALS: [f32; 7] = [-42.7, -46.1, -54.0, -63.4, -75.1, -88.7, -104.2];
    /// The same three tables for Piano 2, `keysrig piano2 report` at base.
    const BASE2_PEAK: [f32; 12] = [
        -44.3, -33.7, -27.8, -23.7, -20.6, -18.0, -15.7, -13.8, -12.1, -10.7, -9.5, -8.7,
    ];
    const BASE2_BELL: [f32; 12] = [
        -35.7, -30.3, -25.5, -21.4, -17.6, -14.3, -11.1, -8.3, -5.8, -3.3, -1.0, 0.5,
    ];
    const BASE2_PARTIALS: [f32; 7] = [-40.9, -44.3, -51.9, -58.8, -67.8, -82.6, -97.6];

    /// The bench's `comp` figure: eight chords from feathered to hard,
    /// then two quiet bells — the render the body's level is judged on.
    fn comp_events() -> Vec<(f32, Ev)> {
        let mut ev = Vec::new();
        let voicing = [53u8, 57, 60, 65];
        let mut t = 0.0;
        for i in 0..8 {
            let v = (34 + i * 12).min(127) as u8;
            for (j, &p) in voicing.iter().enumerate() {
                let tt = t + j as f32 * 0.009;
                ev.push((tt, Ev::On(p, v)));
                ev.push((tt + 0.42, Ev::Off(p)));
            }
            ev.push((t, Ev::On(41, v.saturating_sub(6))));
            ev.push((t + 0.42, Ev::Off(41)));
            t += 0.55;
        }
        ev.push((t + 0.4, Ev::On(72, 30)));
        ev.push((t + 3.4, Ev::Off(72)));
        ev.push((t + 1.0, Ev::On(79, 26)));
        ev.push((t + 4.0, Ev::Off(79)));
        ev.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        ev
    }

    /// What the body alone puts out under a performance: the render with
    /// the SOUNDBOARD knob at its default minus the render with it at zero,
    /// which is exact because everything from the bridge sum to the bus is
    /// linear and the two renders are otherwise identical.
    fn body_only(events: &[(f32, Ev)], secs: f32, bypass: bool) -> Vec<f32> {
        let with = render(Patch::Piano1, &[], events, secs, bypass);
        let without = render(Patch::Piano1, &[(7, 0.0)], events, secs, bypass);
        with.iter().zip(&without).map(|(a, b)| a - b).collect()
    }

    /// **Acceptance A1.** The contact time is an OUTPUT of the real voice:
    /// it falls monotonically across MIDI 8..127 and lands on the two
    /// figures the instrument was signed off with, and there is no
    /// velocity-to-width curve in this file to have put it there.
    #[test]
    fn contact_comes_out_of_the_hammer_on_the_real_string() {
        let mut prev = f32::INFINITY;
        let mut table = String::new();
        for m in (8..=127).step_by(7).chain(std::iter::once(127)) {
            let ms = contact_ms(m);
            table.push_str(&format!("  MIDI {m:3} -> {ms:.3} ms\n"));
            assert!(ms <= prev + 1e-6, "contact grew with velocity at MIDI {m}: {ms} > {prev}\n{table}");
            prev = ms;
        }
        println!("JAZZ contact on the real C4 unison:\n{table}");
        let soft = contact_ms(8);
        let loud = contact_ms(127);
        assert!((soft - 2.1).abs() <= 0.15 * 2.1, "MIDI 8 contact {soft} ms is not near 2.1");
        assert!((loud - 1.0).abs() <= 0.15 * 1.0, "MIDI 127 contact {loud} ms is not near 1.0");
        // The concert hammer: a hair shorter at the soft end, the same
        // bounce off the agraffe segment at the loud end.
        let soft2 = contact_ms_on(&PianoBase::CONCERT, &PianoKnobs::default(), 8);
        let loud2 = contact_ms_on(&PianoBase::CONCERT, &PianoKnobs::default(), 127);
        println!("CONCERT: MIDI 8 {soft2:.3} ms, MIDI 127 {loud2:.3} ms");
        assert!(soft2 < soft && soft2 > 1.6);
        assert!((0.8..=1.2).contains(&loud2));
        // The curve is not in the file. Spelled in two halves so that this
        // line is not the match.
        let me = include_str!("string.rs");
        let word = concat!("sig", "ma");
        assert!(!me.contains(word), "a width curve is back in string.rs");
    }

    /// **Acceptance A2, the partial table.** `keysrig piano report`'s
    /// levels at C4 mezzo, partials 1..=8, against the table the
    /// instrument was signed off with (`BASE_PARTIALS`, base 898cbfb):
    /// each within 2 dB, the notched eighth still under the report's find
    /// threshold. This is a re-derivation, not a re-voicing: the level
    /// scale (`LOOP_UNITS_PER_MPS`) is the one number tuned, and it was
    /// tuned on the VELOCITY table's mean, not here.
    #[test]
    fn the_partials_at_mezzo_stay_where_they_were_signed_off() {
        for (patch, base, stiff) in [
            (Patch::Piano1, BASE_PARTIALS, 1.0_f32),
            (Patch::Piano2, BASE2_PARTIALS, 0.85),
        ] {
            let x = render(patch, &[], &[(0.0, Ev::On(60, 90)), (6.0, Ev::Off(60))], 2.2, false);
            let got = partial_levels_of(&x, 8, stiff);
            let mut worst = 0.0_f32;
            for n in 0..7 {
                let d = got[n] - base[n];
                println!("{patch:?} n{}: {:+7.1} dB (signed off {:+7.1}, moved {d:+.1})", n + 1, got[n], base[n]);
                worst = worst.max(d.abs());
            }
            println!("{patch:?} n8: {:+7.1} dB (was under the -107 dB find threshold)", got[7]);
            assert!(worst <= 2.0, "{patch:?}: a mezzo partial moved {worst:.1} dB");
            assert!(got[7] < -100.0, "{patch:?}: n8 came out of the strike notch at {:.1} dB", got[7]);
        }
    }

    /// **Acceptance A2, the velocity table.** The twelve rows of `keysrig
    /// piano report`'s velocity response on E4 keep their shape: after one
    /// overall gain (the mean offset, printed and stated in the doc of
    /// `LOOP_UNITS_PER_MPS`) every row's peak sits within 2 dB of the
    /// signed-off table. The bright-partials column is printed beside it:
    /// a felt pulse is not a Gaussian, and where it differs is stated in
    /// `HammerBase::JAZZ`'s doc rather than tuned away here.
    #[test]
    fn the_velocity_table_keeps_its_shape() {
        for (patch, base_pk, base_bell) in [
            (Patch::Piano1, BASE_PEAK, BASE_BELL),
            (Patch::Piano2, BASE2_PEAK, BASE2_BELL),
        ] {
            let x = render(patch, &[], &velramp_events(), 20.5, false);
            let t = velocity_table(&x);
            let gain = (0..12).map(|i| t[i].0 - base_pk[i]).sum::<f32>() / 12.0;
            let mut worst = 0.0_f32;
            let mut worst_bell = 0.0_f32;
            println!("{patch:?}: overall gain {gain:+.2} dB");
            for i in 0..12 {
                let v = (8 + i * 11).min(127);
                let d = t[i].0 - gain - base_pk[i];
                let db_ = t[i].1 - base_bell[i];
                println!(
                    "  {v:3}  peak {:+6.1} (drift {d:+.1})  bell/f0 {:+6.1} (drift {db_:+.1})  asym {:.3}",
                    t[i].0, t[i].1, t[i].2
                );
                worst = worst.max(d.abs());
                worst_bell = worst_bell.max(db_.abs());
            }
            println!("  worst peak drift {worst:.2} dB, worst bell drift {worst_bell:.1} dB");
            assert!(worst <= 2.0, "{patch:?}: a velocity row drifted {worst:.1} dB after one gain");
            assert!(gain.abs() < 3.0, "{patch:?}: the overall gain is {gain:.1} dB, not a calibration");
            // The sweep itself: the bright partials rise with velocity,
            // and by about what they used to.
            let sweep = t[11].1 - t[0].1;
            let base_sweep = base_bell[11] - base_bell[0];
            assert!((sweep - base_sweep).abs() < 8.0, "{patch:?}: bell sweep {sweep:.1} dB vs {base_sweep:.1}");
        }
    }

    /// The rail velocity under the hammer, as `tick` reads it.
    fn rail_velocity(v: &PianoVoice) -> f32 {
        let at = v.strike as f32;
        v.strings.iter().map(|s| s.read(1.0) - s.read(at)).sum::<f32>() / UNISON as f32
    }

    /// **Acceptance A3.** A hammer meeting a string moving TOWARD it adds
    /// less energy than one meeting the same string moving away, and is
    /// thrown back harder for it — the honest version of what
    /// `tine::Mode::seed_within`'s `LANDING_FLOOR` fakes for the modal
    /// banks, and the reason the piano needs no such floor: a restrike
    /// against motion subtracts because the felt does negative work.
    ///
    /// A fortissimo C4 rings for 300 ms, then a mezzo hammer lands at the
    /// two moments inside one period where the string under it is about
    /// to move furthest toward, and furthest away, over the contact that
    /// follows. Over the CONTACT, not at the instant of touch: the
    /// velocity at the strike point of a struck string is a train of wave
    /// fronts, and `scan_restrike_phase` showed the instantaneous velocity
    /// at touch predicts nothing while the mean over the next millisecond
    /// predicts the rebound exactly. The energy the string keeps is read
    /// off the loop itself 12 ms later, against the un-restruck note at
    /// the same instant.
    #[test]
    fn a_hammer_meeting_a_string_moving_toward_it_adds_less() {
        let knobs = PianoKnobs::default();
        let n0 = (0.30 * RATE) as usize;
        let period = (RATE / f0_of(60)) as usize + 1;
        let contact = (contact_ms(64) * 1e-3 * RATE) as usize;
        let loop_energy = |v: &PianoVoice| -> f64 {
            v.strings.iter().map(|s| s.buf.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>()).sum()
        };
        // The bed's rail velocity for a period and a contact beyond n0.
        let mut v = PianoVoice::new(RATE);
        v.strike_based(60, 1.0, &knobs, &PianoBase::JAZZ);
        for _ in 0..n0 {
            v.tick();
        }
        let rails: Vec<f32> = (0..period + contact)
            .map(|_| {
                let r = rail_velocity(&v);
                v.tick();
                r
            })
            .collect();
        let motion = |k: usize| rails[k..k + contact].iter().sum::<f32>() / contact as f32;
        let toward = (0..period).min_by(|&a, &b| motion(a).partial_cmp(&motion(b)).unwrap()).unwrap();
        let away = (0..period).max_by(|&a, &b| motion(a).partial_cmp(&motion(b)).unwrap()).unwrap();
        let after = (0.012 * RATE) as usize;
        let run = |restrike: Option<usize>, until: usize| -> (f64, f32) {
            let mut v = PianoVoice::new(RATE);
            v.strike_based(60, 1.0, &knobs, &PianoBase::JAZZ);
            for i in 0..until {
                if Some(i) == restrike {
                    v.strike_based(60, 64.0 / 127.0, &knobs, &PianoBase::JAZZ);
                }
                v.tick();
            }
            (loop_energy(&v), v.hammer.velocity())
        };
        let (bed_t, _) = run(None, n0 + toward + after);
        let (bed_a, _) = run(None, n0 + away + after);
        let (e_t, out_t) = run(Some(n0 + toward), n0 + toward + after);
        let (e_a, out_a) = run(Some(n0 + away), n0 + away + after);
        let added_t = e_t - bed_t;
        let added_a = e_a - bed_a;
        let (e_rest, out_rest) = {
            let mut v = PianoVoice::new(RATE);
            v.strike_based(60, 64.0 / 127.0, &knobs, &PianoBase::JAZZ);
            for _ in 0..after {
                v.tick();
            }
            (loop_energy(&v), v.hammer.velocity())
        };
        println!(
            "string motion over the contact: {:+.4} (toward) / {:+.4} (away) loop units; loop energy added {added_t:+.4} toward, {added_a:+.4} away, {e_rest:.4} at rest; hammer thrown back at {out_t:.2} / {out_a:.2} m/s (rest {out_rest:.2})",
            motion(toward),
            motion(away)
        );
        assert!(motion(toward) < 0.0 && motion(away) > 0.0, "no motion under the hammer to speak of");
        assert!(added_t < added_a, "toward {added_t} should add less than away {added_a}");
        assert!(added_t < e_rest, "toward {added_t} should add less than a strike at rest {e_rest}");
        assert!(out_t < out_a, "toward should throw the hammer back harder: {out_t} vs {out_a}");
    }

    /// The VOICING knob moves the felt, and the felt moves the sound the
    /// way the width knob it replaces did: the bright partials at mezzo
    /// fall at full mellow and rise at full bright, monotonically, by an
    /// amount printed here and stated at `VOICING_STIFFNESS`.
    #[test]
    fn the_voicing_knob_moves_the_felt_not_a_curve() {
        let bell_at = |voicing: f32| -> (f32, f32) {
            let x = render(Patch::Piano1, &[(0, voicing)], &velramp_events(), 20.5, false);
            let t = velocity_table(&x);
            (t[5].1, t[11].1)
        };
        let (mellow_mf, mellow_ff) = bell_at(0.0);
        let (mid_mf, mid_ff) = bell_at(0.5);
        let (bright_mf, bright_ff) = bell_at(1.0);
        println!(
            "bright partials re f0 at MIDI 63: mellow {mellow_mf:+.1}, centre {mid_mf:+.1}, bright {bright_mf:+.1} (the old knob: -37.3 / -18.6 / -8.0)"
        );
        println!(
            "                        at MIDI 127: mellow {mellow_ff:+.1}, centre {mid_ff:+.1}, bright {bright_ff:+.1} (the old knob: -13.2 / -4.8 / +0.2)"
        );
        assert!(mellow_mf < mid_mf - 6.0, "mellow did not dull the mezzo by 6 dB");
        assert!(bright_mf > mid_mf + 4.0, "bright did not brighten the mezzo by 4 dB");
        assert!(mellow_ff < mid_ff && bright_ff > mid_ff);
        for m in [8u8, 64, 127] {
            let mut soft = PianoKnobs::default();
            soft.voicing = 0.0;
            let mut hard = PianoKnobs::default();
            hard.voicing = 1.0;
            let a = contact_ms_on(&PianoBase::JAZZ, &soft, m);
            let b = contact_ms_on(&PianoBase::JAZZ, &hard, m);
            println!("MIDI {m}: contact {a:.2} ms mellow, {b:.2} ms bright");
            assert!(a > b, "softer felt should stay longer at MIDI {m}");
        }
    }

    /// The follower's seed against what the strike actually reaches, across
    /// the velocity range: `SEED_OF_FREE_PEAK` is the fraction this prints.
    #[test]
    fn the_follower_is_seeded_near_what_the_strike_reaches() {
        let knobs = PianoKnobs::default();
        let mut worst = 0.0_f32;
        for m in [8u8, 30, 64, 90, 127] {
            let mut v = PianoVoice::new(RATE);
            v.strike_based(60, f32::from(m) / 127.0, &knobs, &PianoBase::JAZZ);
            let seed = v.env;
            let out = run_voice(&mut v, 0.02);
            let reached = peak(&out);
            let free = PianoBase::JAZZ.hammer.free_peak_force(hammer::velocity_of_midi(f32::from(m) / 127.0)) * POUR;
            println!(
                "MIDI {m:3}: seed {seed:.4}, reached {reached:.4} ({:.2} of the free peak {free:.4}); seed / reached {:.2}",
                reached / free,
                seed / reached
            );
            worst = worst.max((seed / reached).max(reached / seed));
        }
        assert!(worst < 2.0, "the seed is off the strike's peak by {worst:.1}x");
    }

    /// A pianissimo strike from silence is not declared finished before its
    /// hammer has spoken. The follower's floor is 2e-5 and the felt's first
    /// samples are under it; the seed (`SEED_OF_FREE_PEAK`) is what carries
    /// the voice across, and with the seed at zero the frame would skip the
    /// voice on its first block.
    #[test]
    fn a_soft_strike_is_not_finished_before_the_hammer_speaks() {
        let mut v = PianoVoice::new(RATE);
        v.strike_based(60, 8.0 / 127.0, &PianoKnobs::default(), &PianoBase::JAZZ);
        let mut heard = 0.0_f32;
        for i in 0..(0.05 * RATE) as usize {
            let s = v.tick();
            heard = heard.max(s.abs());
            assert!(!v.finished(), "finished at sample {i}, having reached {heard}");
        }
        assert!(heard > 1.0e-3, "the soft strike never spoke: {heard}");
    }

    /// A second strike inside the first one's contact keeps the force
    /// CONTINUOUS: the hammer keeps its position and takes the new speed,
    /// so the output's first difference over the restrike is a lone
    /// strike's own to within a factor, and no step. Its slope does
    /// change — the second difference says by how much, and is printed,
    /// not judged: a strike 0.4 ms into another is a switch bounce, not a
    /// hand, and what the superposition law forbids is the step. The pour
    /// this replaces restarted its pulse from the tail value, which WAS a
    /// step.
    #[test]
    fn a_restrike_inside_the_contact_keeps_the_force_continuous() {
        let knobs = PianoKnobs::default();
        let d1 = |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        let d2 = |x: &[f32]| x.windows(3).map(|w| (w[2] - 2.0 * w[1] + w[0]).abs()).fold(0.0, f32::max);
        let mut lone = PianoVoice::new(RATE);
        lone.strike_based(60, 102.0 / 127.0, &knobs, &PianoBase::JAZZ);
        let lone_out = run_voice(&mut lone, 0.02);
        let mut v = PianoVoice::new(RATE);
        v.strike_based(60, 102.0 / 127.0, &knobs, &PianoBase::JAZZ);
        let mut out = Vec::new();
        for i in 0..(0.02 * RATE) as usize {
            if i == 20 {
                assert!(v.hammer.in_contact(), "the first hammer is not in contact at sample 20");
                // Re-voiced hard, on purpose: the felt under the string
                // must be the one the strike landed on (`strike_with`), or
                // the force jumps fivefold mid-contact.
                let mut bright = knobs;
                bright.voicing = 1.0;
                v.strike_based(60, 102.0 / 127.0, &bright, &PianoBase::JAZZ);
            }
            out.push(v.tick());
        }
        let (a1, b1) = (d1(&lone_out), d1(&out));
        let (a2, b2) = (d2(&lone_out), d2(&out));
        println!(
            "first difference: lone {a1:.5}, restrike inside the contact {b1:.5} ({:.2}x); second difference {a2:.5} vs {b2:.5} ({:.2}x)",
            b1 / a1,
            b2 / a2
        );
        assert!(b1 <= 2.0 * a1, "restrike stepped {b1} against a lone strike's {a1}");
    }

    /// The plate follows the patch: Piano 2 is strung over the concert
    /// board, Piano 1 over the jazz one, and switching between them
    /// re-designs the plate in place.
    #[test]
    fn the_plate_follows_the_patch() {
        let mut k = Keys::new(RATE);
        k.set_patch(Patch::Piano2);
        let concert: Vec<f32> = board::Board::new(RATE, board::Plate::Concert).modes().collect();
        let jazz: Vec<f32> = board::Board::new(RATE, board::Plate::Jazz).modes().collect();
        let got: Vec<f32> = k.body.board.modes().collect();
        assert_eq!(got, concert, "Piano 2 is not on the concert plate");
        k.set_patch(Patch::Piano1);
        let got: Vec<f32> = k.body.board.modes().collect();
        assert_eq!(got, jazz, "Piano 1 is not on the jazz plate");
    }

    /// **Acceptance B.** Under a held C4 the body's output carries energy
    /// at the plate's modes below 300 Hz — at least three of them — that
    /// stands 10 dB or more above the same measurement with the plate
    /// bypassed and the network alone.
    #[test]
    fn the_plate_rings_at_its_modes_under_a_held_c4() {
        let ev = [(0.0, Ev::On(60, 90)), (3.0, Ev::Off(60))];
        let with = body_only(&ev, 2.0, false);
        let without = body_only(&ev, 2.0, true);
        let w_with = window(&with, 0.05, 1.5);
        let w_without = window(&without, 0.05, 1.5);
        let modes: Vec<f32> = board::Board::new(RATE, PianoBase::JAZZ.plate).modes().filter(|&f| f < 300.0).collect();
        let mut clear = 0;
        for &f in &modes {
            let a = db(goertzel_hann(w_with, f, RATE));
            let b = db(goertzel_hann(w_without, f, RATE));
            println!("mode {f:6.1} Hz: plate in {a:6.1} dB, bypassed {b:6.1} dB, +{:.1}", a - b);
            if a - b >= 10.0 {
                clear += 1;
            }
        }
        println!("{clear} of {} modes below 300 Hz clear 10 dB", modes.len());
        assert!(clear >= 3, "only {clear} modes stand 10 dB over the network alone");
    }

    /// The body's level under `comp` with the plate in, against the
    /// network alone at the level it had before the plate — within 1 dB,
    /// with the two halves at equal power. Prints both halves, which is
    /// how `BOARD_SEND` was set.
    #[test]
    fn the_body_keeps_its_level_with_the_plate_in() {
        let ev = comp_events();
        let total = body_only(&ev, 9.0, false);
        let fdn = body_only(&ev, 9.0, true);
        let fdn_rms = rms(&fdn);
        // The network alone is at FDN_MIX of what it was; undo that to get
        // the body the brief measured.
        let before = fdn_rms / FDN_MIX;
        let plate: Vec<f32> = total.iter().zip(&fdn).map(|(a, b)| a - b).collect();
        let plate_rms = rms(&plate);
        let after = rms(&total);
        println!(
            "body under comp: before {:.1} dBFS; now network {:.1} + plate {:.1} = {:.1} dBFS ({:+.2} dB); plate/network {:+.1} dB",
            db(before), db(fdn_rms), db(plate_rms), db(after), db(after / before), db(plate_rms / fdn_rms)
        );
        assert!((db(after / before)).abs() <= 1.0, "the body moved {:+.2} dB", db(after / before));
        assert!((db(plate_rms / fdn_rms)).abs() <= 3.0, "the split is not near equal power");
    }

    /// Diagnostic: the restrike across one period of a ringing ff C4 —
    /// what the string was doing under the hammer, how the hammer left,
    /// and what the loop holds afterwards.
    #[test]
    #[ignore]
    fn scan_restrike_phase() {
        let knobs = PianoKnobs::default();
        let n0 = (0.30 * RATE) as usize;
        let period = (RATE / f0_of(60)) as usize + 1;
        let loop_energy = |v: &PianoVoice| -> f64 {
            v.strings.iter().map(|s| s.buf.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>()).sum()
        };
        let run = |restrike: Option<usize>| -> (f64, f64, f32, f32, f32) {
            let mut v = PianoVoice::new(RATE);
            v.strike_based(60, 1.0, &knobs, &PianoBase::JAZZ);
            let mut e = 0.0_f64;
            let mut rv = 0.0;
            let mut le = 0.0;
            let end = n0 + period + (0.5 * RATE) as usize;
            for i in 0..end {
                if Some(i) == restrike {
                    rv = rail_velocity(&v);
                    v.strike_based(60, 64.0 / 127.0, &knobs, &PianoBase::JAZZ);
                }
                if let Some(r) = restrike {
                    if i == r + (0.012 * RATE) as usize {
                        le = loop_energy(&v);
                    }
                } else if i == n0 + (0.012 * RATE) as usize {
                    le = loop_energy(&v);
                }
                let s = v.tick();
                if i >= n0 {
                    e += f64::from(s) * f64::from(s);
                }
            }
            (e, le, rv, v.hammer.velocity(), v.hammer.contact_secs() * 1e3)
        };
        let (bed, _, _, _, _) = run(None);
        println!("bed output energy {bed:.4}");
        let mut k = 0;
        while k < period {
            let (e, le, rv, out_v, c) = run(Some(n0 + k));
            // the bed's loop energy at the same instant
            let mut v = PianoVoice::new(RATE);
            v.strike_based(60, 1.0, &knobs, &PianoBase::JAZZ);
            for _ in 0..n0 + k + (0.012 * RATE) as usize {
                v.tick();
            }
            let le_bed = loop_energy(&v);
            println!(
                "offset {k:3}: rail v {rv:+.4}  added output {:+8.4}  loop energy after {le:.5} vs bed {le_bed:.5} ({:+.1}%)  hammer out {out_v:+.2} m/s  contact {c:.2} ms",
                e - bed,
                (le / le_bed - 1.0) * 100.0
            );
            k += 8;
        }
    }

    /// Timing, not a test: `cargo test --release timing -- --ignored
    /// --nocapture`. The plate's cost in situ, best of five, on the
    /// bench's `comp` figure and on a 64-voice pedalled ceiling, with the
    /// plate in and bypassed in the same process — the only way to read a
    /// few percent off a machine that is doing other things.
    #[test]
    #[ignore]
    fn timing_of_the_plate() {
        let mut ceiling = Vec::new();
        for p in 24..88u8 {
            ceiling.push((0.0, Ev::On(p, 100)));
        }
        for (label, events, secs) in [("comp", comp_events(), 13.4), ("64 voices held, pedal", ceiling, 5.0)] {
            for bypass in [true, false] {
                let mut best = f64::MAX;
                for _ in 0..5 {
                    let mut k = Keys::new(RATE);
                    k.set_patch(Patch::Piano1);
                    k.body.bypass_board(bypass);
                    if label.starts_with("64") {
                        k.set_pedal(true);
                    }
                    let total = ((secs * RATE) as usize / BLOCK + 1) * BLOCK;
                    let mut scratch = vec![0.0_f32; BLOCK * 2];
                    let mut next = 0;
                    let mut frame = 0;
                    let t0 = std::time::Instant::now();
                    while frame < total {
                        let end = (frame + BLOCK) as f32 / RATE;
                        while next < events.len() && events[next].0 < end {
                            match events[next].1 {
                                Ev::On(p, v) => k.note_on(p, f32::from(v) / 127.0),
                                Ev::Off(p) => k.note_off(p),
                            }
                            next += 1;
                        }
                        scratch.fill(0.0);
                        k.render(&mut scratch, BLOCK, 2);
                        std::hint::black_box(&scratch);
                        frame += BLOCK;
                    }
                    best = best.min(t0.elapsed().as_secs_f64());
                }
                println!(
                    "{label}: plate {}: {:.1} ms for {secs} s = {:.0}x real time ({:.1} ns/sample)",
                    if bypass { "bypassed" } else { "in" },
                    best * 1e3,
                    secs as f64 / best,
                    best * 1e9 / (secs as f64 * RATE as f64)
                );
            }
        }
    }

    /// **Scan, not a test.** `cargo test --release scan -- --ignored
    /// --nocapture` prints, for a grid of felt exponents and action maps,
    /// the contact at the ends, the mezzo partials against the signed-off
    /// table, and the velocity table's shape against the signed-off one —
    /// through the whole instrument, at Keys level, by striking the voice
    /// at the MIDI fraction that the candidate map would send the shipped
    /// map's hammer speed for. This is how the constants in `HammerBase`
    /// and `velocity_of_midi` were chosen.
    #[test]
    #[ignore]
    fn scan_hammers() {
        // Keys-level minus bare-voice offset, per measurement, on the
        // shipped hammer: the body and the pan, which are linear and the
        // same for every candidate.
        let knobs = PianoKnobs::default();
        let single_bare = |base: &PianoBase, m: f32| -> Vec<f32> {
            let mut v = PianoVoice::new(RATE);
            v.strike_based(60, m, &knobs, base);
            run_voice(&mut v, 2.1)
        };
        let ramp_bare = |base: &PianoBase, q: f32| -> Vec<f32> {
            let mut v = PianoVoice::new(RATE);
            let mut out = Vec::new();
            for i in 0..12 {
                let m = ((8 + i * 11).min(127) as f32 / 127.0).powf(q);
                v.silence();
                v.strike_based(64, m, &knobs, base);
                out.extend(run_voice(&mut v, 1.7));
            }
            out
        };
        let keys_p = partial_levels(&render(Patch::Piano1, &[], &[(0.0, Ev::On(60, 90)), (6.0, Ev::Off(60))], 2.2, false), 7);
        let bare_p = partial_levels(&single_bare(&PianoBase::JAZZ, 90.0 / 127.0), 7);
        let keys_t = velocity_table(&render(Patch::Piano1, &[], &velramp_events(), 20.5, false));
        let bare_t = velocity_table(&ramp_bare(&PianoBase::JAZZ, 1.0));
        let off_p: Vec<f32> = (0..7).map(|i| keys_p[i] - bare_p[i]).collect();
        let off_pk: Vec<f32> = (0..12).map(|i| keys_t[i].0 - bare_t[i].0).collect();
        let off_bell: Vec<f32> = (0..12).map(|i| keys_t[i].1 - bare_t[i].1).collect();
        println!("offsets keys-bare: partials {off_p:.1?} peak {off_pk:.1?} bell {off_bell:.1?}");
        // (grid, base, base tables, soft-end contact target, mass)
        let grids: [(&[f32], PianoBase, [f32; 7], [f32; 12], [f32; 12], f32, f32); 2] = [
            (&[2.2], PianoBase::JAZZ, BASE_PARTIALS, BASE_PEAK, BASE_BELL, 2.1, 1.0e-3),
            (&[2.2, 2.6, 3.0, 3.3], PianoBase::CONCERT, BASE2_PARTIALS, BASE2_PEAK, BASE2_BELL, 2.0, 1.1e-3),
        ];
        let q = hammer::MAP_EXPONENT;
        for (exps, base0, base_p, base_pk, base_bell, soft, mass) in grids {
            for &p in exps {
                let m8 = 8.0_f32 / 127.0;
                // Bisect K so the MIDI 8 contact is `soft` ms.
                let (mut lo, mut hi) = (1.0e7_f32, 1.0e14_f32);
                let mut k = 1.0e10;
                for _ in 0..40 {
                    k = (lo.ln() * 0.5 + hi.ln() * 0.5).exp();
                    let base = PianoBase {
                        hammer: hammer::HammerBase { mass, stiffness: k, exponent: p, ..base0.hammer },
                        ..base0
                    };
                    let mut v = PianoVoice::new(RATE);
                    v.strike_based(60, m8, &knobs, &base);
                    for _ in 0..(0.05 * RATE) as usize {
                        v.tick();
                        if v.hammer.idle() { break; }
                    }
                    if v.hammer.contact_secs() * 1e3 > soft { lo = k } else { hi = k }
                }
                let base = PianoBase {
                    hammer: hammer::HammerBase { mass, stiffness: k, exponent: p, ..base0.hammer },
                    ..base0
                };
                let contact = |midi: f32| -> f32 {
                    let mut v = PianoVoice::new(RATE);
                    v.strike_based(60, midi / 127.0, &knobs, &base);
                    for _ in 0..(0.05 * RATE) as usize {
                        v.tick();
                        if v.hammer.idle() { break; }
                    }
                    v.hammer.contact_secs() * 1e3
                };
                let pl = partial_levels_of(&single_bare(&base, 90.0 / 127.0), 7, base.stiff);
                let t = velocity_table(&ramp_bare(&base, 1.0));
                let dp: Vec<f32> = (0..7).map(|i| pl[i] + off_p[i] - base_p[i]).collect();
                let pk: Vec<f32> = (0..12).map(|i| t[i].0 + off_pk[i] - base_pk[i]).collect();
                let gain = pk.iter().sum::<f32>() / 12.0;
                let pk: Vec<f32> = pk.iter().map(|x| x - gain).collect();
                let bell: Vec<f32> = (0..12).map(|i| t[i].1 + off_bell[i] - base_bell[i]).collect();
                print!(
                    "q {q:.2} p {p:.1} M {:.1} g K {k:.2e}: contact {:.2}/{:.2}/{:.2}/{:.2}/{:.2}/{:.2} ms  gain {gain:+.1} dB  dPartials",
                    mass * 1e3, contact(8.0), contact(19.0), contact(30.0), contact(52.0), contact(90.0), contact(127.0)
                );
                for x in &dp { print!(" {x:+.1}"); }
                print!("  dPeak");
                for x in &pk { print!(" {x:+.1}"); }
                print!("  dBell");
                for x in &bell { print!(" {x:+.1}"); }
                print!("  asym");
                for r in &t { print!(" {:.2}", r.2); }
                println!();
            }
        }
    }
}
