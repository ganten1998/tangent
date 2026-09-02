//! The noise-reduction kernel for Tangent Denoise: a learned bank of
//! band-scoped downward expanders, built as a dynamic EQ.
//!
//! # The shape
//!
//! One signal path through a cascade of 32 minimum-phase peaking-cut biquads,
//! log-spaced from 60 Hz to 16 kHz, whose cut depths are the outputs of 32
//! downward expanders. A band whose depth is 0 dB is not "a biquad at unity"
//! — it is skipped entirely, so the all-gains-zero state is bit-transparent
//! and survives a null test against the dry track. This is the dynamic-EQ
//! formulation rather than a crossover tree or an FFT, which buys the honest
//! marketing line: **no spectral processing, no musical noise, no pre-echo;
//! phase moves only in bands being attenuated, exactly as an EQ cut would.**
//! (Not "zero phase alteration" — a minimum-phase cut always moves phase
//! where it cuts. Nothing else can be claimed and nothing else is.)
//!
//! Detection never touches the audio: a parallel sidechain of 32 band
//! followers — a PAIR of cascaded complex one-pole resonators each, eight
//! multiplies a band — measures per-band energy. One pole alone was tried
//! and rejected: its 6 dB/oct skirts let a −20 dBFS tone at 1 kHz leak
//! into the 60 Hz detector above its threshold, and one loud tone anywhere
//! opened every band. The pair's 12 dB/oct skirts keep distant bands deaf
//! to it while staying deliberately WIDE nearby (Q 2 against the cut
//! bells' 5.5): a tone leaking into its neighbours' detectors opens those
//! bands too, so the bells that keep cutting sit far enough away that
//! their skirts barely graze the tone. Masking does the rest — noise
//! under a band that a signal has opened is noise the signal is covering.
//!
//! # The dynamics, as a system
//!
//! Per band: soft-knee downward expansion (ratio 1.5:1 to 3:1 on the SMOOTH
//! dial — gate slopes are what chatter), depth capped by the REDUCE dial
//! (0..24 dB, ~12 by default — past 12–15 dB any expander bank starts to
//! sound processed), hysteresis with open and close thresholds 4 dB apart
//! plus a 40 ms minimum hold (a band hovering at its threshold must not
//! flap), envelope and depth smoothing in the log domain, and a
//! program-dependent release: right after a transient the cut re-engages
//! several times faster, because the hit masked the noise and the gap behind
//! it should not hiss.
//!
//! Then, every control tick, the depths are smoothed ACROSS the bank:
//! adjacent bands may not differ by more than 6 dB. Two bells at very
//! different depths beat against each other in the overlap region between
//! their centres; CEDAR-style designs constrain the slope of the gain curve
//! across frequency for exactly this reason. Reduction is only ever lowered
//! to satisfy the constraint, never raised — over-cutting signal to honour a
//! curve would be backwards.
//!
//! # Learn
//!
//! While the `learn` toggle is up, the kernel applies no reduction and
//! accumulates per-band statistics of what it hears — the user plays it
//! isolated noise. On the falling edge each band's threshold freezes at
//! mean + 2σ of its log energy, NOT at the mean: a short noise sample's
//! mean underestimates its excursions, and a threshold at the mean would
//! leave half the noise poking through. The thresholds are ordinary state —
//! [`Denoise::thresholds`] and [`Denoise::set_thresholds`] move all 32 as
//! normalized values so a session can persist them as symbols.
//!
//! # Lookahead
//!
//! 3 ms, a toggle, on by default: the DELAY sits on the signal path only,
//! so the detector effectively reads the future and a band is open by the
//! time a transient reaches the bells. [`Denoise::latency`] reports the
//! honest sample count for the current setting — 0 with the toggle down.
//!
//! # Sample rate
//!
//! Taken once, in `new`. Band centres, resonator bandwidths, every time
//! constant and the control-tick length itself are derived from hertz and
//! milliseconds, so the kernel is rate-invariant by construction — and a
//! test holds it to that at 44.1k, 48k and 96k.

// Nothing reaches this module yet: the suite's engine lane wires it later,
// and proving the DSP before any panel exists is the point. Until then every
// item here is dead to rustc.
#![allow(dead_code)]

// ── what the dials map to ───────────────────────────────────────────────────

/// How many bands, which is also how many threshold symbols a session will
/// persist. 32 log-spaced bands is ~0.26 octave each — wide enough that a
/// band's gain moving reads as breathing rather than as a tone (musical
/// noise needs isolated-sinusoid-narrow bins), fine enough to take a hum
/// out without dimming the octave around it.
pub const DENOISE_BANDS: usize = 32;

/// Where the bands live, in Hz, log-spaced ends included. Below 60 Hz is
/// rumble a high-pass owns; above 16 kHz is a region where cutting "hiss"
/// mostly cuts air, and at 44.1k the top band still sits safely under
/// Nyquist.
pub const DENOISE_BAND_HZ: (f32, f32) = (60.0, 16_000.0);

/// The cut bells' Q. 5.5 puts each bell's bandwidth at about its own slot's
/// width (0.26 octave), so a run of closed bands sums to an even shelf of
/// reduction between centres instead of a comb of dips.
pub const DENOISE_BAND_Q: f32 = 5.5;

/// The detector resonators' Q — deliberately far wider than the bells (see
/// the module header): a tone must open its neighbourhood, not just its bin.
const DENOISE_DETECTOR_Q: f32 = 2.0;

/// The top of the REDUCE dial: the per-band depth cap, mapped linearly from
/// 0 dB (bottom of the travel is off) to 24 dB. Default sits at 12 — the
/// depth every published measurement of this architecture agrees is where
/// "quieter" starts becoming "processed".
pub const DENOISE_REDUCE_DB: f32 = 24.0;

/// What the SMOOTH dial spans: the expansion ratio, geometric. 1.5:1 at the
/// bottom is barely more than a wire below threshold; 3:1 at the top is as
/// steep as this design goes, because gate-like slopes are what turn a level
/// wobbling near threshold into flapping.
pub const DENOISE_RATIO: (f32, f32) = (1.5, 3.0);

/// What the RELEASE dial spans, in milliseconds, geometric: how long a cut
/// takes to re-engage once signal leaves a band. 60 ms breathes audibly on
/// speech pauses; 800 ms lets a whole bar of noise through after each note.
pub const DENOISE_RELEASE_MS: (f32, f32) = (60.0, 800.0);

/// Where the sidechain high-pass sweeps, in Hz, geometric; the bottom of
/// the dial is OFF. It blinds the DETECTOR only — an intentional bass part
/// should not hold the low bands open for the noise under it.
pub const DENOISE_SC_HPF_HZ: (f32, f32) = (20.0, 1_000.0);

/// The lookahead, in milliseconds, when its toggle is up. 3 ms is enough
/// for a band to be open before a transient reaches the bells; Brusfri
/// spends 20 ms on the same job and nothing in the physics needs that.
pub const DENOISE_LOOKAHEAD_MS: f32 = 3.0;

/// The span a normalized threshold maps across, in dBFS of band level.
/// The very bottom is OFF — a band whose threshold sits at 0.0 never
/// reduces, which is both the freshly-constructed state (nothing learned,
/// the unit is a wire) and what a band that learned pure silence becomes.
pub const DENOISE_THR_DB: (f32, f32) = (-100.0, 0.0);

/// Open and close thresholds sit this far apart: a band OPENS this far
/// above the learned threshold and closes back AT it. The classic
/// gate-chatter cure — a band that has opened must fall 4 dB below where
/// it opened before it may close — but the whole window sits above the
/// stored threshold rather than centred on it, because the threshold was
/// learned at the top of the noise's excursions (mean + 2σ) and the fast
/// gate envelope rides those tops: an open line any closer gets tripped
/// by the very noise the band exists to cut, and every false open zeroes
/// a cut that then takes a whole release time to rebuild.
pub const DENOISE_HYSTERESIS_DB: f32 = 4.0;

/// Once open, a band stays open at least this long. Decays and reverb tails
/// wobble across the threshold on their way down; 40 ms of hold turns what
/// would be a flutter into one clean close.
pub const DENOISE_HOLD_MS: f32 = 40.0;

/// The cross-band slope limit: adjacent bands' depths may not differ by
/// more than this, enforced every control tick by LOWERING the deeper band.
/// See the module header for why (band-edge beating).
pub const DENOISE_SLOPE_DB: f32 = 6.0;

/// Full width of the expander's soft knee, in dB, centred on the threshold.
/// 6 dB bends a level drifting across the threshold instead of grabbing it.
pub const DENOISE_KNEE_DB: f32 = 6.0;

/// How many standard deviations above its mean log energy a band's learned
/// threshold sits. The mean is the wrong statistic — a short learn pass
/// underestimates the noise's excursions, and everything the threshold
/// misses is noise that leaks — so the threshold tracks the top of the
/// band's distribution instead.
pub const DENOISE_LEARN_SIGMA: f32 = 2.0;

/// Fixed headroom added ON TOP of mean + kσ when a threshold freezes.
/// The kσ term finds the noise's ceiling; this term lifts the threshold
/// past it so the whole distribution lands on the steep part of the
/// expander curve. Without it the noise sits at most ~2σ under threshold
/// and a 2:1 ratio can never buy more than ~2σ of cut — the depth the
/// REDUCE dial promises would be unreachable at the shipped settings.
pub const DENOISE_LEARN_HEADROOM_DB: f32 = 4.0;

/// How fast a cut lets go when signal arrives, in milliseconds. Opening is
/// the direction that shaves transients, so it is fast and fixed; the
/// lookahead buys the detector the rest of the head start.
const DENOISE_OPEN_MS: f32 = 2.0;

/// The control tick, in milliseconds — how often expander maths, the
/// cross-band constraint and coefficient redesign run. Fixed in TIME, not
/// in samples, so the per-tick energy statistics (and therefore the learned
/// σ) read the same at every sample rate. ~0.7 ms is far below any gain
/// trajectory this unit produces.
const DENOISE_TICK_MS: f32 = 0.667;

/// The detector keeps TWO envelopes per band because its two consumers
/// want opposite biases. The GATE envelope rises near-instantly (so an
/// onset is seen inside the lookahead window) and falls over ~12 ms; that
/// asymmetry makes it ride the TOPS of the band's level distribution —
/// which errs toward opening, exactly right for protecting signal. The
/// LEVEL envelope is symmetric at ~12 ms: an unbiased estimate of the same
/// statistic the learn pass measured, which is what the expander curve
/// must compare against the threshold — feed the curve the peak-riding
/// gate envelope instead and the learn margin (mean + 2σ) cancels out to
/// nothing, and so does the reduction.
const ENV_RISE_MS: f32 = 1.5;
const ENV_FALL_MS: f32 = 12.0;
const LEVEL_MS: f32 = 12.0;

/// The slow reference the transient detector compares against, and how far
/// above it the envelope must jump to count as a transient.
const SLOW_MS: f32 = 250.0;
const TRANSIENT_DB: f32 = 12.0;

/// How long the "just saw a transient" flag takes to decay, and how much
/// faster the release runs while it is up (0.35 × the dialled time).
const BOOST_FALL_MS: f32 = 300.0;
const RELEASE_FAST_SCALE: f32 = 0.35;

/// Below this a band's depth is 0 dB in every way that matters, and the
/// biquad is skipped — this is the line the bit-transparency guarantee
/// stands on.
const DEPTH_EPS_DB: f32 = 0.05;

/// Redesigning coefficients for a depth move smaller than this would be
/// work nobody can hear.
const REDESIGN_EPS_DB: f32 = 0.01;

/// A learn pass shorter than this many control ticks (~10 ms) is a
/// misclick, not a noise profile, and must not wipe a good one.
const MIN_LEARN_TICKS: f64 = 16.0;

/// Below this a 0..=1 dial is at its resting position and means OFF —
/// same figure as the fixed chain's `OFF`, duplicated because this kernel
/// must stand alone.
const DENOISE_OFF: f32 = 0.0005;

/// Below this a sample is on its way to a denormal and is worth nothing.
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

// ── the parameters ──────────────────────────────────────────────────────────

/// Every dial, normalised 0..=1. The mapping to what the DSP actually wants
/// lives in the consts above, next to the reasons. `learn` and `lookahead`
/// are toggles: at or above 0.5 they are on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DenoiseParams {
    /// Toggle. While up, no reduction is applied and the kernel listens —
    /// play it isolated noise. On the falling edge the thresholds freeze.
    pub learn: f32,
    /// The per-band depth cap across 0..[`DENOISE_REDUCE_DB`], linear;
    /// bottom of the travel is off.
    pub reduce: f32,
    /// The expansion ratio across [`DENOISE_RATIO`], geometric. Low is
    /// smoother, exactly as the dial's name promises.
    pub smooth: f32,
    /// Across [`DENOISE_RELEASE_MS`], geometric.
    pub release: f32,
    /// The sidechain high-pass across [`DENOISE_SC_HPF_HZ`]; 0 is off.
    pub sc_hpf: f32,
    /// Toggle. [`DENOISE_LOOKAHEAD_MS`] of signal delay so the detector
    /// reads ahead; [`Denoise::latency`] tells the truth about it.
    pub lookahead: f32,
}

/// The shipped sound: nothing learned yet (a wire), 12 dB cap, middle
/// ratio and release, detector hearing everything, lookahead on.
impl Default for DenoiseParams {
    fn default() -> Self {
        Self {
            learn: 0.0,
            reduce: 0.5,
            smooth: 0.5,
            release: 0.5,
            sc_hpf: 0.0,
            lookahead: 1.0,
        }
    }
}

// ── per-band tables and state ───────────────────────────────────────────────

/// One band's fixed design-time numbers, computed once in `new`: the bell's
/// prewarped trigonometry in f64 (redesign happens at control rate and
/// precision is cheap there) and the detector pole in f32 (that part runs
/// per sample).
#[derive(Clone, Copy)]
struct BandDesign {
    /// cos(ω₀) of the bell centre.
    cs: f64,
    /// sin(ω₀) / 2Q of the bell.
    alpha: f64,
    /// The detector pole, rectangular, and its input gain (1 − r), which
    /// normalises a centre-frequency tone to unity through the resonator.
    pr: f32,
    pi: f32,
    g: f32,
    /// True when the centre sits too close to Nyquist to design a bell —
    /// only reachable at absurdly low sample rates; the band then never
    /// cuts.
    inert: bool,
}

/// One bell's audio-rate coefficients. f32, per the house split: design
/// maths above is where precision buys accuracy; here it would buy
/// nanoseconds. `b1 == a1` for a peaking section — stored anyway so the
/// inner loop stays shaped like every other biquad in the app.
#[derive(Clone, Copy)]
struct Coef {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Coef {
    const WIRE: Coef = Coef {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };
}

/// One channel of one bell: transposed direct form II, the form that
/// behaves when coefficients move under it — which here they do on any
/// tick where a band's depth is travelling.
#[derive(Clone, Copy, Default)]
struct Biquad {
    z1: f32,
    z2: f32,
}

impl Biquad {
    #[inline]
    fn run(&mut self, c: &Coef, x: f32) -> f32 {
        let y = c.b0 * x + self.z1;
        self.z1 = flush(c.b1 * x - c.a1 * y + self.z2);
        self.z2 = flush(c.b2 * x - c.a2 * y);
        y
    }
}

/// Everything one band remembers between samples and ticks.
#[derive(Clone, Copy)]
struct Band {
    /// The detector's two cascaded resonator stages, rectangular.
    re: f32,
    im: f32,
    re2: f32,
    im2: f32,
    /// Energy accumulated over the current control tick.
    eacc: f32,
    /// The gate envelope (peak-riding), the level envelope (unbiased), and
    /// the slow reference under them — all dB. See `ENV_RISE_MS` for why
    /// there are two.
    env_db: f32,
    level_db: f32,
    slow_db: f32,
    /// 1 right after a transient, decaying toward 0; scales the release.
    boost: f32,
    /// The hysteresis state: open means "signal here, do not cut".
    open: bool,
    /// Control ticks of minimum-hold remaining before a close is allowed.
    hold: u32,
    /// The smoothed, cross-band-constrained cut, in positive dB — the
    /// number the meter shows and the bell applies.
    depth_db: f32,
    /// What the current coefficients were designed from.
    applied_db: f32,
    /// Whether the bell is in the circuit at all (false at 0 dB — skipped,
    /// bit-transparent).
    active: bool,
    coef: Coef,
    bq: [Biquad; 2],
    /// The frozen threshold, normalized and in dB; norm below
    /// [`DENOISE_OFF`] means the band never reduces.
    thr_norm: f32,
    thr_db: f32,
    /// Learn accumulators: tick count, Σ level, Σ level² (dB domain).
    n: f64,
    sum: f64,
    sumsq: f64,
}

impl Band {
    const IDLE: Band = Band {
        re: 0.0,
        im: 0.0,
        re2: 0.0,
        im2: 0.0,
        eacc: 0.0,
        env_db: -120.0,
        level_db: -120.0,
        slow_db: -120.0,
        boost: 0.0,
        open: false,
        hold: 0,
        depth_db: 0.0,
        applied_db: 0.0,
        active: false,
        coef: Coef::WIRE,
        bq: [Biquad { z1: 0.0, z2: 0.0 }; 2],
        thr_norm: 0.0,
        thr_db: DENOISE_THR_DB.0,
        n: 0.0,
        sum: 0.0,
        sumsq: 0.0,
    };

    /// Forget what was playing, keep what was learned.
    fn quiesce(&mut self) {
        let (thr_norm, thr_db) = (self.thr_norm, self.thr_db);
        *self = Band::IDLE;
        self.thr_norm = thr_norm;
        self.thr_db = thr_db;
    }
}

/// The per-block numbers the control tick works from, derived from the
/// dials once per `process` call.
struct TickPlan {
    learning: bool,
    /// The REDUCE cap, positive dB; 0 disables all cutting.
    cap_db: f32,
    /// ratio − 1: dB of cut per dB under threshold, past the knee.
    slope: f32,
    /// Depth smoothing toward MORE cut at the dialled release (boost 0)
    /// and at the after-a-transient release (boost 1); the per-band
    /// coefficient interpolates between them.
    a_rel_slow: f32,
    a_rel_fast: f32,
}

// ── the denoiser ────────────────────────────────────────────────────────────

/// The learned multiband downward expander, one signal path of skippable
/// bell cuts plus a resonator sidechain. See the module header for the
/// design; see [`Denoise::process`] for the contract.
pub struct Denoise {
    sample_rate: f32,
    design: [BandDesign; DENOISE_BANDS],
    band: [Band; DENOISE_BANDS],
    /// Control tick length in samples (time-fixed: ~0.7 ms), and how far
    /// through the current tick the sample loop is.
    tick_len: u32,
    phase: u32,
    /// Tick-domain smoothing coefficients for the fixed time constants,
    /// computed once in `new`.
    a_rise: f32,
    a_fall: f32,
    a_level: f32,
    a_slow: f32,
    boost_fall: f32,
    a_open: f32,
    hold_ticks: u32,
    /// The sidechain high-pass state (one-pole low-pass; the high-pass is
    /// input minus this), primed at the input while off, like the comp's.
    hp_lp: f32,
    /// The lookahead line, interleaved stereo, and where it is.
    ring: Vec<f32>,
    rpos: usize,
    la_len: usize,
    la_on: bool,
    /// Whether the previous block was learning, for edge detection.
    learning: bool,
    /// How many bands have a live (non-off) threshold; 0 and not learning
    /// means the whole unit is provably a wire.
    live: u32,
}

impl Denoise {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() {
            sample_rate.max(8_000.0)
        } else {
            44_100.0
        };
        let sr = f64::from(sample_rate);

        // The bands: centres log-spaced across DENOISE_BAND_HZ, each with a
        // bell (fixed Q, gain designed later per tick) and a detector pole
        // whose bandwidth in HERTZ tracks the centre — which is what makes
        // the per-tick energy statistics rate-invariant.
        let mut design = [BandDesign {
            cs: 0.0,
            alpha: 0.0,
            pr: 0.0,
            pi: 0.0,
            g: 0.0,
            inert: true,
        }; DENOISE_BANDS];
        for (k, d) in design.iter_mut().enumerate() {
            let hz = f64::from(Self::center_hz(k));
            if hz > 0.45 * sr {
                continue; // only reachable at silly-low rates; never cuts
            }
            let w0 = std::f64::consts::TAU * hz / sr;
            let bw = hz / f64::from(DENOISE_DETECTOR_Q);
            let r = (-std::f64::consts::PI * bw / sr).exp();
            *d = BandDesign {
                cs: w0.cos(),
                alpha: w0.sin() / (2.0 * f64::from(DENOISE_BAND_Q)),
                pr: (r * w0.cos()) as f32,
                pi: (r * w0.sin()) as f32,
                g: (1.0 - r) as f32,
                inert: false,
            };
        }

        // The control tick, fixed in time. Every tick-domain coefficient
        // below derives from the tick's TRUE duration at this rate, so a
        // 29-sample tick at 44.1k and a 64-sample tick at 96k mean the
        // same milliseconds.
        let tick_len = ((sr * f64::from(DENOISE_TICK_MS) / 1_000.0).round() as u32).max(8);
        let tick_s = f64::from(tick_len) / sr;
        let a_of = |ms: f32| (1.0 - (-tick_s * 1_000.0 / f64::from(ms)).exp()) as f32;

        let la_len = ((sr * f64::from(DENOISE_LOOKAHEAD_MS) / 1_000.0).round() as usize).max(1);

        Self {
            sample_rate,
            design,
            band: [Band::IDLE; DENOISE_BANDS],
            tick_len,
            phase: 0,
            a_rise: a_of(ENV_RISE_MS),
            a_fall: a_of(ENV_FALL_MS),
            a_level: a_of(LEVEL_MS),
            a_slow: a_of(SLOW_MS),
            boost_fall: (-tick_s * 1_000.0 / f64::from(BOOST_FALL_MS)).exp() as f32,
            a_open: a_of(DENOISE_OPEN_MS),
            hold_ticks: (f64::from(DENOISE_HOLD_MS) / (tick_s * 1_000.0)).round() as u32,
            hp_lp: 0.0,
            ring: vec![0.0; la_len * 2],
            rpos: 0,
            la_len,
            la_on: true,
            learning: false,
            live: 0,
        }
    }

    /// Band `k`'s centre frequency, in Hz — the geometry the whole file is
    /// built on, public so a meter can label what it draws.
    pub fn center_hz(k: usize) -> f32 {
        let t = k as f32 / (DENOISE_BANDS - 1) as f32;
        DENOISE_BAND_HZ.0 * (DENOISE_BAND_HZ.1 / DENOISE_BAND_HZ.0).powf(t)
    }

    /// The frozen per-band thresholds, normalized across
    /// [`DENOISE_THR_DB`] — 0.0 is "this band never reduces". These are the
    /// 32 state symbols a session persists.
    pub fn thresholds(&self) -> [f32; DENOISE_BANDS] {
        let mut out = [0.0_f32; DENOISE_BANDS];
        for (o, b) in out.iter_mut().zip(self.band.iter()) {
            *o = b.thr_norm;
        }
        out
    }

    /// Restore a persisted profile. Values are clamped to 0..=1 and
    /// anything non-finite lands on 0 (off) — a corrupt session file must
    /// never arm a gate.
    pub fn set_thresholds(&mut self, thresholds: &[f32; DENOISE_BANDS]) {
        for (b, &t) in self.band.iter_mut().zip(thresholds.iter()) {
            let norm = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
            b.thr_norm = norm;
            b.thr_db = DENOISE_THR_DB.0 + (DENOISE_THR_DB.1 - DENOISE_THR_DB.0) * norm;
        }
        self.recount();
    }

    /// The honest latency, in samples, for the current lookahead setting —
    /// what a host must be told. "Current" means as of the last `process`
    /// call (or the default, lookahead on, before the first).
    pub fn latency(&self) -> u32 {
        if self.la_on {
            self.la_len as u32
        } else {
            0
        }
    }

    /// Each band's applied cut right now, in positive dB — the meter's
    /// numbers, post-constraint, exactly what the bells are doing.
    pub fn depth_db(&self) -> [f32; DENOISE_BANDS] {
        let mut out = [0.0_f32; DENOISE_BANDS];
        for (o, b) in out.iter_mut().zip(self.band.iter()) {
            *o = b.depth_db;
        }
        out
    }

    fn recount(&mut self) {
        self.live = self
            .band
            .iter()
            .filter(|b| b.thr_norm >= DENOISE_OFF)
            .count() as u32;
    }

    /// The falling edge of the learn toggle: freeze each band's threshold
    /// at mean + [`DENOISE_LEARN_SIGMA`]·σ of the log energy it heard. A
    /// band that heard essentially silence normalizes to ~0 and lands on
    /// OFF, which is the right answer — there is nothing there to cut.
    fn freeze_thresholds(&mut self) {
        for b in self.band.iter_mut() {
            if b.n < MIN_LEARN_TICKS {
                continue; // a misclick, not a profile — keep the old value
            }
            let mean = b.sum / b.n;
            let var = (b.sumsq / b.n - mean * mean).max(0.0);
            let thr = mean
                + f64::from(DENOISE_LEARN_SIGMA) * var.sqrt()
                + f64::from(DENOISE_LEARN_HEADROOM_DB);
            let span = f64::from(DENOISE_THR_DB.1 - DENOISE_THR_DB.0);
            let norm = ((thr - f64::from(DENOISE_THR_DB.0)) / span).clamp(0.0, 1.0) as f32;
            b.thr_norm = norm;
            b.thr_db = DENOISE_THR_DB.0 + (DENOISE_THR_DB.1 - DENOISE_THR_DB.0) * norm;
        }
        for b in self.band.iter_mut() {
            b.n = 0.0;
            b.sum = 0.0;
            b.sumsq = 0.0;
        }
        self.recount();
    }

    /// Design one bell for a cut of `depth_db`. RBJ peaking with the fixed
    /// per-band trigonometry, so a redesign is one `powf` and a handful of
    /// divides — cheap enough to run per band per control tick while a
    /// depth is travelling. At depth 0 this would collapse to the identity
    /// (b ≡ a for A = 1), but depth 0 never gets here: the band is skipped.
    fn design_coef(d: &BandDesign, depth_db: f32) -> Coef {
        let a = 10.0_f64.powf(f64::from(-depth_db) / 40.0);
        let a0 = 1.0 + d.alpha / a;
        Coef {
            b0: ((1.0 + d.alpha * a) / a0) as f32,
            b1: ((-2.0 * d.cs) / a0) as f32,
            b2: ((1.0 - d.alpha * a) / a0) as f32,
            a1: ((-2.0 * d.cs) / a0) as f32,
            a2: ((1.0 - d.alpha / a) / a0) as f32,
        }
    }

    /// One control tick: per-band envelope, learn or expander+hysteresis,
    /// time smoothing, then the cross-band slope constraint and coefficient
    /// redesign. Everything in the dB domain until the final `powf`.
    fn control(&mut self, plan: &TickPlan) {
        let inv_tick = 1.0 / self.tick_len as f32;
        let mut depth = [0.0_f32; DENOISE_BANDS];

        for (k, b) in self.band.iter_mut().enumerate() {
            // The tick's band level. The ×4 calibrates the complex
            // resonator — a centre-frequency tone of amplitude A settles at
            // |y| ≈ A/2 (the analytic half), so 4·|y|² reads A² and the
            // thresholds speak dBFS of tone amplitude. Floored at −120 dB
            // so silence is a number, not a logarithm of zero.
            let level_db = 10.0 * (4.0 * b.eacc * inv_tick).max(1.0e-12).log10();
            b.eacc = 0.0;

            // Log-domain envelopes: the peak-riding gate one and the
            // unbiased level one.
            let a = if level_db > b.env_db {
                self.a_rise
            } else {
                self.a_fall
            };
            b.env_db += a * (level_db - b.env_db);
            b.level_db += self.a_level * (level_db - b.level_db);

            // The transient flag: envelope far above its slow reference
            // means a hit just landed; the release runs faster for the
            // next few hundred ms so the gap behind the hit does not hiss.
            b.slow_db += self.a_slow * (b.env_db - b.slow_db);
            if b.env_db > b.slow_db + TRANSIENT_DB {
                b.boost = 1.0;
            } else {
                b.boost *= self.boost_fall;
            }

            if plan.learning {
                // Statistics on the RAW tick level, not the smoothed
                // envelope: σ must capture the excursions the smoothing
                // exists to hide.
                b.n += 1.0;
                b.sum += f64::from(level_db);
                b.sumsq += f64::from(level_db) * f64::from(level_db);
                depth[k] = 0.0;
                b.depth_db = 0.0;
                continue;
            }

            let off = b.thr_norm < DENOISE_OFF
                || self.design[k].inert
                || plan.cap_db < DEPTH_EPS_DB;
            let target = if off {
                b.open = false;
                b.hold = 0;
                0.0
            } else {
                // Hysteresis first, on the fast gate envelope: opening
                // happens the full window above the learned threshold,
                // closing back at the threshold itself and only after the
                // hold has run out. See DENOISE_HYSTERESIS_DB for why the
                // window sits above rather than straddling.
                if b.env_db >= b.thr_db + DENOISE_HYSTERESIS_DB {
                    b.open = true;
                    b.hold = self.hold_ticks;
                } else if b.open {
                    if b.hold > 0 {
                        b.hold -= 1;
                    } else if b.env_db <= b.thr_db {
                        b.open = false;
                    }
                }
                if b.open {
                    0.0
                } else {
                    // The static curve: soft-knee downward expansion, on
                    // the UNBIASED level envelope — the statistic the
                    // threshold was learned from. `under` is how far below
                    // threshold the band sits; the quadratic meets both
                    // straight segments tangent, so there is no corner for
                    // a level to sit on and buzz.
                    let under = b.thr_db - b.level_db;
                    let w = DENOISE_KNEE_DB;
                    let wanted = if 2.0 * under <= -w {
                        0.0
                    } else if 2.0 * under < w {
                        let t = under + 0.5 * w;
                        plan.slope * t * t / (2.0 * w)
                    } else {
                        plan.slope * under
                    };
                    wanted.min(plan.cap_db)
                }
            };

            // Depth smoothing, dB domain. Toward LESS cut is the fast fixed
            // opening; toward MORE cut is the release, interpolated toward
            // its after-a-transient speed by the boost flag.
            let a = if target < b.depth_db {
                self.a_open
            } else {
                plan.a_rel_slow + (plan.a_rel_fast - plan.a_rel_slow) * b.boost
            };
            b.depth_db += a * (target - b.depth_db);
            // Flush the decaying tail to a true zero — but ONLY while the
            // target is zero. A rising depth starts each tick a fraction
            // of a dB above nothing, well inside any epsilon; flush that
            // and no cut can ever climb out of zero (the stale-clamp
            // disease, met before and it cost an afternoon).
            if target <= 0.0 && b.depth_db < DEPTH_EPS_DB {
                b.depth_db = 0.0;
            }
            depth[k] = b.depth_db;
        }

        // The cross-band slope constraint: one pass up, one pass down, each
        // only ever LOWERING a band toward its neighbour + 6 dB. Two sweeps
        // reach a fixed point because min() is monotone. The constrained
        // value is written back as the band's real depth — the smoothing
        // above must continue from what was actually applied, or the state
        // and the sound drift apart.
        for k in 1..DENOISE_BANDS {
            depth[k] = depth[k].min(depth[k - 1] + DENOISE_SLOPE_DB);
        }
        for k in (0..DENOISE_BANDS - 1).rev() {
            depth[k] = depth[k].min(depth[k + 1] + DENOISE_SLOPE_DB);
        }

        for (k, b) in self.band.iter_mut().enumerate() {
            b.depth_db = depth[k];
            if b.depth_db < DEPTH_EPS_DB {
                // Skipped entirely: this is the bit-transparent state. The
                // delay registers keep whatever tail was ringing; by the
                // time the band re-engages the depth is a fraction of a dB
                // and the bell is within rounding of the identity, so the
                // stale tail never replays audibly (and is cleared on
                // re-activation below anyway).
                b.active = false;
                b.applied_db = 0.0;
            } else if !b.active || (b.depth_db - b.applied_db).abs() > REDESIGN_EPS_DB {
                if !b.active {
                    // Arriving from skipped: start the bell from silence.
                    // Its first coefficients are near-identity (the depth
                    // just crossed 0.05 dB), so an empty state is a click
                    // of nothing.
                    b.bq = [Biquad::default(); 2];
                }
                b.coef = Self::design_coef(&self.design[k], b.depth_db);
                b.applied_db = b.depth_db;
                b.active = true;
            }
        }
    }

    /// Apply the denoiser in place to an interleaved buffer.
    ///
    /// `channels` is the stride; anything past the first two channels is
    /// left alone and fewer than two is returned untouched, as the fixed
    /// chain does — a third channel is not a thing to guess about.
    ///
    /// With nothing learned, learn down and lookahead off, this is
    /// provably a wire: it returns before touching the buffer.
    pub fn process(
        &mut self,
        buf: &mut [f32],
        frames: usize,
        channels: usize,
        params: &DenoiseParams,
    ) {
        // A dial is a number out of a settings file somebody may have
        // edited: clamp, and treat anything non-finite as the dial's
        // resting place.
        let dial = |v: f32, rest: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                rest
            }
        };
        let learn_on = dial(params.learn, 0.0) >= 0.5;
        let la_on = dial(params.lookahead, 1.0) >= 0.5;

        // Toggle edges are block-rate events, handled before any audio.
        if la_on != self.la_on {
            // The line's history belongs to the other timeline; a 3 ms
            // splice on a toggle is inherent to changing latency and the
            // host is told the new number by `latency()`.
            self.ring.iter_mut().for_each(|s| *s = 0.0);
            self.rpos = 0;
            self.la_on = la_on;
        }
        if learn_on && !self.learning {
            // Learning starts: reduction snaps off NOW — the user is
            // playing noise at the detector and must hear it raw — and
            // stale statistics from an abandoned pass are dropped.
            for b in self.band.iter_mut() {
                b.depth_db = 0.0;
                b.applied_db = 0.0;
                b.active = false;
                b.open = false;
                b.hold = 0;
                b.bq = [Biquad::default(); 2];
                b.n = 0.0;
                b.sum = 0.0;
                b.sumsq = 0.0;
            }
            self.learning = true;
        } else if !learn_on && self.learning {
            self.freeze_thresholds();
            self.learning = false;
        }

        if channels < 2 {
            return;
        }
        // The provable-wire fast path: nothing learned, not learning, no
        // lookahead delay owed — the buffer is not even read.
        if !self.learning && self.live == 0 && !self.la_on {
            return;
        }

        // Design maths in f64, once per block; the audio path below stays
        // f32.
        let sr = f64::from(self.sample_rate);
        let tick_s = f64::from(self.tick_len) / sr;
        let geo = |range: (f32, f32), t: f32| {
            f64::from(range.0) * f64::from(range.1 / range.0).powf(f64::from(t))
        };
        let rel_ms = geo(DENOISE_RELEASE_MS, dial(params.release, 0.5));
        let a_rel = |ms: f64| (1.0 - (-tick_s * 1_000.0 / ms).exp()) as f32;
        let plan = TickPlan {
            learning: self.learning,
            cap_db: dial(params.reduce, 0.5) * DENOISE_REDUCE_DB,
            slope: (geo(DENOISE_RATIO, dial(params.smooth, 0.5)) - 1.0) as f32,
            a_rel_slow: a_rel(rel_ms),
            a_rel_fast: a_rel(rel_ms * f64::from(RELEASE_FAST_SCALE)),
        };
        let hpf = dial(params.sc_hpf, 0.0);
        let hpf_on = hpf >= DENOISE_OFF;
        let a_hp = if hpf_on {
            (1.0 - (-std::f64::consts::TAU * geo(DENOISE_SC_HPF_HZ, hpf) / sr).exp()) as f32
        } else {
            0.0
        };

        for frame in buf.chunks_exact_mut(channels).take(frames) {
            let (l, r) = (frame[0], frame[1]);

            // The sidechain reads the UNDELAYED input — with the lookahead
            // ring on the signal path below, that is what "the detector
            // reads ahead" physically is. Mono mid: the cut lands on both
            // channels identically (per-side expansion would drag the
            // image), so one detector serves.
            let mut d = 0.5 * (l + r);
            if hpf_on {
                self.hp_lp = flush(self.hp_lp + a_hp * (d - self.hp_lp));
                d -= self.hp_lp;
            } else {
                // Primed, so engaging the filter later opens from "no bass
                // removed yet" rather than from a stale average.
                self.hp_lp = d;
            }
            for (b, dz) in self.band.iter_mut().zip(self.design.iter()) {
                if dz.inert {
                    continue;
                }
                // The pair of cascaded complex resonators, rectangular:
                // eight multiplies, |y|² is band energy with no rectifier
                // ripple and no phase anywhere near the audio, and the
                // squared skirts keep a loud tone two octaves away out of
                // this band's decisions entirely.
                let re = dz.pr * b.re - dz.pi * b.im + dz.g * d;
                let im = dz.pi * b.re + dz.pr * b.im;
                b.re = flush(re);
                b.im = flush(im);
                let re2 = dz.pr * b.re2 - dz.pi * b.im2 + dz.g * re;
                let im2 = dz.pi * b.re2 + dz.pr * b.im2 + dz.g * im;
                b.re2 = flush(re2);
                b.im2 = flush(im2);
                b.eacc += re2 * re2 + im2 * im2;
            }

            self.phase += 1;
            if self.phase >= self.tick_len {
                self.control(&plan);
                self.phase = 0;
            }

            // The signal path: the lookahead delay, then only the bells
            // that are actually cutting.
            let (mut xl, mut xr) = if self.la_on {
                let at = self.rpos * 2;
                let out = (self.ring[at], self.ring[at + 1]);
                self.ring[at] = l;
                self.ring[at + 1] = r;
                self.rpos += 1;
                if self.rpos >= self.la_len {
                    self.rpos = 0;
                }
                out
            } else {
                (l, r)
            };
            for b in self.band.iter_mut() {
                if b.active {
                    xl = b.bq[0].run(&b.coef, xl);
                    xr = b.bq[1].run(&b.coef, xr);
                }
            }
            frame[0] = xl;
            frame[1] = xr;
        }
    }

    /// Drop every trace of what was playing: bells, resonators, envelopes,
    /// depths, the lookahead line, any half-done learn statistics. The
    /// learned THRESHOLDS stay — they are settings, not signal, exactly as
    /// a cleared EQ keeps its curve.
    pub fn clear(&mut self) {
        for b in self.band.iter_mut() {
            b.quiesce();
        }
        self.ring.iter_mut().for_each(|s| *s = 0.0);
        self.rpos = 0;
        self.phase = 0;
        self.hp_lp = 0.0;
    }
}

// ── proof ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// Deterministic shaped noise: xorshift32 whitened, then a fixed
    /// one-pole high-pass at 150 Hz and low-pass at 2 kHz so the energy
    /// lives inside the band range the unit covers. Interleaved stereo,
    /// both channels the same signal (the detector is mid anyway).
    struct Noise {
        seed: u32,
        lp: f32,
        hp_lp: f32,
        a_lp: f32,
        a_hp: f32,
        amp: f32,
    }

    impl Noise {
        fn new(seed: u32, amp: f32, sr: f32) -> Self {
            let a = |hz: f32| 1.0 - (-std::f32::consts::TAU * hz / sr).exp();
            Noise {
                seed,
                lp: 0.0,
                hp_lp: 0.0,
                a_lp: a(2_000.0),
                a_hp: a(150.0),
                amp,
            }
        }

        fn next(&mut self) -> f32 {
            self.seed ^= self.seed << 13;
            self.seed ^= self.seed >> 17;
            self.seed ^= self.seed << 5;
            let white = (self.seed as f32 / u32::MAX as f32) * 2.0 - 1.0;
            self.lp += self.a_lp * (white - self.lp);
            self.hp_lp += self.a_hp * (self.lp - self.hp_lp);
            (self.lp - self.hp_lp) * self.amp
        }

        fn frames(&mut self, frames: usize) -> Vec<f32> {
            let mut buf = vec![0.0_f32; frames * 2];
            for f in buf.chunks_exact_mut(2) {
                let v = self.next();
                f[0] = v;
                f[1] = v;
            }
            buf
        }
    }

    /// An interleaved stereo sine, both channels in phase.
    fn sine(hz: f32, amp: f32, frames: usize, sr: f32) -> Vec<f32> {
        let mut buf = vec![0.0_f32; frames * 2];
        let mut phase = 0.0_f32;
        for f in buf.chunks_exact_mut(2) {
            phase += hz * std::f32::consts::TAU / sr;
            let v = amp * phase.sin();
            f[0] = v;
            f[1] = v;
        }
        buf
    }

    fn add(buf: &mut [f32], other: &[f32]) {
        for (a, b) in buf.iter_mut().zip(other.iter()) {
            *a += b;
        }
    }

    /// The left channel's RMS from `from` frames on, in dB.
    fn rms_db(buf: &[f32], from: usize) -> f32 {
        let left: Vec<f32> = buf.chunks_exact(2).skip(from).map(|f| f[0]).collect();
        let ms = left.iter().map(|v| v * v).sum::<f32>() / left.len() as f32;
        10.0 * ms.max(1.0e-18).log10()
    }

    /// A Hann-windowed Goertzel-style correlation at exactly `hz`, on the
    /// left channel over `[from, from+len)` frames: the sine's amplitude in
    /// dB, insensitive to the noise around it and to the lookahead's phase.
    /// Hann, not rectangular — a rect window's leakage lies (see the string
    /// bench war story).
    fn tone_db(buf: &[f32], hz: f32, sr: f32, from: usize, len: usize) -> f32 {
        let (mut re, mut im, mut wsum) = (0.0_f64, 0.0_f64, 0.0_f64);
        for n in 0..len {
            let x = f64::from(buf[(from + n) * 2]);
            let w = 0.5
                - 0.5
                    * (std::f64::consts::TAU * n as f64 / len as f64).cos();
            let ph = std::f64::consts::TAU * f64::from(hz) * (from + n) as f64 / f64::from(sr);
            re += x * w * ph.cos();
            im += x * w * ph.sin();
            wsum += w;
        }
        let amp = 2.0 * (re * re + im * im).sqrt() / wsum;
        (20.0 * amp.max(1.0e-12).log10()) as f32
    }

    /// Learn `seconds` of the given noise at defaults, freeze, and hand the
    /// unit back ready to suppress.
    fn learned(seed: u32, amp: f32, sr: f32, seconds: f32) -> Denoise {
        let mut dn = Denoise::new(sr);
        let learn = DenoiseParams {
            learn: 1.0,
            ..DenoiseParams::default()
        };
        let frames = (sr * seconds) as usize;
        let mut noise = Noise::new(seed, amp, sr);
        let mut buf = noise.frames(frames);
        dn.process(&mut buf, frames, 2, &learn);
        // The falling edge freezes the thresholds.
        let mut tiny = [0.0_f32; 8];
        dn.process(&mut tiny, 4, 2, &DenoiseParams::default());
        dn
    }

    /// **Nothing learned is a wire** — bit-exact with the lookahead off,
    /// whatever the other dials say, and the same with a full set of
    /// floor thresholds explicitly written in. "This effect is not doing
    /// anything" must survive a null test against the dry track.
    #[test]
    fn nothing_learned_and_thresholds_at_the_floor_are_bit_transparent() {
        let frames = 4_800;
        let extremes = [0.0_f32, 1.0];
        for bits in 0..16_u32 {
            let d = |bit: u32| extremes[(bits >> bit & 1) as usize];
            let p = DenoiseParams {
                learn: 0.0,
                lookahead: 0.0,
                reduce: d(0),
                smooth: d(1),
                release: d(2),
                sc_hpf: d(3),
            };
            let mut dn = Denoise::new(SR);
            if bits >= 8 {
                // Same claim with the floor written in by hand.
                dn.set_thresholds(&[0.0; DENOISE_BANDS]);
            }
            let mut buf = sine(1_000.0, 0.5, frames, SR);
            add(&mut buf, &Noise::new(7, 0.1, SR).frames(frames));
            let before = buf.clone();
            dn.process(&mut buf, frames, 2, &p);
            assert_eq!(buf, before, "a denoiser with nothing to do changed the signal (dials {bits:04b})");
        }
    }

    /// **Lookahead is an honest delay and nothing else**: with nothing
    /// learned and the toggle up, the output is the input shifted by
    /// exactly `latency()` samples — bit for bit — and `latency()` says
    /// 3 ms' worth. Off, it says zero.
    #[test]
    fn lookahead_is_an_honest_delay_and_latency_tells_the_truth() {
        let frames = 4_800;
        let mut dn = Denoise::new(SR);
        let p = DenoiseParams::default();
        assert_eq!(dn.latency(), (SR * DENOISE_LOOKAHEAD_MS / 1_000.0).round() as u32);
        let la = dn.latency() as usize;

        let mut buf = sine(440.0, 0.5, frames, SR);
        add(&mut buf, &Noise::new(3, 0.2, SR).frames(frames));
        let before = buf.clone();
        dn.process(&mut buf, frames, 2, &p);
        for f in 0..frames {
            let want = if f < la { [0.0, 0.0] } else { [before[(f - la) * 2], before[(f - la) * 2 + 1]] };
            assert_eq!([buf[f * 2], buf[f * 2 + 1]], want, "frame {f} is not a clean {la}-sample delay");
        }

        let off = DenoiseParams {
            lookahead: 0.0,
            ..p
        };
        dn.process(&mut [0.0; 8], 4, 2, &off);
        assert_eq!(dn.latency(), 0, "lookahead off still reports latency");
    }

    /// **No setting the panel can reach may blow up.** Every dial at both
    /// extremes — including the learn and lookahead toggles flapping —
    /// against a signal well past full scale: the output stays finite and
    /// bounded.
    #[test]
    fn no_setting_the_panel_can_reach_makes_it_explode() {
        let frames = 4_800;
        let extremes = [0.0_f32, 1.0];
        for bits in 0..64_u32 {
            let d = |bit: u32| extremes[(bits >> bit & 1) as usize];
            let p = DenoiseParams {
                learn: d(0),
                reduce: d(1),
                smooth: d(2),
                release: d(3),
                sc_hpf: d(4),
                lookahead: d(5),
            };
            let mut dn = Denoise::new(SR);
            for pass in 0..2 {
                // Second pass flips learn, so every combination also
                // exercises the freeze edge.
                let p = if pass == 1 {
                    DenoiseParams {
                        learn: 1.0 - p.learn,
                        ..p
                    }
                } else {
                    p
                };
                let mut buf = sine(55.0, 2.0, frames, SR);
                add(&mut buf, &Noise::new(11, 1.0, SR).frames(frames));
                dn.process(&mut buf, frames, 2, &p);
                assert!(
                    buf.iter().all(|s| s.is_finite()),
                    "dials {bits:06b} pass {pass} produced a non-finite sample"
                );
                let peak = buf.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
                assert!(peak < 8.0, "dials {bits:06b} pass {pass} reached {peak}");
            }
        }
    }

    /// **Learn, then suppress**: a second of shaped noise teaches the
    /// thresholds; the same noise afterwards comes out at least 8 dB
    /// quieter broadband, at the shipped default settings.
    #[test]
    fn learned_noise_comes_back_at_least_8_db_quieter() {
        let mut dn = learned(0xBAD5EED, 0.05, SR, 1.0);
        let frames = SR as usize;
        let mut noise = Noise::new(0xBAD5EED, 0.05, SR);
        let mut buf = noise.frames(frames);
        let in_db = rms_db(&buf, frames / 2);
        dn.process(&mut buf, frames, 2, &DenoiseParams::default());
        let out_db = rms_db(&buf, frames / 2);
        assert!(
            in_db - out_db >= 8.0,
            "the learned noise only dropped {:.1} dB ({in_db:.1} -> {out_db:.1})",
            in_db - out_db
        );
    }

    /// **The signal survives the suppression that kills the noise**: a
    /// −20 dBFS 1 kHz sine over the learned noise keeps its level within
    /// 1.5 dB of clean, measured by Hann-windowed correlation, while the
    /// unit is actively cutting elsewhere. This is the whole point of the
    /// wide detector skirts and the cross-band slope limit.
    #[test]
    fn a_tone_survives_the_suppression_that_kills_the_noise() {
        let sr = SR;
        let frames = 2 * sr as usize;
        let (from, len) = (frames / 2, sr as usize / 2);
        let amp = 10.0_f32.powf(-20.0 / 20.0);

        let clean = sine(1_000.0, amp, frames, sr);
        let clean_db = tone_db(&clean, 1_000.0, sr, from, len);

        let mut dn = learned(0xBAD5EED, 0.05, sr, 1.0);
        let mut buf = sine(1_000.0, amp, frames, sr);
        add(&mut buf, &Noise::new(0xBAD5EED, 0.05, sr).frames(frames));
        dn.process(&mut buf, frames, 2, &DenoiseParams::default());
        let got_db = tone_db(&buf, 1_000.0, sr, from, len);

        assert!(
            (got_db - clean_db).abs() <= 1.5,
            "the tone moved from {clean_db:.2} to {got_db:.2} dB under suppression"
        );
        // And the suppression was real while the tone played: bands far
        // from 1 kHz were still cutting.
        let depth = dn.depth_db();
        let far = depth[..8].iter().chain(depth[26..].iter()).cloned().fold(0.0_f32, f32::max);
        assert!(far > 3.0, "no band far from the tone was cutting ({far:.1} dB) — the test proves nothing");
    }

    /// **Noise straddling a threshold cannot flap.** The learned threshold
    /// sits, by construction, right where the learning noise's excursions
    /// top out — the worst case for chatter. Per 64-frame block, no band's
    /// depth may move by anything near full scale, at the same noise and
    /// at +3 and +6 dB.
    #[test]
    fn noise_straddling_a_threshold_cannot_flap() {
        for gain_db in [0.0_f32, 3.0, 6.0] {
            let gain = 10.0_f32.powf(gain_db / 20.0);
            let mut dn = learned(0x5EED, 0.05, SR, 1.0);
            let p = DenoiseParams::default();
            let mut noise = Noise::new(0x5EED, 0.05 * gain, SR);
            // Settle for half a second first.
            let mut buf = noise.frames(SR as usize / 2);
            dn.process(&mut buf, SR as usize / 2, 2, &p);

            let mut last = dn.depth_db();
            let mut worst = 0.0_f32;
            for _ in 0..(2 * SR as usize / 64) {
                let mut buf = noise.frames(64);
                dn.process(&mut buf, 64, 2, &p);
                let now = dn.depth_db();
                for (a, b) in now.iter().zip(last.iter()) {
                    worst = worst.max((a - b).abs());
                }
                last = now;
            }
            // Full-depth flapping at the default cap would be 12 dB per
            // block; a legitimate fast open moves ~half the depth in one
            // 1.3 ms block. Chatter is the repeated full swing, and the
            // hysteresis + hold + knee keep every per-block move well
            // under it.
            assert!(
                worst <= 6.0,
                "at +{gain_db} dB a band moved {worst:.1} dB in one 64-frame block"
            );
        }
    }

    /// **One hot band between two cold ones is reined to the slope**: with
    /// a hand-written hostile profile (one threshold high, neighbours off)
    /// the applied depths still never step more than 6 dB band-to-band —
    /// the cross-band constraint holds where it is hardest.
    #[test]
    fn a_hot_band_between_cold_neighbours_is_reined_to_the_slope() {
        let mut thr = [0.0_f32; DENOISE_BANDS];
        thr[15] = 0.9; // −10 dBFS: everything below it wants the full cap
        let mut dn = Denoise::new(SR);
        dn.set_thresholds(&thr);
        let p = DenoiseParams {
            reduce: 1.0, // 24 dB cap, so the unconstrained target is 24
            ..DenoiseParams::default()
        };
        let frames = SR as usize;
        let mut buf = Noise::new(42, 0.01, SR).frames(frames);
        dn.process(&mut buf, frames, 2, &p);

        let depth = dn.depth_db();
        for k in 1..DENOISE_BANDS {
            assert!(
                (depth[k] - depth[k - 1]).abs() <= DENOISE_SLOPE_DB + 0.1,
                "bands {} and {k} are {:.1} and {:.1} dB apart",
                k - 1,
                depth[k - 1],
                depth[k]
            );
        }
        // The constraint was actually load-bearing: the hot band wanted
        // 24 dB and got held to its off neighbours + 6.
        assert!(
            depth[15] > 3.0 && depth[15] <= DENOISE_SLOPE_DB + 0.1,
            "band 15 applied {:.1} dB — the hostile profile never engaged the constraint",
            depth[15]
        );
        assert!(depth[13] < 0.1 && depth[17] < 0.1, "an off band is cutting");
    }

    /// **The same suppression at every sample rate.** Nothing here is a
    /// length table — band Hz, tick time and every constant derive from
    /// the rate — so learn-then-suppress lands the same broadband figure
    /// at 44.1k, 48k and 96k.
    #[test]
    fn the_suppression_is_the_same_at_every_sample_rate() {
        let drop_at = |sr: f32| {
            let mut dn = learned(0xABCD, 0.05, sr, 1.0);
            let frames = sr as usize;
            let mut noise = Noise::new(0xABCD, 0.05, sr);
            let mut buf = noise.frames(frames);
            let in_db = rms_db(&buf, frames / 2);
            dn.process(&mut buf, frames, 2, &DenoiseParams::default());
            in_db - rms_db(&buf, frames / 2)
        };
        let (a, b, c) = (drop_at(44_100.0), drop_at(48_000.0), drop_at(96_000.0));
        for (x, y) in [(a, b), (b, c), (a, c)] {
            assert!(
                (x - y).abs() < 1.5,
                "suppression across rates disagrees: {a:.2} / {b:.2} / {c:.2} dB"
            );
        }
    }

    /// **Thresholds are state**: they round-trip through
    /// `thresholds`/`set_thresholds` exactly, garbage in the array lands
    /// on OFF, and a fresh unit given a learned unit's numbers suppresses
    /// within half a dB of the original.
    #[test]
    fn thresholds_round_trip_as_the_state_they_are() {
        let dn = learned(0x1234, 0.05, SR, 1.0);
        let t = dn.thresholds();
        assert!(t.iter().any(|&v| v > 0.0), "learning produced an empty profile");

        let mut copy = Denoise::new(SR);
        copy.set_thresholds(&t);
        assert_eq!(copy.thresholds(), t, "the profile changed in the round trip");

        let mut garbage = t;
        garbage[0] = f32::NAN;
        garbage[1] = -5.0;
        garbage[2] = 7.0;
        let mut dn2 = Denoise::new(SR);
        dn2.set_thresholds(&garbage);
        let back = dn2.thresholds();
        assert_eq!(back[0], 0.0, "NaN did not land on off");
        assert_eq!(back[1], 0.0, "a negative threshold did not clamp");
        assert_eq!(back[2], 1.0, "an oversized threshold did not clamp");

        let suppress = |dn: &mut Denoise| {
            let frames = SR as usize;
            let mut noise = Noise::new(0x1234, 0.05, SR);
            let mut buf = noise.frames(frames);
            let in_db = rms_db(&buf, frames / 2);
            dn.process(&mut buf, frames, 2, &DenoiseParams::default());
            in_db - rms_db(&buf, frames / 2)
        };
        let mut orig = learned(0x1234, 0.05, SR, 1.0);
        assert!(
            (suppress(&mut orig) - suppress(&mut copy)).abs() < 0.5,
            "a restored profile does not suppress like the learned one"
        );
    }

    /// **Clear forgets what was playing and keeps what was learned**: after
    /// a clear the depths are zero, the lookahead line is silent, and the
    /// thresholds — settings, not signal — are exactly as frozen.
    #[test]
    fn clear_forgets_the_signal_and_keeps_the_profile() {
        let mut dn = learned(0x77, 0.05, SR, 1.0);
        let t = dn.thresholds();
        let frames = SR as usize / 2;
        let mut buf = Noise::new(0x77, 0.05, SR).frames(frames);
        dn.process(&mut buf, frames, 2, &DenoiseParams::default());
        assert!(dn.depth_db().iter().any(|&d| d > 1.0), "nothing was cutting before the clear");

        dn.clear();
        assert!(dn.depth_db().iter().all(|&d| d == 0.0), "a depth survived the clear");
        assert_eq!(dn.thresholds(), t, "the clear ate the learned profile");

        // The delay line is empty: silence in, silence out, immediately.
        let mut silent = vec![0.0_f32; 512 * 2];
        dn.process(&mut silent, 512, 2, &DenoiseParams::default());
        assert!(silent.iter().all(|&s| s == 0.0), "the lookahead line replayed old audio");
    }

    /// Mono is left alone rather than half-processed, and a third channel
    /// passes through a working stereo pair untouched.
    #[test]
    fn a_mono_buffer_and_a_third_channel_are_left_alone() {
        let mut dn = learned(0x9, 0.05, SR, 1.0);
        let mut mono: Vec<f32> = (0..256).map(|i| (i as f32 * 0.02).sin()).collect();
        let before = mono.clone();
        dn.process(&mut mono, 256, 1, &DenoiseParams::default());
        assert_eq!(mono, before, "a mono buffer was written to");

        let frames = 4_800;
        let mut dn = learned(0x9, 0.05, SR, 1.0);
        let mut buf = vec![0.0_f32; frames * 3];
        let mut noise = Noise::new(0x9, 0.05, SR);
        for (i, f) in buf.chunks_exact_mut(3).enumerate() {
            let v = noise.next();
            f[0] = v;
            f[1] = v;
            f[2] = (i as f32 * 0.01).sin();
        }
        let third: Vec<f32> = buf.chunks_exact(3).map(|f| f[2]).collect();
        dn.process(&mut buf, frames, 3, &DenoiseParams::default());
        let third_after: Vec<f32> = buf.chunks_exact(3).map(|f| f[2]).collect();
        assert_eq!(third_after, third, "the third channel was touched");
    }

    /// **While learning, no reduction is applied**: the learn pass itself
    /// is a clean wire (lookahead off) — the user must hear the noise they
    /// are teaching, raw.
    #[test]
    fn the_learn_pass_itself_is_a_wire() {
        let mut dn = Denoise::new(SR);
        let p = DenoiseParams {
            learn: 1.0,
            lookahead: 0.0,
            ..DenoiseParams::default()
        };
        let frames = SR as usize / 2;
        let mut buf = Noise::new(0xF00D, 0.2, SR).frames(frames);
        let before = buf.clone();
        dn.process(&mut buf, frames, 2, &p);
        assert_eq!(buf, before, "learning coloured the signal it was measuring");
    }
}
