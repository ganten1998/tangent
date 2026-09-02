//! A record/repro tape path, after the Tascam 388 at its fixed 7½ IPS.
//!
//! Eight tracks on quarter-inch tape is under a sixteenth of an inch each, and
//! the narrow track at a middling speed is the whole voicing: a top end that
//! the spec sheet stops quoting a hair under 16 kHz, a head bump low around
//! 70 Hz, hiss a person can actually hear, and a transport whose wow is part
//! of why anybody still wants one.
//!
//! # An insert, not an echo
//!
//! The transport physics here descend from the tape echo in `effects.rs` —
//! the phase-accumulator wow and flutter, the tanh record amplifier, the head
//! bump, hiss as a modelled signal rather than a defect — but the topology is
//! the opposite. An echo is a feedback loop, and everything it does compounds
//! per trip. This is one pass: signal goes onto the tape once, comes off it
//! once, and the machine's opinions are applied exactly once each. So the
//! per-pass losses that an echo can leave subtle have to carry the sound on
//! their own here, and each one is a knob.
//!
//! # The transient story, which is the point of the unit
//!
//! **Tape does not duck attacks; it dulls them.** At high record levels the
//! shortest wavelengths on the tape partially erase themselves as they are
//! written — the freshly-magnetised domains sit in the write field long enough
//! to be turned back. The result is program-dependent treble loss: hit the
//! machine hard and the attack loses its 8 kHz before it loses any level at
//! all. That is why a snare through a 388 sounds rounded rather than turned
//! down, and it is a thing no gain element — no compressor — can do, because a
//! gain element takes the same decibels from every band. Here it is a
//! dual-rate envelope follower on the record level driving a one-pole corner
//! downward, and the test proves the band ratio a gain could not produce.
//!
//! # Signal flow
//!
//! sub roll (one-pole high-pass, the transformer bottom) → record, at four
//! times the session rate: bias-linearised tanh with the refused flux fed
//! back, then the repro line-up trim → HF self-erasure (the envelope-driven
//! corner above) → the tape: a short Hermite-interpolated play head swayed by
//! wow and flutter whose depths are themselves noise-modulated, plus a
//! whisper of scrape → repro EQ: head-bump peak and HF roll → shaped hiss,
//! level-tracked → output trim. The `drive` knob is also the record level,
//! and the repro trim that follows it is the machine's line-up, not a knob;
//! the only trim a person turns is on the way out.
//!
//! # Why the record stage runs at four times the rate
//!
//! A tanh evaluated once per sample is a memoryless nonlinearity, and a
//! memoryless nonlinearity generates harmonics without limit — every one of
//! them above Nyquist folds back into the band as inharmonic hash, at the
//! frequency it lands on rather than the one it was born at, which no filter
//! downstream can ever separate from the music. The knob was driven fourteen
//! decibels harder than it used to be (see [`DRIVE_TANH`]), and measured on a
//! 7 kHz tone at −6 dBFS at full travel the fold reached −9.7 dB relative to
//! the tone itself: a third of the output was aliasing. So the record
//! amplifier and its feedback both run at 4× through a two-stage
//! windowed-sinc pair, exactly as `units::drive` runs its diode clipper, and
//! the same measurement now reads better than 30 dB down.
//!
//! The pair costs [`OS_LATENCY`] samples of group delay, and it costs the
//! unit nothing, because this unit already had a play head: the head's rest
//! position is shortened by exactly that many samples, so the reported and
//! the real latency are both still [`BASE_MS`].
//!
//! # Params
//!
//! Normalised 0..=1, like everything in `effects.rs`; the mapping to whatever
//! the DSP actually wants lives in the consts next to each stage. Design math
//! runs in f64, audio in f32, and every voicing frequency is written in hertz
//! so the unit is the same machine at every sample rate.

// ── what a person can change ────────────────────────────────────────────────

/// The six knobs. Defaults are the shipped sound: a track of the machine at a
/// healthy record level, not a bypass and not a caricature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TapeParams {
    /// Record level into the saturation, and with it how hard the
    /// self-erasure leans on attacks. **0 is the wire** — a record level's
    /// "off" is the bottom of its travel, not the middle of it — and the
    /// shipped 0.6 is the machine at a healthy 0 VU. See [`DRIVE_TANH`].
    pub drive: f32,
    /// How much head bump. 0 is flat; see [`BUMP_DB`].
    pub bump: f32,
    /// How much high end the repro chain gives up. 0 is out of circuit; see
    /// [`ROLL_HZ`].
    pub roll: f32,
    /// Wow and flutter together, one master. 0 parks the play head; see
    /// [`WOW_DEPTH_MS`].
    pub wobble: f32,
    /// Tape hiss. 0 is silent tape, which no real reel ever was; see
    /// [`HISS_MAX`].
    pub hiss: f32,
    /// Output trim. 0.5 is unity; see [`TRIM_DB`].
    pub trim: f32,
}

impl Default for TapeParams {
    fn default() -> Self {
        Self {
            // A healthy 0 VU: the peaks round audibly, the body does not.
            drive: 0.6,
            // +2.5 dB at 70 Hz, which is the machine's own figure.
            bump: 0.5,
            // Parks the repro corner where the spec sheet stops, just under
            // 16 kHz.
            roll: 0.12,
            // Present on sustained piano, invisible on speech.
            wobble: 0.3,
            // Audible in the gaps if you listen for it, like the real thing.
            hiss: 0.25,
            trim: 0.5,
        }
    }
}

// ── the voicing ─────────────────────────────────────────────────────────────

/// Sample rate every length below is designed at; runtime rates scale from
/// here. Mirrors the private constant of the same name in `effects.rs` — the
/// house sample-rate law, restated because this module cannot reach that one.
const DESIGN_RATE: f32 = 44_100.0;

/// The tanh drive factor at each end of the knob, swept geometrically.
///
/// **`d` is the record level, and it is easiest to read as where the tape
/// saturates.** `tanh(d·x)/d` is exactly "gain up by `d`, into a fixed tape
/// characteristic, and back down by `d`" — the record amplifier and the repro
/// trim of a real machine, in one expression — so its SMALL-signal gain is
/// one at any setting. It is not one at program level, and the sentence that
/// used to stand here — "the knob never doubles as a volume control" — was
/// the small-signal fact overreaching into a claim about music. What a
/// compressive curve costs a signal depends on how far up the curve the
/// signal sits: measured on broadband programme peaking at −18 dBFS, the top
/// of this dial gave away 11.8 dB. [`sat_makeup`] is the answer, and it is the answer a real
/// machine uses — line the repro amplifier up so the alignment tone comes
/// back at the level it went in at, and let everything hotter than the tone
/// pay the compression, which is the effect. What the dial costs now is in
/// [`sat_makeup`]'s own note, pinned by a test at the shipped default. The
/// shoulder sits at `x ≈ 1/d`: at the bottom of the travel that is +20 dBFS,
/// which nothing can reach, and at the top it is −26 dBFS, which is under
/// where a track's BODY sits. Bias is why the curve is a smooth tanh at all:
/// an unbiased head has a dead zone at the origin and crossover fuzz, and the
/// AC bias every mastering engineer calibrated is precisely the linearisation
/// that leaves only this compression behind.
///
/// # Why the top is 20 and not 4
///
/// It was 4, and at 4 the knob was a control that measured live and sounded
/// dead. A DAW track does not arrive at full scale; alignment puts 0 VU at
/// −18 dBFS, so a healthy body is an amplitude near 0.126, and against that a
/// factor of 4 is a shoulder the signal never reaches. Measured end to end on
/// a −20 dBFS tone the old knob ran from 0.001 % distortion to 0.96 %, with
/// three quarters of its travel under 0.15 %, and the self-erasure it is
/// supposed to drive tilted 8 kHz against 200 Hz by 0.8 dB at the very top —
/// measurably wired, audibly nothing, which is the worst of the three ways a
/// control can be broken. Every test in this module drove the unit at −6 dBFS
/// or hotter, where the knob DID work, so nothing here saw it.
///
/// The bottom of the travel is untouched, so "0 is a wire" still means what it
/// said. The top now reaches the saturation the unit is named after at the
/// level people actually record at: the same sweep runs to 17 % and tilts the
/// band by 8 dB, the shipped position rounds a −6 dBFS peak by several percent
/// and pulls its top to around 4 kHz while leaving the body alone, and full
/// travel is a machine being slammed.
///
/// # What the range change did to saved values, and why nothing migrates it
///
/// The dial persists as its raw 0..=1 position under a state header that did
/// not change with it, so a `drive` saved by a build from before this reads
/// back as a hotter record level: the old top, `d = 4`, now sits at 0.696 of
/// the travel (`ln(10·4)/ln(200)`, the new mapping inverted), and a stored
/// 1.0 that used to mean 1.5 % distortion means 21.9 % today. That is
/// deliberate and it is stated in the changelog rather than migrated,
/// because this unit has never appeared in a tagged build — there is no
/// session in the world holding the old meaning. The day one ships, this
/// paragraph becomes a migration instead.
const DRIVE_TANH: (f32, f32) = (0.1, 20.0);

/// How much of the flux the tape refused comes back one SESSION sample later.
///
/// The magnetisation a saturated tape fails to take does not vanish — the
/// unclosed domains push back on the field that is still being written. Fed
/// forward, the refusal makes the compression program-dependent: a lone
/// transient is rounded a little less than a sustained scream, which is the
/// difference between tape and a waveshaper table. Exactly zero in the
/// linear region, because there the tape takes everything.
///
/// **The delay is a TIME, so oversampling must not shorten it.** The loop
/// runs at 4× now, and a plain one-sample memory there would be a quarter of
/// the memory it was — a silent quartering of the one mechanism that makes
/// this saturation program-dependent at all. So the refusal is held per
/// oversampled sub-phase ([`Tape::sat_err`] is `[[f32; OS]; 2]`): the value
/// read at sub-phase `k` is the one written at sub-phase `k` of the previous
/// session sample, which is `z⁻⁴` at 4× and therefore exactly the `z⁻¹` this
/// constant has always meant.
const SAT_FB: f32 = 0.35;

/// How far above the session rate the record stage runs, and the two
/// windowed-sinc filters that take it there and back.
///
/// The design is `units::drive`'s, for the reason that module argues at
/// length and this one inherits whole: `B` runs at 4× where every fold lands
/// deep in its stopband, so it is a short true halfband, while `A` runs at 2×
/// and guards the fold onto the AUDIBLE band — and an exact halfband is the
/// wrong tool there, because its transition straddles the fold point and
/// content just above Nyquist would come back just below it at about −10 dB.
/// So `A` gets more taps and a cutoff shaded to [`OS_A_CUTOFF`], which moves
/// the whole fold region into true stopband.
///
/// Restated here rather than imported because those items are private to that
/// module, the same reason [`DESIGN_RATE`], [`flush`] and [`TINY`] are
/// restated at the top of this file. Two numbers are deliberately NOT the
/// amp's:
///
/// - **The 4× stage is 21 taps, not 23**, so that the pair's total group
///   delay is a whole number of session samples ([`OS_LATENCY`]) and the play
///   head can absorb it exactly.
/// - **The 2× stage is 79 taps at a cutoff of 0.235, not 63 at 0.224.** The
///   amp's shorter filter is 1.3 dB down at 20 kHz, and the signal crosses it
///   twice: 2.6 dB off the top of the band at the setting this unit calls a
///   wire. An overdrive pedal that rolls off at 6 kHz never notices; a tape
///   machine whose entire subject is the top of the band cannot spend it. The
///   longer filter buys the passband back — under a tenth of a dB at 20 kHz —
///   and still puts the whole fold region in true stopband: 20 dB down at
///   24 kHz, 42 at 25, past 80 by 26, so the worst alias it can pass sits at
///   23 kHz, 42 dB down, and everything below 22 kHz is 80 dB down or
///   better.
const OS: usize = 4;
const OS_A_TAPS: usize = 79;
const OS_B_TAPS: usize = 21;

/// Stage A's cutoff as a fraction of its own running rate; 0.25 would be the
/// exact halfband. Shaded below the fold for the reason [`OS`] gives.
const OS_A_CUTOFF: f64 = 0.235;

/// What the up/down pair delays the record stage by, in SESSION samples.
///
/// Each linear-phase filter delays by half its taps at its OWN rate, and the
/// signal meets four of them. The 2× pair is 39 taps' worth each, which is
/// 19½ session samples each and 39 for the two; the 4× pair is 10 each, 2½
/// session samples each and 5 for the two. Forty-four in all — 0.9 ms in a
/// 48 kHz session. Whole, by the tap counts chosen in [`OS`]: a half-sample
/// here would have to be rounded away somewhere, and the place it would land
/// is the head position that keeps the unit's reported latency honest.
const OS_LATENCY: usize = (OS_A_TAPS - 1) / 2 + (OS_B_TAPS - 1) / 4;

/// The line-up level the repro trim is calibrated at, in dBFS.
///
/// **0 VU, which this app puts at −18 dBFS.** A machine is lined up by
/// recording a tone at 0 VU and setting the repro amplifier so it comes back
/// at 0 VU; [`sat_makeup`] does exactly that arithmetic, so the alignment
/// level is the one place on the dial where the record level is guaranteed
/// not to be a volume control.
const ALIGN_DBFS: f64 = -18.0;

/// The record chain's bottom, as a one-pole high-pass corner in hertz.
///
/// Input transformers and the head's own low-end limit: nothing below the
/// mid-30s comes back off a narrow track, and letting it into the saturation
/// just wastes headroom on rumble.
const SUB_ROLL_HZ: f32 = 35.0;

/// Where the self-erasure corner sits with nothing on the tape, in hertz.
///
/// Above the audio band on purpose: at rest the stage is a wire. As the
/// record level rises the corner falls by [`ERASE_OCTAVES`], and the audible
/// "16 kHz-and-falling" tape top arrives around a quarter of full level.
const ERASE_TOP_HZ: f32 = 22_000.0;

/// How far the corner falls, in octaves per unit of record-level envelope,
/// and the floor it can never pass. Two octaves at full level puts a slammed
/// track's top near 5 kHz, which is what a pinned VU meter sounds like.
const ERASE_OCTAVES: f32 = 2.0;
const ERASE_FLOOR_HZ: f32 = 2_500.0;

/// The dual rates of the envelope that drives the corner.
///
/// **Fast in, slow out, and the asymmetry IS the transient rounding.** The
/// erasure happens at write time, so the corner has to fall as fast as the
/// attack arrives — a millisecond. It recovers at the speed the level
/// actually leaves the tape's saturation region, which is the tens of
/// milliseconds a note body takes to settle.
const ERASE_ATTACK_MS: f32 = 1.0;
const ERASE_RELEASE_MS: f32 = 80.0;

/// The play-head line: its capacity and where the head sits in it, in
/// milliseconds. Two milliseconds of base delay is enough for the sway to
/// never run the head off either end and short enough that the unit stays an
/// insert rather than a slap; [`Tape::latency_frames`] reports it.
///
/// The head's REST position is this less [`OS_LATENCY`], because the record
/// stage's oversampler is already holding that many samples: the two together
/// come to [`BASE_MS`] and that is what the host is told. The head is still
/// far longer than the sway, which is a fifth of a millisecond at its
/// deepest — see [`WOW_DEPTH_MS`].
const LINE_MS: f32 = 15.0;
const BASE_MS: f32 = 2.0;

/// Wow: the slow one. Under 1 Hz — the reel servo hunting and the tape
/// packing unevenly — and the depth in absolute milliseconds of sway, so the
/// pitch bend it makes is the same at every sample rate.
const WOW_HZ: f32 = 0.6;
const WOW_DEPTH_MS: f32 = 0.20;

/// Flutter: the fast pair, in hertz. Two rates rather than one because a
/// transport has more than one rotating part — a pinch-roller rate and a
/// capstan-bearing rate — and a single sinusoid reads as a test tone where
/// two beat against each other and read as a machine.
const FLUTTER_HZ: (f32, f32) = (6.3, 11.0);
const FLUTTER_DEPTH_MS: (f32, f32) = (0.012, 0.005);

/// The depth wander: how far the noise is allowed to breathe the wow and
/// flutter depths, as a range of scale, and how the noise moves.
///
/// **A transport's drag is not a metronome.** Real wow and flutter spectra
/// are smeared, not lines, because the friction that causes them varies with
/// the wind of the reel. Sampled-and-held uniform noise, slewed, scales the
/// sinusoids: held long enough to read as drift, slewed so the depth never
/// steps audibly.
const DEPTH_WANDER: (f32, f32) = (0.5, 0.95);
const WOW_HOLD_S: f32 = 0.7;
const FLUTTER_HOLD_S: f32 = 0.23;
const WANDER_SLEW_S: f32 = 0.1;

/// Scrape: the head's own whisper, as band-limited noise swaying the tap by
/// fractions of a microsecond. Tape dragging on a fixed head sticks and
/// slips at kilohertz rates; the sidebands it sprays are 50 dB down, felt as
/// a faint graininess rather than heard as anything.
const SCRAPE_MS: f32 = 0.000_5;
const SCRAPE_BAND_HZ: (f32, f32) = (800.0, 3_000.0);

/// The head bump: where and how much, as a single peaking biquad.
///
/// A record head is a magnetic circuit with a physical length, and at the
/// wavelength that matches it the response lifts. On this machine the lift
/// is centred near 70 Hz and the service data calls it about +2.5 dB, which
/// the knob's midpoint reproduces; full travel doubles it for effect.
const BUMP_HZ: f64 = 70.0;
const BUMP_Q: f64 = 1.1;
pub const BUMP_DB: (f32, f32) = (0.0, 5.0);

/// The repro HF roll, as a one-pole corner swept geometrically.
///
/// Gap loss and spacing loss, the fixed part of playing back a narrow track.
/// Zero is out of circuit; the default parks the corner just under 16 kHz,
/// which is where this machine's own response quietly stops; full travel is
/// a worn head that never saw a cotton swab.
pub const ROLL_HZ: (f32, f32) = (20_000.0, 2_500.0);

/// The hiss shaper: three one-pole corners in hertz and their injection
/// gains, plus a direct white term.
///
/// **Tape hiss is pink-ish, not white**, because the repro chain's
/// equalisation tilts the head's flat noise downward — a white hiss is a
/// digital tell. The three-pole ladder is the standard economy pinking
/// arrangement: each pole hands off to the next an octave-ish apart, and the
/// sum falls within a fraction of a dB per octave of true −3. The corners
/// are in hertz and the gains are rescaled per rate so the same colour comes
/// out of every machine.
const PINK_HZ: [f64; 3] = [16.5, 264.6, 3_946.0];
const PINK_GAIN: [f64; 3] = [0.099_046, 0.296_516_4, 1.052_691_3];
const PINK_WHITE: f64 = 0.184_8;

/// How loud the hiss can get, in full scale at the top of the knob, and how
/// it follows the program.
///
/// **The floor is why silence still sounds like tape.** The tracker raises
/// the hiss with the music — the machine's electronics and the tape's own
/// modulation noise ride the level — and lets it fall back when the music
/// stops, but never to nothing: a stopped mix on a 388 still has tape
/// against the head. Squared knob taper, because ears meet noise in dB.
const HISS_MAX: f32 = 2.0e-3;
const HISS_FLOOR: f32 = 0.3;
const HISS_ATTACK_MS: f32 = 10.0;
const HISS_RELEASE_MS: f32 = 300.0;

/// Output trim, in dB across the knob. Midpoint is exactly unity.
pub const TRIM_DB: (f32, f32) = (-12.0, 12.0);

// ── small parts ─────────────────────────────────────────────────────────────

/// Below this a sample is on its way to a denormal and worth nothing.
/// Mirrors `effects::flush`, which is private to that module.
const TINY: f32 = 1.0e-20;

#[inline]
fn flush(x: f32) -> f32 {
    if x.abs() < TINY {
        0.0
    } else {
        x
    }
}

/// A knob value as the DSP may use it: 0..=1, and never NaN. The same
/// contract `Params::sane` enforces at the settings boundary, enforced here
/// because a kernel cannot know who loaded its params from what file.
#[inline]
fn norm(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.5
    }
}

/// The exact one-pole coefficient for a corner in hertz — `1 - e^-w`, not
/// the `w` approximation, because the erasure corner lives near the top of
/// the band where the two stop agreeing.
#[inline]
fn lp_a(sample_rate: f64, hz: f64) -> f32 {
    let w = std::f64::consts::TAU * (hz / sample_rate).clamp(1.0e-5, 0.49);
    (1.0 - (-w).exp()) as f32
}

/// The per-sample coefficient of an envelope with a time constant in
/// milliseconds.
#[inline]
fn env_a(sample_rate: f64, ms: f64) -> f32 {
    (1.0 - (-1.0 / (sample_rate * ms * 1.0e-3)).exp()) as f32
}

/// Xorshift white noise in −1..1. Tape noise, not a number anybody is
/// betting on; it must not allocate or lock. Same idiom as the echo's.
#[inline]
fn white(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// Xorshift uniform in 0..1, for the sampled-and-held depth wander.
#[inline]
fn uniform(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state as f32 / u32::MAX as f32
}

/// Modified Bessel I₀ by its power series — all the Kaiser window below
/// needs, and it converges in a few dozen terms for any β this file uses.
/// `units::drive`'s, restated: see [`OS`].
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    for k in 1..60 {
        term *= (x * x * 0.25) / ((k * k) as f64);
        sum += term;
        if term < sum * 1.0e-18 {
            break;
        }
    }
    sum
}

/// A Kaiser-windowed sinc low-pass with its cutoff at `fc` of the running
/// rate, DC gain one. At `fc = 0.25` this is the classic halfband — every
/// second off-centre tap a true zero.
fn windowed_sinc<const N: usize>(beta: f64, fc: f64) -> [f32; N] {
    let m = (N - 1) as f64 / 2.0;
    let denom = bessel_i0(beta);
    let mut taps = [0.0_f32; N];
    for (i, t) in taps.iter_mut().enumerate() {
        let k = i as f64 - m;
        let ideal = if k == 0.0 {
            2.0 * fc
        } else {
            (std::f64::consts::TAU * fc * k).sin() / (std::f64::consts::PI * k)
        };
        let r = k / m;
        let win = bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / denom;
        *t = (ideal * win) as f32;
    }
    taps
}

/// A direct-form FIR delay line. The taps live outside it so both channels
/// and both directions share one designed set.
#[derive(Clone)]
struct Fir<const N: usize> {
    z: [f32; N],
    pos: usize,
}

impl<const N: usize> Fir<N> {
    fn new() -> Self {
        Self {
            z: [0.0; N],
            pos: 0,
        }
    }

    #[inline]
    fn run(&mut self, taps: &[f32; N], x: f32) -> f32 {
        self.pos = if self.pos == 0 { N - 1 } else { self.pos - 1 };
        self.z[self.pos] = x;
        let mut acc = 0.0_f32;
        let (head, tail) = self.z.split_at(self.pos);
        for (t, v) in taps.iter().zip(tail.iter().chain(head.iter())) {
            acc += t * v;
        }
        acc
    }

    fn clear(&mut self) {
        self.z = [0.0; N];
        self.pos = 0;
    }
}

/// One channel's four delay lines: up through 2× then 4×, and back down the
/// same way.
#[derive(Clone)]
struct OsChannel {
    up_a: Fir<OS_A_TAPS>,
    up_b: Fir<OS_B_TAPS>,
    dn_b: Fir<OS_B_TAPS>,
    dn_a: Fir<OS_A_TAPS>,
}

impl OsChannel {
    fn new() -> Self {
        Self {
            up_a: Fir::new(),
            up_b: Fir::new(),
            dn_b: Fir::new(),
            dn_a: Fir::new(),
        }
    }

    fn clear(&mut self) {
        self.up_a.clear();
        self.up_b.clear();
        self.dn_b.clear();
        self.dn_a.clear();
    }
}

/// The repro amplifier's line-up: the gain that puts a 0 VU tone back where
/// it came in, for a record level of `d`.
///
/// **This is the machine's own calibration, not a makeup control.** A tape
/// machine is lined up by recording a tone at [`ALIGN_DBFS`] and trimming the
/// repro amplifier until it plays back at the level it went in at; every
/// record level has its own trim and the engineer sets it once. Here it is
/// solved instead of turned: the RMS the record curve returns for that tone,
/// backed out, by midpoint quadrature over a QUARTER cycle. A quarter is the
/// whole average because `tanh²` is even and `sin` mirrors about π/2, so the
/// other three quarters are copies; 64 points there agrees with 20 000 to six
/// decimals, which is why this can sit in the per-block design math.
///
/// **What it does and does not restore.** It pins the level exactly where the
/// music sits and nowhere else, which is the whole of its job: quieter than
/// the tone rides up, hotter than the tone still pays, and the difference
/// between those two IS the compression the dial exists to buy. Measured on a
/// programme at the line-up level the dial now costs 0.27 dB at the shipped
/// 0.6 and 1.71 dB at the very top, where before it cost 0.65 and 10.6. On
/// FLAT white noise the same sweep still costs 1.4 dB and 5.6 — and that
/// difference is the whole point: white noise puts a third of its energy
/// above 8 kHz, where no tape has any, and what it is losing up there is the
/// self-erasure taking the top off the band. That is a tone change, it is the
/// effect the unit is named for, and a line-up that chased it would be
/// undoing the thing it is meant to make audible.
///
/// [`SAT_FB`] is deliberately absent from the calculation. Its entire purpose
/// is to make the curve depend on what came before it, and a calibration that
/// depended on the program would not be a calibration.
fn sat_makeup(d: f64) -> f32 {
    const STEPS: usize = 64;
    let amp = 10.0_f64.powf(ALIGN_DBFS / 20.0) * std::f64::consts::SQRT_2;
    let mut sum = 0.0_f64;
    for i in 0..STEPS {
        let th = std::f64::consts::FRAC_PI_2 * (i as f64 + 0.5) / STEPS as f64;
        let y = (d * amp * th.sin()).tanh() / d;
        sum += y * y;
    }
    let out_rms = (sum / STEPS as f64).sqrt();
    let in_rms = amp / std::f64::consts::SQRT_2;
    (in_rms / out_rms.max(1.0e-12)) as f32
}

/// Four-point third-order Hermite, the polynomial from `varispeed.rs`.
///
/// The echo reads its tape with linear interpolation and gets away with it
/// because every repeat is also low-passed; this path is heard ONCE and
/// wide open, and linear interpolation's error is a lowpass whose corner
/// moves with the sway — a dullness that breathes. Hermite is two more
/// multiplies and flat enough that what you hear is the wow, not the
/// arithmetic.
#[inline]
fn hermite(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c0 = y1;
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + c0
}

/// Read `delay` samples behind `at`, Hermite-interpolated. `delay` must be
/// clamped by the caller to `2.0..=(len - 4)`, which `process` does.
#[inline]
fn tap(buf: &[f32], at: usize, delay: f32) -> f32 {
    let n = buf.len();
    let whole = delay as usize;
    let frac = delay - whole as f32;
    // `+ n` before the subtraction: `at` is smaller than `whole` for the
    // first pass through the buffer, and this is unsigned.
    let i1 = (at + n - whole) % n;
    let i0 = (i1 + 1) % n;
    let i2 = (i1 + n - 1) % n;
    let i3 = (i1 + n - 2) % n;
    hermite(buf[i0], buf[i1], buf[i2], buf[i3], frac)
}

/// One peaking biquad, transposed direct form II — the same form as
/// `effects::Biquad`, which only knows low-pass and high-pass. Coefficients
/// from the analog peaking prototype via the bilinear transform, designed in
/// f64 because a 70 Hz centre at 96 kHz puts `cos w0` five decimals from 1.
#[derive(Debug, Clone, Copy, Default)]
struct Peak {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Peak {
    fn set(&mut self, sample_rate: f64, hz: f64, q: f64, db: f64) {
        let a = 10.0_f64.powf(db / 40.0);
        let w0 = std::f64::consts::TAU * (hz / sample_rate).clamp(1.0e-5, 0.49);
        let alpha = w0.sin() / (2.0 * q);
        let cs = w0.cos();
        let a0 = 1.0 + alpha / a;
        // At 0 dB the numerator and denominator are the same polynomial, so
        // the section is exactly a wire — no epsilon bypass needed.
        self.b0 = ((1.0 + alpha * a) / a0) as f32;
        self.b1 = ((-2.0 * cs) / a0) as f32;
        self.b2 = ((1.0 - alpha * a) / a0) as f32;
        self.a1 = ((-2.0 * cs) / a0) as f32;
        self.a2 = ((1.0 - alpha / a) / a0) as f32;
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = flush(self.b1 * x - self.a1 * y + self.z2);
        self.z2 = flush(self.b2 * x - self.a2 * y);
        y
    }

    fn clear(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// The pinking ladder: white noise into three one-poles whose corners are in
/// hertz, plus the direct term. See [`PINK_HZ`].
#[derive(Debug, Clone, Copy)]
struct Pink {
    p: [f32; 3],
    g: [f32; 3],
    b: [f32; 3],
    rng: u32,
    seed: u32,
}

impl Pink {
    fn new(sample_rate: f32, seed: u32) -> Self {
        let mut p = [0.0_f32; 3];
        let mut g = [0.0_f32; 3];
        for i in 0..3 {
            let pole = |rate: f64| (-std::f64::consts::TAU * PINK_HZ[i] / rate).exp();
            let ps = pole(f64::from(sample_rate));
            let p44 = pole(f64::from(DESIGN_RATE));
            p[i] = ps as f32;
            // Same DC gain per section at every rate, so the colour designed
            // at 44.1k comes out of a 96k machine too.
            g[i] = (PINK_GAIN[i] * (1.0 - ps) / (1.0 - p44)) as f32;
        }
        Self {
            p,
            g,
            b: [0.0; 3],
            rng: seed,
            seed,
        }
    }

    #[inline]
    fn step(&mut self) -> f32 {
        let w = white(&mut self.rng);
        let mut sum = 0.0;
        for i in 0..3 {
            self.b[i] = flush(self.p[i] * self.b[i] + w * self.g[i]);
            sum += self.b[i];
        }
        sum + w * PINK_WHITE as f32
    }

    fn clear(&mut self) {
        self.b = [0.0; 3];
        self.rng = self.seed;
    }
}

// ── the machine ─────────────────────────────────────────────────────────────

/// One stereo pair of tracks through the record and repro chain. See the
/// module header for the signal flow.
pub struct Tape {
    sample_rate: f32,
    /// The tape: one line per channel, because the two tracks are magnetised
    /// independently — but read by one head at one place, which is why the
    /// sway below is shared.
    line: [Vec<f32>; 2],
    at: usize,
    /// The play head's rest position in whole samples. Whole on purpose:
    /// with the wobble at zero the tap interpolates at exactly zero fraction
    /// and hands samples through untouched. Shortened by [`OS_LATENCY`], for
    /// the reason [`BASE_MS`] gives.
    base: usize,
    /// What the head and the record stage's oversampler delay BETWEEN them,
    /// which is what a host with delay compensation has to be told.
    latency: usize,
    /// Sub-roll one-pole state, per channel; the high-pass is `x` minus it.
    sub_lp: [f32; 2],
    /// The record stage's oversampler: one designed set of taps, and the four
    /// delay lines each channel walks them with. See [`OS`].
    taps_a: [f32; OS_A_TAPS],
    taps_b: [f32; OS_B_TAPS],
    os: [OsChannel; 2],
    /// The flux the tape refused one session sample ago, held per oversampled
    /// sub-phase. See [`SAT_FB`] for why it is a sub-phase and not a sample.
    sat_err: [[f32; OS]; 2],
    /// The record-level envelope that drives the self-erasure, and the
    /// erasure one-pole itself.
    erase_env: [f32; 2],
    erase_lp: [f32; 2],
    /// Repro EQ state.
    bump: [Peak; 2],
    roll_lp: [f32; 2],
    /// Hiss: a pinking ladder per channel — two tracks, two noises — and the
    /// shared program tracker that rides them up and down.
    pink: [Pink; 2],
    hiss_env: f32,
    /// Where the modulators are, in turns. One set for both channels: one
    /// capstan.
    wow: f32,
    flutter: [f32; 2],
    /// The depth wander: sample-and-hold targets, slewed states, countdowns.
    wander_rng: u32,
    wow_scale: f32,
    wow_target: f32,
    wow_hold: u32,
    flutter_scale: f32,
    flutter_target: f32,
    flutter_hold: u32,
    /// Scrape band-limit states.
    scrape_lo: f32,
    scrape_hi: f32,
    // Per-rate coefficients, fixed at construction — the sample rate is,
    // too, per the house law.
    a_sub: f32,
    a_att: f32,
    a_rel: f32,
    a_hatt: f32,
    a_hrel: f32,
    a_scrape_lo: f32,
    a_scrape_hi: f32,
    a_wander: f32,
}

impl Tape {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let cap = (LINE_MS * 1.0e-3 * sr) as usize + 8;
        // The head gives up what the oversampler already holds, so the two
        // together are still BASE_MS. `saturating_sub` and the floor are for
        // a rate so low that 2 ms is shorter than the filter pair — 8 kHz is
        // the lowest this can be built at and no engine runs there, but a
        // kernel does not get to assume that, and a head at least four
        // samples deep is what `tap` needs to interpolate at all.
        let base = ((BASE_MS * 1.0e-3 * sr).round() as usize)
            .saturating_sub(OS_LATENCY)
            .max(4);
        Self {
            sample_rate: sr,
            line: [vec![0.0; cap], vec![0.0; cap]],
            at: 0,
            base,
            latency: base + OS_LATENCY,
            sub_lp: [0.0; 2],
            taps_a: windowed_sinc(8.0, OS_A_CUTOFF),
            taps_b: windowed_sinc(8.0, 0.25),
            os: [OsChannel::new(), OsChannel::new()],
            sat_err: [[0.0; OS]; 2],
            erase_env: [0.0; 2],
            erase_lp: [0.0; 2],
            bump: [Peak::default(); 2],
            roll_lp: [0.0; 2],
            pink: [Pink::new(sr, 0xA5A5_0001), Pink::new(sr, 0x5EED_C0DE)],
            hiss_env: 0.0,
            wow: 0.0,
            // Apart, so the two flutter rates never start in step.
            flutter: [0.0, 0.37],
            wander_rng: 0x1B0F_5EED,
            wow_scale: 0.5,
            wow_target: 0.5,
            wow_hold: 0,
            flutter_scale: 0.5,
            flutter_target: 0.5,
            flutter_hold: 0,
            scrape_lo: 0.0,
            scrape_hi: 0.0,
            a_sub: lp_a(f64::from(sr), f64::from(SUB_ROLL_HZ)),
            a_att: env_a(f64::from(sr), f64::from(ERASE_ATTACK_MS)),
            a_rel: env_a(f64::from(sr), f64::from(ERASE_RELEASE_MS)),
            a_hatt: env_a(f64::from(sr), f64::from(HISS_ATTACK_MS)),
            a_hrel: env_a(f64::from(sr), f64::from(HISS_RELEASE_MS)),
            a_scrape_lo: lp_a(f64::from(sr), f64::from(SCRAPE_BAND_HZ.0)),
            a_scrape_hi: lp_a(f64::from(sr), f64::from(SCRAPE_BAND_HZ.1)),
            a_wander: env_a(f64::from(sr), f64::from(WANDER_SLEW_S * 1_000.0)),
        }
    }

    /// The structural delay of the record stage and the play head together,
    /// in frames — what a host with delay compensation should be told. It is
    /// [`BASE_MS`] at every rate an engine actually runs at, oversampler and
    /// all; see that constant.
    pub fn latency_frames(&self) -> usize {
        self.latency
    }

    /// In place on an interleaved buffer; `channels` is the stride. Stereo
    /// only: anything past the first two channels is left alone, and a mono
    /// buffer is returned untouched rather than half-processed — the same
    /// contract as `Effects::process`.
    pub fn process(&mut self, buf: &mut [f32], frames: usize, channels: usize, p: &TapeParams) {
        if channels < 2 {
            return;
        }
        debug_assert!(buf.len() >= frames * channels);
        let sr = self.sample_rate;

        // ── per-block design math, f64, then down to f32 ────────────────
        let drive = norm(p.drive);
        let bump = norm(p.bump);
        let roll = norm(p.roll);
        let wobble = norm(p.wobble);
        let hiss = norm(p.hiss);
        let trim = norm(p.trim);

        let d64 = f64::from(DRIVE_TANH.0)
            * f64::from(DRIVE_TANH.1 / DRIVE_TANH.0).powf(f64::from(drive));
        let d = d64 as f32;
        // The repro amplifier, lined up for this record level. See
        // [`sat_makeup`]: solved once per block, in f64, like every other
        // design number here.
        let makeup = sat_makeup(d64);
        let bump_db = f64::from(BUMP_DB.0 + (BUMP_DB.1 - BUMP_DB.0) * bump);
        for b in &mut self.bump {
            b.set(f64::from(sr), BUMP_HZ, BUMP_Q, bump_db);
        }
        let roll_on = roll > 1.0e-3;
        let roll_a = if roll_on {
            let hz = f64::from(ROLL_HZ.0) * f64::from(ROLL_HZ.1 / ROLL_HZ.0).powf(f64::from(roll));
            lp_a(f64::from(sr), hz)
        } else {
            0.0
        };
        let trim_gain =
            10.0_f64.powf(f64::from((trim - 0.5) * (TRIM_DB.1 - TRIM_DB.0)) / 20.0) as f32;
        // White noise carries its power in bandwidth, so the same sample
        // variance is quieter per hertz at 96k; the square root keeps the
        // audible hiss level the same on every machine.
        let hiss_level = hiss * hiss * HISS_MAX * (DESIGN_RATE / sr).sqrt();
        let wow_amp = WOW_DEPTH_MS * 1.0e-3 * sr * wobble;
        let flutter_amp = (
            FLUTTER_DEPTH_MS.0 * 1.0e-3 * sr * wobble,
            FLUTTER_DEPTH_MS.1 * 1.0e-3 * sr * wobble,
        );
        let scrape_amp = SCRAPE_MS * 1.0e-3 * sr * wobble;
        let n = self.line[0].len();
        let tau = std::f32::consts::TAU;

        for i in 0..frames {
            let f = i * channels;

            // ── the transport, shared by both channels ──────────────────
            self.wow += WOW_HZ / sr;
            if self.wow >= 1.0 {
                self.wow -= 1.0;
            }
            for (ph, hz) in self.flutter.iter_mut().zip([FLUTTER_HZ.0, FLUTTER_HZ.1]) {
                *ph += hz / sr;
                if *ph >= 1.0 {
                    *ph -= 1.0;
                }
            }
            if self.wow_hold == 0 {
                self.wow_target = uniform(&mut self.wander_rng);
                self.wow_hold = (WOW_HOLD_S * sr) as u32;
            }
            self.wow_hold -= 1;
            self.wow_scale += (self.wow_target - self.wow_scale) * self.a_wander;
            if self.flutter_hold == 0 {
                self.flutter_target = uniform(&mut self.wander_rng);
                self.flutter_hold = (FLUTTER_HOLD_S * sr) as u32;
            }
            self.flutter_hold -= 1;
            self.flutter_scale += (self.flutter_target - self.flutter_scale) * self.a_wander;
            let wsc = DEPTH_WANDER.0 + (DEPTH_WANDER.1 - DEPTH_WANDER.0) * self.wow_scale;
            let fsc = DEPTH_WANDER.0 + (DEPTH_WANDER.1 - DEPTH_WANDER.0) * self.flutter_scale;
            let w = white(&mut self.wander_rng);
            self.scrape_lo = flush(self.scrape_lo + self.a_scrape_lo * (w - self.scrape_lo));
            self.scrape_hi = flush(self.scrape_hi + self.a_scrape_hi * (w - self.scrape_hi));
            let scrape = (self.scrape_hi - self.scrape_lo) * scrape_amp;
            let sway = (tau * self.wow).sin() * wow_amp * wsc
                + (tau * self.flutter[0]).sin() * flutter_amp.0 * fsc
                + (tau * self.flutter[1]).sin() * flutter_amp.1 * fsc
                + scrape;
            let delay = (self.base as f32 + sway).clamp(2.0, (n - 4) as f32);

            // The hiss rides the program, judged before this frame is
            // overwritten.
            let program = buf[f].abs().max(buf[f + 1].abs());
            let ah = if program > self.hiss_env {
                self.a_hatt
            } else {
                self.a_hrel
            };
            self.hiss_env = flush(self.hiss_env + (program - self.hiss_env) * ah);
            let hiss_gain =
                hiss_level * (HISS_FLOOR + (1.0 - HISS_FLOOR) * self.hiss_env.min(1.0));

            for ch in 0..2 {
                let x = buf[f + ch];

                // The sub roll: keep the rumble out of the headroom.
                self.sub_lp[ch] = flush(self.sub_lp[ch] + self.a_sub * (x - self.sub_lp[ch]));
                let u = x - self.sub_lp[ch];

                // The record level, dual-rate. This envelope — not any gain
                // element — is what rounds the attacks, spectrally, below.
                let lvl = (u * d).abs();
                let a = if lvl > self.erase_env[ch] {
                    self.a_att
                } else {
                    self.a_rel
                };
                self.erase_env[ch] = flush(self.erase_env[ch] + (lvl - self.erase_env[ch]) * a);

                // The record amplifier and the tape, at 4×. See [`OS`] for
                // why, [`DRIVE_TANH`] for the curve and [`SAT_FB`] for the
                // refusal fed back into it. Two 2× steps per session sample,
                // two 4× steps per 2× step; the ×2 on each zero-stuffed push
                // restores the amplitude an interpolator's inserted zeros
                // dilute.
                let st = &mut self.os[ch];
                let mut y = 0.0_f32;
                for h in 0..2 {
                    let s2 = st.up_a.run(&self.taps_a, if h == 0 { 2.0 * u } else { 0.0 });
                    for q in 0..2 {
                        let s4 = st.up_b.run(&self.taps_b, if q == 0 { 2.0 * s2 } else { 0.0 });

                        // Read the refusal from this sub-phase BEFORE writing
                        // it: what is there is what this sub-phase left one
                        // session sample ago, which is the z⁻¹ SAT_FB has
                        // always meant.
                        let phase = h * 2 + q;
                        let e = self.sat_err[ch][phase];
                        let w = (d * (s4 + SAT_FB * e)).tanh() / d;
                        self.sat_err[ch][phase] = flush(s4 - w);

                        // Decimate: filter at 4×, keep every second sample
                        // into the 2× filter, keep every second of those.
                        let d4 = st.dn_b.run(&self.taps_b, w);
                        if q == 1 {
                            let d2 = st.dn_a.run(&self.taps_a, d4);
                            if h == 1 {
                                y = d2;
                            }
                        }
                    }
                }
                // The repro amplifier's line-up, not a knob. See
                // [`sat_makeup`].
                let y = y * makeup;

                // HF self-erasure: the corner falls as the level rises.
                let corner = (ERASE_TOP_HZ
                    * (-(ERASE_OCTAVES * self.erase_env[ch].min(1.4))).exp2())
                .max(ERASE_FLOOR_HZ);
                let ae = 1.0 - (-tau * corner / sr).exp();
                self.erase_lp[ch] = flush(self.erase_lp[ch] + ae * (y - self.erase_lp[ch]));
                self.line[ch][self.at] = self.erase_lp[ch];

                // The play head, swaying.
                let mut v = tap(&self.line[ch], self.at, delay);

                // Repro EQ: the bump, then the roll.
                v = self.bump[ch].process(v);
                if roll_on {
                    self.roll_lp[ch] = flush(self.roll_lp[ch] + roll_a * (v - self.roll_lp[ch]));
                    v = self.roll_lp[ch];
                }

                // The hiss, after the EQ: it is the repro chain's noise, and
                // the tone knobs of the machine do not touch it much.
                v += self.pink[ch].step() * hiss_gain;

                buf[f + ch] = v * trim_gain;
            }

            self.at += 1;
            if self.at >= n {
                self.at = 0;
            }
        }
    }

    /// Forget everything: the tape, the envelopes, the noise. What was
    /// ringing when the unit left circuit must not be waiting when it comes
    /// back half an hour later.
    pub fn clear(&mut self) {
        for l in &mut self.line {
            l.fill(0.0);
        }
        self.at = 0;
        self.sub_lp = [0.0; 2];
        for c in &mut self.os {
            c.clear();
        }
        self.sat_err = [[0.0; OS]; 2];
        self.erase_env = [0.0; 2];
        self.erase_lp = [0.0; 2];
        for b in &mut self.bump {
            b.clear();
        }
        self.roll_lp = [0.0; 2];
        for p in &mut self.pink {
            p.clear();
        }
        self.hiss_env = 0.0;
        self.wow = 0.0;
        self.flutter = [0.0, 0.37];
        self.wander_rng = 0x1B0F_5EED;
        self.wow_scale = 0.5;
        self.wow_target = 0.5;
        self.wow_hold = 0;
        self.flutter_scale = 0.5;
        self.flutter_target = 0.5;
        self.flutter_hold = 0;
        self.scrape_lo = 0.0;
        self.scrape_hi = 0.0;
    }
}

// ── proof ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Hann-weighted Goertzel magnitude. **Hann-weighted, and that is the
    /// lesson written in blood elsewhere in this repo**: a rectangular
    /// Goertzel window leaks so badly across bins that it lies about band
    /// levels; the taper is not optional.
    fn band(buf: &[f32], sample_rate: f64, hz: f64) -> f64 {
        let n = buf.len();
        let w = std::f64::consts::TAU * hz / sample_rate;
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0_f64, 0.0_f64);
        for (i, &x) in buf.iter().enumerate() {
            let win =
                0.5 * (1.0 - (std::f64::consts::TAU * i as f64 / (n as f64 - 1.0)).cos());
            let s0 = f64::from(x) * win + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0).sqrt()
    }

    fn rms(buf: &[f32]) -> f64 {
        let sum: f64 = buf.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
        (sum / buf.len() as f64).sqrt()
    }

    /// Run interleaved stereo through the unit in renderer-sized blocks, so
    /// every test also exercises block boundaries.
    fn feed(t: &mut Tape, input: &[f32], p: &TapeParams) -> Vec<f32> {
        let mut out = input.to_vec();
        let total = out.len() / 2;
        let mut done = 0;
        while done < total {
            let n = (total - done).min(512);
            t.process(&mut out[done * 2..(done + n) * 2], n, 2, p);
            done += n;
        }
        out
    }

    fn left(stereo: &[f32]) -> Vec<f32> {
        stereo.chunks_exact(2).map(|f| f[0]).collect()
    }

    fn to_stereo(mono: &[f32]) -> Vec<f32> {
        mono.iter().flat_map(|v| [*v, *v]).collect()
    }

    fn db(ratio: f64) -> f64 {
        20.0 * ratio.log10()
    }

    /// Every colour switched off is a wire, to within a tenth of a decibel
    /// of broadband level. Not bit-exact — the play head is a real delay and
    /// the sub roll is always in circuit — but level-transparent, which is
    /// what "off" has to mean for an insert.
    #[test]
    fn neutral_settings_pass_the_audio_within_a_tenth_of_a_decibel() {
        let sr = 44_100.0_f32;
        let frames = (sr * 1.2) as usize;
        let mono: Vec<f32> = (0..frames)
            .map(|i| {
                let t = i as f32 / sr;
                0.07 * (std::f32::consts::TAU * 400.0 * t).sin()
                    + 0.07 * (std::f32::consts::TAU * 1_000.0 * t).sin()
                    + 0.07 * (std::f32::consts::TAU * 3_300.0 * t).sin()
            })
            .collect();
        let input = to_stereo(&mono);
        // Steady state only: skip the envelope settling and the line fill.
        let lo = ((sr * 0.25) as usize) * 2;
        let hi = ((sr * 1.15) as usize) * 2;
        let at = |drive: f32| {
            let p = TapeParams {
                drive,
                bump: 0.0,
                roll: 0.0,
                wobble: 0.0,
                hiss: 0.0,
                trim: 0.5,
            };
            let out = feed(&mut Tape::new(sr), &input, &p);
            db(rms(&out[lo..hi]) / rms(&input[lo..hi]))
        };

        // **Drive at the BOTTOM, not the middle.** It is a record level, and
        // a record level's off is the end of its travel — the same place
        // every other knob here parks. This used to read 0.5 and pass only
        // because the drive's top was too low to matter at 0.5; see
        // [`DRIVE_TANH`].
        let off = at(0.0);
        assert!(
            off.abs() < 0.1,
            "neutral settings moved the broadband level by {off:.3} dB"
        );

        // **And at the setting the unit actually ships at**, which is the
        // check that was missing. Moving this test's drive from the middle to
        // the bottom was correct and it was also where the level regression
        // got through: with the guard anchored at an end of the travel,
        // nothing on the bench was watching the position every new bay opens
        // at. [`sat_makeup`] is what holds it.
        let shipped = at(TapeParams::default().drive);
        assert!(
            shipped.abs() < 0.4,
            "the SHIPPED drive moved a programme at record level by \
             {shipped:.3} dB — the record level is doubling as a volume \
             control"
        );
    }

    /// **An attack loses treble before it loses level, which no gain element
    /// could do.** A gain takes the same decibels from every band; here the
    /// loud first milliseconds lose far more at 8 kHz than at 500 Hz, and
    /// the 8 kHz comes back as the level settles — the self-erasure, working.
    #[test]
    fn an_attack_loses_treble_before_it_loses_level_which_no_gain_could_do() {
        let sr = 48_000.0_f32;
        let pre = 960; // 20 ms of silence in front
        let total = pre + (sr * 0.4) as usize;
        let step = pre + (sr * 0.03) as usize; // loud for 30 ms, then quiet
        let mono: Vec<f32> = (0..total)
            .map(|i| {
                if i < pre {
                    return 0.0;
                }
                let t = i as f32 / sr;
                let amp = if i < step { 0.5 } else { 0.12 };
                amp * (std::f32::consts::TAU * 500.0 * t).sin()
                    + amp * (std::f32::consts::TAU * 8_000.0 * t).sin()
            })
            .collect();
        let input = to_stereo(&mono);
        // Half travel, on a −6 dBFS attack. The knob used to read 0.7 here
        // and it is the SAME record level: [`DRIVE_TANH`]'s top moved from 4
        // to 20, so the factor that used to sit at 0.7 now sits just under
        // half. The claim being measured is about the erasure, not about the
        // dial, so the operating point is what had to be preserved.
        let p = TapeParams {
            drive: 0.5,
            bump: 0.0,
            roll: 0.0,
            wobble: 0.0,
            hiss: 0.0,
            trim: 0.5,
        };
        let mut unit = Tape::new(sr);
        let lat = unit.latency_frames();
        let out = left(&feed(&mut unit, &input, &p));

        let win = 240; // the first 5 ms
        let sus = pre + (sr * 0.25) as usize; // well after the step down
        let ratio = |at: usize, hz: f64| {
            band(&out[at + lat..at + lat + win], sr.into(), hz)
                / band(&mono[at..at + win], sr.into(), hz)
        };
        let r8_attack = ratio(pre, 8_000.0);
        let r500_attack = ratio(pre, 500.0);
        // The window after the first: the corner has finished falling and
        // sits where the level put it.
        let r8_settled = ratio(pre + win, 8_000.0);
        let r500_settled = ratio(pre + win, 500.0);
        let r8_sustain = ratio(sus, 8_000.0);

        // The spectral claim, in the first 5 ms: 8 kHz already loses at
        // least 1.5 dB MORE than 500 Hz. A pure gain element gives every
        // band the same ratio, so this ratio-of-ratios is the thing a gain
        // cannot fake. The margin is modest here because the erasure cannot
        // precede the level — the follower needs its millisecond — which is
        // itself the physics.
        assert!(
            r8_attack < r500_attack * 10.0_f64.powf(-1.5 / 20.0),
            "the attack lost 8 kHz x{r8_attack:.3} vs 500 Hz x{r500_attack:.3} — \
             not spectral enough to be self-erasure"
        );
        // Once the corner has landed, the differential is unmistakable:
        // 3 dB more loss at 8 kHz than at 500 Hz while the level holds.
        assert!(
            r8_settled < r500_settled * 10.0_f64.powf(-3.0 / 20.0),
            "settled attack lost 8 kHz x{r8_settled:.3} vs 500 Hz x{r500_settled:.3} — \
             the corner never came down"
        );
        // The program-dependence claim: the same 8 kHz band is healthier in
        // the sustain, because the level fell and the corner recovered.
        assert!(
            r8_settled < r8_sustain * 10.0_f64.powf(-2.0 / 20.0),
            "8 kHz in the attack (x{r8_settled:.3}) is not rounder than in the \
             sustain (x{r8_sustain:.3})"
        );
        // And the 500 Hz band was merely compressed, not destroyed: what it
        // lost is level, bounded by the saturation.
        assert!(
            r500_attack > 10.0_f64.powf(-6.0 / 20.0),
            "500 Hz lost more than 6 dB in the attack (x{r500_attack:.3}) — \
             that is a gain change, not a spectral one"
        );
    }

    /// A steady 1 kHz through full wobble bends by a believable transport's
    /// amount: worn machines measure a few tenths of a percent, and anything
    /// past that is seasickness, not tape.
    #[test]
    fn full_wobble_bends_a_sine_by_a_believable_fraction_of_a_percent() {
        let sr = 48_000.0_f32;
        let frames = (sr * 4.0) as usize;
        let mono: Vec<f32> = (0..frames)
            .map(|i| 0.3 * (std::f32::consts::TAU * 1_000.0 * i as f32 / sr).sin())
            .collect();
        let input = to_stereo(&mono);
        let p = TapeParams {
            drive: 0.5,
            bump: 0.0,
            roll: 0.0,
            wobble: 1.0,
            hiss: 0.0,
            trim: 0.5,
        };
        let mut unit = Tape::new(sr);
        let out = left(&feed(&mut unit, &input, &p));

        // Rising zero crossings, sub-sample by linear interpolation — the
        // slope at a sine's crossing is steep enough that the interpolation
        // error is orders below what is being measured.
        let skip = (sr * 0.5) as usize;
        let mut crossings = Vec::new();
        for i in skip + 1..out.len() {
            if out[i - 1] < 0.0 && out[i] >= 0.0 {
                let frac = f64::from(out[i - 1]) / f64::from(out[i - 1] - out[i]);
                crossings.push((i - 1) as f64 + frac);
            }
        }
        let periods: Vec<f64> = crossings.windows(2).map(|w| w[1] - w[0]).collect();
        // Five-cycle smoothing knocks down the scrape jitter without dulling
        // the 11 Hz flutter, whose period is ninety of these.
        let smooth: Vec<f64> = periods
            .windows(5)
            .map(|w| w.iter().sum::<f64>() / 5.0)
            .collect();
        let mean = smooth.iter().sum::<f64>() / smooth.len() as f64;
        let (lo, hi) = smooth
            .iter()
            .fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
        let peak_to_peak = (hi - lo) / mean;
        assert!(
            (0.000_5..0.004).contains(&peak_to_peak),
            "peak-to-peak pitch deviation {:.4}% is outside the believable \
             0.05%..0.4% of a real transport",
            peak_to_peak * 100.0
        );
    }

    /// The hiss falls off like pink noise — between −2 and −4 dB per octave
    /// across the band that matters — and ducks back towards its floor once
    /// the music stops, instead of hanging at its playing level.
    #[test]
    fn the_hiss_slopes_down_like_pink_noise_and_ducks_when_the_music_stops() {
        let sr = 48_000.0_f32;
        let p = TapeParams {
            drive: 0.5,
            bump: 0.0,
            roll: 0.0,
            wobble: 0.0,
            hiss: 1.0,
            trim: 0.5,
        };

        // The slope, on silence: nothing in the output but the shaper.
        let mut unit = Tape::new(sr);
        let warm = 4_096;
        let windows = 16;
        let len = 8_192;
        let silence = vec![0.0_f32; (warm + windows * len) * 2];
        let out = left(&feed(&mut unit, &silence, &p));
        let freqs = [250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0];
        let mut powers = [0.0_f64; 6];
        for w in 0..windows {
            let seg = &out[warm + w * len..warm + (w + 1) * len];
            for (k, f) in freqs.iter().enumerate() {
                // Three adjacent bins per window, tripling the degrees of
                // freedom of a noise estimate that would otherwise wander.
                for off in [0.98, 1.0, 1.02] {
                    let m = band(seg, sr.into(), f * off);
                    powers[k] += m * m;
                }
            }
        }
        // Least-squares slope of dB against octaves.
        let xs: Vec<f64> = freqs.iter().map(|f| (f / 250.0).log2()).collect();
        let ys: Vec<f64> = powers.iter().map(|p| 10.0 * p.log10()).collect();
        let xm = xs.iter().sum::<f64>() / 6.0;
        let ym = ys.iter().sum::<f64>() / 6.0;
        let slope = xs
            .iter()
            .zip(&ys)
            .map(|(x, y)| (x - xm) * (y - ym))
            .sum::<f64>()
            / xs.iter().map(|x| (x - xm) * (x - xm)).sum::<f64>();
        assert!(
            (-4.0..-2.0).contains(&slope),
            "the hiss slopes at {slope:.2} dB/octave, which is not pink-ish"
        );

        // The ducking: a second of music, then silence. The hiss right after
        // the last note is louder than the hiss a second and a half later.
        let mut unit = Tape::new(sr);
        let note = (sr * 1.0) as usize;
        let total = (sr * 3.0) as usize;
        let mono: Vec<f32> = (0..total)
            .map(|i| {
                if i < note {
                    0.4 * (std::f32::consts::TAU * 1_000.0 * i as f32 / sr).sin()
                } else {
                    0.0
                }
            })
            .collect();
        let out = left(&feed(&mut unit, &to_stereo(&mono), &p));
        let win = |from: f32, to: f32| &out[(sr * from) as usize..(sr * to) as usize];
        let early = rms(win(1.05, 1.35));
        let late = rms(win(2.55, 2.85));
        assert!(
            early > late * 1.2,
            "the hiss did not duck after the music stopped: {early:.2e} then {late:.2e}"
        );
        assert!(late > 1.0e-7, "the hiss floor vanished entirely: {late:.2e}");
    }

    /// **Nothing the six knobs can reach may blow up.** Every combination of
    /// bottom, middle and top on all six at once, against a signal with a
    /// step in the middle of every block, on one unit whose state rides
    /// through the whole sweep — a knob drag, not a bench test. Then NaN
    /// params, then a clear that must actually clear.
    #[test]
    fn no_setting_the_six_knobs_can_reach_makes_it_explode() {
        let sr = 48_000.0_f32;
        let mut unit = Tape::new(sr);
        let block: Vec<f32> = (0..128)
            .flat_map(|i| {
                let t = i as f32 / sr;
                let amp = if i < 64 { 1.2 } else { 0.1 };
                let v = amp
                    * ((std::f32::consts::TAU * 60.0 * t).sin() * 0.6
                        + (std::f32::consts::TAU * 3_300.0 * t).sin() * 0.4);
                [v, -v]
            })
            .collect();
        let positions = [0.0_f32, 0.5, 1.0];
        for combo in 0..3_usize.pow(6) {
            let at = |k: u32| positions[combo / 3_usize.pow(k) % 3];
            let p = TapeParams {
                drive: at(0),
                bump: at(1),
                roll: at(2),
                wobble: at(3),
                hiss: at(4),
                trim: at(5),
            };
            let mut buf = block.clone();
            unit.process(&mut buf, 128, 2, &p);
            for v in &buf {
                assert!(
                    v.is_finite() && v.abs() < 16.0,
                    "combo {combo} produced {v} — trim tops at +12 dB and the \
                     bump at +5, so nothing honest gets near this"
                );
            }
        }

        // A settings file can hand a kernel NaN; the kernel must shrug.
        let p = TapeParams {
            drive: f32::NAN,
            bump: f32::INFINITY,
            roll: -3.0,
            wobble: f32::NAN,
            hiss: 2.0,
            trim: f32::NAN,
        };
        let mut buf = block.clone();
        unit.process(&mut buf, 128, 2, &p);
        assert!(
            buf.iter().all(|v| v.is_finite()),
            "NaN params reached the audio"
        );

        // And the tail dies when told to.
        unit.clear();
        let quiet = TapeParams {
            hiss: 0.0,
            wobble: 0.0,
            ..TapeParams::default()
        };
        let mut buf = vec![0.0_f32; 256 * 2];
        unit.process(&mut buf, 256, 2, &quiet);
        assert!(
            buf.iter().all(|v| v.abs() < 1.0e-9),
            "clear() left something on the tape"
        );

        // The stereo-only contract: a mono buffer is left alone, not
        // half-processed.
        let mut mono: Vec<f32> = (0..256).map(|i| (i as f32 * 0.02).sin()).collect();
        let before = mono.clone();
        unit.process(&mut mono, 256, 1, &TapeParams::default());
        assert_eq!(mono, before, "a mono buffer was written to");
    }

    /// A 96 kHz machine is the same machine: the head bump lifts the same
    /// hertz by the same decibels and the roll costs the same at the same
    /// probe, at 44.1, 48 and 96.
    #[test]
    fn the_bump_and_the_roll_sit_at_the_same_hertz_at_every_rate() {
        let rates = [44_100.0_f32, 48_000.0, 96_000.0];

        let probe = |sr: f32, hz: f32, amp: f32, p: &TapeParams| {
            let frames = (sr * 1.0) as usize;
            let mono: Vec<f32> = (0..frames)
                .map(|i| amp * (std::f32::consts::TAU * hz * i as f32 / sr).sin())
                .collect();
            let input = to_stereo(&mono);
            let mut unit = Tape::new(sr);
            let out = left(&feed(&mut unit, &input, p));
            let lo = (sr * 0.25) as usize;
            let hi = (sr * 0.95) as usize;
            db(band(&out[lo..hi], sr.into(), hz.into())
                / band(&mono[lo..hi], sr.into(), hz.into()))
        };

        // The bump, wide open: +5 dB at 70 Hz less the sub roll's ~1 dB.
        let bump = TapeParams {
            drive: 0.5,
            bump: 1.0,
            roll: 0.0,
            wobble: 0.0,
            hiss: 0.0,
            trim: 0.5,
        };
        let bumps: Vec<f64> = rates.iter().map(|sr| probe(*sr, 70.0, 0.2, &bump)).collect();
        for (sr, g) in rates.iter().zip(&bumps) {
            assert!(
                (3.0..4.8).contains(g),
                "the bump reads {g:.2} dB at {sr} Hz — wrong size"
            );
        }
        let spread = bumps.iter().fold(f64::MIN, |m, v| m.max(*v))
            - bumps.iter().fold(f64::MAX, |m, v| m.min(*v));
        assert!(
            spread < 0.4,
            "the bump moved by {spread:.2} dB across sample rates"
        );

        // The roll at half travel puts its corner near 7 kHz; a 5 kHz probe
        // should cost the same couple of dB everywhere. Drive at the bottom
        // keeps the self-erasure parked so only the roll is being measured.
        let roll = TapeParams {
            drive: 0.0,
            bump: 0.0,
            roll: 0.5,
            wobble: 0.0,
            hiss: 0.0,
            trim: 0.5,
        };
        let rolls: Vec<f64> = rates
            .iter()
            .map(|sr| probe(*sr, 5_000.0, 0.1, &roll))
            .collect();
        for (sr, g) in rates.iter().zip(&rolls) {
            assert!(
                (-2.8..-1.0).contains(g),
                "the roll reads {g:.2} dB at 5 kHz at {sr} Hz — wrong corner"
            );
        }
        let spread = rolls.iter().fold(f64::MIN, |m, v| m.max(*v))
            - rolls.iter().fold(f64::MAX, |m, v| m.min(*v));
        assert!(
            spread < 0.4,
            "the roll moved by {spread:.2} dB across sample rates"
        );
    }

    /// Total harmonic distortion of a 220 Hz tone, as a fraction of the
    /// fundamental. Harmonics 2 through 7; anything above that is under the
    /// measurement's own noise.
    fn thd(out: &[f32], sr: f32) -> f64 {
        let f1 = band(out, sr.into(), 220.0);
        let rest: f64 = (2..=7)
            .map(|h| band(out, sr.into(), 220.0 * f64::from(h)).powi(2))
            .sum();
        rest.sqrt() / f1.max(1.0e-12)
    }

    /// **The knob that measured live and sounded dead.**
    ///
    /// Every test above drives this unit at −6 dBFS or hotter, and at those
    /// levels the old drive knob worked. A DAW track is not at −6 dBFS: the
    /// alignment every meter in this app is built around puts 0 VU at −18, so
    /// the level to prove a saturator at is the one a person actually records.
    /// Swept end to end against a −18 dBFS tone the old knob ran from 0.001 %
    /// distortion to about 1.5 %, three quarters of that in the last eighth of
    /// the travel, and barely moved the self-erasure corner at all —
    /// measurably wired, audibly nothing, which is exactly what the owner
    /// reported and what no existing test could see.
    ///
    /// So the claim is pinned where it can be heard: at a level people
    /// record at, the bottom of the dial is a wire, the middle is a machine
    /// running clean, and the top is saturating AND dulling — the two halves
    /// of tape, both of them, at once.
    #[test]
    fn the_drive_knob_saturates_at_the_level_people_actually_record_at() {
        let sr = 48_000.0_f32;
        let frames = (sr * 1.0) as usize;
        // −18 dBFS peak. Quiet, deliberately: if the knob works here it
        // works on everything louder.
        const NOMINAL: f32 = 0.126;
        let clean = TapeParams {
            bump: 0.0,
            roll: 0.0,
            wobble: 0.0,
            hiss: 0.0,
            trim: 0.5,
            ..TapeParams::default()
        };
        let measure = |drive: f32| -> f64 {
            let mono: Vec<f32> = (0..frames)
                .map(|i| NOMINAL * (std::f32::consts::TAU * 220.0 * i as f32 / sr).sin())
                .collect();
            let out = left(&feed(
                &mut Tape::new(sr),
                &to_stereo(&mono),
                &TapeParams { drive, ..clean },
            ));
            let (lo, hi) = ((sr * 0.3) as usize, (sr * 0.95) as usize);
            thd(&out[lo..hi], sr)
        };

        let off = measure(0.0);
        let noon = measure(0.5);
        let slammed = measure(1.0);
        assert!(
            off < 1.0e-4,
            "the bottom of the drive dial is not a wire: {:.4} % THD",
            off * 100.0
        );
        assert!(
            (5.0e-4..2.0e-2).contains(&noon),
            "half travel reads {:.3} % THD — a clean machine is a few tenths \
             of a percent, not silence and not fuzz",
            noon * 100.0
        );
        assert!(
            slammed > 0.05,
            "full drive on a −18 dBFS track reads only {:.2} % THD — the knob \
             does not reach the tape",
            slammed * 100.0
        );
        assert!(
            noon > off * 10.0 && slammed > noon * 10.0,
            "the dial does not climb: {off:.6}, {noon:.6}, {slammed:.6}"
        );

        // And the other half of tape, at the same level: the top of the band
        // goes before the bottom does. A gain element cannot do this, which
        // is the whole argument of the module header.
        let two_tone: Vec<f32> = (0..frames)
            .map(|i| {
                let t = i as f32 / sr;
                NOMINAL * 0.5 * (std::f32::consts::TAU * 200.0 * t).sin()
                    + NOMINAL * 0.5 * (std::f32::consts::TAU * 8_000.0 * t).sin()
            })
            .collect();
        let tilt = |drive: f32| -> f64 {
            let out = left(&feed(
                &mut Tape::new(sr),
                &to_stereo(&two_tone),
                &TapeParams { drive, ..clean },
            ));
            let (lo, hi) = ((sr * 0.3) as usize, (sr * 0.95) as usize);
            let at = |hz: f64| {
                db(band(&out[lo..hi], sr.into(), hz) / band(&two_tone[lo..hi], sr.into(), hz))
            };
            at(8_000.0) - at(200.0)
        };
        assert!(
            tilt(0.0).abs() < 0.5,
            "the bottom of the dial already tilts the band by {:.2} dB",
            tilt(0.0)
        );
        assert!(
            tilt(1.0) < -3.0,
            "full drive on a −18 dBFS track tilts 8 kHz against 200 Hz by \
             only {:.2} dB — the self-erasure never leaves the ultrasonics",
            tilt(1.0)
        );
    }

    /// **The oversampler earns its keep, and this is the test that says so.**
    ///
    /// A memoryless tanh evaluated once per sample folds every harmonic it
    /// makes above Nyquist back into the band, at whatever frequency the fold
    /// lands on — which for a 7 kHz tone is 1, 2, 5, 6, 8 kHz and a dozen
    /// more, none of them a multiple of anything the listener played. That
    /// hash cannot be filtered out afterwards and it is the one distortion
    /// artefact that has no analogue counterpart: real tape has no Nyquist.
    ///
    /// The measurement uses the arithmetic gift of the probe frequency. 7 kHz
    /// against a 48 kHz rate has a greatest common divisor of 1 kHz, so every
    /// harmonic and every fold of every harmonic lands exactly on a multiple
    /// of 1 kHz — and of the twenty-three such multiples in the band, exactly
    /// three (7, 14, 21) are honest harmonics. The other twenty can only hold
    /// aliasing. No window function, no bin hunting: measure the twenty and
    /// compare them to the tone.
    ///
    /// Un-oversampled this reads −9.7 dB at full drive — a third of the
    /// output. The stated bar is 30 dB down, which the 4× stage clears with
    /// room to spare (it measures −35.9), and which reverting the
    /// oversampler fails by twenty.
    #[test]
    fn full_drive_on_a_high_note_does_not_fold_the_band_back_on_itself() {
        let sr = 48_000.0_f32;
        let frames = (sr * 0.7) as usize;
        // Everything that is not the saturator, off: the wobble spreads a
        // tone into sidebands and the hiss fills every bin, and both would be
        // read as aliasing by a measurement that cannot tell them apart.
        let clean = TapeParams {
            drive: 1.0,
            bump: 0.0,
            roll: 0.0,
            wobble: 0.0,
            hiss: 0.0,
            trim: 0.5,
        };
        // −6 dBFS: loud, because the fold gets worse the harder the curve is
        // driven and the claim is about the worst case a person can reach.
        let mono: Vec<f32> = (0..frames)
            .map(|i| 0.5 * (std::f32::consts::TAU * 7_000.0 * i as f32 / sr).sin())
            .collect();

        let residue = |drive: f32| -> f64 {
            let out = left(&feed(
                &mut Tape::new(sr),
                &to_stereo(&mono),
                &TapeParams { drive, ..clean },
            ));
            let seg = &out[(sr * 0.25) as usize..(sr * 0.65) as usize];
            let fund = band(seg, sr.into(), 7_000.0);
            let mut alias = 0.0_f64;
            for k in 1..24 {
                let hz = 1_000.0 * f64::from(k);
                // 7, 14 and 21 kHz are the tone's own harmonics; everything
                // else on the grid is a fold.
                if k % 7 == 0 {
                    continue;
                }
                let m = band(seg, sr.into(), hz);
                alias += m * m;
            }
            db(alias.sqrt() / fund.max(1.0e-12))
        };

        let slammed = residue(1.0);
        assert!(
            slammed < -30.0,
            "full drive on a 7 kHz tone folded {slammed:.1} dB of inharmonic \
             hash back into the band — the record stage is aliasing"
        );
        // And at the position it ships at, where the curve is barely bent,
        // the fold has to be nowhere at all.
        let shipped = residue(TapeParams::default().drive);
        assert!(
            shipped < -55.0,
            "the shipped drive folded {shipped:.1} dB of hash into the band"
        );
    }

    /// **The record level is not a volume control at the level the machine is
    /// lined up for, and this pins where it stops being free.**
    ///
    /// The dial went from a top of 4 to a top of 20 and took the level with
    /// it: measured broadband at −18 dBFS, full travel gave away 11.8 dB, and
    /// both of this module's neutrality guards had been re-anchored to the
    /// BOTTOM of the travel, so nothing was watching the position every new
    /// bay opens at. [`sat_makeup`] lines the repro amplifier up per record
    /// level, the way a machine is lined up, and this is its proof.
    ///
    /// The program is the line-up level itself — three tones summing to
    /// −18 dBFS RMS, low enough in the band that the self-erasure is not the
    /// thing being measured. What the top of the dial still costs is stated
    /// rather than trimmed away, because it is the erasure taking the top off
    /// the band, and a makeup that chased THAT would be undoing the effect.
    #[test]
    fn the_record_level_holds_its_level_where_the_machine_is_lined_up() {
        let sr = 48_000.0_f32;
        let frames = (sr * 1.2) as usize;
        // Three tones at −18 dBFS RMS together: 0 VU, the level ALIGN_DBFS
        // names and sat_makeup solves for.
        let amp = 10.0_f32.powf(-18.0 / 20.0) * (2.0_f32 / 3.0).sqrt();
        let mono: Vec<f32> = (0..frames)
            .map(|i| {
                let t = i as f32 / sr;
                amp * (std::f32::consts::TAU * 400.0 * t).sin()
                    + amp * (std::f32::consts::TAU * 1_000.0 * t).sin()
                    + amp * (std::f32::consts::TAU * 3_300.0 * t).sin()
            })
            .collect();
        let input = to_stereo(&mono);
        let (lo, hi) = ((sr * 0.3) as usize * 2, (sr * 1.15) as usize * 2);
        let at = |drive: f32| {
            let p = TapeParams {
                drive,
                bump: 0.0,
                roll: 0.0,
                wobble: 0.0,
                hiss: 0.0,
                trim: 0.5,
            };
            let out = feed(&mut Tape::new(sr), &input, &p);
            db(rms(&out[lo..hi]) / rms(&input[lo..hi]))
        };

        // The bottom is a wire and the shipped default is within a quarter of
        // a decibel of it. Between them the knob is character only.
        for drive in [0.0_f32, 0.2, 0.4, TapeParams::default().drive] {
            let delta = at(drive);
            assert!(
                delta.abs() < 0.4,
                "drive {drive} moved a 0 VU program by {delta:.3} dB — up to \
                 the shipped setting this dial may not touch the level"
            );
        }
        // Past the default it costs, and the cost is bounded and stated: the
        // whole travel is under two decibels on this program, where before
        // the line-up it was over ten.
        let slammed = at(1.0);
        assert!(
            (-2.5..0.0).contains(&slammed),
            "full drive moved a 0 VU program by {slammed:.2} dB — outside the \
             one to two decibels of top-end loss the erasure is allowed"
        );
        // Monotone downward: no setting of a record level may make a track
        // LOUDER than the one below it, which a mis-signed trim would.
        let mut last = f64::INFINITY;
        for step in 0..=10 {
            let here = at(step as f32 / 10.0);
            assert!(
                here <= last + 0.05,
                "drive {} read {here:.3} dB against {last:.3} dB the step \
                 before — the line-up is running the wrong way",
                step as f32 / 10.0
            );
            last = here;
        }
    }

    /// **The oversampler is invisible to the host.** The record stage's
    /// filter pair delays the signal by [`OS_LATENCY`] samples, and the play
    /// head gives up exactly that many, so what a host is told to compensate
    /// is still [`BASE_MS`] — at every rate an engine runs at. A unit that
    /// quietly grew three quarters of a millisecond would slide against every
    /// other track in the session and nothing would say why.
    #[test]
    fn the_oversampler_costs_the_host_no_latency() {
        for sr in [44_100.0_f32, 48_000.0, 96_000.0] {
            let want = (BASE_MS * 1.0e-3 * sr).round() as usize;
            let got = Tape::new(sr).latency_frames();
            assert_eq!(
                got, want,
                "at {sr} Hz the tape reports {got} frames of latency, not the \
                 {want} that BASE_MS promises"
            );
        }
    }

    /// **No knob on this unit may be a decoration.**
    ///
    /// The generic net under the five specific tests above: sweep each
    /// control from the shipped setting to each end of its travel and demand
    /// that the OUTPUT moves — not that the parameter was stored, which is
    /// what a field assignment proves and what a dead control also passes.
    ///
    /// The floors are measured, one per knob, a few decibels under what each
    /// actually delivers, because "moves at all" is too weak a bar: a knob
    /// that lost nine tenths of its authority to a mapping change would sail
    /// through a shared threshold. They are difference energy against the
    /// program, so a knob that only re-times the signal (wobble) scores as
    /// loudly as one that changes its level.
    #[test]
    fn every_knob_changes_the_sound_and_by_how_much_is_pinned() {
        let sr = 48_000.0_f32;
        let frames = (sr * 1.5) as usize;
        let mut seed = 0x2B1D_F00D_u32;
        // Broadband at −12 dBFS: something for every knob to act on.
        let mono: Vec<f32> = (0..frames).map(|_| 0.25 * white(&mut seed)).collect();
        let input = to_stereo(&mono);

        // symbol, what it does, and the floor in dB relative to the program.
        let knobs: [(&str, fn(&mut TapeParams, f32), f64); 6] = [
            ("drive", |p, v| p.drive = v, -6.0),
            // A 70 Hz peak is a thin slice of broadband noise; the bump's
            // own size is pinned in hertz by the sample-rate test.
            ("bump", |p, v| p.bump = v, -30.0),
            ("roll", |p, v| p.roll = v, -8.0),
            ("wobble", |p, v| p.wobble = v, -3.0),
            // Hiss is meant to sit under the music. Its shape and its
            // ducking are pinned by their own test; this only says it is
            // there.
            ("hiss", |p, v| p.hiss = v, -42.0),
            ("trim", |p, v| p.trim = v, 0.0),
        ];

        let shipped = feed(&mut Tape::new(sr), &input, &TapeParams::default());
        let (lo, hi) = ((sr * 0.4) as usize * 2, (sr * 1.4) as usize * 2);
        for (name, set, floor) in knobs {
            let mut best = f64::MIN;
            for end in [0.0_f32, 1.0] {
                let mut p = TapeParams::default();
                set(&mut p, end);
                let moved = feed(&mut Tape::new(sr), &input, &p);
                let diff: f64 = shipped[lo..hi]
                    .iter()
                    .zip(&moved[lo..hi])
                    .map(|(a, b)| f64::from(*a - *b).powi(2))
                    .sum();
                let program: f64 = shipped[lo..hi].iter().map(|a| f64::from(*a).powi(2)).sum();
                best = best.max(db((diff / program).sqrt()));
            }
            assert!(
                best > floor,
                "{name} moved the output by only {best:.2} dB from the shipped \
                 setting — it is wired to nothing a person could hear"
            );
        }
    }
}
