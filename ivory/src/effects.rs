//! Reverb and delay, on everything the app plays.
//!
//! Two knobs on the recorder, modelled on the Tascam 388's effect returns, and
//! one number each: how much. Everything else is chosen here, because the
//! person turning them is playing the piano with their other hand.
//!
//! # Where this sits
//!
//! On the instrument sum, after the slots and the built-in and before the
//! metronome and the input monitor. So:
//!
//! - a VST3 and the built-in FM get the same treatment, which is the point;
//! - the click stays dry, because a reverb tail on a count-in is a worse
//!   count-in and nobody has ever wanted one;
//! - what is heard is what is recorded, because the take is tapped downstream.
//!
//! # Why these algorithms
//!
//! The reverb is Schroeder-Moorer (the "Freeverb" arrangement): eight comb
//! filters in parallel into four allpasses in series, per channel, with the
//! right channel's delays offset so the two decorrelate into something wide.
//! It is thirty years old, it is public domain, it is about a hundred lines,
//! and it costs a few hundred multiply-adds a sample. A convolution reverb
//! would sound better and would mean shipping impulse responses, a partitioned
//! FFT, and a latency budget, for a control that is off by default.
//!
//! The delay is a delay line with feedback and a lowpass in the loop, and its
//! **time comes from the session tempo**. That is the one real opinion in this
//! file: a musician with one knob wants repeats that land on the beat, and
//! Tangent is the rare effect that already knows the tempo because there is a
//! metronome next to it.
//!
//! # Denormals
//!
//! Every feedback path here decays towards zero and never reaches it, and a
//! float that small costs a hundred times a normal one on some hardware — an
//! audio thread that was fine can start dropping buffers a minute after the
//! last note. [`flush`] is the guard, applied where each loop closes.

// ── what a person can change ────────────────────────────────────────────────

/// Where a delay's repeats land, in beats.
///
/// **Named divisions, not a time in milliseconds.** The knob is next to a
/// metronome, and a delay that drifts off the beat every time the tempo
/// changes is a delay somebody has to keep re-setting. Free time is the one
/// entry that does not follow the tempo, for anybody who wants a slapback that
/// stays put.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Division {
    Quarter,
    #[default]
    DottedEighth,
    Eighth,
    TripletEighth,
    Sixteenth,
    /// A fixed 375ms, whatever the tempo is doing.
    Free,
}

impl Division {
    /// Every value, in the order a menu should offer them: longest first.
    pub const ALL: [Division; 6] = [
        Division::Quarter,
        Division::DottedEighth,
        Division::Eighth,
        Division::TripletEighth,
        Division::Sixteenth,
        Division::Free,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Division::Quarter => "1/4",
            Division::DottedEighth => "1/8 dotted",
            Division::Eighth => "1/8",
            Division::TripletEighth => "1/8 triplet",
            Division::Sixteenth => "1/16",
            Division::Free => "free",
        }
    }

    /// How it is written to the settings file, and read back.
    pub fn key(self) -> &'static str {
        match self {
            Division::Quarter => "quarter",
            Division::DottedEighth => "dotted-eighth",
            Division::Eighth => "eighth",
            Division::TripletEighth => "triplet-eighth",
            Division::Sixteenth => "sixteenth",
            Division::Free => "free",
        }
    }

    pub fn from_key(key: &str) -> Option<Division> {
        Division::ALL.into_iter().find(|d| d.key() == key)
    }

    /// Beats per repeat. `Free` has none: see [`FREE_SECS`].
    fn beats(self) -> Option<f32> {
        Some(match self {
            Division::Quarter => 1.0,
            Division::DottedEighth => 0.75,
            Division::Eighth => 0.5,
            Division::TripletEighth => 1.0 / 3.0,
            Division::Sixteenth => 0.25,
            Division::Free => return None,
        })
    }
}

/// How steeply a filter gets rid of what is past its corner.
///
/// **Three real slopes, not a continuous "resonance-ish" control.** 24 dB is
/// the default because it is the one that does what somebody pointing at a
/// filter means — "take that off" — and 6 dB is there because sometimes the
/// answer is a tilt rather than a cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Slope {
    /// One pole. A tilt.
    Six,
    /// One biquad, Butterworth.
    Twelve,
    /// Two cascaded biquads, Butterworth-aligned.
    #[default]
    TwentyFour,
}

impl Slope {
    /// Every value, gentlest first, which is the order a menu should offer.
    pub const ALL: [Slope; 3] = [Slope::Six, Slope::Twelve, Slope::TwentyFour];

    pub fn label(self) -> &'static str {
        match self {
            Slope::Six => "6 dB/oct",
            Slope::Twelve => "12 dB/oct",
            Slope::TwentyFour => "24 dB/oct",
        }
    }

    /// How it is written to the settings file, and read back.
    pub fn key(self) -> &'static str {
        match self {
            Slope::Six => "6",
            Slope::Twelve => "12",
            Slope::TwentyFour => "24",
        }
    }

    pub fn from_key(key: &str) -> Option<Slope> {
        Slope::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// How many all-pass stages the phaser runs.
///
/// **These are the pedals.** Two is the MXR Phase 45, four is the Phase 90 —
/// which is what this is voiced as and what it ships at — and six and eight
/// are the deeper members of the family. Each PAIR of stages puts one notch
/// in the spectrum, so the number is not a quality setting: it is how many
/// notches sweep past.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stages {
    /// One notch. Gentle — the Phase 45.
    Two,
    /// Two notches. **The Phase 90**, and the sound anybody picturing a
    /// phaser is picturing.
    #[default]
    Four,
    /// Three notches, thicker.
    Six,
    /// Four. Past what any of the pedals did, and there when somebody wants
    /// the sweep to be the whole part.
    Eight,
}

impl Stages {
    pub const ALL: [Stages; 4] = [Stages::Two, Stages::Four, Stages::Six, Stages::Eight];

    /// The largest, which is what the state array is sized for.
    pub const MAX: usize = 8;

    pub fn count(self) -> usize {
        match self {
            Stages::Two => 2,
            Stages::Four => 4,
            Stages::Six => 6,
            Stages::Eight => 8,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Stages::Two => "2 - Phase 45",
            Stages::Four => "4 - Phase 90",
            Stages::Six => "6",
            Stages::Eight => "8",
        }
    }

    /// How it is written to the settings file, and read back.
    pub fn key(self) -> &'static str {
        match self {
            Stages::Two => "2",
            Stages::Four => "4",
            Stages::Six => "6",
            Stages::Eight => "8",
        }
    }

    pub fn from_key(key: &str) -> Option<Stages> {
        Stages::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// Seconds per repeat with [`Division::Free`], and the fallback when there is
/// no sensible tempo to sync to.
const FREE_SECS: f32 = 0.375;

/// Everything about the three effects a person can change.
///
/// **One struct, sent whole.** The alternative is an atomic per field, and
/// there are eleven of them — eleven names to keep in step across the settings
/// file, the menu, the shared state and the renderer. These change when
/// somebody opens a menu, which is never, in audio terms; so they cross to the
/// audio thread as one value behind a lock the renderer only reaches for when
/// a flag says something moved.
///
/// Every field is 0..=1 unless it says otherwise, because that is what a
/// control is, and the mapping to whatever the DSP actually wants lives next to
/// the DSP.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// How long the room rings. Maps to the comb feedback.
    pub reverb_size: f32,
    /// How fast the top comes off the tail.
    pub reverb_damp: f32,
    /// How far apart the two channels' rooms are. 0 is mono.
    pub reverb_width: f32,
    /// How much of the plate's dispersion is in circuit.
    ///
    /// **The chirp, as a control.** 1.0 is the sheet of steel; 0 is a tank
    /// with no plate physics in it at all, which is a perfectly good small
    /// reverb and is not what this is called. See [`Dispersion`].
    pub plate_dispersion: f32,
    /// Silence in front of the plate, across [`PLATE_PREDELAY_MS`].
    ///
    /// The plate itself has none — see the constant. This is the tape machine
    /// every studio patched in front of theirs.
    pub plate_predelay: f32,

    pub delay_division: Division,
    /// Which of the twelve Mode Selector positions the machine is on: which
    /// playback heads are live, and whether the spring is in circuit. See
    /// [`EchoMode`].
    pub echo_mode: EchoMode,
    /// How much of each repeat comes back for the next.
    pub delay_feedback: f32,
    /// How much darker each repeat is than the one before it.
    pub delay_tone: f32,
    /// How far the right channel's time is from the left's. 0 is mono.
    pub delay_width: f32,

    /// Sweep speed, mapped across [`CHORUS_RATE_HZ`].
    pub chorus_rate: f32,
    /// How far the sweep moves the delay.
    pub chorus_depth: f32,
    /// How much of the wet is inverted into the right channel — the CE-1's
    /// own trick. See [`Chorus`].
    pub chorus_width: f32,
    /// The bucket brigade's bandwidth. Down is darker and more like the
    /// hardware; up is a cleaner chorus than ever existed in 1976.
    pub chorus_tone: f32,

    /// Sweep speed, mapped across [`PHASER_RATE_HZ`].
    pub phaser_rate: f32,
    /// How much of the last stage comes back to the first.
    ///
    /// **This is the difference between the two Phase 90s.** The block-logo
    /// pedal has the feedback resistor and the sharper, more vocal sweep; the
    /// script logo leaves it out and is softer. Ships in between, nearer the
    /// block.
    pub phaser_feedback: f32,
    /// How many all-pass stages. See [`Stages`].
    pub phaser_stages: Stages,
    /// How far up the spectrum the notches travel, across
    /// [`PHASER_SWEEP_HZ`]. Down is the narrow, mid-focused sweep of the
    /// pedal; up opens it out over most of the band.
    pub phaser_range: f32,

    pub hpf_slope: Slope,
    /// A lift right at the corner. 0 is flat Butterworth.
    pub hpf_resonance: f32,

    pub lpf_slope: Slope,
    /// A lift right at the corner. 0 is flat Butterworth.
    pub lpf_resonance: f32,

    /// How fast it lets go, mapped across [`RELEASE_MS`].
    pub limiter_release: f32,
    /// How far below the ceiling it starts easing in, across [`KNEE_DB`].
    ///
    /// The one control that trades the two failure modes against each other:
    /// hard is transparent until it is not, soft starts working earlier and
    /// distorts less when it does.
    pub limiter_knee: f32,
}

impl Default for Params {
    /// **These are the sound**, and they are what every knob was voiced
    /// against before there was a menu to change them. A reset goes here.
    fn default() -> Self {
        Self {
            // **2.2 seconds, bright, wide, and 18 ms in front of it.** A
            // plate voiced where a plate is used: long enough to be a room,
            // short enough to sit under a piano, and with the predelay every
            // engineer patched in already patched in.
            reverb_size: 0.34,
            reverb_damp: 0.55,
            reverb_width: 0.80,
            plate_dispersion: 0.75,
            plate_predelay: 0.12,
            delay_division: Division::DottedEighth,
            // **Head one alone, no spring.** Eleven of the twelve positions
            // are a choice about the sound; this is the one where "the
            // repeat" means exactly one thing, which is what a control next
            // to a metronome has to mean before anybody goes looking.
            echo_mode: EchoMode::One,
            delay_feedback: 0.42,
            delay_tone: 0.55,
            delay_width: 0.60,
            chorus_rate: 0.28,
            chorus_depth: 0.55,
            chorus_width: 0.85,
            chorus_tone: 0.45,
            // **The block-logo Phase 90, near enough.** A slow sweep, most
            // of the feedback the pedal has, four stages, and the narrow
            // mid-focused range the JFETs actually cover — a phaser voiced
            // across the whole spectrum sounds like a flanger with the
            // delay missing.
            phaser_rate: 0.22,
            phaser_feedback: 0.55,
            phaser_stages: Stages::default(),
            phaser_range: 0.35,
            hpf_slope: Slope::default(),
            hpf_resonance: 0.0,
            lpf_slope: Slope::default(),
            lpf_resonance: 0.0,
            // **Dialled in for general use.** A release that does not pump on
            // piano, and enough knee that the first decibel of reduction is
            // not the one you hear arriving. Where it starts working is the
            // knob, not a parameter.
            limiter_release: 0.40,
            limiter_knee: 0.50,
        }
    }
}

impl Params {
    /// Clamp everything into range. Called on the way in from the settings
    /// file, which a person can edit and a later build can have written.
    pub fn sane(mut self) -> Self {
        for v in [
            &mut self.reverb_size,
            &mut self.reverb_damp,
            &mut self.reverb_width,
            &mut self.plate_dispersion,
            &mut self.plate_predelay,
            &mut self.delay_feedback,
            &mut self.delay_tone,
            &mut self.delay_width,
            &mut self.chorus_rate,
            &mut self.chorus_depth,
            &mut self.chorus_width,
            &mut self.chorus_tone,
            &mut self.phaser_rate,
            &mut self.phaser_feedback,
            &mut self.phaser_range,
            &mut self.hpf_resonance,
            &mut self.lpf_resonance,
            &mut self.limiter_release,
            &mut self.limiter_knee,
        ] {
            *v = if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.5
            };
        }
        self
    }
}

/// Below this, a sample is on its way to a denormal and is worth nothing.
///
/// 1e-20 in amplitude is 400 dB down. Nothing musical lives here.
const TINY: f32 = 1.0e-20;

/// Zero anything too small to hear, before it becomes expensive to add.
#[inline]
fn flush(x: f32) -> f32 {
    if x.abs() < TINY {
        0.0
    } else {
        x
    }
}

// ── the plate ───────────────────────────────────────────────────────────────
//
// A steel plate, after the EMT 140: a 2 m x 1 m sheet, half a millimetre
// thick, hung under tension and driven by a coil at one point while an
// accelerometer listens at another.
//
// # What makes a plate a plate
//
// Not its size. Three consequences of one fact — that bending waves in a thin
// plate are DISPERSIVE, so high frequencies travel faster than low ones:
//
// 1. **The chirp.** Group velocity on the real plate runs from about 20 m/s at
//    20 Hz to 628 m/s at 20 kHz. Measured at one pickup the top of the band
//    arrives after a millisecond and the bottom after thirty. That smear,
//    treble first, is the bright "pew" on the front of every hit, and it is
//    the one thing a room cannot do — air is not dispersive, so a room's early
//    reflections arrive with the waveform intact. An emulation without it is a
//    small bright room, not a plate. See [`Dispersion`].
//
// 2. **Instant density, and no early reflections at all.** Because the top of
//    the band crosses the plate in a millisecond, the echoes are already too
//    dense to count by the time anybody hears them. There is no build-up and
//    no reflection pattern to place a listener in a space. What predelay a
//    record has, an engineer patched in front.
//
// 3. **Flat modal density.** A room's modes crowd together as f^2; a plate's
//    are spread evenly — about 1.3 per hertz for this one, right across the
//    band. So there is no low-frequency modal signature to give away a room,
//    and no thinning at the top.
//
// # Why this shape and not the physics
//
// The plate equation can be integrated directly, and the published work that
// does it for exactly this plate needed aggressive mode-rejection to reach
// real time for ONE instance on a desktop. Three `Effects` run here at once
// and this file counts multiply-adds. So: an allpass tank for density, with
// the dispersion the tank cannot produce added explicitly on top, and a decay
// that is banded because the real plate's is.
//
// The tank is the figure-of-eight arrangement — a cascade of input diffusers
// into two branches that feed each other — which is thirty years old and
// published. The delay lengths here are NOT the published ones: they are
// mutually prime values chosen for a loop of about 75 ms, because a plate is
// small and the classic numbers describe a much larger tank.

/// Where the decay control sweeps, in seconds.
///
/// **The EMT's own read-out.** The damper is a fibreglass panel that winds
/// between 11 mm and 66 mm from the plate, and the remote head that drove it
/// was marked in seconds: 0.5 to 5.5, in tenths. Anything else here would be a
/// number about the knob rather than about the plate.
pub const PLATE_DECAY_S: (f32, f32) = (0.5, 5.5);

/// How far the predelay reaches, in milliseconds.
///
/// **The plate has none**, which is why this is here. The tail starts the
/// instant the coil moves, so every record with air in front of its plate got
/// it from a tape machine patched in front. 150 ms is longer than anybody
/// patched and short enough to stay a predelay rather than a slap.
pub const PLATE_PREDELAY_MS: (f32, f32) = (0.0, 150.0);

/// The corner the tone control sweeps, in hertz.
///
/// The damper absorbs treble harder than bass, so bringing it in tilts the
/// decay as well as shortening it. This is the corner above which the tail is
/// pulled down towards [`PLATE_TONE_RATIO`] of its mid-band length.
const PLATE_TONE_HZ: (f32, f32) = (1_400.0, 9_000.0);

/// How much shorter the top of the band decays than the middle, at each end of
/// the tone control.
///
/// **Measured, not chosen.** With the damper out the real plate rings about
/// five seconds at 500 Hz and about one and a half at 10 kHz — a ratio near
/// 0.3 — and bringing the damper in shortens the top faster than the bottom.
/// A plate whose highs last as long as its mids is a spring.
const PLATE_TONE_RATIO: (f32, f32) = (0.22, 0.85);

/// Sample rate the lengths below are written at.
const DESIGN_RATE: f32 = 44_100.0;

/// The four input diffusers, in samples at [`DESIGN_RATE`].
///
/// **Short, and that is the point.** These are 1.8 to 5.7 ms: by the time the
/// tank sees the signal it has already been smeared past the point where any
/// single reflection can be picked out, which is what "no early reflections"
/// has to mean in a delay network. Mutually prime, so they cannot agree with
/// each other about which frequencies to reinforce.
const PLATE_IN_AP: [usize; 4] = [79, 113, 191, 251];

/// Their coefficients. Away from zero, which does nothing, and away from one,
/// which buzzes.
const PLATE_IN_G: [f32; 4] = [0.750, 0.750, 0.625, 0.625];

/// The tank: two branches, each an allpass, a delay, the damper, a second
/// allpass and a second delay. All at [`DESIGN_RATE`], all mutually prime.
///
/// The eight together are a loop of 3300 samples — **about 75 ms** — which is
/// the number that matters: it is how often the tail passes the damper, and it
/// is what [`Plate::feedback`] solves against to turn a decay in seconds into
/// a coefficient.
const PLATE_AP1: [usize; 2] = [131, 173];
const PLATE_D1: [usize; 2] = [661, 617];
const PLATE_AP2: [usize; 2] = [293, 359];
const PLATE_D2: [usize; 2] = [557, 509];

/// One trip round the tank, in samples at [`DESIGN_RATE`]. Kept as a constant
/// rather than summed at runtime so [`Plate::feedback`] is a closed form.
const PLATE_LOOP: f32 = 3_300.0;

/// The tank diffusers' coefficients.
const PLATE_TANK_G1: f32 = 0.70;
const PLATE_TANK_G2: f32 = 0.50;

/// How far the first tank allpass in each branch is swept, in samples, and how
/// fast.
///
/// **Small, slow, and not optional.** An allpass loop with fixed lengths
/// settles into standing tones — the network agrees with itself about a
/// handful of frequencies and rings on them after the plate would have stopped.
/// A few samples of movement at a rate nobody can hear breaks the agreement.
/// The real plate does this for free: it is a sheet of metal under tension in
/// a room, and it is never quite the same length twice.
const PLATE_MOD_SAMPLES: f32 = 6.0;
const PLATE_MOD_HZ: f32 = 0.9;

/// Scales the whole wet path.
///
/// Voiced against [`PLATE_TRIM_AT`], and scaled by the tank's energy so the
/// decay control changes how LONG and not how loud. See the comment on
/// [`Plate::process`] for the arithmetic and for why it is a square root.
const PLATE_TRIM: f32 = 0.34;

/// The decay position the trim above was voiced at, as a fraction of
/// [`PLATE_DECAY_S`]. 0.4 of 0.5..5.5 is 2.5 seconds — a plate.
const PLATE_TRIM_AT: f32 = 0.4;

/// A plain delay line with a fixed length, read at the write point.
struct Line {
    buf: Vec<f32>,
    at: usize,
}

impl Line {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            at: 0,
        }
    }

    #[inline]
    fn push(&mut self, x: f32) -> f32 {
        let out = self.buf[self.at];
        self.buf[self.at] = flush(x);
        self.at += 1;
        if self.at >= self.buf.len() {
            self.at = 0;
        }
        out
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.at = 0;
    }
}

/// A Schroeder allpass: unity magnitude, scrambled phase. What turns a handful
/// of discrete echoes into something that sounds continuous.
struct Diffuser {
    buf: Vec<f32>,
    at: usize,
    /// The length actually in use. Only the tank's first pair move theirs, and
    /// they move it inside a buffer allocated for the widest sweep — see
    /// [`PLATE_MOD_SAMPLES`], and see `ReverbChannel::set_offset` in the file
    /// this replaced for why an audio-thread reallocation is not an option.
    len: usize,
}

impl Diffuser {
    fn new(cap: usize) -> Self {
        Self {
            buf: vec![0.0; cap.max(1)],
            at: 0,
            len: cap.max(1),
        }
    }

    fn set_len(&mut self, len: usize) {
        self.len = len.clamp(1, self.buf.len());
    }

    #[inline]
    fn process(&mut self, x: f32, g: f32) -> f32 {
        // Read `len` back from the write point, which for a line this short is
        // cheaper than an interpolated tap and inaudible against it: the
        // modulation below moves whole samples slowly enough that the step is
        // far under the diffusion it is already applying.
        let read = (self.at + self.buf.len() - self.len) % self.buf.len();
        let buffered = self.buf[read];
        let v = x + buffered * g;
        self.buf[self.at] = flush(v);
        self.at += 1;
        if self.at >= self.buf.len() {
            self.at = 0;
        }
        buffered - v * g
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.at = 0;
    }
}

/// How many first-order sections the dispersion chain runs.
///
/// Twelve is where the differential group delay stops buying much and the
/// multiply count starts mattering: three multiply-adds each, twice over for
/// stereo, on every sample the plate is up.
const DISPERSION_STAGES: usize = 12;

/// The coefficients, lowest first.
///
/// **Staggered rather than identical, and that is the whole design.** One
/// first-order allpass with a NEGATIVE coefficient has its group delay maximum
/// at DC and its minimum at Nyquist — the right shape, the lows held back —
/// but a single corner. The real plate's delay goes as the inverse root of
/// frequency, a smooth slide across the whole band, so the chain spreads its
/// coefficients geometrically: the high ones hold the bottom octave back hard,
/// the low ones keep the slide going up through the mids.
///
/// Written out rather than computed so the voicing is a thing somebody can
/// read, argue with, and change one number of.
const DISPERSION_G: [f32; DISPERSION_STAGES] = [
    -0.94, -0.92, -0.90, -0.87, -0.84, -0.80, -0.75, -0.70, -0.63, -0.55, -0.46, -0.36,
];

/// A cascade of first-order allpasses that delays low frequencies more than
/// high ones.
///
/// # Why this and not a filter bank
///
/// It costs three multiply-adds a stage and it is exactly unity magnitude at
/// every frequency, which is what dispersion IS — the plate does not change
/// how loud anything is on its way across, only when it arrives. A crossover
/// into delayed bands would colour the tail and leave seams at the corners.
#[derive(Clone)]
struct Dispersion {
    x1: [f32; DISPERSION_STAGES],
    y1: [f32; DISPERSION_STAGES],
}

impl Dispersion {
    fn new() -> Self {
        Self {
            x1: [0.0; DISPERSION_STAGES],
            y1: [0.0; DISPERSION_STAGES],
        }
    }

    /// `amount` scales how many stages are in circuit, 0..=1. At 0 the chain
    /// is a wire; at 1 every stage is working.
    #[inline]
    fn process(&mut self, x: f32, amount: f32) -> f32 {
        let mut v = x;
        for i in 0..DISPERSION_STAGES {
            // Coefficient scaled towards zero rather than stages switched out,
            // so the control is continuous. An allpass at g = 0 is a one-sample
            // delay and nothing else, which is the right thing to fade to.
            let g = DISPERSION_G[i] * amount;
            let y = g * v + self.x1[i] - g * self.y1[i];
            self.x1[i] = v;
            self.y1[i] = flush(y);
            v = y;
        }
        v
    }

    fn clear(&mut self) {
        self.x1 = [0.0; DISPERSION_STAGES];
        self.y1 = [0.0; DISPERSION_STAGES];
    }
}

/// A one-pole low-pass, used as the tank's damper and as the input bandwidth.
#[derive(Clone, Copy, Default)]
struct Pole {
    z: f32,
}

impl Pole {
    /// `a` is the pole: 0 passes everything, near 1 passes almost nothing.
    #[inline]
    fn lp(&mut self, x: f32, a: f32) -> f32 {
        self.z = flush(x * (1.0 - a) + self.z * a);
        self.z
    }

    fn clear(&mut self) {
        self.z = 0.0;
    }
}

/// The plate.
struct Plate {
    /// Predelay, allocated for the longest [`PLATE_PREDELAY_MS`] can ask.
    pre: [Vec<f32>; 2],
    pre_at: usize,
    /// The coil is not a tweeter. Rolls the very top off what goes in, so the
    /// tank is not asked to diffuse something the plate could never carry.
    bandwidth: [Pole; 2],
    in_ap: [[Diffuser; 4]; 2],
    dispersion: [Dispersion; 2],
    ap1: [Diffuser; 2],
    d1: [Line; 2],
    damp: [Pole; 2],
    ap2: [Diffuser; 2],
    d2: [Line; 2],
    /// What each branch hands the other. The figure of eight.
    cross: [f32; 2],
    /// The modulation phase for [`PLATE_MOD_SAMPLES`], in turns.
    lfo: f32,
    /// Base lengths of the swept pair, in samples at this rate.
    ap1_base: [f32; 2],
    sample_rate: f32,
}

impl Plate {
    fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let scale = |n: usize| ((n as f32) * sr / DESIGN_RATE).max(1.0);
        let pre_cap = (sr * PLATE_PREDELAY_MS.1 / 1_000.0) as usize + 2;
        Self {
            pre: [vec![0.0; pre_cap], vec![0.0; pre_cap]],
            pre_at: 0,
            bandwidth: [Pole::default(); 2],
            in_ap: std::array::from_fn(|_| {
                std::array::from_fn(|i| Diffuser::new(scale(PLATE_IN_AP[i]) as usize))
            }),
            dispersion: [Dispersion::new(), Dispersion::new()],
            // The swept pair are allocated for their base length plus the
            // full sweep; the sweep only decides how much is in use.
            ap1: std::array::from_fn(|i| {
                Diffuser::new((scale(PLATE_AP1[i]) + PLATE_MOD_SAMPLES + 2.0) as usize)
            }),
            d1: std::array::from_fn(|i| Line::new(scale(PLATE_D1[i]) as usize)),
            damp: [Pole::default(); 2],
            ap2: std::array::from_fn(|i| Diffuser::new(scale(PLATE_AP2[i]) as usize)),
            d2: std::array::from_fn(|i| Line::new(scale(PLATE_D2[i]) as usize)),
            cross: [0.0; 2],
            lfo: 0.0,
            ap1_base: std::array::from_fn(|i| scale(PLATE_AP1[i])),
            sample_rate: sr,
        }
    }

    /// This decay's tank coefficient.
    ///
    /// **A time in seconds is the control; a coefficient is what the loop
    /// wants**, and solving for it here rather than mapping a knob onto one by
    /// ear is what lets the decay dial read in seconds and be telling the
    /// truth. See `the_decay_dial_is_a_time_in_seconds`, which is the test
    /// that holds this honest.
    ///
    /// **`g` is applied per BRANCH, not per loop, and the factor of two is the
    /// whole difference between a plate that rings for the time on the dial
    /// and one that rings for half of it.** Each branch multiplies its output
    /// by `g` on the way to the other, so a full trip round the figure of
    /// eight — [`PLATE_LOOP`] samples, both branches — costs `g²`. Falling
    /// 60 dB in `t60` seconds therefore wants `g² ^ (t60 / loop_secs) = 10^-3`,
    /// which is the 1.5 below rather than the 3 that a single-loop reading
    /// gives. The first version had the 3; the test measured 1.09 seconds
    /// against a dial asking for 2.20 and said so.
    ///
    /// The damper is a real loss on top of this and pulls the measured time a
    /// little under the requested one, more so as the tone control comes down.
    /// That is the plate behaving like a plate — the panel absorbs — and not
    /// an error in the solution.
    fn feedback(&self, t60: f32) -> f32 {
        let loop_secs = PLATE_LOOP / DESIGN_RATE;
        let t = t60.max(0.05);
        10f32.powf(-1.5 * loop_secs / t)
    }

    /// Seconds of decay at this position of the control.
    fn decay_secs(size: f32) -> f32 {
        PLATE_DECAY_S.0 + (PLATE_DECAY_S.1 - PLATE_DECAY_S.0) * size.clamp(0.0, 1.0)
    }

    /// One stereo frame in, the plate's output out. Wet only; the caller mixes.
    #[inline]
    fn process(&mut self, input: [f32; 2], p: &Params, predelay: f32) -> [f32; 2] {
        // ── the input path ──────────────────────────────────────────────
        //
        // Predelay first, because it is meant to be silence in front of the
        // plate and not silence in front of the diffusers.
        let pre_len = ((predelay.clamp(0.0, 1.0) * PLATE_PREDELAY_MS.1 / 1_000.0)
            * self.sample_rate) as usize;
        let cap = self.pre[0].len();
        let pre_len = pre_len.min(cap - 1);
        let read = (self.pre_at + cap - pre_len) % cap;
        let mut v = [self.pre[0][read], self.pre[1][read]];
        self.pre[0][self.pre_at] = flush(input[0]);
        self.pre[1][self.pre_at] = flush(input[1]);
        self.pre_at += 1;
        if self.pre_at >= cap {
            self.pre_at = 0;
        }

        for ch in 0..2 {
            v[ch] = self.bandwidth[ch].lp(v[ch], 0.18);
            for (ap, g) in self.in_ap[ch].iter_mut().zip(PLATE_IN_G) {
                v[ch] = ap.process(v[ch], g);
            }
            // **The chirp, applied where it is heard.** In the input path the
            // very first arrival already carries it, which is the attack;
            // inside the tank it would only accumulate, and the plate's
            // signature is on the front of the note.
            v[ch] = self.dispersion[ch].process(v[ch], p.plate_dispersion.clamp(0.0, 1.0));
        }

        // ── the tank ────────────────────────────────────────────────────
        let t60 = Self::decay_secs(p.reverb_size);
        let g = self.feedback(t60);
        // The damper. `tone` moves both where the top starts coming off and
        // how much shorter the top is than the middle; one control, because on
        // the real plate one panel does both.
        let tone = p.reverb_damp.clamp(0.0, 1.0);
        // Inverted against the control: turning tone UP is a brighter tail,
        // which means a HIGHER corner and a ratio nearer one.
        let corner = PLATE_TONE_HZ.0 + (PLATE_TONE_HZ.1 - PLATE_TONE_HZ.0) * tone;
        let ratio = PLATE_TONE_RATIO.0 + (PLATE_TONE_RATIO.1 - PLATE_TONE_RATIO.0) * tone;
        // A one-pole whose corner is `corner`, expressed as its pole.
        let damp_a = (-std::f32::consts::TAU * corner / self.sample_rate).exp();
        // How much of the damped signal is mixed back in. At ratio 1 the
        // damper is out of circuit and the tail is flat; at ratio 0.22 the top
        // decays to a fifth of the mid-band length.
        let damp_mix = 1.0 - ratio;

        self.lfo += PLATE_MOD_HZ / self.sample_rate;
        if self.lfo >= 1.0 {
            self.lfo -= 1.0;
        }
        let sweep = (std::f32::consts::TAU * self.lfo).sin() * PLATE_MOD_SAMPLES * 0.5;

        let mut out = [0.0_f32; 2];
        for ch in 0..2 {
            // The other branch's return, plus this channel's fresh input.
            let mut x = v[ch] + self.cross[ch ^ 1];
            self.ap1[ch].set_len((self.ap1_base[ch] + sweep) as usize);
            x = self.ap1[ch].process(x, PLATE_TANK_G1);
            x = self.d1[ch].push(x);
            // Damp: the mid band passes, the top is pulled towards `ratio`.
            let low = self.damp[ch].lp(x, damp_a);
            x = x - (x - low) * damp_mix;
            x = self.ap2[ch].process(x, PLATE_TANK_G2);
            // The tap. Taken BEFORE the second delay so the two channels are
            // hearing different points on the loop rather than the same point
            // one delay apart, which is what makes them decorrelate.
            out[ch] = x;
            x = self.d2[ch].push(x);
            self.cross[ch] = flush(x * g);
        }
        // **Width is a fold, not a delay length.** The two branches already
        // run different lengths, so the pair decorrelates on its own; this
        // control only decides how much of that difference survives. At 0 it
        // is the 1957 plate with its single pickup, at 1 it is the 140ST's
        // two. Done arithmetically rather than by moving a buffer, because a
        // length that moves under a drag is the war story on the reverb this
        // replaced — a knob drag is a few hundred block boundaries and every
        // one of them was a `malloc` on the audio thread.
        let w = p.reverb_width.clamp(0.0, 1.0);
        let mid = (out[0] + out[1]) * 0.5;
        [mid + (out[0] - mid) * w, mid + (out[1] - mid) * w]
    }

    fn clear(&mut self) {
        for b in &mut self.pre {
            b.fill(0.0);
        }
        self.pre_at = 0;
        for ch in 0..2 {
            self.bandwidth[ch].clear();
            for ap in &mut self.in_ap[ch] {
                ap.clear();
            }
            self.dispersion[ch].clear();
            self.ap1[ch].clear();
            self.d1[ch].clear();
            self.damp[ch].clear();
            self.ap2[ch].clear();
            self.d2[ch].clear();
        }
        self.cross = [0.0; 2];
        self.lfo = 0.0;
    }
}

// ── the tape echo ───────────────────────────────────────────────────────────
//
// A tape echo after the Roland RE-201: a free loop of quarter-inch tape in an
// open chamber, an erase head, a record head, and THREE playback heads at
// fixed distances down the path.
//
// # Why three heads and a table
//
// The RE-201's front panel has one control that is not a knob — a twelve-way
// Mode Selector — and it is the instrument. Each position switches a different
// combination of the three playback heads, and seven of the twelve also bring
// in a spring reverb. Two heads at once is not "a delay with more feedback":
// it is two rhythms against each other from one line, which is the sound
// people buy these for. See [`EchoMode`], whose table is Roland's own.
//
// # Why the heads are not 1:2:3
//
// They are meant to be. Measurements on real machines put them nearer
// **1 : 1.94 : 2.85** — the spacing was never exact and the tape path was
// never quite straight. An emulation on the round numbers sounds tidier than
// any RE-201 ever did, and it is the tidiness people notice.
//
// # Why the time still follows the tempo
//
// The hardware's Repeat Rate is a capstan speed, continuous and unmarked. This
// file's one real opinion was that a delay next to a metronome should land on
// the beat, and that opinion is kept: [`Division`] sets the time at head one
// and the other two follow it by the ratios above. Free is still there for
// anybody who wants a fixed slapback, and it is what the hardware really was.
//
// What is NOT compromised is what happens while the time is moving. On the
// real machine turning Repeat Rate spins the capstan, so audio already on the
// tape plays back at the new speed and the pitch slides — Roland's own manual
// describes it rising and then falling as fresh signal reaches the heads.
// Reading a delay line at a length that is SLEWING is exactly that, which is
// why the slew here is a tape transport spinning up rather than a smoothing
// filter: see [`TAPE_SLEW`].

/// Where each playback head sits, as a multiple of the first head's time.
///
/// **Measured, not nominal.** See the section header.
const HEAD_RATIO: [f32; 3] = [1.0, 1.94, 2.85];

/// Which heads a Mode Selector position switches on, and whether it also
/// brings in the reverb.
///
/// **This table is Roland's, read off their own documentation rather than
/// reconstructed.** It cross-checks against their tap-delay ceiling table,
/// which caps modes 1/5/8/10/11 at one second, 2/4/6/9 at two and 3/7 at
/// three — exactly the modes whose LOWEST active head is one, two and three.
/// Two independent tables agreeing is the only reason to trust a twelve-way
/// switch nobody here has in front of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EchoMode {
    /// Head 1. The short slap, and the default: it is the one position where
    /// "the repeat" means one thing, which is what a knob next to a metronome
    /// wants.
    #[default]
    One,
    /// Head 2.
    Two,
    /// Head 3.
    Three,
    /// Heads 2 and 3.
    Four,
    /// Head 1, with reverb.
    Five,
    /// Head 2, with reverb.
    Six,
    /// Head 3, with reverb.
    Seven,
    /// Heads 1 and 2, with reverb.
    Eight,
    /// Heads 2 and 3, with reverb.
    Nine,
    /// Heads 1 and 3, with reverb.
    Ten,
    /// All three heads, with reverb. The big one.
    Eleven,
    /// The spring alone, no tape at all.
    ReverbOnly,
}

impl EchoMode {
    /// Every position, in the order the switch turns.
    pub const ALL: [EchoMode; 12] = [
        EchoMode::One,
        EchoMode::Two,
        EchoMode::Three,
        EchoMode::Four,
        EchoMode::Five,
        EchoMode::Six,
        EchoMode::Seven,
        EchoMode::Eight,
        EchoMode::Nine,
        EchoMode::Ten,
        EchoMode::Eleven,
        EchoMode::ReverbOnly,
    ];

    /// Which of the three playback heads are live.
    pub fn heads(self) -> [bool; 3] {
        match self {
            EchoMode::One | EchoMode::Five => [true, false, false],
            EchoMode::Two | EchoMode::Six => [false, true, false],
            EchoMode::Three | EchoMode::Seven => [false, false, true],
            EchoMode::Four | EchoMode::Nine => [false, true, true],
            EchoMode::Eight => [true, true, false],
            EchoMode::Ten => [true, false, true],
            EchoMode::Eleven => [true, true, true],
            EchoMode::ReverbOnly => [false; 3],
        }
    }

    /// Whether the spring is in circuit.
    pub fn reverb(self) -> bool {
        !matches!(
            self,
            EchoMode::One | EchoMode::Two | EchoMode::Three | EchoMode::Four
        )
    }

    /// What the row shows. **The heads, not the number**, because the number
    /// is a position on a switch nobody here is looking at and the heads are
    /// what it does.
    pub fn label(self) -> &'static str {
        match self {
            EchoMode::One => "1 - head 1",
            EchoMode::Two => "2 - head 2",
            EchoMode::Three => "3 - head 3",
            EchoMode::Four => "4 - heads 2+3",
            EchoMode::Five => "5 - head 1 + spring",
            EchoMode::Six => "6 - head 2 + spring",
            EchoMode::Seven => "7 - head 3 + spring",
            EchoMode::Eight => "8 - heads 1+2 + spring",
            EchoMode::Nine => "9 - heads 2+3 + spring",
            EchoMode::Ten => "10 - heads 1+3 + spring",
            EchoMode::Eleven => "11 - all three + spring",
            EchoMode::ReverbOnly => "12 - spring only",
        }
    }

    /// How it is written to the settings file, and read back.
    pub fn key(self) -> &'static str {
        match self {
            EchoMode::One => "1",
            EchoMode::Two => "2",
            EchoMode::Three => "3",
            EchoMode::Four => "4",
            EchoMode::Five => "5",
            EchoMode::Six => "6",
            EchoMode::Seven => "7",
            EchoMode::Eight => "8",
            EchoMode::Nine => "9",
            EchoMode::Ten => "10",
            EchoMode::Eleven => "11",
            EchoMode::ReverbOnly => "reverb",
        }
    }

    pub fn from_key(key: &str) -> Option<EchoMode> {
        EchoMode::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// How much of each repeat comes back for the next, at each end of Intensity.
///
/// **0.90, and the ceiling is a decision rather than a limit.** A Space Echo's
/// Intensity reaches self-oscillation, and that howl is half of what the box
/// is famous for. This is a send on a master bus in a piano application, and
/// the reason written here when the top was 0.72 has not stopped being true:
/// a control somebody will turn all the way up to hear what it does should not
/// be an oscillator. What HAS changed is that 0.72 was well short of the long
/// smeared wash the machine is actually used for.
///
/// 0.90 is where it sits because `the_delay_does_not_run_away_at_any_setting`
/// wants the tail under 35% after five seconds, which at the shipped time is
/// a touch over thirteen repeats and bounds the coefficient at about 0.924.
/// The saturation in the loop would hold a higher setting bounded rather than
/// let it blow up — tape does not explode, it compresses — so this is one
/// constant away from the hardware if that trade is ever taken.
const TAPE_FEEDBACK: (f32, f32) = (0.0, 0.90);

/// Longest the line can hold, in seconds. Three heads means the last one wants
/// nearly three times the first, so this is what bounds head one's time.
const MAX_TAPE_SECS: f32 = 6.0;

/// How fast the transport reaches a new speed, as a fraction per sample.
///
/// **This is a motor, and the number is why the pitch slides.** A delay whose
/// length jumps is a click; one whose length ramps is a varispeed, and a
/// varispeed is what a capstan changing speed IS. Slow enough to hear the
/// slide, fast enough to arrive inside a bar.
const TAPE_SLEW: f32 = 0.000_02;

/// Wow: the slow one, in hertz and in fraction of the delay.
///
/// Under about 4 Hz by definition. Roland attributes the RE-201's "pleasing
/// chorus effect" to mechanical friction and tape slip, and this is that.
const WOW_HZ: f32 = 0.72;

/// Flutter: the fast one. Above 4 Hz, and on the real transport it is the
/// pinch roller's own rotation — a roller that has been parked against the
/// capstan for thirty years has a flat on it, and the period of that flat is
/// the period of the warble everybody is imitating.
const FLUTTER_HZ: f32 = 6.3;

/// How far the two move the tape, as a fraction of the delay, at full depth.
const WOW_DEPTH: f32 = 0.0035;
const FLUTTER_DEPTH: f32 = 0.0011;

/// Where the record amplifier starts to round things off.
///
/// Tape does not clip; it compresses, and it starts long before anything a
/// meter would call overload. The curve is `tanh`, which is what a saturating
/// magnetic medium's transfer function looks like near the origin and is one
/// instruction where a hysteresis model is a differential equation.
const TAPE_DRIVE: f32 = 1.6;

/// The playback loss, as a one-pole corner in hertz at each end of the tone
/// control.
///
/// **Each pass is darker than the last, and that is the whole reason tape echo
/// sits under a piano.** Head gap loss, spacing loss and the tape's own
/// self-erasure all take the top off, every trip round the loop, so the
/// fifteenth repeat is a soft thud where the first was a note.
const TAPE_TONE_HZ: (f32, f32) = (1_100.0, 7_500.0);

/// The bass lift the record amplifier's head bump puts back, and where.
///
/// The RE-201's own Bass control is +10 dB at 100 Hz per the service manual;
/// this is the fixed part of that character, the low-end thickening that is
/// there whether or not anybody touches the tone controls.
const TAPE_BUMP_HZ: f32 = 100.0;
const TAPE_BUMP: f32 = 0.18;

/// How loud the hiss is, relative to full scale.
///
/// **Roland skipped the compander.** Every other bucket-brigade and tape echo
/// of the era wrapped its delay path in one to bury the noise; the RE-201 got
/// a noise gate instead, so the hiss arrives with the note and leaves with it.
/// That is why it is modelled: a silent tape echo is a delay, and the noise
/// coming up under a held chord as the repeats build is the sound.
const TAPE_HISS: f32 = 2.2e-5;

/// Read `len` samples back from `at`, interpolating.
///
/// Free rather than a method because the chorus reads its own line the same
/// way, and two implementations of "read backwards with a fraction" is two
/// chances to get the wraparound wrong.
#[inline]
fn tap(buf: &[f32], at: usize, len: f32) -> f32 {
    let n = buf.len();
    let whole = len as usize;
    let frac = len - whole as f32;
    // `+ n` before the subtraction: `at` is smaller than `whole` for the first
    // pass through the buffer, and this is unsigned.
    let a = (at + n - whole.min(n - 1)) % n;
    let b = (a + n - 1) % n;
    buf[a] * (1.0 - frac) + buf[b] * frac
}

/// The spring tank, in parallel with the tape.
///
/// **Three springs in a "Z", per Roland**, and they are coupled — each one's
/// motion drives the others, which is where a spring's chirp and its rattle
/// both come from. Modelled as three dispersive delay lines with a little
/// cross-feed rather than as a physical spring: a spring is a dispersive
/// waveguide and this is the cheap end of that family, which is the right end
/// for something that is one of twelve positions on a switch.
struct Spring {
    line: [Vec<f32>; 3],
    at: usize,
    /// Two allpasses per spring, which is what makes the chirp rather than an
    /// echo. A real tank has far more dispersion than this; two is where it
    /// starts reading as a spring instead of a slap.
    ap: [[AllPass; 2]; 3],
    lp: [f32; 3],
}

/// Spring lengths, in samples at [`DESIGN_RATE`]. Mutually prime, and short —
/// a tank is a few tens of milliseconds, not a room.
const SPRING_LEN: [usize; 3] = [1_237, 1_499, 1_783];
/// How much of each spring's output drives the next. The "Z".
const SPRING_COUPLE: f32 = 0.22;
/// How long the tank rings.
const SPRING_FEEDBACK: f32 = 0.76;
/// The allpass coefficient that makes the chirp.
const SPRING_DISPERSION: f32 = -0.62;

impl Spring {
    fn new(sample_rate: f32) -> Self {
        let scale = |n: usize| ((n as f32) * sample_rate / DESIGN_RATE).max(1.0) as usize;
        Self {
            line: std::array::from_fn(|i| vec![0.0; scale(SPRING_LEN[i])]),
            at: 0,
            ap: [[AllPass::default(); 2]; 3],
            lp: [0.0; 3],
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let mut out = 0.0;
        let mut prev = 0.0;
        for i in 0..3 {
            let n = self.line[i].len();
            let at = self.at % n;
            let mut v = self.line[i][at];
            out += v;
            // The chirp.
            for ap in &mut self.ap[i] {
                v = ap.process(v, SPRING_DISPERSION);
            }
            // A tank is not a tweeter.
            self.lp[i] = flush(v * 0.35 + self.lp[i] * 0.65);
            v = self.lp[i];
            self.line[i][at] = flush(x + v * SPRING_FEEDBACK + prev * SPRING_COUPLE);
            prev = v;
        }
        self.at = self.at.wrapping_add(1);
        out * 0.33
    }

    fn clear(&mut self) {
        for l in &mut self.line {
            l.fill(0.0);
        }
        self.at = 0;
        self.ap = [[AllPass::default(); 2]; 3];
        self.lp = [0.0; 3];
    }
}

/// A three-head tape echo whose first head follows the session tempo.
struct TapeEcho {
    /// The tape itself. **One loop, mono**, because that is what is in the
    /// machine: the three heads read the same magnetisation at three points.
    tape: Vec<f32>,
    at: usize,
    /// Head one's delay in samples, slewed towards the target. The other two
    /// are this times [`HEAD_RATIO`].
    len: f32,
    /// Playback loss state, one per output channel plus one in the loop.
    loss: f32,
    /// The head bump's state.
    bump: f32,
    /// Where the two modulators are, in turns.
    wow: f32,
    flutter: f32,
    /// Hiss generator. A plain xorshift: this is tape noise, not a random
    /// number anybody is betting on, and it must not allocate or lock.
    noise: u32,
    spring: Spring,
    sample_rate: f32,
}

impl TapeEcho {
    fn new(sample_rate: f32) -> Self {
        let cap = (sample_rate * MAX_TAPE_SECS) as usize + 2;
        Self {
            tape: vec![0.0; cap],
            at: 0,
            len: sample_rate * FREE_SECS,
            loss: 0.0,
            bump: 0.0,
            wow: 0.0,
            flutter: 0.37,
            noise: 0x1234_5678,
            spring: Spring::new(sample_rate),
            sample_rate,
        }
    }

    /// Where head one's time is heading, in samples, from the tempo.
    ///
    /// **Head one, and the others follow.** The longest head reaches nearly
    /// three times this, so the cap has to be taken against that rather than
    /// against head one, or mode 11 at a slow tempo reads off the end of the
    /// tape.
    fn target(&self, bpm: f64, p: &Params) -> f32 {
        let secs = match p.delay_division.beats() {
            Some(beats) if bpm.is_finite() && bpm > 1.0 => (60.0 / bpm) as f32 * beats,
            _ => FREE_SECS,
        };
        let cap = (self.tape.len() - 2) as f32 / HEAD_RATIO[2];
        (secs * self.sample_rate).clamp(1.0, cap)
    }

    /// One frame. `input` is stereo but the tape is mono — the machine has one
    /// record head — so the two are summed on the way in and the heads are
    /// spread across the pair on the way out.
    #[inline]
    fn process(&mut self, input: [f32; 2], target: f32, p: &Params) -> [f32; 2] {
        let mode = p.echo_mode;
        let heads = mode.heads();
        let feedback =
            TAPE_FEEDBACK.0 + (TAPE_FEEDBACK.1 - TAPE_FEEDBACK.0) * p.delay_feedback.clamp(0.0, 1.0);

        // ── the transport ───────────────────────────────────────────────
        //
        // The length CHASES rather than jumps, and everything about how this
        // sounds while it moves comes from that. See `TAPE_SLEW`.
        let step = TAPE_SLEW * self.sample_rate;
        self.len += (target - self.len).clamp(-step, step);

        self.wow += WOW_HZ / self.sample_rate;
        if self.wow >= 1.0 {
            self.wow -= 1.0;
        }
        self.flutter += FLUTTER_HZ / self.sample_rate;
        if self.flutter >= 1.0 {
            self.flutter -= 1.0;
        }
        let warble = (std::f32::consts::TAU * self.wow).sin() * WOW_DEPTH
            + (std::f32::consts::TAU * self.flutter).sin() * FLUTTER_DEPTH;

        // ── the heads ───────────────────────────────────────────────────
        let mut wet = [0.0_f32; 2];
        let mut loop_sum = 0.0_f32;
        let cap = (self.tape.len() - 2) as f32;
        let width = p.delay_width.clamp(0.0, 1.0);
        for (i, on) in heads.iter().enumerate() {
            if !on {
                continue;
            }
            let len = (self.len * HEAD_RATIO[i] * (1.0 + warble)).clamp(1.0, cap);
            let v = tap(&self.tape, self.at, len);
            // **The heads spread across the pair, and at width 0 they do not.**
            // The machine is mono; this is the one liberty taken with it, and
            // it is a liberty a person can turn off. Head 1 sits left, head 3
            // right, head 2 in the middle, which puts the rhythm the ratios
            // make into the image as well as the time.
            let pan = match i {
                0 => -1.0,
                2 => 1.0,
                _ => 0.0,
            } * width;
            wet[0] += v * (1.0 - pan).min(1.0);
            wet[1] += v * (1.0 + pan).min(1.0);
            loop_sum += v;
        }

        // ── the loop ────────────────────────────────────────────────────
        //
        // Everything that makes a repeat sound older than the one before it
        // happens here, once per trip round the tape.
        //
        // The playback loss first: gap loss, spacing loss and self-erasure all
        // take the top off, and the tone control moves where.
        let tone = p.delay_tone.clamp(0.0, 1.0);
        let corner = TAPE_TONE_HZ.0 + (TAPE_TONE_HZ.1 - TAPE_TONE_HZ.0) * tone;
        let a = (-std::f32::consts::TAU * corner / self.sample_rate).exp();
        self.loss = flush(loop_sum * (1.0 - a) + self.loss * a);
        let mut back = self.loss;

        // The head bump: a little of the very bottom put back, which is what a
        // record head's geometry does and why tape echo thickens as it repeats
        // rather than just darkening.
        let ba = (-std::f32::consts::TAU * TAPE_BUMP_HZ / self.sample_rate).exp();
        self.bump = flush(back * (1.0 - ba) + self.bump * ba);
        back += self.bump * TAPE_BUMP;

        // Hiss, INSIDE the loop, so it builds with the repeats exactly as it
        // does on the machine. See `TAPE_HISS`.
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        let hiss = (self.noise as f32 / u32::MAX as f32 - 0.5) * TAPE_HISS;

        // The record amplifier and the tape. `tanh` because magnetic media
        // compress rather than clip, and because a bounded nonlinearity in a
        // feedback path is what keeps a high Intensity from becoming a
        // divergence rather than a howl.
        let dry = (input[0] + input[1]) * 0.5;
        let write = (dry + back * feedback + hiss) * TAPE_DRIVE;
        self.tape[self.at] = flush(write.tanh() / TAPE_DRIVE);
        self.at += 1;
        if self.at >= self.tape.len() {
            self.at = 0;
        }

        // ── the spring ──────────────────────────────────────────────────
        //
        // In PARALLEL with the tape and fed from the dry, which is where
        // Roland put it: not inside the echo's feedback, so the reverb does
        // not repeat and the repeats are not reverberated.
        if mode.reverb() {
            let s = self.spring.process(dry);
            wet[0] += s;
            wet[1] += s;
        }
        wet
    }

    fn clear(&mut self) {
        self.tape.fill(0.0);
        self.loss = 0.0;
        self.bump = 0.0;
        self.spring.clear();
    }
}

// ── chorus ──────────────────────────────────────────────────────────────────

/// The sweep's slowest and fastest, in Hz.
///
/// The CE-1's rate control covers roughly this. The bottom is a swell you feel
/// rather than hear; the top is where chorus becomes vibrato and then becomes
/// seasick.
const CHORUS_RATE_HZ: (f32, f32) = (0.12, 3.2);

/// The delay the sweep moves through, in milliseconds.
///
/// A bucket brigade at these clock rates is a few milliseconds long. Under
/// about 4ms it is a flanger; over about 12ms the pitch wobble becomes an
/// audible detune rather than a shimmer.
const CHORUS_MS: (f32, f32) = (4.5, 11.5);

/// The bucket brigade's own bandwidth, in Hz, at each end of the tone control.
///
/// **A BBD is dark, and that is most of what a CE-1 sounds like.** The chips
/// clock at a few tens of kHz and every stage loses treble; a chorus that
/// returns the top end intact sounds like a plugin, not like the pedal.
const CHORUS_LP_HZ: (f32, f32) = (2_600.0, 9_000.0);

/// A stereo chorus after the Boss CE-1.
///
/// # The stereo
///
/// **One delay line, two outputs, and the right one is inverted.** That is the
/// CE-1's actual circuit and the reason it is remembered: the wet signal is
/// added to one output and subtracted from the other, which puts the modulated
/// copy in opposite polarity across the pair. The result is enormously wide
/// from a single voice — much wider than two independent LFOs get you.
///
/// It also means the wet CANCELS if somebody sums the two channels to mono,
/// which is exactly what the hardware does and is the reason `chorus_width` is
/// a control rather than a constant: at 0 both channels get the wet in phase,
/// which is narrow and mono-safe, and at 1 it is the pedal.
///
/// # The sweep
///
/// Triangle, not sine. The CE-1's LFO is a charging capacitor and the sweep
/// is close to linear between its turning points, which is why the pitch shift
/// sits at a steady value and then reverses rather than easing continuously.
struct Chorus {
    buf: [Vec<f32>; 2],
    at: usize,
    /// The LFO's position, 0..1, wrapping.
    phase: f32,
    /// One-pole lowpass state for the BBD bandwidth, per channel.
    lp: [f32; 2],
    sample_rate: f32,
}

impl Chorus {
    fn new(sample_rate: f32) -> Self {
        // Room for the longest delay the depth can reach, and a little over.
        let cap = (sample_rate * CHORUS_MS.1 * 2.0 / 1000.0) as usize + 4;
        Self {
            buf: [vec![0.0; cap], vec![0.0; cap]],
            at: 0,
            phase: 0.0,
            lp: [0.0; 2],
            sample_rate,
        }
    }

    /// The triangle, as -1..=1.
    #[inline]
    fn triangle(phase: f32) -> f32 {
        // Up for the first half, down for the second.
        if phase < 0.5 {
            phase * 4.0 - 1.0
        } else {
            3.0 - phase * 4.0
        }
    }

    #[inline]
    fn process(&mut self, input: [f32; 2], p: &Params) -> [f32; 2] {
        let rate = CHORUS_RATE_HZ.0 + (CHORUS_RATE_HZ.1 - CHORUS_RATE_HZ.0) * p.chorus_rate;
        self.phase = (self.phase + rate / self.sample_rate).fract();

        // The sweep, in samples. Centred in the range so depth opens outward
        // from the middle rather than from the short end — a depth of zero is
        // then a fixed short delay, which is a comb filter and not a wobble.
        let (lo, hi) = (
            CHORUS_MS.0 * self.sample_rate / 1000.0,
            CHORUS_MS.1 * self.sample_rate / 1000.0,
        );
        let centre = (lo + hi) * 0.5;
        let swing = (hi - lo) * 0.5 * p.chorus_depth;
        let len = centre + Self::triangle(self.phase) * swing;

        // The BBD's bandwidth, as a one-pole coefficient.
        let cutoff = CHORUS_LP_HZ.0 + (CHORUS_LP_HZ.1 - CHORUS_LP_HZ.0) * p.chorus_tone;
        let a = (-std::f32::consts::TAU * cutoff / self.sample_rate).exp();

        let mut wet = [0.0_f32; 2];
        for ch in 0..2 {
            let tapped = tap(&self.buf[ch], self.at, len);
            self.lp[ch] = flush(tapped * (1.0 - a) + self.lp[ch] * a);
            wet[ch] = self.lp[ch];
            self.buf[ch][self.at] = flush(input[ch]);
        }
        self.at = (self.at + 1) % self.buf[0].len();

        // **The inversion.** At width 1 the right channel gets the wet in
        // opposite polarity, which is the CE-1. At 0 both get it the same way,
        // which is narrow and survives a mono fold.
        let right_sign = 1.0 - 2.0 * p.chorus_width;
        [wet[0], wet[1] * right_sign]
    }

    fn clear(&mut self) {
        for b in &mut self.buf {
            b.fill(0.0);
        }
        self.lp = [0.0; 2];
    }
}

// ── the filters ─────────────────────────────────────────────────────────────
//
// Two knobs, one implementation. A high-pass and a low-pass differ here by
// three coefficient signs and nothing else, so they are one type with a
// direction rather than two types that have to be kept in step.

/// Where the high-pass knob sweeps, in Hz.
///
/// The top is 1.2 kHz rather than anything higher because past that a
/// high-pass on a piano stops being "take the rumble out" and starts being an
/// effect, and this is the knob people reach for to do the first thing.
pub const HPF_HZ: (f32, f32) = (20.0, 1_200.0);

/// Where the low-pass knob sweeps, in Hz — ascending like every other dial
/// now, by the owner's word: fully clockwise is 20 kHz and OUT OF THE WAY,
/// turning it down darkens. See [`Sends::lpf`].
pub const LPF_HZ: (f32, f32) = (200.0, 20_000.0);

/// How much the resonance row can lift the corner, in dB of Q.
///
/// Expressed as a Q multiplier, not a gain: it is the last stage's Q that
/// moves, which is what a filter's resonance IS. 1.0 leaves Butterworth alone.
const RESONANCE: (f32, f32) = (1.0, 4.0);

/// The Butterworth Q values for a 4th-order (24 dB/oct) cascade.
///
/// Not two identical 0.707 stages — that is a Linkwitz-Riley alignment and it
/// sags 6 dB at the corner. These are the real ones: `1 / (2 cos(pi (2k+1) / 8))`
/// for k = 0, 1.
const BUTTER_Q4: (f32, f32) = (0.541_196_1, 1.306_563);

/// How fast the phaser sweeps, at each end of its knob, in hertz.
///
/// **The pedal's own range and a little either side.** A Phase 90's speed
/// control covers roughly a third of a hertz to eight; below that it stops
/// reading as movement and above it stops reading as a phaser.
pub const PHASER_RATE_HZ: (f32, f32) = (0.05, 8.0);
/// The bottom and top of where the notches travel, in hertz, at the widest
/// setting of the Range control. The narrow end of Range is a band centred in
/// the middle of this.
pub const PHASER_SWEEP_HZ: (f32, f32) = (120.0, 3_600.0);

/// One first-order all-pass: unity magnitude, and the phase turns.
///
/// **Nothing here changes how loud anything is**, which is the whole trick:
/// four of these in series leave the signal untouched to a meter and
/// completely rearranged in phase, and the notches only appear when the
/// result is added back to the dry. That is also why the mix knob below can
/// never reach 100% wet — at fully wet there are no notches at all, only a
/// signal whose phase has been moved, which is inaudible. See `Phaser::run`.
#[derive(Debug, Clone, Copy, Default)]
struct AllPass {
    x1: f32,
    y1: f32,
}

impl AllPass {
    #[inline]
    fn process(&mut self, x: f32, a: f32) -> f32 {
        let y = a * x + self.x1 - a * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// The MXR Phase 90, as a stereo pair sharing one sweep.
///
/// **One LFO for both channels, on purpose.** The pedal is mono; two
/// independent sweeps would be a stereo phaser, which is a different and
/// much wider-sounding effect and not the one the owner asked for. What the
/// two channels do NOT share is their all-pass state, because they are
/// different signals.
struct Phaser {
    stages: [[AllPass; Stages::MAX]; 2],
    /// The last output of each channel's chain, for the feedback path.
    fb: [f32; 2],
    /// Where the sweep is, in turns. Kept in `f64` because it accumulates for
    /// as long as the app is open, and an `f32` phase at 48 kHz has lost the
    /// bottom of its fraction within the hour.
    phase: f64,
    sample_rate: f32,
}

impl Phaser {
    fn new(sample_rate: f32) -> Self {
        Self {
            stages: [[AllPass::default(); Stages::MAX]; 2],
            fb: [0.0; 2],
            phase: 0.0,
            sample_rate: sample_rate.max(8_000.0),
        }
    }

    /// Advance the sweep by one frame and return this frame's all-pass
    /// coefficient.
    #[inline]
    fn step(&mut self, p: &Params) -> f32 {
        let hz = PHASER_RATE_HZ.0
            * (PHASER_RATE_HZ.1 / PHASER_RATE_HZ.0).powf(p.phaser_rate.clamp(0.0, 1.0));
        self.phase += f64::from(hz / self.sample_rate);
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }
        // A sine, which is what the pedal's op-amp oscillator makes and what
        // keeps the sweep from having a corner at each end.
        let lfo = 0.5 - 0.5 * (std::f32::consts::TAU * self.phase as f32).cos();
        // **The sweep is EXPONENTIAL in frequency**, because pitch is. A
        // linear sweep between 120 Hz and 3.6 kHz spends nine tenths of its
        // travel above a kilohertz and lurches through the bottom octave.
        let (lo, hi) = PHASER_SWEEP_HZ;
        let centre = (lo * hi).sqrt();
        let span = (hi / lo).sqrt().powf(p.phaser_range.clamp(0.0, 1.0).max(0.05));
        let (lo, hi) = (centre / span, centre * span);
        let f = lo * (hi / lo).powf(lfo);
        // **The first-order all-pass whose −90° point is `f`, and the sign
        // is the whole filter.**
        //
        // `(1 - d)/(1 + d)` is the same expression negated, it is what this
        // shipped as, and it is not an all-pass with a sweep in it — it is
        // very nearly the identity: its phase moves one degree across the
        // entire audio band, so the blend below adds a signal to a copy of
        // itself and there are no notches anywhere. Every "is it transparent"
        // test passes on it, because an identity filter is perfectly
        // transparent. What caught it was asking whether a tone in the
        // sweep's path ever gets quieter.
        let d = (std::f32::consts::PI * f / self.sample_rate).tan();
        (d - 1.0) / (d + 1.0)
    }

    /// One stereo frame in, the WET signal out. The caller does the blend.
    #[inline]
    fn run(&mut self, x: [f32; 2], a: f32, p: &Params) -> [f32; 2] {
        let n = p.phaser_stages.count();
        // **Kept well under one.** The feedback path runs through four
        // all-passes whose magnitude is exactly unity, so a coefficient of
        // 1.0 is a loop with no loss in it at all — an oscillator, at
        // whatever frequency the sweep happens to be at.
        //
        // 0.80 rather than something nearer one because this is across a
        // MASTER BUS. Feedback is what puts resonant peaks between the
        // notches — it is the difference between the two Phase 90s and the
        // reason the block logo sounds vocal — but a peak is a boost, and at
        // 0.92 the top of the knob added ten decibels to whatever the sweep
        // was sitting on. Seven is already generous for a control somebody
        // will turn all the way up to hear what it does.
        let fb = p.phaser_feedback.clamp(0.0, 1.0) * 0.80;
        let mut out = [0.0f32; 2];
        for ch in 0..2 {
            let mut v = x[ch] + self.fb[ch] * fb;
            for stage in self.stages[ch].iter_mut().take(n) {
                v = stage.process(v, a);
            }
            // Nothing infinite is allowed back into the loop. A denormal or
            // a NaN from a stalled buffer would otherwise live in `fb` for
            // the rest of the session.
            self.fb[ch] = if v.is_finite() { v.clamp(-4.0, 4.0) } else { 0.0 };
            out[ch] = v;
        }
        out
    }

    fn clear(&mut self) {
        self.stages = [[AllPass::default(); Stages::MAX]; 2];
        self.fb = [0.0; 2];
        self.phase = 0.0;
    }
}

/// One second-order section, transposed direct form II.
///
/// TDF-II because it is the one that behaves when the coefficients move under
/// it, which here they do every block that somebody is turning the knob.
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = flush(self.b1 * x - self.a1 * y + self.z2);
        self.z2 = flush(self.b2 * x - self.a2 * y);
        y
    }

    /// RBJ cookbook, low-pass or high-pass by `high`.
    fn set(&mut self, sample_rate: f32, hz: f32, q: f32, high: bool) {
        let w = std::f32::consts::TAU * (hz / sample_rate).clamp(1.0e-5, 0.49);
        let (sn, cs) = w.sin_cos();
        let alpha = sn / (2.0 * q.max(0.05));
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = if high {
            let c = (1.0 + cs) / 2.0;
            (c, -2.0 * c, c)
        } else {
            let c = (1.0 - cs) / 2.0;
            (c, 2.0 * c, c)
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = (-2.0 * cs) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    fn clear(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// One pole, for the 6 dB setting.
#[derive(Debug, Clone, Copy, Default)]
struct OnePole {
    a: f32,
    z: f32,
}

impl OnePole {
    /// Returns the low-passed value; the caller subtracts it for a high-pass.
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        self.z = flush(self.z + self.a * (x - self.z));
        self.z
    }

    fn set(&mut self, sample_rate: f32, hz: f32) {
        let w = std::f32::consts::TAU * (hz / sample_rate).clamp(1.0e-5, 0.49);
        // The exact one-pole coefficient, not the `w` approximation, because
        // this knob goes to 20 kHz where the two stop agreeing.
        self.a = 1.0 - (-w).exp();
    }

    fn clear(&mut self) {
        self.z = 0.0;
    }
}

/// A high-pass or a low-pass, at one of three slopes, on one channel.
#[derive(Debug, Clone, Copy, Default)]
struct Filter {
    one: OnePole,
    a: Biquad,
    b: Biquad,
}

impl Filter {
    #[inline]
    fn process(&mut self, x: f32, slope: Slope, high: bool) -> f32 {
        match slope {
            Slope::Six => {
                let lp = self.one.process(x);
                if high {
                    x - lp
                } else {
                    lp
                }
            }
            Slope::Twelve => self.a.process(x),
            Slope::TwentyFour => self.b.process(self.a.process(x)),
        }
    }

    /// Set whichever stages this slope runs, and only those.
    ///
    /// The Q of section `a` is not a constant: at 12 dB it is the whole filter
    /// and wants Butterworth's 0.707, at 24 dB it is half of a pair and wants
    /// 0.541. Setting it once for "a filter" and then correcting it elsewhere
    /// is how the 24 dB setting ends up quietly being a 12 dB one.
    ///
    /// Resonance always lands on the LAST section — the one already nearest
    /// resonant. Spread across both, it moves the corner as well as the peak.
    fn set(&mut self, sample_rate: f32, hz: f32, slope: Slope, resonance: f32, high: bool) {
        let lift = RESONANCE.0 + (RESONANCE.1 - RESONANCE.0) * resonance.clamp(0.0, 1.0);
        match slope {
            Slope::Six => self.one.set(sample_rate, hz),
            Slope::Twelve => {
                self.a.set(
                    sample_rate,
                    hz,
                    std::f32::consts::FRAC_1_SQRT_2 * lift,
                    high,
                );
            }
            Slope::TwentyFour => {
                self.a.set(sample_rate, hz, BUTTER_Q4.0, high);
                self.b.set(sample_rate, hz, BUTTER_Q4.1 * lift, high);
            }
        }
    }

    fn clear(&mut self) {
        self.one.clear();
        self.a.clear();
        self.b.clear();
    }
}

// ── the limiter ─────────────────────────────────────────────────────────────

/// What the limiter knob spans, in dBFS of THRESHOLD: fully left to fully
/// right.
///
/// **This one dial runs the other way and it is the only one that does.**
/// Every other knob here is off at the bottom of its travel; a threshold is
/// off at the TOP, fully clockwise, where it is above everything and catches
/// nothing. That is what a threshold control is on hardware and it is what
/// makes twelve o'clock -24 dB — the middle of the dial being the middle of
/// the range only works if the range runs the way the dial does.
pub const LIMITER_DB: (f32, f32) = (-48.0, 0.0);

/// What the GAIN dial covers, in dB. Unity at rest, and twenty-four decibels
/// of drive at the top.
///
/// **It only goes up, and it is allowed to clip.** This is the control that
/// replaced the limiter's automatic makeup: with the limiter in circuit it is
/// the loudness knob, driving the mix into a ceiling somebody chose; with the
/// limiter bypassed there is nothing above it, and turning it far enough will
/// clip the master. That is not a gap in the design — a gain control that
/// cannot be misused is a trim, and the limiter is right there.
pub const GAIN_DB: (f32, f32) = (0.0, 24.0);

/// Below this many dB of threshold the limiter is doing something.
///
/// A hair under zero rather than zero, so the resting position is genuinely
/// out of the circuit rather than a limiter that never quite catches anything.
const LIMITER_IN_AT_DB: f32 = -0.05;

/// What the release row spans, in milliseconds.
const RELEASE_MS: (f32, f32) = (20.0, 600.0);

/// What the knee row spans, in dB below the ceiling.
const KNEE_DB: (f32, f32) = (0.0, 9.0);

/// Phases the detector reconstructs between each pair of samples.
///
/// **This is what "true peak" means here.** A sample-peak limiter reads the
/// numbers in the buffer; a converter draws a smooth curve THROUGH those
/// numbers and that curve overshoots them. Four phases is what BS.1770 asks
/// for at 48 kHz, and it is the difference between "-1 dBFS in the file" and
/// "-1 dB out of the socket".
const TP_PHASES: usize = 8;

/// Taps in the reconstruction filter. Odd, so phase 0 is exactly the input
/// sample and costs nothing to "reconstruct".
const TP_TAPS: usize = 21;

/// The reconstruction filter's group delay.
const TP_CENTRE: usize = TP_TAPS / 2;

/// How far ahead the limiter looks, in milliseconds.
///
/// **Not zero, and this is why.** The first build of this had no lookahead at
/// all: detect the peak between two samples, reduce the gain on the spot. It
/// let 0.5 dB past the ceiling, and the reason is worth keeping. The peak
/// BETWEEN two samples is not made by those two samples — a converter builds
/// it from twenty of them either side. Reducing the gain on one sample and
/// leaving its neighbours alone barely moves the curve they all reconstruct
/// together.
///
/// So the gain has to be down across the whole kernel, which means it has to
/// start going down before the peak arrives. One millisecond is long enough to
/// ramp over twenty taps without the ramp itself being audible, and short
/// enough that the whole limiter costs 1.2 ms — a fifth of one buffer at the
/// rate this app opens.
const LOOKAHEAD_MS: f32 = 1.0;

/// A peak limiter that sees between the samples.
///
/// Two things happen per sample: the true peak is reconstructed at four
/// eighths of a sample either side of the centre tap, and the gain that peak
/// demands is drawn backwards as a straight line across the lookahead, kept
/// only where it is lower than what is already planned. By the time a peak
/// reaches the output, the gain has been at its final value for long enough
/// that every tap contributing to that peak was scaled by it.
///
/// The ceiling is a floor-level guarantee rather than an average: the applied
/// gain is never above the planned envelope, and the planned envelope is never
/// above what the loudest reconstructed point in range can take.
struct Limiter {
    /// `[phase][tap]`, phase 0 omitted because it is the identity.
    taps: [[f32; TP_TAPS]; TP_PHASES - 1],
    /// The last `TP_TAPS` driven input samples per channel. Index 0 is newest.
    hist: [[f32; TP_TAPS]; 2],
    /// The audio itself, delayed by the lookahead. Interleaved stereo.
    delay: Vec<[f32; 2]>,
    /// The gain planned for each of the next `look` output samples.
    env: Vec<f32>,
    /// Where the output is being read from, in both ring buffers.
    at: usize,
    /// Lookahead in samples. `env` and `delay` are one longer than this.
    look: usize,
    /// The gain actually applied, which chases `env` down at once and up on
    /// the release.
    gain: f32,
    sample_rate: f32,
}

impl Limiter {
    fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let mut taps = [[0.0; TP_TAPS]; TP_PHASES - 1];
        for (i, row) in taps.iter_mut().enumerate() {
            let frac = (i + 1) as f32 / TP_PHASES as f32;
            for (k, t) in row.iter_mut().enumerate() {
                // A windowed sinc, centred, offset by the phase. `hist` runs
                // newest-first, so tap k reads k samples ago.
                let x = k as f32 - TP_CENTRE as f32 + frac;
                let sinc = if x.abs() < 1.0e-6 {
                    1.0
                } else {
                    let pix = std::f32::consts::PI * x;
                    pix.sin() / pix
                };
                // Blackman, which buys stopband depth at a little width — the
                // right trade when the output is a maximum rather than audio.
                let n = k as f32 / (TP_TAPS - 1) as f32;
                let w = 0.42 - 0.5 * (std::f32::consts::TAU * n).cos()
                    + 0.08 * (2.0 * std::f32::consts::TAU * n).cos();
                *t = sinc * w;
            }
        }
        let look = ((LOOKAHEAD_MS * 0.001 * sr) as usize).max(TP_TAPS);
        Self {
            taps,
            hist: [[0.0; TP_TAPS]; 2],
            delay: vec![[0.0; 2]; look + 1],
            env: vec![1.0; look + 1],
            at: 0,
            look,
            gain: 1.0,
            sample_rate: sr,
        }
    }

    /// The loudest point on the reconstructed curve around the centre tap.
    #[inline]
    fn true_peak(&self) -> f32 {
        let mut peak = 0.0f32;
        for hist in &self.hist {
            peak = peak.max(hist[TP_CENTRE].abs());
            for row in &self.taps {
                let mut acc = 0.0;
                for (t, h) in row.iter().zip(hist) {
                    acc += t * h;
                }
                peak = peak.max(acc.abs());
            }
        }
        peak
    }

    /// The gain a peak demands, with a soft knee below the ceiling.
    #[inline]
    fn wanted(peak: f32, threshold: f32, knee_db: f32) -> f32 {
        if peak <= TINY {
            return 1.0;
        }
        let over_db = 20.0 * (peak / threshold).log10();
        if over_db <= -knee_db {
            1.0
        } else if knee_db > 1.0e-3 && over_db < knee_db {
            // A quadratic through (-knee, 0 dB) tangent to both segments,
            // which is the standard soft knee.
            let t = over_db + knee_db;
            10f32.powf(-(t * t / (4.0 * knee_db)) / 20.0)
        } else {
            threshold / peak
        }
    }

    /// Push one stereo frame; return the frame `look + TP_CENTRE` ago,
    /// limited.
    #[inline]
    fn process(&mut self, x: [f32; 2], threshold: f32, knee_db: f32, release: f32) -> [f32; 2] {
        for (hist, &sample) in self.hist.iter_mut().zip(&x) {
            hist.rotate_right(1);
            hist[0] = sample;
        }
        let n = self.env.len();
        // The audio that the peak just measured belongs to: the centre tap.
        let centre = [self.hist[0][TP_CENTRE], self.hist[1][TP_CENTRE]];
        let write = (self.at + self.look) % n;
        self.delay[write] = centre;

        // **Draw the gain backwards to now.** This sample leaves the limiter
        // in `look` samples' time; the gain it needs has to be reached by
        // then, so plan a straight line from where the gain is to where it
        // must be, and keep whichever is lower at every step — an earlier,
        // louder peak's plan is not allowed to lift this one.
        let want = Self::wanted(self.true_peak(), threshold, knee_db);
        if want < 1.0 {
            let from = self.gain;
            for k in 0..=self.look {
                let i = (self.at + k) % n;
                let t = k as f32 / self.look as f32;
                let along = from + (want - from) * t;
                if along < self.env[i] {
                    self.env[i] = along;
                }
            }
        }

        // Down with the plan, up on the release — and never above the plan,
        // which is what makes the ceiling a guarantee.
        let target = self.env[self.at];
        if target < self.gain {
            self.gain = target;
        } else {
            let ms = RELEASE_MS.0 + (RELEASE_MS.1 - RELEASE_MS.0) * release.clamp(0.0, 1.0);
            let coeff = (-1.0 / (ms * 0.001 * self.sample_rate)).exp();
            self.gain = target + (self.gain - target) * coeff;
        }
        let out = self.delay[self.at];
        self.env[self.at] = 1.0;
        self.at = (self.at + 1) % n;
        [out[0] * self.gain, out[1] * self.gain]
    }

    fn clear(&mut self) {
        self.hist = [[0.0; TP_TAPS]; 2];
        self.delay.iter_mut().for_each(|s| *s = [0.0; 2]);
        self.env.iter_mut().for_each(|g| *g = 1.0);
        self.at = 0;
        self.gain = 1.0;
    }
}

// ── the pair ────────────────────────────────────────────────────────────────

/// Below this a knob is off, and off means the whole effect is skipped.
///
/// **Not just multiplied by zero.** A reverb at zero mix still runs eight combs
/// per channel per sample, and both are off by default: the common case has to
/// cost nothing, or every user pays for a feature most of them never turn on.
const OFF: f32 = 0.0005;

/// How fast a knob's value reaches the audio, per sample.
///
/// The knob is read once a block and applied per sample through this, because
/// stepping a mix coefficient at a block boundary is a click, and dragging a
/// knob is a few hundred block boundaries.
const KNOB_SLEW: f32 = 0.0004;

/// How much of each effect, straight off the knobs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sends {
    pub reverb: f32,
    pub delay: f32,
    pub chorus: f32,
    /// The phaser's depth, 0..=1. **1.0 is the pedal's own fifty-fifty
    /// blend, which is as deep as notches go** — see `Effects::process`.
    pub phaser: f32,
    /// Corner frequency, across [`HPF_HZ`]. 0 is out of the way.
    pub hpf: f32,
    /// Corner frequency, across [`LPF_HZ`] — **1.0 is out of the way**, like
    /// the limiter: the dial reads in hertz now and a frequency dial where
    /// right meant LOWER was the one everybody got wrong.
    pub lpf: f32,
    /// Output gain, 0..=1 across [`GAIN_DB`]. **0.0 is unity** — this one
    /// only ever adds, so its rest is the bottom of its travel.
    pub gain: f32,
    /// The limiter's threshold, across [`LIMITER_DB`]. **1.0 is bypass**, not
    /// 0.0: see the constant. This is the one field here whose resting value
    /// is the top of its range.
    pub limiter: f32,
}

impl Default for Sends {
    /// Everything off — which for the limiter means fully clockwise.
    fn default() -> Self {
        Self {
            reverb: 0.0,
            delay: 0.0,
            chorus: 0.0,
            phaser: 0.0,
            hpf: 0.0,
            lpf: 1.0,
            gain: 0.0,
            limiter: 1.0,
        }
    }
}

impl Sends {
    /// This position's threshold, in dBFS.
    pub fn threshold_db(limiter: f32) -> f32 {
        LIMITER_DB.0 + (LIMITER_DB.1 - LIMITER_DB.0) * limiter.clamp(0.0, 1.0)
    }

    /// This position's output gain, in dB. 0 dB at rest.
    pub fn gain_db(gain: f32) -> f32 {
        GAIN_DB.0 + (GAIN_DB.1 - GAIN_DB.0) * gain.clamp(0.0, 1.0)
    }
}

/// The three effects on the instrument bus.
pub struct Effects {
    plate: Plate,
    echo: TapeEcho,
    chorus: Chorus,
    phaser: Phaser,
    hpf: [Filter; 2],
    lpf: [Filter; 2],
    limiter: Limiter,
    /// Where the wet mixes actually are, chasing where the knobs say.
    mix: Sends,
    sample_rate: f32,
    /// Everything was off last block, so the tails are already flushed and do
    /// not need flushing again.
    idle: bool,
    /// Return the WET signal alone, with no copy of what came in.
    ///
    /// **This is the difference between an insert and a send, and getting it
    /// wrong is inaudible in the code and unmissable in the room.** Every
    /// effect here is ADDITIVE — `out = dry + wet * knob`, never a crossfade —
    /// which is right for an insert across a bus, where the dry has to come
    /// out the other side.
    ///
    /// A SEND is handed a copy of signal that is already going to the master by
    /// another route. Returning `dry + wet` from that would add the dry a
    /// second time: up to six decibels louder and comb-filtered against
    /// itself, at every setting, which reads as "the reverb makes everything
    /// louder and strange" rather than as a routing mistake.
    ///
    /// The chain still RUNS on the dry — the reverb has to be fed something,
    /// and with the chorus at zero there would otherwise be nothing to feed it
    /// — so this subtracts the dry from the sum at the end rather than
    /// withholding it at the start.
    wet_only: bool,
    /// The lowest gain the limiter applied since anybody last asked, as a
    /// linear multiplier. 1.0 is "it did nothing".
    ///
    /// **Read and reset**, like the meter peaks, because gain reduction is a
    /// transient: the moment worth showing is a few samples long and the UI
    /// asks sixty times a second. An average would read as almost nothing on
    /// exactly the material the limiter is there for.
    gr: f32,
}

impl Effects {
    pub fn new(sample_rate: f32) -> Self {
        Self::build(sample_rate, false)
    }

    /// An `Effects` for a SEND: the wet signal alone. See [`wet_only`].
    ///
    /// [`wet_only`]: Effects::wet_only
    pub fn new_send(sample_rate: f32) -> Self {
        Self::build(sample_rate, true)
    }

    fn build(sample_rate: f32, wet_only: bool) -> Self {
        let sr = sample_rate.max(8_000.0);
        Self {
            wet_only,
            plate: Plate::new(sr),
            echo: TapeEcho::new(sr),
            chorus: Chorus::new(sr),
            phaser: Phaser::new(sr),
            hpf: [Filter::default(); 2],
            lpf: [Filter::default(); 2],
            limiter: Limiter::new(sr),
            mix: Sends::default(),
            sample_rate: sr,
            idle: true,
            gr: 1.0,
        }
    }

    /// Apply both, in place, to an interleaved buffer.
    ///
    /// `reverb` and `delay` are 0..=1 straight off the knobs. `bpm` sets the
    /// delay time. `channels` is the buffer's stride; anything past the first
    /// two is left alone, because these are stereo effects and a third channel
    /// is not a thing to guess about.
    pub fn process(
        &mut self,
        buf: &mut [f32],
        frames: usize,
        channels: usize,
        sends: Sends,
        p: &Params,
        bpm: f64,
    ) {
        let want = Sends {
            reverb: sends.reverb.clamp(0.0, 1.0),
            delay: sends.delay.clamp(0.0, 1.0),
            chorus: sends.chorus.clamp(0.0, 1.0),
            phaser: sends.phaser.clamp(0.0, 1.0),
            hpf: sends.hpf.clamp(0.0, 1.0),
            lpf: sends.lpf.clamp(0.0, 1.0),
            gain: sends.gain.clamp(0.0, 1.0),
            limiter: sends.limiter.clamp(0.0, 1.0),
        };
        // Every knob at rest AND every mix arrived there: nothing to do, and
        // nothing ringing that would be cut off by not doing it.
        // **The limiter's "off" is the top of its travel**, unlike everything
        // else here. Getting this backwards would leave the whole effects
        // block running on a panel with nothing switched on — or, far worse,
        // skip a limiter that was set to -48 dB.
        let quiet = |m: &Sends| {
            m.reverb < OFF
                && m.delay < OFF
                && m.chorus < OFF
                && m.phaser < OFF
                && m.hpf < OFF
                // The low-pass rests at the TOP of its travel, limiter-style.
                && m.lpf > 1.0 - OFF
                // The gain rests at the BOTTOM of its, like the sends.
                && m.gain < OFF
                && Sends::threshold_db(m.limiter) >= LIMITER_IN_AT_DB
        };
        if quiet(&want) && quiet(&self.mix) {
            // **A send returns silence, not its input.** The buffer holds a
            // copy of signal that is already reaching the master; handing it
            // back untouched because nothing is switched on would double it.
            if self.wet_only {
                buf.iter_mut()
                    .take(frames * channels)
                    .for_each(|v| *v = 0.0);
            }
            if !self.idle {
                // Once, on the way down: whatever was still ringing when the
                // knob reached zero must not be waiting inside the buffers to
                // be heard when it is turned up again half an hour later.
                self.clear();
                self.idle = true;
            }
            return;
        }
        self.idle = false;
        if channels < 2 {
            if self.wet_only {
                buf.iter_mut()
                    .take(frames * channels)
                    .for_each(|v| *v = 0.0);
            }
            return;
        }
        let target = self.echo.target(bpm, p);
        // **Decay changes how long, not how loud**, and the arithmetic is the
        // same one the Freeverb here used to need — only the loop it is
        // solved against has changed.
        //
        // The tank's impulse response is `g^n`, so its ENERGY is `1/(1 - g²)`
        // and its amplitude goes as the root of that. Normalising by `1 - g`
        // instead — the steady-state gain — overshoots badly: it made the
        // shortest decay several times louder than the longest, because a
        // short tail's level is its early diffusion and that barely depends on
        // the feedback at all.
        //
        // Without this, somebody reaching for a longer plate gets a louder one
        // and turns the send back down to compensate, and now two controls are
        // fighting over one thing.
        let plate_g = self.plate.feedback(Plate::decay_secs(p.reverb_size));
        let voiced_g = self.plate.feedback(Plate::decay_secs(PLATE_TRIM_AT));
        let energy = |g: f32| (1.0 - g * g).max(1.0e-6).sqrt();
        let trim = PLATE_TRIM * energy(plate_g) / energy(voiced_g);

        // The filters' corners, set once a block. A biquad's coefficients are
        // six transcendental functions; per-sample they would cost more than
        // the filter, and the knob does not move fast enough to notice.
        //
        // **The knob is the corner, and nothing else.** It was also a wet/dry
        // crossfade, on the theory that fading in from zero avoided a click.
        // What that actually built was a filter whose SLOPE depended on how
        // far the knob was turned: at half wet a 24 dB/octave low-pass reaches
        // 6 dB of rejection, which is not a 24 dB/octave low-pass and is not
        // what the menu says it is.
        //
        // No crossfade is needed, because the ends of the sweep are already
        // nothing: at zero the high-pass sits at 20 Hz and the low-pass at the
        // top of hearing, where a filter of any slope does nothing to music.
        // The corner follows the SLEWED knob rather than the target so a fast
        // drag glides instead of stepping.
        let sweep = |range: (f32, f32), t: f32| range.0 * (range.1 / range.0).powf(t);
        // Well clear of Nyquist: a 4th-order section at 20 kHz against a
        // 44.1 k stream is a quarter of an octave from the wall.
        let top = self.sample_rate * 0.45;
        let hpf_hz = sweep(HPF_HZ, self.mix.hpf).min(top);
        let lpf_hz = sweep(LPF_HZ, self.mix.lpf).min(top);
        // Running while EITHER the knob or the slewed value is up, so a knob
        // dropped to zero glides its corner back out of the way before the
        // filter stops rather than being switched out from under the signal.
        let phaser_on = want.phaser > OFF || self.mix.phaser > OFF;
        if !phaser_on {
            // The all-passes hold a decaying tail through the feedback path;
            // left standing while the knob is at zero it would be waiting to
            // be heard the next time somebody turned it up.
            self.phaser.clear();
        }
        let hpf_on = want.hpf > OFF || self.mix.hpf > OFF;
        // **Running while either the knob or the slewed value is up**, the
        // same rule as the filters: a gain dropped to zero has to glide back
        // to unity, and switching it out at the top of the glide is a step of
        // however many decibels were left.
        let gain_on = want.gain > OFF || self.mix.gain > OFF;
        let lpf_on = want.lpf < 1.0 - OFF || self.mix.lpf < 1.0 - OFF;
        // **The limiter's delay line is emptied while it is not in use.**
        //
        // A guard rather than a fix, and it is worth being exact about which:
        // the line holds a millisecond of audio, and playing that back on
        // re-engage an hour later would be an audible fragment of something
        // else at full level — but it cannot happen today, because `mix`
        // reaches zero by SLEWING and the limiter keeps running the whole way
        // down, flushing itself with the silence it is being handed. A test
        // written for the fragment passed with this line removed, which is the
        // only reason that is known.
        //
        // It stays because it costs one comparison a block and stops being
        // free the day anything sets `mix` instead of easing it.
        let limiter_on = Sends::threshold_db(want.limiter) < LIMITER_IN_AT_DB
            || Sends::threshold_db(self.mix.limiter) < LIMITER_IN_AT_DB;
        if !limiter_on {
            self.limiter.clear();
        }
        for ch in 0..2 {
            if hpf_on {
                self.hpf[ch].set(self.sample_rate, hpf_hz, p.hpf_slope, p.hpf_resonance, true);
            } else {
                self.hpf[ch].clear();
            }
            if lpf_on {
                self.lpf[ch].set(
                    self.sample_rate,
                    lpf_hz,
                    p.lpf_slope,
                    p.lpf_resonance,
                    false,
                );
            } else {
                self.lpf[ch].clear();
            }
        }
        // **The threshold, and NOTHING that pays for it.**
        //
        // This dial used to carry an automatic makeup exactly equal to the
        // threshold, so turning it down made the mix louder rather than
        // quieter and the output always sat at 0 dBTP. That is a one-knob
        // maximiser, and it meant two things nobody could get at: the CEILING
        // was never anybody's to choose — every setting landed on full
        // scale — and there was no way to drive the limiter without also
        // moving where it caught.
        //
        // The two jobs are two dials now. This one is a ceiling and only a
        // ceiling; the GAIN beside it is the loudness, and it is applied
        // BELOW — which is what makes it a drive and what lets it clip the
        // master when the limiter is out of circuit. See `GAIN_DB`.
        let threshold_db = Sends::threshold_db(self.mix.limiter);
        let threshold = 10f32.powf(threshold_db / 20.0);
        let knee_db = KNEE_DB.0 + (KNEE_DB.1 - KNEE_DB.0) * p.limiter_knee;

        for f in 0..frames {
            let at = f * channels;
            let dry = [buf[at], buf[at + 1]];

            self.mix.reverb += (want.reverb - self.mix.reverb) * KNOB_SLEW;
            self.mix.delay += (want.delay - self.mix.delay) * KNOB_SLEW;
            self.mix.chorus += (want.chorus - self.mix.chorus) * KNOB_SLEW;
            self.mix.phaser += (want.phaser - self.mix.phaser) * KNOB_SLEW;
            self.mix.hpf += (want.hpf - self.mix.hpf) * KNOB_SLEW;
            self.mix.lpf += (want.lpf - self.mix.lpf) * KNOB_SLEW;
            self.mix.limiter += (want.limiter - self.mix.limiter) * KNOB_SLEW;
            self.mix.gain += (want.gain - self.mix.gain) * KNOB_SLEW;
            // Decibels, converted per sample so a drag glides rather than
            // stepping at each block boundary. `powf` per frame is the price
            // of that and it is only paid while the dial is off unity.
            let drive = if gain_on {
                10f32.powf(Sends::gain_db(self.mix.gain) / 20.0)
            } else {
                1.0
            };

            // **Chorus, then delay, then reverb** — the order a pedalboard
            // runs and a desk's sends imply. A chorus AFTER the reverb smears
            // the tail into mush; the delay repeating an already chorused note
            // is what makes the repeats shimmer.
            let wet_c = self.chorus.process(dry, p);
            let chorused = [
                dry[0] + wet_c[0] * self.mix.chorus,
                dry[1] + wet_c[1] * self.mix.chorus,
            ];
            // **The phaser sits between the chorus and the delay**, which
            // is where it sits on a pedalboard: a phased signal repeating is
            // a phaser into a delay, and a delay into a phaser sweeps the
            // repeats and the note together and reads as neither.
            //
            // **And it is a BLEND, not an addition, unlike every other send
            // here.** An all-pass chain has unity magnitude — adding it to
            // the dry at full strength is where the notches come from, and
            // adding MORE than the dry starts filling them back in. The
            // knob's top is the pedal's own fifty-fifty, which is as deep as
            // a notch goes; past that there is nothing better to reach.
            let phased = if phaser_on {
                let a = self.phaser.step(p);
                let wet_p = self.phaser.run(chorused, a, p);
                let w = 0.5 * self.mix.phaser;
                [
                    chorused[0] * (1.0 - w) + wet_p[0] * w,
                    chorused[1] * (1.0 - w) + wet_p[1] * w,
                ]
            } else {
                chorused
            };
            let wet_d = self.echo.process(phased, target, p);
            let into_verb = [
                phased[0] + wet_d[0] * self.mix.delay,
                phased[1] + wet_d[1] * self.mix.delay,
            ];
            let plate = self.plate.process(into_verb, p, p.plate_predelay);
            let wet_r = [plate[0] * trim, plate[1] * trim];
            let mut out = [0.0f32; 2];
            for ch in 0..2 {
                out[ch] = phased[ch] + wet_d[ch] * self.mix.delay + wet_r[ch] * self.mix.reverb;
                // A send keeps only what it made. See `wet_only`: `phased`
                // carries the dry through both the chorus's addition and the
                // phaser's blend, so taking the dry back out of the sum
                // leaves exactly what the effects contributed.
                if self.wet_only {
                    out[ch] -= dry[ch];
                }
            }

            // **After the effects, not before them.** These two are the tone
            // of what comes out, so a reverb tail that rumbles is something
            // the high-pass can fix. Ahead of the reverb it would only stop
            // the tail being made, which is a different and less useful knob.
            //
            // Each is crossfaded rather than switched: at rest the filter
            // still runs (it costs four multiplies) but contributes nothing,
            // so there is no click at the moment somebody leaves zero.
            if hpf_on {
                for (o, f) in out.iter_mut().zip(&mut self.hpf) {
                    *o = f.process(*o, p.hpf_slope, true);
                }
            }
            if lpf_on {
                for (o, f) in out.iter_mut().zip(&mut self.lpf) {
                    *o = f.process(*o, p.lpf_slope, false);
                }
            }

            // **The gain, and it is BEFORE the limiter.** That is what makes
            // it a drive: with a ceiling in circuit, turning this up pushes
            // more of the mix into it and the limiter holds the top. With the
            // limiter bypassed there is nothing above this at all and enough
            // of it will clip — see `GAIN_DB`, which says why that is on
            // purpose.
            if gain_on {
                out[0] *= drive;
                out[1] *= drive;
            }
            // **Last, and after everything.** A limiter that is not the final
            // stage is not a limiter; it is an effect that something else gets
            // to overshoot afterwards.
            if limiter_on {
                out = self
                    .limiter
                    .process(out, threshold, knee_db, p.limiter_release);
                if self.limiter.gain < self.gr {
                    self.gr = self.limiter.gain;
                }
            }

            buf[at] = out[0];
            buf[at + 1] = out[1];
        }

        // **The flush, for a fade that finished inside this block.**
        //
        // The check at the top of this function catches a call that BEGINS
        // with everything at rest. A single long call that spans the whole
        // fade-out reaches zero down here instead, and used to leave the tail
        // standing in the buffers until some later call happened to start
        // quiet — so the promise made up there, that nothing rings on from an
        // hour ago, was only kept when the knob reached zero between calls.
        //
        // With the room this file used to have, an unflushed tail was already
        // below anything audible by the time the mix arrived at zero. A plate
        // rings for seconds, and the leftover comes back under the next note
        // as the mix slews up again. `turning_everything_off_flushes_what_was_
        // ringing` measured 7.9e-6 where it wanted nothing.
        if quiet(&want) && quiet(&self.mix) {
            self.clear();
            self.idle = true;
        }
    }

    /// How hard the limiter has been working since this was last called, in
    /// **decibels of reduction** — a positive number, 0.0 for none.
    ///
    /// Resets on read. See [`Effects::gr`].
    pub fn gain_reduction_db(&mut self) -> f32 {
        let g = std::mem::replace(&mut self.gr, 1.0);
        if g >= 1.0 {
            0.0
        } else {
            -20.0 * g.max(1.0e-6).log10()
        }
    }

    /// Drop every tail. Called when every knob reaches zero, and on a stop.
    pub fn clear(&mut self) {
        self.plate.clear();
        self.echo.clear();
        self.chorus.clear();
        for f in self.hpf.iter_mut().chain(self.lpf.iter_mut()) {
            f.clear();
        }
        self.limiter.clear();
        self.gr = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A send returns what it made, and nothing it was given.**
    ///
    /// Every effect here is additive — `out = dry + wet * knob` — which is
    /// right for an insert and wrong for a send: the bus is handed a COPY of
    /// signal that already reaches the master by another route, so returning
    /// the dry with it would add that signal twice. Six decibels and a comb
    /// filter, at every setting.
    #[test]
    fn a_send_never_returns_the_signal_it_was_handed() {
        // Long enough for a comb to come back: the shortest one in a
        // Freeverb-shaped reverb is over a thousand samples, so a short buffer
        // would show silence and prove nothing.
        const FRAMES: usize = 4096;
        let p = Params::default();

        // Nothing switched on: a send is silent, an insert is a wire.
        let mut send = Effects::new_send(48_000.0);
        let mut buf = vec![0.5_f32; FRAMES * 2];
        send.process(&mut buf, FRAMES, 2, Sends::default(), &p, 120.0);
        assert!(
            buf.iter().all(|v| v.abs() < 1.0e-9),
            "an idle send handed its input back, which would double it"
        );

        let mut insert = Effects::new(48_000.0);
        let mut buf = vec![0.5_f32; FRAMES * 2];
        insert.process(&mut buf, FRAMES, 2, Sends::default(), &p, 120.0);
        assert!(
            buf.iter().all(|v| (v - 0.5).abs() < 1.0e-6),
            "an idle insert was supposed to pass its input through untouched"
        );

        // Reverb up: the send returns a tail and no copy of the input. The
        // first frame is the one that says so — a reverb has pre-delay before
        // anything comes back, so anything there is the dry leaking through.
        let mut send = Effects::new_send(48_000.0);
        let sends = Sends {
            reverb: 1.0,
            limiter: 1.0,
            ..Sends::default()
        };
        let mut buf = vec![0.5_f32; FRAMES * 2];
        send.process(&mut buf, FRAMES, 2, sends, &p, 120.0);
        assert!(
            buf[0].abs() < 0.05,
            "the first frame came back at {}, which is the dry signal",
            buf[0]
        );
        // And it did something: silence would mean the chain never ran.
        let peak = buf.iter().fold(0.0_f32, |m, v| m.max(v.abs()));
        assert!(peak > 1.0e-6, "the send produced nothing at all");
    }

    /// **A width move must not restart the whole room.** The width knob is a
    /// buffer length, and this code used to rebuild BOTH reverb channels
    /// from fresh `Vec`s whenever it moved — an allocation on the audio
    /// thread for every block of a knob drag, and a tail wiped from both
    /// sides. Now the buffers are allocated once and the knob moves the wrap
    /// points; only the right channel, which carries the spread, restarts.
    /// The left tail riding through a width change is the observable proof
    /// that nothing was rebuilt.
    #[test]
    fn a_width_change_does_not_silence_the_left_tail() {
        const FRAMES: usize = 2048;
        let mut fx = Effects::new_send(SR);
        let mut p = Params::default();
        let sends = Sends {
            reverb: 1.0,
            limiter: 1.0,
            ..Sends::default()
        };

        // Ring the room with a burst.
        let mut buf = vec![0.5_f32; FRAMES * 2];
        fx.process(&mut buf, FRAMES, 2, sends, &p, 120.0);

        // Move the knob well past its epsilon and hand the chain silence:
        // the LEFT channel keeps ringing, because its lengths never moved.
        p.reverb_width = 0.2;
        let mut tail = vec![0.0_f32; FRAMES * 2];
        fx.process(&mut tail, FRAMES, 2, sends, &p, 120.0);
        let left = tail
            .chunks_exact(2)
            .map(|f| f[0].abs())
            .fold(0.0_f32, f32::max);
        assert!(
            left > 1.0e-5,
            "the width change silenced the left tail: peak {left}"
        );

        // And a full sweep, a step per block — a drag, as the UI delivers
        // one — stays on the rails at every position.
        for i in 0..=100 {
            p.reverb_width = i as f32 / 100.0;
            let mut b = vec![0.0_f32; 256 * 2];
            fx.process(&mut b, 256, 2, sends, &p, 120.0);
            assert!(
                b.iter().all(|v| v.is_finite()),
                "the width sweep went non-finite at {}",
                p.reverb_width
            );
        }
    }

    const SR: f32 = 48_000.0;

    /// Reconstruct the analogue peak of a buffer, INDEPENDENTLY of the
    /// limiter's own detector.
    ///
    /// **Eight times, thirty-three taps, against the limiter's four and
    /// thirteen.** A true-peak claim checked with the same filter that made it
    /// is not a check; it is the detector agreeing with itself. This is the
    /// slower, longer reconstruction a converter is closer to.
    fn true_peak(buf: &[f32], channels: usize, ch: usize) -> f32 {
        const PHASES: usize = 8;
        const TAPS: usize = 33;
        const CENTRE: isize = (TAPS / 2) as isize;
        let x: Vec<f32> = buf.iter().skip(ch).step_by(channels).copied().collect();
        let mut peak = 0.0f32;
        // **The interior only.** Off the ends this convolution has no samples
        // to read and treats them as silence, which is a step from zero into
        // whatever the block starts on — and a truncated sinc rings on a step
        // by about 9%. Measured over the whole buffer that ringing reads as
        // the limiter overshooting by 0.9 dB, which is a bug in the ruler.
        for n in CENTRE as usize..x.len().saturating_sub(CENTRE as usize) {
            peak = peak.max(x[n].abs());
            for ph in 1..PHASES {
                let frac = ph as f32 / PHASES as f32;
                let mut acc = 0.0;
                for k in 0..TAPS {
                    let t = k as f32 - CENTRE as f32 - frac;
                    let sinc = if t.abs() < 1.0e-9 {
                        1.0
                    } else {
                        let pit = std::f32::consts::PI * t;
                        pit.sin() / pit
                    };
                    let w = 0.42
                        - 0.5 * (std::f32::consts::TAU * k as f32 / (TAPS - 1) as f32).cos()
                        + 0.08 * (2.0 * std::f32::consts::TAU * k as f32 / (TAPS - 1) as f32).cos();
                    let i = n as isize + k as isize - CENTRE;
                    if i >= 0 && (i as usize) < x.len() {
                        acc += sinc * w * x[i as usize];
                    }
                }
                peak = peak.max(acc.abs());
            }
        }
        peak
    }

    /// A sine, interleaved stereo, at `amp`, starting at sample `from`.
    ///
    /// **`from` matters.** Restarting the phase at every block puts a step in
    /// the waveform at every block boundary, which is a click with content all
    /// the way to Nyquist — and then the thing being measured is the limiter's
    /// response to a click rather than to the sine it was handed.
    fn sine(frames: usize, hz: f32, amp: f32, from: usize) -> Vec<f32> {
        let mut v = vec![0.0; frames * 2];
        for f in 0..frames {
            let t = (from + f) as f32;
            let x = amp * (std::f32::consts::TAU * hz * t / SR).sin();
            v[f * 2] = x;
            v[f * 2 + 1] = x;
        }
        v
    }

    /// A knob position that puts the threshold at `db`.
    fn threshold_at(db: f32) -> f32 {
        (db - LIMITER_DB.0) / (LIMITER_DB.1 - LIMITER_DB.0)
    }

    /// **The gain-reduction readout is the reduction that happened.**
    ///
    /// It is what the wash behind the master readout shows, so a number that
    /// drifts from what the DSP actually did is a meter that lies about the
    /// one thing it exists to report.
    #[test]
    fn the_gain_reduction_readout_matches_the_gain_applied() {
        let p = Params::default();
        let sends = Sends {
            limiter: threshold_at(-12.0),
            ..Sends::default()
        };
        let mut fx = Effects::new(SR);
        // Nothing in: nothing reduced.
        let mut quiet = vec![0.0; 2_048 * 2];
        fx.process(&mut quiet, 2_048, 2, sends, &p, 120.0);
        assert_eq!(fx.gain_reduction_db(), 0.0, "silence was limited");

        // A full-scale sine against a -12 dB threshold: about 12 dB of it has
        // to come off.
        for block in 0..40 {
            let mut buf = sine(2_048, 1_000.0, 1.0, block * 2_048);
            fx.process(&mut buf, 2_048, 2, sends, &p, 120.0);
        }
        let mut last = sine(2_048, 1_000.0, 1.0, 40 * 2_048);
        let before = last.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        fx.process(&mut last, 2_048, 2, sends, &p, 120.0);
        let after = last.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        let reported = fx.gain_reduction_db();
        // What the LIMITER did, which is what the meter reports. There is
        // nothing to add back any more: the automatic makeup is gone and the
        // reduction the meter shows is the whole of the difference between
        // what went in and what came out.
        let measured = -20.0 * (after / before).log10();
        assert!(
            (reported - measured).abs() < 0.5,
            "it reported {reported:.2} dB of reduction and applied {measured:.2}"
        );
        assert!(
            (reported - 12.0).abs() < 1.0,
            "a full-scale sine against -12 dB lost {reported:.2} dB"
        );
        // And reading it clears it.
        assert_eq!(fx.gain_reduction_db(), 0.0, "the reading did not reset");
    }

    /// **The output brickwalls at the THRESHOLD, wherever the threshold is.**
    ///
    /// It used to brickwall at 0 dBTP whatever the dial said, because the
    /// automatic makeup gave back exactly what the ceiling took. With that
    /// gone, the dial means what it says: set it to −6 and nothing leaves
    /// above −6.
    ///
    /// Measured on the reconstructed waveform rather than on the samples,
    /// because that is where the claim lives: a buffer whose samples all sit
    /// under full scale can still hand a converter a curve that goes over,
    /// and "true peak" is the promise that it does not.
    #[test]
    fn the_limiter_holds_a_true_peak_ceiling_at_every_threshold() {
        // 7 kHz at 48 k is a little over six samples a cycle: the peaks land
        // between samples most of the time, which is the case a sample-peak
        // limiter gets wrong.
        for (hz, amp) in [(7_000.0, 1.0), (11_000.0, 0.9), (997.0, 1.0)] {
            for want_db in [-1.0_f32, -6.0, -18.0, -36.0] {
                let mut fx = Effects::new(SR);
                let p = Params::default();
                let sends = Sends {
                    limiter: threshold_at(want_db),
                    ..Sends::default()
                };
                let mut worst = 0.0f32;
                for block in 0..40 {
                    let mut buf = sine(2_048, hz, amp, block * 2_048);
                    fx.process(&mut buf, 2_048, 2, sends, &p, 120.0);
                    // Skip the first blocks: the knob is still slewing, and
                    // the threshold slews with it.
                    if block >= 30 {
                        worst = worst.max(true_peak(&buf, 2, 0));
                    }
                }
                let got_db = 20.0 * worst.log10();
                assert!(
                    got_db <= want_db + 0.35,
                    "{hz} Hz reconstructed to {got_db:.2} dBTP with a \
                     {want_db:.1} dB threshold"
                );
                // And it is passing signal, not just being quiet. Against the
                // THRESHOLD, not against full scale: a −36 dB ceiling that
                // let −3 dBTP through would be no ceiling at all.
                assert!(
                    got_db > want_db - 3.0,
                    "{hz} Hz only reached {got_db:.2} dBTP against a {want_db:.1} dB \
                     threshold - nothing is getting through"
                );
            }
        }
    }

    // ── the phaser ──────────────────────────────────────────────────────────

    /// Peak level of a tone through the phaser at a given depth.
    fn phaser_peak(mix: f32, p: &Params, hz: f32) -> f32 {
        let mut fx = Effects::new(SR);
        let sends = Sends { phaser: mix, ..Sends::default() };
        let mut worst = 0.0f32;
        for block in 0..40 {
            let mut buf = sine(2_048, hz, 0.5, block * 2_048);
            fx.process(&mut buf, 2_048, 2, sends, p, 120.0);
            if block >= 20 {
                worst = worst.max(buf.iter().fold(0.0f32, |a, b| a.max(b.abs())));
            }
        }
        worst
    }

    /// **An all-pass chain on its own changes nothing you can hear**, and the
    /// notches are made by adding it back to the dry.
    ///
    /// This is the fact the mix knob is built around and the one that is easy
    /// to get backwards: a phaser wired like the three sends beside it —
    /// `dry + wet * knob`, with the knob reaching 1.0 — would get SHALLOWER
    /// past halfway and vanish entirely at the top, because at equal parts
    /// dry and all-pass the cancellation is total and past that the dry stops
    /// dominating. The knob's top is the pedal's own fifty-fifty for exactly
    /// this reason.
    #[test]
    fn the_all_pass_chain_alone_is_inaudible_and_the_notches_come_from_the_blend() {
        // No feedback, so this is a bare chain of all-passes and nothing
        // else — the property being asserted is about the filter, not about
        // the resonance around it.
        let p = Params { phaser_feedback: 0.0, ..Params::default() };
        let mut fx = Effects::new(SR);
        let sends = Sends { phaser: 1.0, ..Sends::default() };
        // Drive the chain directly and look at what leaves it, rather than
        // what the blend leaves: `run` is the all-pass chain and `process`
        // is the pedal.
        let mut worst_in = 0.0f32;
        let mut worst_out = 0.0f32;
        for block in 0..40 {
            let buf = sine(2_048, 800.0, 0.5, block * 2_048);
            for f in 0..2_048 {
                let x = [buf[f * 2], buf[f * 2 + 1]];
                let a = fx.phaser.step(&p);
                let y = fx.phaser.run(x, a, &p);
                if block >= 20 {
                    worst_in = worst_in.max(x[0].abs());
                    worst_out = worst_out.max(y[0].abs());
                }
            }
        }
        let db = 20.0 * (worst_out / worst_in).log10();
        assert!(
            db.abs() < 0.5,
            "four all-passes changed the level by {db:.2} dB - they are not all-pass"
        );
        let _ = sends;
    }

    /// **The notches are there.**
    ///
    /// The test above cannot show this and a broken filter proved it: a
    /// coefficient with the wrong sign made the all-pass very nearly the
    /// IDENTITY — one degree of phase across the whole audio band — and an
    /// identity filter is perfectly all-pass, perfectly transparent, and
    /// passes every "does it change the level" test there is. What it does
    /// not do is notch. This is the test that asked.
    ///
    /// Measured as the quietest 5 ms window over a few seconds of sweep
    /// against the same tone with the phaser at rest. **Short windows on
    /// purpose**: a notch is only over a given frequency for a few
    /// milliseconds at a time, and a block long enough to hold thirty cycles
    /// of the tone averages straight over it.
    #[test]
    fn a_tone_in_the_sweeps_path_is_notched() {
        // A fast sweep so several notches cross the tone inside the window,
        // and the wide range so they cross it at all.
        let p = Params {
            phaser_rate: 0.8,
            phaser_range: 1.0,
            ..Params::default()
        };
        let trough = |mix: f32| {
            let mut fx = Effects::new(SR);
            let sends = Sends { phaser: mix, ..Sends::default() };
            let mut lowest = f32::INFINITY;
            for block in 0..400 {
                let mut buf = sine(256, 700.0, 0.5, block * 256);
                fx.process(&mut buf, 256, 2, sends, &p, 120.0);
                // Once the knob has slewed.
                if block >= 100 {
                    let e: f32 = buf.iter().map(|v| v * v).sum::<f32>() / buf.len() as f32;
                    lowest = lowest.min(e.sqrt());
                }
            }
            lowest
        };
        let off = trough(0.0);
        let on = trough(1.0);
        let depth = 20.0 * (off / on).log10();
        assert!(
            depth > 10.0,
            "the deepest the sweep got was {depth:.2} dB below the dry tone"
        );
    }

    /// **The feedback loop cannot run away.**
    ///
    /// Every stage in it has unity magnitude, so a feedback coefficient of
    /// 1.0 would be a loop with no loss anywhere — an oscillator at whatever
    /// frequency the sweep happens to be sitting at, which on a master bus is
    /// a tone nobody asked for at whatever the monitors are set to.
    #[test]
    fn the_feedback_never_runs_away_even_at_the_top_of_its_travel() {
        for stages in Stages::ALL {
            let p = Params {
                phaser_feedback: 1.0,
                phaser_stages: stages,
                ..Params::default()
            };
            let mut fx = Effects::new(SR);
            let sends = Sends { phaser: 1.0, ..Sends::default() };
            let mut worst = 0.0f32;
            for block in 0..200 {
                // An IMPULSE and then silence: whatever is left ringing is
                // the loop feeding itself.
                let mut buf = vec![0.0f32; 2_048 * 2];
                if block == 0 {
                    buf[0] = 1.0;
                    buf[1] = 1.0;
                }
                fx.process(&mut buf, 2_048, 2, sends, &p, 120.0);
                if block >= 100 {
                    worst = worst.max(buf.iter().fold(0.0f32, |a, b| a.max(b.abs())));
                }
                assert!(
                    buf.iter().all(|v| v.is_finite()),
                    "{} stages produced a non-finite sample",
                    stages.count()
                );
            }
            assert!(
                worst < 0.05,
                "{} stages were still ringing at {worst:.3} four seconds after one impulse",
                stages.count()
            );
        }
    }

    /// The phaser at rest costs nothing and passes the signal untouched.
    #[test]
    fn the_phaser_at_rest_is_not_in_the_circuit() {
        let p = Params::default();
        let mut fx = Effects::new(SR);
        let mut buf = sine(2_048, 1_000.0, 0.5, 0);
        let before = buf.clone();
        fx.process(&mut buf, 2_048, 2, Sends::default(), &p, 120.0);
        assert_eq!(buf, before, "the phaser did something at zero");
        assert_eq!(Sends::default().phaser, 0.0);
        // And the same tone, unchanged, whatever the parameters say — a
        // parameter cannot switch an effect on that the knob has switched off.
        let loud = Params {
            phaser_rate: 1.0,
            phaser_feedback: 1.0,
            phaser_range: 1.0,
            phaser_stages: Stages::Eight,
            ..Params::default()
        };
        let mut buf = sine(2_048, 1_000.0, 0.5, 0);
        let before = buf.clone();
        fx.process(&mut buf, 2_048, 2, Sends::default(), &loud, 120.0);
        assert_eq!(buf, before);
    }

    /// **More stages means more notches**, and the level never runs away.
    ///
    /// Every pair of all-pass stages puts one notch in the spectrum, so the
    /// number is how many sweep past rather than a quality setting. The
    /// second half of this is the bound that matters on a master bus: a
    /// phaser is a blend of a signal with a phase-shifted copy of itself, so
    /// the sum can peak above either — that is what the resonance between the
    /// notches IS — and it must not do so by very much.
    #[test]
    fn more_stages_notch_more_and_never_run_away() {
        for stages in Stages::ALL {
            // **The maximum feedback, which is the worst case for level.**
            // At rest the resonance is mild; the number worth bounding is
            // the one somebody reaches by turning the knob to the end.
            let p = Params {
                phaser_rate: 0.8,
                phaser_range: 1.0,
                phaser_feedback: 1.0,
                phaser_stages: stages,
                ..Params::default()
            };
            let off = phaser_peak(0.0, &p, 700.0);
            let on = phaser_peak(1.0, &p, 700.0);
            assert!(off > 0.0 && on > 0.0);
            let db = 20.0 * (on / off).log10();
            assert!(
                db < 9.0,
                "{} stages at full feedback made a 700 Hz tone {db:.2} dB louder",
                stages.count()
            );
        }
        assert_eq!(Stages::Two.count(), 2);
        assert_eq!(Stages::Four.count(), 4, "the Phase 90 is a four-stage pedal");
        assert_eq!(Stages::Eight.count(), Stages::MAX);
    }

    /// **Fully right is 0 dB, and 0 dB is out of the circuit.**
    ///
    /// The one dial here that rests at the TOP of its travel. Twelve o'clock
    /// is the middle of the range because the range runs the way the dial
    /// does — if these three ever stop agreeing, the knob is lying about where
    /// the limiter is.
    #[test]
    fn the_limiter_dial_runs_from_off_at_the_top() {
        assert_eq!(Sends::threshold_db(1.0), 0.0, "fully right is not 0 dB");
        assert_eq!(
            Sends::threshold_db(0.5),
            -24.0,
            "twelve o'clock is not -24 dB"
        );
        assert_eq!(Sends::threshold_db(0.0), -48.0, "fully left is not -48 dB");

        // And at rest it does nothing at all, bit for bit.
        let p = Params::default();
        let mut fx = Effects::new(SR);
        let mut buf = sine(2_048, 1_000.0, 1.0, 0);
        let before = buf.clone();
        fx.process(&mut buf, 2_048, 2, Sends::default(), &p, 120.0);
        assert_eq!(buf, before, "the limiter did something at 0 dB");
        assert_eq!(
            Sends::default().limiter,
            1.0,
            "a default Sends has the limiter switched IN"
        );
    }

    /// **The threshold is a CEILING and nothing else.**
    ///
    /// It used to be a loudness control: an automatic makeup exactly equal to
    /// the threshold meant turning it down made the mix LOUDER and the output
    /// always landed on full scale. That was one knob doing two jobs, and it
    /// took the ceiling away from the person who wanted to choose it. The two
    /// are separate dials now — this and the GAIN — and the test that used to
    /// assert the makeup asserts its absence.
    #[test]
    fn the_threshold_is_a_ceiling_and_lowering_it_lowers_the_level() {
        let p = Params::default();
        // A steady tone well under full scale. **No transient bursts**: with
        // a quarter-second release a spike holds the gain down long after it
        // has gone, which is the limiter behaving correctly and a terrible
        // way to measure level.
        let energy = |limiter: f32, gain: f32| {
            let mut fx = Effects::new(SR);
            let sends = Sends { limiter, gain, ..Sends::default() };
            let mut sum = 0.0f64;
            let mut n = 0u64;
            for block in 0..60 {
                let mut buf = sine(2_048, 220.0, 0.25, block * 2_048);
                fx.process(&mut buf, 2_048, 2, sends, &p, 120.0);
                // Once the knob has slewed and the release has settled.
                if block >= 50 {
                    for v in &buf {
                        sum += f64::from(*v) * f64::from(*v);
                        n += 1;
                    }
                }
            }
            10.0 * (sum / n as f64).max(1e-30).log10()
        };
        // −12 dBFS in, and the limiter out of circuit: it comes out at −12.
        let off = energy(1.0, 0.0);
        assert!((off - -15.0).abs() < 1.0, "the tone measured {off:.1} dB");
        // At a −24 dB threshold it is pulled DOWN and stays down. The old
        // makeup put it back at the ceiling; there is nothing to put it back
        // now, which is the point.
        let mid = energy(0.5, 0.0);
        assert!(
            mid < off - 6.0,
            "at -24 dB it measured {mid:.1} dB against {off:.1} with the limiter out - \
             the automatic makeup is back"
        );
        // And further down is quieter still, monotonically, which is what a
        // ceiling does and what the old control could not do at all.
        let deep = energy(0.0, 0.0);
        assert!(
            deep < mid - 6.0,
            "at -48 dB it measured {deep:.1} dB, no lower than the {mid:.1} at -24"
        );
    }

    /// **The gain is the loudness control now, and it is a drive.**
    ///
    /// Turning it up with a ceiling in circuit pushes more of the mix into
    /// the ceiling; the level arrives at the threshold and stops there. That
    /// is the behaviour the old one-knob limiter had built in and could not
    /// be talked out of.
    #[test]
    fn the_gain_drives_the_limiter_and_the_ceiling_holds() {
        let p = Params::default();
        let peak = |limiter: f32, gain: f32| {
            let mut fx = Effects::new(SR);
            let sends = Sends { limiter, gain, ..Sends::default() };
            let mut worst = 0.0f32;
            for block in 0..60 {
                let mut buf = sine(2_048, 220.0, 0.25, block * 2_048);
                fx.process(&mut buf, 2_048, 2, sends, &p, 120.0);
                if block >= 50 {
                    for v in &buf {
                        worst = worst.max(v.abs());
                    }
                }
            }
            20.0 * worst.max(1e-9).log10()
        };
        // A −6 dB ceiling. Driven hard, the output sits on it.
        let threshold = 0.875; // -6 dB across LIMITER_DB
        assert!((Sends::threshold_db(threshold) - -6.0).abs() < 0.01);
        let driven = peak(threshold, 1.0);
        assert!(
            driven < -6.0 + 0.6,
            "with a -6 dB ceiling and +24 dB of drive the peak reached {driven:.2} dBFS"
        );
        assert!(
            driven > -6.0 - 1.5,
            "the drive did not reach the ceiling: {driven:.2} dBFS"
        );
        // **And with the limiter OUT, the same drive clips.** This is the
        // property the owner asked for in as many words — "even allowing
        // clipping in the master if user isn\'t careful" — and it is the
        // reason the gain sits before the limiter rather than after it.
        let unlimited = peak(1.0, 1.0);
        assert!(
            unlimited >= 0.0,
            "with the limiter bypassed, +24 dB of drive peaked at only {unlimited:.2} dBFS"
        );
    }

    /// **Three slopes, and they are the slopes they are named after.**
    ///
    /// 6, 12 and 24 dB an octave are first, second and fourth order, and the
    /// response of each is checked against the Butterworth its name promises:
    /// `1 / sqrt(1 + r^2n)`, where `r` is how many times past the corner the
    /// frequency is. Checking the ASYMPTOTE alone would pass a filter that is
    /// the right steepness in the far stopband and the wrong shape anywhere
    /// near the corner, which is the part anybody hears.
    #[test]
    fn the_filter_slopes_are_what_they_say() {
        for (slope, order) in [
            (Slope::Six, 1_i32),
            (Slope::Twelve, 2),
            (Slope::TwentyFour, 4),
        ] {
            for high in [true, false] {
                // Both measurement points stay under an eighth of the sample
                // rate: a digital filter's response bends towards Nyquist, and
                // measuring a low-pass at 16 kHz against 48 k reads 3.9 dB an
                // octave for a perfectly good 6 — that is the warping, not the
                // filter. Deeper than this the levels reach -90 dB, where an
                // f32 biquad is at its own floor and reads shallow.
                let corner = if high { 2_000.0_f32 } else { 200.0 };
                let theory = |hz: f32| {
                    let r = if high { corner / hz } else { hz / corner };
                    -10.0 * (1.0 + r.powi(2 * order)).log10()
                };
                let measure = |hz: f32| {
                    let mut f = Filter::default();
                    f.set(SR, corner, slope, 0.0, high);
                    let n = 48_000;
                    let mut peak = 0.0f32;
                    for i in 0..n {
                        let y = f.process(
                            (std::f32::consts::TAU * hz * i as f32 / SR).sin(),
                            slope,
                            high,
                        );
                        // The last third, once the transient has gone.
                        if i > n * 2 / 3 {
                            peak = peak.max(y.abs());
                        }
                    }
                    20.0 * peak.max(1.0e-9).log10()
                };
                let side = if high { "high-pass" } else { "low-pass" };
                for mul in [0.5_f32, 0.25, 1.0] {
                    let hz = if high { corner * mul } else { corner / mul };
                    let (got, want) = (measure(hz), theory(hz));
                    assert!(
                        (got - want).abs() < 1.2,
                        "{slope:?} {side} at {hz:.0} Hz measured {got:.1} dB, \
                         Butterworth says {want:.1} dB"
                    );
                }
                // And the pair of points an octave apart really is 6 dB per
                // order, which is what the label on the menu claims.
                let (near, far) = if high {
                    (corner * 0.25, corner * 0.125)
                } else {
                    (corner * 4.0, corner * 8.0)
                };
                let per_octave = theory(near) - theory(far);
                assert!(
                    (per_octave - f32::from(order as i16) * 6.0).abs() < 0.5,
                    "{slope:?} works out at {per_octave:.1} dB an octave"
                );
            }
        }
    }

    /// **A filter half way up is still the slope it says it is.**
    ///
    /// The knob used to be the corner AND a wet/dry mix, so at half travel a
    /// 24 dB/octave low-pass rejected 6 dB — the crossfade put the unfiltered
    /// signal straight back. Measured through `Effects`, not through `Filter`,
    /// because the bug was never in the filter.
    #[test]
    fn a_filter_at_half_travel_still_rejects() {
        // Half way up the low-pass sweep is about 2 kHz; 16 kHz is three
        // octaves past it and a fourth-order filter owes about 72 dB there.
        let p = Params::default();
        let sends = Sends {
            lpf: 0.5,
            ..Sends::default()
        };
        let mut fx = Effects::new(SR);
        let mut worst: f32 = 0.0;
        for block in 0..60 {
            let mut buf = sine(2_048, 16_000.0, 1.0, block * 2_048);
            fx.process(&mut buf, 2_048, 2, sends, &p, 120.0);
            // Once the knob has slewed all the way up.
            if block >= 50 {
                worst = worst.max(buf.iter().skip(64).fold(0.0f32, |a, b| a.max(b.abs())));
            }
        }
        let db = 20.0 * worst.max(1.0e-9).log10();
        assert!(
            db < -30.0,
            "a half-open 24 dB/oct low-pass left 16 kHz at {db:.1} dB - it is \
             mixing the dry signal back in"
        );
    }

    /// Every new knob at zero leaves the signal alone.
    ///
    /// The filters are crossfaded and the limiter is bypassed, so "off" has to
    /// mean bit-for-bit off — not "nearly", which on a filter is a tone change
    /// nobody asked for and on a limiter is level.
    #[test]
    fn the_filters_and_the_limiter_do_nothing_at_zero() {
        let mut fx = Effects::new(SR);
        let p = Params::default();
        let sends = Sends {
            reverb: 0.5,
            ..Sends::default()
        };
        let mut buf = sine(512, 440.0, 0.5, 0);
        let before = buf.clone();
        fx.process(&mut buf, 512, 2, sends, &p, 120.0);
        // The reverb is on, so the buffer changed - but the change is the
        // reverb's. With the reverb off too, nothing at all happens.
        let mut clean = Effects::new(SR);
        let mut buf2 = before.clone();
        clean.process(&mut buf2, 512, 2, Sends::default(), &p, 120.0);
        assert_eq!(buf2, before, "every knob at zero is not a no-op");
    }

    /// The three wet sends, with the filters and limiter out of the way.
    fn sends(reverb: f32, delay: f32, chorus: f32) -> Sends {
        Sends {
            reverb,
            delay,
            chorus,
            ..Sends::default()
        }
    }

    /// One impulse in, `frames` of output, at 120 bpm with default settings.
    fn strike(fx: &mut Effects, frames: usize, s: Sends) -> Vec<f32> {
        strike_with(fx, frames, s, &Params::default(), 120.0)
    }

    /// As [`strike`], at a stated tempo and settings. **The tempo has to be the
    /// same one the warm-up used**: the delay time slews, so a different bpm
    /// here starts it moving back and the repeat arrives somewhere between.
    fn strike_with(fx: &mut Effects, frames: usize, s: Sends, p: &Params, bpm: f64) -> Vec<f32> {
        let mut out = vec![0.0_f32; frames * 2];
        out[0] = 1.0;
        out[1] = 1.0;
        fx.process(&mut out, frames, 2, s, p, bpm);
        out
    }

    fn peak(v: &[f32]) -> f32 {
        v.iter().fold(0.0_f32, |m, x| m.max(x.abs()))
    }

    /// **Off has to be silent, and it has to be free.** The default state of
    /// all three knobs, and if a buffer comes back changed with every one at
    /// zero then every user is paying for effects they did not ask for.
    #[test]
    fn every_knob_at_zero_does_not_touch_the_signal() {
        let mut fx = Effects::new(SR);
        let mut buf: Vec<f32> = (0..512).map(|i| (i as f32 * 0.01).sin()).collect();
        let before = buf.clone();
        fx.process(
            &mut buf,
            256,
            2,
            Sends::default(),
            &Params::default(),
            120.0,
        );
        assert_eq!(buf, before, "an effect at zero changed the signal");
    }

    /// Turning reverb up puts energy where there was none, and it decays.
    #[test]
    fn reverb_makes_a_tail_that_dies() {
        let mut fx = Effects::new(SR);
        let out = strike(&mut fx, 24_000, sends(1.0, 0.0, 0.0));
        // Well past the impulse, so this is the room and not the input.
        let early = peak(&out[2_000 * 2..6_000 * 2]);
        let late = peak(&out[18_000 * 2..]);
        assert!(early > 1.0e-4, "there was no tail at all: {early}");
        assert!(late < early, "the tail is not decaying: {early} -> {late}");

        // Stereo: the two channels must not be the same signal, or the spread
        // is doing nothing and the reverb is mono.
        let (l, r): (Vec<f32>, Vec<f32>) = out
            .chunks_exact(2)
            .skip(3_000)
            .map(|f| (f[0], f[1]))
            .unzip();
        let diff = l
            .iter()
            .zip(&r)
            .fold(0.0_f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(
            diff > 1.0e-5,
            "both channels are identical: the reverb is mono"
        );
    }

    /// **Size is a real control.** A bigger room rings longer, or the menu is
    /// a row of numbers that do nothing.
    #[test]
    fn a_bigger_room_rings_longer() {
        let tail = |size: f32| {
            let mut fx = Effects::new(SR);
            let p = Params {
                reverb_size: size,
                ..Params::default()
            };
            let out = strike_with(&mut fx, 48_000, sends(1.0, 0.0, 0.0), &p, 120.0);
            peak(&out[36_000 * 2..])
        };
        let (small, large) = (tail(0.0), tail(1.0));
        assert!(
            large > small * 4.0,
            "a room at full size rings {large:e} against {small:e} at none"
        );
    }

    /// **Size changes how long, not how loud.** A comb's gain rises with its
    /// feedback, so an unnormalised size control is also a volume control —
    /// and somebody reaching for a longer tail turns the send back down to
    /// compensate, leaving two controls fighting over one thing.
    #[test]
    fn size_does_not_change_how_loud_the_reverb_is() {
        // **Energy over the whole tail**, not the peak just after the strike.
        // A bigger hall spreads the same energy over more time, so its
        // moment-to-moment level is genuinely lower and a peak measurement
        // would be asking for the wrong thing to be equal.
        let energy = |size: f32| {
            let mut fx = Effects::new(SR);
            let p = Params {
                reverb_size: size,
                ..Params::default()
            };
            let out = strike_with(&mut fx, 96_000, sends(1.0, 0.0, 0.0), &p, 120.0);
            (out.iter()
                .map(|s| f64::from(*s) * f64::from(*s))
                .sum::<f64>())
            .sqrt()
        };
        let (small, large) = (energy(0.0), energy(1.0));
        assert!(
            large < small * 2.0 && small < large * 2.0,
            "the smallest room carries {small:e} and the largest {large:e}: \
             size is a volume control"
        );
    }

    /// **The plate's own signature: the bottom of the band arrives last.**
    ///
    /// Bending waves in a thin plate are dispersive — group velocity on the
    /// real EMT 140 runs from about 20 m/s at 20 Hz to 628 m/s at 20 kHz, so
    /// measured at one pickup the top of the band lands after a millisecond
    /// and the bottom after thirty. That is the bright "pew" on the front of
    /// every hit and it is the one thing a room cannot do.
    ///
    /// Tested on [`Dispersion`] alone rather than through the whole plate: the
    /// tank's diffusion smears arrival times on purpose, so measuring the
    /// chirp downstream of it would be measuring the diffuser. The centre of
    /// mass of the impulse response is the honest measure of "when did this
    /// band arrive" for a filter that changes no level anywhere.
    #[test]
    fn the_plate_holds_the_bottom_of_the_band_back() {
        const N: usize = 4096;
        let mut d = Dispersion::new();
        let mut h = vec![0.0_f32; N];
        h[0] = 1.0;
        for v in &mut h {
            *v = d.process(*v, 1.0);
        }

        // Where the energy of one band sits in time, in samples. A one-pole
        // each way is enough: this is asking which of two broad regions of the
        // spectrum arrives first, not drawing a group-delay curve.
        let centroid = |cut: f32, high: bool| {
            let a = (-std::f32::consts::TAU * cut / SR).exp();
            let (mut z, mut num, mut den) = (0.0_f32, 0.0_f64, 0.0_f64);
            for (n, x) in h.iter().enumerate() {
                z = x * (1.0 - a) + z * a;
                let y = if high { x - z } else { z };
                let e = f64::from(y) * f64::from(y);
                num += e * n as f64;
                den += e;
            }
            num / den.max(1.0e-30)
        };
        let low = centroid(200.0, false);
        let high = centroid(4_000.0, true);

        assert!(
            low > high * 3.0,
            "the low band's energy sits at {low:.0} samples and the high at \
             {high:.0}: there is no chirp, which means there is no plate"
        );
        // And it is a real amount of time, not a rounding error. The real
        // plate spreads about 29 ms across the band; one pass of this chain
        // carries a fraction of that and the tank's recirculation does the
        // rest, so the floor here is deliberately modest.
        let spread_ms = (low - high) * 1_000.0 / f64::from(SR);
        assert!(
            spread_ms > 1.0,
            "only {spread_ms:.2} ms between the bottom of the band and the top"
        );
    }

    /// **An all-pass changes when, never how loud.** Dispersion that also
    /// filtered would be a tone control wearing a physics hat — and the plate
    /// does not lose level on its way across, it only rearranges arrival.
    #[test]
    fn the_dispersion_is_flat() {
        for hz in [110.0_f32, 440.0, 1_760.0, 7_040.0] {
            let mut d = Dispersion::new();
            let frames = 24_000;
            // Well past the chain's own delay, so this is the steady state and
            // not the onset.
            let mut peak = 0.0_f32;
            for n in 0..frames {
                let x = (std::f32::consts::TAU * hz * n as f32 / SR).sin();
                let y = d.process(x, 1.0);
                if n > frames / 2 {
                    peak = peak.max(y.abs());
                }
            }
            assert!(
                (peak - 1.0).abs() < 0.02,
                "a {hz} Hz tone came out at {peak:.3} instead of unity: the \
                 dispersion chain is filtering"
            );
        }
    }

    /// **The decay dial reads in seconds, and it is telling the truth.**
    ///
    /// The EMT's remote was marked 0.5 to 5.5 seconds because the damper
    /// position really did set a reverberation time, and a dial that says
    /// seconds while meaning "some coefficient" is a dial that lies every time
    /// somebody matches a plate to a tempo. [`Plate::feedback`] solves for the
    /// coefficient rather than mapping a knob onto one by ear; this is what
    /// checks that the solution is right in the tank it was solved for.
    #[test]
    fn the_decay_dial_is_a_time_in_seconds() {
        // **Measured as a slope, not as a threshold crossing.** A tail is
        // noisy sample to sample; the fall between two windows a known
        // distance apart averages that away and extrapolates to sixty decibels
        // without needing the tail to actually get there.
        let measured = |size: f32| {
            let mut fx = Effects::new_send(SR);
            let p = Params {
                reverb_size: size,
                ..Params::default()
            };
            let out = strike_with(&mut fx, 192_000, sends(1.0, 0.0, 0.0), &p, 120.0);
            let rms = |from: usize, to: usize| {
                let s: f64 = out[from * 2..to * 2]
                    .iter()
                    .map(|v| f64::from(*v) * f64::from(*v))
                    .sum();
                (s / ((to - from) * 2) as f64).sqrt()
            };
            // Both windows well after the build-up and well before the buffer
            // runs out, half a second apart.
            let a = rms(9_600, 19_200);
            let b = rms(33_600, 43_200);
            let db = 20.0 * (a / b.max(1.0e-30)).log10();
            // Sixty decibels at this rate of fall.
            60.0 * 0.5 / db
        };

        for size in [0.0_f32, 0.34, 1.0] {
            let want = f64::from(Plate::decay_secs(size));
            let got = measured(size);
            assert!(
                got > want * 0.6 && got < want * 1.7,
                "the dial asked for {want:.2} s of decay and the plate rang \
                 for {got:.2} s"
            );
        }
    }

    /// **The plate has no predelay, so the control has to supply it.**
    ///
    /// A real plate starts the instant the coil moves — there is no distance
    /// for sound to cross and no early reflection pattern — which is why every
    /// studio that wanted air in front of theirs patched a tape machine in.
    /// This checks that turning it up really is silence and not a fade.
    #[test]
    fn the_predelay_is_silence_in_front_of_the_plate() {
        let p = Params {
            plate_predelay: 1.0,
            ..Params::default()
        };
        let mut fx = Effects::new_send(SR);
        let out = strike_with(&mut fx, 48_000, sends(1.0, 0.0, 0.0), &p, 120.0);

        // 150 ms at the top of the sweep; measure comfortably inside it.
        let quiet = peak(&out[..(SR as usize / 10) * 2]);
        let ringing = peak(&out[(SR as usize / 5) * 2..]);
        assert!(
            quiet < 1.0e-6,
            "something arrived {quiet:e} inside the predelay window"
        );
        assert!(
            ringing > 1.0e-4,
            "nothing arrived after the predelay either: {ringing:e}"
        );
    }

    /// **The delay lands on the beat**, and the division says where. The whole
    /// reason the time is not a knob: at 120 bpm a dotted eighth is 375 ms, and
    /// a repeat anywhere else is a mistake somebody has to hear to find.
    #[test]
    fn the_delay_follows_the_tempo_and_the_division() {
        let cases = [
            (120.0_f64, Division::DottedEighth, 0.375_f32),
            (90.0, Division::DottedEighth, 0.5),
            (120.0, Division::Quarter, 0.5),
            (120.0, Division::Eighth, 0.25),
            (120.0, Division::Sixteenth, 0.125),
            // Free ignores the tempo entirely, which is the point of it.
            (200.0, Division::Free, FREE_SECS),
        ];
        for (bpm, division, want) in cases {
            let p = Params {
                delay_division: division,
                ..Params::default()
            };
            let mut fx = Effects::new(SR);
            // Long enough for the slew to arrive before the impulse is sent.
            let mut warm = vec![0.0_f32; 2 * 96_000];
            fx.process(&mut warm, 96_000, 2, sends(0.0, 1.0, 0.0), &p, bpm);

            let out = strike_with(&mut fx, 48_000, sends(0.0, 1.0, 0.0), &p, bpm);
            // The loudest thing after the impulse itself is the first repeat.
            let left: Vec<f32> = out.chunks_exact(2).map(|f| f[0].abs()).collect();
            let (at, _) =
                left.iter()
                    .enumerate()
                    .skip(64)
                    .fold(
                        (0usize, 0.0_f32),
                        |(bi, bv), (i, v)| {
                            if *v > bv {
                                (i, *v)
                            } else {
                                (bi, bv)
                            }
                        },
                    );
            let got = at as f32 / SR;
            assert!(
                (got - want).abs() < 0.02,
                "{} at {bpm} bpm landed at {got:.3}s, not {want:.3}s",
                division.label()
            );
        }
    }

    /// Repeats get quieter, or the knob is an oscillator — at every feedback
    /// setting the menu can reach.
    #[test]
    fn the_delay_does_not_run_away_at_any_setting() {
        for feedback in [0.0_f32, 0.5, 1.0] {
            let p = Params {
                delay_feedback: feedback,
                ..Params::default()
            };
            let mut fx = Effects::new(SR);
            let out = strike_with(&mut fx, 48_000 * 6, sends(0.0, 1.0, 0.0), &p, 120.0);
            let first = peak(&out[..48_000]);
            let last = peak(&out[48_000 * 10..]);
            assert!(
                last < first * 0.35,
                "feedback {feedback} went {first:e} -> {last:e} after five seconds"
            );
        }
    }

    /// Where the repeats land, in seconds, for one mode. Feedback off, so
    /// what comes back is the heads and not a train of them.
    fn head_arrivals(mode: EchoMode, frames: usize) -> Vec<f32> {
        let p = Params {
            echo_mode: mode,
            delay_feedback: 0.0,
            // Mono: the heads pan apart at the shipped width, and a test that
            // measured one channel would find head three quiet for a reason
            // that has nothing to do with where it is.
            delay_width: 0.0,
            ..Params::default()
        };
        let mut fx = Effects::new_send(SR);
        let mut warm = vec![0.0_f32; 2 * 96_000];
        fx.process(&mut warm, 96_000, 2, sends(0.0, 1.0, 0.0), &p, 120.0);
        let out = strike_with(&mut fx, frames, sends(0.0, 1.0, 0.0), &p, 120.0);

        let mag: Vec<f32> = out.chunks_exact(2).map(|f| f[0].abs()).collect();
        let ceiling = mag.iter().skip(64).fold(0.0_f32, |m, v| m.max(*v));
        // Local maxima that are a real fraction of the loudest arrival, with a
        // guard so one repeat cannot be counted twice.
        let mut hits = Vec::new();
        let mut last = 0usize;
        for i in 64..mag.len() - 1 {
            if mag[i] > ceiling * 0.25 && mag[i] >= mag[i - 1] && mag[i] > mag[i + 1] {
                if i - last > 2_000 {
                    hits.push(i as f32 / SR);
                }
                last = i;
            }
        }
        hits
    }

    /// **The three heads are not evenly spaced, and the ratios are the sound.**
    ///
    /// They were meant to be 1 : 2 : 3 and they are not — measurements on real
    /// machines put them nearer 1 : 1.94 : 2.85, because the spacing was never
    /// exact and the tape path was never quite straight. An emulation on the
    /// round numbers is tidier than any RE-201 ever was, and the tidiness is
    /// the thing people hear as wrong.
    #[test]
    fn the_three_heads_sit_where_the_machine_puts_them() {
        let hits = head_arrivals(EchoMode::Eleven, 96_000);
        assert!(
            hits.len() >= 3,
            "wanted three arrivals from all three heads, found {hits:?}"
        );
        let first = hits[0];
        for (i, want) in HEAD_RATIO.iter().enumerate() {
            let got = hits[i] / first;
            assert!(
                (got - want).abs() < 0.06,
                "head {} landed at {got:.3} times the first, wanted {want:.3} \
                 (arrivals {hits:?})",
                i + 1
            );
        }
    }

    /// **Every position of the Mode Selector is a different machine.**
    ///
    /// Roland's own table, checked one position at a time: the right number of
    /// arrivals, at the right multiples of head one. A switch whose twelve
    /// positions were not twelve different things would be a switch nobody
    /// needed, and this is the only part of the emulation where being wrong is
    /// silent — a mode that quietly reads the wrong head still makes a repeat.
    #[test]
    fn the_mode_selector_switches_the_heads_roland_says_it_does() {
        // Head one's own time, from the mode that can only be head one.
        let unit = head_arrivals(EchoMode::One, 96_000)[0];
        for mode in EchoMode::ALL {
            let live: Vec<usize> = mode
                .heads()
                .iter()
                .enumerate()
                .filter(|(_, on)| **on)
                .map(|(i, _)| i)
                .collect();
            if live.is_empty() {
                // Reverb only: the tape is out of circuit entirely.
                let hits = head_arrivals(mode, 96_000);
                assert!(
                    hits.is_empty() || hits.iter().all(|t| *t < unit * 0.5),
                    "{} produced tape repeats at {hits:?}",
                    mode.label()
                );
                continue;
            }
            let hits = head_arrivals(mode, 144_000);
            assert_eq!(
                hits.len(),
                live.len(),
                "{} should sound {} head(s), found arrivals at {hits:?}",
                mode.label(),
                live.len()
            );
            for (hit, head) in hits.iter().zip(&live) {
                let got = hit / unit;
                let want = HEAD_RATIO[*head];
                assert!(
                    (got - want).abs() < 0.08,
                    "{} put head {} at {got:.3} units, wanted {want:.3}",
                    mode.label(),
                    head + 1
                );
            }
        }
    }

    /// **The spring is in seven of the twelve, and only those seven.**
    ///
    /// Positions one to four are the tape alone and twelve is the spring
    /// alone. Getting this backwards is inaudible in the code and obvious in
    /// the room — and `reverb()` is one `matches!` away from covering the
    /// wrong four.
    #[test]
    fn the_spring_is_in_circuit_exactly_where_the_table_says() {
        // Between head one's time and the shortest repeat: anything here came
        // from the spring, because no head reads this early.
        let spring_energy = |mode: EchoMode| {
            let p = Params {
                echo_mode: mode,
                delay_feedback: 0.0,
                ..Params::default()
            };
            let mut fx = Effects::new_send(SR);
            let out = strike_with(&mut fx, 12_000, sends(0.0, 1.0, 0.0), &p, 120.0);
            // Head one is at 375 ms; 12 000 frames is 250 ms, so nothing the
            // tape makes can be in this window at all.
            peak(&out)
        };
        for mode in EchoMode::ALL {
            let e = spring_energy(mode);
            if mode.reverb() {
                assert!(
                    e > 1.0e-5,
                    "{} is supposed to have the spring in it and made {e:e}",
                    mode.label()
                );
            } else {
                assert!(
                    e < 1.0e-6,
                    "{} is tape only and something rang at {e:e}",
                    mode.label()
                );
            }
        }
    }

    /// **Every repeat is older than the one before it.**
    ///
    /// Head gap loss, spacing loss and the tape's own self-erasure take the
    /// top off on every trip round the loop, which is the whole reason a tape
    /// echo sits under a piano where a clean digital delay competes with it.
    /// Measured as a spectral centroid rather than a level: the repeats get
    /// quieter too, and quieter is not the claim.
    #[test]
    fn each_repeat_is_darker_than_the_one_before() {
        let p = Params {
            delay_feedback: 0.85,
            delay_width: 0.0,
            ..Params::default()
        };
        let mut fx = Effects::new_send(SR);
        let mut warm = vec![0.0_f32; 2 * 96_000];
        fx.process(&mut warm, 96_000, 2, sends(0.0, 1.0, 0.0), &p, 120.0);
        let out = strike_with(&mut fx, 96_000, sends(0.0, 1.0, 0.0), &p, 120.0);

        // How much of a window's energy is above a few kilohertz, as a share
        // of all of it. A one-pole split is plenty to answer "is this darker".
        let brightness = |from: usize, to: usize| {
            let a = (-std::f32::consts::TAU * 3_000.0 / SR).exp();
            let (mut z, mut hi, mut all) = (0.0_f32, 0.0_f64, 0.0_f64);
            for f in out[from * 2..to * 2].chunks_exact(2) {
                z = f[0] * (1.0 - a) + z * a;
                let h = f64::from(f[0] - z);
                hi += h * h;
                all += f64::from(f[0]) * f64::from(f[0]);
            }
            hi / all.max(1.0e-30)
        };

        // 375 ms a repeat at 120 bpm: the first around 18 000 frames, the
        // fourth around 72 000.
        let first = brightness(16_000, 20_000);
        let fourth = brightness(70_000, 74_000);
        assert!(
            fourth < first * 0.8,
            "the first repeat was {first:.4} bright and the fourth {fourth:.4}: \
             the tape is not losing anything"
        );
    }

    /// **Tape compresses; it does not clip.** Twenty decibels more going in
    /// has to come out as less than twenty more, or the record amplifier is a
    /// wire and the machine is a digital delay with a filter on it.
    #[test]
    fn the_tape_saturates_rather_than_clipping() {
        let level = |amp: f32| {
            let p = Params {
                delay_feedback: 0.2,
                delay_width: 0.0,
                ..Params::default()
            };
            let mut fx = Effects::new_send(SR);
            let mut warm = vec![0.0_f32; 2 * 96_000];
            fx.process(&mut warm, 96_000, 2, sends(0.0, 1.0, 0.0), &p, 120.0);
            let mut out = vec![0.0_f32; 48_000 * 2];
            out[0] = amp;
            out[1] = amp;
            fx.process(&mut out, 48_000, 2, sends(0.0, 1.0, 0.0), &p, 120.0);
            peak(&out)
        };
        let quiet = level(0.05);
        let loud = level(0.5);
        let ratio = loud / quiet.max(1.0e-12);
        assert!(
            ratio < 9.0,
            "ten times the input came back {ratio:.2} times as loud: nothing \
             is saturating"
        );
        assert!(
            ratio > 2.0,
            "ten times the input came back only {ratio:.2} times as loud: that \
             is not compression, it is a brick wall"
        );
    }

    /// **The transport warbles, and that is the chorus everybody means.**
    ///
    /// Roland attribute the RE-201's "pleasing chorus effect" to mechanical
    /// friction and tape slip rather than to anything they designed in. Wow
    /// under 4 Hz and flutter above it move the heads, which moves the pitch —
    /// so a held tone comes back with its own beat against itself.
    #[test]
    fn the_tape_warbles() {
        let p = Params {
            delay_feedback: 0.0,
            delay_width: 0.0,
            ..Params::default()
        };
        let mut fx = Effects::new_send(SR);
        let frames = 96_000;
        let mut buf = vec![0.0_f32; frames * 2];
        for (i, f) in buf.chunks_exact_mut(2).enumerate() {
            let v = (i as f32 * 440.0 * std::f32::consts::TAU / SR).sin() * 0.5;
            f[0] = v;
            f[1] = v;
        }
        fx.process(&mut buf, frames, 2, sends(0.0, 1.0, 0.0), &p, 120.0);

        // **Measured as phase, not as level**, and the first version of this
        // test got that wrong: moving a delay line moves the PITCH of what is
        // coming out of it and leaves the amplitude alone, so an envelope
        // measurement of a warbling sine reads dead flat. It did — 0.41497
        // against 0.41498 — and said the tape was running perfectly.
        //
        // Timing the upward zero crossings against a perfectly regular grid
        // integrates the frequency error instead of sampling it, so a
        // fraction of a percent of speed variation shows up as whole samples
        // of drift.
        let y: Vec<f32> = buf.chunks_exact(2).map(|f| f[0]).collect();
        let mut cross = Vec::new();
        for i in 24_000..y.len() - 1 {
            if y[i] <= 0.0 && y[i + 1] > 0.0 {
                // Linear interpolation, so this is sub-sample and the
                // measurement is not quantised to the thing being measured.
                let t = f64::from(-y[i]) / f64::from(y[i + 1] - y[i]);
                cross.push(i as f64 + t);
            }
        }
        assert!(cross.len() > 200, "not enough tone came back to time");

        // Straight line through the first and last crossing: whatever is left
        // is the transport moving rather than the tone being at a slightly
        // different pitch than assumed.
        let (t0, t1) = (cross[0], cross[cross.len() - 1]);
        let step = (t1 - t0) / (cross.len() - 1) as f64;
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for (k, t) in cross.iter().enumerate() {
            let d = t - (t0 + step * k as f64);
            lo = lo.min(d);
            hi = hi.max(d);
        }
        let spread = hi - lo;
        assert!(
            spread > 1.0,
            "the crossings drifted by {spread:.3} samples against a regular \
             grid: the tape is running perfectly, which no tape does"
        );
    }

    /// A mode round-trips through the string the settings file stores.
    #[test]
    fn a_mode_round_trips_through_its_key() {
        for mode in EchoMode::ALL {
            assert_eq!(EchoMode::from_key(mode.key()), Some(mode), "{mode:?}");
        }
        assert_eq!(EchoMode::from_key("nonsense"), None);
    }

    /// **The chorus is wide, and the width control is what makes it wide.**
    /// The CE-1's trick is one delay line inverted into the right channel; at
    /// full width the two outputs must be opposite, and at zero they must not.
    #[test]
    fn the_chorus_inverts_one_channel_at_full_width() {
        let difference = |width: f32| {
            let mut fx = Effects::new(SR);
            let p = Params {
                chorus_width: width,
                ..Params::default()
            };
            // A tone rather than an impulse: a chorus is heard on sustained
            // sound and an impulse leaves the delay line almost empty.
            let frames = 24_000;
            let mut buf = vec![0.0_f32; frames * 2];
            for (i, f) in buf.chunks_exact_mut(2).enumerate() {
                let v = (i as f32 * 220.0 * std::f32::consts::TAU / SR).sin() * 0.5;
                f[0] = v;
                f[1] = v;
            }
            fx.process(&mut buf, frames, 2, sends(0.0, 0.0, 1.0), &p, 120.0);
            // How far the two channels have been pushed apart. The input was
            // identical in both, so all of this is the effect.
            buf.chunks_exact(2)
                .skip(12_000)
                .fold(0.0_f32, |m, f| m.max((f[0] - f[1]).abs()))
        };
        let (narrow, wide) = (difference(0.0), difference(1.0));
        assert!(
            narrow < 1.0e-6,
            "at zero width the channels differ by {narrow}, so it is not mono-safe"
        );
        assert!(
            wide > 0.05,
            "at full width the channels differ by only {wide}, so the inversion is not happening"
        );
    }

    /// A chorus has to MOVE. A fixed delay is a comb filter, which is a tone
    /// change and not a chorus, and it is what a broken LFO sounds like.
    #[test]
    fn the_chorus_sweeps() {
        let mut fx = Effects::new(SR);
        let p = Params {
            chorus_rate: 1.0,
            chorus_depth: 1.0,
            ..Params::default()
        };
        let frames = 48_000;
        let mut buf = vec![0.0_f32; frames * 2];
        for (i, f) in buf.chunks_exact_mut(2).enumerate() {
            let v = (i as f32 * 440.0 * std::f32::consts::TAU / SR).sin() * 0.5;
            f[0] = v;
            f[1] = v;
        }
        fx.process(&mut buf, frames, 2, sends(0.0, 0.0, 1.0), &p, 120.0);
        let out: Vec<f32> = buf.chunks_exact(2).map(|f| f[0]).collect();

        // **The OUTPUT's envelope**, not the wet signal's amplitude. The wet is
        // a delayed copy of the same sine and its amplitude never changes,
        // whatever the sweep is doing; what moves is where dry and wet cancel.
        // A stuck sweep is a fixed comb, and a fixed comb has a fixed envelope.
        //
        // **Short windows.** Three periods of the tone, not thirty: at full
        // rate the sweep carries the comb through most of a cycle inside a long
        // window, so every long window finds a constructive moment and the
        // envelope looks flat when it is anything but.
        let mut lo = f32::MAX;
        let mut hi = 0.0_f32;
        for w in (8_000..44_000).step_by(300) {
            let d = peak(&out[w..w + 300]);
            lo = lo.min(d);
            hi = hi.max(d);
        }
        assert!(
            hi > lo * 1.4,
            "the sweep is not moving: the envelope sat between {lo:e} and {hi:e}"
        );
    }

    /// **Nothing rings on from an hour ago.** Turning the knobs down flushes
    /// the tails, so turning them back up starts from silence rather than
    /// replaying whatever was in the buffers.
    #[test]
    fn turning_everything_off_flushes_what_was_ringing() {
        let mut fx = Effects::new(SR);
        strike(&mut fx, 8_000, sends(1.0, 1.0, 1.0));
        // Down, and long enough for the mixes to slew to zero.
        let mut off = vec![0.0_f32; 2 * 48_000 * 2];
        fx.process(
            &mut off,
            48_000 * 2,
            2,
            Sends::default(),
            &Params::default(),
            120.0,
        );
        // Up again, into silence.
        let mut back = vec![0.0_f32; 2 * 4_000];
        fx.process(
            &mut back,
            4_000,
            2,
            sends(1.0, 1.0, 1.0),
            &Params::default(),
            120.0,
        );
        assert!(
            peak(&back) < 1.0e-6,
            "an old tail came back when the knob did: {}",
            peak(&back)
        );
    }

    /// A room at 96 kHz is the same room, not one half as long.
    #[test]
    fn the_room_is_the_same_size_at_every_sample_rate() {
        let tail_secs = |sr: f32| {
            let mut fx = Effects::new(sr);
            let frames = (sr * 2.0) as usize;
            let out = strike(&mut fx, frames, sends(1.0, 0.0, 0.0));
            let left: Vec<f32> = out.chunks_exact(2).map(|f| f[0].abs()).collect();
            let peak = left.iter().skip(1000).fold(0.0_f32, |m, v| m.max(*v));
            // Where it falls to a thousandth of its loudest.
            left.iter().rposition(|v| *v > peak * 0.001).unwrap_or(0) as f32 / sr
        };
        let (a, b) = (tail_secs(44_100.0), tail_secs(96_000.0));
        assert!(
            (a - b).abs() < a * 0.2,
            "the tail is {a:.2}s at 44.1k and {b:.2}s at 96k"
        );
    }

    /// Mono and odd channel counts are left alone rather than half-processed.
    #[test]
    fn a_mono_buffer_is_not_mangled() {
        let mut fx = Effects::new(SR);
        let mut buf: Vec<f32> = (0..256).map(|i| (i as f32 * 0.02).sin()).collect();
        let before = buf.clone();
        fx.process(
            &mut buf,
            256,
            1,
            sends(1.0, 1.0, 1.0),
            &Params::default(),
            120.0,
        );
        assert_eq!(buf, before, "a mono buffer was written to");
    }

    /// **Nothing a menu can reach may blow up.** Every parameter at both
    /// extremes, all three sends open, against a signal loud enough to be
    /// unkind: the output has to stay finite and bounded.
    #[test]
    fn no_setting_a_menu_can_reach_makes_it_explode() {
        let extremes = [0.0_f32, 1.0];
        for a in extremes {
            for b in extremes {
                for division in Division::ALL {
                    let p = Params {
                        reverb_size: a,
                        reverb_damp: b,
                        reverb_width: a,
                        plate_dispersion: b,
                        plate_predelay: a,
                        delay_division: division,
                        echo_mode: EchoMode::Eleven,
                        delay_feedback: a,
                        delay_tone: b,
                        delay_width: b,
                        chorus_rate: a,
                        chorus_depth: b,
                        chorus_width: a,
                        chorus_tone: b,
                        phaser_rate: a,
                        phaser_feedback: b,
                        phaser_stages: Stages::Eight,
                        phaser_range: a,
                        hpf_slope: Slope::TwentyFour,
                        hpf_resonance: b,
                        lpf_slope: Slope::TwentyFour,
                        lpf_resonance: a,
                        limiter_release: b,
                        limiter_knee: a,
                    };
                    let mut fx = Effects::new(SR);
                    let frames = 24_000;
                    // **Fresh input each block**, as the renderer hands it
                    // over. Re-processing the same buffer would be a feedback
                    // loop of the test's own making, and would fail for a
                    // reason that says nothing about the effects.
                    let mut phase = 0.0_f32;
                    for _ in 0..8 {
                        let mut buf = vec![0.0_f32; frames * 2];
                        for f in buf.chunks_exact_mut(2) {
                            phase += 110.0 * std::f32::consts::TAU / SR;
                            let v = phase.sin();
                            f[0] = v;
                            f[1] = -v;
                        }
                        fx.process(&mut buf, frames, 2, sends(1.0, 1.0, 1.0), &p, 120.0);
                        assert!(
                            buf.iter().all(|s| s.is_finite()),
                            "{} produced a non-finite sample",
                            division.label()
                        );
                        // Generous, because everything open on a full-scale
                        // input is not a mix anybody would use. What this
                        // catches is a runaway, not a hot setting.
                        assert!(
                            peak(&buf) < 12.0,
                            "{} reached {} with everything open",
                            division.label(),
                            peak(&buf)
                        );
                    }
                }
            }
        }
    }

    /// Rubbish in the settings file does not become rubbish in a feedback loop.
    #[test]
    fn parameters_are_clamped_on_the_way_in() {
        let p = Params {
            reverb_size: f32::NAN,
            delay_feedback: 40.0,
            chorus_depth: -3.0,
            ..Params::default()
        }
        .sane();
        assert!(p.reverb_size.is_finite());
        assert!((0.0..=1.0).contains(&p.delay_feedback));
        assert!((0.0..=1.0).contains(&p.chorus_depth));
    }

    /// Every division survives a trip through the settings file.
    #[test]
    fn a_division_round_trips_through_its_key() {
        for d in Division::ALL {
            assert_eq!(Division::from_key(d.key()), Some(d), "{}", d.label());
            assert!(!d.label().is_empty());
        }
        assert_eq!(Division::from_key("nonsense"), None);
    }
}
