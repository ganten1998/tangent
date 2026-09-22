//! The noise-reduction kernel for Tangent Denoise: TWO engines behind one
//! selector, sharing one learned 32-band profile.
//!
//! # Which one, and why there are two
//!
//! [`DenoiseKernel::SpectralStudio`] and [`DenoiseKernel::SpectralLive`]
//! are weighted overlap-add: a root-Hann frame, one FFT, a real
//! non-negative gain per band, one inverse FFT, overlap-add. They are the
//! default and they are what the panel's REDUCE dial is a promise about.
//! [`DenoiseKernel::Bank`] is the original 32 overlapping minimum-phase
//! bells, described below, kept because it is the only one that can be
//! bit-transparent and report zero latency.
//!
//! The split exists because of two measurements, both of them in this
//! file's tests.
//!
//! **The dial did not tell the truth, and could not be made to.** The bank
//! is a cascade of peaking cuts at Q 5.5 on a 0.26-octave grid, so their
//! skirts overlap and the composite is a SUM of what each band was asked
//! for. A uniform 12 dB ask landed at −21.2 dB at 100 Hz and −7.9 dB at
//! 16 kHz; the error was not an offset that could be trimmed out once,
//! because it GREW with the dial — the same measurement at 24 dB read
//! −50.0 and −17.4. Above 8 kHz the bank saturated near 10 dB and stopped
//! answering the dial at all: 18 dB → 24 dB bought 0.01 dB. That last
//! number is "I still hear white noise when I push it hard", written down.
//! The spectral kernels' band weights are a PARTITION of every bin rather
//! than an overlap, so equal band gains reconstruct to exactly that gain,
//! flat, at every bin.
//!
//! **A moving minimum-phase cut moves phase; a real gain on a bin does
//! not.** Multiplying a bin by a real non-negative scalar multiplies its
//! real and imaginary parts by the same number, so the argument comes back
//! untouched — an identity, not an approximation. A biquad's phase is tied
//! to its magnitude by the Hilbert relation and cannot be: measured, ~37°
//! of excursion for one bell, a 62.6° composite swing at 100 Hz as the
//! bank opens, and 4–17 dB of excess sidebands against the equivalent pure
//! magnitude cut. That is the other reported symptom.
//!
//! # What the two share
//!
//! One 32-band learned profile, persisted as the same `t0..t31` symbols it
//! always was. The kernels do not measure the same quantity through it —
//! the bank reads resonator band energy against a tone calibration, the
//! spectral path reads a windowed periodogram's per-bin level — so **a
//! profile belongs to the kernel that learned it**, and switching after a
//! learn is a re-learn.
//!
//! # The bank
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
//! Then, every control tick, the TARGETS are smoothed ACROSS the bank:
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
//! leave half the noise poking through. (The SPECTRAL kernels freeze a
//! GEOMETRIC mean into the same slots instead — see
//! [`Denoise::freeze_spectral_thresholds`] — which is the other half of why
//! a profile belongs to the kernel that learned it.) The thresholds are
//! ordinary state — [`Denoise::thresholds`] and [`Denoise::set_thresholds`]
//! move all 32 as normalized values so a session can persist them as
//! symbols.
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

/// Where the band CENTRES live, in Hz, log-spaced ends included. The edges
/// do not end the coverage: band 0 is a low SHELF (its cut extends to DC —
/// rumble and hum floor ride with it) and the top band is a high SHELF
/// (its cut extends to Nyquist). The first build stopped the bells at
/// 16 kHz and a beta ear caught it inside an hour: white noise is flat per
/// hertz, a third of its bandwidth lived above the last bell, and the
/// untouched top shelf of hiss read as "something missing frequency-wise"
/// however deep the bells cut.
pub const DENOISE_BAND_HZ: (f32, f32) = (60.0, 15_500.0);

/// The edge shelves' slope parameter (RBJ S). Under 1 keeps the knee
/// gentle enough that the shelf and its neighbouring bell sum without a
/// dip between them.
const DENOISE_SHELF_S: f64 = 0.9;

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

/// What the SMOOTH dial spans: the expansion ratio, geometric. 1.7:1 at the
/// bottom is a gentle tuck; 4.5:1 at the top closes a floor briskly without
/// being a gate. The first span topped out at 3:1 and its middle sat near
/// 2:1 — under which the noise never reached the REDUCE dial's depth and
/// the floor audibly FADED with the music instead of staying pinned. The
/// hysteresis-plus-hold pair is what guards against flapping; the ratio no
/// longer has to be timid on its behalf.
pub const DENOISE_RATIO: (f32, f32) = (1.7, 4.5);

/// What the RELEASE dial spans, in milliseconds, geometric: how long a cut
/// takes to re-engage once signal leaves a band. 60 ms breathes audibly on
/// speech pauses; 800 ms lets a whole bar of noise through after each note.
pub const DENOISE_RELEASE_MS: (f32, f32) = (60.0, 800.0);
/// **How fast a band OPENS** on the spectral kernels, across this range in
/// ms, geometric: the rise of a band's gain from the floor toward the signal
/// it has decided is there. The bank has its own fixed [`DENOISE_OPEN_MS`].
///
/// The shipped 10 ms is the owner's number. Measured on his take against the
/// reference render: the old kernel snapped a band from −12 dB to unity in
/// four frames, and every bit of room noise inside that band came up with
/// the note and dropped out after it — the "digital" sound, written down as
/// a gain that moved 12 dB in 25 ms and back again at every word.
pub const DENOISE_ATTACK_MS: (f32, f32) = (1.0, 100.0);
/// **The treble boost's reach**, in dB: the dial at the top of its travel is
/// this much lift on the SIGNAL above the shelf, and none on the residual.
pub const DENOISE_TREBLE_DB: f32 = 6.0;
/// Where the treble shelf begins to rise and where it has fully risen, Hz.
/// A denoiser dulls — the bands it cuts hardest are the ones where a room's
/// noise lives and a voice's air lives too — and a shelf on the part that
/// survives is the compensation; on the residual it would only be more
/// hiss, which is why the shelf rides the presence and not the output.
const TREBLE_SHELF_HZ: (f32, f32) = (2_500.0, 6_000.0);
/// **How far over its print a band must read before it starts to open, and
/// where it is fully open**, in dB of band energy over the band's own
/// learned floor — at SMOOTH's softest and at its hardest.
///
/// This is the second half of the digital sound. The band presence `p`
/// is a detection statistic that SATURATES: a bin 12 dB over its print
/// reads as signal at 0.99999 whether it holds a syllable or a chair, so a
/// chair opened its band to unity. Measured on the owner's take, events
/// 6–20 dB over the floor came through at −1 to −4 dB where the reference
/// held them at −5 to −7. The knee below puts a chair PART way open and a
/// note fully open, and the SMOOTH dial is how wide that knee is.
///
/// **Where the knee sits was measured, not guessed**, and the first guess
/// was wrong. Events up to about 12 dB over the print need no knee at all:
/// the broadband gate ([`VAD_QUIET_DB`]) reads such a frame as room, the
/// tracker absorbs the level within 50 ms and the decision-directed SNR
/// never rises, so the mixture lands under the floor and clamps to it —
/// traced frame by frame in `a_quiet_event_opens_a_band_only_part_way`'s
/// first draft. What came through on the take was the 15–20 dB events,
/// which a knee ending at 18 left fully open. So the knee starts where the
/// gate's programme side does and runs well past it: a 20 dB event opens
/// about two thirds of the way, a 30 dB note all the way.
pub const OPEN_SOFT_DB: (f32, f32) = (9.0, 27.0);
pub const OPEN_HARD_DB: (f32, f32) = (9.0, 15.0);

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
/// LEVEL envelope is symmetric and PER BAND, from 12 ms at the top of the
/// grid to 400 ms at the bottom (see [`LEVEL_BW_MS_HZ`] for why it has to
/// vary): an unbiased estimate of the same
/// statistic the learn pass measured, which is what the expander curve
/// must compare against the threshold — feed the curve the peak-riding
/// gate envelope instead and the learn margin (mean + 2σ) cancels out to
/// nothing, and so does the reduction.
const ENV_RISE_MS: f32 = 1.5;
const ENV_FALL_MS: f32 = 12.0;
const LEVEL_MS: f32 = 12.0;

/// The LEVEL envelope's time constant is per band, and this is what sets
/// it: `LEVEL_BW_MS_HZ / bandwidth`, floored at [`LEVEL_MS`] and capped at
/// [`LEVEL_MAX_MS`].
///
/// A band's energy estimate is a chi-square with about `2·B·τ` degrees of
/// freedom, so a FIXED τ gives the top band (12.9 kHz of detector
/// bandwidth) a quarter of a decibel of scatter and the bottom one (50 Hz)
/// several decibels of it. That scatter is not cosmetic: the expander's
/// slope multiplies it by ratio − 1, and the depth smoother is asymmetric
/// (2 ms toward less cut, the dialled release toward more), so it
/// RECTIFIES the scatter into a slow random walk. Measured on twelve
/// seconds of stationary noise at the top of the dial, band 0's
/// second-by-second mean wandered over 3.3 dB and never settled, while
/// band 16 and above sat still — the bank "never arriving" was always a
/// low-band statistics problem wearing a dynamics problem's clothes.
///
/// 20 s·Hz is about forty degrees of freedom, so every band's level
/// estimate carries roughly a decibel of scatter instead of a quarter of
/// one at the top and several at the bottom. It puts the 60 Hz band on the
/// 400 ms cap, band 8 at 159 ms, band 16 at 45 ms and everything above
/// about 1.7 kHz back on the fixed 12 ms — which is where the scatter was
/// already small and where nothing needed changing.
///
/// Only the statistic the STATIC CURVE reads is slowed. The gate envelope
/// that decides open-or-closed and protects transients is untouched, so
/// nothing about how fast a band opens for signal changes; what is slower
/// is only how fast a band that is already closed adjusts the DEPTH of its
/// cut, and a bass noise floor that moves faster than a third of a second
/// is not a noise floor. Going further has little left to buy: 40 s·Hz and
/// an 800 ms cap took the same measurement from 0.898 dB only to 0.813,
/// for twice the sluggishness.
const LEVEL_BW_MS_HZ: f32 = 20_000.0;
const LEVEL_MAX_MS: f32 = 400.0;

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

/// Below this a 0..=1 dial is at its resting position and means OFF —
/// same figure as the fixed chain's `OFF`, duplicated because this kernel
/// must stand alone.
const DENOISE_OFF: f32 = 0.0005;

/// Below this a sample is on its way to a denormal and is worth nothing.
const TINY: f32 = 1.0e-20;

/// Zero anything too small to hear, before it becomes expensive to add.
#[inline]
/// Hermite ease over 0..=1, clamped: the knee shape the opening rule and
/// the treble shelf both use, so the two curves are one curve.
fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn flush(x: f32) -> f32 {
    if x.abs() < TINY {
        0.0
    } else {
        x
    }
}

/// Zero anything that is not a number at all. A NaN or an infinity arriving
/// from upstream is not a quiet sample and not a loud one — it is the end of
/// every recursion it touches, in both kernels — so it becomes silence at
/// the door. See the call site in [`Denoise::process`] for why it is spent
/// there and not in one pass over the block.
#[inline]
fn flush_bad(x: f32) -> f32 {
    if x.is_finite() {
        x
    } else {
        0.0
    }
}

// ── which kernel is in the circuit ──────────────────────────────────────────

/// How many positions the KERNEL selector has. Spelled as a literal so the
/// face's `ParamSpec` table stays `const`; a test pins it to
/// [`DenoiseKernel::ALL`].
pub const DENOISE_KERNEL_STEPS: i32 = 3;

/// Which of the two engines runs. The dial is appended at the END of the
/// face's table because ids and symbols are persisted forever, and a
/// session written before this parameter existed must keep meaning what it
/// meant — which, since a missing value reads as the spec's default, is
/// **Spectral Studio**. That is a deliberate change of sound for old
/// sessions and it is the right one: the bank never delivered the dial it
/// promised (see [`DenoiseKernel::Bank`]), so "the setting you saved" was
/// never the reduction you heard.
///
/// The two families are not two flavours of the same thing. They differ in
/// what a gain change does to PHASE, which is the whole of the owner's
/// first symptom:
///
/// * A real, non-negative gain on an FFT bin leaves phase mathematically
///   untouched — the same scalar multiplies the real and the imaginary
///   part, so the argument of the complex number is an identity, not an
///   approximation.
/// * A minimum-phase biquad cannot do that. Its phase is tied to its
///   magnitude by the Hilbert relation, so a bell that moves its depth
///   moves its phase: measured, ~37° of excursion for ONE bell and a 62.6°
///   composite swing at 100 Hz as the bank opens, with 4–17 dB of excess
///   sidebands against the equivalent pure-magnitude cut.
///
/// So the spectral path is the future and the bank is kept for the one
/// thing it does that no overlap-add can: with the lookahead toggle down
/// and nothing learned it is BIT-transparent and reports zero latency.
/// That is a real shipped capability with a passing test, and a unit that
/// can be inserted for free on every strip is worth keeping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenoiseKernel {
    /// The 32 overlapping minimum-phase bells. Zero latency with the
    /// lookahead down, bit-transparent when it is doing nothing, and the
    /// only choice that can claim either. Its faults are measured and
    /// documented in this file's tests and are not tunable away: the
    /// composite of 32 overlapping bells is a SUM, so a uniform 12 dB ask
    /// lands at −21.2 dB at 100 Hz and −7.9 dB at 16 kHz, and the error
    /// grows with the dial rather than sitting still to be trimmed out.
    Bank,
    /// Weighted overlap-add, n = 1024 at 44.1/48 kHz (2048 at 88.2/96,
    /// 4096 at 176.4/192 — the frame lasts the same 21 ms at every rate).
    /// 21 ms of latency, reported honestly, and a gain that is a real
    /// scalar per bin.
    SpectralStudio,
    /// The same kernel at half the frame: n = 512 at 44.1/48 kHz, so
    /// 10.7 ms of latency for tracking through the unit. Half the frame is
    /// half the frequency resolution — 94 Hz bins at 48 kHz — so the low
    /// bands get coarser and the profile learned at one length is not the
    /// profile the other wants. Learn with the kernel you will play.
    SpectralLive,
}

impl DenoiseKernel {
    /// Selector order, and therefore the order the face's cycling row and
    /// every persisted value use. APPEND-ONLY, like the id table itself.
    pub const ALL: [DenoiseKernel; 3] = [
        DenoiseKernel::Bank,
        DenoiseKernel::SpectralStudio,
        DenoiseKernel::SpectralLive,
    ];

    /// What the panel calls it.
    pub fn label(self) -> &'static str {
        match self {
            DenoiseKernel::Bank => "Bank",
            DenoiseKernel::SpectralStudio => "Spectral Studio",
            DenoiseKernel::SpectralLive => "Spectral Live",
        }
    }

    /// Decode the normalised dial: N positions across the travel, nearest
    /// wins, exactly the menu convention the EQ's shape knob uses.
    pub fn from_dial(v: f32) -> Self {
        let last = (Self::ALL.len() - 1) as f32;
        let at = ((v.clamp(0.0, 1.0) * last).round() as usize).min(Self::ALL.len() - 1);
        Self::ALL[at]
    }

    /// Where this position sits on the 0..=1 dial — the inverse of
    /// [`DenoiseKernel::from_dial`], so a default can be written as a
    /// kernel rather than as a magic number.
    pub fn dial(self) -> f32 {
        let at = Self::ALL.iter().position(|k| *k == self).unwrap_or(0);
        at as f32 / (Self::ALL.len() - 1) as f32
    }
}

// ── the spectral kernel's constants ─────────────────────────────────────────

/// The block the host may hand us in one call, restated here because this
/// kernel stands alone — the same 512 the engine caps at and the same
/// figure `builtin_fx`'s scratch is cut to. It only sizes the output FIFO's
/// headroom; nothing breaks at a larger block, because `process` chunks the
/// buffer by hop anyway.
const SPECTRAL_MAX_BLOCK: usize = 512;

/// Where the 32nd band's centre goes, in Hz, and therefore where the first
/// 31 bands have to reach. Above it band 31 answers for everything out to
/// Nyquist — the same "the edge band owns the rest of the spectrum" rule
/// the bank's shelves follow, and for the same reason: hiss does not stop
/// where a grid does. Held under Nyquist at low rates so the walk always
/// has somewhere to land.
const SPECTRAL_TOP_HZ: f64 = 20_000.0;

/// The narrowest a band may be, in BINS — the floor under the ERB walk in
/// [`Spectral::lay_out_bands`], and the number the two properties the band
/// structure exists to buy actually depend on.
///
/// It was 2, and 2 was not enough. Measured at 48 kHz: the bottom SEVEN
/// spacings of Spectral Studio and the bottom TEN of Spectral Live came out
/// at exactly two bins, which is where three things went wrong at once.
///
/// * **The band energy is a chi-square with about `2·B` degrees of freedom
///   per frame.** A two-bin band is worth about 1.5 bins of weight, so its
///   γ scatters over several dB, a hot frame pushes `(γ−1)` through the
///   decision-directed recursion, and the band briefly stops answering the
///   dial. Measured per frame over the settled half of an eight-second run
///   at the top of the dial, band 0's peak-to-peak was **13.02 dB** at
///   Studio and 5.26 dB at Live, with 0.117 % of Studio band-frames more
///   than 1 dB short of what REDUCE promised. The settled snapshot the
///   acceptance test reads was flat to 0.00 dB the whole time, because a
///   snapshot cannot see a frame it did not land on.
/// * **The realised mask was not slope-limited down there.** The doc below
///   argues that interpolating across triangles constrains how fast the
///   mask can change with frequency; across two bins it constrains almost
///   nothing. With signal present the biggest bin-to-bin step measured
///   13.27 dB (Studio) and 10.33 dB (Live) — free per-bin behaviour, in the
///   region the argument is about.
/// * **The aliasing figure was not one number.** The same measurement of
///   the realised mask's circular impulse response read −23.5 dB at Studio
///   and −16.1 dB at Live: a 7.4 dB spread between two frame lengths that
///   are supposed to differ only in resolution.
///
/// At four bins all three read differently: band 0's per-frame
/// peak-to-peak is 1.05 dB at Studio and 0.00 at Live (0.00 at both once
/// [`DD_BW_MS_HZ`] is in), the worst bin-to-bin step is 6.57 and 5.44 dB,
/// and the aliasing is −30.6 and −33.4 dB — better at both lengths and
/// within 2.8 dB of each other. The cost is resolution at the bottom: band
/// 1's centre moves from 94 Hz to 188 Hz at Studio and from 188 to 375 at
/// Live. That is the honest trade, and it is the right way round — a band
/// too narrow to measure is not resolution, it is noise with a centre
/// frequency.
///
/// Bins, not hertz, deliberately. The scatter, the slope limit and the
/// aliasing all scale with the frame, so a fixed BIN count gives the two
/// kernels the same behaviour; a fixed frequency would give Live half the
/// degrees of freedom and twice the aliasing.
const MIN_BAND_BINS: f64 = 4.0;

/// The a-priori SNR the speech-presence test is tuned for, as a power
/// ratio: 15 dB. Gerkmann & Hendriks' fixed ξ_H1, and the two constants
/// below are algebraic consequences of it, precomputed so the hot loop
/// does not.
const SPP_XI_H1: f32 = 31.62;

/// `ln(1 / (1 + SPP_XI_H1))` — the constant part of the log generalised
/// likelihood ratio.
const LOG_GLR_FACT: f32 = -3.485;

/// `SPP_XI_H1 / (1 + SPP_XI_H1)` — what multiplies the a-posteriori SNR in
/// the same ratio.
const GLR_EXP: f32 = 0.9693;

/// The log-GLR is exponentiated, so it is clamped first. 200 is far past
/// where the probability has saturated at 1 and comfortably inside f32's
/// range; without it one enormous bin overflows to infinity and the
/// probability comes back NaN.
///
/// **The clamp is applied with a branch, not with `f32::min`, and that
/// sentence is load-bearing.** `f32::min` returns THE OTHER OPERAND when
/// one side is NaN — `NAN.min(200.0)` is `200.0`, measured — so the obvious
/// spelling of this clamp does the exact opposite of what the paragraph
/// above claims: handed a NaN it answers 200, 200 gives `ph1 == 1.0`
/// exactly, and `(1.0 − ph1) · p` is then `0.0 · inf`, which is a NaN
/// stored into the noise estimate for the life of the session. That was the
/// shipped fault. See the call site in [`Denoise::spectral_suppress`].
const GLR_CLAMP: f32 = 200.0;

/// The stagnation cap. Once the smoothed probability of speech in a bin has
/// sat near 1 for a while, the noise estimate stops updating in that bin
/// FOREVER — the estimate is frozen by its own confidence. Capping the
/// instantaneous probability at 0.99 leaves 1 % of every frame flowing into
/// the estimate, which is enough to unstick it. This is NOT optional and
/// removing it does not fail loudly: it just locks the estimate at whatever
/// it happened to hold, and the unit slowly stops working on sustained
/// material.
const SPP_STAGNATION_CAP: f32 = 0.99;

/// How long the smoothed speech-presence probability remembers, and how
/// long the noise PSD estimate does. Stored as TIME, never as the per-frame
/// coefficients: the coefficients are derived from the hop at the running
/// rate, which is what makes a 1024-point frame at 48 kHz and a 2048-point
/// frame at 96 kHz behave identically.
const TAU_PH1MEAN_MS: f32 = 152.0;
const TAU_LAMBDA_MS: f32 = 71.7;

/// The decision-directed a-priori SNR's two weights, as time constants. The
/// classical estimator uses one fixed weight and pays for it either in
/// musical noise (weight too low) or in smeared transients (too high); the
/// weight here interpolates between them by how much signal the band
/// already believes it has, so a noise-only band forgets over a quarter of
/// a second and a band with a note in it forgets in 8 ms.
const TAU_DD_NOISE_MS: f32 = 250.0;
const TAU_DD_SIGNAL_MS: f32 = 8.0;

/// The NOISE-side time constant is PER BAND, and this is what sets it:
/// `DD_BW_MS_HZ / bandwidth`, floored at [`TAU_DD_NOISE_MS`] and capped at
/// [`TAU_DD_NOISE_MAX_MS`]. The bank's [`LEVEL_BW_MS_HZ`] is the same
/// argument in the other engine, and it is here for the same reason.
///
/// A band's energy estimate is a chi-square with about `2·B·τ` degrees of
/// freedom, so a FIXED τ hands the bottom bands — the narrowest ones, even
/// with [`MIN_BAND_BINS`] under them — several times the scatter the top
/// bands carry. The decision-directed recursion then RECTIFIES that
/// scatter: `(γ−1)` is clamped at zero from below and not from above, so a
/// hot frame raises ξ and a cold one cannot lower it past the floor, and
/// the band spends the next few frames believing it has signal in it. That
/// is the whole of band 0's per-frame wobble; see [`MIN_BAND_BINS`] for the
/// numbers. 120 s·Hz puts Studio's band 0 on the 1 s cap, Live's band 0 at
/// 512 ms and everything above about 480 Hz of bandwidth back on the fixed
/// 250 ms, where the scatter was already small — and because the figure is
/// per HERTZ, the two frame lengths end up with the same degrees of freedom
/// rather than Live with half.
///
/// Only the NOISE side is slowed. `tau` still interpolates to
/// [`TAU_DD_SIGNAL_MS`] by `q²` the moment a band believes it has signal,
/// so nothing about how fast a band opens changes — the same discipline
/// [`LEVEL_BW_MS_HZ`] keeps against the bank's gate envelope. Nor does it
/// slow the re-close: when signal stops, `s/λ` collapses as `g_H1²`, which
/// squares itself away in about five frames whatever `a` is.
const DD_BW_MS_HZ: f32 = 120_000.0;
const TAU_DD_NOISE_MAX_MS: f32 = 1_000.0;

/// The ceiling on the decision-directed recursion's HISTORY term, `s/λ`.
///
/// `γ` one line above it is already clamped to the same figure and for the
/// same reason; this is the half that was missed. 30 dB of a-priori SNR is
/// `g_H1 = 0.9990`, so anything past it is a number, not a decision — and
/// without the clamp an interior band whose profile is OFF reaches
/// `s/λ ≈ 7·10⁵` (its λ is the −100 dB placeholder [`refloor`] lays for a
/// band nobody taught, not a measurement), which is a quarter of the way to
/// pinning its live neighbours' gain at unity through the cross-band vote.
/// The vote now skips OFF bands outright, so this is belt and braces —
/// stated as such, because a guard that cannot be reverted to red is worth
/// only what its comment says.
const XI_HIST_MAX: f32 = 1_000.0;

/// The floor under the a-priori SNR, in dB. This is the constant that makes
/// the REDUCE dial tell the truth: it pins the deepest the speech-present
/// gain can ever go at ξ/(1+ξ) = −40.1 dB, which is below the bottom of the
/// dial (24 dB), so in a noise-only frame the mixed gain is ALWAYS at or
/// under the dialled floor and the floor's own clamp lands it exactly ON
/// the dial in every band. Raise this and the top of the dial stops being
/// reachable.
const XI_MIN_DB: f32 = -20.0;

/// How far above the learned profile the tracked noise estimate may climb.
/// The floor is one-way — the estimate may never fall below what the user
/// taught it — and this is the other side: a noise print is a measurement,
/// and an estimate that has wandered this far above it is tracking the
/// PROGRAMME, not the noise.
///
/// # Why 24, and why it used to be 18
///
/// It was 12 first, and 12 put a CLIFF inside the range of "the room got a
/// bit louder than when I taught it". Past the ceiling λ pins, γ stays
/// large, ξ stays large, `g_H1 → 1` and every band opens: measured with the
/// profile learned and the same noise played back hotter, at a dial asking
/// 12 dB, the delivered cut went 12.00 dB at +16 dB to **4.07 dB at +18**
/// and 1.71 dB at +20. That is the exact symptom this whole unit was
/// commissioned for — the user pushes REDUCE and almost nothing happens —
/// arriving without a sound and without a warning. 18 moved that cliff to
/// +20 and there it sat.
///
/// The ceiling is not decoration: it is what stops the tracker following
/// PROGRAMME and cutting signal, so widening it is paid for in the tone
/// test. Measured, a −20 dBFS 1 kHz tone over learned noise (bound 1.5 dB):
///
/// ```text
///     ceiling   tone moves (Studio / Live)   full dial holds to
///        12 dB      −0.04 / −0.10 dB              +16 dB
///        18 dB      −0.09 / −0.23 dB              +20 dB
///        24 dB      −0.32 / −0.70 dB              +24 dB
///        30 dB      −1.04 / −1.27 dB              past +24 dB
/// ```
///
/// **18 only became the binding number the day [`VAD_QUIET_DB`] let the
/// tracker actually climb.** While the estimate was frozen by its own
/// speech-presence probability this ceiling was very nearly theoretical: on
/// the challenge take, sweeping it from 18 to 60 dB moved every number the
/// scorer prints by 0.01 dB or less, because λ never came within sight of
/// it. With the broadband gate in, λ goes where the room actually is — and
/// on that take the room between words sits 6.4 dB over its print in the
/// mids at the median and past 12 dB in a quarter of the frames. Measured
/// on `~/Desktop/challenge/dry.wav` at the shipped dial with the gate at
/// 9–15 dB:
///
/// ```text
///     ceiling   BATHROOM HUMP   residual flatness   voice down
///        18 dB      +0.74 dB          0.2136          −4.33 dB
///        20 dB      +0.59             0.2210          −4.37
///        22 dB      +0.51             0.2261          −4.39
///        24 dB      +0.45             0.2298          −4.40
///        30 dB      +0.41             0.2322          −4.43
/// ```
///
/// The curve flattens: 18→24 buys 0.29 dB of tilt, 24→30 buys 0.04. 24 is
/// where it stops paying, and it costs almost nothing on the take (0.07 dB
/// of voice against 18) because the gate, not the ceiling, is what decides
/// whether λ moves at all. It is also the row the tone table above already
/// measured, so the six decibels beyond 18 cost 0.23 dB more of a −20 dBFS
/// tone out of a 1.5 dB budget.
///
/// **How much room this number has is decided by the GATE, not by itself.**
/// At the shipped gate of 9–15 dB a −20 dBFS tone over learned noise holds
/// its frames on the programme side, λ never comes near this ceiling, and
/// `a_tone_survives_the_suppression_that_kills_the_noise` passes at every
/// value tried from 18 up to 30. Widen the gate to 9–18 and the tone's
/// frames start reading as room: λ climbs to whatever this number allows
/// and the tone comes back **1.94 dB** down at a ceiling of 24. Widen it
/// to 15–21 and the same ceiling costs **11.98 dB** of it. The two
/// constants are only independent while the gate stays clear of the tone,
/// and the note under [`VAD_QUIET_DB`] is where that margin is kept.
///
/// It is still a cliff — it has moved, not tapered, and
/// `a_stale_print_holds_the_dial_until_it_does_not` writes down where it
/// now stands so the next person changing this number is arguing with a
/// measurement. A genuine taper wants a minimum-statistics tracker that
/// does not reference the print at all, which is a different unit's worth
/// of work.
const NOISE_CEIL_DB: f32 = 24.0;

/// **The two ends of the broadband speech-presence gate**, in dB of whole-
/// frame level above the whole PRINT — the sum of the per-bin floors the
/// learn pass laid down. At or below [`VAD_QUIET_DB`] the frame is room and
/// every bin's noise estimate is allowed to hear it in full; at or above
/// [`VAD_SPEECH_DB`] the frame is programme and the per-bin probability
/// decides alone, exactly as it did before. Between them the weight ramps
/// linearly.
///
/// # The measurement that put them there
///
/// The owner's word for the shipped sound was "a crispy bathroom sounding
/// frequency that sounds like a digital artifact", set against a render of
/// the same take by a tool he likes. That sentence is two numbers — the
/// residual's mid-band tilt between words, and how flat the residual is —
/// and both said the same thing. Between words we took 11.3 dB out of the
/// low bands and the air and 8.5 out of 800 Hz–5 kHz, which leaves a
/// mid-forward floor, which is what a small tiled room sounds like; and the
/// residual came back markedly PEAKIER than the noise it removed (0.183
/// against the source's 0.268) where the reference left one slightly
/// flatter (0.270).
///
/// The cause was never the amount — both tools take about 7 dB out once
/// makeup gain is removed. The cause is that the mid band spent **a third
/// of the between-word frames wide open**: per-frame gain better than −6 dB
/// in 32.8 % of them, against the reference's 7.2 %. And the reason it did
/// is that on that take the room between words sits **6.4 dB over its own
/// learned print at the median, and more than 12 dB over it in 27 % of
/// frames**, in the mids alone — the low band and the air sit within about
/// a decibel of theirs. The mids are where a room's low-level activity
/// lives (breath, mouth, clothing, a chair), and a noise print taken over
/// the quietest half-second of a take does not describe it.
///
/// The tracker is supposed to climb to meet exactly that, and could not.
/// The speech-presence probability is a PER-BIN SNR test against a fixed
/// 15 dB ([`SPP_XI_H1`]), so a bin sitting 12 dB over its print reads as
/// speech at `p = 0.99999` whether it is a word or a chair. λ froze at the
/// print, γ stayed at +12 dB, ξ stayed large, and the band rode open.
/// Measured over the frames more than half a second from anything the
/// analysis calls speech — 75 % of the between-word mid energy — we
/// delivered −8.55 dB where the reference delivered −12.88.
///
/// A per-bin test cannot fix this, because in one bin a chair and a
/// syllable look the same. A frame that is quiet ACROSS THE WHOLE SPECTRUM
/// is not a syllable, and that is the whole of this gate.
///
/// # Why the reference is the whole print, and not λ and not a minimum
///
/// The comparison is the frame's total periodogram against Σ`floor` — the
/// print the user taught, summed across the spectrum, laid down once per
/// profile in [`Denoise::refloor`] rather than per frame.
///
/// **A gate is only as good as the line it is judged against, and the line
/// has to be one the gate cannot move.** The first draft compared the frame
/// against Σλ, the tracker's own current belief, which is a feedback loop
/// with a fixed point in the wrong place: a −20 dBFS tone over learned
/// noise half-opens the gate, λ climbs a little, the frame's excess over λ
/// drops, the gate opens further. Measured, it settles at 8.65 dB of excess
/// (Studio) with λ pinned to its ceiling and the tone **1.96 dB down**
/// against a 1.5 dB budget. That is the right failure to have found early:
/// an estimator that can eat a steady tone can eat a held note.
///
/// It is not a long-window broadband MINIMUM either, which scores about the
/// same on the tilt and fails the same way — a running minimum over
/// 1.5–2 s puts the reference at the take's own quietest bar, and a
/// two-second held tone is then quiet by its own measure.
///
/// Against the print the gate has the property that matters: **it can only
/// ever let the tracker hear MORE, never less.** The weight multiplies a
/// probability that is already there, and a product with something in
/// 0..=1 is at or below it, so no case that worked before can stop working
/// — and because the denominator is fixed by the profile, nothing the gate
/// does to the estimate can move the line it is judged against.
///
/// And the print is the right line even though the print is what was wrong,
/// because the fault is per BAND and the sum is not. On the challenge take
/// the mid print under-reads the room by 6–12 dB, but the frame TOTAL is
/// carried by the low band and the air, where the same print is accurate to
/// about a decibel. That is the whole reason to ask a broadband question.
///
/// # Why 9 and 15, and what actually sets the top
///
/// On the challenge take the two populations are miles apart. Frame level
/// over the whole print, taken with the analysis' own masks: the quiet 30 %
/// of frames run 0.2 dB at the tenth percentile to 11.3 dB at the
/// ninety-ninth, and the loud 15 % run 30.8 to 36.3. Nineteen decibels of
/// empty air between them, and no threshold anywhere in that gap changes
/// which side a noise frame or a voice frame lands on. What the top end
/// really decides is the 55 % of frames that are NEITHER — onsets, tails,
/// breaths, half-syllables — and those are spread right through it. Swept
/// at the shipped dial with [`NOISE_CEIL_DB`] at 24:
///
/// ```text
///     gate           BATHROOM HUMP   flatness   voice down   TRUE reduction
///     none (shipped)     +3.24 dB     0.1784     −3.79 dB       −6.81 dB
///      8–14 dB           +0.52        0.2225     −4.33          −7.31
///      9–15 dB           +0.45        0.2298     −4.40          −7.30
///     10–16 dB           +0.38        0.2375     −4.51          −7.26
///     12–18 dB           +0.28        0.2477     −4.76          −7.08
///     15–21 dB           +0.21        0.2578     −5.02          −6.89
///     the reference      +0.48        0.2704     −6.15          −6.65
/// ```
///
/// Every row of that table is inside the reference tool's own voice cost,
/// so the voice is not what stops us going higher. The TONE is. **A
/// −20 dBFS 1 kHz tone over the noise this file's tests learn on holds the
/// frame only 17.5 dB above the print** — a musical signal at 17 dB SNR is
/// a quiet one, and a gate calibrated for speech at 30 dB will happily call
/// its frames room. Measured by running the whole suite at each setting:
/// 9–15 passes at every ceiling tried up to 30 dB, 9–16 passes at 24, and
/// **9–18 fails, the tone 1.94 dB down against a 1.5 dB budget** — because
/// a weight of 0.94 still leaks six percent of a very confident bin's
/// observation into λ every frame, and two seconds of that is plenty.
///
/// So the top is set by the tone, not by the take: 15 dB is 2.5 dB clear of
/// the tone's frames and 15 dB clear of the take's speech. What that costs
/// is legible in the table — 9–15 gives up 0.24 dB of tilt and 0.028 of
/// flatness against 15–21 — and it is the right way round, because the tilt
/// target is met either way and a signal that dies is not.
///
/// A six-decibel RAMP rather than a step, because a step makes the
/// tracker's speed a function of which side of one number a frame landed
/// on, and the frames that decide this are the ones nearest the line.
const VAD_QUIET_DB: f32 = 9.0;
const VAD_SPEECH_DB: f32 = 15.0;

/// A learn pass shorter than this is a misclick, not a noise profile, and
/// must not wipe a good one. 200 ms, up from the 10.7 ms this file used to
/// spend: a tenth of a second is a mouse bounce, and both kernels' learned
/// statistics are averages that mean nothing over one.
const MIN_LEARN_MS: f32 = 200.0;

/// Below this a band energy is treated as silence when its logarithm is
/// taken. The learn pass is a GEOMETRIC mean, so a single frame of true
/// digital silence would otherwise contribute −∞ and take the whole band
/// with it.
const LN_FLOOR: f64 = 1.0e-12;

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
    /// reads ahead; [`Denoise::latency`] tells the truth about it. The
    /// spectral kernels IGNORE it — a frame already looks n samples wide
    /// and there is nothing left to look ahead at.
    pub lookahead: f32,
    /// Which engine, across [`DenoiseKernel::ALL`], nearest position wins.
    /// **Appended last, and it stays last**: the id and symbol tables are
    /// persisted forever, so a new parameter takes a new number at the end
    /// of the road rather than moving anything already written down.
    pub kernel: f32,
    /// **How fast a band opens** on the spectral kernels, across
    /// [`DENOISE_ATTACK_MS`], geometric. Appended after `kernel`, as every
    /// parameter after the first seven must be: ids and symbols are
    /// persisted forever. Ignored by the bank, which has [`DENOISE_OPEN_MS`].
    pub attack: f32,
    /// **Treble boost** on the signal that survives, across
    /// 0..[`DENOISE_TREBLE_DB`], linear. Never on the residual — see
    /// [`TREBLE_SHELF_HZ`]. Appended after `attack`. Ignored by the bank.
    pub treble: f32,
}

/// The shipped sound: nothing learned yet (a wire), 12 dB cap, the
/// SOFTEST knee, a 10 ms attack and a 100 ms release, 2 dB of treble on
/// what survives, detector hearing everything, lookahead on — and the
/// SPECTRAL STUDIO kernel, because it is the one that delivers the dial the
/// panel is promising. The bank is a deliberate choice, not the default one.
///
/// The four numbers are the owner's (2026-09-16): "Attack=10ms
/// Release=100ms Smooth=0 Treble Boost = 2 dB". `release` is 100 ms on the
/// geometric [`DENOISE_RELEASE_MS`] sweep, which
/// `the_shipped_release_is_a_hundred_milliseconds` pins.
impl Default for DenoiseParams {
    fn default() -> Self {
        Self {
            learn: 0.0,
            reduce: 0.5,
            smooth: 0.0,
            release: 0.197,
            sc_hpf: 0.0,
            lookahead: 1.0,
            kernel: 0.5,
            attack: 0.5,
            treble: 2.0 / DENOISE_TREBLE_DB,
        }
    }
}

#[cfg(test)]
impl DenoiseParams {
    /// Read any knob this build knows from `TANGENT_DN_<NAME>`, for the
    /// offline render harness. Kept beside the struct so a knob added here
    /// is added here too.
    fn apply_env_overrides(&mut self, dial: &dyn Fn(&str, f32) -> f32) {
        self.sc_hpf = dial("TANGENT_DN_SC_HPF", self.sc_hpf);
        self.lookahead = dial("TANGENT_DN_LOOKAHEAD", self.lookahead);
        self.attack = dial("TANGENT_DN_ATTACK", self.attack);
        self.treble = dial("TANGENT_DN_TREBLE", self.treble);
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
    /// −1: this band is the low SHELF (cut extends to DC). +1: the high
    /// SHELF (cut extends to Nyquist). 0: an ordinary bell.
    shelf: i8,
    /// True when the centre sits too close to Nyquist to design a bell —
    /// only reachable at absurdly low sample rates; the band then never
    /// cuts.
    inert: bool,
    /// This band's LEVEL smoothing coefficient — per band, because the
    /// confidence of an energy estimate depends on the bandwidth it was
    /// taken over. See [`LEVEL_BW_MS_HZ`].
    a_level: f32,
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
    /// **What the BANK gates on**, in dB. Equal to `thr_db` for a print the
    /// bank learned; for one a spectral kernel learned it is the bank's own
    /// measurement of the same noise, taken by `learn_feed` during the
    /// pass, because the two kernels measure a band differently and a
    /// spectral print read as a bank knee sat in the middle of the noise -
    /// the owner's "learn on bank then switch works, but not vice versa".
    /// Persisted beside the print as `k0..k31`; see `Denoise::knees`.
    knee_db: f32,
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
        knee_db: DENOISE_THR_DB.0,
        n: 0.0,
        sum: 0.0,
        sumsq: 0.0,
    };

    /// Forget what was playing, keep what was learned.
    fn quiesce(&mut self) {
        let (thr_norm, thr_db, knee_db) = (self.thr_norm, self.thr_db, self.knee_db);
        *self = Band::IDLE;
        self.thr_norm = thr_norm;
        self.thr_db = thr_db;
        self.knee_db = knee_db;
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

// ── the transform ───────────────────────────────────────────────────────────

/// In-place radix-2 complex FFT, f32 — lifted verbatim from
/// `units::convolver.rs`, which is the house's audio-path transform, because
/// a unit stands alone and the alternative was worse. The one in
/// `tangent-ui/src/faces.rs` looks closer to hand and must NOT be used here:
/// it is forward-only, it advances its twiddles by recurrence with no
/// re-seed across the whole transform, and it lives on the far side of the
/// UI/audio firewall — a display's FFT may drift a degree and nobody hears
/// it, and nothing in the audio path may depend on a crate the audio path
/// does not own.
///
/// f32 is enough. At 4096 points, the longest frame any supported rate asks
/// for, the forward-inverse roundtrip error sits near −130 dB, which is
/// under the dither of the file this unit is cleaning.
fn fft(re: &mut [f32], im: &mut [f32], inverse: bool) {
    let n = re.len();
    if n < 2 || n & (n - 1) != 0 || im.len() != n {
        return;
    }
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = std::f32::consts::TAU / len as f32 * if inverse { 1.0 } else { -1.0 };
        let (wr, wi) = (ang.cos(), ang.sin());
        let mut i = 0;
        while i < n {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (ur, ui) = (re[i + k], im[i + k]);
                let (vr, vi) = (
                    re[i + k + len / 2] * cr - im[i + k + len / 2] * ci,
                    re[i + k + len / 2] * ci + im[i + k + len / 2] * cr,
                );
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + len / 2] = ur - vr;
                im[i + k + len / 2] = ui - vi;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
            i += len;
        }
        len <<= 1;
    }
    if inverse {
        let s = 1.0 / n as f32;
        for v in re.iter_mut() {
            *v *= s;
        }
        for v in im.iter_mut() {
            *v *= s;
        }
    }
}

// ── the weighted overlap-add scaffolding ────────────────────────────────────

/// The frame machinery the spectral kernel runs inside: staging, windows,
/// the transform pair and the output FIFO. It is written and PROVEN as a
/// pure delay first — with the gain array left at one it must return the
/// input, sample for sample, to within f32 rounding — because every
/// measurement the suppression makes is a measurement THROUGH this, and a
/// wrong scaffold makes every later number a lie.
///
/// # The window, and why there is no normalisation array
///
/// Root-Hann, `w[i] = sin(π·(i + ½)/n)`, at BOTH analysis and synthesis, at
/// 75 % overlap. The periodic (half-sample-offset) form is the one that
/// matters: `w²` is then a periodic Hann, and a periodic Hann summed at hop
/// n/4 is EXACTLY 2 — the four `½` terms give 2 and the four cosine terms
/// are `cos(θ)`, `cos(θ + π/2)`, `cos(θ + π)`, `cos(θ + 3π/2)`, which cancel
/// in pairs for every θ. So synthesis normalisation is one multiply by 0.5.
/// No accumulated-window array, no per-sample divide, no guard against a
/// small denominator at the edges — none of that machinery exists here
/// because the arithmetic makes it unnecessary, and a test pins
/// `Σw² = 2.000000` to a part in a million so it stays that way.
///
/// # The staging, and why the latency is n and not n − R
///
/// The buffers follow the convolver's pattern: an input ring of n stereo
/// frames, a fill counter, an overlap-add accumulator of n stereo frames,
/// and an output FIFO. Input arrives in chunks of `min(remaining, R −
/// fill)` — never one sample at a time — and a frame fires whenever the
/// fill reaches the hop.
///
/// Frame m, fired once (m+1)R samples have been consumed, covers input
/// `[(m+1)R − n, (m+1)R)`. A sample t is touched by exactly n/R = 4 frames
/// and is finished by frame `⌊(t + n)/R⌋ − 1`, so after frame m every
/// sample `t < (m+2)R − n` is complete: that is R newly finished samples per
/// frame, and after m+1 frames exactly (m+1)R samples have been emitted,
/// the first of them standing for input time R − n. Emit them as they come
/// and the delay is n − R.
///
/// The FIFO is nevertheless primed with R frames of silence, and the
/// reported latency is n. The reason is that a host block boundary can land
/// anywhere. With T = n + mR + r samples handed in, m+1 frames have fired
/// and (m+1)R samples have been flushed, so n − R + r remain unconsumed —
/// and r is whatever the host chose, up to R − 1, which puts the worst case
/// at n − 1. A latency figure that is only true when blocks divide evenly
/// by 256 is not a latency figure; n is the block-agnostic bound, the
/// priming buys exactly it, and an impulse test measures it rather than
/// arguing it.
struct Spectral {
    /// The frame length in samples and the hop, n/4. Both zero until the
    /// first [`Spectral::retune`].
    n: usize,
    hop: usize,
    /// Root-Hann, `n` live entries; the tail of the Vec belongs to longer
    /// frames this rate could be asked for.
    win: Vec<f32>,
    /// The last n stereo frames, interleaved, written circularly. `wpos` is
    /// the NEXT write, which is also the oldest sample.
    in_ring: Vec<f32>,
    wpos: usize,
    /// How much input has arrived toward the next frame.
    stage_fill: usize,
    /// Overlap-add, n stereo frames, circular. `ap` is where the next
    /// frame's first sample lands, which is also the oldest complete
    /// sample.
    acc: Vec<f32>,
    ap: usize,
    /// Finished output, interleaved, a ring of `n + SPECTRAL_MAX_BLOCK`
    /// frames — far more than the R the occupancy provably stays under,
    /// which is the convolver's habit and costs a few kilobytes.
    fifo: Vec<f32>,
    out_head: usize,
    out_len: usize,
    /// Transform scratch. The stereo pair is packed as one complex signal,
    /// `l + j·r`, so `re` carries left and `im` carries right on the way in
    /// and on the way back out.
    re: Vec<f32>,
    im: Vec<f32>,
    /// The gain applied to each bin, full length and conjugate-symmetric.
    /// Left at one by the scaffolding; the suppression writes it.
    gain: Vec<f32>,
    /// Per bin, 0..=n/2: which band it mostly belongs to and how far it
    /// has travelled toward the next one. Weight `1 − frac` to `map_lo`
    /// and `frac` to `map_lo + 1`, so every bin's weights sum to one.
    /// Precomputed per frame length; see [`Spectral::lay_out_bands`].
    map_lo: Vec<u16>,
    map_frac: Vec<f32>,
    /// Σ of each band's weights — how many bins it is worth, so a band
    /// energy can be turned into a per-bin level.
    wsum: [f32; DENOISE_BANDS],
    /// Per band: the NOISE-side time constant of the decision-directed
    /// recursion, in ms. Per band because the confidence of an energy
    /// estimate depends on the bandwidth it was taken over — the spectral
    /// twin of the bank's `BandDesign::a_level`. See [`DD_BW_MS_HZ`].
    tau_dd: [f32; DENOISE_BANDS],
    /// This frame's per-bin power and per-band energy.
    pow: Vec<f32>,
    e: [f32; DENOISE_BANDS],
    /// The learn pass's accumulators: Σ ln(band level) and how many frames
    /// went into it. A GEOMETRIC mean, not the bank's mean + 2σ + 4 dB —
    /// see [`Denoise::freeze_thresholds`].
    ln_sum: [f64; DENOISE_BANDS],
    ln_n: f64,
    /// How many frames the current learn pass has thrown away for being
    /// only partly full of the material being learned.
    warm: usize,
    /// Per bin: the tracked noise power, the smoothed speech-presence
    /// probability behind the stagnation cap, this frame's instantaneous
    /// probability, and the one-way floor and ceiling the learned profile
    /// puts around the estimate.
    lambda: Vec<f32>,
    ph1mean: Vec<f32>,
    ph1: Vec<f32>,
    floor: Vec<f32>,
    ceil: Vec<f32>,
    /// Σ`floor` over the live bins — the whole spectrum's worth of what the
    /// user taught the unit, as one number. Kept because the broadband
    /// speech gate of [`VAD_QUIET_DB`] compares every frame against it, and
    /// it only changes when a profile does. Written by
    /// [`Denoise::refloor`], which is the only writer of `floor`.
    floor_sum: f32,
    /// Per band: last frame's estimated SIGNAL power, which is what the
    /// decision-directed recursion feeds on.
    s_prev: [f32; DENOISE_BANDS],
    /// The two fixed smoothers' per-frame coefficients, derived from the
    /// hop at the running rate. Derived, never stored as constants — that
    /// is what makes 1024 points at 48 kHz and 2048 at 96 behave alike.
    a_ph1: f32,
    a_psd: f32,
    /// **Each band's gain as the ear last heard it**, dB, the state the
    /// attack and release move — a frame writes a TARGET and this follows
    /// it. Unity at rest.
    g_db: [f32; DENOISE_BANDS],
    /// **No frame has been heard since the last reset**, so there is no
    /// gain to follow FROM: the first frame takes its target outright. A
    /// follower started at unity glided down at the RELEASE rate, and a bay
    /// rebuilt under a live microphone reopened the room onto its noise at
    /// full level for a quarter of a second. The target was right from the
    /// first frame; only the follower lagged.
    fresh: bool,
    /// Each band's learned floor as energy, the print the opening rule
    /// measures a frame's band energy against. Laid down by `refloor`.
    floor_b: [f32; DENOISE_BANDS],
    /// How much of the treble dial each band gets, 0..=1 up the shelf.
    shelf: [f32; DENOISE_BANDS],
}

impl Spectral {
    /// Cut every buffer to the longest frame this sample rate will ever
    /// ask for. THE ONLY ALLOCATION THIS KERNEL EVER DOES — a kernel
    /// change re-derives the window and the band tables in place, because
    /// a dial move happens on the audio thread and the audio thread does
    /// not allocate.
    fn new(max_n: usize) -> Self {
        Self {
            n: 0,
            hop: 0,
            win: vec![0.0; max_n],
            in_ring: vec![0.0; 2 * max_n],
            wpos: 0,
            stage_fill: 0,
            acc: vec![0.0; 2 * max_n],
            ap: 0,
            fifo: vec![0.0; 2 * (max_n + SPECTRAL_MAX_BLOCK)],
            out_head: 0,
            out_len: 0,
            re: vec![0.0; max_n],
            im: vec![0.0; max_n],
            gain: vec![1.0; max_n],
            map_lo: vec![0; max_n / 2 + 1],
            map_frac: vec![0.0; max_n / 2 + 1],
            wsum: [1.0; DENOISE_BANDS],
            tau_dd: [TAU_DD_NOISE_MS; DENOISE_BANDS],
            pow: vec![0.0; max_n / 2 + 1],
            e: [0.0; DENOISE_BANDS],
            ln_sum: [0.0; DENOISE_BANDS],
            ln_n: 0.0,
            warm: 0,
            lambda: vec![0.0; max_n / 2 + 1],
            ph1mean: vec![0.5; max_n / 2 + 1],
            ph1: vec![0.0; max_n / 2 + 1],
            floor: vec![0.0; max_n / 2 + 1],
            ceil: vec![0.0; max_n / 2 + 1],
            floor_sum: 0.0,
            s_prev: [0.0; DENOISE_BANDS],
            a_ph1: 0.0,
            a_psd: 0.0,
            g_db: [0.0; DENOISE_BANDS],
            fresh: true,
            floor_b: [1.0; DENOISE_BANDS],
            shelf: [0.0; DENOISE_BANDS],
        }
    }

    /// Drop any half-done learn pass. The first frames after the toggle
    /// goes up are part old material and part new — the ring is n samples
    /// long and the pass is only R samples old — so they are counted out
    /// rather than averaged in.
    fn restart_learn(&mut self) {
        self.ln_sum = [0.0; DENOISE_BANDS];
        self.ln_n = 0.0;
        self.warm = 0;
    }

    /// The output ring's length in stereo frames.
    fn cap(&self) -> usize {
        self.fifo.len() / 2
    }

    /// Point the machinery at a frame length: re-derive the window, empty
    /// everything. No allocation — see [`Spectral::new`].
    fn retune(&mut self, n: usize, sr: f32) {
        self.n = n;
        self.hop = n / 4;
        for (i, w) in self.win[..n].iter_mut().enumerate() {
            *w = (std::f32::consts::PI * (i as f32 + 0.5) / n as f32).sin();
        }
        self.lay_out_bands(sr);
        // TIME constants into per-frame coefficients, at the frame rate
        // this length gives at this sample rate.
        let frame_ms = 1_000.0 * self.hop as f32 / sr;
        self.a_ph1 = (-frame_ms / TAU_PH1MEAN_MS).exp();
        self.a_psd = (-frame_ms / TAU_LAMBDA_MS).exp();
        self.reset();
    }

    /// Forget the audio: rings, accumulator and FIFO to silence, the FIFO
    /// re-primed with one hop so the reported latency is n from the first
    /// sample rather than from the fourth frame.
    fn reset(&mut self) {
        self.in_ring.fill(0.0);
        self.acc.fill(0.0);
        self.fifo.fill(0.0);
        self.gain.fill(1.0);
        self.wpos = 0;
        self.stage_fill = 0;
        self.ap = 0;
        self.out_head = 0;
        self.out_len = self.hop;
        self.ph1mean.fill(0.5);
        self.ph1.fill(0.0);
        self.s_prev = [0.0; DENOISE_BANDS];
        self.g_db = [0.0; DENOISE_BANDS];
        self.fresh = true;
        self.restart_learn();
    }

    /// Lay out the 32 bands for the current frame length and rate, and
    /// build the per-bin table that both reads energy out of them and
    /// writes gain back into them.
    ///
    /// # The widths
    ///
    /// `max(2 bins, k·ERB(f))`, with Glasberg & Moore's
    /// `ERB(f) = 24.7·(4.37·f/1000 + 1)`. The auditory scale is the right
    /// one because the thing being hidden is being hidden from an ear: a
    /// band should be about as wide as the ear's own filter there, so a
    /// band's worth of gain change is a change the ear reads as one event
    /// rather than as a comb. The 2-bin floor is the transform's own limit
    /// — below it a "band" is one bin plus leakage, and a single-bin gain
    /// is exactly the free per-bin mask this design is avoiding.
    ///
    /// `k` is not a magic number, it is SOLVED: bisected until 31 widths
    /// walked up from DC land on [`SPECTRAL_TOP_HZ`], so band 31's centre
    /// sits there and everything from it to Nyquist rides with it. The walk
    /// is monotone in `k` (each width is non-decreasing in `k` and each
    /// centre is non-decreasing in the one before it), so bisection is
    /// exact rather than a search that might not converge. At 48 kHz with
    /// n = 1024 that lands `k` near 1.2 and the low bands on the 2-bin
    /// floor, which is the honest reading of what a 47 Hz bin can resolve.
    ///
    /// # The weights, and why the SAME ones both ways
    ///
    /// Triangular, RNNoise's arrangement: a bin between two band centres
    /// splits `1 − frac` and `frac` between them, and every bin's weights
    /// sum to exactly one. That one property is load-bearing twice over.
    /// Reading, it makes the band energies a partition of the frame's
    /// energy rather than an overlapping over-count. Writing, it means a
    /// bank of EQUAL band gains reconstructs to exactly that gain at every
    /// bin — which is the whole of the fix for the reduction dial. The
    /// bank's bells could not do this and no amount of tuning would have
    /// let them: they are peaking cuts whose skirts overlap, so their
    /// composite is a SUM (a 12 dB ask landing at −21.2 dB at 100 Hz),
    /// whereas these weights are a partition and their composite is an
    /// identity.
    ///
    /// # And why band gains rather than free per-bin gains
    ///
    /// A per-bin mask is a filter whose impulse response is as long as the
    /// frame, so its circular convolution wraps, which is audible
    /// pre-echo. Interpolating a mask across triangles instead constrains
    /// how fast the mask can change with frequency, which is the same thing
    /// as constraining how long its impulse response is. The band structure
    /// buys the acyclic-convolution safety that zero-padding would
    /// otherwise cost a doubled FFT. **Do not "improve" this into a per-bin
    /// mask.**
    ///
    /// The size of what it buys is measured rather than asserted, by
    /// `the_band_mask_stays_short_at_both_frame_lengths`: the REALISED
    /// mask's circular impulse response, with signal present at the top of
    /// the dial, puts −30.6 dB of its energy outside ±R/2 at Studio and
    /// −33.4 dB at Live, against −6.6 and −6.7 dB for the free per-bin
    /// Wiener mask built from the same frame's own `pow`/`lambda`. So the
    /// triangles are worth 24 to 27 dB — and, since [`MIN_BAND_BINS`] went
    /// to four, worth about the SAME at both frame lengths. They were not:
    /// on a two-bin floor the same measurement read −23.5 and −16.1 dB, a
    /// 7.4 dB spread between two kernels that differ only in resolution.
    fn lay_out_bands(&mut self, sr: f32) {
        let n = self.n;
        let bins = n / 2;
        let bin_hz = f64::from(sr) / n as f64;
        let nyq = f64::from(sr) / 2.0;
        // Band 31's centre. Held under Nyquist at low rates so the walk
        // always has somewhere to land — at 44.1 kHz this is 19.8 kHz
        // rather than 20, which no ear and no test can tell apart.
        let top = SPECTRAL_TOP_HZ.min(0.9 * nyq);

        let erb = |f: f64| 24.7 * (4.37 * f / 1_000.0 + 1.0);
        let walk = |k: f64, out: Option<&mut [f64; DENOISE_BANDS]>| -> f64 {
            let mut f = 0.0_f64;
            let mut sink = out;
            if let Some(o) = sink.as_deref_mut() {
                o[0] = 0.0;
            }
            for b in 1..DENOISE_BANDS {
                f += (MIN_BAND_BINS * bin_hz).max(k * erb(f));
                if let Some(o) = sink.as_deref_mut() {
                    o[b] = f;
                }
            }
            f
        };
        let (mut lo, mut hi) = (0.0_f64, 64.0_f64);
        for _ in 0..64 {
            let mid = 0.5 * (lo + hi);
            if walk(mid, None) < top {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let mut centres = [0.0_f64; DENOISE_BANDS];
        walk(0.5 * (lo + hi), Some(&mut centres));
        // The treble shelf, by band centre — see `TREBLE_SHELF_HZ`.
        for (s, c) in self.shelf.iter_mut().zip(centres.iter()) {
            let (lo_hz, hi_hz) = TREBLE_SHELF_HZ;
            *s = smoothstep((*c as f32 - lo_hz) / (hi_hz - lo_hz));
        }

        // Onto the bin grid, strictly increasing and inside the spectrum.
        // Rounding can collide two centres at silly rates; the max() below
        // is what keeps the interpolation's divisor non-zero rather than a
        // guard buried in the hot loop.
        let mut bnd = [0_usize; DENOISE_BANDS];
        for b in 1..DENOISE_BANDS {
            let want = (centres[b] / bin_hz).round() as usize;
            let room = bins - (DENOISE_BANDS - 1 - b);
            bnd[b] = want.max(bnd[b - 1] + 1).min(room);
        }

        for k in 0..=bins {
            let (mut lo_b, mut frac) = (DENOISE_BANDS - 1, 0.0_f32);
            for b in 0..DENOISE_BANDS - 1 {
                if k < bnd[b + 1] {
                    lo_b = b;
                    frac = (k - bnd[b]) as f32 / (bnd[b + 1] - bnd[b]) as f32;
                    break;
                }
            }
            self.map_lo[k] = lo_b as u16;
            self.map_frac[k] = frac;
        }

        self.wsum = [0.0; DENOISE_BANDS];
        for k in 0..=bins {
            let (b, frac) = (self.map_lo[k] as usize, self.map_frac[k]);
            self.wsum[b] += 1.0 - frac;
            if b + 1 < DENOISE_BANDS {
                self.wsum[b + 1] += frac;
            }
        }
        // A band with no bins under it would divide by zero downstream.
        // Only reachable if the grid above ever collides, which the
        // strictly-increasing walk forbids — belt and braces, one branch,
        // outside the hot loop.
        for w in self.wsum.iter_mut() {
            *w = w.max(1.0e-6);
        }

        // And the per-band decision-directed time constant, which is a
        // function of the same bandwidths — derived once here rather than
        // per frame, exactly as the bank derives `a_level` in `new`.
        for (t, w) in self.tau_dd.iter_mut().zip(self.wsum.iter()) {
            let bw = (w * bin_hz as f32).max(1.0);
            *t = (DD_BW_MS_HZ / bw).clamp(TAU_DD_NOISE_MS, TAU_DD_NOISE_MAX_MS);
        }
    }

    /// The per-bin power of the analysed frame, and the 32 band energies
    /// that fall out of it.
    ///
    /// The stereo identity is what makes this cheap. The pair went in
    /// packed as `l + j·r`, so the spectrum is `X_L[k] + j·X_R[k]` — the
    /// two are tangled, and unpacking them costs a pass. They do not need
    /// unpacking: for a real gain, all the detector wants is the total
    /// power, and
    ///
    /// ```text
    ///     |X_L[k]|² + |X_R[k]|² = (|Z[k]|² + |Z[n−k]|²) / 2
    /// ```
    ///
    /// falls straight out of `X_L = (Z[k] + Z*[n−k])/2` and
    /// `X_R = (Z[k] − Z*[n−k])/2j` — the cross terms cancel. One forward
    /// transform, one inverse, both channels, no unpacking pass. **This
    /// identity is what makes the CPU budget work; do not "simplify" it
    /// into two real FFTs.**
    fn band_power(&mut self) {
        let n = self.n;
        let bins = n / 2;
        for k in 0..=bins {
            let m = if k == 0 { 0 } else { n - k };
            let a = self.re[k] * self.re[k] + self.im[k] * self.im[k];
            let b = self.re[m] * self.re[m] + self.im[m] * self.im[m];
            // **The periodogram is where a non-finite observation is
            // stopped, and it is stopped rather than propagated.**
            //
            // `process` already scrubs non-finite SAMPLES, so nothing
            // wrapped in an infinity gets this far — but a finite sample can
            // still overflow HERE, because this line squares. One sample at
            // 1e20 comes out of a 1024-point transform at about 7e19, and
            // 7e19 squared is past f32's 3.4e38: `pow[k]` is infinity with
            // every input sample finite. Downstream that is fatal and
            // SILENT. `spectral_suppress` computes `(1 − ph1)·p`, `ph1`
            // reaches exactly 1.0 on any loud bin, `0.0 · inf` is NaN, and
            // `f32::clamp` passes NaN through unchanged — so `lambda[k]` is
            // NaN forever, `lam_b[b].max(1e-30)` quietly turns it back into
            // 1e-30, ξ explodes and every band opens. The unit stops
            // reducing, the output stays finite, nothing logs and nothing
            // clicks. One branch per bin, in a loop that already runs.
            //
            // Zero, not the previous value: a bin whose power did not fit
            // in a float is a bin this frame knows nothing about, and
            // silence is the observation that moves the tracker least.
            let p = 0.5 * (a + b);
            self.pow[k] = if p.is_finite() { p } else { 0.0 };
        }
        self.e = [0.0; DENOISE_BANDS];
        for k in 0..=bins {
            let (b, frac) = (self.map_lo[k] as usize, self.map_frac[k]);
            let p = self.pow[k];
            self.e[b] += (1.0 - frac) * p;
            if b + 1 < DENOISE_BANDS {
                self.e[b + 1] += frac * p;
            }
        }
    }

    /// One band's energy as a LEVEL: the mean power per bin under the
    /// band's weights, divided by n.
    ///
    /// The division by n is the calibration, and it is chosen so the
    /// number means the same thing at every frame length and every sample
    /// rate. Windowed white noise of per-channel RMS σ has
    /// `E[|Z[k]|²] = Σw²·2σ² = nσ²`, so this reads exactly `σ²` — i.e.
    /// **flat noise at −60 dBFS RMS reads −60 dB in every band**,
    /// whatever n is. That is what lets a profile learned at Studio's 1024
    /// points still mean something at Live's 512, and what lets it be
    /// stored in the same `t0..t31` slots the bank has always used.
    fn band_level(&self, b: usize) -> f32 {
        self.e[b] / (self.wsum[b] * self.n as f32)
    }


    /// Analyse the staged frame into `re`/`im` — the last n stereo frames,
    /// root-Hann windowed, packed as `l + j·r` and transformed.
    fn analyse(&mut self) {
        let n = self.n;
        let mut idx = self.wpos;
        for i in 0..n {
            let w = self.win[i];
            self.re[i] = w * self.in_ring[2 * idx];
            self.im[i] = w * self.in_ring[2 * idx + 1];
            idx += 1;
            if idx == n {
                idx = 0;
            }
        }
        fft(&mut self.re[..n], &mut self.im[..n], false);
    }

    /// Apply `gain`, transform back, window, overlap-add, and hand the hop
    /// that just became complete to the FIFO.
    ///
    /// A real, non-negative gain multiplies the real and the imaginary part
    /// of a bin by the same scalar, which is why this is the whole of the
    /// phase story: the argument of every bin comes back exactly as it went
    /// in. And because the packed signal's spectrum is `X_L + j·X_R`, one
    /// symmetric real gain array filters BOTH channels identically in one
    /// pass — no unpacking, one inverse transform.
    fn synthesise(&mut self) {
        let n = self.n;
        for k in 0..n {
            let g = self.gain[k];
            self.re[k] *= g;
            self.im[k] *= g;
        }
        fft(&mut self.re[..n], &mut self.im[..n], true);

        let mut a = self.ap;
        for i in 0..n {
            let w = self.win[i];
            self.acc[2 * a] += w * self.re[i];
            self.acc[2 * a + 1] += w * self.im[i];
            a += 1;
            if a == n {
                a = 0;
            }
        }

        // The hop starting at `ap` is now complete: every frame that will
        // ever touch it has landed. Halve it (Σw² = 2), hand it over, and
        // zero the slots for the frame that lands on them next.
        let cap = self.cap();
        let mut a = self.ap;
        let mut o = self.out_head + self.out_len;
        if o >= cap {
            o -= cap;
        }
        for _ in 0..self.hop {
            self.fifo[2 * o] = 0.5 * self.acc[2 * a];
            self.fifo[2 * o + 1] = 0.5 * self.acc[2 * a + 1];
            self.acc[2 * a] = 0.0;
            self.acc[2 * a + 1] = 0.0;
            a += 1;
            if a == n {
                a = 0;
            }
            o += 1;
            if o == cap {
                o = 0;
            }
        }
        self.out_len += self.hop;
        self.ap = a;
    }
}

/// The per-block numbers one spectral frame works from, derived from the
/// dials once per `process` call — the spectral twin of [`TickPlan`].
struct SpectralPlan {
    learning: bool,
    /// The dialled floor as a SIGNED gain in dB, so it can be mixed with a
    /// gain rather than subtracted from one: −24 dB at the top of REDUCE.
    floor_db: f32,
    /// Per-frame one-pole coefficients for a band gain rising (opening)
    /// and falling (closing), from the ATTACK and RELEASE dials.
    a_att: f32,
    a_rel: f32,
    /// The opening knee, dB over the band's print: closed at or below the
    /// first, fully open at or above the second. From SMOOTH.
    open_lo: f32,
    open_hi: f32,
    /// The treble lift at the top of the shelf, dB, from the TREBLE dial.
    treble_db: f32,
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
    /// **No tick has run since the bands were cleared**, so there is no cut
    /// to smooth FROM: the first tick takes its target outright. The cut
    /// used to start at zero and climb at the RELEASE rate, and a bay rebuilt
    /// under a live microphone reopened the room onto its noise 13 dB over
    /// the settled level. One tick only: after it every move is smoothed
    /// again, because a bell whose depth jumps tick to tick is the artefact
    /// the smoothing exists to prevent - the first jump lands on bells that
    /// are themselves starting from silence.
    fresh: bool,
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
    /// Which engine ran the last block — what [`Denoise::latency`] answers
    /// about, exactly as `la_on` already is.
    kernel: DenoiseKernel,
    /// The overlap-add machinery, cut in `new` to `studio_n` (the longest
    /// frame this rate can be asked for) so switching kernels is arithmetic
    /// rather than an allocator call.
    sp: Spectral,
    studio_n: usize,
    live_n: usize,
    /// How many control ticks [`MIN_LEARN_MS`] is at this rate — the bank's
    /// misclick guard, in samples rather than in a fixed count, so it means
    /// the same 200 ms everywhere.
    min_learn_ticks: f64,
    /// What the last falling edge of the LEARN toggle did with the pass it
    /// was handed. Read by [`Denoise::last_learn`]; see [`DenoiseLearn`].
    learn_outcome: DenoiseLearn,
}

/// **What a learn pass did, so a panel can say so out loud.**
///
/// Two of the four values are refusals, and they exist because both of them
/// used to be silent. [`DenoiseLearn::TooShort`] is the mouse-bounce guard
/// [`MIN_LEARN_MS`] has always applied; it kept the old profile and told
/// nobody. [`DenoiseLearn::Silent`] is the one this enum was added for:
///
/// A learn pass over DIGITAL SILENCE teaches a floor below the bottom of
/// [`DENOISE_THR_DB`]. Every band clamps to `thr_norm = 0`, which is OFF;
/// [`Denoise::recount`] then reads nothing live, and both kernels take the
/// wire path — `process` returns the buffer untouched and reports success.
/// The unit is off, the panel shows a full set of frozen thresholds, and
/// nothing anywhere says the profile it just took is a profile of nothing.
/// The offline probe hit it first (`--denoise-test` picked the quietest
/// half-second of a take that is 7.4 % true zeroes and wrote a file
/// bit-identical to its input), and a person holding LEARN over a silent
/// bar gets exactly the same, with a mouse instead of a command line.
///
/// So it is now impossible AND visible: impossible because the freeze is
/// refused and whatever profile was there stands, visible because this
/// value says which of the two happened.
///
/// Not persisted anywhere — it is the state of one pass, not a setting —
/// so it carries none of the append-only obligations the serialized
/// parameter enums do. Still: append at the end if it ever grows, because
/// the reason those rules exist is that somebody always ends up matching on
/// the discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenoiseLearn {
    /// Nothing has been taught since this unit was built or cleared.
    None,
    /// A pass finished and a new profile is in force.
    Taught,
    /// The pass was shorter than [`MIN_LEARN_MS`]. The old profile stands.
    TooShort,
    /// The pass heard nothing above the bottom of [`DENOISE_THR_DB`] —
    /// digital silence, a muted input, or a stretch of a file that is
    /// literal zeroes. Taking it would have turned the unit into a wire
    /// that reports success, so the old profile stands.
    Silent,
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
            shelf: 0,
            a_level: 0.0,
        }; DENOISE_BANDS];
        for (k, d) in design.iter_mut().enumerate() {
            let hz = f64::from(Self::center_hz(k));
            if hz > 0.45 * sr {
                continue; // only reachable at silly-low rates; never cuts
            }
            let w0 = std::f64::consts::TAU * hz / sr;
            // The edge bands answer for everything beyond the grid, so
            // their DETECTORS listen wide (a whole octave) where an inner
            // band listens to its slot: hiss at 21 kHz must open — and be
            // cut by — the top shelf even though no centre sits on it.
            let edge = k == 0 || k == DENOISE_BANDS - 1;
            let det_q = if edge { 1.2 } else { f64::from(DENOISE_DETECTOR_Q) };
            let bw = hz / det_q;
            let r = (-std::f64::consts::PI * bw / sr).exp();
            *d = BandDesign {
                cs: w0.cos(),
                alpha: w0.sin() / (2.0 * f64::from(DENOISE_BAND_Q)),
                pr: (r * w0.cos()) as f32,
                pi: (r * w0.sin()) as f32,
                g: (1.0 - r) as f32,
                inert: false,
                // Filled in below, once the tick length is known.
                a_level: 0.0,
                shelf: if k == 0 {
                    -1
                } else if k == DENOISE_BANDS - 1 {
                    1
                } else {
                    0
                },
            };
        }

        // The control tick, fixed in time. Every tick-domain coefficient
        // below derives from the tick's TRUE duration at this rate, so a
        // 29-sample tick at 44.1k and a 64-sample tick at 96k mean the
        // same milliseconds.
        let tick_len = ((sr * f64::from(DENOISE_TICK_MS) / 1_000.0).round() as u32).max(8);
        let tick_s = f64::from(tick_len) / sr;
        let a_of = |ms: f32| (1.0 - (-tick_s * 1_000.0 / f64::from(ms)).exp()) as f32;

        // The per-band LEVEL coefficient needs the tick length, which is
        // only known now; the bandwidth each one derives from is the same
        // `hz / det_q` the resonator above was built with.
        for (k, d) in design.iter_mut().enumerate() {
            if d.inert {
                continue;
            }
            let hz = Self::center_hz(k);
            let edge = k == 0 || k == DENOISE_BANDS - 1;
            let bw = hz / if edge { 1.2 } else { DENOISE_DETECTOR_Q };
            let ms = (LEVEL_BW_MS_HZ / bw).clamp(LEVEL_MS, LEVEL_MAX_MS);
            d.a_level = a_of(ms);
        }

        let la_len = ((sr * f64::from(DENOISE_LOOKAHEAD_MS) / 1_000.0).round() as usize).max(1);

        // The frame lengths, chosen so the frame lasts the same ~21 ms at
        // every rate rather than the same number of samples: frequency
        // resolution is what a spectral denoiser trades against smearing,
        // and a 1024-point frame at 192 kHz would be a quarter of the
        // resolution at four times the rate for no reason anybody can hear.
        // Live is half of Studio at every rate, for the same reason it is
        // half at 48 kHz.
        let studio_n = if sample_rate <= 64_000.0 {
            1_024
        } else if sample_rate <= 128_000.0 {
            2_048
        } else {
            4_096
        };
        let mut sp = Spectral::new(studio_n);
        sp.retune(studio_n, sample_rate);

        let mut dn = Self {
            sample_rate,
            design,
            band: [Band::IDLE; DENOISE_BANDS],
            tick_len,
            phase: 0,
            fresh: true,
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
            kernel: DenoiseKernel::SpectralStudio,
            sp,
            studio_n,
            live_n: studio_n / 2,
            min_learn_ticks: f64::from(MIN_LEARN_MS) / (tick_s * 1_000.0),
            learn_outcome: DenoiseLearn::None,
        };
        // Nothing is learned yet, so this lays a floor of nothing — but it
        // is the one place the per-bin arrays get their shape, and a unit
        // whose tracker has never been floored would clamp against zeroes.
        dn.refloor();
        dn
    }

    /// The frame length this kernel wants at this rate; the bank has none.
    fn n_for(&self, kernel: DenoiseKernel) -> usize {
        match kernel {
            DenoiseKernel::Bank => self.studio_n,
            DenoiseKernel::SpectralStudio => self.studio_n,
            DenoiseKernel::SpectralLive => self.live_n,
        }
    }

    /// Move to another engine. This changes LATENCY, so it is a splice
    /// however it is done — nothing can cross-fade two streams 21 ms apart,
    /// and the host is told the new number by [`Denoise::latency`]. Both
    /// paths are therefore emptied: the one leaving must not keep a tail
    /// for the next time it is chosen, and the one arriving must not open
    /// on the other's history. What is LEARNED survives, exactly as it
    /// survives [`Denoise::clear`] — a profile is a setting.
    fn switch_kernel(&mut self, kernel: DenoiseKernel) {
        self.kernel = kernel;
        for b in self.band.iter_mut() {
            b.quiesce();
        }
        self.fresh = true;
        self.ring.iter_mut().for_each(|s| *s = 0.0);
        self.rpos = 0;
        self.phase = 0;
        self.hp_lp = 0.0;
        let n = self.n_for(kernel);
        if n == self.sp.n {
            self.sp.reset();
        } else {
            self.sp.retune(n, self.sample_rate);
        }
        self.refloor();
    }

    /// Apply the selector and lookahead edges WITHOUT audio, so
    /// [`Denoise::latency`] is right before the first block.
    ///
    /// `latency()` answers from `self.kernel` and `self.la_on`, and those
    /// two fields were only ever written by `process`. A unit built from a
    /// saved blob therefore answered with the CONSTRUCTOR's engine whatever
    /// the session said — measured, `Denoise::new(48_000.0).latency()` is
    /// 1024 for a unit whose parameters say Bank, Spectral Studio and
    /// Spectral Live alike. The host reads that number once at load
    /// (`instrument::prepare_builtin`) and stores it as the seat's latency;
    /// built-ins have no latency handle, so the only correction road is
    /// `Backend::reported_latency` in the block loop — and that call is
    /// skipped for a BYPASSED bay by design. A project reopened with this
    /// bay bypassed therefore held a phantom 1024 frames in that cell for
    /// the whole session, and `publish_latency` aligned every other lane
    /// behind it.
    ///
    /// It does exactly what `process`'s first section does and no more: the
    /// same two edges, in the same order, both allocation-free, and neither
    /// touching the LEARN edge — freezing a profile is an audio-thread
    /// event with a falling dial behind it, not something a host-side
    /// primer may do. Idempotent: called with the engine already seated it
    /// is a pair of comparisons.
    pub fn prime(&mut self, params: &DenoiseParams) {
        let dial = |v: f32, rest: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                rest
            }
        };
        let kernel = DenoiseKernel::from_dial(dial(params.kernel, 0.5));
        if kernel != self.kernel {
            self.switch_kernel(kernel);
        }
        let la_on = dial(params.lookahead, 1.0) >= 0.5;
        if la_on != self.la_on {
            self.ring.iter_mut().for_each(|s| *s = 0.0);
            self.rpos = 0;
            self.la_on = la_on;
        }
    }

    /// Band `k`'s centre frequency, in Hz — the geometry the whole file is
    /// built on, public so a meter can label what it draws.
    pub fn center_hz(k: usize) -> f32 {
        let t = k as f32 / (DENOISE_BANDS - 1) as f32;
        DENOISE_BAND_HZ.0 * (DENOISE_BAND_HZ.1 / DENOISE_BAND_HZ.0).powf(t)
    }

    /// **What the last learn pass did.** Four answers, two of them
    /// refusals; see [`DenoiseLearn`] for why a refusal has to be sayable.
    ///
    /// The panel's job with this is one line of text after the LEARN button
    /// comes back up. [`DenoiseLearn::Silent`] in particular must not be
    /// left to the meters: the refusal is what keeps the old profile alive,
    /// so the meters look exactly as they did a moment before and the user
    /// has no way to tell a pass that worked from one that was thrown away.
    pub fn last_learn(&self) -> DenoiseLearn {
        self.learn_outcome
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
            // A print with no knees of its own gates the bank at its
            // thresholds, which is every bank print's reading anyway.
            b.knee_db = b.thr_db;
        }
        self.recount();
        self.refloor();
    }
    /// **The bank's knees**, normalized like [`Denoise::thresholds`]: the 32
    /// `k0..k31` symbols a session persists beside the print. Equal to the
    /// thresholds for a print the bank learned; for one a spectral kernel
    /// learned, the bank's own measurement of the same noise. See
    /// [`Band::knee_db`].
    pub fn knees(&self) -> [f32; DENOISE_BANDS] {
        let span = DENOISE_THR_DB.1 - DENOISE_THR_DB.0;
        let mut out = [0.0_f32; DENOISE_BANDS];
        for (o, b) in out.iter_mut().zip(self.band.iter()) {
            *o = ((b.knee_db - DENOISE_THR_DB.0) / span).clamp(0.0, 1.0);
        }
        out
    }
    /// Restore persisted knees, after [`Denoise::set_thresholds`]. Clamped
    /// and non-finite-proofed the same way.
    pub fn set_knees(&mut self, knees: &[f32; DENOISE_BANDS]) {
        for (b, &t) in self.band.iter_mut().zip(knees.iter()) {
            let norm = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
            b.knee_db = DENOISE_THR_DB.0 + (DENOISE_THR_DB.1 - DENOISE_THR_DB.0) * norm;
        }
    }

    /// The honest latency, in samples, for the current settings — what a
    /// host must be told. "Current" means as of the last `process` call, or
    /// the last [`Denoise::prime`] — which is what a host calls when it has
    /// seated parameters and needs the number BEFORE any audio has run. The
    /// constructed defaults answer until one of the two has happened.
    ///
    /// The bank answers with its lookahead line, zero when the toggle is
    /// down. A spectral kernel answers with the FULL FRAME, n, not n − R:
    /// the overlap-add's own algorithmic delay is n − R, but output leaves
    /// this unit in hops of R and a host block boundary lands wherever it
    /// likes, so the worst case over an arbitrary block size is n − 1. n is
    /// the block-agnostic bound, the FIFO's one-hop priming buys exactly
    /// it, and `the_spectral_latency_is_the_frame_and_an_impulse_proves_it`
    /// measures it with an impulse at seven different block sizes rather
    /// than taking this comment's word for it.
    pub fn latency(&self) -> u32 {
        match self.kernel {
            DenoiseKernel::Bank => {
                if self.la_on {
                    self.la_len as u32
                } else {
                    0
                }
            }
            _ => self.sp.n as u32,
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
    ///
    /// **All THIRTY-TWO of them landing there is not an answer**, and the
    /// pass is refused rather than taken. See [`DenoiseLearn::Silent`]: a
    /// profile of nothing turns the unit into a wire that reports success,
    /// with a full set of thresholds on the panel and no way to tell. The
    /// outcome — taken, too short, or silent — comes back out of
    /// [`Denoise::last_learn`], because a refusal that keeps the old
    /// profile is by construction invisible in everything else the panel
    /// can see.
    fn freeze_thresholds(&mut self) {
        if self.kernel != DenoiseKernel::Bank {
            self.freeze_spectral_thresholds();
            return;
        }
        let min_ticks = self.min_learn_ticks;
        // **Proposed first, written second**, so the all-silent case can be
        // refused whole. A band that heard silence landing on OFF is still
        // the right answer for that band; thirty-two of them landing there
        // is not an answer, it is a unit that has just been switched off
        // without saying so. See [`DenoiseLearn::Silent`].
        let mut proposed: [f32; DENOISE_BANDS] = [0.0; DENOISE_BANDS];
        let mut taught = false;
        for (p, b) in proposed.iter_mut().zip(self.band.iter()) {
            *p = b.thr_norm;
            if b.n < min_ticks {
                continue; // a misclick, not a profile — keep the old value
            }
            taught = true;
            let mean = b.sum / b.n;
            let var = (b.sumsq / b.n - mean * mean).max(0.0);
            let thr = mean
                + f64::from(DENOISE_LEARN_SIGMA) * var.sqrt()
                + f64::from(DENOISE_LEARN_HEADROOM_DB);
            let span = f64::from(DENOISE_THR_DB.1 - DENOISE_THR_DB.0);
            *p = ((thr - f64::from(DENOISE_THR_DB.0)) / span).clamp(0.0, 1.0) as f32;
        }
        self.learn_outcome = if !taught {
            DenoiseLearn::TooShort
        } else if proposed.iter().all(|n| *n < DENOISE_OFF) {
            DenoiseLearn::Silent
        } else {
            for (p, b) in proposed.iter().zip(self.band.iter_mut()) {
                b.thr_norm = *p;
                b.thr_db = DENOISE_THR_DB.0 + (DENOISE_THR_DB.1 - DENOISE_THR_DB.0) * *p;
                b.knee_db = b.thr_db;
            }
            DenoiseLearn::Taught
        };
        for b in self.band.iter_mut() {
            b.n = 0.0;
            b.sum = 0.0;
            b.sumsq = 0.0;
        }
        self.sp.restart_learn();
        self.recount();
        self.refloor();
    }

    /// The spectral falling edge: freeze each band at the GEOMETRIC mean of
    /// the levels it heard.
    ///
    /// Not the arithmetic mean, and emphatically not the bank's
    /// mean + 2σ + 4 dB. A noise print is one take of one room and the user
    /// is holding a mouse: a door, a chair, a breath, one frame of the
    /// take's own music leaking in — the arithmetic mean is dragged bodily
    /// upward by a single loud frame, and a threshold set too high is a
    /// threshold that eats signal. The mean of the LOGARITHM cannot be: one
    /// frame 20 dB hot moves a 200-frame average by a tenth of a decibel.
    /// It is SoX's `noiseprof` choice for exactly this reason, and it fails
    /// in the safe direction — a pass that catches some silence reads LOW,
    /// and a floor that is too low suppresses less rather than damaging
    /// more.
    ///
    /// The result lands in the same `thr_norm` the bank uses and persists
    /// through the same `t0..t31` symbols. It does NOT mean the same thing
    /// numerically — the bank measures resonator band energy against a
    /// tone calibration, this measures a windowed periodogram's per-bin
    /// level — so **a profile belongs to the kernel that learned it**.
    /// Switching kernels after a learn is a re-learn, and the panel should
    /// say so.
    ///
    /// Two passes are refused instead of taken, and both keep whatever
    /// profile was already there: one shorter than [`MIN_LEARN_MS`], and
    /// one that heard nothing above the bottom of [`DENOISE_THR_DB`]. The
    /// second is [`DenoiseLearn::Silent`] and it is the one that used to be
    /// invisible — every band clamping to OFF makes the unit a wire that
    /// reports success. [`Denoise::last_learn`] is how the panel finds out
    /// which of the three happened.
    fn freeze_spectral_thresholds(&mut self) {
        // The misclick guard measures how long the PASS was, not how many
        // frames survived it: the warm-up frames thrown away at the top are
        // still time the user spent holding the button, and a guard that
        // forgot them would refuse a pass three frames longer than it asked
        // for.
        let heard = (self.sp.ln_n + self.sp.warm as f64) * self.sp.hop as f64;
        let need = f64::from(MIN_LEARN_MS) * f64::from(self.sample_rate) / 1_000.0;
        if self.sp.ln_n <= 0.0 || heard < need {
            self.learn_outcome = DenoiseLearn::TooShort;
        } else {
            // **Proposed first, written second.** A pass over digital
            // silence puts every band's geometric mean below the bottom of
            // [`DENOISE_THR_DB`], all 32 norms clamp to zero, `recount`
            // reads nothing live and the kernel becomes a wire that reports
            // success — with a full set of thresholds on the panel and
            // nothing saying they are thresholds of nothing. Refusing the
            // whole freeze keeps whatever was there and
            // [`Denoise::last_learn`] says which happened. See
            // [`DenoiseLearn::Silent`].
            let span = f64::from(DENOISE_THR_DB.1 - DENOISE_THR_DB.0);
            let mut proposed: [f32; DENOISE_BANDS] = [0.0; DENOISE_BANDS];
            for (p, sum) in proposed.iter_mut().zip(self.sp.ln_sum.iter()) {
                // 10·log10(exp(mean ln E)) — the geometric mean, in dB.
                let db = 10.0 / std::f64::consts::LN_10 * (sum / self.sp.ln_n);
                *p = ((db - f64::from(DENOISE_THR_DB.0)) / span).clamp(0.0, 1.0) as f32;
            }
            self.learn_outcome = if proposed.iter().all(|n| *n < DENOISE_OFF) {
                DenoiseLearn::Silent
            } else {
                // The print is the spectral kernel's own reading; the bank's
                // knee beside it is the bank's, taken by `learn_feed` during
                // the same pass. A band the bank's ear did not hear enough
                // of keeps the threshold as its knee.
                let knees = self.bank_knees_proposed();
                for ((p, b), k) in proposed.iter().zip(self.band.iter_mut()).zip(knees) {
                    b.thr_norm = *p;
                    b.thr_db = DENOISE_THR_DB.0 + (DENOISE_THR_DB.1 - DENOISE_THR_DB.0) * *p;
                    b.knee_db = k.unwrap_or(b.thr_db);
                }
                DenoiseLearn::Taught
            };
        }
        for b in self.band.iter_mut() {
            b.n = 0.0;
            b.sum = 0.0;
            b.sumsq = 0.0;
        }
        self.sp.restart_learn();
        self.recount();
        self.refloor();
    }

    /// Design one bell for a cut of `depth_db`. RBJ peaking with the fixed
    /// per-band trigonometry, so a redesign is one `powf` and a handful of
    /// divides — cheap enough to run per band per control tick while a
    /// depth is travelling. At depth 0 this would collapse to the identity
    /// (b ≡ a for A = 1), but depth 0 never gets here: the band is skipped.
    fn design_coef(d: &BandDesign, depth_db: f32) -> Coef {
        let a = 10.0_f64.powf(f64::from(-depth_db) / 40.0);
        if d.shelf != 0 {
            // The edge bands cut as SHELVES — everything beyond the grid
            // falls with them, which is the whole reason they exist (see
            // DENOISE_BAND_HZ). RBJ cookbook shelf, cut form (A < 1),
            // corner at the band's own centre.
            let cs = d.cs;
            let sn = (1.0 - cs * cs).max(0.0).sqrt();
            let sqa = a.sqrt();
            let alpha_s = sn / 2.0
                * ((a + 1.0 / a) * (1.0 / DENOISE_SHELF_S - 1.0) + 2.0).sqrt();
            let (b0, b1, b2, ra0, ra1, ra2) = if d.shelf > 0 {
                (
                    a * ((a + 1.0) + (a - 1.0) * cs + 2.0 * sqa * alpha_s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cs),
                    a * ((a + 1.0) + (a - 1.0) * cs - 2.0 * sqa * alpha_s),
                    (a + 1.0) - (a - 1.0) * cs + 2.0 * sqa * alpha_s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cs),
                    (a + 1.0) - (a - 1.0) * cs - 2.0 * sqa * alpha_s,
                )
            } else {
                (
                    a * ((a + 1.0) - (a - 1.0) * cs + 2.0 * sqa * alpha_s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cs),
                    a * ((a + 1.0) - (a - 1.0) * cs - 2.0 * sqa * alpha_s),
                    (a + 1.0) + (a - 1.0) * cs + 2.0 * sqa * alpha_s,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cs),
                    (a + 1.0) + (a - 1.0) * cs - 2.0 * sqa * alpha_s,
                )
            };
            return Coef {
                b0: (b0 / ra0) as f32,
                b1: (b1 / ra0) as f32,
                b2: (b2 / ra0) as f32,
                a1: (ra1 / ra0) as f32,
                a2: (ra2 / ra0) as f32,
            };
        }
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
        let fresh = std::mem::replace(&mut self.fresh, false);

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
            b.level_db += self.design[k].a_level * (level_db - b.level_db);

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
                if b.env_db >= b.knee_db + DENOISE_HYSTERESIS_DB {
                    b.open = true;
                    b.hold = self.hold_ticks;
                } else if b.open {
                    if b.hold > 0 {
                        b.hold -= 1;
                    } else if b.env_db <= b.knee_db {
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
                    let under = b.knee_db - b.level_db;
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

            // The TARGET, unsmoothed. The smoothing happens in a second
            // pass, after the cross-band constraint has had its say — see
            // the constraint's own comment for why that ordering is the
            // whole fix for the depth jump.
            depth[k] = target;
        }

        // The cross-band slope constraint, MASKING-EXEMPT: one pass up,
        // one pass down, each only ever LOWERING a band toward its
        // neighbour + 6 dB — but a band that is open BECAUSE SIGNAL LIVES
        // IN IT does not export its zero to the bands beside it. The
        // constraint exists to smooth the learned profile's jaggedness
        // among CLOSED bands (band-edge beating between adjacent cuts);
        // letting an open band drag its neighbours halfway open raised the
        // noise floor around every note — the "white noise fading into
        // sound" a beta ear named precisely. Psychoacoustics carries the
        // open band's own edge: the signal that opened it masks the noise
        // it admits, and its neighbours' floors staying PINNED is what
        // makes the break-through crisp.
        //
        // **It runs on the TARGET, before the smoother, and that ordering
        // is load-bearing.** It used to run on the smoothed depth, after —
        // a hard `min` applied to the number the bell was about to be
        // designed from. The `open` flags are booleans, so the moment a
        // neighbour stops being masking-exempt the constraint arrives all
        // at once and the depth moved as much as 5.55 dB in a single
        // 0.667 ms tick. A minimum-phase bell whose coefficients jump is
        // the definition of the artefact this unit was reported for: its
        // phase is tied to its magnitude, so a discontinuous gain is a
        // discontinuous phase. Constraining the target instead puts every
        // such transition through the same 2 ms opening envelope that every
        // other gain move already uses, and the bell stops being asked to
        // teleport.
        let open: [bool; DENOISE_BANDS] = {
            let mut o = [false; DENOISE_BANDS];
            for (k, b) in self.band.iter().enumerate() {
                o[k] = b.open;
            }
            o
        };
        for k in 1..DENOISE_BANDS {
            if !open[k - 1] {
                depth[k] = depth[k].min(depth[k - 1] + DENOISE_SLOPE_DB);
            }
        }
        for k in (0..DENOISE_BANDS - 1).rev() {
            if !open[k + 1] {
                depth[k] = depth[k].min(depth[k + 1] + DENOISE_SLOPE_DB);
            }
        }

        for (k, b) in self.band.iter_mut().enumerate() {
            // Depth smoothing, dB domain, on the CONSTRAINED target.
            // Toward LESS cut is the fast fixed opening; toward MORE cut is
            // the release, interpolated toward its after-a-transient speed
            // by the boost flag.
            let target = depth[k];
            let a = if fresh {
                // No cut was heard yet to move FROM: the target, outright.
                1.0
            } else if target < b.depth_db {
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
        let kernel = DenoiseKernel::from_dial(dial(params.kernel, 0.5));

        // Selector edges are block-rate events, handled before any audio,
        // and the kernel goes first: the learn edge below has to freeze
        // through the engine that is actually going to use the profile.
        if kernel != self.kernel {
            self.switch_kernel(kernel);
        }
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
            self.sp.restart_learn();
            self.learning = true;
        } else if !learn_on && self.learning {
            self.freeze_thresholds();
            self.learning = false;
        }

        if channels < 2 {
            return;
        }
        // A `frames` past the end of the buffer is a caller's bug, not a
        // panic: the bank's `chunks_exact_mut` already stopped short, and
        // the spectral path indexes directly, so it is clamped here where
        // both can see it.
        let frames = frames.min(buf.len() / channels);
        if self.kernel != DenoiseKernel::Bank {
            self.process_spectral(buf, frames, channels, params);
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
            // **Non-finite input is scrubbed where each kernel READS it,
            // never at the top of `process`.** Tangent hosts third-party
            // LV2 plugins in the same chain and nothing washes the buffer
            // between them; `wavread.rs` already scrubs file samples for
            // exactly this reason, and `effects.rs` guards the delay's
            // feedback with exactly this pattern. It is done here rather
            // than in one pass over the block because the wire fast path
            // above must keep its promise not to touch the buffer at all —
            // and because a wire IS allowed to pass an infinity through.
            //
            // The bank's version of this fault is the loud one: one
            // infinity used to enter the resonator recursions and the
            // biquad state and stay there, and 96,000 of the next 192,000
            // output samples came out non-finite.
            let (l, r) = (flush_bad(frame[0]), flush_bad(frame[1]));

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

    /// What one spectral frame needs out of the dials, worked out once per
    /// block: the same split the bank's [`TickPlan`] makes, for the same
    /// reason — a dial is read at block rate and used at frame rate.
    fn spectral_plan(&self, params: &DenoiseParams) -> SpectralPlan {
        let dial = |v: f32, rest: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                rest
            }
        };
        let geo = |range: (f32, f32), t: f32| range.0 * (range.1 / range.0).powf(t);
        let frame_ms = 1_000.0 * self.sp.hop.max(1) as f32 / self.sample_rate.max(1.0);
        let coef = |ms: f32| 1.0 - (-frame_ms / ms.max(0.1)).exp();
        let d = DenoiseParams::default();
        let smooth = dial(params.smooth, d.smooth);
        SpectralPlan {
            learning: self.learning,
            floor_db: -(dial(params.reduce, 0.5) * DENOISE_REDUCE_DB),
            a_att: coef(geo(DENOISE_ATTACK_MS, dial(params.attack, d.attack))),
            a_rel: coef(geo(DENOISE_RELEASE_MS, dial(params.release, d.release))),
            open_lo: OPEN_SOFT_DB.0 + (OPEN_HARD_DB.0 - OPEN_SOFT_DB.0) * smooth,
            open_hi: OPEN_SOFT_DB.1 + (OPEN_HARD_DB.1 - OPEN_SOFT_DB.1) * smooth,
            treble_db: dial(params.treble, d.treble) * DENOISE_TREBLE_DB,
        }
    }

    /// Write the gain for every bin of the frame `analyse` just produced.
    ///
    /// While the learn toggle is up this is still a PURE DELAY — every bin
    /// passes at unity and the frame's only job is to be measured. The
    /// user is playing isolated noise at the unit and has to hear it raw,
    /// which is the same contract the bank's learn pass keeps.
    fn spectral_gains(&mut self, plan: &SpectralPlan) {
        self.sp.band_power();
        if plan.learning {
            // The first three frames of a pass straddle the toggle: the
            // ring is n samples long and only R of them are new, so a
            // quarter-full, half-full and three-quarter-full frame would
            // each read low. Count them out rather than average them in.
            if self.sp.warm < 3 {
                self.sp.warm += 1;
            } else {
                for b in 0..DENOISE_BANDS {
                    let level = f64::from(self.sp.band_level(b)).max(LN_FLOOR);
                    self.sp.ln_sum[b] += level.ln();
                }
                self.sp.ln_n += 1.0;
            }
            self.sp.gain[..self.sp.n].fill(1.0);
            return;
        }
        if self.live == 0 {
            // Nothing learned is a wire, here as everywhere: the frame
            // machinery still owes its delay, but no bin is touched.
            self.sp.gain[..self.sp.n].fill(1.0);
            return;
        }
        self.spectral_suppress(plan);
    }

    /// Track the noise in every bin, decide how much of each band is
    /// signal, and write the gain the frame will be multiplied by.
    ///
    /// # The noise estimate: MMSE with a speech-presence probability
    ///
    /// Per BIN, Gerkmann & Hendriks. Each bin's a-posteriori SNR
    /// `γ = |Y|²/λ` is turned into a generalised likelihood ratio against a
    /// fixed a-priori SNR of 15 dB ([`SPP_XI_H1`], with [`LOG_GLR_FACT`]
    /// and [`GLR_EXP`] its two precomputed pieces), and that ratio becomes
    /// a probability that this bin holds signal. The noise estimate then
    /// updates toward `p·λ + (1−p)·|Y|²` — that is, toward the observation
    /// only in proportion to how sure we are it is NOT signal. This is why
    /// a sustained note does not slowly get absorbed into the noise floor
    /// the way a minimum-statistics tracker's does.
    ///
    /// [`SPP_STAGNATION_CAP`] is not decoration. Once a bin's smoothed
    /// probability has sat at 1 for a while its estimate stops updating
    /// forever — frozen by its own confidence — and the failure is silent:
    /// the unit simply stops working on sustained material. Capping the
    /// instantaneous probability leaves 1 % of every frame flowing in,
    /// which is enough to unstick it.
    ///
    /// **That per-bin probability is then multiplied by ONE number for the
    /// whole frame**, the broadband gate of [`VAD_QUIET_DB`], which is what
    /// makes this unit usable on a recording rather than on a call. A bin
    /// sitting 12 dB over its print reads as speech at 0.99999 whether it
    /// holds a syllable or a chair, so on a real take the mid band — the
    /// only band where a room's low-level activity lives — froze its
    /// estimate at the print and rode open through a third of the frames
    /// between words. A frame that is quiet across the WHOLE spectrum is
    /// not a syllable, and the weight only ever lets the estimate hear more
    /// of the observation, never less.
    ///
    /// The estimate is FLOORED at the learned profile and ceilinged
    /// [`NOISE_CEIL_DB`] above it. The floor is one-way and it is the whole
    /// point of having asked the user to teach the unit: a tracker that may
    /// fall below what it was taught will, on a quiet passage, decide the
    /// room is quieter than it is and stop cutting. The ceiling is the
    /// other side — an estimate that far above the print is tracking the
    /// programme — and it only started mattering once the gate above let
    /// the estimate climb at all.
    ///
    /// # The gain: decision-directed, then mixed against the dial
    ///
    /// Per BAND. The a-priori SNR ξ comes from the classical
    /// decision-directed recursion, mixing the PREVIOUS frame's estimated
    /// signal power with this frame's raw excess `max(γ − 1, 0)`. Using the
    /// previous estimated SIGNAL power (gain² × energy) rather than the
    /// previous ξ is what makes the estimator self-quenching in noise: a
    /// small gain feeds back a small power, which keeps ξ small, and the
    /// recursion settles at [`XI_MIN_DB`] instead of at the ~−4 dB the raw
    /// excess alone would hold it at. The weight is expressed as a time
    /// constant that slides from [`TAU_DD_NOISE_MS`] to
    /// [`TAU_DD_SIGNAL_MS`] with `q²`, so a noise-only band forgets over a
    /// quarter second and a band with a note in it forgets in 8 ms — which
    /// is what keeps transients from smearing without paying for it in
    /// musical noise everywhere else.
    ///
    /// Then the line this whole rewrite exists for:
    ///
    /// ```text
    ///     g_dB[b] = p[b]·20log10(g_H1[b]) + (1 − p[b])·C_dB
    /// ```
    ///
    /// with `C_dB = −(reduce · 24)`, the dialled floor, and `p[b]` the
    /// band's speech-presence probability — which costs nothing extra,
    /// because the per-bin probabilities the noise tracker already computed
    /// aggregate through the SAME triangular weights the energies do. No
    /// second detector.
    ///
    /// In a noise-only frame ξ has bottomed out at [`XI_MIN_DB`], so
    /// `g_H1` is −40.1 dB — below the deepest the dial can ask for — and
    /// the mixture therefore lands at or under `C_dB` in every band
    /// whatever `p` happens to be. The clamp puts it exactly ON `C_dB`:
    /// flat, and equal to the dial, in every band and every octave. That
    /// clamp is the REDUCE dial's own meaning (a CAP on the cut, exactly
    /// the bank's `wanted.min(cap_db)`), not a fudge to make a test pass.
    ///
    /// `p[b]` is the weighted SUM of the bin probabilities, clamped into
    /// 0..=1 — a detection statistic that saturates, not an average. The
    /// distinction is the whole difference between this working and not,
    /// and it was measured: averaged instead, a −20 dBFS tone at 1 kHz
    /// came back 4.3 dB down, because a tone fills two bins of a
    /// three-and-a-half-bin band and an average therefore reports 60 %
    /// confidence about a band that plainly contains a note. The question
    /// the mixture is asking is "is there signal here", and one bin's worth
    /// of certainty is a yes.
    ///
    /// One thing the specification says about `p` is not quite true and it
    /// does not matter: `p` does NOT go to zero on noise. The bins' own
    /// probabilities sit near 0.03–0.12 rather than at 0, so a narrow band
    /// reads about 0.24 and a wide one saturates at 1. The dial's exactness
    /// comes from the CAP, not from `p` vanishing — with `g_H1` pinned at
    /// −40.1 dB by [`XI_MIN_DB`], the mixture is at or under `C_dB` for
    /// every `p` in 0..=1, so the clamp lands it on the dial either way.
    fn spectral_suppress(&mut self, plan: &SpectralPlan) {
        let n = self.sp.n;
        let bins = n / 2;

        // ── is this frame quiet all the way across? ─────────────────────
        //
        // One number, taken before the tracker runs, because the tracker is
        // about to consume it: λ is written in place below, so a sum taken
        // inside that loop would be half this frame's and half the last
        // one's.
        //
        // The denominator is the PRINT and not λ, and that is the whole
        // difference between a gate and a runaway. Measured against λ, a
        // −20 dBFS tone over learned noise settles into a feedback: the
        // gate half-opens, λ climbs a little, the frame's excess over λ
        // falls, the gate opens further. It converges at 8.65 dB of excess
        // with λ pinned to its ceiling and the tone 1.96 dB down — a test
        // failure with a 1.5 dB budget, and the right failure to have,
        // because a noise tracker that can eat a steady tone will eat a
        // held note. The print cannot drift, so nothing the gate does to
        // the estimate can move the line the gate is judged against.
        //
        // Finite by construction and therefore clamped rather than
        // branched: `pow` is finite because `Spectral::band_power` refuses
        // to store anything else, and `floor_sum` is a sum of terms
        // `refloor` floors at 1e−30. The `max(TINY)` is for digital
        // silence, where `heard` is a true zero and the logarithm would
        // otherwise be −∞: that frame reads −200 dB of excess, which is as
        // quiet as a frame can be and exactly the answer wanted. See
        // [`VAD_QUIET_DB`] for what this buys.
        let mut heard = 0.0_f32;
        for k in 0..=bins {
            heard += self.sp.pow[k];
        }
        let frame_db = 10.0 * (heard / self.sp.floor_sum.max(TINY)).max(TINY).log10();
        let speech =
            ((frame_db - VAD_QUIET_DB) / (VAD_SPEECH_DB - VAD_QUIET_DB)).clamp(0.0, 1.0);

        // ── the per-bin noise tracker ───────────────────────────────────
        let (a_ph1, a_psd) = (self.sp.a_ph1, self.sp.a_psd);
        for k in 0..=bins {
            let lam = self.sp.lambda[k];
            // `p` is finite by construction: `Spectral::band_power` is the
            // only writer of `pow` and it refuses to store anything else,
            // which is one guard rather than two and also keeps `e[b]`
            // finite for the recursion below. A second check here would be
            // a branch per bin that no revert could ever turn red.
            let p = self.sp.pow[k];
            let snr_post = p / lam;
            // The likelihood ratio's LOGARITHM, clamped, and then the
            // probability by the logistic identity
            // `glr/(1+glr) = 1/(1 + e^−log glr)` rather than by
            // exponentiating and dividing.
            //
            // This is not a style preference, it is the difference between
            // working and not. In f64 `exp(200)` is a large finite number
            // and the textbook form is fine; in f32 it is INFINITY, and
            // `inf/(1+inf)` is NaN — one NaN gain poisons a bin, the
            // inverse transform spreads it across the whole frame, and the
            // overlap-add spreads that across the whole buffer. It takes a
            // band whose profile is OFF (floor ~1e−7) with real signal in
            // it to reach the clamp at all, which is exactly the case the
            // noise-only acceptance tests cannot produce and a tone over
            // band-limited noise produces immediately.
            // The clamp is NOT `f32::min`, and the difference is the
            // difference between a guard and a trap. `f32::min` returns THE
            // OTHER OPERAND when one side is NaN — measured,
            // `NAN.min(200.0) == 200.0` — so the line the comment on
            // [`GLR_CLAMP`] describes as stopping an overflow becoming NaN
            // in fact converted a NaN into exactly the value that
            // MANUFACTURES one: `log_glr = 200` gives `ph1 == 1.0` exactly,
            // and `(1.0 − ph1) · p` is then `0.0 · inf`. An unknown
            // a-posteriori SNR is not certainty about speech; it is no
            // information, so it reads as the constant term, which is
            // `ph1 = 0.03`.
            //
            // **Honest note: with the two guards above in place this branch
            // is unreachable, and no test in this file can turn it red.**
            // `p` is finite because `Spectral::band_power` is the only
            // writer of `pow` and refuses anything else; `lam` is finite and
            // STRICTLY POSITIVE because the store below writes either
            // `floor[k]` or a value clamped into `floor[k]..=ceil[k]`, and
            // `refloor` floors that at 1e−30 — so `p / lam` cannot be NaN.
            // It is one comparison, it is defence in depth for the day one
            // of those two arguments stops holding, and it is here because
            // this exact line's NaN semantics being the opposite of its
            // comment is what made the fault invisible for a whole
            // engine's worth of work.
            let log_glr = if snr_post.is_finite() {
                (LOG_GLR_FACT + GLR_EXP * snr_post).min(GLR_CLAMP)
            } else {
                LOG_GLR_FACT
            };
            let mut ph1 = 1.0 / (1.0 + (-log_glr).exp());
            let mean = a_ph1 * self.sp.ph1mean[k] + (1.0 - a_ph1) * ph1;
            self.sp.ph1mean[k] = mean;
            if mean > SPP_STAGNATION_CAP {
                ph1 = ph1.min(SPP_STAGNATION_CAP);
            }
            // **The frame's own verdict multiplies the bin's.** A bin 12 dB
            // over its print reads as speech at 0.99999 whether it is a
            // syllable or a chair; a frame that is quiet across the whole
            // spectrum is not a syllable. The product is at or below `ph1`
            // for every weight in 0..=1, so this can only let the estimate
            // hear MORE of the observation than it used to — never less.
            // See [`VAD_QUIET_DB`].
            //
            // `ph1` itself is left alone: it is the band presence the GAIN
            // mixes against the dial below, and that is a question about
            // this bin, not about the frame around it.
            let track = ph1 * speech;
            let est = track * lam + (1.0 - track) * p;
            let next = a_psd * lam + (1.0 - a_psd) * est;
            // **A non-finite estimate is never stored, and never survives
            // being found.** `clamp` compares with `<` and `>`, both false
            // for NaN, so it passes NaN through UNCHANGED — which is how
            // one bad frame used to lock this tracker for the life of the
            // session, healed only by `clear()`, `set_thresholds()` or a
            // kernel change, none of which normal playback calls. Falling
            // back to the FLOOR rather than to `lam` matters: `lam` is the
            // thing that might already be NaN, and the floor is what the
            // user taught, so the recursion re-enters from the print
            // instead of from a value that cannot be recovered from.
            self.sp.lambda[k] = if next.is_finite() {
                next.clamp(self.sp.floor[k], self.sp.ceil[k])
            } else {
                self.sp.floor[k]
            };
            self.sp.ph1[k] = ph1;
        }

        // ── band aggregates, through the same weights as the energy ─────
        let mut lam_b = [0.0_f32; DENOISE_BANDS];
        let mut pres = [0.0_f32; DENOISE_BANDS];
        for k in 0..=bins {
            let (b, frac) = (self.sp.map_lo[k] as usize, self.sp.map_frac[k]);
            let (lam, ph1) = (self.sp.lambda[k], self.sp.ph1[k]);
            lam_b[b] += (1.0 - frac) * lam;
            pres[b] += (1.0 - frac) * ph1;
            if b + 1 < DENOISE_BANDS {
                lam_b[b + 1] += frac * lam;
                pres[b + 1] += frac * ph1;
            }
        }

        // ── the decision-directed a-priori SNR ──────────────────────────
        let frame_ms = 1_000.0 * self.sp.hop as f32 / self.sample_rate;
        let xi_min = 10.0_f32.powf(XI_MIN_DB / 20.0).powi(2);
        let mut xi = [0.0_f32; DENOISE_BANDS];
        for b in 0..DENOISE_BANDS {
            let lam = lam_b[b].max(1.0e-30);
            let gamma = (self.sp.e[b] / lam).clamp(1.0e-6, 1_000.0);
            let s = self.sp.s_prev[b];
            let q = s / (s + lam);
            // The noise-side constant is this band's own; the signal side
            // is not, and that is the point. See [`DD_BW_MS_HZ`].
            let tau_n = self.sp.tau_dd[b];
            let tau = tau_n + (TAU_DD_SIGNAL_MS - tau_n) * q * q;
            let a = (-frame_ms / tau).exp();
            let hist = (s / lam).min(XI_HIST_MAX);
            xi[b] = (a * hist + (1.0 - a) * (gamma - 1.0).max(0.0)).max(xi_min);
        }

        // Cross-band smoothing, [0.25, 0.5, 0.25], on the ESTIMATE and
        // never on the output. Smoothing the GAIN would be the bank's
        // mistake in a new costume — a constraint applied after the fact to
        // the thing the ear hears. Smoothing the estimate instead means a
        // band's neighbours vote on how much signal it has, which halves
        // the variance a two-bin band's energy carries, and the weights sum
        // to one so a flat estimate stays exactly flat.
        //
        // LINEAR, not logarithmic, and that was measured rather than
        // assumed. ξ spans orders of magnitude, which is the usual argument
        // for smoothing its logarithm — but a geometric mean of one loud
        // band and two quiet ones is dominated by the quiet ones, and a
        // −20 dBFS tone sitting on a band centre came back 2.4 dB down
        // because its own neighbours voted it away. The arithmetic mean
        // biases the other way, toward keeping signal, which is the right
        // direction to be wrong in; and where it matters for the dial —
        // every band bottomed out at [`XI_MIN_DB`] — the two agree exactly,
        // because a mean of equal numbers is that number in either domain.
        //
        // An OFF neighbour is skipped, and its share goes back to the band
        // itself. A band whose profile is OFF has no measurement in it: it
        // never reduces, so nothing ever asked what its λ was, and
        // [`Denoise::refloor`] lays it the −100 dB placeholder that
        // [`DENOISE_THR_DB`]'s bottom works out to. Its ξ is therefore not
        // an opinion about signal, it is `energy / placeholder` — measured
        // at 7·10⁵ for an interior OFF band against 6·10⁻² for a live one
        // beside it — and a quarter of that lands on a live neighbour and
        // pins its gain at unity. Measured with bands 20..22 off at a dial
        // asking 12 dB, band 19 delivered 6.25 dB and band 23 delivered
        // 7.42; runs of one and two were safe only because no band in them
        // is interior, so the triangular refloor still gave them a real λ
        // from their live neighbours. OFF runs are an expected state, not a
        // corruption — `freeze_thresholds` documents a silent band landing
        // there as "the right answer", `set_thresholds` maps any non-finite
        // persisted value to OFF, and a lossy-decoded learn source drives
        // band 31 there through an ordinary learn pass.
        let mut sm = [0.0_f32; DENOISE_BANDS];
        for b in 0..DENOISE_BANDS {
            let vote = |k: usize| {
                if self.band[k].thr_norm < DENOISE_OFF {
                    xi[b]
                } else {
                    xi[k]
                }
            };
            let lo = vote(b.saturating_sub(1));
            let hi = vote((b + 1).min(DENOISE_BANDS - 1));
            sm[b] = 0.25 * lo + 0.5 * xi[b] + 0.25 * hi;
        }

        // ── the gain ────────────────────────────────────────────────────
        //
        // Three things changed here on 2026-09-16, all measured on the
        // owner's take against a render he prefers (the numbers are on
        // `DENOISE_ATTACK_MS` and `OPEN_SOFT_DB`):
        //
        // **The presence is gated by how far the band is over its print.**
        // `p` saturates on any evidence at all, so a quiet noise opened its
        // band as fully as a note. `open` is a knee on the band's excess
        // over its learned floor: a chair reads ~10 dB and opens part way,
        // a note reads 30 and opens fully, and the room between words
        // reads nothing and stays exactly on the dial.
        //
        // **The gain is a TARGET, and a one-pole in dB follows it** — up at
        // the attack, down at the release. The old gain was written fresh
        // every frame, which is a multiband gate switching at 5 ms, and
        // that switching was the digital sound. The mixture still lands
        // exactly on the dial in a noise-only frame, so the dial-truth
        // tests hold; they hold on the settled value, and the settling is
        // now a release rather than a step.
        //
        // **The treble shelf rides the presence.** It lifts what survives,
        // by up to the dial above `TREBLE_SHELF_HZ`, and never the residual:
        // the term is `p · shelf`, so a closed band gets none of it, and the
        // clamp's ceiling is the shelf rather than unity so the lift can be
        // heard at all.
        let mut g_lin = [1.0_f32; DENOISE_BANDS];
        for b in 0..DENOISE_BANDS {
            let off = self.band[b].thr_norm < DENOISE_OFF;
            let g_h1 = sm[b] / (1.0 + sm[b]);
            // Next frame's decision-directed history: the SPEECH estimate,
            // which is the H1 gain's work and not the floored output's —
            // the floor is a cosmetic minimum, not a belief about signal.
            self.sp.s_prev[b] = g_h1 * g_h1 * self.sp.e[b];
            if off {
                // A band with nothing learned in it never reduces. Same
                // contract the bank keeps, and the reason `thresholds` can
                // be all zeroes and mean "this unit is a wire".
                g_lin[b] = 1.0;
                self.band[b].depth_db = 0.0;
                self.sp.g_db[b] = 0.0;
                continue;
            }
            let excess_db = 10.0
                * (self.sp.e[b] / self.sp.floor_b[b].max(TINY)).max(TINY).log10();
            let open = smoothstep((excess_db - plan.open_lo) / (plan.open_hi - plan.open_lo));
            let p = pres[b].clamp(0.0, 1.0) * open;
            let h1_db = 20.0 * g_h1.max(1.0e-6).log10();
            let lift = plan.treble_db * self.sp.shelf[b];
            let target = (p * (h1_db + lift) + (1.0 - p) * plan.floor_db)
                .clamp(plan.floor_db, lift.max(0.0));
            let was = self.sp.g_db[b];
            let a = if target > was { plan.a_att } else { plan.a_rel };
            let now = if self.sp.fresh { target } else { was + a * (target - was) };
            let now = if now.is_finite() { now } else { plan.floor_db };
            self.sp.g_db[b] = now;
            g_lin[b] = 10.0_f32.powf(now / 20.0);
            // The meter reads the CUT the audio gets, per band, whichever
            // kernel is running; a lifted band reads as no cut.
            self.band[b].depth_db = -now.min(0.0);
        }
        self.sp.fresh = false;

        // ── back onto the bins, and mirrored ────────────────────────────
        for k in 0..=bins {
            let (b, frac) = (self.sp.map_lo[k] as usize, self.sp.map_frac[k]);
            let hi = if b + 1 < DENOISE_BANDS { g_lin[b + 1] } else { g_lin[b] };
            self.sp.gain[k] = (1.0 - frac) * g_lin[b] + frac * hi;
        }
        // The gain is real AND conjugate-symmetric, which is what makes it
        // one filter applied to both packed channels rather than two.
        for k in bins + 1..n {
            self.sp.gain[k] = self.sp.gain[n - k];
        }
    }

    /// Re-derive the per-bin noise floor and ceiling from the learned
    /// profile, and start the tracker from the floor.
    ///
    /// The profile is stored per BAND as a level — the mean power per bin,
    /// normalised by n, so it means the same at any frame length (see
    /// `Spectral::band_level`). Coming back the other way multiplies by the
    /// CURRENT n, which is what lets a profile learned at Studio's 1024
    /// points be read at Live's 512 without lying about how loud the room
    /// was. Interpolating it onto bins uses the same partition-of-unity
    /// weights as everything else, so a flat profile lands as a flat floor.
    fn refloor(&mut self) {
        let n = self.sp.n;
        if n == 0 {
            return;
        }
        let bins = n / 2;
        let ceil_mul = 10.0_f32.powf(NOISE_CEIL_DB / 10.0);
        let mut per_band = [0.0_f32; DENOISE_BANDS];
        for (p, b) in per_band.iter_mut().zip(self.band.iter()) {
            *p = 10.0_f32.powf(b.thr_db / 10.0) * n as f32;
        }
        let mut sum = 0.0_f32;
        let mut fb = [0.0_f32; DENOISE_BANDS];
        for k in 0..=bins {
            let (b, frac) = (self.sp.map_lo[k] as usize, self.sp.map_frac[k]);
            let hi = if b + 1 < DENOISE_BANDS { per_band[b + 1] } else { per_band[b] };
            let floor = ((1.0 - frac) * per_band[b] + frac * hi).max(1.0e-30);
            self.sp.floor[k] = floor;
            self.sp.ceil[k] = floor * ceil_mul;
            sum += floor;
            // The tracker starts AT the print rather than at zero: it may
            // only ever climb away from what it was taught, so opening
            // below the floor would just be one clamp's worth of nothing.
            self.sp.lambda[k] = floor;
            self.sp.ph1mean[k] = 0.5;
            // The band's own print, through the same weights `band_power`
            // sums a frame's energy with, so the two are comparable.
            fb[b] += (1.0 - frac) * floor;
            if b + 1 < DENOISE_BANDS {
                fb[b + 1] += frac * floor;
            }
        }
        // The whole print as one number, for the broadband gate. Summed
        // HERE and not per frame because it is a property of the profile,
        // not of the audio — the same reason `wsum` and `tau_dd` are laid
        // down in `lay_out_bands` rather than recomputed in the hot loop.
        self.sp.floor_sum = sum;
        self.sp.floor_b = fb.map(|v| v.max(1.0e-30));
        self.sp.s_prev = [0.0; DENOISE_BANDS];
    }


    /// The spectral path: stage the block into the frame machinery a hop at
    /// a time, firing a frame whenever one is full.
    ///
    /// The chunking is the point. Input arrives in slices of
    /// `min(remaining, R − fill)`, never one sample at a time, so a
    /// 512-sample block at 48 kHz is two ring copies and two frames rather
    /// than 512 trips round a branch. The FIFO is popped BEFORE the frame
    /// fires, which is what the one-hop priming pays for: the invariant
    /// `out_len ≥ R − stage_fill` holds at every chunk boundary, so a host
    /// asking for one sample at a time is served from the FIFO exactly as a
    /// host asking for 512 is.
    ///
    /// The input is read out of the buffer before the output is written
    /// into it, because they are the same buffer.
    /// The sidechain high-pass the bank's ear reads through, from the
    /// SC HPF dial: on or off, and the one-pole coefficient when on.
    fn sidechain_hpf(&self, params: &DenoiseParams) -> (bool, f32) {
        let sr = f64::from(self.sample_rate);
        let geo = |range: (f32, f32), t: f32| {
            f64::from(range.0) * f64::from(range.1 / range.0).powf(f64::from(t))
        };
        // The dial's own clamp, as `process` reads every knob.
        let hpf = if params.sc_hpf.is_finite() { params.sc_hpf.clamp(0.0, 1.0) } else { 0.0 };
        let hpf_on = hpf >= DENOISE_OFF;
        let a_hp = if hpf_on {
            (1.0 - (-std::f64::consts::TAU * geo(DENOISE_SC_HPF_HZ, hpf) / sr).exp()) as f32
        } else {
            0.0
        };
        (hpf_on, a_hp)
    }
    /// **The bank's ear, run during a learn on a spectral kernel.** One
    /// input frame through the same mono mid, the same sidechain high-pass
    /// and the same 32 resonators the bank listens with, and the same tick
    /// into `control` - which, learning, does nothing but accumulate the
    /// per-band statistics `bank_knees_proposed` reads at the falling edge.
    /// So a print a spectral kernel learns carries a bank knee measured the
    /// bank's own way, and switching to the bank afterwards gates where a
    /// bank learn of the same room would have. Costs nothing outside a
    /// pass, and a pass is a few seconds with a mouse held down.
    fn learn_feed(&mut self, l: f32, r: f32, hpf_on: bool, a_hp: f32) {
        let mut d = 0.5 * (l + r);
        if hpf_on {
            self.hp_lp = flush(self.hp_lp + a_hp * (d - self.hp_lp));
            d -= self.hp_lp;
        } else {
            self.hp_lp = d;
        }
        for (b, dz) in self.band.iter_mut().zip(self.design.iter()) {
            if dz.inert {
                continue;
            }
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
            let plan = TickPlan {
                learning: true,
                cap_db: 0.0,
                slope: 0.0,
                a_rel_slow: 0.0,
                a_rel_fast: 0.0,
            };
            self.control(&plan);
            self.phase = 0;
        }
    }
    /// **What the bank would have frozen**, per band, from the statistics
    /// its ear accumulated during the pass: mean + [`DENOISE_LEARN_SIGMA`]·σ
    /// + [`DENOISE_LEARN_HEADROOM_DB`], in dB, clamped to the threshold
    /// range - the arithmetic `freeze_thresholds` does for a bank learn.
    /// `None` for a band that heard fewer ticks than a pass needs.
    fn bank_knees_proposed(&self) -> [Option<f32>; DENOISE_BANDS] {
        let min_ticks = self.min_learn_ticks;
        let mut out = [None; DENOISE_BANDS];
        for (o, b) in out.iter_mut().zip(self.band.iter()) {
            if b.n < min_ticks {
                continue;
            }
            let mean = b.sum / b.n;
            let var = (b.sumsq / b.n - mean * mean).max(0.0);
            let thr = mean
                + f64::from(DENOISE_LEARN_SIGMA) * var.sqrt()
                + f64::from(DENOISE_LEARN_HEADROOM_DB);
            *o = Some((thr as f32).clamp(DENOISE_THR_DB.0, DENOISE_THR_DB.1));
        }
        out
    }
    fn process_spectral(
        &mut self,
        buf: &mut [f32],
        frames: usize,
        channels: usize,
        params: &DenoiseParams,
    ) {
        let (n, hop, cap) = (self.sp.n, self.sp.hop, self.sp.cap());
        if n == 0 {
            return;
        }
        let plan = self.spectral_plan(params);
        let (hpf_on, a_hp) = self.sidechain_hpf(params);
        let mut done = 0usize;
        while done < frames {
            let take = (hop - self.sp.stage_fill).min(frames - done);
            let mut w = self.sp.wpos;
            let mut o = self.sp.out_head;
            for i in 0..take {
                let f = (done + i) * channels;
                // Scrubbed on the way into the frame, for the reason the
                // bank's own read gives. Here it also keeps the OUTPUT
                // finite: an infinity in the analysed frame comes back out
                // of the inverse transform smeared across the whole hop.
                let (l, r) = (flush_bad(buf[f]), flush_bad(buf[f + 1]));
                self.sp.in_ring[2 * w] = l;
                self.sp.in_ring[2 * w + 1] = r;
                if self.learning {
                    self.learn_feed(l, r, hpf_on, a_hp);
                }
                w += 1;
                if w == n {
                    w = 0;
                }
                buf[f] = self.sp.fifo[2 * o];
                buf[f + 1] = self.sp.fifo[2 * o + 1];
                o += 1;
                if o == cap {
                    o = 0;
                }
            }
            self.sp.wpos = w;
            self.sp.out_head = o;
            self.sp.out_len -= take;
            self.sp.stage_fill += take;
            done += take;
            if self.sp.stage_fill == hop {
                self.sp.stage_fill = 0;
                self.sp.analyse();
                self.spectral_gains(&plan);
                self.sp.synthesise();
            }
        }
    }

    /// Every byte of Vec capacity in the unit, for the tests' proof that
    /// `process` never allocates: equal before and after means no internal
    /// buffer so much as considered growing. The same accessor the
    /// convolver keeps, for the same contract.
    #[cfg(test)]
    fn footprint(&self) -> usize {
        let v = |x: &Vec<f32>| x.capacity() * size_of::<f32>();
        self.ring.capacity() * size_of::<f32>()
            + v(&self.sp.win)
            + v(&self.sp.in_ring)
            + v(&self.sp.acc)
            + v(&self.sp.fifo)
            + v(&self.sp.re)
            + v(&self.sp.im)
            + v(&self.sp.gain)
            + v(&self.sp.pow)
            + v(&self.sp.lambda)
            + v(&self.sp.ph1mean)
            + v(&self.sp.ph1)
            + v(&self.sp.floor)
            + v(&self.sp.ceil)
            + v(&self.sp.map_frac)
            + self.sp.map_lo.capacity() * size_of::<u16>()
    }

    /// Drop every trace of what was playing: bells, resonators, envelopes,
    /// depths, the lookahead line, the frame machinery, any half-done learn
    /// statistics. The learned THRESHOLDS stay — they are settings, not
    /// signal, exactly as a cleared EQ keeps its curve.
    pub fn clear(&mut self) {
        for b in self.band.iter_mut() {
            b.quiesce();
        }
        self.fresh = true;
        self.ring.iter_mut().for_each(|s| *s = 0.0);
        self.rpos = 0;
        self.phase = 0;
        self.hp_lp = 0.0;
        self.sp.reset();
        self.refloor();
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

    /// Deterministic WHITE noise — the same xorshift32 core as [`Noise`]
    /// with nothing filtered off it, so its power is flat per hertz from
    /// DC to Nyquist and every one of the 32 bands is handed real energy.
    ///
    /// It exists BESIDE [`Noise`] rather than replacing it because the two
    /// fixtures hide opposite things, and this unit's worst behaviour is
    /// in the half [`Noise`] cannot reach. That fixture runs its whitened
    /// source through a one-pole high-pass at 150 Hz and a one-pole
    /// low-pass at 2 kHz, which parks essentially all of its energy
    /// between roughly 150 Hz and 2 kHz — the dense middle of the grid,
    /// where a probe frequency sits under the overlapping skirts of three
    /// or four bells at once and the bank's summation error happens to
    /// flatter it. Above 2 kHz the fixture's own low-pass is already
    /// rolling off at 6 dB/octave, so by 16 kHz there is nothing left to
    /// measure at all: every band from about 4 kHz up is asked about
    /// silence, answers "nothing here", and passes. That is how a green
    /// suite has coexisted with a unit that audibly still hisses — the top
    /// octave, which is exactly where hiss lives and exactly where the
    /// grid runs out at 15.5 kHz and hands the rest of the spectrum to a
    /// single shelf, was never exercised by anything.
    ///
    /// Real programme noise is not band-limited to the region a design is
    /// good at, and neither is this. Interleaved stereo, both channels the
    /// same signal, like [`Noise`] — the detector is mid anyway. No sample
    /// rate is taken, because white noise is defined without one: every
    /// rate gets the same flat spectrum out to its own Nyquist, which is
    /// the property a rate-invariance check actually wants, rather than a
    /// fixture whose corner frequencies must be re-derived per rate.
    struct FullBandNoise {
        seed: u32,
        amp: f32,
    }

    impl FullBandNoise {
        fn new(seed: u32, amp: f32) -> Self {
            FullBandNoise { seed, amp }
        }

        fn next(&mut self) -> f32 {
            self.seed ^= self.seed << 13;
            self.seed ^= self.seed >> 17;
            self.seed ^= self.seed << 5;
            ((self.seed as f32 / u32::MAX as f32) * 2.0 - 1.0) * self.amp
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

    /// The PHASE of the same Hann-windowed correlation [`tone_db`] takes
    /// the magnitude of, in radians, over `[from, from+len)` frames of the
    /// left channel.
    ///
    /// It exists because the owner's first symptom is a phase symptom, and
    /// an argument about phase settled with a magnitude measurement is not
    /// settled. Correlating rather than differencing waveforms means the
    /// noise the tone is buried in averages away, and taking the phase from
    /// the same correlation the level comes from means the two numbers
    /// cannot disagree about which frequency they are describing.
    fn tone_phase(buf: &[f32], hz: f32, sr: f32, from: usize, len: usize) -> f32 {
        let (mut re, mut im) = (0.0_f64, 0.0_f64);
        for n in 0..len {
            let x = f64::from(buf[(from + n) * 2]);
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / len as f64).cos();
            let ph = std::f64::consts::TAU * f64::from(hz) * n as f64 / f64::from(sr);
            re += x * w * ph.cos();
            im += x * w * ph.sin();
        }
        im.atan2(re) as f32
    }

    /// Test-only measurement tools. Nothing here is compiled into the
    /// unit; it exists so that claims about this bank can be made as
    /// numbers instead of adjectives, and it is deliberately written to be
    /// built on — the figures the target tests below print are the
    /// baseline every later commit on this unit gets judged against.
    ///
    /// Three tools, one per question this unit keeps being asked:
    ///
    /// * [`settled_response_db`] — WHAT IS THE BANK DOING TO THE SPECTRUM
    ///   RIGHT NOW. This is the one the suite was missing. The meter's
    ///   [`Denoise::depth_db`] reports each bell's own design depth, and
    ///   it is NOT the reduction any frequency receives: the bells are
    ///   peaking cuts at Q 5.5 on a 0.26-octave grid, so their skirts
    ///   overlap by construction and the composite is a sum, not a copy,
    ///   of the per-band numbers. Believing the meter is how a bank can be
    ///   "12 dB down" everywhere on its own display while the audio is
    ///   21 dB down at 100 Hz and 8 dB down at 16 kHz. The only honest
    ///   read is to run an impulse through the exact cascade the audio
    ///   path is applying and transform it.
    /// * [`gain_trajectory`] — HOW THE DEPTHS GOT THERE. A settled figure
    ///   says nothing about whether it settled, and it is the TRAVEL that
    ///   the owner hears as phase artefacts: a minimum-phase cut moves
    ///   phase whenever its gain moves, so every trajectory here is also
    ///   a phase trajectory. Anything measuring the composite response has
    ///   to be able to prove the bank had stopped moving first.
    /// * [`tone_db`] — WHAT HAPPENED TO ONE NARROWBAND THING. Already
    ///   written above and re-exported here so the three sit together;
    ///   read its own comment for why the window is Hann and not
    ///   rectangular. [`composite_cut_at`] builds on it to give the FFT a
    ///   second opinion computed without an FFT at all, which is the one
    ///   safeguard this file has against arguing a DSP rewrite from a
    ///   measurement bug. As of 2026-09-02 the two routes agree to
    ///   between 0.009 and 0.025 dB across the four settings.
    ///
    /// [`power_spectrum`] and [`band_power_db`] fall out of the same FFT
    /// and answer the fourth question, which is the only one a listener
    /// actually asks: how much energy left the signal, in a named slice of
    /// the spectrum. Coefficients promise; this measures.
    mod analysis {
        use super::*;

        /// The Hann-windowed narrowband correlator, re-exported so all the
        /// measurement tools are reachable under one name (see the module
        /// note above). [`composite_cut_at`] is what puts it to work here.
        pub(super) use super::tone_db;

        /// How long an impulse response gets transformed. 32768 points is
        /// 683 ms at 48 kHz and puts 1.46 Hz between bins — long enough
        /// that the narrowest thing in the bank (the 60 Hz bell at Q 5.5,
        /// so about 11 Hz wide and ringing for roughly 30 ms) has decayed
        /// to nothing well before the window ends. That matters: truncate
        /// an IIR tail and the transform reports the truncation instead of
        /// the filter, and every number below would be quietly wrong in
        /// the low bands — which are precisely the bands where the
        /// overlap error is largest.
        const FFT_N: usize = 32_768;

        /// The segment length for the averaged spectra. 4096 points is
        /// 11.7 Hz per bin at 48 kHz and, over a one-second window at 50%
        /// overlap, averages 22 periodograms — enough that a band figure
        /// is a measurement rather than one draw from a chi-square. Short
        /// segments are the right trade here because every band this file
        /// integrates over is hundreds of bins wide; resolution is free
        /// and variance is not.
        const PSD_N: usize = 4_096;

        /// A radix-2 Cooley–Tukey FFT, in place, over parallel real and
        /// imaginary arrays whose length must be a power of two. Written
        /// out rather than pulled in because the house rule is no new
        /// crates and this is forty lines of textbook; it runs in test
        /// code only, where allocating freely costs nothing.
        pub(super) fn fft(re: &mut [f64], im: &mut [f64]) {
            let n = re.len();
            assert_eq!(n, im.len(), "the FFT halves must be the same length");
            assert!(n.is_power_of_two(), "the FFT length must be a power of two");

            // Decimation in time: permute into bit-reversed order first,
            // then every butterfly stage reads and writes in place.
            let mut j = 0_usize;
            let mut i = 1_usize;
            while i < n {
                let mut bit = n >> 1;
                while j & bit != 0 {
                    j ^= bit;
                    bit >>= 1;
                }
                j |= bit;
                if i < j {
                    re.swap(i, j);
                    im.swap(i, j);
                }
                i += 1;
            }

            let mut len = 2_usize;
            while len <= n {
                let ang = -std::f64::consts::TAU / len as f64;
                let (wr, wi) = (ang.cos(), ang.sin());
                let half = len / 2;
                let mut base = 0_usize;
                while base < n {
                    // The twiddle is advanced by repeated multiplication
                    // rather than recomputed per butterfly. At 32768
                    // points the accumulated drift is far below anything
                    // a dB figure can show, and this stays readable.
                    let (mut cr, mut ci) = (1.0_f64, 0.0_f64);
                    let mut k = 0_usize;
                    while k < half {
                        let (ur, ui) = (re[base + k], im[base + k]);
                        let (xr, xi) = (re[base + k + half], im[base + k + half]);
                        let (vr, vi) = (xr * cr - xi * ci, xr * ci + xi * cr);
                        re[base + k] = ur + vr;
                        im[base + k] = ui + vi;
                        re[base + k + half] = ur - vr;
                        im[base + k + half] = ui - vi;
                        let ncr = cr * wr - ci * wi;
                        ci = cr * wi + ci * wr;
                        cr = ncr;
                        k += 1;
                    }
                    base += len;
                }
                len <<= 1;
            }
        }

        /// The composite magnitude response, in dB of GAIN (so a cut is
        /// negative), of the bell cascade exactly as it stands right now,
        /// sampled at each frequency in `hz`.
        ///
        /// It reads the live coefficients out of the unit and runs a unit
        /// impulse through a fresh copy of the cascade, so the measurement
        /// cannot disturb the state it is measuring — the running
        /// biquads' own delay registers are left alone. Only bands whose
        /// bell is `active` are in the chain, which is the same test the
        /// audio path makes, so a skipped band contributes exactly the
        /// nothing it contributes in the product. `Biquad::run`'s denormal
        /// flush is left in place on purpose: it is what ships, and the
        /// tail it eats is 400 dB down.
        ///
        /// Bins are interpolated linearly in dB. The response is smooth
        /// on the scale of a bin — the narrowest feature in the bank is
        /// about 11 Hz wide against 1.46 Hz bins — so the interpolation is
        /// not where any error in these numbers comes from.
        pub(super) fn settled_response_db(dn: &Denoise, hz: &[f32], sr: f32) -> Vec<f32> {
            let mut chain: Vec<(Coef, Biquad)> = dn
                .band
                .iter()
                .filter(|b| b.active)
                .map(|b| (b.coef, Biquad::default()))
                .collect();

            let mut re = vec![0.0_f64; FFT_N];
            let mut im = vec![0.0_f64; FFT_N];
            for (i, slot) in re.iter_mut().enumerate() {
                let mut x = if i == 0 { 1.0_f32 } else { 0.0_f32 };
                for (c, q) in chain.iter_mut() {
                    x = q.run(c, x);
                }
                *slot = f64::from(x);
            }
            fft(&mut re, &mut im);

            let bin_hz = f64::from(sr) / FFT_N as f64;
            let mag_db = |k: usize| {
                let p = re[k] * re[k] + im[k] * im[k];
                10.0 * p.max(1.0e-30).log10()
            };
            hz.iter()
                .map(|&f| {
                    let x = (f64::from(f) / bin_hz).clamp(0.0, (FFT_N / 2 - 1) as f64);
                    let k = x.floor() as usize;
                    let t = x - k as f64;
                    (mag_db(k) * (1.0 - t) + mag_db(k + 1) * t) as f32
                })
                .collect()
        }

        /// The composite magnitude response, in dB of GAIN (so a cut is
        /// negative), of the SPECTRAL kernel exactly as it stands right
        /// now, sampled at each frequency in `hz`.
        ///
        /// The spectral twin of [`settled_response_db`], and simpler for a
        /// reason worth naming: there is no filter to excite and no
        /// transform to take, because the kernel's response IS an array of
        /// per-bin gains. It reads them out of the unit without disturbing
        /// anything, which is the same discipline the bank version keeps
        /// with its fresh copy of the cascade.
        ///
        /// Bins are interpolated linearly in dB. The mask is smooth on the
        /// scale of a bin by construction — it is a triangular
        /// interpolation across bands hundreds of hertz wide, which is
        /// exactly the property the band structure exists to buy — so the
        /// interpolation is not where any error in these numbers comes
        /// from.
        pub(super) fn spectral_response_db(dn: &Denoise, hz: &[f32], sr: f32) -> Vec<f32> {
            let n = dn.sp.n;
            let bins = n / 2;
            let bin_hz = f64::from(sr) / n as f64;
            let db = |k: usize| 20.0 * f64::from(dn.sp.gain[k]).max(1.0e-12).log10();
            hz.iter()
                .map(|&f| {
                    let x = (f64::from(f) / bin_hz).clamp(0.0, bins as f64);
                    let k = x.floor() as usize;
                    let t = x - k as f64;
                    (db(k) * (1.0 - t) + db(k.min(bins - 1) + 1) * t) as f32
                })
                .collect()
        }

        /// The cut at one frequency in dB, taken from the AUDIO: the drop
        /// in power between a dry and a wet spectrum inside a narrow slice
        /// around `hz`.
        ///
        /// A twelfth of an octave either side — wide enough at 11.7 Hz per
        /// bin to hold a real number of bins at 1 kHz (about fourteen) and
        /// narrow enough at 14 kHz not to average two different answers
        /// together. It exists to keep [`spectral_response_db`] honest:
        /// reading a gain array and reporting the gain array proves nothing
        /// on its own, and everything this unit's rewrite is argued from
        /// wants a second opinion arrived at a completely different way.
        pub(super) fn psd_cut_at(dry: &[f64], wet: &[f64], sr: f32, hz: f32) -> f32 {
            let edge = 2.0_f32.powf(1.0 / 24.0);
            band_power_db(dry, sr, hz / edge, hz * edge)
                - band_power_db(wet, sr, hz / edge, hz * edge)
        }

        /// The composite cut at ONE frequency, in positive dB, measured by
        /// pushing a real tone through a fresh copy of the settled cascade
        /// and correlating it back out with [`tone_db`] — rather than by
        /// transforming an impulse, which is what
        /// [`settled_response_db`] does.
        ///
        /// It exists to keep that function honest. It is new code — a
        /// hand-written radix-2 FFT plus an interpolation between bins —
        /// and later commits on this unit are going to be argued from its
        /// output, so it gets a second opinion arrived at a completely
        /// different way: no transform, no bins, no interpolation, just a
        /// sine through the filters and a correlation. Steady state, not
        /// impulse — the tone runs for 200 ms before the correlation
        /// window opens, so the bells' startup ringing is long dead and
        /// what comes back is the response rather than the transient.
        /// When both are right they agree to a few hundredths of a dB;
        /// when they disagree, one of them is broken and the number is in
        /// the failure text.
        pub(super) fn composite_cut_at(dn: &Denoise, hz: f32, sr: f32) -> f32 {
            let settle = (sr * 0.2) as usize;
            let window = (sr * 0.5) as usize;
            let mut buf = sine(hz, 1.0, settle + window, sr);
            let dry = tone_db(&buf, hz, sr, settle, window);

            let mut chain: Vec<(Coef, Biquad)> = dn
                .band
                .iter()
                .filter(|b| b.active)
                .map(|b| (b.coef, Biquad::default()))
                .collect();
            for f in buf.chunks_exact_mut(2) {
                let mut x = f[0];
                for (c, q) in chain.iter_mut() {
                    x = q.run(c, x);
                }
                f[0] = x;
                f[1] = x;
            }
            dry - tone_db(&buf, hz, sr, settle, window)
        }

        /// Push `frames` of the interleaved stereo `buf` through `dn` in
        /// `chunk`-frame slices, recording every band's applied cut after
        /// each slice.
        ///
        /// The snapshots are `chunk` frames apart. At 48 kHz the control
        /// tick is 32 samples, so a chunk of 64 gives a 1.33 ms grid —
        /// two ticks, which is as fine as [`Denoise::depth_db`] can
        /// honestly be sampled from outside; asking for less than a tick
        /// just returns the same numbers twice.
        pub(super) fn gain_trajectory(
            dn: &mut Denoise,
            buf: &mut [f32],
            frames: usize,
            chunk: usize,
            params: &DenoiseParams,
        ) -> Vec<[f32; DENOISE_BANDS]> {
            let chunk = chunk.max(1);
            let mut out = Vec::with_capacity(frames / chunk + 1);
            let mut done = 0_usize;
            while done < frames {
                let n = chunk.min(frames - done);
                dn.process(&mut buf[done * 2..(done + n) * 2], n, 2, params);
                out.push(dn.depth_db());
                done += n;
            }
            out
        }

        /// The averaged power spectrum of the LEFT channel over
        /// `[from, from + len)` frames: Hann-windowed [`PSD_N`] segments
        /// at 50% overlap, periodograms averaged, `PSD_N / 2 + 1` bins of
        /// power out.
        ///
        /// The window's own gain is deliberately NOT normalised away.
        /// Every figure this file takes from a spectrum is a DIFFERENCE
        /// between two spectra measured the same way — the same window,
        /// the same segmentation, the same length — and in a difference
        /// the window's gain cancels exactly. Normalising it would add a
        /// constant to both sides and a rounding error to neither.
        pub(super) fn power_spectrum(buf: &[f32], from: usize, len: usize) -> Vec<f64> {
            let win: Vec<f64> = (0..PSD_N)
                .map(|n| 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / PSD_N as f64).cos())
                .collect();
            let mut acc = vec![0.0_f64; PSD_N / 2 + 1];
            let mut segments = 0_usize;
            let mut start = from;
            while start + PSD_N <= from + len && (start + PSD_N) * 2 <= buf.len() {
                let mut re = vec![0.0_f64; PSD_N];
                let mut im = vec![0.0_f64; PSD_N];
                for (n, (r, w)) in re.iter_mut().zip(win.iter()).enumerate() {
                    *r = f64::from(buf[(start + n) * 2]) * w;
                }
                fft(&mut re, &mut im);
                for (k, a) in acc.iter_mut().enumerate() {
                    *a += re[k] * re[k] + im[k] * im[k];
                }
                segments += 1;
                start += PSD_N / 2;
            }
            let scale = 1.0 / segments.max(1) as f64;
            acc.iter_mut().for_each(|a| *a *= scale);
            acc
        }

        /// Total power in `[lo, hi)` Hz of a [`power_spectrum`] result, in
        /// dB. A bin belongs to the slice when its centre does; with
        /// 11.7 Hz bins and slices hundreds of bins wide, which side of an
        /// edge one bin lands on is far below the numbers being compared.
        pub(super) fn band_power_db(psd: &[f64], sr: f32, lo: f32, hi: f32) -> f32 {
            let bin_hz = f64::from(sr) / PSD_N as f64;
            let mut total = 0.0_f64;
            for (k, &p) in psd.iter().enumerate() {
                let f = k as f64 * bin_hz;
                if f >= f64::from(lo) && f < f64::from(hi) {
                    total += p;
                }
            }
            (10.0 * total.max(1.0e-300).log10()) as f32
        }
    }

    /// The shipped dials with one kernel selected — every test that is not
    /// ABOUT the selector says which engine it means, because the two
    /// answer differently on purpose and a test that does not name one is
    /// a test whose meaning changes when the default does.
    /// **Render a real recording through the unit, offline**, so a change
    /// to the kernel is measured against a reference on the owner's own
    /// take rather than felt. Ignored: it reads paths from the environment.
    ///
    /// ```text
    /// TANGENT_DN_IN=/path/control.wav TANGENT_DN_OUT=/path/out.wav \
    /// TANGENT_DN_LEARN=26.95,28.85 [TANGENT_DN_REDUCE=0.5] \
    /// [TANGENT_DN_ATTACK=..] [TANGENT_DN_RELEASE=..] [TANGENT_DN_SMOOTH=..] \
    /// [TANGENT_DN_TREBLE=..] [TANGENT_DN_KERNEL=0.5] \
    /// cargo test -p tangent --bin tangent -- render_a_take_through_the_denoiser --ignored --nocapture
    /// ```
    ///
    /// The learn pass is the named stretch of the SAME file, played once
    /// with the toggle up; then the whole file goes through in 512-frame
    /// blocks, which is what the host does. Written as 32-bit float at the
    /// file's own rate, delayed by exactly the unit's reported latency so
    /// the output lines up with the input sample for sample.
    #[test]
    #[ignore]
    fn render_a_take_through_the_denoiser() {
        let var = |k: &str| std::env::var(k).ok();
        let Some(input) = var("TANGENT_DN_IN") else {
            eprintln!("TANGENT_DN_IN not set; nothing rendered");
            return;
        };
        let out = var("TANGENT_DN_OUT").expect("TANGENT_DN_OUT");
        let learn = var("TANGENT_DN_LEARN").expect("TANGENT_DN_LEARN=start,end seconds");
        let (l0, l1) = learn.split_once(',').expect("start,end");
        let (l0, l1): (f32, f32) = (l0.trim().parse().unwrap(), l1.trim().parse().unwrap());
        let dial = |k: &str, rest: f32| var(k).and_then(|v| v.parse().ok()).unwrap_or(rest);
        let wav = crate::wavread::read(std::path::Path::new(&input)).expect("read input");
        let sr = wav.rate as f32;
        let left = &wav.channels[0];
        let right = wav.channels.get(1).unwrap_or(left);
        let frames = left.len().min(right.len());
        let mut dn = Denoise::new(sr);
        let mut params = DenoiseParams {
            reduce: dial("TANGENT_DN_REDUCE", 0.5),
            release: dial("TANGENT_DN_RELEASE", DenoiseParams::default().release),
            smooth: dial("TANGENT_DN_SMOOTH", DenoiseParams::default().smooth),
            kernel: dial("TANGENT_DN_KERNEL", DenoiseParams::default().kernel),
            ..DenoiseParams::default()
        };
        // Every knob a newer build may have grown is read the same way, so
        // this harness need not be edited when one lands.
        params.apply_env_overrides(&dial);
        // The print: either the bay's own saved state — `TANGENT_DN_STATE`
        // names an `insert-state-<seat>-<bay>.bin`, the text blob the host
        // writes, and its `t0..t31` lines ARE the thresholds — or the named
        // stretch of this file, toggle up, then the toggle down.
        let mut tiny = [0.0_f32; 8];
        if let Some(state) = var("TANGENT_DN_STATE") {
            let text = std::fs::read_to_string(&state).expect("read state blob");
            let mut thr = [0.0_f32; DENOISE_BANDS];
            let mut found = 0;
            for line in text.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    if let Some(i) = k.strip_prefix('t').and_then(|n| n.parse::<usize>().ok()) {
                        if i < DENOISE_BANDS {
                            thr[i] = v.trim().parse().unwrap_or(0.0);
                            found += 1;
                        }
                    }
                }
            }
            assert_eq!(found, DENOISE_BANDS, "the blob carried {found} thresholds");
            dn.process(&mut tiny, 4, 2, &params);
            dn.set_thresholds(&thr);
            eprintln!("thresholds from {state}; latency {} frames", dn.latency());
        } else {
            let (a, b) = (((l0 * sr) as usize).min(frames), ((l1 * sr) as usize).min(frames));
            let mut seg: Vec<f32> = (a..b).flat_map(|i| [left[i], right[i]]).collect();
            let learning = DenoiseParams { learn: 1.0, ..params };
            for chunk in seg.chunks_mut(1024) {
                dn.process(chunk, chunk.len() / 2, 2, &learning);
            }
            dn.process(&mut tiny, 4, 2, &params);
            eprintln!("learned {:?} over {:.2}s; latency {} frames", dn.last_learn(), l1 - l0, dn.latency());
        }
        // The take, in host-sized blocks, then flushed by the latency.
        let lat = dn.latency() as usize;
        let mut buf: Vec<f32> = (0..frames).flat_map(|i| [left[i], right[i]]).collect();
        buf.extend(std::iter::repeat_n(0.0, lat * 2));
        for chunk in buf.chunks_mut(512 * 2) {
            dn.process(chunk, chunk.len() / 2, 2, &params);
        }
        let spec = tangent_record::wav::WavSpec {
            sample_rate: wav.rate,
            channels: 2,
            format: tangent_record::wav::SampleFormat::Float32,
        };
        let bext = tangent_record::wav::Bext::new(tangent_record::wav::Wallclock::now_utc(), spec);
        let mut w = tangent_record::wav::WavWriter::create(std::path::Path::new(&out), spec, &bext)
            .expect("create output");
        w.write_interleaved(&buf[lat * 2..(lat + frames) * 2]).expect("write");
        w.finish().expect("finish");
        eprintln!("wrote {out}");
    }

    /// The band a frequency lands in, for the spectral kernel that is up.
    fn band_at(dn: &Denoise, hz: f32, sr: f32) -> usize {
        let k = (hz * dn.sp.n as f32 / sr).round() as usize;
        dn.sp.map_lo[k.min(dn.sp.n / 2)] as usize
    }

    /// Run `buf` through in hop-sized blocks and return band `b`'s smoothed
    /// gain in dB after every block — the thing the ear hears move.
    fn gain_db_trace(dn: &mut Denoise, buf: &mut [f32], p: &DenoiseParams, b: usize) -> Vec<f32> {
        let hop = dn.sp.hop.max(1);
        let frames = buf.len() / 2;
        let mut out = Vec::with_capacity(frames / hop + 1);
        let mut done = 0;
        while done < frames {
            let n = hop.min(frames - done);
            dn.process(&mut buf[done * 2..(done + n) * 2], n, 2, p);
            out.push(dn.sp.g_db[b]);
            done += n;
        }
        out
    }

    /// **The shipped release is a hundred milliseconds** on the geometric
    /// sweep — the owner's number, pinned so a change to the range moves
    /// the default with it rather than silently shipping a different time.
    #[test]
    fn the_shipped_release_is_a_hundred_milliseconds() {
        let d = DenoiseParams::default();
        let ms = |r: (f32, f32), t: f32| r.0 * (r.1 / r.0).powf(t);
        let release = ms(DENOISE_RELEASE_MS, d.release);
        assert!((release - 100.0).abs() < 1.0, "the shipped release is {release:.1} ms");
        let attack = ms(DENOISE_ATTACK_MS, d.attack);
        assert!((attack - 10.0).abs() < 0.1, "the shipped attack is {attack:.2} ms");
        assert_eq!(d.smooth, 0.0, "the shipped knee is not the softest");
        assert!((d.treble * DENOISE_TREBLE_DB - 2.0).abs() < 1e-6, "the shipped treble boost is not 2 dB");
    }

    /// **A band opens at the attack and closes at the release**, and the
    /// two dials move those times. This is the gate-switching-at-5-ms
    /// fault, written as the numbers it has to be instead of.
    #[test]
    fn a_band_opens_at_the_attack_and_closes_at_the_release() {
        let sr = SR;
        let kernel = DenoiseKernel::SpectralStudio;
        // A burst of a 1 kHz tone, 30 dB over the room, between two
        // stretches of the room alone. Returns the band's gain trace, the
        // frame in ms, and the indices where the tone starts and stops.
        let burst = |attack: f32, release: f32| -> (Vec<f32>, f32, usize, usize) {
            let mut dn = learned_full_band(kernel, 0xA77A, 0.05, sr, 1.0);
            let p = DenoiseParams { attack, release, ..on(kernel) };
            let (pre, on_len, post) = (sr as usize, (0.4 * sr) as usize, (1.5 * sr) as usize);
            let mut buf = FullBandNoise::new(0xA77A, 0.05).frames(pre + on_len + post);
            add(&mut buf[pre * 2..(pre + on_len) * 2], &sine(1_000.0, 0.3, on_len, sr));
            let b = band_at(&dn, 1_000.0, sr);
            let hop = dn.sp.hop;
            let t = gain_db_trace(&mut dn, &mut buf, &p, b);
            (t, 1_000.0 * hop as f32 / sr, pre / hop, (pre + on_len) / hop)
        };
        let floor = -(0.5 * DENOISE_REDUCE_DB);
        let (t, ms, on_at, off_at) = burst(0.5, 0.197);
        let at = |t: &[f32], from: usize, ms_after: f32| t[(from as f32 + ms_after / ms).round() as usize];
        assert!(
            (at(&t, on_at, -20.0) - floor).abs() < 0.75,
            "before the burst the band was not on the dial: {:.2}",
            at(&t, on_at, -20.0)
        );
        let plateau = t[off_at - 20..off_at].iter().sum::<f32>() / 20.0;
        assert!(plateau > -1.0, "a 30 dB tone did not open its band: plateau {plateau:.2} dB");
        let opened = (0..40)
            .find(|&i| t[on_at + i] > plateau - 1.0)
            .map(|i| i as f32 * ms);
        assert!(opened.is_some_and(|o| o <= 60.0), "the band took {opened:?} ms to open at a 10 ms attack");
        assert!(
            at(&t, off_at, 40.0) > floor + 4.0,
            "40 ms after the tone a 100 ms release had already shut the band: {:.2}",
            at(&t, off_at, 40.0)
        );
        assert!(
            (at(&t, off_at, 700.0) - floor).abs() < 1.0,
            "700 ms after the tone the band was not back on the dial: {:.2}",
            at(&t, off_at, 700.0)
        );
        // Release at the top of its travel: still well open 250 ms later.
        let (t8, _, _, off8) = burst(0.5, 1.0);
        assert!(at(&t8, off8, 250.0) > floor + 4.0, "an 800 ms release had shut the band within 250 ms");
        // Attack at the top of its travel is slower than the shipped one.
        let (t1, _, on1, off1) = burst(1.0, 0.197);
        let plateau1 = t1[off1 - 20..off1].iter().sum::<f32>() / 20.0;
        let opened1 = (0..80)
            .find(|&i| t1[on1 + i] > plateau1 - 1.0)
            .map(|i| i as f32 * ms);
        assert!(opened1.is_some_and(|o| o >= 80.0), "a 100 ms attack opened in {opened1:?} ms");
    }

    /// **A quiet event opens its band only part way**: the chair, the
    /// breath, the thing 10 dB over the room — the sounds the owner asked
    /// this pass to look for — no longer bring the whole band to unity.
    #[test]
    fn a_quiet_event_opens_a_band_only_part_way() {
        let sr = SR;
        let kernel = DenoiseKernel::SpectralStudio;
        // **A BROADBAND event, like the ones on the take.** A lone quiet
        // tone is a different case: it moves one band of thirty-two, the
        // frame reads as room to the broadband gate, and the tracker absorbs
        // it into the noise estimate — which is that gate's whole purpose. A
        // chair, a breath, a page turn are broadband; the frame reads over
        // the gate, the estimate freezes, and the presence alone decides.
        //
        // Sustained and well over the gate's top, because anything under
        // about 12 dB the tracker absorbs on its own (see `OPEN_SOFT_DB`).
        // The expectation is derived from the knee rather than written as
        // a number, so a retuned knee moves the test with it.
        let run = |over_db: f32| -> (f32, f32) {
            let mut dn = learned_full_band(kernel, 0xC0DE, 0.05, sr, 1.0);
            let p = on(kernel);
            let (pre, on_len) = (sr as usize, (0.8 * sr) as usize);
            let mut dry = FullBandNoise::new(0xC0DE, 0.05).frames(pre + on_len);
            let amp = 0.05 * 10.0_f32.powf(over_db / 20.0);
            add(&mut dry[pre * 2..], &FullBandNoise::new(0xE4E47, amp).frames(on_len));
            let b = band_at(&dn, 1_000.0, sr);
            // `process` writes its output over its input, so the trace and
            // the probe each get their own copy of the dry signal.
            let mut buf = dry.clone();
            let t = gain_db_trace(&mut dn, &mut buf, &p, b);
            let settled = t[t.len() - 30..].iter().sum::<f32>() / 30.0;
            let mut probe = learned_full_band(kernel, 0xC0DE, 0.05, sr, 1.0);
            let mut most = dry[..(pre + on_len * 3 / 4) * 2].to_vec();
            let _ = gain_db_trace(&mut probe, &mut most, &p, b);
            let excess = 10.0 * (probe.sp.e[b] / probe.sp.floor_b[b]).log10();
            (settled, excess)
        };
        let floor = -(0.5 * DENOISE_REDUCE_DB);
        let expect = |excess: f32| {
            let open = smoothstep((excess - OPEN_SOFT_DB.0) / (OPEN_SOFT_DB.1 - OPEN_SOFT_DB.0));
            (1.0 - open) * floor
        };
        let (mid, ex_m) = run(18.0);
        let (loud, ex_l) = run(32.0);
        assert!(
            (15.0..=25.0).contains(&ex_m),
            "the mid burst reads {ex_m:.1} dB over the print; the test wants it inside the knee"
        );
        assert!(ex_l > OPEN_SOFT_DB.1, "the loud burst reads only {ex_l:.1} dB over the print");
        let want = expect(ex_m);
        assert!(
            (mid - want).abs() < 2.0 && mid < -1.5 && mid > floor + 1.0,
            "a {ex_m:.0} dB event settled at {mid:.2} dB; the knee says {want:.2} (floor {floor})"
        );
        assert!(loud > -1.0, "a {ex_l:.0} dB burst settled at {loud:.2} dB");
    }

    /// **The treble boost lifts the signal and never the residual.**
    #[test]
    fn the_treble_boost_lifts_the_signal_and_never_the_residual() {
        let sr = SR;
        let kernel = DenoiseKernel::SpectralStudio;
        let run = |treble: f32| -> (f32, f32) {
            let mut dn = learned_full_band(kernel, 0x7EB1E, 0.05, sr, 1.0);
            let p = DenoiseParams { treble, ..on(kernel) };
            let (pre, on_len) = (sr as usize, (0.6 * sr) as usize);
            let mut buf = FullBandNoise::new(0x7EB1E, 0.05).frames(pre + on_len);
            add(&mut buf[pre * 2..], &sine(8_000.0, 0.3, on_len, sr));
            let b = band_at(&dn, 8_000.0, sr);
            let hop = dn.sp.hop;
            let t = gain_db_trace(&mut dn, &mut buf, &p, b);
            let gap = t[pre / hop - 20..pre / hop].iter().sum::<f32>() / 20.0;
            (gap, t[t.len() - 20..].iter().sum::<f32>() / 20.0)
        };
        let floor = -(0.5 * DENOISE_REDUCE_DB);
        let (gap2, sig2) = run(2.0 / DENOISE_TREBLE_DB);
        let (gap0, sig0) = run(0.0);
        assert!(
            (gap2 - floor).abs() < 0.75 && (gap0 - floor).abs() < 0.75,
            "the residual moved with the treble dial: {gap0:.2} vs {gap2:.2} (dial {floor})"
        );
        assert!((sig2 - 2.0).abs() < 0.8, "a +2 dB boost lifted an 8 kHz tone by {sig2:.2} dB");
        assert!(sig0.abs() < 0.8, "with the boost off the tone read {sig0:.2} dB");
    }

    fn on(kernel: DenoiseKernel) -> DenoiseParams {
        DenoiseParams {
            kernel: kernel.dial(),
            // SMOOTH is the bank's expansion RATIO, and every bank
            // measurement in this module was taken at the 2.8:1 the dial's
            // middle gives. The shipped default is the spectral kernels'
            // softest knee, which on the bank is 1.7:1 and cuts less; the
            // bank is tested where it was measured.
            smooth: if kernel == DenoiseKernel::Bank { 0.5 } else { DenoiseParams::default().smooth },
            // And its RELEASE, for the same reason: the bank's block-by-block
            // step sizes were measured at the 219 ms the middle gives.
            release: if kernel == DenoiseKernel::Bank { 0.5 } else { DenoiseParams::default().release },
            ..DenoiseParams::default()
        }
    }

    /// Learn `seconds` of the given noise on `kernel` at otherwise shipped
    /// dials, freeze, and hand the unit back ready to suppress. A profile
    /// belongs to the engine that learned it (the bank measures resonator
    /// band energy, the spectral kernels measure a windowed periodogram),
    /// so the kernel is an argument rather than an assumption.
    fn learned(kernel: DenoiseKernel, seed: u32, amp: f32, sr: f32, seconds: f32) -> Denoise {
        let mut dn = Denoise::new(sr);
        let learn = DenoiseParams {
            learn: 1.0,
            ..on(kernel)
        };
        let frames = (sr * seconds) as usize;
        let mut noise = Noise::new(seed, amp, sr);
        let mut buf = noise.frames(frames);
        dn.process(&mut buf, frames, 2, &learn);
        // The falling edge freezes the thresholds.
        let mut tiny = [0.0_f32; 8];
        dn.process(&mut tiny, 4, 2, &on(kernel));
        dn
    }

    /// The same learn-and-freeze pass as [`learned`], but taught with
    /// [`FullBandNoise`] — so all 32 thresholds come back armed, including
    /// the ones above 4 kHz that a [`Noise`]-taught profile leaves sitting
    /// at OFF because there was nothing up there to hear.
    fn learned_full_band(
        kernel: DenoiseKernel,
        seed: u32,
        amp: f32,
        sr: f32,
        seconds: f32,
    ) -> Denoise {
        let mut dn = Denoise::new(sr);
        let learn = DenoiseParams {
            learn: 1.0,
            ..on(kernel)
        };
        let frames = (sr * seconds) as usize;
        let mut noise = FullBandNoise::new(seed, amp);
        let mut buf = noise.frames(frames);
        dn.process(&mut buf, frames, 2, &learn);
        // The falling edge freezes the thresholds.
        let mut tiny = [0.0_f32; 8];
        dn.process(&mut tiny, 4, 2, &on(kernel));
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
                // THE BANK, named. Bit-exactness is the bank's own claim
                // and nothing else in this file can make it: an
                // overlap-add reconstructs to a few ULPs, not to the
                // sample, and it owes n samples of delay besides. That is
                // the whole reason the bank stays in the product.
                ..on(DenoiseKernel::Bank)
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
        let p = on(DenoiseKernel::Bank);
        // `latency` answers about the last block's settings, exactly as it
        // always has, so the selector has to have been seen once before it
        // can be asked — the same handshake the lookahead toggle needs.
        dn.process(&mut [0.0; 8], 4, 2, &p);
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
    /// against a signal well past full scale AND against non-finite input:
    /// the output stays finite and bounded, and the unit is still a
    /// denoiser afterwards.
    ///
    /// The second pass used to feed nothing but finite audio, which made
    /// this sweep structurally blind to the fault that actually shipped: a
    /// single infinity from an upstream plugin permanently converted the
    /// spectral kernels into wires, and the output stayed finite the whole
    /// time. So the poison goes in here, and the assertion afterwards is
    /// that the unit STILL CUTS — an assertion about output finiteness
    /// alone passed on the broken build and proved nothing.
    #[test]
    fn no_setting_the_panel_can_reach_makes_it_explode() {
        let frames = 4_800;
        let extremes = [0.0_f32, 1.0];
        for bits in 0..64_u32 {
            let d = |bit: u32| extremes[(bits >> bit & 1) as usize];
            // Every engine, against every other combination: a selector
            // that is only ever tested in its default position is a
            // selector with two untested positions.
            for kernel in DenoiseKernel::ALL {
            let p = DenoiseParams {
                learn: d(0),
                reduce: d(1),
                smooth: d(2),
                release: d(3),
                sc_hpf: d(4),
                lookahead: d(5),
                kernel: kernel.dial(),
                // The two late dials ride bits already in the sweep, so
                // both extremes of each are met without four times the
                // combinations: attack with reduce, treble with lookahead.
                attack: d(1),
                treble: d(5),
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
                // One of each kind of nonsense a plugin upstream can hand
                // over, spread so they land in different frames. The
                // finite-but-enormous case belongs to
                // `an_enormous_finite_sample_does_not_lock_the_noise_tracker`
                // and not here: the peak bound below is about the unit not
                // amplifying, and a wire is entitled to pass 1e20 along.
                buf[2 * 100] = f32::INFINITY;
                buf[2 * 900 + 1] = f32::NEG_INFINITY;
                buf[2 * 1_700] = f32::NAN;
                let went_in = buf.iter().filter(|s| !s.is_finite()).count();
                dn.process(&mut buf, frames, 2, &p);
                // The bound is "no MORE non-finite out than in", not "none
                // out", because one reachable setting is the provable wire
                // — bank, nothing learned, lookahead down — which returns
                // without reading the buffer, and a wire passing an
                // infinity through is a wire behaving correctly. Any unit
                // that actually processes must emit none, and the bank used
                // to emit 96,000 for the three that went in.
                let came_out = buf.iter().filter(|s| !s.is_finite()).count();
                assert!(
                    came_out <= went_in,
                    "dials {bits:06b} pass {pass} on {} turned {went_in} non-finite input \
                     samples into {came_out}",
                    kernel.label()
                );
                let peak = buf
                    .iter()
                    .filter(|s| s.is_finite())
                    .fold(0.0_f32, |m, x| m.max(x.abs()));
                assert!(peak < 8.0, "dials {bits:06b} pass {pass} reached {peak}");
            }
            // And the poison did not cost the unit its job. Teach it, feed
            // it a block carrying every kind of non-finite sample, then the
            // noise it was taught: it must still reduce. The old spectral
            // kernels passed the finiteness assertion above and failed THIS
            // one, permanently, which is the whole reason it is here.
            let mut dn = learned_full_band(kernel, 0x51CC, 0.05, SR, 1.0);
            let good = DenoiseParams {
                reduce: 1.0,
                ..on(kernel)
            };
            let mut bad = FullBandNoise::new(0x51CC, 0.05).frames(frames);
            bad[2 * 100] = f32::INFINITY;
            bad[2 * 900 + 1] = f32::NEG_INFINITY;
            bad[2 * 1_700] = f32::NAN;
            dn.process(&mut bad, frames, 2, &good);
            let (cut, still_bad) = cut_over_two_seconds(&mut dn, &good, 0x51CC, 0.05);
            // Checked explicitly, because an all-NaN buffer does NOT show
            // up as a small cut: `rms_db` takes `ms.max(1e-18)`, and
            // `f32::max` returns the other operand for a NaN, so a
            // destroyed unit reports −180 dB out and an enormous cut.
            assert_eq!(
                still_bad,
                0,
                "{}: {still_bad} non-finite output samples two seconds after a block \
                 carrying an infinity and a NaN",
                kernel.label()
            );
            assert!(
                cut > 8.0,
                "{}: after one block carrying an infinity and a NaN, the unit only \
                 removes {cut:.2} dB of the noise it learned",
                kernel.label()
            );
            }
        }
    }

    /// **Learn, then suppress**: a second of shaped noise teaches the
    /// thresholds; the same noise afterwards comes out at least 8 dB
    /// quieter broadband, at the shipped default settings.
    #[test]
    fn learned_noise_comes_back_at_least_8_db_quieter() {
        for kernel in DenoiseKernel::ALL {
            let mut dn = learned(kernel, 0xBAD5EED, 0.05, SR, 1.0);
            let frames = SR as usize;
            let mut noise = Noise::new(0xBAD5EED, 0.05, SR);
            let mut buf = noise.frames(frames);
            let in_db = rms_db(&buf, frames / 2);
            dn.process(&mut buf, frames, 2, &on(kernel));
            let out_db = rms_db(&buf, frames / 2);
            assert!(
                in_db - out_db >= 8.0,
                "{}: the learned noise only dropped {:.1} dB ({in_db:.1} -> {out_db:.1})",
                kernel.label(),
                in_db - out_db
            );
        }
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
        let amp = 10.0_f32.powf(-20.0 / 20.0);

        let clean = sine(1_000.0, amp, frames, sr);

        for kernel in DenoiseKernel::ALL {
            let mut dn = learned(kernel, 0xBAD5EED, 0.05, sr, 1.0);
            let mut buf = sine(1_000.0, amp, frames, sr);
            add(&mut buf, &Noise::new(0xBAD5EED, 0.05, sr).frames(frames));
            dn.process(&mut buf, frames, 2, &on(kernel));
            // Aligned to the kernel's own latency, so the correlation
            // windows describe the same half-second of programme whichever
            // engine is running.
            let lat = dn.latency() as usize;
            let (from, len) = (frames / 2, sr as usize / 2);
            let clean_db = tone_db(&clean, 1_000.0, sr, from - lat, len);
            let got_db = tone_db(&buf, 1_000.0, sr, from, len);

            // 2.5 dB and not 1.5 since the opening knee (2026-09-16): a
            // tone this far over its band's room — about 24 dB on the Live
            // kernel's wider bands — opens the band most but not all of the
            // way, and costs up to 2 dB by design. The reference render the
            // knee was tuned against takes 4–5 dB off the same signal.
            assert!(
                (got_db - clean_db).abs() <= 2.5,
                "{}: the tone moved from {clean_db:.2} to {got_db:.2} dB under suppression",
                kernel.label()
            );
            // And the suppression was real while the tone played: bands
            // far from 1 kHz were still cutting. Without this the test
            // would pass on a wire.
            let depth = dn.depth_db();
            let far = depth[..8]
                .iter()
                .chain(depth[26..].iter())
                .cloned()
                .fold(0.0_f32, f32::max);
            assert!(
                far > 3.0,
                "{}: no band far from the tone was cutting ({far:.1} dB) — the test proves nothing",
                kernel.label()
            );
        }
    }

    /// **The spectral kernel is not a flat attenuator wearing a denoiser's
    /// name.** The two acceptance tests above feed it nothing but noise,
    /// where the honest answer and the answer a plain volume knob would
    /// give are the SAME number — so on their own they would pass on a
    /// fader. This is the test that separates them: a −20 dBFS tone over
    /// the learned noise, and the signal-to-noise ratio has to improve by
    /// the dial. The tone stays put, the floor around it drops by what was
    /// asked for, and the difference between those two facts is the whole
    /// product.
    #[test]
    fn the_spectral_kernel_improves_the_signal_to_noise_and_not_just_the_level() {
        let sr = SR;
        let frames = 2 * sr as usize;
        let amp = 10.0_f32.powf(-20.0 / 20.0);
        for kernel in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
            for reduce in [0.5_f32, 1.0] {
                let dial = reduce * DENOISE_REDUCE_DB;
                let mut dn = learned_full_band(kernel, 0x5A11, 0.02, sr, 1.0);
                let p = DenoiseParams {
                    reduce,
                    ..on(kernel)
                };
                let mut dry = sine(1_000.0, amp, frames, sr);
                add(&mut dry, &FullBandNoise::new(0x5A11, 0.02).frames(frames));
                let mut wet = dry.clone();
                dn.process(&mut wet, frames, 2, &p);
                let lat = dn.latency() as usize;

                let (from, len) = (frames / 2, sr as usize / 2);
                let tone_in = tone_db(&dry, 1_000.0, sr, from - lat, len);
                let tone_out = tone_db(&wet, 1_000.0, sr, from, len);
                // The floor, measured well away from the tone so none of
                // its skirt is in the slice.
                let psd_dry = analysis::power_spectrum(&dry, from - lat, len);
                let psd_wet = analysis::power_spectrum(&wet, from, len);
                let floor_cut = analysis::band_power_db(&psd_dry, sr, 4_000.0, 16_000.0)
                    - analysis::band_power_db(&psd_wet, sr, 4_000.0, 16_000.0);

                assert!(
                    (tone_out - tone_in).abs() <= 1.5,
                    "{} at {dial:.0} dB: the tone moved {:.2} dB ({tone_in:.2} -> {tone_out:.2})",
                    kernel.label(),
                    tone_out - tone_in
                );
                assert!(
                    (floor_cut - dial).abs() <= 1.5,
                    "{} at {dial:.0} dB: the floor around the tone dropped {floor_cut:.2} dB, \
                     not the dial",
                    kernel.label()
                );
                // Said once more as the thing a listener hears: the tone
                // sits that much further above the floor than it did.
                let gained = (tone_out - tone_in) + floor_cut;
                assert!(
                    gained >= dial - 1.5,
                    "{} at {dial:.0} dB: the signal-to-noise only improved {gained:.2} dB",
                    kernel.label()
                );
            }
        }
    }

    /// **A real gain on a bin leaves phase alone; a moving bell cannot.**
    /// The owner's first symptom, measured rather than argued.
    ///
    /// The same experiment on all three engines: learned noise with a quiet
    /// 1 kHz probe under it, so the band around the probe is cutting hard,
    /// and the probe's PHASE at the output compared against the input
    /// delayed by the kernel's own reported latency. Multiplying a bin by a
    /// real non-negative scalar multiplies its real and imaginary parts by
    /// the same number, so the argument is an identity — the spectral
    /// kernels come back inside a degree. A minimum-phase peaking cut has
    /// its phase tied to its magnitude by the Hilbert relation and cannot:
    /// the bank rotates the same probe by tens of degrees for the same
    /// depth of cut.
    ///
    /// The bank's number is asserted too, as a FLOOR rather than a ceiling.
    /// A phase test that cannot see phase would pass on all three engines
    /// and prove nothing about any of them.
    #[test]
    fn a_real_bin_gain_leaves_phase_alone_and_a_moving_bell_does_not() {
        let sr = SR;
        let frames = 3 * sr as usize;
        let probe = 1_000.0_f32;
        // Quiet enough to be a probe rather than a signal: the band must
        // still be cutting while it is being measured.
        let amp = 10.0_f32.powf(-45.0 / 20.0);

        let mut worst = [0.0_f32; 3];
        let mut cut = [0.0_f32; 3];
        for (i, kernel) in DenoiseKernel::ALL.iter().enumerate() {
            let mut dn = learned_full_band(*kernel, 0xB0A5, 0.05, sr, 1.0);
            let p = DenoiseParams {
                reduce: 1.0,
                ..on(*kernel)
            };
            let mut dry = sine(probe, amp, frames, sr);
            add(&mut dry, &FullBandNoise::new(0xB0A5, 0.05).frames(frames));
            let mut wet = dry.clone();
            dn.process(&mut wet, frames, 2, &p);
            let lat = dn.latency() as usize;

            // Several windows across the settled second half, because the
            // claim is that phase does not MOVE — one window could catch an
            // excursion at either end of its travel and call it zero.
            let len = sr as usize / 4;
            for w in 0..6 {
                let from = frames / 2 + w * len / 2;
                if from + len > frames {
                    break;
                }
                let a = tone_phase(&dry, probe, sr, from - lat, len);
                let b = tone_phase(&wet, probe, sr, from, len);
                let mut d = (b - a).to_degrees();
                while d > 180.0 {
                    d -= 360.0;
                }
                while d < -180.0 {
                    d += 360.0;
                }
                worst[i] = worst[i].max(d.abs());
                cut[i] = cut[i].max(
                    tone_db(&dry, probe, sr, from - lat, len) - tone_db(&wet, probe, sr, from, len),
                );
            }
        }

        let table = DenoiseKernel::ALL
            .iter()
            .enumerate()
            .map(|(i, k)| {
                format!(
                    "  {:<16} {:>6.2} deg at {:>5.1} dB of cut",
                    k.label(),
                    worst[i],
                    cut[i]
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        println!("phase excursion on a 1 kHz probe:\n{table}");

        // All three are cutting hard, so all three are being asked the
        // same question.
        for (i, k) in DenoiseKernel::ALL.iter().enumerate() {
            assert!(
                cut[i] > 8.0,
                "{} only cut {:.1} dB at the probe, so its phase figure describes \
                 nothing:\n{table}",
                k.label(),
                cut[i]
            );
        }
        for w in worst.iter().skip(1) {
            assert!(*w < 1.0, "a real bin gain moved the probe's phase:\n{table}");
        }
        assert!(
            worst[0] > 10.0,
            "the bank did not move phase either, so this test cannot see phase and \
             proves nothing about the kernels that pass it:\n{table}"
        );
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
            // The cross-band slope rule is the bank's own machinery; the
            // spectral kernels smooth an ESTIMATE across bands instead and
            // have no per-band bell to rein in.
            ..on(DenoiseKernel::Bank)
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
        // Measured in a RATE-INDEPENDENT band. The top band is a shelf that
        // reaches Nyquist, so at 96k it honestly removes hiss from 19 kHz
        // to 48 kHz that does not exist at 44.1k — a broadband RMS would
        // read that extra octave as "disagreement" when it is exactly the
        // crisper coverage this pass added. So both signals are folded
        // through the same 10 kHz one-pole before the RMS: the shared, un-warped audible
        // suppression is what must match across rates, and it does.
        let lp15 = |buf: &[f32], from: usize, sr: f32| -> f32 {
            let a = 1.0 - (-std::f32::consts::TAU * 10_000.0 / sr).exp();
            let mut z = 0.0f32;
            let mut acc = 0.0f64;
            let mut n = 0u64;
            for (i, s) in buf.chunks_exact(2).enumerate() {
                z += a * (s[0] - z);
                if i >= from {
                    acc += f64::from(z) * f64::from(z);
                    n += 1;
                }
            }
            10.0 * ((acc / n.max(1) as f64).max(1e-30)).log10() as f32
        };
        let drop_at = |kernel: DenoiseKernel, sr: f32| {
            let mut dn = learned(kernel, 0xABCD, 0.05, sr, 1.0);
            let frames = sr as usize;
            let mut noise = Noise::new(0xABCD, 0.05, sr);
            let mut buf = noise.frames(frames);
            let in_db = lp15(&buf, frames / 2, sr);
            dn.process(&mut buf, frames, 2, &on(kernel));
            in_db - lp15(&buf, frames / 2, sr)
        };
        // Every engine. The spectral kernels earn this the hard way: their
        // frame LENGTH changes with the rate (1024 points at 48 kHz, 2048
        // at 96) precisely so the frame DURATION does not, and every time
        // constant in the detector is derived from that duration rather
        // than stored as a per-frame coefficient. If any of that were
        // wrong, 96 kHz would answer differently here.
        for kernel in DenoiseKernel::ALL {
            let (a, b, c) = (
                drop_at(kernel, 44_100.0),
                drop_at(kernel, 48_000.0),
                drop_at(kernel, 96_000.0),
            );
            for (x, y) in [(a, b), (b, c), (a, c)] {
                assert!(
                    (x - y).abs() < 1.5,
                    "{}: suppression across rates disagrees: {a:.2} / {b:.2} / {c:.2} dB",
                    kernel.label()
                );
            }
        }
    }

    /// **Thresholds are state**: they round-trip through
    /// `thresholds`/`set_thresholds` exactly, garbage in the array lands
    /// on OFF, and a fresh unit given a learned unit's numbers suppresses
    /// within half a dB of the original.
    #[test]
    fn thresholds_round_trip_as_the_state_they_are() {
        let dn = learned(DenoiseKernel::Bank, 0x1234, 0.05, SR, 1.0);
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
            dn.process(&mut buf, frames, 2, &on(DenoiseKernel::Bank));
            in_db - rms_db(&buf, frames / 2)
        };
        let mut orig = learned(DenoiseKernel::Bank, 0x1234, 0.05, SR, 1.0);
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
        let mut dn = learned(DenoiseKernel::Bank, 0x77, 0.05, SR, 1.0);
        let t = dn.thresholds();
        let frames = SR as usize / 2;
        let mut buf = Noise::new(0x77, 0.05, SR).frames(frames);
        dn.process(&mut buf, frames, 2, &on(DenoiseKernel::Bank));
        assert!(dn.depth_db().iter().any(|&d| d > 1.0), "nothing was cutting before the clear");

        dn.clear();
        assert!(dn.depth_db().iter().all(|&d| d == 0.0), "a depth survived the clear");
        assert_eq!(dn.thresholds(), t, "the clear ate the learned profile");

        // The delay line is empty: silence in, silence out, immediately.
        let mut silent = vec![0.0_f32; 512 * 2];
        dn.process(&mut silent, 512, 2, &on(DenoiseKernel::Bank));
        assert!(silent.iter().all(|&s| s == 0.0), "the lookahead line replayed old audio");
    }

    /// **A cleared spectral denoiser comes back CLOSED on the noise it was
    /// cutting.** A bay rebuilt by an engine restart or a session switch is
    /// created, cleared and handed its print - and the band gains used to
    /// start at unity and glide down at the RELEASE rate, so the room reopened
    /// onto the noise at full level for a quarter of a second (two at the
    /// slowest release). The first frame's target was already right; only the
    /// follower lagged. Measured in 10 ms windows from the first frame out,
    /// against the level the same unit settles to.
    #[test]
    fn a_cleared_denoiser_is_back_on_the_noise_at_once() {
        // The owner's bay as saved: Bank, lookahead on, REDUCE all the way.
        let owners = DenoiseParams {
            lookahead: 1.0,
            reduce: 1.0,
            ..on(DenoiseKernel::Bank)
        };
        let cases = DenoiseKernel::ALL
            .into_iter()
            .flat_map(|kernel| {
                [DenoiseParams::default().release, 1.0]
                    .map(|release| (kernel, DenoiseParams { release, ..on(kernel) }))
            })
            .chain([(DenoiseKernel::Bank, owners)]);
        for (kernel, p) in cases {
            let release = p.release;
            {
                let mut dn = learned(kernel, 0x5a, 0.05, SR, 1.0);
                let mut noise = Noise::new(0x5b, 0.05, SR);
                let settle = SR as usize;
                let mut buf = noise.frames(settle);
                for chunk in buf.chunks_mut(512) {
                    dn.process(chunk, chunk.len() / 2, 2, &p);
                }
                let settled = rms_db(&buf, settle - SR as usize / 5);
                let raw = rms_db(&noise.frames(SR as usize / 5), 0);
                assert!(raw - settled > 6.0, "{}: nothing was cutting ({raw:.1} vs {settled:.1})", kernel.label());

                dn.clear();
                let lat = dn.latency() as usize;
                let win = SR as usize / 100;
                let span = lat + 20 * win;
                let mut after = noise.frames(span);
                for chunk in after.chunks_mut(512) {
                    dn.process(chunk, chunk.len() / 2, 2, &p);
                }
                // From the first frame the new instance writes out.
                let first = lat + lat / 4;
                let mut worst = f32::MIN;
                let mut k = 0;
                while first + (k + 1) * win <= span {
                    let at = &after[(first + k * win) * 2..(first + (k + 1) * win) * 2];
                    worst = worst.max(rms_db(at, 0) - settled);
                    k += 1;
                }
                assert!(
                    worst < 3.0,
                    "{} (release {release}): the cleared unit let the noise through {worst:.1} dB over its settled level",
                    kernel.label()
                );
            }
        }
    }

    /// Mono is left alone rather than half-processed, and a third channel
    /// passes through a working stereo pair untouched.
    #[test]
    fn a_mono_buffer_and_a_third_channel_are_left_alone() {
        for kernel in DenoiseKernel::ALL {
            let mut dn = learned(kernel, 0x9, 0.05, SR, 1.0);
            let mut mono: Vec<f32> = (0..256).map(|i| (i as f32 * 0.02).sin()).collect();
            let before = mono.clone();
            dn.process(&mut mono, 256, 1, &on(kernel));
            assert_eq!(mono, before, "{}: a mono buffer was written to", kernel.label());

            let frames = 4_800;
            let mut dn = learned(kernel, 0x9, 0.05, SR, 1.0);
            let mut buf = vec![0.0_f32; frames * 3];
            let mut noise = Noise::new(0x9, 0.05, SR);
            for (i, f) in buf.chunks_exact_mut(3).enumerate() {
                let v = noise.next();
                f[0] = v;
                f[1] = v;
                f[2] = (i as f32 * 0.01).sin();
            }
            let third: Vec<f32> = buf.chunks_exact(3).map(|f| f[2]).collect();
            dn.process(&mut buf, frames, 3, &on(kernel));
            let third_after: Vec<f32> = buf.chunks_exact(3).map(|f| f[2]).collect();
            assert_eq!(
                third_after,
                third,
                "{}: the third channel was touched",
                kernel.label()
            );
        }
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
            ..on(DenoiseKernel::Bank)
        };
        let frames = SR as usize / 2;
        let mut buf = Noise::new(0xF00D, 0.2, SR).frames(frames);
        let before = buf.clone();
        dn.process(&mut buf, frames, 2, &p);
        assert_eq!(buf, before, "learning coloured the signal it was measuring");

        // The spectral kernels cannot be bit-exact — they owe a frame of
        // delay — but the claim is the same one and has to be checked the
        // same way: while learning, the output is the INPUT, moved.
        for kernel in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
            let mut dn = Denoise::new(SR);
            let p = DenoiseParams {
                learn: 1.0,
                ..on(kernel)
            };
            let mut buf = Noise::new(0xF00D, 0.2, SR).frames(frames);
            let before = buf.clone();
            dn.process(&mut buf, frames, 2, &p);
            let lat = dn.latency() as usize;
            let mut worst = 0.0_f32;
            for f in lat..frames {
                worst = worst.max((buf[f * 2] - before[(f - lat) * 2]).abs());
            }
            assert!(
                worst < 1.0e-5,
                "{} coloured the signal it was measuring by {worst:.2e}",
                kernel.label()
            );
        }
    }

    // ── the spectral scaffolding ────────────────────────────────────────────
    //
    // Three tests, and they come BEFORE any suppression exists. Every number
    // the gain law is ever judged by is measured through this machinery, so
    // if the machinery is wrong every one of those numbers is a lie told
    // confidently. Prove the frame arithmetic first.

    /// **Σw² = 2.000000, and the whole synthesis rests on it.** Root-Hann in
    /// its periodic form, `sin(π(i+½)/n)`, squares to a periodic Hann; a
    /// periodic Hann summed at hop n/4 is exactly 2, because the four ½
    /// terms give 2 and the four cosines are a quarter-turn apart and cancel
    /// in pairs. That single arithmetic fact is why synthesis normalisation
    /// is one multiply by 0.5 — no accumulated-window array, no per-sample
    /// divide, no edge guard — so it is pinned to a part in a million rather
    /// than asserted in a comment.
    #[test]
    fn the_root_hann_windows_square_and_sum_to_exactly_two() {
        for n in [512_usize, 1_024, 2_048, 4_096] {
            let w: Vec<f64> = (0..n)
                .map(|i| (std::f64::consts::PI * (i as f64 + 0.5) / n as f64).sin())
                .collect();
            let hop = n / 4;
            // Every phase within the hop, not just one: the claim is that
            // the sum is flat, and a single sample cannot see flatness.
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for off in 0..hop {
                let sum: f64 = (0..4).map(|m| w[off + m * hop].powi(2)).sum();
                lo = lo.min(sum);
                hi = hi.max(sum);
            }
            assert!(
                (lo - 2.0).abs() < 1.0e-6 && (hi - 2.0).abs() < 1.0e-6,
                "n = {n}: Σw² ranges over {lo:.9}..{hi:.9}, not 2.000000"
            );
        }
    }

    /// **The scaffolding is a pure delay.** With nothing learned the
    /// spectral kernels apply a gain of exactly one to every bin, so the
    /// windows, the staging, the transform pair and the overlap-add have to
    /// hand back the input unchanged apart from the delay they owe — and
    /// unchanged means to within f32 rounding through two 1024-point
    /// transforms, not to within "sounds fine".
    ///
    /// Both channels, and they carry DIFFERENT signals on purpose: the pair
    /// is packed as one complex sequence `l + j·r` and transformed once, so
    /// a mistake in the packing shows up as one channel leaking into the
    /// other and identical channels would hide it completely.
    #[test]
    fn the_spectral_scaffolding_is_a_pure_delay() {
        for kernel in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
            for sr in [44_100.0_f32, 48_000.0, 96_000.0] {
                let frames = 3 * sr as usize / 4;
                let mut dn = Denoise::new(sr);
                let p = on(kernel);
                let mut buf = sine(440.0, 0.4, frames, sr);
                // The right channel gets its own, unrelated signal.
                let other = Noise::new(0x5EED, 0.3, sr).frames(frames);
                for (f, o) in buf.chunks_exact_mut(2).zip(other.chunks_exact(2)) {
                    f[1] = o[0];
                }
                let before = buf.clone();
                dn.process(&mut buf, frames, 2, &p);
                let lat = dn.latency() as usize;

                let mut worst = 0.0_f32;
                for f in lat..frames {
                    for ch in 0..2 {
                        worst = worst.max((buf[f * 2 + ch] - before[(f - lat) * 2 + ch]).abs());
                    }
                }
                assert!(
                    worst < 1.0e-5,
                    "{} at {sr} Hz is not a pure delay: worst sample error {worst:.3e}",
                    kernel.label()
                );
                // And the delay is real, not an accident of a quiet
                // fixture: the first `lat` samples are the primed silence.
                // Not bit-zero — the frame the impulse landed in is
                // reconstructed through two f32 transforms and its rounding
                // is spread over the whole frame, so the honest bound is
                // the same float-error one the delay itself is held to,
                // 125 dB under the signal.
                let head = buf[..lat * 2].iter().fold(0.0_f32, |m, s| m.max(s.abs()));
                assert!(
                    head < 1.0e-5,
                    "{} leaked {head:.3e} into its priming",
                    kernel.label()
                );
            }
        }
    }

    /// **The reported latency is the frame, and an impulse proves it.**
    ///
    /// The overlap-add's own algorithmic delay is n − R: after m+1 frames
    /// exactly (m+1)R samples are finished, the first standing for input
    /// time R − n. A competing design reports that number and it is wrong,
    /// because output leaves in hops of R while a host block boundary lands
    /// wherever it likes: with T = n + mR + r samples handed in, n − R + r
    /// remain unconsumed and r runs to R − 1, so the worst case over an
    /// arbitrary block size is n − 1 and the block-agnostic bound is n. The
    /// FIFO's one-hop priming buys exactly that, and the argument is settled
    /// here by measurement — an impulse in, and the sample it comes back on
    /// — at seven block sizes including ones that share no factor with the
    /// hop.
    #[test]
    fn the_spectral_latency_is_the_frame_and_an_impulse_proves_it() {
        for kernel in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
            for block in [1_usize, 7, 64, 100, 256, 512, 1_000] {
                let frames = 16_384;
                let mut dn = Denoise::new(SR);
                let p = on(kernel);
                let mut buf = vec![0.0_f32; frames * 2];
                buf[0] = 1.0;
                buf[1] = 1.0;
                let mut done = 0;
                while done < frames {
                    let n = block.min(frames - done);
                    dn.process(&mut buf[done * 2..(done + n) * 2], n, 2, &p);
                    done += n;
                }
                let want = dn.latency() as usize;
                assert_eq!(
                    want,
                    dn.sp.n,
                    "{} reports {want} rather than its frame",
                    kernel.label()
                );
                // Where the energy actually landed: the peak of the output.
                let at = (0..frames)
                    .max_by(|&a, &b| buf[a * 2].abs().total_cmp(&buf[b * 2].abs()))
                    .expect("a non-empty buffer");
                assert_eq!(
                    at,
                    want,
                    "{} in blocks of {block}: the impulse came back at {at}, \
                     not at the reported {want}",
                    kernel.label()
                );
                // And nothing audible came back EARLIER than the report —
                // a latency that is sometimes short is worse than one that
                // is long, because a host cannot compensate for it. The
                // bound is f32 transform noise, 125 dB under the impulse,
                // not bit-zero: two transforms do not round to nothing.
                let early = buf[..want * 2].iter().fold(0.0_f32, |m, s| m.max(s.abs()));
                assert!(
                    early < 1.0e-5,
                    "{} in blocks of {block} answered {early:.3e} before its \
                     reported latency was up",
                    kernel.label()
                );
            }
        }
    }

    /// **Changing kernel changes the reported latency, and the report is
    /// live.** The selector is the third thing on this unit that moves
    /// latency (the lookahead toggle and the sample rate are the others),
    /// and a host that is not told cannot compensate. `latency` answers
    /// about the last processed block, so the handshake is: set the dial,
    /// process, read.
    #[test]
    fn the_selector_moves_the_reported_latency() {
        let mut dn = Denoise::new(SR);
        let mut buf = [0.0_f32; 8];
        for (kernel, want) in [
            (DenoiseKernel::Bank, (SR * DENOISE_LOOKAHEAD_MS / 1_000.0).round() as u32),
            (DenoiseKernel::SpectralStudio, 1_024),
            (DenoiseKernel::SpectralLive, 512),
        ] {
            dn.process(&mut buf, 4, 2, &on(kernel));
            assert_eq!(dn.latency(), want, "{} reported the wrong latency", kernel.label());
        }
        // At 96 kHz the frame is twice as long, so the FRAME TIME — which
        // is what sets the frequency resolution — is the same 21.3 ms.
        let mut dn = Denoise::new(96_000.0);
        dn.process(&mut buf, 4, 2, &on(DenoiseKernel::SpectralStudio));
        assert_eq!(dn.latency(), 2_048, "the frame did not follow the rate");
    }

    /// **The band weights are a partition, not an overlap.** Every bin's
    /// weights sum to exactly one, every band owns at least two bins, the
    /// centres climb, and band 31's reach runs to Nyquist.
    ///
    /// This is the arithmetic the reduction dial's honesty is built on. If
    /// the weights sum to one at every bin then a bank of EQUAL band gains
    /// reconstructs to exactly that gain at every bin — flat, and equal to
    /// what was asked for. The bank of bells could never do this: its
    /// sections are peaking cuts whose skirts overlap, so its composite is
    /// a sum that runs away with the dial. Here the composite is an
    /// identity, and this test is where that is checked rather than
    /// assumed.
    #[test]
    fn the_band_weights_partition_every_bin_exactly() {
        for sr in [44_100.0_f32, 48_000.0, 96_000.0, 192_000.0] {
            for kernel in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
                let mut dn = Denoise::new(sr);
                dn.process(&mut [0.0; 8], 4, 2, &on(kernel));
                let sp = &dn.sp;
                let bins = sp.n / 2;

                let mut worst = 0.0_f32;
                let mut seen = [false; DENOISE_BANDS];
                for k in 0..=bins {
                    let (b, frac) = (sp.map_lo[k] as usize, sp.map_frac[k]);
                    let sum = (1.0 - frac) + if b + 1 < DENOISE_BANDS { frac } else { 0.0 };
                    worst = worst.max((sum - 1.0).abs());
                    seen[b] = true;
                    assert!(
                        (0.0..=1.0).contains(&frac),
                        "{} at {sr} Hz: bin {k} has weight {frac}",
                        kernel.label()
                    );
                }
                assert!(
                    worst < 1.0e-6,
                    "{} at {sr} Hz: a bin's weights miss unity by {worst:.2e}",
                    kernel.label()
                );
                // The last band is the only one a bin may never START in —
                // it has no successor to ramp toward — so every OTHER band
                // must be some bin's lower neighbour, or a band would be
                // reading and writing nothing at all.
                for (b, s) in seen.iter().enumerate() {
                    assert!(*s, "{} at {sr} Hz: band {b} owns no bin", kernel.label());
                }
                // Two bins minimum, which is the transform's own limit: a
                // one-bin band is a free per-bin gain wearing a band's
                // name, and this file spends a paragraph on why that is
                // the thing being avoided.
                for b in 0..DENOISE_BANDS {
                    assert!(
                        sp.wsum[b] >= 1.0,
                        "{} at {sr} Hz: band {b} is only {:.2} bins wide",
                        kernel.label(),
                        sp.wsum[b]
                    );
                }
                // And band 31 reaches the top: the last bin is entirely
                // its, so nothing above the grid is left uncovered — the
                // mistake a beta ear caught in the bank inside an hour.
                assert_eq!(
                    sp.map_lo[bins] as usize,
                    DENOISE_BANDS - 1,
                    "{} at {sr} Hz: Nyquist belongs to band {}",
                    kernel.label(),
                    sp.map_lo[bins]
                );
                assert_eq!(sp.map_frac[bins], 0.0);
            }
        }
    }

    /// **A learn pass is a measurement of the noise, in decibels that mean
    /// the same thing everywhere.** White noise at a known RMS is played
    /// in, and every one of the 32 bands comes back reading that RMS —
    /// whatever the frame length and whatever the sample rate, because the
    /// level is normalised by n on the way out (see
    /// `Spectral::band_level`). That normalisation is what lets one set of
    /// `t0..t31` symbols mean something at 1024 points and at 512, and at
    /// 48 kHz and at 96.
    ///
    /// The tolerance is 3.5 dB and it is not slack — it is a bias with a
    /// closed form. A geometric mean of a band energy reads
    /// `ψ(W) − ln(W)` nepers under the arithmetic one, where W is how many
    /// bins the band is worth, so a wide band is essentially unbiased
    /// (W = 30 gives −0.07 dB) and the narrowest band on the 2-bin floor is
    /// not (W ≈ 1.5 gives −1.6 dB, and the DC bin's halved degrees of
    /// freedom take band 0 to a measured −2.9). It is the price of the
    /// geometric mean's immunity to one loud frame, it is confined to the
    /// bottom band or two, and it errs LOW — a floor under the truth
    /// suppresses less rather than eating signal — which is why the second
    /// assertion here is one-sided and much tighter than the first.
    #[test]
    fn a_learned_profile_reads_the_noise_it_was_taught_at_every_frame_length() {
        for sr in [44_100.0_f32, 48_000.0, 96_000.0] {
            for kernel in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
                for amp in [0.05_f32, 0.5] {
                    let dn = learned_full_band(kernel, 0xF10A, amp, sr, 1.0);
                    // Uniform in ±amp, so RMS is amp/√3.
                    let want = 20.0 * (amp / 3.0_f32.sqrt()).log10();
                    let thr = dn.thresholds();
                    let db = |t: f32| DENOISE_THR_DB.0 + (DENOISE_THR_DB.1 - DENOISE_THR_DB.0) * t;
                    let read = |b: usize| db(thr[b]);
                    let worst = (0..DENOISE_BANDS)
                        .map(|b| (read(b) - want).abs())
                        .fold(0.0_f32, f32::max);
                    let over = (0..DENOISE_BANDS)
                        .map(|b| read(b) - want)
                        .fold(f32::NEG_INFINITY, f32::max);
                    let table = || {
                        (0..DENOISE_BANDS)
                            .map(|b| format!("{:.1}", read(b)))
                            .collect::<Vec<_>>()
                            .join(" ")
                    };
                    assert!(
                        worst < 3.5,
                        "{} at {sr} Hz, amp {amp}: a band read {worst:.1} dB off the \
                         {want:.1} dBFS it was taught\n  {}",
                        kernel.label(),
                        table()
                    );
                    // And the miss is one-sided: the geometric mean reads
                    // LOW, never high. A floor above the real noise is a
                    // floor that starts eating signal.
                    assert!(
                        over < 0.5,
                        "{} at {sr} Hz, amp {amp}: a band read {over:.1} dB ABOVE the \
                         {want:.1} dBFS it was taught\n  {}",
                        kernel.label(),
                        table()
                    );
                }
            }
        }
    }

    /// **A misclick does not wipe a good profile.** The guard is 200 ms of
    /// pass, measured as pass length rather than as surviving frames, and
    /// under it the previous profile is kept untouched.
    #[test]
    fn a_learn_pass_too_short_to_mean_anything_keeps_the_old_profile() {
        for kernel in DenoiseKernel::ALL {
            let mut dn = learned_full_band(kernel, 0x9E1, 0.05, SR, 1.0);
            let good = dn.thresholds();
            assert!(good.iter().any(|&t| t > 0.0), "{} learned nothing", kernel.label());

            // 100 ms of something much louder, then let go.
            let frames = SR as usize / 10;
            let mut buf = FullBandNoise::new(0x9E1, 0.5).frames(frames);
            let learn = DenoiseParams {
                learn: 1.0,
                ..on(kernel)
            };
            dn.process(&mut buf, frames, 2, &learn);
            dn.process(&mut [0.0; 8], 4, 2, &on(kernel));
            assert_eq!(
                dn.thresholds(),
                good,
                "{}: a 100 ms misclick moved the profile",
                kernel.label()
            );
        }
    }

    /// **The bank arrives, and stays.** Twelve seconds of stationary noise
    /// at the top of the dial: no band's second-by-second mean depth may
    /// move more than 1.5 dB from one second to the next.
    ///
    /// It used to move 3.34 dB, in the LOW bands only, and it did not
    /// shrink when the run was made longer — which is what "the bank never
    /// settles" meant. A bell whose gain never stops moving is a bell whose
    /// PHASE never stops moving, because a minimum-phase section ties the
    /// two together, so this was a candidate for the reported artefacts in
    /// its own right.
    ///
    /// The cause was statistics, not dynamics. A band's energy estimate is
    /// a chi-square with about `2·B·τ` degrees of freedom, so at a fixed
    /// τ the 50 Hz-wide bottom band carried several decibels of scatter
    /// where the 12.9 kHz-wide top band carried a quarter of one; the
    /// expander's slope multiplied that scatter by ratio − 1, and the
    /// asymmetric depth smoother — fast toward less cut, slow toward more —
    /// RECTIFIED it into a slow random walk. Giving each band a level
    /// window inversely proportional to its bandwidth
    /// ([`LEVEL_BW_MS_HZ`]) equalises the confidence and the walk stops.
    ///
    /// Measured per band rather than in aggregate, and over twelve windows
    /// rather than two, because a walk that happens to be near its old
    /// value at both ends of a short run reads as zero.
    #[test]
    fn the_bank_settles_and_stops_travelling() {
        let p = DenoiseParams {
            reduce: 1.0,
            ..on(DenoiseKernel::Bank)
        };
        let mut dn = learned_full_band(DenoiseKernel::Bank, 0xD1A1, 0.05, SR, 1.0);
        let frames = 12 * SR as usize;
        let mut buf = FullBandNoise::new(0xD1A1, 0.05).frames(frames);
        let traj = analysis::gain_trajectory(&mut dn, &mut buf, frames, 512, &p);
        let per_sec = traj.len() / 12;
        let mean = |seg: &[[f32; DENOISE_BANDS]], k: usize| {
            seg.iter().map(|s| s[k]).sum::<f32>() / seg.len() as f32
        };

        // The first two seconds are the bank arriving, which is allowed.
        let (mut worst, mut at) = (0.0_f32, (0, 0));
        for w in 3..12 {
            let seg = &traj[w * per_sec..(w + 1) * per_sec];
            let prev = &traj[(w - 1) * per_sec..w * per_sec];
            for k in 0..DENOISE_BANDS {
                let d = (mean(seg, k) - mean(prev, k)).abs();
                if d > worst {
                    worst = d;
                    at = (w, k);
                }
            }
        }
        assert!(
            worst < 1.5,
            "band {} moved {worst:.2} dB between second {} and the one before \
             it — the bank is still travelling (it was 3.34 dB before the \
             per-band level window)",
            at.1,
            at.0
        );
    }

    /// **A band that stays closed does not jump when its neighbour opens.**
    ///
    /// The cross-band slope rule is masking-exempt: a band that is open
    /// because signal lives in it does not export its zero sideways. The
    /// flags are booleans, so the moment a neighbour stops being exempt the
    /// constraint arrives all at once — and it used to arrive at the
    /// SMOOTHED depth, as a hard `min` on the number the bell was about to
    /// be designed from. Measured on a tone gated on and off over learned
    /// noise, a band that never opened at all had its applied depth move
    /// 3.41 dB in one 0.667 ms tick. A minimum-phase bell whose
    /// coefficients jump is precisely the artefact this unit was reported
    /// for.
    ///
    /// Constraining the TARGET instead puts the transition through the same
    /// 2 ms opening envelope every other gain move already uses, and the
    /// same measurement reads 0.80 dB — no longer a discontinuity, just a
    /// gain move like any other. The bound here is 1.5 dB: comfortably
    /// under the old behaviour, comfortably over the new one, and set where
    /// it is because the honest claim is "no faster than an ordinary open",
    /// not "arbitrarily slow".
    ///
    /// # What that repair did NOT move, said plainly
    ///
    /// The defect report `907fd2c` put in the tree names a different
    /// number: "5.50 dB of single-block movement at the default cap and
    /// 10.40 dB at REDUCE 1.0", over a 64-frame BLOCK and over ANY band.
    /// The commit that constrained the target claimed the jump as repaired.
    /// Re-measured on the same fixture, old against new, that probe reads
    /// 5.01 → 4.98 dB at the default cap and 9.52 → 8.95 dB at the top of
    /// the dial. It moved six per cent. **It was never the constraint.**
    ///
    /// What the constraint was doing is the number this test's first
    /// assertion holds: a band that stayed CLOSED throughout moved 3.41 dB
    /// in a tick and now moves 0.75, and over a 64-frame block 3.36 and now
    /// 1.49. The residual ~9 dB is an ordinary gate OPEN — a band with a
    /// note arriving in it, going from full cut to none through the 2 ms
    /// [`DENOISE_OPEN_MS`] envelope, which over the two control ticks in a
    /// 64-frame block is arithmetically allowed to cover 48.7 % of the
    /// depth, or 11.7 dB of 24. The deleted `noise_straddling_a_threshold`
    /// test said the same thing in its own words: "a legitimate fast open
    /// moves ~half the depth in one 1.3 ms block."
    ///
    /// So the second assertion pins THAT, two ways: against the envelope's
    /// own arithmetic ceiling (structural — nothing may move faster than
    /// the smoother it goes through, which is exactly what the constraint
    /// used to do), and against 10.0 dB (the recorded number, 8.95, with
    /// room). If a 9 dB coefficient step per 1.33 ms is judged too fast for
    /// a minimum-phase bell, the lever is `DENOISE_OPEN_MS` and both bounds
    /// follow it down on their own.
    #[test]
    fn a_closed_band_does_not_jump_when_its_neighbour_stops_being_exempt() {
        let p = DenoiseParams {
            reduce: 1.0,
            ..on(DenoiseKernel::Bank)
        };
        let mut dn = learned_full_band(DenoiseKernel::Bank, 0x0C7A, 0.05, SR, 1.0);
        let frames = 4 * SR as usize;
        let noise = FullBandNoise::new(0x0C7A, 0.05).frames(frames);
        let mut buf = sine(1_000.0, 0.3, frames, SR);
        add(&mut buf, &noise);
        // A quarter second of tone-over-noise, a quarter second of noise
        // alone, repeatedly: bands around 1 kHz open and close, and their
        // neighbours enter and leave the exemption with them.
        for (i, f) in buf.chunks_exact_mut(2).enumerate() {
            if (i / (SR as usize / 4)) % 2 != 0 {
                f[0] = noise[i * 2];
                f[1] = noise[i * 2 + 1];
            }
        }
        // Kept dry: `process` works in place, so the second pass below
        // needs its own copy of the fixture rather than the first pass's
        // output.
        let dry = buf.clone();

        // One control tick at a time, so a "one-tick move" is what it says.
        let tick = dn.tick_len as usize;
        let mut prev_depth = dn.depth_db();
        let mut prev_open = [false; DENOISE_BANDS];
        let (mut worst, mut at) = (0.0_f32, 0);
        let mut done = 0usize;
        let mut moved = false;
        while done < frames {
            let n = tick.min(frames - done);
            dn.process(&mut buf[done * 2..(done + n) * 2], n, 2, &p);
            let depth = dn.depth_db();
            let mut open = [false; DENOISE_BANDS];
            for (k, b) in dn.band.iter().enumerate() {
                open[k] = b.open;
            }
            for k in 0..DENOISE_BANDS {
                // Only bands that were closed for BOTH ticks: a band that
                // opened is allowed to move fast, that is its whole job.
                if !open[k] && !prev_open[k] {
                    let d = (depth[k] - prev_depth[k]).abs();
                    if d > worst {
                        worst = d;
                        at = k;
                    }
                }
                moved |= open[k] != prev_open[k];
            }
            prev_depth = depth;
            prev_open = open;
            done += n;
        }
        assert!(
            moved,
            "no band ever changed its open flag, so the exemption was never \
             crossed and this test proves nothing"
        );
        assert!(
            worst < 1.5,
            "band {at}'s applied depth moved {worst:.2} dB in one tick while \
             it was closed throughout — the cross-band constraint is jumping \
             the smoother again (it was 3.41 dB before the reorder)"
        );

        // ── and the ordinary open, which is what the 10.40 dB in the
        // defect report was actually measuring ─────────────────────────────
        let block = 64usize;
        let mut dn = learned_full_band(DenoiseKernel::Bank, 0x0C7A, 0.05, SR, 1.0);
        let ticks = block / dn.tick_len as usize;
        // What the 2 ms opening envelope can cover in `ticks` control
        // ticks, as a fraction of the full depth — derived from the
        // shipped coefficient, so slowing DENOISE_OPEN_MS tightens this
        // bound automatically instead of leaving a magic number behind.
        let ceiling = (1.0 - (1.0 - dn.a_open).powi(ticks as i32)) * p.reduce * DENOISE_REDUCE_DB;
        let mut again = dry;
        let traj = analysis::gain_trajectory(&mut dn, &mut again, frames, block, &p);
        let (mut open_worst, mut open_at) = (0.0_f32, 0usize);
        for w in traj.windows(2) {
            for k in 0..DENOISE_BANDS {
                let d = (w[1][k] - w[0][k]).abs();
                if d > open_worst {
                    open_worst = d;
                    open_at = k;
                }
            }
        }
        println!(
            "worst applied-depth move over any band in one {block}-frame block at              REDUCE 1.0: {open_worst:.2} dB (band {open_at}); the open envelope's own              ceiling over {ticks} ticks is {ceiling:.2} dB"
        );
        assert!(
            open_worst <= ceiling,
            "band {open_at} moved {open_worst:.2} dB in one {block}-frame block, past the              {ceiling:.2} dB the {DENOISE_OPEN_MS} ms open envelope can cover in {ticks}              ticks — something is reaching the applied depth without going through the              smoother"
        );
        assert!(
            open_worst < 10.0,
            "the ordinary open moved {open_worst:.2} dB in one {block}-frame block              (it was 9.52 dB before the constraint was reordered and 8.95 after; the              defect report's 10.40 dB figure is this measurement, not the constraint's)"
        );
    }

    /// **Nothing on the audio thread allocates, including a kernel
    /// change.** Every Vec capacity in the unit is byte-identical before
    /// and after ten seconds of audio that flips the selector, the
    /// lookahead and the learn toggle underneath it — the arrangement the
    /// convolver's own contract test uses, for the same reason: a `resize`
    /// nobody noticed is a malloc on the audio callback, and a malloc on
    /// the audio callback is a dropout on somebody else's machine.
    ///
    /// The kernel change is the interesting half. Switching between
    /// Spectral Studio and Spectral Live moves the frame from 1024 points
    /// to 512, which is exactly the kind of change that invites a `vec!` —
    /// every buffer is cut in `new` to the LONGEST frame this rate can ask
    /// for and the switch only re-derives the window, the band layout and
    /// the per-frame coefficients in place.
    ///
    /// The timing is printed as well, because a kernel that cannot run in
    /// realtime is not allocation-free, it is just differently unusable.
    /// **A print learned on a spectral kernel gates the bank like the
    /// bank's own.** The spectral learn freezes each band at the geometric
    /// mean of a windowed periodogram; the bank gates on mean + 2σ + 4 dB
    /// of its resonators' level, a different measurement of the same
    /// noise. Read as a bank knee, the spectral print sat in the middle of
    /// the noise and the bank chattered on it - the owner's "learn on bank
    /// then switch to Studio or Live works, but not vice versa". So a
    /// spectral pass now runs the bank's ear as well and freezes a bank
    /// knee beside the print, and switching to the bank afterwards reads
    /// what a bank learn of the same room would have.
    #[test]
    fn a_print_learned_on_a_spectral_kernel_gates_the_bank_like_its_own() {
        let span = DENOISE_THR_DB.1 - DENOISE_THR_DB.0;
        for kernel in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
            let spectral = learned(kernel, 0x5EED, 0.05, SR, 1.5);
            let bank = learned(DenoiseKernel::Bank, 0x5EED, 0.05, SR, 1.5);
            let (knees, own) = (spectral.knees(), bank.thresholds());
            let mut worst = 0.0f32;
            for (b, (k, t)) in knees.iter().zip(own.iter()).enumerate() {
                if *t < DENOISE_OFF {
                    continue;
                }
                let d = (k - t).abs() * span;
                assert!(
                    d < 2.0,
                    "{}: band {b} knee {:.1} dB against the bank's own {:.1} dB",
                    kernel.label(),
                    DENOISE_THR_DB.0 + k * span,
                    DENOISE_THR_DB.0 + t * span
                );
                worst = worst.max(d);
            }
            assert!(worst > 0.0, "{}: no band was compared", kernel.label());
            // And the print itself is still the spectral kernel's: a knee
            // is not a threshold, or the floor would have moved.
            assert!(
                spectral.thresholds().iter().zip(knees.iter()).any(|(t, k)| (t - k).abs() * span > 3.0),
                "{}: the knees are the print - the spectral floor was overwritten",
                kernel.label()
            );

            // Switched to the bank, the print cuts the noise it learned.
            let mut dn = spectral;
            let frames = (SR * 1.0) as usize;
            let mut buf = Noise::new(0x5EED, 0.05, SR).frames(frames);
            let before = (buf.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / buf.len() as f64).sqrt();
            let mut warm = Noise::new(0x5EED, 0.05, SR).frames(frames);
            dn.process(&mut warm, frames, 2, &on(DenoiseKernel::Bank));
            dn.process(&mut buf, frames, 2, &on(DenoiseKernel::Bank));
            let after = (buf.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / buf.len() as f64).sqrt();
            let cut = 20.0 * (before / after.max(1.0e-12)).log10();
            assert!(
                cut > 6.0,
                "{} print on the bank cut the noise by {cut:.1} dB",
                kernel.label()
            );
        }
    }

    /// A print with no knees of its own - every print from before the
    /// knees existed - gates the bank at its thresholds, as it always did,
    /// and a knee set afterwards wins.
    #[test]
    fn a_print_without_knees_gates_at_its_thresholds() {
        let mut dn = Denoise::new(SR);
        let mut thr = [0.0_f32; DENOISE_BANDS];
        for (i, t) in thr.iter_mut().enumerate() {
            *t = 0.3 + 0.01 * i as f32;
        }
        dn.set_thresholds(&thr);
        for (k, t) in dn.knees().iter().zip(dn.thresholds().iter()) {
            assert!((k - t).abs() < 1.0e-5, "a print without knees gates at {k}, not its threshold {t}");
        }
        let mut knees = thr;
        knees[4] = 0.8;
        dn.set_knees(&knees);
        assert!((dn.knees()[4] - 0.8).abs() < 1.0e-6);
        assert!((dn.thresholds()[4] - thr[4]).abs() < 1.0e-6, "a knee moved the print");
    }

    #[test]
    fn nothing_on_the_audio_thread_allocates_not_even_a_kernel_change() {
        let seconds = 10.0_f32;
        let frames = (SR * seconds) as usize;
        let mut dn = learned_full_band(DenoiseKernel::SpectralStudio, 0xA110C, 0.05, SR, 1.0);
        let mut buf = FullBandNoise::new(0xA110C, 0.05).frames(frames);
        let before = dn.footprint();

        let start = std::time::Instant::now();
        let mut pos = 0usize;
        let mut step = 0usize;
        while pos < frames {
            // Deliberately awkward block sizes, and every dial that can
            // move machinery moving between them.
            let n = [480_usize, 512, 1, 333, 1_024][step % 5].min(frames - pos);
            let p = DenoiseParams {
                learn: if step % 17 == 0 { 1.0 } else { 0.0 },
                lookahead: if step % 3 == 0 { 0.0 } else { 1.0 },
                reduce: (step % 5) as f32 / 4.0,
                ..on(DenoiseKernel::ALL[step % 3])
            };
            dn.process(&mut buf[pos * 2..(pos + n) * 2], n, 2, &p);
            pos += n;
            step += 1;
        }
        let elapsed = start.elapsed().as_secs_f64();

        assert_eq!(
            dn.footprint(),
            before,
            "process grew a buffer somewhere — the allocation contract is broken"
        );
        assert!(
            buf.iter().all(|s| s.is_finite()),
            "ten seconds of dial-flipping produced a non-finite sample"
        );
        let factor = f64::from(seconds) / elapsed;
        println!("denoise: {factor:.0}x realtime across all three kernels");
        #[cfg(not(debug_assertions))]
        assert!(
            factor > 20.0,
            "only {factor:.1}x realtime in release — this unit is meant to \
             sit on every strip"
        );
    }


    // ── the estimator's edges ───────────────────────────────────────────────

    /// The mean applied cut across all 32 bands, in positive dB — one
    /// number for "is this unit still doing its job".
    fn mean_depth(dn: &Denoise) -> f32 {
        let d = dn.depth_db();
        d.iter().sum::<f32>() / d.len() as f32
    }

    /// Learned noise in, cut out, measured from the AUDIO over the second
    /// half of two seconds — and the unit is left running where it ended,
    /// so two calls in a row measure "before" and "after" on one continuous
    /// stream.
    fn cut_over_two_seconds(dn: &mut Denoise, p: &DenoiseParams, seed: u32, amp: f32) -> (f32, usize) {
        let frames = 2 * SR as usize;
        let dry = FullBandNoise::new(seed, amp).frames(frames);
        let mut wet = dry.clone();
        dn.process(&mut wet, frames, 2, p);
        let bad = wet.iter().filter(|s| !s.is_finite()).count();
        (rms_db(&dry, frames / 2) - rms_db(&wet, frames / 2), bad)
    }

    /// Run `dn` forward to the next frame boundary, so that a block of
    /// exactly one hop handed in afterwards fires exactly one frame — and
    /// `depth_db()`, `sp.pow` and `sp.lambda` then describe THAT frame.
    ///
    /// Without this a test can only see what survived, never what happened:
    /// a one-frame excursion inside a tenth of a second of audio is gone by
    /// the time the block returns, which is how the settled-snapshot
    /// measurements in this file missed 13 dB holes for as long as they
    /// did.
    fn to_frame_boundary(dn: &mut Denoise, p: &DenoiseParams, seed: u32, amp: f32) {
        let pad = (dn.sp.hop - dn.sp.stage_fill) % dn.sp.hop;
        if pad > 0 {
            let mut b = FullBandNoise::new(seed, amp).frames(pad);
            dn.process(&mut b, pad, 2, p);
        }
    }

    /// **One non-finite input sample must not turn the unit into a wire,
    /// and must not reach the output.**
    ///
    /// Tangent hosts third-party LV2 plugins and the denoise sits in the
    /// same chain; nothing washes the buffer between them, `wavread.rs`
    /// already scrubs file samples on the way in, and `effects.rs` guards
    /// the delay's feedback recursion with exactly the pattern this one was
    /// missing. So a NaN or an infinity arriving from upstream is a real
    /// event, and both engines used to be destroyed by one.
    ///
    /// The SPECTRAL failure was the quiet one and it was permanent. One
    /// infinity in the analysed frame made `pow[k]` non-finite; the tracker
    /// then computed `(1 − ph1)·p` with `ph1` exactly 1.0, which is
    /// `0.0 · inf` = NaN; and `f32::clamp` passes NaN through unchanged, so
    /// `lambda[k]` was NaN for the life of the session. Downstream
    /// `lam_b[b].max(1e-30)` turned the NaN quietly back into 1e-30, ξ
    /// exploded, every band opened, and the measured broadband cut went
    /// from 23.99 dB to 0.00 with the output still finite, nothing logged
    /// and nothing audibly broken. Only `clear()`, `set_thresholds()` or a
    /// kernel change healed it, and normal playback calls none of them.
    ///
    /// The BANK failure was the loud one: 96,000 of the next 192,000 output
    /// samples came out non-finite.
    ///
    /// Both assertions matter and they guard different lines. "Still
    /// cutting" is the tracker's guards; "output finite" is the scrub at
    /// each kernel's own read.
    #[test]
    fn one_non_finite_sample_does_not_stop_the_denoiser() {
        for kernel in DenoiseKernel::ALL {
            for (name, poison) in [
                ("+inf", f32::INFINITY),
                ("-inf", f32::NEG_INFINITY),
                ("NaN", f32::NAN),
            ] {
                let mut dn = learned_full_band(kernel, 0x5AFE, 0.05, SR, 1.0);
                let p = DenoiseParams {
                    reduce: 1.0,
                    ..on(kernel)
                };
                let (before, _) = cut_over_two_seconds(&mut dn, &p, 0x5AFE, 0.05);
                let depth_before = mean_depth(&dn);

                // One sample, one channel, in the middle of an ordinary
                // block of the same noise.
                let tenth = SR as usize / 10;
                let mut blk = FullBandNoise::new(0x5AFE, 0.05).frames(tenth);
                blk[2 * (tenth / 2)] = poison;
                dn.process(&mut blk, tenth, 2, &p);
                assert!(
                    blk.iter().all(|s| s.is_finite()),
                    "{}: one {name} sample put {} non-finite samples into the OUTPUT",
                    kernel.label(),
                    blk.iter().filter(|s| !s.is_finite()).count()
                );

                // Two more seconds of exactly the noise it was cutting.
                let (after, bad) = cut_over_two_seconds(&mut dn, &p, 0x5AFE, 0.05);
                assert_eq!(
                    bad,
                    0,
                    "{}: {bad} non-finite output samples two seconds after one {name}",
                    kernel.label()
                );
                let lam_bad = dn.sp.lambda[..=dn.sp.n / 2]
                    .iter()
                    .filter(|v| !v.is_finite())
                    .count();
                assert_eq!(
                    lam_bad,
                    0,
                    "{}: {lam_bad} of the noise tracker's bins are non-finite after one {name}",
                    kernel.label()
                );
                assert!(
                    (before - after).abs() < 0.5,
                    "{}: one {name} sample cost the unit its cut — {before:.2} dB before, \
                     {after:.2} dB two seconds after (band depths {:.2} -> {:.2})",
                    kernel.label(),
                    depth_before,
                    mean_depth(&dn)
                );
            }
        }
    }

    /// **A finite sample too big to square must not lock the noise tracker
    /// either**, and it is a different guard from the one above.
    ///
    /// `1e20` is a perfectly ordinary f32. It comes out of a 1024-point
    /// transform at about 7e19, and `band_power` squares that: 4.9e39, past
    /// f32's 3.4e38, so `pow[k]` is INFINITY with every input sample
    /// finite. Scrubbing the input cannot catch this — the input was fine —
    /// and from there it is the same NaN latch as above, measured the same
    /// way: 513 of 513 bins non-finite and the cut gone from 24 dB to zero
    /// for the rest of the session. The guard is one branch in
    /// `Spectral::band_power`, which is also what keeps the band energies
    /// finite for the decision-directed recursion.
    #[test]
    fn an_enormous_finite_sample_does_not_lock_the_noise_tracker() {
        for kernel in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
            let mut dn = learned_full_band(kernel, 0x5AFE, 0.05, SR, 1.0);
            let p = DenoiseParams {
                reduce: 1.0,
                ..on(kernel)
            };
            let (before, _) = cut_over_two_seconds(&mut dn, &p, 0x5AFE, 0.05);

            // Exactly one frame, so what is read afterwards is the frame
            // the overflow landed in rather than whatever survived it.
            to_frame_boundary(&mut dn, &p, 0x5AFE, 0.05);
            let hop = dn.sp.hop;
            let mut blk = FullBandNoise::new(0x5AFE, 0.05).frames(hop);
            blk[2 * (hop / 2)] = 1.0e20;
            dn.process(&mut blk, hop, 2, &p);
            let pow_bad = dn.sp.pow[..=dn.sp.n / 2]
                .iter()
                .filter(|v| !v.is_finite())
                .count();
            assert_eq!(
                pow_bad,
                0,
                "{}: {pow_bad} bins of the periodogram overflowed to non-finite on one \
                 1e20 sample — the band energies built from them are non-finite too",
                kernel.label()
            );

            let (after, _) = cut_over_two_seconds(&mut dn, &p, 0x5AFE, 0.05);
            let lam_bad = dn.sp.lambda[..=dn.sp.n / 2]
                .iter()
                .filter(|v| !v.is_finite())
                .count();
            assert_eq!(
                lam_bad,
                0,
                "{}: {lam_bad} tracker bins are non-finite after one 1e20 sample — \
                 the periodogram overflowed and the NaN latched",
                kernel.label()
            );
            assert!(
                (before - after).abs() < 0.5,
                "{}: one 1e20 sample cost the unit its cut — {before:.2} dB before, \
                 {after:.2} dB two seconds after",
                kernel.label()
            );
        }
    }

    /// **The noise tracker cannot hold a value that is not a number**, however
    /// it got one.
    ///
    /// The two tests above come at the recursion through the audio, which
    /// is how the fault was found; this one puts the poison directly where
    /// it used to lodge, because the contract is about the RECURSION and
    /// not about any particular route into it. Both assertions guard a
    /// different line:
    ///
    /// * The estimate heals. `f32::clamp` compares with `<` and `>`, both
    ///   false for NaN, so `next.clamp(floor, ceil)` used to store NaN
    ///   unchanged and every later frame re-entered from it. It now falls
    ///   back to the learned floor.
    /// * The frame it heals in still cuts. `f32::min` returns THE OTHER
    ///   OPERAND when one side is NaN, so the log-GLR clamp used to answer
    ///   200 for an unknown SNR — which is `ph1 == 1.0` exactly, i.e. total
    ///   certainty that the bin is signal — and that certainty is voted
    ///   into the band's presence statistic and opens it. An unknown SNR
    ///   now reads as the constant term, `ph1 ≈ 0.03`.
    #[test]
    fn the_noise_tracker_cannot_latch_a_non_finite_estimate() {
        for kernel in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
            for poison in [f32::NAN, f32::INFINITY] {
                let mut dn = learned_full_band(kernel, 0x1A7C, 0.05, SR, 1.0);
                let p = DenoiseParams {
                    reduce: 1.0,
                    ..on(kernel)
                };
                let frames = 2 * SR as usize;
                let mut buf = FullBandNoise::new(0x1A7C, 0.05).frames(frames);
                dn.process(&mut buf, frames, 2, &p);
                to_frame_boundary(&mut dn, &p, 0x1A7C, 0.05);
                let before = dn.depth_db();

                // Every bin the middle band owns, plus its neighbours'.
                let bins = dn.sp.n / 2;
                let poisoned: Vec<usize> = (0..=bins)
                    .filter(|&k| (14..=16).contains(&(dn.sp.map_lo[k] as usize)))
                    .collect();
                assert!(!poisoned.is_empty(), "the fixture poisoned no bins");
                for &k in poisoned.iter() {
                    dn.sp.lambda[k] = poison;
                }

                // ONE frame, so the depth read below is the depth in the
                // frame that healed — the excursion, not what was left of
                // it a tenth of a second later.
                let hop = dn.sp.hop;
                let mut blk = FullBandNoise::new(0x1A7C, 0.05).frames(hop);
                dn.process(&mut blk, hop, 2, &p);

                let left = poisoned
                    .iter()
                    .filter(|&&k| !dn.sp.lambda[k].is_finite())
                    .count();
                assert_eq!(
                    left,
                    0,
                    "{}: {left} of {} poisoned bins are still non-finite — the recursion \
                     latched what it was handed",
                    kernel.label(),
                    poisoned.len()
                );
                let after = dn.depth_db();
                let worst = (14..=16).fold(0.0_f32, |m, b| m.max((before[b] - after[b]).abs()));
                assert!(
                    worst < 1.0,
                    "{}: band 14..16's cut moved {worst:.2} dB in the frame that healed a \
                     {poison} — an unknown SNR is being read as certainty of signal",
                    kernel.label()
                );
            }
        }
    }

    /// **The bottom bands are wide enough to measure.**
    ///
    /// The ERB walk's floor was two bins, and two bins is where three
    /// separate promises stopped being kept at once — the per-frame
    /// flatness of the dial, the slope limit the triangles are supposed to
    /// impose, and an aliasing figure that meant the same thing at both
    /// frame lengths. [`MIN_BAND_BINS`] carries the measurements; this
    /// holds the geometry it depends on, at every rate and both frames,
    /// because the walk is a binary search and a rate it was never tried at
    /// is a rate where it may land anywhere.
    #[test]
    fn every_band_is_at_least_four_bins_wide() {
        // The DECISION, held against a literal. Asserting the layout
        // against [`MIN_BAND_BINS`] alone would be a test that moves
        // whenever the thing it guards moves — it would still catch the
        // walk rounding a band away, which is the second assertion below,
        // but it could never catch anybody putting the floor back to two.
        assert!(
            MIN_BAND_BINS >= 4.0,
            "the ERB walk's floor is {MIN_BAND_BINS} bins. At two, band 0's per-frame \
             peak-to-peak was 13.02 dB at the top of the dial, the realised mask stepped \
             13.27 dB between adjacent bins, and the aliasing figure differed by 7.4 dB \
             between the two frame lengths. See MIN_BAND_BINS for the whole table."
        );
        for sr in [44_100.0_f32, 48_000.0, 88_200.0, 96_000.0, 192_000.0] {
            for kernel in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
                let mut dn = Denoise::new(sr);
                dn.process(&mut [0.0_f32; 8], 4, 2, &on(kernel));
                let bins = dn.sp.n / 2;
                // The band edges, read back out of the map the kernel
                // actually uses rather than recomputed from the walk: a
                // test that re-derives the thing it is checking checks
                // nothing.
                let mut edge = [0_usize; DENOISE_BANDS];
                for k in (0..=bins).rev() {
                    let b = dn.sp.map_lo[k] as usize;
                    if dn.sp.map_frac[k] == 0.0 {
                        edge[b] = k;
                    }
                }
                // And the walk actually DELIVERS its floor at this rate:
                // the centres are rounded onto the bin grid and squeezed
                // by `min(room)`, either of which could collapse a band
                // without the constant changing at all.
                for b in 1..DENOISE_BANDS {
                    let width = edge[b] - edge[b - 1];
                    assert!(
                        width >= MIN_BAND_BINS as usize,
                        "{sr} Hz {}: band {b} is {width} bins wide, under the {} the \
                         statistics and the slope limit are argued from (edges {:?})",
                        kernel.label(),
                        MIN_BAND_BINS,
                        &edge[..8]
                    );
                }
            }
        }
    }

    /// **The realised mask's impulse response is short, and about equally
    /// short at both frame lengths.**
    ///
    /// This is the number `lay_out_bands` argues from and it had never been
    /// measured in the tree. A per-bin mask is a filter as long as the
    /// frame, so its circular convolution wraps and the wrap is audible
    /// pre-echo; interpolating a mask across triangles constrains how fast
    /// it can change with frequency, which is the same thing as
    /// constraining how long its impulse response is.
    ///
    /// Measured here as: transform the mask the kernel is actually applying
    /// after two seconds of a tone over learned noise at the top of the
    /// dial, and take the fraction of its circular impulse response's
    /// energy that lands more than half a hop from the centre. The free
    /// per-bin Wiener mask built from the SAME frame's own `pow` and
    /// `lambda` is the control — it is what "improve this into a per-bin
    /// mask" would cost.
    ///
    /// On the old two-bin floor this read −23.5 dB at Studio and −16.1 at
    /// Live: a 7.4 dB spread between two kernels that differ only in
    /// resolution, which is why the bound below is the same number for both
    /// rather than one each.
    #[test]
    fn the_band_mask_stays_short_at_both_frame_lengths() {
        let alias_db = |g: &[f32], n: usize| -> f32 {
            let mut re = vec![0.0_f64; n];
            let mut im = vec![0.0_f64; n];
            for (k, slot) in re.iter_mut().enumerate() {
                *slot = f64::from(g[k]);
            }
            analysis::fft(&mut re, &mut im);
            let (mut total, mut out) = (0.0_f64, 0.0_f64);
            for i in 0..n {
                let e = re[i] * re[i] + im[i] * im[i];
                total += e;
                if i.min(n - i) > n / 4 {
                    out += e;
                }
            }
            (10.0 * (out / total.max(1.0e-300)).max(1.0e-300).log10()) as f32
        };

        let mut rows = Vec::new();
        for kernel in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
            let mut dn = learned_full_band(kernel, 0x0C7A, 0.05, SR, 1.0);
            let p = DenoiseParams {
                reduce: 1.0,
                ..on(kernel)
            };
            let frames = 2 * SR as usize;
            let mut buf = sine(220.0, 0.3, frames, SR);
            add(&mut buf, &FullBandNoise::new(0x0C7A, 0.05).frames(frames));
            dn.process(&mut buf, frames, 2, &p);

            let (n, bins) = (dn.sp.n, dn.sp.n / 2);
            // The mask is not flat — a flat mask is a delta and would pass
            // this test while proving nothing about the slope limit.
            let (lo, hi) = dn.sp.gain[..=bins]
                .iter()
                .fold((f32::INFINITY, 0.0_f32), |(l, h), &g| (l.min(g), h.max(g)));
            assert!(
                20.0 * (hi / lo.max(1.0e-12)).log10() > 12.0,
                "{}: the mask only spans {:.1} dB, so this measurement is about a delta",
                kernel.label(),
                20.0 * (hi / lo.max(1.0e-12)).log10()
            );

            let mut free = vec![0.0_f32; n];
            for k in 0..=bins {
                let xi = ((dn.sp.pow[k] / dn.sp.lambda[k].max(1.0e-30)) - 1.0).max(0.01);
                free[k] = xi / (1.0 + xi);
            }
            for k in bins + 1..n {
                free[k] = free[n - k];
            }
            rows.push((
                kernel.label(),
                n,
                alias_db(&dn.sp.gain, n),
                alias_db(&free, n),
            ));
        }

        let table = rows
            .iter()
            .map(|r| {
                format!(
                    "  {:16} n={:>5}: realised mask {:>6.1} dB, free per-bin mask {:>6.1} dB",
                    r.0, r.1, r.2, r.3
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        println!("energy outside +-R/2 of the mask's impulse response:\n{table}");
        assert!(
            rows.iter().all(|r| r.2 < -27.0),
            "the band mask is no longer short (it must stay under -27 dB at BOTH frame \
             lengths — it was -23.5/-16.1 on the two-bin floor):\n{table}"
        );
        assert!(
            rows.iter().all(|r| r.3 - r.2 > 15.0),
            "the band structure has stopped buying anything over a free per-bin mask:\n{table}"
        );
    }

    /// **The dial is flat and equal to itself in EVERY frame, not just the
    /// one a settled snapshot happens to land on.**
    ///
    /// [`noise_only_gain_is_flat_and_equals_the_dial`] reads
    /// `analysis::spectral_response_db`, which reads the LAST frame's mask;
    /// the trajectory it also collects was only ever reduced to a drift
    /// figure between two tenth-run means, and a mean is exactly the wrong
    /// instrument for an excursion. Measured per frame over the settled
    /// half of eight seconds, band 0 was 13.02 dB peak-to-peak at Studio's
    /// top of dial — that is a 13 dB hole in the reduction, on and gone
    /// inside 21 ms, in the band where a hole is a thump. 0.117 % of Studio
    /// band-frames were more than 1 dB short of the dial at 24 dB, 0.025 %
    /// at 18 and 0.008 % at 12; Live's worst was 5.26 dB.
    ///
    /// Both halves of the cause are in the fix and both are pinned
    /// elsewhere: [`MIN_BAND_BINS`] (a two-bin band's energy is a
    /// chi-square with about three degrees of freedom) and [`DD_BW_MS_HZ`]
    /// (the recursion rectifies that scatter, so the narrow bands need a
    /// longer noise-side memory the way the bank's level window does).
    #[test]
    fn the_gain_is_flat_and_equal_to_the_dial_in_every_frame() {
        let mut rows = Vec::new();
        for kernel in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
            for &reduce in REDUCE_SPAN.iter() {
                let cap = reduce * DENOISE_REDUCE_DB;
                let mut dn = learned_full_band(kernel, 0xD1A1, 0.05, SR, 1.0);
                let p = DenoiseParams {
                    reduce,
                    ..on(kernel)
                };
                let frames = 8 * SR as usize;
                let mut buf = FullBandNoise::new(0xD1A1, 0.05).frames(frames);
                // One snapshot per FRAME: the mask only moves when a frame
                // fires, so a finer grid would report the same number twice
                // and a coarser one would step over the excursion.
                let hop = dn.sp.hop;
                let traj = analysis::gain_trajectory(&mut dn, &mut buf, frames, hop, &p);
                let settled = &traj[traj.len() / 2..];

                let (mut worst_pp, mut worst_short, mut at) = (0.0_f32, 0.0_f32, 0usize);
                let mut short = 0usize;
                for b in 0..DENOISE_BANDS {
                    let lo = settled.iter().map(|s| s[b]).fold(f32::INFINITY, f32::min);
                    let hi = settled
                        .iter()
                        .map(|s| s[b])
                        .fold(f32::NEG_INFINITY, f32::max);
                    if hi - lo > worst_pp {
                        worst_pp = hi - lo;
                        at = b;
                    }
                    worst_short = worst_short.max(cap - lo);
                    short += settled.iter().filter(|s| cap - s[b] > 1.0).count();
                }
                let pct = 100.0 * short as f32 / (settled.len() * DENOISE_BANDS) as f32;
                rows.push((kernel.label(), cap, worst_pp, at, worst_short, pct));
            }
        }

        let table = rows
            .iter()
            .map(|r| {
                format!(
                    "  {:16} dial {:>4.1} dB: worst per-band peak-to-peak {:>6.3} dB (band {}), \
                     worst single-frame shortfall {:>6.3} dB, {:.3} % of band-frames >1 dB short",
                    r.0, r.1, r.2, r.3, r.4, r.5
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        println!("per-frame band depths over the settled half of 8 s:\n{table}");
        assert!(
            rows.iter().all(|r| r.2 < 1.0),
            "a band's cut is not flat FRAME BY FRAME (peak-to-peak must stay under 1.0 dB; \
             it was 13.02 dB at band 0 on the two-bin floor):\n{table}"
        );
        assert!(
            rows.iter().all(|r| r.4 < 1.0),
            "a band fell short of the dial in a single frame (must stay under 1.0 dB; \
             it was 13.02 dB):\n{table}"
        );
    }

    /// **A run of OFF bands does not rob its live neighbours of the dial.**
    ///
    /// Both acceptance tests learn with [`learned_full_band`], which arms
    /// all 32, so neither can see this. OFF runs are an expected state:
    /// `freeze_thresholds` documents a band that heard silence landing
    /// there as "the right answer", `set_thresholds` maps any non-finite
    /// persisted value to OFF, and an ordinary learn pass on lossy-decoded
    /// material drives the top band there through its own brickwall.
    ///
    /// An OFF band never reduces, so it has no λ worth the name — only the
    /// −100 dB placeholder `refloor` lays for a band nobody taught — and
    /// its ξ is therefore `energy / placeholder`, measured at 7·10⁵. The
    /// cross-band vote used to hand a quarter of that to each neighbour,
    /// which pins `g_H1` at unity and the mixed gain at 0 dB. Measured at a
    /// dial asking 12 dB with bands 20..22 off: band 19 delivered 6.25 dB
    /// and band 23 delivered 7.42. Runs of one and two were safe only
    /// because no band in them is interior — the triangular refloor still
    /// gave those a real λ from their live neighbours — which is why the
    /// sweep below starts at three and why a run of one is checked too.
    #[test]
    fn a_run_of_off_bands_does_not_rob_its_live_neighbours() {
        let mut rows = Vec::new();
        for kernel in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
            for run in 1..=5usize {
                for &reduce in [0.5_f32, 1.0].iter() {
                    let cap = reduce * DENOISE_REDUCE_DB;
                    let mut dn = learned_full_band(kernel, 0x0FF0, 0.05, SR, 1.0);
                    let mut thr = dn.thresholds();
                    for t in thr[20..20 + run].iter_mut() {
                        *t = 0.0;
                    }
                    dn.set_thresholds(&thr);
                    let p = DenoiseParams {
                        reduce,
                        ..on(kernel)
                    };
                    let frames = 4 * SR as usize;
                    let mut buf = FullBandNoise::new(0x0FF0, 0.05).frames(frames);
                    dn.process(&mut buf, frames, 2, &p);
                    let d = dn.depth_db();
                    // The bands either side of the run, and the run itself
                    // — which must still be a wire, because that is what
                    // OFF means.
                    let (below, above) = (d[19], d[20 + run]);
                    let inside = d[20..20 + run].iter().fold(0.0_f32, |m, &x| m.max(x.abs()));
                    rows.push((kernel.label(), run, cap, below, above, inside));
                }
            }
        }

        let table = rows
            .iter()
            .map(|r| {
                format!(
                    "  {:16} {} OFF from band 20, dial {:>4.1} dB: band 19 cuts {:>6.3}, \
                     band {} cuts {:>6.3}, the OFF run itself {:>6.3}",
                    r.0,
                    r.1,
                    r.2,
                    r.3,
                    20 + r.1,
                    r.4,
                    r.5
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        println!("live neighbours of an OFF run:\n{table}");
        assert!(
            rows.iter().all(|r| r.5 < 0.05),
            "a band whose profile is OFF reduced anyway:\n{table}"
        );
        assert!(
            rows.iter().all(|r| (r.3 - r.2).abs() < 0.5 && (r.4 - r.2).abs() < 0.5),
            "a live band beside an OFF run is not getting the dialled cut (it was 6.25 and \
             7.42 dB against a 12 dB dial before the vote learned to skip OFF):\n{table}"
        );
    }

    /// **A stale noise print holds the dial for 18 dB, and then it does
    /// not — and this is where it stops.**
    ///
    /// [`NOISE_CEIL_DB`] is a deliberate one-way limit: an estimate that
    /// has wandered far above what the user taught is tracking the
    /// PROGRAMME, not the noise, and letting it climb costs signal. Past
    /// it, λ pins, γ stays large, ξ stays large, every band opens, and the
    /// unit stops reducing — which is a CLIFF, not a taper, and the exact
    /// symptom the whole rewrite was commissioned for ("I push REDUCE and
    /// almost nothing happens"). At the first 12 dB the cliff fell inside
    /// the range of "the room got a bit louder": 12.00 dB delivered at
    /// +16 dB above the print, 4.07 at +18, 1.71 at +20. At 18 it moved to
    /// +20, which is where the guarantee below used to read 18.
    ///
    /// **It now stands at +20, and the ladder printed below is why the
    /// number moved.** [`NOISE_CEIL_DB`] went to 24 for the challenge
    /// take's sake (see its own note), and this is the other end of that
    /// change measured rather than assumed: both spectral kernels deliver
    /// the full 12.00 dB at +20 dB of staleness and start to give way at
    /// +24 (Studio 10.71, Live 8.19). So the promise is now two decibels
    /// wider than it was, and the assertion is written at 20 rather than at
    /// 18 — a guarantee that is not raised when the thing it guards
    /// improves is a guarantee that quietly stops being tested.
    ///
    /// The number was chosen against a measurement and this is the
    /// measurement. The other direction needs no ceiling and is checked
    /// too: a QUIETER room than the print still gets the whole dial,
    /// because the floor is one-way.
    ///
    /// The bank's own cliff is in the table for scale and is not asserted —
    /// it is an expander whose thresholds were frozen at mean + 2σ of a
    /// quieter room, and it stops expanding at +6 dB. That is a different
    /// mechanism in a different engine and it is not new here.
    #[test]
    fn a_stale_print_holds_the_dial_until_it_does_not() {
        // The GUARANTEE, as a literal: how stale a print may be and still
        // buy the whole dial. Written down here rather than read out of
        // [`NOISE_CEIL_DB`], because a bound taken from the constant it
        // guards follows that constant down and reports nothing.
        const STALE_GUARANTEE_DB: f32 = 20.0;
        assert!(
            NOISE_CEIL_DB >= STALE_GUARANTEE_DB,
            "NOISE_CEIL_DB is {NOISE_CEIL_DB} dB, under the {STALE_GUARANTEE_DB} dB of \
             staleness this unit promises to absorb"
        );
        let ladder = [-20.0_f32, -6.0, 0.0, 6.0, 12.0, 18.0, 20.0, 24.0];
        let mut rows = Vec::new();
        for kernel in DenoiseKernel::ALL {
            for &db in ladder.iter() {
                let mut dn = learned_full_band(kernel, 0x40E7, 0.05, SR, 1.0);
                let p = DenoiseParams {
                    reduce: 0.5,
                    ..on(kernel)
                };
                let frames = 4 * SR as usize;
                let amp = 0.05 * 10.0_f32.powf(db / 20.0);
                let dry = FullBandNoise::new(0x40E7, amp).frames(frames);
                let mut wet = dry.clone();
                dn.process(&mut wet, frames, 2, &p);
                rows.push((kernel, db, rms_db(&dry, frames / 2) - rms_db(&wet, frames / 2)));
            }
        }

        let table = rows
            .iter()
            .map(|r| {
                format!(
                    "  {:16} the room is {:+5.0} dB against the print: {:>6.2} dB removed \
                     (dial asks 12.0)",
                    r.0.label(),
                    r.1,
                    r.2
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        println!("a print that no longer describes the room:\n{table}");
        // The guarantee, on the spectral kernels the dial is a promise
        // about: everything at or under the ceiling delivers the dial.
        assert!(
            rows.iter()
                .filter(|r| r.0 != DenoiseKernel::Bank && r.1 <= STALE_GUARANTEE_DB)
                .all(|r| r.2 > 11.5),
            "the spectral kernels no longer deliver the dial for a print stale by up to \
             {STALE_GUARANTEE_DB} dB — the cliff has moved DOWN into the range of an \
             ordinary room change (it was between +16 and +18 dB when NOISE_CEIL_DB \
             was 12, and between +20 and +24 when it went to 24):\n{table}"
        );
    }

    /// **A room that got livelier in ONE region than the bar it was taught
    /// on is still the room, and still gets the whole dial.**
    ///
    /// This is [`VAD_QUIET_DB`]'s guard, and it is the synthetic form of
    /// the fault the owner heard as "a crispy bathroom sounding frequency
    /// that sounds like a digital artifact". On his take the mid band
    /// between words sits 6 dB over its own learned print at the median and
    /// past 12 dB in a quarter of the frames — breath, mouth, clothing, a
    /// chair, none of which is in the quietest half-second anybody picks to
    /// teach the unit with. The speech-presence probability is a PER-BIN
    /// SNR test against a fixed 15 dB, so every one of those bins read as
    /// speech at 0.99999, λ froze at the print, and the band rode open
    /// through a third of the between-word frames. Measured against a
    /// reference render of the same take: −8.55 dB delivered where the
    /// reference delivered −12.88.
    ///
    /// So: a print taught on flat noise, then the same noise with the
    /// 150 Hz–2 kHz region alone brought up about 13 dB. The frame as a
    /// whole barely moves — that region is a slice of a full-band spectrum
    /// — so the broadband gate calls it room and lets the estimate climb;
    /// the bins in it, on their own, look exactly like speech.
    ///
    /// **The window is the first second, and that is the measurement, not
    /// a convenience.** Left running for six seconds the un-gated tracker
    /// gets there in the end: [`SPP_STAGNATION_CAP`] leaks one percent of
    /// every frame into a pinned estimate, which closes a 13 dB gap with an
    /// effective time constant of several seconds. A gap between two words
    /// is two hundred milliseconds. An estimate that arrives after the
    /// take is over is the same as one that never arrives, so what is
    /// asserted is that the room is caught inside a second.
    ///
    /// **The revert that makes this bite is dropping `* speech` from
    /// `track` in [`Denoise::spectral_suppress`].** Measured with it gone:
    /// the lively region comes back **2.97 dB down at 400 Hz and 4.46 at
    /// 1 kHz** against a dial asking 12, while the quiet region at 8 kHz
    /// still gets its 12.04 — the mid-forward tilt, in three numbers, in
    /// one run. With the gate in they read 11.78, 12.17 and 12.04.
    #[test]
    fn a_room_livelier_in_one_region_than_its_print_still_gets_the_dial() {
        let mut rows = Vec::new();
        for kernel in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
            let mut dn = learned_full_band(kernel, 0x11EF, 0.05, SR, 1.0);
            let p = DenoiseParams {
                reduce: 0.5,
                ..on(kernel)
            };
            // Six seconds: long against λ's 71.7 ms and against the
            // decision-directed recursion's second, so what is measured is
            // where the tracker ARRIVES rather than how it set off.
            let frames = 6 * SR as usize;
            let mut dry = FullBandNoise::new(0x11EF, 0.05).frames(frames);
            add(&mut dry, &Noise::new(0x5E77, 0.30, SR).frames(frames));
            let mut wet = dry.clone();
            dn.process(&mut wet, frames, 2, &p);
            let (from, len) = (SR as usize / 4, SR as usize);
            let psd_dry = analysis::power_spectrum(&dry, from, len);
            let psd_wet = analysis::power_spectrum(&wet, from, len);
            let at = |hz: f32| analysis::psd_cut_at(&psd_dry, &psd_wet, SR, hz);
            let lift = |hz: f32| {
                let flat = analysis::power_spectrum(
                    &FullBandNoise::new(0x11EF, 0.05).frames(frames),
                    from,
                    len,
                );
                analysis::psd_cut_at(&psd_dry, &flat, SR, hz)
            };
            rows.push((kernel, lift(400.0), at(400.0), at(1_000.0), at(8_000.0)));
        }

        let table = rows
            .iter()
            .map(|r| {
                format!(
                    "  {:16} the lively region is {:+5.1} dB over its print: \
                     {:>5.2} dB cut at 400 Hz, {:>5.2} at 1 kHz, {:>5.2} at 8 kHz \
                     (dial asks 12.0)",
                    r.0.label(),
                    r.1,
                    r.2,
                    r.3,
                    r.4
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        println!("a print that under-reads one region:\n{table}");
        assert!(
            rows.iter().all(|r| r.1 > 9.0),
            "the fixture is not lively enough to be about anything:\n{table}"
        );
        assert!(
            rows.iter().all(|r| r.2 > 10.0 && r.3 > 10.0),
            "the lively region no longer gets the dial — the tracker has stopped being \
             allowed to hear a room that is livelier than the bar it was taught on, which \
             is the whole of the mid-forward tilt the owner called a bathroom:\n{table}"
        );
        assert!(
            rows.iter().all(|r| r.4 > 10.0),
            "the quiet region stopped getting the dial, so this test is measuring a unit \
             that has broken generally rather than the tilt:\n{table}"
        );
    }

    /// **A learn pass over digital silence keeps the profile it had, and
    /// says that it did.**
    ///
    /// A pass over true zeroes puts every band's geometric mean below the
    /// bottom of [`DENOISE_THR_DB`]. All 32 norms clamp to zero, which is
    /// OFF; [`Denoise::recount`] then reads nothing live and both kernels
    /// take the wire path — `process` hands the buffer back untouched and
    /// reports success. The panel meanwhile shows a full set of frozen
    /// thresholds, because a freeze DID happen. There is no error, no
    /// meter movement and no sound: the unit is off and looks armed.
    ///
    /// It was found by the offline probe, which picks the quietest
    /// half-second of a file to learn on and landed on a stretch of the
    /// zeroes that make up 7.4 % of the owner's take — it wrote a file
    /// bit-identical to its input. A person holding LEARN over a silent bar
    /// gets the same thing with a mouse.
    ///
    /// **The revert that makes this bite is deleting the
    /// [`DenoiseLearn::Silent`] arm from `freeze_spectral_thresholds` (or
    /// from `freeze_thresholds`, for the bank).** The profile is then
    /// overwritten with 32 zeroes and the assertions below fail three ways
    /// at once: the thresholds move, `last_learn` stops saying anything,
    /// and the unit stops cutting.
    #[test]
    fn a_learn_pass_over_digital_silence_keeps_the_profile_it_had() {
        for kernel in DenoiseKernel::ALL {
            let mut dn = learned_full_band(kernel, 0x51_1E, 0.05, SR, 1.0);
            assert_eq!(
                dn.last_learn(),
                DenoiseLearn::Taught,
                "{}: the fixture's own learn pass did not take",
                kernel.label()
            );
            let taught = dn.thresholds();
            assert!(
                taught.iter().any(|t| *t >= DENOISE_OFF),
                "{}: the fixture learned nothing, so this test is about nothing",
                kernel.label()
            );

            // Four seconds of digital silence, held under LEARN, then the
            // falling edge — the panel's own gesture, over a silent bar.
            //
            // Four and not one because of the BANK, whose learn pass reads
            // a per-band level envelope that runs to 400 ms at the bottom
            // of the grid (see [`LEVEL_BW_MS_HZ`]). One second of zeroes
            // leaves those envelopes only 21 dB down from the noise that
            // came before, which is a quiet room and not silence, and the
            // pass is rightly taken. Four seconds puts them 87 dB down,
            // under the bottom of [`DENOISE_THR_DB`], which is the state
            // this guard is about. The spectral kernels get there in one
            // frame either way — their statistic is the frame's own
            // periodogram, with no envelope in front of it.
            let learn = DenoiseParams {
                learn: 1.0,
                ..on(kernel)
            };
            let frames = 4 * SR as usize;
            let mut quiet = vec![0.0_f32; frames * 2];
            dn.process(&mut quiet, frames, 2, &learn);
            let mut tiny = [0.0_f32; 8];
            dn.process(&mut tiny, 4, 2, &on(kernel));

            assert_eq!(
                dn.last_learn(),
                DenoiseLearn::Silent,
                "{}: a pass over digital silence did not report itself as one",
                kernel.label()
            );
            assert_eq!(
                dn.thresholds(),
                taught,
                "{}: a pass over digital silence overwrote the profile that was there",
                kernel.label()
            );

            // And the unit is still a denoiser, not a wire: the same noise
            // it was taught still comes back cut.
            let p = on(kernel);
            let n = 2 * SR as usize;
            let dry = FullBandNoise::new(0x51_1E, 0.05).frames(n);
            let mut wet = dry.clone();
            dn.process(&mut wet, n, 2, &p);
            let cut = rms_db(&dry, n / 2) - rms_db(&wet, n / 2);
            assert!(
                cut > 6.0,
                "{}: after a silent learn pass the unit only removes {cut:.2} dB — it has \
                 been turned into a wire that reports success",
                kernel.label()
            );
        }
    }

    // ── the target, met ─────────────────────────────────────────────────────
    //
    // The two tests below assert what this unit is SUPPOSED to do. They were
    // written first, against the bank, where they failed and carried
    // `#[ignore]`; the numbers they measured then are kept below as
    // [`BASE_A`] and [`BASE_B`] and quoted in every failure message, so a
    // later regression is legible as a distance travelled BACKWARD rather
    // than as a bare number nobody can place. The assertions have not been
    // touched — the bounds are the ones the failing versions carried, and
    // the spectral kernel meets them with room to spare.
    //
    // What they measure between them is one claim from two ends: the first
    // reads the COEFFICIENTS the kernel is applying, the second reads the
    // AUDIO that came out. Neither is enough alone. And neither is enough
    // even together — both feed the unit nothing but noise, where a flat
    // fader and an honest denoiser give the same answer — which is why
    // `the_spectral_kernel_improves_the_signal_to_noise_and_not_just_the_level`
    // sits above them and is the test to read first if this unit is ever
    // suspected of having become a volume knob.

    /// The four REDUCE settings both target tests span the dial with: a
    /// quarter, half, three quarters and all of it, so 6, 12, 18 and 24 dB
    /// of promised reduction. The default sits at 0.5.
    const REDUCE_SPAN: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

    /// What [`noise_only_gain_is_flat_and_equals_the_dial`] measured on
    /// THE BANK on 2026-09-02, one row per [`REDUCE_SPAN`] entry: (dialled
    /// reduction, mean cut, peak-to-peak spread) in dB across 100 Hz to
    /// 16 kHz. The target is a mean equal to the dial and a spread of 0.00.
    /// Quoted in the failure text so a regression reads as a distance
    /// travelled rather than as a bare number.
    ///
    /// The middle column is never the first. At the shipped default the
    /// bank cut 18.06 dB on average when it was asked for 12, and the cut
    /// ranged over 15 dB from one probe frequency to the next. The spectral
    /// kernel returns the dial to two decimal places with a spread of 0.00.
    const BASE_A: [(f32, f32, f32); 4] = [
        (6.0, 8.63, 6.67),
        (12.0, 18.06, 15.03),
        (18.0, 26.91, 27.63),
        (24.0, 29.37, 32.52),
    ];

    /// What [`the_dial_tells_the_truth_in_every_octave`] measured on THE
    /// BANK on 2026-09-02, one row per [`REDUCE_SPAN`] entry: the drop in
    /// dB broadband, below 500 Hz, across 500 Hz to 4 kHz, and above
    /// 8 kHz. The target is the dial in all four columns.
    ///
    /// Read the last column down. It is 5.53, 9.83, 10.05, 10.06 — above
    /// 8 kHz the bank saturated at about 10 dB and then stopped responding
    /// to the dial ENTIRELY: turning REDUCE from 18 dB to 24 dB bought one
    /// hundredth of a decibel up there. That is the owner's "I still hear
    /// white noise when I push it hard", written down. The same column on
    /// the spectral kernel is 6.00, 12.00, 18.00, 24.00.
    const BASE_B: [[f32; 4]; 4] = [
        [6.32, 8.49, 8.95, 5.53],
        [11.24, 16.71, 18.70, 9.83],
        [11.69, 22.58, 27.89, 10.05],
        [11.71, 24.31, 29.27, 10.06],
    ];

    /// **The dial is a promise about the SPECTRUM, and it is kept.** With
    /// nothing but learned noise going in, every band should close, and the
    /// composite gain the kernel is applying should be a flat shelf of
    /// exactly the dialled reduction from 100 Hz to 16 kHz.
    ///
    /// The bank could not do this and no tuning would have let it. Its
    /// sections are peaking cuts at Q 5.5 on a 0.26-octave grid, so their
    /// skirts overlap and a probe frequency in the dense middle receives
    /// their SUM while the top octave is left to a lone shelf: measured, a
    /// 12 dB ask landed at −21.2 dB at 100 Hz, −18.6 at 1 kHz, −15.1 at
    /// 8 kHz and −7.9 at 16 kHz, and the error GREW with the dial because
    /// every overlapping cut deepened together — the same measurement at
    /// 24 dB read −50.0 against −17.4. [`BASE_A`] is that row of numbers,
    /// quoted in the failure text so the distance travelled stays legible.
    ///
    /// The spectral kernel keeps the promise by construction, and it is
    /// worth being precise about which construction, because it is two
    /// facts and not one:
    ///
    /// * The band weights are a PARTITION of every bin (pinned separately
    ///   in `the_band_weights_partition_every_bin_exactly`), so a bank of
    ///   equal band gains reconstructs to exactly that gain at every bin
    ///   rather than to a sum of skirts.
    /// * In a noise-only frame every band's decision-directed ξ has
    ///   bottomed out at [`XI_MIN_DB`], which puts the speech-present gain
    ///   at −40.1 dB — deeper than the deepest the dial can ask for — so
    ///   the mixture `p·g_H1 + (1−p)·C` lands at or under `C` whatever `p`
    ///   is, and the REDUCE cap clamps it exactly onto `C`. The bands come
    ///   out EQUAL, which is the precondition the first fact needs.
    ///
    /// Read like the bank version: the composite is measured from the
    /// coefficients the kernel is actually applying, and
    /// [`analysis::gain_trajectory`] is read FIRST so the assertion message
    /// can report the bank's drift over the last second and rule out "it
    /// had not settled yet" from the failure text alone. The last column is
    /// the two-routes guard — the same cut measured from the AUDIO, by
    /// comparing the dry and wet power spectra in a narrow slice around the
    /// probe — because a measurement of a gain array by reading the gain
    /// array proves nothing on its own.
    #[test]
    fn noise_only_gain_is_flat_and_equals_the_dial() {
        // 100 Hz to 16 kHz, log-spaced: the range the unit claims to
        // cover, sampled finely enough to catch a dip between two bands.
        let probes: Vec<f32> = (0..96)
            .map(|i| 100.0 * (16_000.0_f32 / 100.0).powf(i as f32 / 95.0))
            .collect();

        let mut rows = Vec::new();
        for &reduce in REDUCE_SPAN.iter() {
            let cap = reduce * DENOISE_REDUCE_DB;
            let kernel = DenoiseKernel::SpectralStudio;
            let mut dn = learned_full_band(kernel, 0xD1A1, 0.05, SR, 1.0);
            let p = DenoiseParams {
                reduce,
                ..on(kernel)
            };
            // Six seconds of the same noise: long against every time
            // constant in the detector, so the kernel has arrived.
            let frames = 6 * SR as usize;
            let dry = FullBandNoise::new(0xD1A1, 0.05).frames(frames);
            let mut buf = dry.clone();
            let traj = analysis::gain_trajectory(&mut dn, &mut buf, frames, 512, &p);
            // DRIFT, not jitter. Each band's mean depth over the last
            // tenth of the run against its mean over the tenth before
            // that: a kernel still travelling shows up here, whereas the
            // sample-to-sample wobble of a depth riding a noise envelope
            // averages out — and that wobble is real and never reaches
            // zero, so comparing two adjacent snapshots would answer the
            // wrong question.
            let tenth = (traj.len() / 10).max(1);
            let mean_over = |seg: &[[f32; DENOISE_BANDS]], k: usize| {
                seg.iter().map(|s| s[k]).sum::<f32>() / seg.len() as f32
            };
            let last = &traj[traj.len() - tenth..];
            let prev = &traj[traj.len() - 2 * tenth..traj.len() - tenth];
            let drift = (0..DENOISE_BANDS).fold(0.0_f32, |m, k| {
                m.max((mean_over(last, k) - mean_over(prev, k)).abs())
            });
            // And the SAME trajectory read frame by frame, because a drift
            // between two means is blind to an excursion and the
            // composite below reads one frame only — the LAST. Band 0 was
            // 13.02 dB peak-to-peak here while every number on this line
            // read 0.00; see
            // `the_gain_is_flat_and_equal_to_the_dial_in_every_frame`,
            // which carries the full matrix and both frame lengths.
            let settled = &traj[traj.len() / 2..];
            let jitter = (0..DENOISE_BANDS).fold(0.0_f32, |m, k| {
                let lo = settled.iter().map(|s| s[k]).fold(f32::INFINITY, f32::min);
                let hi = settled.iter().map(|s| s[k]).fold(f32::NEG_INFINITY, f32::max);
                m.max(hi - lo)
            });

            let cut: Vec<f32> = analysis::spectral_response_db(&dn, &probes, SR)
                .iter()
                .map(|g| -g)
                .collect();
            let lo = cut.iter().copied().fold(f32::INFINITY, f32::min);
            let hi = cut.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let mean = cut.iter().sum::<f32>() / cut.len() as f32;

            // The tools check each other. What the coefficients say against
            // what the AUDIO did, at one probe low in the grid and one in
            // the top octave where a disagreement would matter most. If
            // this column is not near zero the argument above it is about a
            // measurement bug, not about the kernel.
            let (from, len) = (frames / 2, frames / 2);
            let psd_dry = analysis::power_spectrum(&dry, from, len);
            let psd_wet = analysis::power_spectrum(&buf, from, len);
            let check = [1_000.0_f32, 14_000.0].iter().fold(0.0_f32, |m, &hz| {
                let by_coef = -analysis::spectral_response_db(&dn, &[hz], SR)[0];
                let by_audio = analysis::psd_cut_at(&psd_dry, &psd_wet, SR, hz);
                m.max((by_coef - by_audio).abs())
            });
            rows.push((reduce, cap, mean, lo, hi, hi - lo, drift, check, jitter));
        }

        let table = rows
            .iter()
            .zip(BASE_A.iter())
            .map(|(r, c)| {
                format!(
                    "  reduce {:.2} (dial {:>4.1} dB): mean cut {:>6.2} dB, \
                     {:>6.2}..{:>6.2}, peak-to-peak {:>5.2} dB \
                     (drift over the last second {:.3} dB, worst per-band \
                     peak-to-peak frame by frame {:.3} dB, coefficients vs \
                     audio {:.3} dB) [bank: mean {:.2}, p-p {:.2}]",
                    r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.8, r.7, c.1, c.2
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Printed as well as asserted: `--nocapture` gives the table this
        // unit's later commits get judged against, without anyone having
        // to make a test fail on purpose to read it.
        println!("noise-only composite gain, Spectral Studio:\n{table}");
        assert!(
            rows.iter().all(|r| r.7 < 1.0),
            "the coefficients and the audio disagree, so nothing below this \
             line means anything:\n{table}"
        );
        assert!(
            rows.iter().all(|r| r.5 < 1.0),
            "the settled composite gain is not flat across 100 Hz..16 kHz \
             (peak-to-peak must be under 1.0 dB):\n{table}"
        );
        assert!(
            rows.iter().all(|r| (r.2 - r.1).abs() < 1.0),
            "the settled composite gain is not the dialled reduction \
             (the mean must be within 1.0 dB of the dial):\n{table}"
        );
        assert!(
            rows.iter().all(|r| r.8 < 1.0),
            "the gain is flat in the LAST frame and not in the others (a band's cut must \
             stay within 1.0 dB of itself frame by frame over the settled half):\n{table}"
        );
    }

    /// **The dial tells the truth in every octave, not on average.** The
    /// measurement a listener would make: learned noise in, how much energy
    /// actually left — broadband, and separately below 500 Hz, across
    /// 500 Hz to 4 kHz, and above 8 kHz. Each lands within 1.5 dB of the
    /// dial.
    ///
    /// This is the same claim as
    /// [`noise_only_gain_is_flat_and_equals_the_dial`] seen from the other
    /// end. That test reads the coefficients; this one reads the audio, so
    /// it also catches anything the dynamics do that a settled response
    /// cannot show — bands that never close, a floor the estimator cannot
    /// reach, a profile that learned nothing up top. Two independent routes
    /// to the same number is what makes the fix believable.
    ///
    /// [`BASE_B`] is what the bank measured. Read its last column down:
    /// 5.53, 9.83, 10.05, 10.06 — above 8 kHz the bank saturated at about
    /// 10 dB and then stopped responding to the dial ENTIRELY, so turning
    /// REDUCE from 18 dB to 24 dB bought one hundredth of a decibel up
    /// there. That was the owner's "I still hear white noise when I push it
    /// hard", written down, and it is the column to check first if this
    /// test ever goes red again.
    ///
    /// Both spectral frame lengths, because Live is not Studio with a
    /// smaller number in it: half the frame is half the frequency
    /// resolution, its bands sit on the 2-bin floor further up the
    /// spectrum, and the whole flatness argument has to survive that.
    #[test]
    fn the_dial_tells_the_truth_in_every_octave() {
        let slices: [(&str, f32, f32); 4] = [
            ("broadband", 20.0, SR / 2.0),
            ("below 500 Hz", 20.0, 500.0),
            ("500 Hz-4 kHz", 500.0, 4_000.0),
            ("above 8 kHz", 8_000.0, SR / 2.0),
        ];

        for kernel in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
            let mut rows = Vec::new();
            for &reduce in REDUCE_SPAN.iter() {
                let cap = reduce * DENOISE_REDUCE_DB;
                let mut dn = learned_full_band(kernel, 0x0C7A, 0.05, SR, 1.0);
                let p = DenoiseParams {
                    reduce,
                    ..on(kernel)
                };
                let frames = 2 * SR as usize;
                let dry = FullBandNoise::new(0x0C7A, 0.05).frames(frames);
                let mut wet = dry.clone();
                dn.process(&mut wet, frames, 2, &p);

                // The second half only: the first second is the kernel
                // arriving, and the frame of latency lives at the front.
                let (from, len) = (frames / 2, frames / 2);
                let psd_dry = analysis::power_spectrum(&dry, from, len);
                let psd_wet = analysis::power_spectrum(&wet, from, len);
                let drops: Vec<f32> = slices
                    .iter()
                    .map(|&(_, lo, hi)| {
                        analysis::band_power_db(&psd_dry, SR, lo, hi)
                            - analysis::band_power_db(&psd_wet, SR, lo, hi)
                    })
                    .collect();
                rows.push((reduce, cap, drops));
            }

            let table = rows
                .iter()
                .zip(BASE_B.iter())
                .map(|(r, base)| {
                    let (reduce, cap, drops) = r;
                    let live = drops
                        .iter()
                        .zip(slices.iter())
                        .map(|(d, s)| format!("{} {:>6.2}", s.0, d))
                        .collect::<Vec<_>>()
                        .join("   ");
                    let was = base
                        .iter()
                        .map(|d| format!("{d:.2}"))
                        .collect::<Vec<_>>()
                        .join("/");
                    format!("  reduce {reduce:.2} (dial {cap:>4.1} dB): {live}  [bank: {was}]")
                })
                .collect::<Vec<_>>()
                .join("\n");

            println!("energy actually removed, {}:\n{table}", kernel.label());
            assert!(
                rows.iter()
                    .all(|r| r.2.iter().all(|d| (d - r.1).abs() <= 1.5)),
                "{}: the reduction the audio actually received is not the \
                 dialled reduction in every slice (each must be within \
                 1.5 dB of the dial):\n{table}",
                kernel.label()
            );
        }
    }
}

