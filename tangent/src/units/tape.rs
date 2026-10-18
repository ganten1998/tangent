//! A record/repro tape path: a quarter-inch two-track at four speeds.
//!
//! The machine is a servo-capstan studio two-track with 2 mm tracks on a
//! high-output mastering tape, NAB equalised, its bias at the recommended
//! over-bias, its heads new and its transport inside its published spec. It
//! runs at 3.75, 7.5, 15 and 30 inches per second, and each speed is its own
//! machine ([`TapeSpeed`]): the head bump sits an octave higher with every
//! doubling, slower tape saturates sooner and loses its treble sooner, and
//! each speed's top end stops where studio machines are specified. A fresh
//! tape opens at 15 ips, lined up at the tape's datasheet level.
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
//! all. That is why a snare hit hard onto tape sounds rounded rather than turned
//! down, and it is a thing no gain element — no compressor — can do, because a
//! gain element takes the same decibels from every band. Here it is a
//! dual-rate envelope follower on the record level driving a one-pole corner
//! downward, and the test proves the band ratio a gain could not produce.
//! The loss is a short-wavelength one, so it lives in wavelength: the same
//! record level costs a slow tape far more of its top than a fast one, and
//! the corner and its floor move with the speed ([`ERASE_TOP_HZ`]). At rest
//! the stage is an exact wire at every speed.
//!
//! # Signal flow
//!
//! The input transformer (a one-pole high-pass at [`TRANSFORMER_HZ`]) → the
//! NAB record shelf → record, at four times the session rate: a
//! bias-linearised tanh with the refused flux fed back → the repro line-up →
//! the NAB shelf's exact inverse → HF self-erasure (the envelope-driven
//! corner above) → the tape: a short Hermite-interpolated play head swayed by
//! wow and flutter whose depths are themselves noise-modulated, plus a
//! whisper of scrape → the head's own top → the hiss, level-tracked → the
//! head contour: the bump, its dip an octave above, and the head's low-end
//! roll-off → Roll → output trim. The `drive` knob is the record level, as
//! the speed's own headroom scales it, and the repro trim that follows it is
//! the machine's line-up, not a knob; the only trim a person turns is on the
//! way out.
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
//! the real latency are both [`BASE_MS`]. That was three quarters of a
//! sample untrue until the decimators kept the samples [`OS_LATENCY`] names;
//! now an impulse leaves on the very frame [`Tape::latency_frames`] reports,
//! at every rate, and `an_impulse_leaves_exactly_when_the_host_is_told_it_will`
//! holds it there.
//!
//! # Params
//!
//! Normalised 0..=1, like everything in `effects.rs`; the mapping to whatever
//! the DSP actually wants lives in the consts next to each stage. Design math
//! runs in f64, audio in f32, and every voicing frequency is written in hertz
//! so the unit is the same machine at every sample rate.

// ── what a person can change ────────────────────────────────────────────────

/// The controls, in id order: the six 5.0 shipped, then the ones appended
/// after them. Defaults are a fresh machine: lined up, healthy, at the studio
/// speed, not a bypass and not a caricature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TapeParams {
    /// Record level into the saturation, and with it how hard the
    /// self-erasure leans on attacks. **0 is the wire**: a record level's
    /// "off" is the bottom of its travel, not the middle of it. A fresh tape
    /// opens at 0.5, the datasheet alignment, where at 15 ips the 3 %
    /// distortion point sits about 10 dB over 0 VU. See [`DRIVE_TANH`].
    pub drive: f32,
    /// How much head bump. 0 is flat; see [`BUMP_DB`].
    pub bump: f32,
    /// How much high end a worn head gives up, on top of the speed's own top.
    /// 0 is out of circuit, and a fresh tape opens there; see [`ROLL_HZ`].
    pub roll: f32,
    /// Wow and flutter together, one master. 0 parks the play head; see
    /// [`WOW_DEPTH_MS`].
    pub wobble: f32,
    /// Tape hiss. 0 is silent tape, which no real reel ever was; see
    /// [`HISS_MAX`].
    pub hiss: f32,
    /// Output trim. 0.5 is unity; see [`TRIM_DB`].
    pub trim: f32,
    /// The tape speed, a selector of four ([`TapeSpeed`]). A fresh tape runs
    /// at 15 ips.
    pub speed: f32,
}

impl Default for TapeParams {
    fn default() -> Self {
        Self {
            // The high-output tape's datasheet alignment (the owner's
            // ruling): at 15 ips the 3 % distortion point sits about 10 dB
            // over 0 VU and a 0 VU tone carries a quarter of a percent of
            // third harmonic, so programme peaking ten over the needle meets
            // the shoulder while the body stays clean.
            drive: 0.5,
            // +2.5 dB on the contour's bell, which at 15 ips sits near 35 Hz.
            bump: 0.5,
            // Out of circuit: each speed has its own top end now, and Roll
            // is what a worn head takes on top of it.
            roll: 0.0,
            // Present on sustained piano, invisible on speech.
            wobble: 0.3,
            // Audible in the gaps if you listen for it, like the real thing.
            hiss: 0.25,
            trim: 0.5,
            // The studio speed.
            speed: TapeSpeed::Ips15.dial(),
        }
    }
}

#[cfg(test)]
impl TapeParams {
    /// Every colour off: the record level at the bottom of its travel, no
    /// bump, no roll, a parked head, silent tape, unity trim, at the studio
    /// speed. The recipe the neutrality tests here and at the seat spell
    /// out, in one place.
    pub(crate) const NEUTRAL: TapeParams = TapeParams {
        drive: 0.0,
        bump: 0.0,
        roll: 0.0,
        wobble: 0.0,
        hiss: 0.0,
        trim: 0.5,
        speed: TapeSpeed::Ips15.dial(),
    };
}

/// **The tape speed**, a selector of four, in inches per second.
///
/// Each speed is its own machine. Everything the tape does in length is the
/// same at every speed (the head bump's wavelength, self-erasure's shortest
/// wavelengths, the head's gap), so in hertz it moves an octave with every
/// doubling; the curve takes more level at a faster speed ([`DMOL_DB`]); and
/// each speed has its own top end ([`HEAD_TOP_EDGE_HZ`]). A change of speed
/// glides the voicing over [`SPEED_GLIDE_MS`] and never moves the track in
/// time or bends its pitch: record and playback run at the same speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapeSpeed {
    Ips3_75,
    Ips7_5,
    Ips15,
    Ips30,
}

/// How many positions the speed selector has. A literal so `builtin_fx`'s
/// table can stay `const`; a test there pins it to [`TapeSpeed::ALL`].
pub const TAPE_SPEED_STEPS: i32 = 4;

impl TapeSpeed {
    /// Selector order, and therefore the order every persisted value and the
    /// face's cycling row use. Frozen at four: append-only, like the ids.
    pub const ALL: [TapeSpeed; 4] = [
        TapeSpeed::Ips3_75,
        TapeSpeed::Ips7_5,
        TapeSpeed::Ips15,
        TapeSpeed::Ips30,
    ];

    /// What the panel calls it.
    pub fn label(self) -> &'static str {
        match self {
            TapeSpeed::Ips3_75 => "3.75 ips",
            TapeSpeed::Ips7_5 => "7.5 ips",
            TapeSpeed::Ips15 => "15 ips",
            TapeSpeed::Ips30 => "30 ips",
        }
    }

    /// The speed in inches per second.
    pub fn ips(self) -> f64 {
        match self {
            TapeSpeed::Ips3_75 => 3.75,
            TapeSpeed::Ips7_5 => 7.5,
            TapeSpeed::Ips15 => 15.0,
            TapeSpeed::Ips30 => 30.0,
        }
    }

    /// Decode the normalised dial: four positions across the travel, nearest
    /// wins, the convention every selector in the suite uses.
    pub fn from_dial(v: f32) -> Self {
        let last = (Self::ALL.len() - 1) as f32;
        let at = ((v.clamp(0.0, 1.0) * last).round() as usize).min(Self::ALL.len() - 1);
        Self::ALL[at]
    }

    /// Where this speed sits on the 0..=1 dial: the inverse of
    /// [`TapeSpeed::from_dial`], so a default can be written as a speed.
    pub const fn dial(self) -> f32 {
        self as usize as f32 / (TapeSpeed::ALL.len() - 1) as f32
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
/// [`sat_makeup`]'s own note, pinned by a test at a fresh tape's Drive. The
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
/// said. The top reaches the saturation the unit is named after at the level
/// people actually record at: on the 7.5 ips machine 5.0 shipped, the same
/// sweep ran to 17 % and tilted the band by 8 dB, its 0.6 rounded a -6 dBFS
/// peak by several percent and pulled its top to around 4 kHz while leaving
/// the body alone, and full travel was a machine being slammed.
///
/// # What a saved Drive means now, and why nothing migrates it
///
/// The dial persists as its raw 0..=1 position under a state header that has
/// not changed, and this range is the one 5.0 shipped, so a saved Drive still
/// names the same `d`. What moved under it is the machine: `d` is the record
/// level at 7.5 ips, and each speed scales it by its own headroom
/// ([`DMOL_DB`]), so a session from 5.0 or 5.1, which opens at 15 ips, records
/// 2.3 dB cooler at the same dial, and 7.5 ips gives it back its old curve.
/// By the owner's ruling that is stated in the changelog rather than
/// migrated: an old session meets the new machine at its saved dials. Its
/// Drive and Roll keep their saved values, too, because a state blob writes
/// every symbol; only a fresh tape opens at the new defaults.
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
/// The designer ([`windowed_sinc`]) is restated below rather than imported,
/// like [`DESIGN_RATE`], [`flush`] and [`TINY`] at the top of this file: the
/// kernel imports nothing from another unit, so no edit to a pedal's file
/// can move the tape's sound. A second oversampler is a second place for an
/// alias to hide, which is why this one has its own fold test
/// (`full_drive_on_a_high_note_does_not_fold_the_band_back_on_itself`) and
/// its own proof that it is the drive's direct form, sample for sample
/// (`the_oversampler_is_the_direct_form_pair_at_less_than_half_the_arithmetic`).
/// Two numbers are deliberately NOT the drive's:
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
///
/// **The pair runs polyphase, which is less than half the arithmetic.** Half
/// of what each interpolator reads is the zeros it stuffs in, half of what
/// each decimator computes is thrown away, and every second off-centre tap of
/// the halfband is zero. The direct form multiplied all of it: 484
/// multiply-adds per channel-sample, the bulk of what the unit cost. Split
/// into the phases each filter actually reads ([`OsTaps`],
/// [`OsChannel`]), it is 200, or 226 with the padding that keeps every dot
/// product on the vector path, on the Analog's precedent in
/// `analog/filter.rs`. Every multiply-add goes through [`fma`], which is
/// never a function call; see there for why the drive's own loop is not
/// borrowed for it.
const OS: usize = 4;
const OS_A_TAPS: usize = 79;
const OS_B_TAPS: usize = 21;

/// The tap counts each dot product of the polyphase pair runs, padded with
/// zeros to a multiple of eight so it stays on the vector path. The 2×
/// interpolator reads the even taps for the sample that lands on a session
/// sample and the odd ones for the one between; the 2× decimator reads all
/// of them, but only for the output it keeps; the 4× stages read the
/// halfband's odd taps, because its even ones are the centre and zeros.
/// A padded tap multiplies a real history sample by zero.
const OS_A_EVEN: usize = pad8(OS_A_TAPS.div_ceil(2));
const OS_A_ODD: usize = pad8(OS_A_TAPS / 2);
const OS_A_FULL: usize = pad8(OS_A_TAPS);
const OS_B_ODD: usize = pad8(OS_B_TAPS / 2);

/// Where the halfband's centre tap reads in a history that holds every
/// second 4× sample: the centre is tap `(21 - 1) / 2 = 10`, and ten 4×
/// samples back on one phase's stream is five entries back.
const OS_B_HALF: usize = (OS_B_TAPS - 1) / 4;

/// `n` rounded up to a multiple of eight.
const fn pad8(n: usize) -> usize {
    n.div_ceil(8) * 8
}

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
///
/// **Whole only if the decimators keep the right samples.** Each keeps one
/// output in two, and the later of a pair has seen one more input: a quarter
/// of a session sample further along at 4×, a half at 2×. The pair used to
/// keep the later of both, so an impulse peaked on 43 with its phase delay
/// at 43.25 across the band, three quarters of a sample ahead of the 44 the
/// head gives up. Keeping the earlier of both (sub-phase `q == 0` at 4×,
/// `h == 0` at 2×, in [`OsChannel::down`]) lands it on 44 exactly and
/// symmetric about its peak; the filters, and so the magnitude and the
/// aliasing, are the same ones.
const OS_LATENCY: usize = (OS_A_TAPS - 1) / 2 + (OS_B_TAPS - 1) / 4;

/// The line-up level the repro trim is calibrated at, in dBFS.
///
/// **0 VU, which this app puts at −18 dBFS.** A machine is lined up by
/// recording a tone at 0 VU and setting the repro amplifier so it comes back
/// at 0 VU; [`sat_makeup`] does exactly that arithmetic, so the alignment
/// level is the one place on the dial where the record level is guaranteed
/// not to be a volume control.
const ALIGN_DBFS: f64 = -18.0;

/// The input transformer's bottom, as a one-pole high-pass corner in hertz.
///
/// It keeps DC and subsonics out of the saturation, where they would only
/// spend headroom. The low end a person hears is the head's, which moves
/// with the speed ([`BUMP_WAVELENGTH_MM`]): studio two-tracks are within
/// 2 dB down to 20 Hz at 15 ips, so the transformer sits well under that.
const TRANSFORMER_HZ: f64 = 10.0;

/// The NAB record shelf: its zero in hertz and its low-frequency lift at
/// each speed in dB, slowest first, undone exactly after the tape.
///
/// NAB's 3180 µs record boost, capped at the 3 to 4 dB shelf that practice
/// recommends, is how the low end goes onto the tape: the long wavelengths
/// reach the curve that much hotter, so a bass note saturates sooner than a
/// midrange one at the same level, and the playback side takes the lift back
/// off with the shelf's exact inverse. 30 ips is AES-equalised, which has no
/// such term, so its shelf is a wire. Both run at the session rate; they are
/// low-frequency stages, so the alias guarantee is untouched.
const NAB_ZERO_HZ: f64 = 62.9;
const NAB_SHELF_DB: [f64; 4] = [4.0, 4.0, 4.0, 0.0];

/// How much more level each speed takes before it saturates, relative to
/// 7.5 ips, in dB, slowest first.
///
/// The curve is the 7.5 ips machine's, and the dial's `d` is that speed's
/// record level. A faster tape writes the same flux over a longer wavelength
/// and takes more of it before its 3 % distortion point: the datasheets put
/// 15 ips 2.0 to 2.5 dB over 7.5, and 30 ips a little over that. So each
/// speed divides `d` by its figure; 3.75 ips is the step to 15 mirrored,
/// which no datasheet quotes. **No cap:** the dial's top at 3.75 ips asks the
/// curve for `d` = 26.1, and the alias guarantee is stated for exactly that
/// (`full_drive_on_a_high_note_does_not_fold_the_band_back_on_itself`), so
/// Drive has no dead travel at any speed.
const DMOL_DB: [f64; 4] = [-2.3, 0.0, 2.3, 2.7];

/// Where the self-erasure corner falls to at each speed, in hertz, slowest
/// first, and how fast it falls with the record level.
///
/// The corner is `TOP / (2^(k e) - 1)` for the record-level envelope `e`,
/// clamped at [`ERASE_CLAMP`], with `k` octaves of [`ERASE_OCTAVES`]: at rest
/// it is infinitely high and the stage an exact wire at every speed, and as
/// the level rises it falls, steeply at a slow speed and hardly at all at a
/// fast one. Fitted to where the datasheets put the 10 kHz compression point
/// against 1 kHz's: 7.5 to 9.5 dB lower at 7.5 ips, 1.5 to 2 at 15 and 1 to
/// 1.5 at 30 (`slower_tape_saturates_its_treble_first_as_the_datasheets_say`).
const ERASE_TOP_HZ: [f64; 4] = [10_000.0, 16_000.0, 100_000.0, 200_000.0];
const ERASE_OCTAVES: f64 = 3.0;
const ERASE_CLAMP: f64 = 1.4;

/// The longest wavelength self-erasure may reach, in micrometres: its corner
/// never falls below the frequency that is this long on the tape.
///
/// Self-erasure is a short-wavelength loss, and its corner may not reach the
/// long wavelengths the line-up tone and the music's body live at. 76.2 µm
/// is 2.5 kHz at 7.5 ips, the floor 5.0 shipped, made a wavelength: 1.25,
/// 2.5, 5 and 10 kHz at the four speeds. It never binds where the 10 kHz
/// compression point is measured.
const ERASE_FLOOR_UM: f64 = 76.2;

/// How many points each speed's self-erasure table holds, evenly spaced in
/// the envelope from 0 to [`ERASE_CLAMP`]. [`Tape::new`] designs the corner's
/// one-pole at every point, so the audio thread only ever interpolates.
const ERASE_POINTS: usize = 129;

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
///
/// The depth scales the band noise as [`DESIGN_RATE`] draws it, and
/// `process` scales the noise so every other rate draws the same sway per
/// hertz: at the shipped wobble, about 0.026 µs RMS of head movement.
const SCRAPE_MS: f32 = 0.000_5;
const SCRAPE_BAND_HZ: (f32, f32) = (800.0, 3_000.0);

/// The head's own top at each speed, slowest first: the band edge, in hertz,
/// where a studio two-track's response is specified 2 dB down.
///
/// The healthy residual spacing, the gap loss and the electronics, which a
/// real machine is calibrated to at its nominal speed, as one matched
/// one-pole per speed with its corner where it is 2 dB down at the edge the
/// plus-or-minus-2-dB specifications quote: 10, 15.5, 21 and 25 kHz, which
/// are corners of 13.1, 20.3, 27.5 and 32.7 kHz.
const HEAD_TOP_EDGE_HZ: [f64; 4] = [10_000.0, 15_500.0, 21_000.0, 25_000.0];

/// The head contour: the bump's wavelength and Q, the dip an octave above
/// it, and the head's low-end roll-off.
///
/// A play head is a magnetic circuit with a physical length, and at the
/// wavelength that matches it the response lifts: about 10.9 mm on studio
/// heads, measured at two speeds, so the bump is a wavelength and its
/// frequency is the speed over it, 8.7, 17.5, 35 and 70 Hz at the four
/// speeds. An octave above it the response dips by about half the lift
/// ([`DIP_SHARE`], between theory's 0.6 and one measured machine), and below
/// it the head's response falls away, here a one-pole high-pass at
/// [`HEAD_LF_SHARE`] of the bump frequency and never under
/// [`HEAD_LF_FLOOR_HZ`]. The bump's size is [`BUMP_DB`] across its dial.
const BUMP_WAVELENGTH_MM: f64 = 10.9;
const BUMP_Q: f64 = 1.1;
const DIP_Q: f64 = 1.4;
const DIP_SHARE: f64 = -0.5;
const HEAD_LF_SHARE: f64 = 0.4;
const HEAD_LF_FLOOR_HZ: f64 = 4.0;
pub const BUMP_DB: (f32, f32) = (0.0, 5.0);

/// The repro HF roll, as a one-pole corner swept geometrically.
///
/// What a worn head and its spacing take from the top, on top of the speed's
/// own top ([`HEAD_TOP_EDGE_HZ`]). Zero is out of circuit, and a fresh tape
/// opens there; full travel is a worn head that never saw a cotton swab.
pub const ROLL_HZ: (f32, f32) = (20_000.0, 2_500.0);

/// How long a speed change takes, as the time constant of the glide every
/// per-speed constant follows, in milliseconds.
///
/// A servo transport re-locks to a new speed in about a third of a second,
/// and a 100 ms one-pole is 95 % of the way there in 0.3 s. Record and
/// playback move together, so the change re-voices the machine without
/// bending the pitch or moving the latency.
const SPEED_GLIDE_MS: f64 = 100.0;

/// How often the voicing is designed again while something glides, in
/// frames. Counted from `new` or `clear` across every block, so a render
/// redesigns on the same frames at any block size.
const SUB_BLOCK: u64 = 32;

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
/// stops, but never to nothing: a stopped machine still has tape against
/// the head. Squared knob taper, because ears meet noise in dB.
///
/// The level is the white source's scale at [`DESIGN_RATE`]; `process`
/// rescales it so every other rate puts the same hiss in every hertz.
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

/// [`norm`] for a control appended after 5.0: a NaN reads as that control's
/// own default rather than the midpoint, because for a selector the midpoint
/// is a real position (15 ips is 2/3 of the speed dial) or worse, a
/// rounding that picks one the dial never meant.
#[inline]
fn norm_or(x: f32, default: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        default
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

/// How long a moving control takes to arrive, as the time constant of the
/// one-pole every [`Glide`] runs, in milliseconds: the house smoothing.
const GLIDE_MS: f64 = 20.0;

/// A one-pole glide that lands: within `snap` of its target it IS the
/// target, so a settled control is an exact number and never a denormal
/// tail. `units::bitcrush`'s, restated for the reason [`OS`] gives.
///
/// **A level glides as a linear gain, never in decibels.** The bottom of a
/// level's travel is silence, which is minus infinity in decibels, and a
/// glide leaving it computes `-inf + a (x + inf)`: not a number, which then
/// never lands, because nothing compares equal to it. A level glided as a
/// gain arrives at an exact zero instead, and the stage it feeds can be
/// skipped there.
#[derive(Clone, Copy, Debug)]
struct Glide {
    cur: f64,
    target: f64,
    snap: f64,
}

impl Glide {
    const fn new(snap: f64) -> Self {
        Self {
            cur: 0.0,
            target: 0.0,
            snap,
        }
    }

    /// One frame of the glide; `a` is the one-pole's per-frame step.
    #[inline]
    fn tick(&mut self, a: f64) -> f64 {
        if self.cur != self.target {
            let next = self.cur + a * (self.target - self.cur);
            self.cur = if (self.target - next).abs() <= self.snap {
                self.target
            } else {
                next
            };
        }
        self.cur
    }

    /// Arrive at once.
    fn settle(&mut self) {
        self.cur = self.target;
    }
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

/// A multiply-add: fused where the machine fuses, a multiply and an add
/// where it does not. Every tap of the record stage's pair goes through it.
///
/// **`f32::mul_add` is an instruction on one target and a call on the
/// rest.** It is the `llvm.fma` intrinsic: one `fmadd` on arm64, but on the
/// x86-64 baseline the Windows, Linux and FreeBSD builds use (no FMA, so
/// every processor can run them) LLVM cannot inline it, and each one becomes
/// a CALL into compiler-builtins' `fmaf`, one per tap. The drive's dot
/// product calls the intrinsic, which is why this pair does not borrow it;
/// the Analog measured what that costs a 4× pair and keeps this shim for it
/// (`analog/filter.rs`), restated here because it is private there. A
/// multiply and an add vectorise on SSE2 and round differently from the
/// fused form in the last bit, so a render is the same bits on one machine,
/// which is all the house ever promises of floats.
#[inline(always)]
fn fma(a: f32, b: f32, c: f32) -> f32 {
    #[cfg(any(target_arch = "aarch64", target_feature = "fma"))]
    {
        a.mul_add(b, c)
    }
    #[cfg(not(any(target_arch = "aarch64", target_feature = "fma")))]
    {
        a * b + c
    }
}

/// A dot product the compiler will vectorise: eight accumulators, tap `t`
/// into accumulator `t % 8`, each an explicit [`fma`], and a tree at the end
/// so the compiler does not rebuild one long chain of adds. Eight because
/// the pair's cost is latency: eight independent chains of `N / 8` each.
/// `b` must be at least `N` long and `N` a multiple of eight, which the
/// padding in [`OsTaps`] gives and the compiler checks.
#[inline(always)]
fn dot<const N: usize>(a: &[f32; N], b: &[f32]) -> f32 {
    const { assert!(N % 8 == 0, "a dot product runs whole chunks of eight") };
    let (ac, _) = a.as_chunks::<8>();
    let (bc, _) = b[..N].as_chunks::<8>();
    let mut acc = [0.0_f32; 8];
    for (x, y) in ac.iter().zip(bc) {
        for k in 0..8 {
            acc[k] = fma(x[k], y[k], acc[k]);
        }
    }
    ((acc[0] + acc[1]) + (acc[2] + acc[3])) + ((acc[4] + acc[5]) + (acc[6] + acc[7]))
}

/// A history a dot product reads as one contiguous slice, newest first.
/// Every sample is written twice, `H` apart, so the window `z[pos..pos + H]`
/// never wraps; `H2` is twice `H`, spelled out because `2 * H` is not a type
/// on stable Rust. The Analog's `History`, restated for the same privacy
/// reason as [`fma`].
#[derive(Clone, Copy)]
struct History<const H: usize, const H2: usize> {
    z: [f32; H2],
    pos: usize,
}

impl<const H: usize, const H2: usize> History<H, H2> {
    const fn new() -> Self {
        const { assert!(H2 == 2 * H, "a history is written twice") };
        Self { z: [0.0; H2], pos: 0 }
    }

    #[inline]
    fn push(&mut self, x: f32) {
        self.pos = if self.pos == 0 { H - 1 } else { self.pos - 1 };
        self.z[self.pos] = x;
        self.z[self.pos + H] = x;
    }

    /// The newest `H` samples, newest first.
    #[inline]
    fn window(&self) -> &[f32] {
        &self.z[self.pos..self.pos + H]
    }

    fn clear(&mut self) {
        self.z = [0.0; H2];
        self.pos = 0;
    }
}

/// The two designed filters, split into the phases the polyphase pair reads
/// them by: designed once per unit and shared by both channels. See
/// [`OS_A_EVEN`] for which phase reads what.
#[derive(Clone)]
struct OsTaps {
    /// `A[0], A[2], .., A[78]`.
    a_even: [f32; OS_A_EVEN],
    /// `A[1], A[3], .., A[77]`, then zeros.
    a_odd: [f32; OS_A_ODD],
    /// `A[0..79]`, then zeros.
    a_full: [f32; OS_A_FULL],
    /// `B[1], B[3], .., B[19]`, then zeros.
    b_odd: [f32; OS_B_ODD],
    /// `B[10]`, exactly one half: an ideal halfband's centre is `2 · 0.25`
    /// and its window is one there.
    b_centre: f32,
}

impl OsTaps {
    fn new() -> Self {
        let a: [f32; OS_A_TAPS] = windowed_sinc(8.0, OS_A_CUTOFF);
        let b: [f32; OS_B_TAPS] = windowed_sinc(8.0, 0.25);
        let mut t = Self {
            a_even: [0.0; OS_A_EVEN],
            a_odd: [0.0; OS_A_ODD],
            a_full: [0.0; OS_A_FULL],
            b_odd: [0.0; OS_B_ODD],
            b_centre: b[(OS_B_TAPS - 1) / 2],
        };
        for (k, v) in a.iter().enumerate() {
            t.a_full[k] = *v;
            if k % 2 == 0 {
                t.a_even[k / 2] = *v;
            } else {
                t.a_odd[k / 2] = *v;
            }
        }
        // The halfband's even taps are its centre and true zeros (1e-17 in
        // f32, rounding residue of `sin(k·π/2)`), so only the odd ones are
        // kept; the test that proves this form equals the direct one also
        // pins that what is dropped is nothing.
        for (j, v) in b.iter().skip(1).step_by(2).enumerate() {
            t.b_odd[j] = *v;
        }
        t
    }
}

/// One channel's record-stage pair, polyphase: a history per filter phase,
/// and nothing for the zeros an interpolator stuffs in or the outputs a
/// decimator throws away.
#[derive(Clone, Copy)]
struct OsChannel {
    /// `2u`, one push per session sample: both phases of the 2× interpolator
    /// read it, the even taps for the 2× sample on the session sample and
    /// the odd ones for the sample between.
    up_a: History<OS_A_EVEN, { 2 * OS_A_EVEN }>,
    /// `2 s2`, two pushes per session sample, read the same way at 4×: the
    /// centre tap for the sub-phase on a 2× sample, the odd taps between.
    up_b: History<OS_B_ODD, { 2 * OS_B_ODD }>,
    /// The tape's 4× output on the sub-phases the 4× decimator keeps
    /// (`q == 0`): its kept output reads the centre tap here, five entries
    /// back.
    dn_b_kept: History<8, 16>,
    /// The tape's 4× output on the sub-phases between (`q == 1`), which the
    /// kept output reads through the halfband's odd taps.
    dn_b_between: History<OS_B_ODD, { 2 * OS_B_ODD }>,
    /// The 4× decimator's kept outputs at 2×, which the 2× decimator reads
    /// whole, once per session sample.
    dn_a: History<OS_A_FULL, { 2 * OS_A_FULL }>,
}

impl OsChannel {
    const fn new() -> Self {
        Self {
            up_a: History::new(),
            up_b: History::new(),
            dn_b_kept: History::new(),
            dn_b_between: History::new(),
            dn_a: History::new(),
        }
    }

    /// One session sample in, its four 4× sub-phases out, in order. The ×2
    /// on each push restores the amplitude an interpolator's zeros dilute.
    #[inline]
    fn up(&mut self, t: &OsTaps, x: f32) -> [f32; OS] {
        self.up_a.push(2.0 * x);
        let w = self.up_a.window();
        let s2 = [dot(&t.a_even, w), dot(&t.a_odd, w)];
        let mut s4 = [0.0_f32; OS];
        for (h, s) in s2.into_iter().enumerate() {
            self.up_b.push(2.0 * s);
            let w = self.up_b.window();
            s4[2 * h] = t.b_centre * w[OS_B_HALF];
            s4[2 * h + 1] = dot(&t.b_odd, w);
        }
        s4
    }

    /// The four 4× sub-phases of one session sample in, one session sample
    /// out. Each decimator keeps the EARLIER output of its pair (sub-phase
    /// `q == 0`, then `h == 0`), which is what puts the pair's delay on
    /// [`OS_LATENCY`] exactly; see there.
    #[inline]
    fn down(&mut self, t: &OsTaps, w: [f32; OS]) -> f32 {
        let mut y = 0.0;
        for h in 0..2 {
            self.dn_b_kept.push(w[2 * h]);
            let d4 = fma(
                t.b_centre,
                self.dn_b_kept.window()[OS_B_HALF],
                dot(&t.b_odd, self.dn_b_between.window()),
            );
            self.dn_a.push(d4);
            if h == 0 {
                y = dot(&t.a_full, self.dn_a.window());
            }
            self.dn_b_between.push(w[2 * h + 1]);
        }
        y
    }

    fn clear(&mut self) {
        self.up_a.clear();
        self.up_b.clear();
        self.dn_b_kept.clear();
        self.dn_b_between.clear();
        self.dn_a.clear();
    }
}

/// One session sample through the record amplifier and the tape at 4×: up
/// through the pair, saturated with the refused flux fed back per sub-phase,
/// and down again. `process` runs exactly this, and so does the test that
/// holds it to the direct form.
///
/// See [`OS`] for why it runs at 4×, [`DRIVE_TANH`] for the curve and
/// [`SAT_FB`] for the refusal: the value read at a sub-phase is the one that
/// sub-phase left one session sample ago, read BEFORE it is written, which
/// is the `z⁻¹` [`SAT_FB`] has always meant. `inv_d` is `1 / d`, worked out
/// once per frame by the caller rather than four times here.
#[inline]
fn record(st: &mut OsChannel, t: &OsTaps, err: &mut [f32; OS], d: f32, inv_d: f32, u: f32) -> f32 {
    let s4 = st.up(t, u);
    let mut w = [0.0_f32; OS];
    for ((w, s), e) in w.iter_mut().zip(s4).zip(err.iter_mut()) {
        *w = (d * (s + SAT_FB * *e)).tanh() * inv_d;
        *e = flush(s - *w);
    }
    st.down(t, w)
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
/// programme at the line-up level at 15 ips, the dial costs 0.07 dB at a
/// fresh tape's 0.5 and 1.43 dB at the very top, where before there was a
/// line-up the top cost over ten. On FLAT white noise the same sweep costs
/// more, and that difference is the whole point: white noise puts a third of
/// its energy above 8 kHz, where no tape has any, and what it is losing up
/// there is the self-erasure taking the top off the band. That is a tone
/// change, it is the effect the unit is named for, and a line-up that
/// chased it would be undoing the thing it is meant to make audible.
///
/// This is the open-loop answer, the curve alone with [`SAT_FB`] left out.
/// The table [`Tape::new`] fills corrects it for the refused flux the way a
/// steady tone actually meets it; see [`sat_makeup_refused`] for that, and
/// for why the correction is only ever allowed to fall.
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

/// [`sat_makeup`] with the refused flux in it: the same quadrature over the
/// line-up tone, each point the steady state of the feedback loop as a
/// sustained tone meets it, `w = tanh(d ((1 + SAT_FB) x - SAT_FB w)) / d`.
///
/// Solved by Newton's method from the open-loop answer. The loop's slope,
/// `1 + SAT_FB sech^2`, is bounded in `[1, 1 + SAT_FB]`, so the iteration
/// converges from anywhere and eight steps land it to the last bit.
///
/// **Why the line-up needs it, and why only its running minimum.** The
/// refused flux lifts a steady tone's output a little (the loop hands the
/// curve back some of what it refused), so 5.0's open-loop line-up let a 0 VU
/// tone come back up to 0.26 dB hot at high Drive, rising by up to 0.12 dB
/// per tenth of the dial: a record level doubling as a volume control, on
/// exactly the tone the line-up is meant to hold. This solve holds a steady
/// tone exactly. But the lift fades on a sine in deep saturation faster
/// than on a dense programme, so with the full correction a programme would
/// rise on the dial's last step instead. The table takes the correction's
/// running minimum from the bottom of the dial up: a factor that never rises
/// with `d`, so a programme that was monotone under 5.0's line-up stays
/// monotone, and a steady tone is held exactly up to where the lift peaks
/// (`d` near 7) and falls a little beyond it. The correction reads -0.02,
/// -0.09, -0.20 and -0.30 dB at `d` = 1, 2.4, 4.1 and 6.9, and holds -0.30
/// from there up.
fn sat_makeup_refused(d: f64) -> f64 {
    const STEPS: usize = 64;
    let fb = f64::from(SAT_FB);
    let amp = 10.0_f64.powf(ALIGN_DBFS / 20.0) * std::f64::consts::SQRT_2;
    let mut sum = 0.0_f64;
    for i in 0..STEPS {
        let th = std::f64::consts::FRAC_PI_2 * (i as f64 + 0.5) / STEPS as f64;
        let x = amp * th.sin();
        let mut w = (d * x).tanh() / d;
        for _ in 0..8 {
            let t = (d * ((1.0 + fb) * x - fb * w)).tanh();
            w -= (w - t / d) / (1.0 + fb * (1.0 - t * t));
        }
        sum += w * w;
    }
    let out_rms = (sum / STEPS as f64).sqrt();
    let in_rms = amp / std::f64::consts::SQRT_2;
    in_rms / out_rms.max(1.0e-12)
}

/// The line-up table [`Tape::new`] fills: at each of [`MAKEUP_POINTS`] record
/// levels, [`sat_makeup`] times the refused flux's correction, the
/// correction replaced by its running minimum from the lowest level up. See
/// [`sat_makeup_refused`].
fn makeup_table() -> [f32; MAKEUP_POINTS] {
    let mut held = 0.0_f64;
    std::array::from_fn(|i| {
        let d = makeup_d(i);
        let open = f64::from(sat_makeup(d));
        held = held.min(20.0 * (sat_makeup_refused(d) / open).log10());
        (open * 10.0_f64.powf(held / 20.0)) as f32
    })
}

/// How many record levels [`Tape::new`] solves the line-up at, evenly
/// spaced in octaves of `d` across [`MAKEUP_D`].
///
/// **A table, so the record level can move without a solve.** [`sat_makeup`]
/// is 64 hyperbolic tangents in f64, and the refused flux's correction nine
/// times that. Once a block that was affordable, but
/// the line-up has to follow the record level wherever it goes, and a level
/// that moves every frame cannot pay a quadrature every frame. 161 points
/// are 0.065 of an octave apart, and linear interpolation between them
/// stays within 0.0021 dB of the direct solve everywhere in the range;
/// `the_line_up_table_is_the_direct_solve` pins a hundredth.
const MAKEUP_POINTS: usize = 161;

/// The record levels the line-up table spans, as `d`: from an octave under
/// the dial's bottom ([`DRIVE_TANH`]), where the line-up is unity to a
/// thousandth of a decibel anyway, up to 64, a little over three times the
/// dial's top. The room above the dial is for the record level the curve
/// is actually asked for, which is the dial's `d` scaled by what the
/// machine records on: a slower tape saturates sooner and an under-biased
/// head sooner still, so the same dial asks the curve for more, and the
/// line-up has to read a true entry there rather than a clamp.
const MAKEUP_D: (f64, f64) = (0.05, 64.0);

/// The record level `d` that line-up entry `i` is solved at.
fn makeup_d(i: usize) -> f64 {
    let (lo, hi) = (MAKEUP_D.0.log2(), MAKEUP_D.1.log2());
    (lo + (hi - lo) * i as f64 / (MAKEUP_POINTS - 1) as f64).exp2()
}

/// The line-up for a record level given as `log2 d`: the two nearest
/// entries of the table, linearly interpolated, held at the ends.
fn makeup_at(table: &[f32; MAKEUP_POINTS], log2_d: f64) -> f32 {
    let (lo, hi) = (MAKEUP_D.0.log2(), MAKEUP_D.1.log2());
    let last = (MAKEUP_POINTS - 1) as f64;
    let x = ((log2_d - lo) / (hi - lo) * last).clamp(0.0, last);
    let i = (x as usize).min(MAKEUP_POINTS - 2);
    let t = (x - i as f64) as f32;
    table[i] + (table[i + 1] - table[i]) * t
}

/// Where a play head `ms` behind the record head rests, in whole samples at
/// `sample_rate`: the delay less the [`OS_LATENCY`] the record stage's
/// oversampler already holds, so the head and the pair together are `ms`.
///
/// `saturating_sub` and the floor are for a rate so low that the delay is
/// shorter than the filter pair. 8 kHz is the lowest a tape can be built at
/// and no engine runs there, but a kernel does not get to assume that, and a
/// head at least four samples deep is what [`tap`] needs to interpolate at
/// all. The rate is floored as [`Tape::new`] floors it, so a seat that asks
/// this of the session rate gets the head the kernel built.
fn head_rest(sample_rate: f32, ms: f32) -> usize {
    ((ms * 1.0e-3 * sample_rate.max(8_000.0)).round() as usize)
        .saturating_sub(OS_LATENCY)
        .max(4)
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

/// The matched one-pole low-pass for a corner of `fc` hertz: the `b` of
/// `y = (1 - b) x + b y1` whose magnitude at `f_m = min(10 kHz, rate / 4)` is
/// the analog one-pole's, with unity at DC.
///
/// **Matched, not mapped,** so a stage costs the same decibels at 10 kHz at
/// every rate, which is where every top-end figure in this file is
/// specified; a mapped corner near the top of the band would cost more at
/// 44.1 kHz than at 96. An infinite corner is an exact wire (`b = 0`), and a
/// blend of two of these stays in `[0, 1)`, so a table of them can be
/// interpolated without ever going unstable. Design time only.
fn matched_b(fc: f64, sample_rate: f64) -> f64 {
    let fm = 10_000.0_f64.min(0.25 * sample_rate);
    let g = 1.0 / (1.0 + (fm / fc).powi(2));
    if 1.0 - g < 1.0e-9 {
        return 0.0;
    }
    let p = 1.0 - g * (std::f64::consts::TAU * fm / sample_rate).cos();
    (p - (p * p - (1.0 - g).powi(2)).max(0.0).sqrt()) / (1.0 - g)
}

/// A one-pole's coefficient in the topology-preserving (trapezoidal) form,
/// `g / (1 + g)` with `g = tan(pi fc / rate)`: the bilinear transform warped
/// at the corner, stable for any corner and any change of it.
fn tpt_g(fc: f64, sample_rate: f64) -> f64 {
    let g = (std::f64::consts::PI * (fc / sample_rate).min(0.49)).tan();
    g / (1.0 + g)
}

/// A first-order section in its trapezoidal form: one state, a low-pass out
/// of it and the high-pass as what the low-pass leaves.
#[derive(Debug, Clone, Copy, Default)]
struct OnePole {
    s: f32,
}

impl OnePole {
    #[inline]
    fn low(&mut self, x: f32, g: f32) -> f32 {
        let v = (x - self.s) * g;
        let y = v + self.s;
        self.s = flush(y + v);
        y
    }

    #[inline]
    fn high(&mut self, x: f32, g: f32) -> f32 {
        x - self.low(x, g)
    }
}

/// A first-order section's coefficients, `y = b0 x + s`, `s = b1 x - a1 y`
/// (transposed direct form II, one state).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Section {
    b0: f32,
    b1: f32,
    a1: f32,
}

impl Section {
    const WIRE: Section = Section {
        b0: 1.0,
        b1: 0.0,
        a1: 0.0,
    };

    /// The NAB record shelf `(s + wz) / (s + wp)`, its zero at
    /// [`NAB_ZERO_HZ`] and its pole `db` decibels under it, by the bilinear
    /// transform warped at 1 kHz: unity at the top, `db` of lift at DC, and
    /// at 0 dB the zero and the pole are one, so the section is a wire.
    fn nab_shelf(db: f64, sample_rate: f64) -> [f64; 3] {
        let wz = std::f64::consts::TAU * NAB_ZERO_HZ;
        let wp = wz / 10.0_f64.powf(db / 20.0);
        let w1k = std::f64::consts::TAU * 1_000.0;
        let k = w1k / (w1k / (2.0 * sample_rate)).tan();
        [(k + wz) / (k + wp), (wz - k) / (k + wp), (wp - k) / (k + wp)]
    }

    /// The section whose coefficients are `c`, and its exact inverse: a
    /// first-order section's inverse swaps its numerator and denominator, so
    /// what goes onto the tape through one comes off it through the other
    /// with nothing left over.
    fn and_inverse(c: [f64; 3]) -> (Section, Section) {
        let [b0, b1, a1] = c;
        (
            Section {
                b0: b0 as f32,
                b1: b1 as f32,
                a1: a1 as f32,
            },
            Section {
                b0: (1.0 / b0) as f32,
                b1: (a1 / b0) as f32,
                a1: (b1 / b0) as f32,
            },
        )
    }

    #[inline]
    fn run(&self, s: &mut f32, x: f32) -> f32 {
        let y = self.b0 * x + *s;
        *s = flush(self.b1 * x - self.a1 * y);
        y
    }
}

/// A peaking bell as a trapezoidal state-variable filter (the bitcrusher's
/// sections, as a bell): the analog bell `(s^2 + s A/Q + 1) / (s^2 + s/(A Q)
/// + 1)`, `A^2` of gain at its centre, warped at the centre.
///
/// **A state-variable section, not a biquad,** because these bells move: the
/// bump's frequency follows a speed glide and its depth the Bump dial, and a
/// direct-form biquad retuned under a signal can ring or blow up where this
/// form stays stable for any sequence of its coefficients. At 0 dB `m1` is
/// zero and the bell is an exact wire.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Bell {
    a1: f32,
    a2: f32,
    a3: f32,
    m1: f32,
}

impl Bell {
    const WIRE: Bell = Bell {
        a1: 1.0,
        a2: 0.0,
        a3: 0.0,
        m1: 0.0,
    };

    /// The bell at `g = tan(pi f / rate)`, `q`, and `db` at its centre.
    fn design(g: f64, q: f64, db: f64) -> Self {
        let a = 10.0_f64.powf(db / 40.0);
        let k = 1.0 / (q * a);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self {
            a1: a1 as f32,
            a2: a2 as f32,
            a3: (g * a2) as f32,
            m1: (k * (a * a - 1.0)) as f32,
        }
    }

    #[inline]
    fn run(&self, st: &mut [f32; 2], x: f32) -> f32 {
        let v3 = x - st[1];
        let v1 = self.a1 * st[0] + self.a2 * v3;
        let v2 = st[1] + self.a2 * st[0] + self.a3 * v3;
        st[0] = flush(2.0 * v1 - st[0]);
        st[1] = flush(2.0 * v2 - st[1]);
        x + self.m1 * v1
    }
}

/// `tan(pi f / rate)` for a bell's centre, its frequency capped short of the
/// rate's Nyquist.
fn bell_g(hz: f64, sample_rate: f64) -> f64 {
    (std::f64::consts::PI * (hz / sample_rate).min(0.49)).tan()
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

/// One speed's voicing: every coefficient that depends on the speed,
/// designed by [`Tape::new`] at the unit's rate. A speed glide blends the two
/// speeds it is between, so nothing is designed on the audio thread while
/// the speed moves; see [`Tape::design`].
#[derive(Clone)]
struct Voicing {
    /// How far this speed moves the dial's `log2 d`: [`DMOL_DB`], in octaves.
    dmol: f64,
    /// The NAB record shelf ([`Section::nab_shelf`]).
    nab: [f64; 3],
    /// The self-erasure one-pole's `b` ([`matched_b`]) at each of
    /// [`ERASE_POINTS`] envelopes, with the floor in it.
    erase: [f64; ERASE_POINTS],
    /// The head's own top, a matched one-pole's `b`.
    top: f64,
    /// The contour bells' `tan(pi f / rate)`: the bump and its dip.
    bump_g: f64,
    dip_g: f64,
    /// The head's low-end roll-off, a trapezoidal one-pole's coefficient.
    low: f64,
}

impl Voicing {
    fn at(speed: TapeSpeed, sample_rate: f64) -> Self {
        let i = speed as usize;
        // The tape's speed in metres per second: every wavelength below is a
        // length on the tape, and its frequency is this over it.
        let v_m = 0.0254 * speed.ips();
        let floor = v_m / (ERASE_FLOOR_UM * 1.0e-6);
        let f_b = v_m / (BUMP_WAVELENGTH_MM * 1.0e-3);
        Self {
            dmol: -DMOL_DB[i] / 20.0 * std::f64::consts::LOG2_10,
            nab: Section::nab_shelf(NAB_SHELF_DB[i], sample_rate),
            erase: std::array::from_fn(|k| {
                let e = ERASE_CLAMP * k as f64 / (ERASE_POINTS - 1) as f64;
                let x = (ERASE_OCTAVES * e).exp2() - 1.0;
                let fc = if x > 0.0 {
                    (ERASE_TOP_HZ[i] / x).max(floor)
                } else {
                    f64::INFINITY
                };
                matched_b(fc, sample_rate)
            }),
            // 2 dB down at the band edge: a one-pole's corner is the edge
            // over `sqrt(10^0.2 - 1)`.
            top: matched_b(HEAD_TOP_EDGE_HZ[i] / (10.0_f64.powf(0.2) - 1.0).sqrt(), sample_rate),
            bump_g: bell_g(f_b, sample_rate),
            dip_g: bell_g(2.0 * f_b, sample_rate),
            low: tpt_g((HEAD_LF_SHARE * f_b).max(HEAD_LF_FLOOR_HZ), sample_rate),
        }
    }
}

/// One stereo pair of tracks through the record and repro chain. See the
/// module header for the signal flow.
pub struct Tape {
    sample_rate: f32,
    /// The tape: one line per channel, because the two tracks are magnetised
    /// independently, but read by one head at one place, which is why the
    /// sway below is shared.
    line: [Vec<f32>; 2],
    at: usize,
    /// The play head's rest position in whole samples. Whole on purpose:
    /// with the wobble at zero the tap interpolates at exactly zero fraction
    /// and hands samples through untouched. Shortened by [`OS_LATENCY`], for
    /// the reason [`BASE_MS`] gives.
    base: usize,
    /// The input transformer, per channel, and its coefficient.
    transformer: [OnePole; 2],
    g_transformer: f32,
    /// The NAB shelf's state on the way onto the tape and off it, per
    /// channel.
    nab_in: [f32; 2],
    nab_out: [f32; 2],
    /// The record stage's oversampler: one designed set of taps, and the
    /// histories each channel reads them against. See [`OS`].
    os_taps: OsTaps,
    os: [OsChannel; 2],
    /// The repro line-up across the whole record-level range, solved once
    /// here so no block solves it again. See [`MAKEUP_POINTS`].
    makeup: [f32; MAKEUP_POINTS],
    /// The flux the tape refused one session sample ago, held per oversampled
    /// sub-phase. See [`SAT_FB`] for why it is a sub-phase and not a sample.
    sat_err: [[f32; OS]; 2],
    /// The record-level envelope that drives the self-erasure, and the
    /// erasure one-pole itself.
    erase_env: [f32; 2],
    erase_lp: [f32; 2],
    /// The head's own top, the contour's two bells and its low-end roll-off,
    /// and Roll: the repro chain's states, per channel.
    top: [f32; 2],
    bump: [[f32; 2]; 2],
    dip: [[f32; 2]; 2],
    low: [OnePole; 2],
    roll_lp: [f32; 2],
    /// Hiss: a pinking ladder per channel (two tracks, two noises) and the
    /// shared program tracker that rides them up and down.
    pink: [Pink; 2],
    hiss_env: f32,
    /// The hiss's level as a linear gain, gliding to the dial. See [`Glide`].
    hiss_level: Glide,
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
    // Per-rate coefficients, fixed at construction: the sample rate is, too,
    // per the house law.
    a_att: f32,
    a_rel: f32,
    a_hatt: f32,
    a_hrel: f32,
    a_scrape_lo: f32,
    a_scrape_hi: f32,
    a_wander: f32,
    /// Every [`Glide`]'s per-frame step, [`GLIDE_MS`] at this rate, and the
    /// speed's, [`SPEED_GLIDE_MS`].
    a_glide: f64,
    a_speed: f64,
    /// The four speeds' voicings, slowest first, designed here at this rate.
    voicings: [Voicing; 4],
    /// The voicing in use, blended from `voicings` at the speed it was
    /// designed for (`voiced_at`, the speed's position on [`TapeSpeed::ALL`];
    /// NaN until the first design).
    voiced_at: f64,
    dmol: f64,
    nab: Section,
    nab_inv: Section,
    erase: [f32; ERASE_POINTS],
    top_b: f32,
    bump_g: f64,
    dip_g: f64,
    low_g: f32,
    /// The contour's two bells, and the `(bump g, dip g, bump dB)` they were
    /// designed at.
    bump_bell: Bell,
    dip_bell: Bell,
    bells_for: (f64, f64, f64),
    /// Roll's coefficient, the corner it was designed at (`log2` Hz), and
    /// whether the stage is in circuit.
    roll_a: f32,
    roll_for: f64,
    roll_on: bool,
    /// The glides: the speed as its position on [`TapeSpeed::ALL`] (which is
    /// `log2` of the speed, since the speeds are octaves apart), the dial's
    /// record level as `log2 d`, the bump in dB, Roll's corner as `log2` Hz,
    /// and the output trim as a gain.
    speed: Glide,
    drive: Glide,
    bump_db: Glide,
    roll: Glide,
    trim: Glide,
    /// Frames since `new` or `clear`: the grid [`SUB_BLOCK`] counts on.
    frame: u64,
    /// Whether the next block is the first since `new` or `clear`, which
    /// snaps every glide to that block's controls.
    fresh: bool,
}

impl Tape {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let cap = (LINE_MS * 1.0e-3 * sr) as usize + 8;
        let base = head_rest(sr, BASE_MS);
        let rate = f64::from(sr);
        let step = |ms: f64| 1.0 - (-1.0 / (rate * ms * 1.0e-3)).exp();
        Self {
            sample_rate: sr,
            line: [vec![0.0; cap], vec![0.0; cap]],
            at: 0,
            base,
            transformer: [OnePole::default(); 2],
            g_transformer: tpt_g(TRANSFORMER_HZ, rate) as f32,
            nab_in: [0.0; 2],
            nab_out: [0.0; 2],
            os_taps: OsTaps::new(),
            os: [OsChannel::new(), OsChannel::new()],
            makeup: makeup_table(),
            sat_err: [[0.0; OS]; 2],
            erase_env: [0.0; 2],
            erase_lp: [0.0; 2],
            top: [0.0; 2],
            bump: [[0.0; 2]; 2],
            dip: [[0.0; 2]; 2],
            low: [OnePole::default(); 2],
            roll_lp: [0.0; 2],
            pink: [Pink::new(sr, 0xA5A5_0001), Pink::new(sr, 0x5EED_C0DE)],
            hiss_env: 0.0,
            hiss_level: Glide::new(1.0e-9),
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
            a_att: env_a(rate, f64::from(ERASE_ATTACK_MS)),
            a_rel: env_a(rate, f64::from(ERASE_RELEASE_MS)),
            a_hatt: env_a(rate, f64::from(HISS_ATTACK_MS)),
            a_hrel: env_a(rate, f64::from(HISS_RELEASE_MS)),
            a_scrape_lo: lp_a(rate, f64::from(SCRAPE_BAND_HZ.0)),
            a_scrape_hi: lp_a(rate, f64::from(SCRAPE_BAND_HZ.1)),
            a_wander: env_a(rate, f64::from(WANDER_SLEW_S * 1_000.0)),
            a_glide: step(GLIDE_MS),
            a_speed: step(SPEED_GLIDE_MS),
            voicings: TapeSpeed::ALL.map(|v| Voicing::at(v, rate)),
            voiced_at: f64::NAN,
            dmol: 0.0,
            nab: Section::WIRE,
            nab_inv: Section::WIRE,
            erase: [0.0; ERASE_POINTS],
            top_b: 0.0,
            bump_g: 0.0,
            dip_g: 0.0,
            low_g: 0.0,
            bump_bell: Bell::WIRE,
            dip_bell: Bell::WIRE,
            bells_for: (f64::NAN, f64::NAN, f64::NAN),
            roll_a: 0.0,
            roll_for: f64::NAN,
            roll_on: false,
            speed: Glide::new(1.0e-7),
            drive: Glide::new(1.0e-7),
            bump_db: Glide::new(1.0e-6),
            roll: Glide::new(1.0e-6),
            trim: Glide::new(1.0e-9),
            frame: 0,
            fresh: true,
        }
    }

    /// The structural delay of the record stage and the play head together,
    /// in frames: what a host with delay compensation should be told. It is
    /// [`BASE_MS`] at every rate an engine actually runs at, oversampler and
    /// all; see that constant. An impulse peaks on exactly this frame.
    ///
    /// **A pure function of the settings and the rate this unit was built
    /// at,** asked of the params rather than of whatever the last block
    /// left behind: a seat that has just restored a bay, or holds one
    /// bypassed, has run no block, and the figure has to be right then too
    /// (the denoiser learned that the hard way, see
    /// `builtin_fx::DenoiseUnit::set_param`). No setting moves the head
    /// today, so every setting answers the same, the speed included: record
    /// and playback change speed together. One that does will answer here.
    pub fn latency_frames(&self, p: &TapeParams) -> usize {
        let _ = p;
        self.base + OS_LATENCY
    }

    /// The most a tape built at `sample_rate` can ever report, at any
    /// setting: what a seat sizes a line by when it has to hold the tape's
    /// delay itself, which a bypassed bay does (`builtin_fx::TapeUnit`
    /// keeps the dry signal as late as the tape, so switching it out
    /// compares two sounds and not two timings). One head today, so it is
    /// the figure [`Tape::latency_frames`] gives at every setting.
    pub fn max_latency_frames(sample_rate: f32) -> usize {
        head_rest(sample_rate, BASE_MS) + OS_LATENCY
    }

    /// Hand every glide its target from the dials, once per block, and on
    /// the first block after `new` or `clear` arrive there at once.
    fn aim(&mut self, p: &TapeParams) {
        let (drive, bump, roll, hiss, trim) = (
            norm(p.drive),
            norm(p.bump),
            norm(p.roll),
            norm(p.hiss),
            norm(p.trim),
        );
        let speed = TapeSpeed::from_dial(norm_or(p.speed, TapeSpeed::Ips15.dial()));
        self.speed.target = speed as usize as f64;
        self.drive.target = f64::from(DRIVE_TANH.0).log2()
            + f64::from(drive) * f64::from(DRIVE_TANH.1 / DRIVE_TANH.0).log2();
        self.bump_db.target = f64::from(BUMP_DB.0 + (BUMP_DB.1 - BUMP_DB.0) * bump);
        // Out of circuit, Roll's corner heads for the top of its sweep, and
        // the stage leaves the circuit only once it is there.
        let top = f64::from(ROLL_HZ.0).log2();
        self.roll.target = if roll > 1.0e-3 {
            top + f64::from(roll) * f64::from(ROLL_HZ.1 / ROLL_HZ.0).log2()
        } else {
            top
        };
        self.trim.target =
            10.0_f64.powf(f64::from((trim - 0.5) * (TRIM_DB.1 - TRIM_DB.0)) / 20.0);
        // **White noise carries its power in bandwidth.** The same sample
        // variance spread over twice the band is half the power per hertz,
        // so at 96 kHz every hertz of the audible band gets 44.1/96 of what
        // it gets at 44.1, and the gain has to rise by the square root of
        // the rates to hand it back. The root used to be the other way up,
        // which doubled the fault where it meant to cancel it: the hiss was
        // 0.74 dB quiet at 48 kHz and 6.8 dB quiet at 96.
        // `the_same_machine_runs_at_every_sample_rate` holds it. The level
        // glides as a linear gain: see [`Glide`] for why it may not glide in
        // decibels.
        let noise_rate = (self.sample_rate / DESIGN_RATE).sqrt();
        self.hiss_level.target = f64::from(hiss * hiss * HISS_MAX * noise_rate);
        if self.fresh {
            // The first block after `new` or `clear` starts AT its controls,
            // not gliding to them from wherever the glides were left.
            for g in [
                &mut self.speed,
                &mut self.drive,
                &mut self.bump_db,
                &mut self.roll,
                &mut self.trim,
                &mut self.hiss_level,
            ] {
                g.settle();
            }
            self.fresh = false;
        }
    }

    /// **The voicing for where the glides are now**, once per [`SUB_BLOCK`].
    ///
    /// Everything here is a blend of tables [`Tape::new`] designed, or a
    /// design that happens only while its control moves, so a settled
    /// machine spends three comparisons here and nothing else. Between two
    /// speeds every constant is blended at the glide's position, which is
    /// `log2` of the speed: piecewise-linear in `log2 v` between the four
    /// speeds the tables are exact at. A blend of two one-poles' `b` is a
    /// one-pole, and of two state-variable bells' `g` a bell, so no blend
    /// can go unstable.
    fn design(&mut self, roll_wanted: bool) {
        let pos = self.speed.cur;
        if pos != self.voiced_at {
            let lo = (pos.floor().max(0.0) as usize).min(TapeSpeed::ALL.len() - 2);
            let t = (pos - lo as f64).clamp(0.0, 1.0);
            let (a, b) = (&self.voicings[lo], &self.voicings[lo + 1]);
            let mix = |x: f64, y: f64| x * (1.0 - t) + y * t;
            self.dmol = mix(a.dmol, b.dmol);
            (self.nab, self.nab_inv) = Section::and_inverse([
                mix(a.nab[0], b.nab[0]),
                mix(a.nab[1], b.nab[1]),
                mix(a.nab[2], b.nab[2]),
            ]);
            for ((e, x), y) in self.erase.iter_mut().zip(&a.erase).zip(&b.erase) {
                *e = mix(*x, *y) as f32;
            }
            self.top_b = mix(a.top, b.top) as f32;
            self.bump_g = mix(a.bump_g, b.bump_g);
            self.dip_g = mix(a.dip_g, b.dip_g);
            self.low_g = mix(a.low, b.low) as f32;
            self.voiced_at = pos;
        }
        let bells = (self.bump_g, self.dip_g, self.bump_db.cur);
        if bells != self.bells_for {
            self.bump_bell = Bell::design(bells.0, BUMP_Q, bells.2);
            self.dip_bell = Bell::design(bells.1, DIP_Q, DIP_SHARE * bells.2);
            self.bells_for = bells;
        }
        let corner = self.roll.cur;
        if corner != self.roll_for {
            self.roll_a = lp_a(f64::from(self.sample_rate), corner.exp2());
            self.roll_for = corner;
        }
        self.roll_on = roll_wanted || self.roll.cur != self.roll.target;
    }

    /// In place on an interleaved buffer; `channels` is the stride. Stereo
    /// only: anything past the first two channels is left alone, and a mono
    /// buffer is returned untouched rather than half-processed, the same
    /// contract as `Effects::process`.
    pub fn process(&mut self, buf: &mut [f32], frames: usize, channels: usize, p: &TapeParams) {
        if channels < 2 {
            return;
        }
        debug_assert!(buf.len() >= frames * channels);
        let sr = self.sample_rate;
        self.aim(p);
        let roll_wanted = norm(p.roll) > 1.0e-3;
        let wobble = norm(p.wobble);
        let noise_rate = (sr / DESIGN_RATE).sqrt();
        let wow_amp = WOW_DEPTH_MS * 1.0e-3 * sr * wobble;
        let flutter_amp = (
            FLUTTER_DEPTH_MS.0 * 1.0e-3 * sr * wobble,
            FLUTTER_DEPTH_MS.1 * 1.0e-3 * sr * wobble,
        );
        // The scrape is white noise through a band too, so its sway per
        // hertz needs the same root; without it the head scraped 3.4 dB
        // less at 96 kHz than at 44.1, and
        // `the_scrape_sprays_the_same_sidebands_at_every_sample_rate`
        // holds that.
        let scrape_amp = SCRAPE_MS * 1.0e-3 * sr * wobble * noise_rate;
        let n = self.line[0].len();
        let tau = std::f32::consts::TAU;
        let e_scale = (ERASE_POINTS - 1) as f32 / ERASE_CLAMP as f32;

        for i in 0..frames {
            let f = i * channels;
            if self.frame.is_multiple_of(SUB_BLOCK) {
                self.design(roll_wanted);
            }
            self.frame += 1;

            // ── the glides, and the record level they make ─────────────────
            self.speed.tick(self.a_speed);
            self.bump_db.tick(self.a_glide);
            self.roll.tick(self.a_glide);
            let trim_gain = self.trim.tick(self.a_glide) as f32;
            // The dial's record level, scaled by this speed's headroom: the
            // `d` the curve is asked for, and the line-up for it, read from
            // the table [`Tape::new`] solved (see [`sat_makeup_refused`]).
            let log2_d = self.drive.tick(self.a_glide) + self.dmol;
            let makeup = makeup_at(&self.makeup, log2_d);
            let d = log2_d.exp2() as f32;
            let inv_d = 1.0 / d;

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
            let hiss_level = self.hiss_level.tick(self.a_glide) as f32;
            let hiss_gain =
                hiss_level * (HISS_FLOOR + (1.0 - HISS_FLOOR) * self.hiss_env.min(1.0));

            for ch in 0..2 {
                // The input transformer: keep DC and rumble out of the
                // headroom.
                let u = self.transformer[ch].high(buf[f + ch], self.g_transformer);

                // The record level, dual-rate. This envelope, not any gain
                // element, is what rounds the attacks, spectrally, below.
                let lvl = (u * d).abs();
                let a = if lvl > self.erase_env[ch] {
                    self.a_att
                } else {
                    self.a_rel
                };
                self.erase_env[ch] = flush(self.erase_env[ch] + (lvl - self.erase_env[ch]) * a);

                // Onto the tape through the NAB record shelf, the record
                // amplifier and the tape at 4× (see [`record`]), the repro
                // amplifier's line-up (not a knob: see [`sat_makeup`]), and
                // off it through the shelf's exact inverse.
                let r = self.nab.run(&mut self.nab_in[ch], u);
                let y = record(&mut self.os[ch], &self.os_taps, &mut self.sat_err[ch], d, inv_d, r);
                let y = self.nab_inv.run(&mut self.nab_out[ch], y * makeup);

                // HF self-erasure: the corner falls as the level rises, read
                // from this speed's table. At rest `b` is zero: a wire.
                let e = self.erase_env[ch].min(ERASE_CLAMP as f32) * e_scale;
                let k = (e as usize).min(ERASE_POINTS - 2);
                let b = self.erase[k] + (self.erase[k + 1] - self.erase[k]) * (e - k as f32);
                self.erase_lp[ch] = flush(y + b * (self.erase_lp[ch] - y));
                self.line[ch][self.at] = self.erase_lp[ch];

                // The play head, swaying, and the head's own top.
                let v = tap(&self.line[ch], self.at, delay);
                self.top[ch] = flush(v + self.top_b * (self.top[ch] - v));
                let mut v = self.top[ch];

                // The hiss: the repro chain's noise, which the head contour
                // and Roll shape as they shape the music. A level glided to
                // exactly nothing skips the stage.
                if hiss_level > 0.0 {
                    v += self.pink[ch].step() * hiss_gain;
                }

                // The head contour, then Roll. Out of circuit, Roll's state
                // follows the signal, so it comes back in without a step.
                v = self.bump_bell.run(&mut self.bump[ch], v);
                v = self.dip_bell.run(&mut self.dip[ch], v);
                v = self.low[ch].high(v, self.low_g);
                if self.roll_on {
                    self.roll_lp[ch] = flush(self.roll_lp[ch] + self.roll_a * (v - self.roll_lp[ch]));
                    v = self.roll_lp[ch];
                } else {
                    self.roll_lp[ch] = v;
                }

                buf[f + ch] = v * trim_gain;
            }

            self.at += 1;
            if self.at >= n {
                self.at = 0;
            }
        }
    }

    /// Forget everything: the tape, the envelopes, the noise, the glides.
    /// What was ringing when the unit left circuit must not be waiting when
    /// it comes back half an hour later, and the next block starts at its
    /// controls.
    pub fn clear(&mut self) {
        for l in &mut self.line {
            l.fill(0.0);
        }
        self.at = 0;
        self.transformer = [OnePole::default(); 2];
        self.nab_in = [0.0; 2];
        self.nab_out = [0.0; 2];
        for c in &mut self.os {
            c.clear();
        }
        self.sat_err = [[0.0; OS]; 2];
        self.erase_env = [0.0; 2];
        self.erase_lp = [0.0; 2];
        self.top = [0.0; 2];
        self.bump = [[0.0; 2]; 2];
        self.dip = [[0.0; 2]; 2];
        self.low = [OnePole::default(); 2];
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
        self.voiced_at = f64::NAN;
        self.bells_for = (f64::NAN, f64::NAN, f64::NAN);
        self.roll_for = f64::NAN;
        self.frame = 0;
        self.fresh = true;
    }

    /// The speed the voicing is at right now, in inches per second: the
    /// glide's position, for the test that holds the glide to a servo's
    /// re-lock.
    #[cfg(test)]
    pub(crate) fn speed_now(&self) -> f64 {
        TapeSpeed::Ips3_75.ips() * self.speed.cur.exp2()
    }
}

// ── proof ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::drive::Fir;

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
    /// of broadband level. Not bit-exact (the play head is a real delay, and
    /// the transformer and the head's own top are always in circuit) but
    /// level-transparent, which is what "off" has to mean for an insert. At
    /// the studio speed a fresh tape opens at, and at 7.5 ips, the speed the
    /// record curve is the machine of. Measured: -0.031 and -0.018 dB at
    /// 15 ips (Drive 0, and a fresh tape's 0.5), -0.051 and -0.057 at 7.5.
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
        let at = |drive: f32, speed: TapeSpeed| {
            let p = TapeParams {
                drive,
                speed: speed.dial(),
                ..TapeParams::NEUTRAL
            };
            let out = feed(&mut Tape::new(sr), &input, &p);
            db(rms(&out[lo..hi]) / rms(&input[lo..hi]))
        };

        for speed in [TapeSpeed::Ips15, TapeSpeed::Ips7_5] {
            // **Drive at the BOTTOM, not the middle.** It is a record level,
            // and a record level's off is the end of its travel, the same
            // place every other knob here parks. This used to read 0.5 and
            // pass only because the drive's top was too low to matter at 0.5;
            // see [`DRIVE_TANH`].
            let off = at(0.0, speed);
            assert!(
                off.abs() < 0.1,
                "neutral settings moved the broadband level by {off:.3} dB at {}",
                speed.label()
            );

            // **And at the setting a fresh tape opens at**, which is the
            // check that was missing. Moving this test's drive from the
            // middle to the bottom was correct and it was also where the
            // level regression got through: with the guard anchored at an
            // end of the travel, nothing on the bench was watching the
            // position every new bay opens at. The line-up is what holds it.
            let fresh = at(TapeParams::default().drive, speed);
            assert!(
                fresh.abs() < 0.4,
                "the fresh drive moved a programme at record level by {fresh:.3} dB \
                 at {}: the record level is doubling as a volume control",
                speed.label()
            );
        }
    }

    /// **An attack loses treble before it loses level, which no gain element
    /// could do.** A gain takes the same decibels from every band; here the
    /// loud first milliseconds lose far more at 8 kHz than at 500 Hz, and
    /// the 8 kHz comes back as the level settles: the self-erasure, working.
    ///
    /// Measured at 7.5 ips, the speed the record curve is the machine of:
    /// -7.7, -11.3, -12.1 and -2.0 dB against the four bars (model -5.8,
    /// -11.1, -11.9 and -2.0). And the loss is a wavelength's, so the settled
    /// 8 kHz-against-500 Hz loss is ordered by speed, deepest at the slowest:
    /// -17.4, -11.3, -1.0 and -0.4 dB at 3.75, 7.5, 15 and 30 ips (model
    /// -17.2, -11.1, -0.9 and -0.4).
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
        let win = 240; // the first 5 ms
        let sus = pre + (sr * 0.25) as usize; // well after the step down
        // Half travel, on a -6 dBFS attack. The knob used to read 0.7 here
        // and it is the SAME record level: [`DRIVE_TANH`]'s top moved from 4
        // to 20, so the factor that used to sit at 0.7 now sits just under
        // half. The claim being measured is about the erasure, not about the
        // dial, so the operating point is what had to be preserved. The
        // windows line up by the latency the host is told, which is exact.
        let at_speed = |speed: TapeSpeed| {
            let p = TapeParams {
                drive: 0.5,
                speed: speed.dial(),
                ..TapeParams::NEUTRAL
            };
            let mut unit = Tape::new(sr);
            let lat = unit.latency_frames(&p);
            let out = left(&feed(&mut unit, &input, &p));
            let ratio = |at: usize, hz: f64| {
                band(&out[at + lat..at + lat + win], sr.into(), hz)
                    / band(&mono[at..at + win], sr.into(), hz)
            };
            // The window after the first: the corner has finished falling
            // and sits where the level put it.
            [
                ratio(pre, 8_000.0),
                ratio(pre, 500.0),
                ratio(pre + win, 8_000.0),
                ratio(pre + win, 500.0),
                ratio(sus, 8_000.0),
            ]
        };
        let [r8_attack, r500_attack, r8_settled, r500_settled, r8_sustain] =
            at_speed(TapeSpeed::Ips7_5);

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

        // The same attack at every speed: the settled loss of 8 kHz against
        // 500 Hz is deepest on the slowest tape, where 8 kHz is the shortest
        // wavelength, and ordered all the way up.
        let settled: Vec<f64> = TapeSpeed::ALL
            .iter()
            .map(|v| {
                let r = at_speed(*v);
                db(r[2] / r[3])
            })
            .collect();
        assert!(
            settled.windows(2).all(|w| w[0] < w[1]),
            "the settled 8 kHz-against-500 Hz loss is not ordered by speed: {settled:.2?} dB \
             at 3.75, 7.5, 15 and 30 ips"
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
            wobble: 1.0,
            ..TapeParams::NEUTRAL
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
            hiss: 1.0,
            ..TapeParams::NEUTRAL
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
    /// bottom, middle and top on all six at once, the controls appended since
    /// at their defaults, against a signal with a step in the middle of every
    /// block, on one unit whose state rides through the whole sweep: a knob
    /// drag, not a bench test. Then NaN params, the end-to-end jumps, the
    /// speed thrown between 3.75 and 30 ips every block, and a clear that
    /// must actually clear.
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
                ..TapeParams::default()
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
            speed: f32::NAN,
        };
        let mut buf = block.clone();
        unit.process(&mut buf, 128, 2, &p);
        assert!(
            buf.iter().all(|v| v.is_finite()),
            "NaN params reached the audio"
        );

        // **Every control end to end on alternate blocks, and the hiss back
        // from silence.** A glide leaving the bottom of a level's travel is
        // where a level gliding in decibels computes its way to NaN (see
        // [`Glide`]), and a NaN level that skipped its stage would not reach
        // the audio, it would only leave the hiss dead for good. So the hiss
        // jumps 0, 1, 0, 1 on silence, every control then jumps with it on
        // the stepped signal, and after that the hiss held up must be there.
        // And a fresh unit starts at its controls rather than gliding to
        // them: its first block of hiss is already most of the way up.
        let silence = vec![0.0_f32; 128 * 2];
        let mut jumps = Tape::new(sr);
        for k in 0..128 {
            let end = (k % 2) as f32;
            let p = if k < 64 {
                TapeParams {
                    hiss: end,
                    ..TapeParams::NEUTRAL
                }
            } else {
                TapeParams {
                    drive: end,
                    bump: end,
                    roll: end,
                    wobble: end,
                    hiss: end,
                    trim: end,
                    speed: end,
                }
            };
            let mut buf = if k < 64 { silence.clone() } else { block.clone() };
            jumps.process(&mut buf, 128, 2, &p);
            assert!(
                buf.iter().all(|v| v.is_finite() && v.abs() < 16.0),
                "block {k} of the end-to-end jumps left the audio at {:?}",
                buf.iter().find(|v| !v.is_finite() || v.abs() >= 16.0)
            );
        }
        let hissing = TapeParams {
            hiss: 1.0,
            ..TapeParams::NEUTRAL
        };
        let mut held = vec![0.0_f32; 4_800 * 2];
        jumps.process(&mut held, 4_800, 2, &hissing);
        let steady = rms(&held[2_400 * 2..]);
        assert!(steady > 1.0e-5, "the hiss never came back after its jumps: {steady:e}");
        let mut fresh = Tape::new(sr);
        let blocks: Vec<f64> = (0..12)
            .map(|_| {
                let mut buf = silence.clone();
                fresh.process(&mut buf, 128, 2, &hissing);
                rms(&buf)
            })
            .collect();
        assert!(
            blocks[0] > 0.5 * blocks[11],
            "a fresh unit's first block of hiss read {:e} against {:e} eleven blocks on: \
             it glided in",
            blocks[0],
            blocks[11]
        );

        // The speed thrown between its two ends every block, slammed: every
        // per-speed table blended mid-glide, two hundred times over.
        let mut thrown = Tape::new(sr);
        for k in 0..200 {
            let p = TapeParams {
                drive: 1.0,
                speed: (k % 2) as f32,
                ..TapeParams::default()
            };
            let mut buf = block.clone();
            thrown.process(&mut buf, 128, 2, &p);
            assert!(
                buf.iter().all(|v| v.is_finite() && v.abs() < 16.0),
                "block {k} of the speed thrown end to end left the audio at {:?}",
                buf.iter().find(|v| !v.is_finite() || v.abs() >= 16.0)
            );
        }

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
    /// probe, at 44.1, 48 and 96. At 30 ips, where the contour's bell sits at
    /// 69.9 Hz: Bump 1 there reads its +5 dB less the dip an octave up and
    /// the head's own roll-off below it (model +3.8 dB, measured +3.79 at
    /// every rate), and Roll at half travel costs its corner's 1.8 dB at
    /// 5 kHz with the head's own top (model -1.86, measured -1.70 to -1.83).
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

        // The bump, wide open, at the speed that puts it at 70 Hz.
        let bump = TapeParams {
            drive: 0.5,
            bump: 1.0,
            speed: TapeSpeed::Ips30.dial(),
            ..TapeParams::NEUTRAL
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
            roll: 0.5,
            speed: TapeSpeed::Ips30.dial(),
            ..TapeParams::NEUTRAL
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
    /// of tape, both of them, at once. At the studio speed a fresh tape opens
    /// at (model: the bottom far under 1e-4, half travel about 0.12 %, the
    /// top over 5 %; the tilt -0.34 dB at the bottom, -5.35 dB at the top;
    /// measured 6e-6, 0.12 % and 17.8 %, and -0.34 and -5.49 dB).
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
    /// room to spare, and which reverting the oversampler fails by twenty.
    ///
    /// **And against the tone that went in, at every speed.** The dial has no
    /// cap: the speeds scale its record level, so its top asks the curve for
    /// up to 26.1 at 3.75 ips where 5.0's top was 20. The guarantee is stated
    /// absolutely, then: full drive folds no more hash into the band than
    /// 5.0's top did, which read -52.3 dB against the input tone; every
    /// speed must read under -50 (model -54.3, -53.9, -56.3 and -55.3 at
    /// 3.75, 7.5, 15 and 30 ips; measured -55.1, -54.6, -56.9 and -55.9).
    /// Against the OUTPUT tone 3.75 ips reads far worse, -29.0, only because
    /// a slow tape's self-erasure takes more from the 7 kHz fundamental than
    /// from the folds below it: the honest measure of the record stage is
    /// the input. At 15 ips against the output: -43.5 at the top, -95.5 at
    /// a fresh tape's Drive.
    #[test]
    fn full_drive_on_a_high_note_does_not_fold_the_band_back_on_itself() {
        let sr = 48_000.0_f32;
        let frames = (sr * 0.7) as usize;
        // Everything that is not the saturator, off: the wobble spreads a
        // tone into sidebands and the hiss fills every bin, and both would be
        // read as aliasing by a measurement that cannot tell them apart.
        let clean = TapeParams {
            drive: 1.0,
            ..TapeParams::NEUTRAL
        };
        // −6 dBFS: loud, because the fold gets worse the harder the curve is
        // driven and the claim is about the worst case a person can reach.
        let mono: Vec<f32> = (0..frames)
            .map(|i| 0.5 * (std::f32::consts::TAU * 7_000.0 * i as f32 / sr).sin())
            .collect();
        let (lo, hi) = ((sr * 0.25) as usize, (sr * 0.65) as usize);
        let tone_in = band(&mono[lo..hi], sr.into(), 7_000.0);

        // The fold against the output tone, and against the input tone.
        let fold = |drive: f32, speed: TapeSpeed| -> (f64, f64) {
            let out = left(&feed(
                &mut Tape::new(sr),
                &to_stereo(&mono),
                &TapeParams {
                    drive,
                    speed: speed.dial(),
                    ..clean
                },
            ));
            let seg = &out[lo..hi];
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
            (db(alias.sqrt() / fund.max(1.0e-12)), db(alias.sqrt() / tone_in))
        };
        let residue = |drive: f32| fold(drive, TapeSpeed::Ips15).0;

        let slammed = residue(1.0);
        assert!(
            slammed < -30.0,
            "full drive on a 7 kHz tone folded {slammed:.1} dB of inharmonic \
             hash back into the band — the record stage is aliasing"
        );
        // And at the position a fresh tape opens at, where the curve is
        // barely bent, the fold has to be nowhere at all.
        let fresh = residue(TapeParams::default().drive);
        assert!(
            fresh < -55.0,
            "the fresh drive folded {fresh:.1} dB of hash into the band"
        );
        for speed in TapeSpeed::ALL {
            let (_, against_input) = fold(1.0, speed);
            assert!(
                against_input < -50.0,
                "full drive at {} folded {against_input:.1} dB of hash against the \
                 tone that went in, more than 5.0's top did",
                speed.label()
            );
        }
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
    /// At the studio speed (model: within 0.15 dB up to Drive 0.6, -1.37 dB
    /// at the top, a largest rise of +0.001; measured -0.15, -1.43 and no
    /// rise).
    ///
    /// **And the dial is never louder for a step up, at any speed.** The
    /// programme's monotone law holds at 7.5 and 3.75 ips too (model rises
    /// -0.001 and +0.002 dB, measured none), where the slow tape's erasure
    /// takes more of the top at the dial's end. And a steady 0 VU tone at
    /// 100 Hz, the long wavelength a line-up that chased the erasure would
    /// have lifted, rises by no more than 0.05 dB from any tenth of Drive to
    /// the next at any of the four speeds (model +0.002 dB at most, measured
    /// +0.002 at 30 ips and none at the others). Not pinned: the
    /// programme at 30 ips, whose last step rises +0.037 dB in the model,
    /// too near the bar to be a fair pin.
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
        let programme = |drive: f32, speed: TapeSpeed| {
            let p = TapeParams {
                drive,
                speed: speed.dial(),
                ..TapeParams::NEUTRAL
            };
            let out = feed(&mut Tape::new(sr), &input, &p);
            db(rms(&out[lo..hi]) / rms(&input[lo..hi]))
        };
        let at = |drive: f32| programme(drive, TapeSpeed::Ips15);

        // The bottom is a wire, and a fresh tape's Drive and the 0.6 that
        // 5.0 shipped are within a few hundredths of it (measured -0.07 and
        // -0.15 dB). Between them the knob is character only.
        for drive in [0.0_f32, 0.2, 0.4, TapeParams::default().drive, 0.6] {
            let delta = at(drive);
            assert!(
                delta.abs() < 0.4,
                "drive {drive} moved a 0 VU program by {delta:.3} dB: up to \
                 the old default this dial may not touch the level"
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
        for speed in [TapeSpeed::Ips15, TapeSpeed::Ips7_5, TapeSpeed::Ips3_75] {
            let mut last = f64::INFINITY;
            for step in 0..=10 {
                let here = programme(step as f32 / 10.0, speed);
                assert!(
                    here <= last + 0.05,
                    "at {} drive {} read {here:.3} dB against {last:.3} dB the step \
                     before: the line-up is running the wrong way",
                    speed.label(),
                    step as f32 / 10.0
                );
                last = here;
            }
        }

        // A steady 0 VU tone at 100 Hz, at every speed.
        let low = 10.0_f32.powf(-18.0 / 20.0) * 2.0_f32.sqrt();
        let hundred: Vec<f32> = (0..(sr * 0.6) as usize)
            .map(|i| low * (std::f32::consts::TAU * 100.0 * i as f32 / sr).sin())
            .collect();
        let (t_lo, t_hi) = ((sr * 0.3) as usize, (sr * 0.6) as usize);
        let tone_in = band(&hundred[t_lo..t_hi], sr.into(), 100.0);
        for speed in TapeSpeed::ALL {
            let mut last = f64::INFINITY;
            for step in 0..=10 {
                let p = TapeParams {
                    drive: step as f32 / 10.0,
                    speed: speed.dial(),
                    ..TapeParams::NEUTRAL
                };
                let out = left(&feed(&mut Tape::new(sr), &to_stereo(&hundred), &p));
                let here = db(band(&out[t_lo..t_hi], sr.into(), 100.0) / tone_in);
                assert!(
                    here <= last + 0.05,
                    "at {} a 0 VU 100 Hz tone read {here:.3} dB at drive {}, against \
                     {last:.3} dB the step before: the dial is a bass volume control",
                    speed.label(),
                    step as f32 / 10.0
                );
                last = here;
            }
        }
    }

    /// **The oversampler is invisible to the host.** The record stage's
    /// filter pair delays the signal by [`OS_LATENCY`] samples, and the play
    /// head gives up exactly that many, so what a host is told to compensate
    /// is still [`BASE_MS`] — at every rate an engine runs at. A unit that
    /// quietly grew three quarters of a millisecond would slide against every
    /// other track in the session and nothing would say why.
    ///
    /// The figure is asked of the settings, so it is also pinned against
    /// them: the head and the pair add up to it exactly, and no position of
    /// any of the seven dials moves it, the speed included, because a report
    /// that moved with a character dial would re-time the lane in the
    /// compensation graph every time somebody turned that dial.
    #[test]
    fn the_oversampler_costs_the_host_no_latency() {
        let positions = [0.0_f32, 0.5, 1.0];
        for sr in [44_100.0_f32, 48_000.0, 96_000.0] {
            let want = (BASE_MS * 1.0e-3 * sr).round() as usize;
            let unit = Tape::new(sr);
            let got = unit.latency_frames(&TapeParams::default());
            assert_eq!(
                got, want,
                "at {sr} Hz the tape reports {got} frames of latency, not the \
                 {want} that BASE_MS promises"
            );
            assert_eq!(
                unit.base + OS_LATENCY,
                got,
                "at {sr} Hz the head ({}) and the pair ({OS_LATENCY}) do not add up \
                 to the {got} frames reported",
                unit.base
            );
            for combo in 0..3_usize.pow(7) {
                let at = |k: u32| positions[combo / 3_usize.pow(k) % 3];
                let p = TapeParams {
                    drive: at(0),
                    bump: at(1),
                    roll: at(2),
                    wobble: at(3),
                    hiss: at(4),
                    trim: at(5),
                    speed: at(6),
                };
                assert_eq!(
                    unit.latency_frames(&p),
                    want,
                    "dial combination {combo} moved the report at {sr} Hz"
                );
            }
        }
    }

    /// The record stage's pair the way it shipped: four of the drive's
    /// direct-form FIRs over the zero-stuffed streams, every output computed
    /// and the ones a decimator does not want dropped, keeping sub-phase
    /// `keep.0` at 4× and `keep.1` at 2×. Given a record level it is the
    /// whole record stage, the refused flux per sub-phase included.
    struct DirectPair {
        a: [f32; OS_A_TAPS],
        b: [f32; OS_B_TAPS],
        up_a: Fir<OS_A_TAPS, { 2 * OS_A_TAPS }>,
        up_b: Fir<OS_B_TAPS, { 2 * OS_B_TAPS }>,
        dn_b: Fir<OS_B_TAPS, { 2 * OS_B_TAPS }>,
        dn_a: Fir<OS_A_TAPS, { 2 * OS_A_TAPS }>,
        err: [f32; OS],
        keep: (usize, usize),
    }

    impl DirectPair {
        fn new(keep: (usize, usize)) -> Self {
            Self {
                a: windowed_sinc(8.0, OS_A_CUTOFF),
                b: windowed_sinc(8.0, 0.25),
                up_a: Fir::new(),
                up_b: Fir::new(),
                dn_b: Fir::new(),
                dn_a: Fir::new(),
                err: [0.0; OS],
                keep,
            }
        }

        fn run(&mut self, u: f32, d: Option<f32>) -> f32 {
            let mut y = 0.0_f32;
            for h in 0..2 {
                let s2 = self.up_a.run(&self.a, if h == 0 { 2.0 * u } else { 0.0 });
                for q in 0..2 {
                    let s4 = self.up_b.run(&self.b, if q == 0 { 2.0 * s2 } else { 0.0 });
                    let phase = h * 2 + q;
                    let w = match d {
                        Some(d) => {
                            let w = (d * (s4 + SAT_FB * self.err[phase])).tanh() / d;
                            self.err[phase] = flush(s4 - w);
                            w
                        }
                        None => s4,
                    };
                    let d4 = self.dn_b.run(&self.b, w);
                    if q == self.keep.0 {
                        let d2 = self.dn_a.run(&self.a, d4);
                        if h == self.keep.1 {
                            y = d2;
                        }
                    }
                }
            }
            y
        }
    }

    /// Where a buffer is loudest, by magnitude.
    fn loudest(v: &[f32]) -> usize {
        (0..v.len())
            .max_by(|a, b| v[*a].abs().total_cmp(&v[*b].abs()))
            .expect("a buffer to look in")
    }

    /// **The oversampler is the direct-form pair, at less than half the
    /// arithmetic.**
    ///
    /// The polyphase pair skips three things the direct form multiplied:
    /// the zeros each interpolator stuffs in, the outputs each decimator
    /// throws away, and the halfband's even taps, which are its centre and
    /// zeros. Skipping them is a speed-up only if what is skipped is
    /// nothing, so the pair is held to the direct form it replaced, built
    /// from the drive's own FIR: on an impulse and on four thousand frames
    /// of noise at half scale the linear pair agrees within a millionth, and
    /// the whole record stage slammed (the top of the dial, the refused flux
    /// fed back) within a hundred-thousandth, which is what the same
    /// additions in another order can leave in f32.
    ///
    /// The same direct-form model keeping the samples the pair used to keep,
    /// the later of each pair, peaks a frame early: the three quarters of a
    /// sample the head was wrong by ([`OS_LATENCY`]). Keeping the earlier
    /// ones it peaks on [`OS_LATENCY`], and so does the polyphase pair. And
    /// the arithmetic is counted from the tap tables the dot products run:
    /// 226 multiply-adds per channel-sample, padding included, against 484.
    #[test]
    fn the_oversampler_is_the_direct_form_pair_at_less_than_half_the_arithmetic() {
        let taps = OsTaps::new();
        // What the polyphase form leaves out is nothing: the halfband's
        // centre is exactly a half and its other even taps are residue.
        let b: [f32; OS_B_TAPS] = windowed_sinc(8.0, 0.25);
        assert_eq!(taps.b_centre, 0.5, "the halfband's centre is not a half");
        for (k, v) in b.iter().enumerate().step_by(2) {
            if k != (OS_B_TAPS - 1) / 2 {
                assert!(v.abs() < 1.0e-16, "halfband tap {k} is {v:e}, not a zero");
            }
        }

        let linear = |input: &[f32], keep: (usize, usize)| -> (Vec<f32>, Vec<f32>) {
            let mut poly = OsChannel::new();
            let mut direct = DirectPair::new(keep);
            input
                .iter()
                .map(|&x| {
                    let s4 = poly.up(&taps, x);
                    (poly.down(&taps, s4), direct.run(x, None))
                })
                .unzip()
        };
        let mut impulse = vec![0.0_f32; 256];
        impulse[0] = 1.0;
        let mut seed = 0x0B5E_ED01_u32;
        let noise: Vec<f32> = (0..4_096).map(|_| 0.5 * white(&mut seed)).collect();
        for (what, input) in [("an impulse", &impulse), ("noise", &noise)] {
            let (poly, direct) = linear(input, (0, 0));
            let worst = poly
                .iter()
                .zip(&direct)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f32, f32::max);
            assert!(
                worst < 1.0e-6,
                "on {what} the polyphase pair is {worst:e} from the direct form"
            );
        }

        let (poly, early) = linear(&impulse, (0, 0));
        assert_eq!(loudest(&poly), OS_LATENCY, "the polyphase pair peaks off its delay");
        assert_eq!(loudest(&early), OS_LATENCY, "the direct form keeping q0/h0 peaks off it");
        let (_, late) = linear(&impulse, (1, 1));
        assert_eq!(
            loudest(&late),
            OS_LATENCY - 1,
            "the direct form keeping the later samples no longer peaks a frame early, \
             so this model is not the pair that shipped"
        );

        let d = DRIVE_TANH.1;
        let mut poly = OsChannel::new();
        let mut err = [0.0_f32; OS];
        let mut direct = DirectPair::new((0, 0));
        let worst = noise
            .iter()
            .map(|&x| (record(&mut poly, &taps, &mut err, d, 1.0 / d, x) - direct.run(x, Some(d))).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            worst < 1.0e-5,
            "the slammed record stage is {worst:e} from the direct form's"
        );

        let direct_macs = 2 * OS_A_TAPS + 4 * OS_B_TAPS + 4 * OS_B_TAPS + 2 * OS_A_TAPS;
        let polyphase_macs = (OS_A_EVEN + OS_A_ODD + 2 * OS_B_ODD) + (2 * (OS_B_ODD + 1) + OS_A_FULL);
        assert_eq!(direct_macs, 484);
        assert_eq!(polyphase_macs, 226);
        assert!(2 * polyphase_macs < direct_macs, "the polyphase pair is not half the work");
    }

    /// **An impulse leaves exactly when the host is told it will.**
    ///
    /// Delay compensation moves every other track by the figure a unit
    /// reports, so a unit whose real delay is not its report is a flam that
    /// nothing in the session explains: a fraction of a sample is a comb
    /// across the top of the band against the same signal on another track.
    /// The pair used to keep the later sample of each decimation, and an
    /// impulse through the tape peaked a whole frame before the report, its
    /// centre three quarters of a sample ahead ([`OS_LATENCY`]).
    ///
    /// So a lone impulse after a second of silence must peak on exactly the
    /// reported frame at every rate an engine runs at and at every speed,
    /// with every colour off and at a fresh tape's settings with the
    /// transport and the hiss still. Minimum-phase stages add their delay
    /// after the peak without moving it.
    ///
    /// With every colour off at 15 and 30 ips its three-point parabolic
    /// centre must also sit within a twentieth of a sample of the report at
    /// 44.1 and 48 kHz, and within a tenth at 96. Self-erasure is an exact
    /// wire at rest, so what is left there is the head's own top, a
    /// minimum-phase one-pole that pulls the centre late, by more of a
    /// sample the higher the rate, because the same corner in hertz is more
    /// samples of delay there: measured +0.020, +0.023 and +0.066 of a sample
    /// at 15 ips and +0.013, +0.016 and +0.051 at 30, at 44.1, 48 and 96 kHz,
    /// where the old picks read 0.73 to 0.82 early. The slower speeds' tops,
    /// 13.1 and 20.3 kHz, are held to the frame only (up to +0.17 at
    /// 3.75 ips at 96 kHz), and so is a fresh tape, whose contour is all
    /// below 100 Hz.
    #[test]
    fn an_impulse_leaves_exactly_when_the_host_is_told_it_will() {
        let mut cases = Vec::new();
        for speed in TapeSpeed::ALL {
            let neutral = TapeParams {
                speed: speed.dial(),
                ..TapeParams::NEUTRAL
            };
            let still = TapeParams {
                wobble: 0.0,
                hiss: 0.0,
                speed: speed.dial(),
                ..TapeParams::default()
            };
            let centred = matches!(speed, TapeSpeed::Ips15 | TapeSpeed::Ips30);
            cases.push((speed.label(), "every colour off", neutral, centred));
            cases.push((speed.label(), "a fresh tape, transport and hiss still", still, false));
        }
        for sr in [44_100.0_f32, 48_000.0, 96_000.0] {
            for &(speed, what, p, centred) in &cases {
                let mut unit = Tape::new(sr);
                let lat = unit.latency_frames(&p);
                let at = sr as usize;
                let mut mono = vec![0.0_f32; at + lat + 4_096];
                mono[at] = 0.5;
                let out = left(&feed(&mut unit, &to_stereo(&mono), &p));
                let peak = loudest(&out);
                assert_eq!(
                    peak,
                    at + lat,
                    "{what} at {speed}, {sr} Hz: the impulse peaked {} frames after it \
                     went in, and the host was told {lat}",
                    peak as i64 - at as i64
                );
                let (a, b, c) = (
                    f64::from(out[peak - 1]),
                    f64::from(out[peak]),
                    f64::from(out[peak + 1]),
                );
                let centre = 0.5 * (a - c) / (a - 2.0 * b + c);
                let bar = if sr > 48_000.0 { 0.1 } else { 0.05 };
                if centred {
                    assert!(
                        centre.abs() < bar,
                        "{what} at {speed}, {sr} Hz: the impulse is centred {centre:+.3} \
                         of a sample off the frame the host was told (bar {bar})"
                    );
                }
            }
        }
    }

    /// **The line-up table is the direct solve.** [`Tape::new`] fills the
    /// table from [`sat_makeup`] and the refused flux's correction
    /// ([`sat_makeup_refused`]), the correction held at its running minimum
    /// from the lowest record level up, and every frame reads between its
    /// points, so the table may only ever be a faster way to the same answer:
    /// at a thousand and one record levels spread evenly in octaves across
    /// [`MAKEUP_D`], it reads within a hundredth of a decibel of the same
    /// thing solved afresh, its running minimum taken on a grid four times
    /// as fine. And its correction never rises with the record level: every
    /// entry's is at or under the one below's, to a ten-thousandth of a
    /// decibel, which is the property that keeps a step up the dial from
    /// being louder. Measured: 0.0021 dB at worst, and the correction holds
    /// -0.30 dB from `d` = 7 up.
    #[test]
    fn the_line_up_table_is_the_direct_solve() {
        let unit = Tape::new(48_000.0);
        let (lo, hi) = (MAKEUP_D.0.log2(), MAKEUP_D.1.log2());
        let fine = 4_000_usize;
        let mut held = 0.0_f64;
        let minimum: Vec<f64> = (0..=fine)
            .map(|j| {
                let d = (lo + (hi - lo) * j as f64 / fine as f64).exp2();
                held = held.min(db(sat_makeup_refused(d) / f64::from(sat_makeup(d))));
                held
            })
            .collect();
        let mut worst = 0.0_f64;
        for k in 0..=1_000_usize {
            let l = lo + (hi - lo) * k as f64 / 1_000.0;
            let table = f64::from(makeup_at(&unit.makeup, l));
            let direct = f64::from(sat_makeup(l.exp2())) * 10.0_f64.powf(minimum[4 * k] / 20.0);
            worst = worst.max(db(table / direct).abs());
        }
        assert!(
            worst < 0.01,
            "the line-up table is {worst:.4} dB from the direct solve"
        );
        let correction: Vec<f64> = (0..MAKEUP_POINTS)
            .map(|i| db(f64::from(unit.makeup[i]) / f64::from(sat_makeup(makeup_d(i)))))
            .collect();
        for (i, w) in correction.windows(2).enumerate() {
            assert!(
                w[1] <= w[0] + 1.0e-4,
                "the line-up's correction rises from {:+.4} to {:+.4} dB at entry {}: \
                 a step up the dial would be louder",
                w[0],
                w[1],
                i + 1
            );
        }
    }

    /// A Butterworth low- or high-pass of even order to measure with: a
    /// cascade of biquads from the analog prototype by the bilinear
    /// transform with the corner prewarped (the cookbook designs), in f64,
    /// so it is the same analog filter at every rate except for the warp of
    /// frequencies far from its corner.
    struct Butter {
        /// Per section: `b0, b1, b2`, `a1, a2` (normalised), and the
        /// transposed direct form's two states.
        sections: Vec<([f64; 3], [f64; 2], [f64; 2])>,
    }

    impl Butter {
        fn new(order: usize, sr: f64, hz: f64, high: bool) -> Self {
            let (sn, cs) = (std::f64::consts::TAU * hz / sr).sin_cos();
            let sections = (0..order / 2)
                .map(|k| {
                    let angle = std::f64::consts::PI * (2 * k + 1) as f64 / (2 * order) as f64;
                    let alpha = sn * angle.sin();
                    let a0 = 1.0 + alpha;
                    let b = if high {
                        [(1.0 + cs) / 2.0, -(1.0 + cs), (1.0 + cs) / 2.0]
                    } else {
                        [(1.0 - cs) / 2.0, 1.0 - cs, (1.0 - cs) / 2.0]
                    };
                    (
                        [b[0] / a0, b[1] / a0, b[2] / a0],
                        [-2.0 * cs / a0, (1.0 - alpha) / a0],
                        [0.0; 2],
                    )
                })
                .collect();
            Self { sections }
        }

        fn run(&mut self, x: f64) -> f64 {
            let mut v = x;
            for (b, a, z) in &mut self.sections {
                let y = b[0] * v + z[0];
                z[0] = b[1] * v - a[0] * y + z[1];
                z[1] = b[2] * v - a[1] * y;
                v = y;
            }
            v
        }
    }

    /// The mean power of `buf` between `lo` and `hi` hertz, eighth-order
    /// skirts each side, past `skip` samples of the filters settling.
    fn band_power(buf: &[f32], sr: f64, lo: f64, hi: f64, skip: usize) -> f64 {
        let mut hp = Butter::new(8, sr, lo, true);
        let mut lp = Butter::new(8, sr, hi, false);
        let mut sum = 0.0;
        for (i, x) in buf.iter().enumerate() {
            let y = lp.run(hp.run(f64::from(*x)));
            if i >= skip {
                sum += y * y;
            }
        }
        sum / (buf.len() - skip) as f64
    }

    /// **The same machine runs at every sample rate.**
    ///
    /// Every voicing number here is in hertz or seconds so that a 96 kHz
    /// session hears the machine a 44.1 kHz one does. Noise is where that is
    /// easiest to get wrong, because white noise of a fixed sample variance
    /// spreads the same power over more hertz at a higher rate, and the
    /// hiss's correction for it was the wrong way up: the tape was 0.74 dB
    /// quieter at 48 kHz and 6.8 dB quieter at 96 than at 44.1.
    ///
    /// So the hiss alone on silence, at the top of its dial with every colour
    /// off, is measured in the octave around 1 kHz, where the ear meets it,
    /// and must read within half a decibel at 48 and at 96 kHz of what it
    /// reads at 44.1. Four seconds of it at each rate pin the band power to a
    /// few hundredths of a decibel. Measured: +0.02 and +0.05 dB; with the
    /// root the wrong way up, -0.72 and -6.71.
    #[test]
    fn the_same_machine_runs_at_every_sample_rate() {
        let p = TapeParams {
            hiss: 1.0,
            ..TapeParams::NEUTRAL
        };
        let octave = |sr: f32| {
            let frames = (sr * 4.0) as usize;
            let out = left(&feed(&mut Tape::new(sr), &vec![0.0; frames * 2], &p));
            10.0 * band_power(&out, sr.into(), 700.0, 1_400.0, (sr * 0.1) as usize).log10()
        };
        let at_44k = octave(44_100.0);
        let offs = [48_000.0_f32, 96_000.0].map(|sr| (sr, octave(sr) - at_44k));
        for (sr, off) in offs {
            assert!(
                off.abs() < 0.5,
                "the hiss around 1 kHz reads {off:+.2} dB at {sr} Hz against 44.1 kHz \
                 (every rate, in dB: {offs:?})"
            );
        }
    }

    /// **The scrape sprays the same sidebands at every sample rate.**
    ///
    /// The twin of the hiss's fault: the scrape is white noise through a
    /// band, swaying the head, and its depth carried no correction at all for
    /// the rate it was drawn at, so the head scraped 0.37 dB less at 48 kHz
    /// and 3.4 dB less at 96 than at 44.1. Scrape is heard, as far as it is
    /// heard, as the grain it sprays around a tone: the sidebands a moving
    /// head writes onto anything playing through it, hundreds of hertz to a
    /// few kilohertz either side.
    ///
    /// So a 5 kHz tone through full wobble, everything else off, is shifted
    /// down to 0 Hz and the band 600 Hz to 4 kHz either side of it measured
    /// against the tone. The wow and flutter sit within 30 Hz of the tone
    /// and the shift's image twice the tone away, far outside the band, so
    /// what is measured is the scrape's own spray, 54 dB under the tone, and
    /// it must read the same within a quarter of a decibel at 48 and at
    /// 96 kHz as at 44.1. Measured: +0.01 and -0.04 dB; without the root,
    /// -0.35 and -3.41.
    #[test]
    fn the_scrape_sprays_the_same_sidebands_at_every_sample_rate() {
        const TONE: f64 = 5_000.0;
        let p = TapeParams {
            wobble: 1.0,
            ..TapeParams::NEUTRAL
        };
        let spray = |sr: f32| {
            let frames = (sr * 3.0) as usize;
            let mono: Vec<f32> = (0..frames)
                .map(|i| 0.25 * (std::f64::consts::TAU * TONE * i as f64 / f64::from(sr)).sin() as f32)
                .collect();
            let out = left(&feed(&mut Tape::new(sr), &to_stereo(&mono), &p));
            let sr = f64::from(sr);
            let w = std::f64::consts::TAU * TONE / sr;
            let mut filters = [0, 1].map(|_| (Butter::new(8, sr, 600.0, true), Butter::new(16, sr, 4_000.0, false)));
            let skip = (sr * 0.5) as usize;
            let (mut sides, mut tone) = (0.0_f64, 0.0_f64);
            for (n, y) in out.iter().enumerate() {
                let y = f64::from(*y);
                let (s, c) = (w * n as f64).sin_cos();
                let mut power = 0.0;
                for ((hp, lp), x) in filters.iter_mut().zip([y * c, -y * s]) {
                    let v = lp.run(hp.run(x));
                    power += v * v;
                }
                if n >= skip {
                    sides += power;
                    tone += y * y;
                }
            }
            // The shift halves both the tone's power and the sidebands', so
            // twice what passed the band, over the tone, is their ratio.
            10.0 * (2.0 * sides / tone).log10()
        };
        let at_44k = spray(44_100.0);
        let offs = [48_000.0_f32, 96_000.0].map(|sr| (sr, spray(sr) - at_44k));
        for (sr, off) in offs {
            assert!(
                off.abs() < 0.25,
                "the scrape's sidebands read {off:+.2} dB at {sr} Hz against 44.1 kHz \
                 (every rate, in dB: {offs:?})"
            );
        }
    }

    /// **No knob on this unit may be a decoration.**
    ///
    /// The generic net under the specific tests above: sweep each control
    /// from a fresh tape's setting to each end of its travel and demand
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
        // Each floor is 3 dB under what the knob measured from a fresh
        // tape, rounded down to the whole decibel; the measured figure is
        // beside it.
        let knobs: [(&str, fn(&mut TapeParams, f32), f64); 7] = [
            // -3.8 dB.
            ("drive", |p, v| p.drive = v, -7.0),
            // -36.4 dB: at 15 ips the contour's bell sits near 35 Hz, a
            // thin slice of broadband noise; the bump's own size is pinned
            // in hertz by its octave test and by the sample-rate test.
            ("bump", |p, v| p.bump = v, -40.0),
            // -2.6 dB.
            ("roll", |p, v| p.roll = v, -6.0),
            // +2.8 dB.
            ("wobble", |p, v| p.wobble = v, -1.0),
            // -38.5 dB. Hiss is meant to sit under the music. Its shape and
            // its ducking are pinned by their own test; this only says it
            // is there.
            ("hiss", |p, v| p.hiss = v, -42.0),
            // +9.5 dB.
            ("trim", |p, v| p.trim = v, 6.0),
            // -5.0 dB: 15 ips to either end of the selector.
            ("speed", |p, v| p.speed = v, -9.0),
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

    /// A sine at `hz` of peak `amp`, through a fresh unit at `p`: its gain
    /// through the tape in dB, Hann-measured in the window from `settle` to
    /// `settle + measure` seconds, output against input. With `harmonic`,
    /// that harmonic's level against the output's fundamental instead.
    fn tone_through(p: &TapeParams, sr: f32, hz: f64, amp: f32, settle: f32, measure: f32, harmonic: Option<f64>) -> f64 {
        let frames = (sr * (settle + measure)) as usize;
        let mono: Vec<f32> = (0..frames)
            .map(|i| amp * (std::f64::consts::TAU * hz * i as f64 / f64::from(sr)).sin() as f32)
            .collect();
        let out = left(&feed(&mut Tape::new(sr), &to_stereo(&mono), p));
        let (lo, hi) = ((sr * settle) as usize, frames);
        let fund = band(&out[lo..hi], sr.into(), hz);
        match harmonic {
            Some(h) => db(band(&out[lo..hi], sr.into(), hz * h) / fund.max(1.0e-30)),
            None => db(fund / band(&mono[lo..hi], sr.into(), hz)),
        }
    }

    /// 0 VU, as a sine's peak: the RMS [`ALIGN_DBFS`] names, times root two.
    fn zero_vu() -> f32 {
        10.0_f32.powf(ALIGN_DBFS as f32 / 20.0) * std::f32::consts::SQRT_2
    }

    /// **The head bump moves an octave with every doubling of speed.**
    ///
    /// The bump is a wavelength on the tape, 10.9 mm, so what it adds at a
    /// frequency at one speed it adds at twice the frequency at twice the
    /// speed. Bump 1 against Bump 0 cancels everything else in the chain (the
    /// transformer, the head's own roll-off, the top), leaving the contour's
    /// two bells: measured at the bump's own frequency and an octave above it
    /// at every speed, each pair of speeds an octave apart agrees within
    /// 0.15 dB, and the bump adds 4.2 to 4.9 dB at its frequency and takes
    /// 0.8 to 1.5 dB an octave above (model +4.53 and -1.13: the +5 dB bell
    /// less the dip's skirt, and the dip less the bell's). Measured +4.535
    /// and -1.132 at every speed.
    ///
    /// **And with the bump at 0 the low end is the head's alone.** The NAB
    /// record shelf lifts the bass by up to 4 dB onto the tape and its
    /// inverse must take exactly that back off, so a quiet tone at the
    /// bump's frequency and an octave above it reads what the input
    /// transformer and the head's own roll-off give it, two one-poles whose
    /// response is known exactly, within 0.1 dB at every speed.
    #[test]
    fn the_head_bump_moves_an_octave_with_every_doubling_of_speed() {
        let sr = 48_000.0_f32;
        let amp = 0.1_f32;
        let at = |speed: TapeSpeed| {
            let f_b = 0.0254 * speed.ips() / (BUMP_WAVELENGTH_MM * 1.0e-3);
            [f_b, 2.0 * f_b].map(|hz| {
                let gain = |bump: f32| {
                    let p = TapeParams {
                        bump,
                        speed: speed.dial(),
                        ..TapeParams::NEUTRAL
                    };
                    tone_through(&p, sr, hz, amp, 1.0, 2.0, None)
                };
                // A trapezoidal one-pole high-pass at `fc` passes
                // `t / sqrt(t^2 + g^2)`, `t = tan(pi f / rate)`.
                let high = |fc: f64| {
                    let (t, g) = (
                        (std::f64::consts::PI * hz / f64::from(sr)).tan(),
                        (std::f64::consts::PI * fc / f64::from(sr)).tan(),
                    );
                    db(t / (t * t + g * g).sqrt())
                };
                let head = high(TRANSFORMER_HZ) + high((HEAD_LF_SHARE * f_b).max(HEAD_LF_FLOOR_HZ));
                let flat = gain(0.0);
                assert!(
                    (flat - head).abs() < 0.1,
                    "at {} a quiet {hz:.1} Hz tone reads {flat:+.2} dB with the bump off, \
                     where the transformer and the head's roll-off give {head:+.2}: the \
                     record shelf did not come off",
                    speed.label()
                );
                gain(1.0) - flat
            })
        };
        let adds: Vec<[f64; 2]> = TapeSpeed::ALL.iter().map(|v| at(*v)).collect();
        for (speed, [bump, dip]) in TapeSpeed::ALL.iter().zip(&adds) {
            assert!(
                (4.2..=4.9).contains(bump),
                "at {} the bump adds {bump:+.2} dB at its own frequency",
                speed.label()
            );
            assert!(
                (-1.5..=-0.8).contains(dip),
                "at {} the bump adds {dip:+.2} dB an octave above it",
                speed.label()
            );
        }
        for (k, w) in adds.windows(2).enumerate() {
            for j in 0..2 {
                assert!(
                    (w[0][j] - w[1][j]).abs() < 0.15,
                    "the bump's {} reads {:+.2} dB at {} and {:+.2} dB an octave up at {}: \
                     it did not move with the speed",
                    ["peak", "dip"][j],
                    w[0][j],
                    TapeSpeed::ALL[k].label(),
                    w[1][j],
                    TapeSpeed::ALL[k + 1].label()
                );
            }
        }
    }

    /// **A speed change glides like a servo relocking.**
    ///
    /// A studio transport re-locks to a new speed in about a third of a
    /// second, and the voicing follows the same way: 20 ms after a switch
    /// from 15 to 3.75 ips the glide is under 30 % of the way there in octaves
    /// and by 0.6 s within 0.5 % of it. And everything per-speed follows it
    /// to the end: 0.6 s after a switch from 15 to 7.5 ips, two quiet tones
    /// at 1 kHz and 10 kHz (Drive 0, so self-erasure is a wire and only the
    /// voicing is measured) read within 0.05 dB of a unit that ran at
    /// 7.5 ips all along. Every per-speed constant is blended in `log2` of
    /// the speed, so there is no corner in that path that lags behind the
    /// rest. Measured: 18 % of the way at 20 ms, 3.763 ips at 0.6 s, and
    /// +0.0000 and +0.0002 dB.
    #[test]
    fn a_speed_change_glides_like_a_servo_relocking() {
        let sr = 48_000.0_f32;
        let at = |speed: TapeSpeed| TapeParams {
            speed: speed.dial(),
            ..TapeParams::NEUTRAL
        };
        let mut unit = Tape::new(sr);
        let mut silence = vec![0.0_f32; 480 * 2];
        unit.process(&mut silence, 480, 2, &at(TapeSpeed::Ips15));
        let mut quiet = vec![0.0_f32; 960 * 2];
        unit.process(&mut quiet, 960, 2, &at(TapeSpeed::Ips3_75));
        let way = |v: f64| (15.0 / v).log2() / 2.0;
        let early = way(unit.speed_now());
        assert!(
            (0.0..0.3).contains(&early),
            "20 ms after 15 -> 3.75 ips the glide is {:.0} % of the way there",
            early * 100.0
        );
        let mut rest = vec![0.0_f32; 27_840 * 2];
        unit.process(&mut rest, 27_840, 2, &at(TapeSpeed::Ips3_75));
        let off = (unit.speed_now() / 3.75).ln().abs();
        assert!(
            off < 1.005_f64.ln(),
            "0.6 s after 15 -> 3.75 ips the speed is {:.3} ips",
            unit.speed_now()
        );

        // Two quiet tones through a switch, against a unit that never moved.
        let amp = 10.0_f32.powf(-30.0 / 20.0);
        let frames = (sr * 1.5) as usize;
        let mono: Vec<f32> = (0..frames)
            .map(|i| {
                let t = i as f64 / f64::from(sr);
                (f64::from(amp) * ((std::f64::consts::TAU * 1_000.0 * t).sin()
                    + (std::f64::consts::TAU * 10_000.0 * t).sin())) as f32
            })
            .collect();
        let input = to_stereo(&mono);
        let switch = (sr * 0.5) as usize;
        let mut moved = Tape::new(sr);
        let mut a = input[..switch * 2].to_vec();
        moved.process(&mut a, switch, 2, &at(TapeSpeed::Ips15));
        let mut b = input[switch * 2..].to_vec();
        let n = frames - switch;
        let mut done = 0;
        while done < n {
            let k = (n - done).min(512);
            moved.process(&mut b[done * 2..(done + k) * 2], k, 2, &at(TapeSpeed::Ips7_5));
            done += k;
        }
        let moved: Vec<f32> = left(&[a, b].concat());
        let still = left(&feed(&mut Tape::new(sr), &input, &at(TapeSpeed::Ips7_5)));
        let (lo, hi) = (switch + (sr * 0.6) as usize, frames);
        for hz in [1_000.0, 10_000.0] {
            let gap = db(band(&moved[lo..hi], sr.into(), hz) / band(&still[lo..hi], sr.into(), hz));
            assert!(
                gap.abs() < 0.05,
                "0.6 s after 15 -> 7.5 ips, {hz} Hz reads {gap:+.3} dB from a 7.5 ips tape"
            );
        }
    }

    /// **Slower tape saturates sooner, by its published headroom.**
    ///
    /// A faster tape writes the same flux over a longer wavelength and takes
    /// more of it before its 3 % point: the datasheets put 15 ips 2.0 to
    /// 2.5 dB over 7.5. So the same 0 VU tone at the same Drive carries more
    /// third harmonic the slower the tape: a 220 Hz tone at Drive 0.6 reads
    /// 1.4 to 2.1 times the HD3 at 7.5 ips that it does at 15, the same again
    /// from 7.5 to 3.75, and a little less at 30 than at 15 (model 1.70, 1.69
    /// and 0.85; measured 1.68, 1.67 and 0.86, from 2.00, 1.20, 0.71 and
    /// 0.61 % at 3.75, 7.5, 15 and 30 ips).
    #[test]
    fn slower_tape_saturates_sooner_by_its_published_headroom() {
        let sr = 48_000.0_f32;
        let hd3 = |speed: TapeSpeed| {
            let p = TapeParams {
                drive: 0.6,
                speed: speed.dial(),
                ..TapeParams::NEUTRAL
            };
            10.0_f64.powf(tone_through(&p, sr, 220.0, zero_vu(), 0.3, 0.6, Some(3.0)) / 20.0)
        };
        let [s3, s7, s15, s30] = TapeSpeed::ALL.map(hd3);
        for (what, ratio, range) in [
            ("7.5 over 15 ips", s7 / s15, 1.4..=2.1),
            ("3.75 over 7.5 ips", s3 / s7, 1.4..=2.1),
            ("30 over 15 ips", s30 / s15, 0.8..=1.0),
        ] {
            assert!(
                range.contains(&ratio),
                "HD3 of a 0 VU tone at Drive 0.6, {what}: {ratio:.2} \
                 ({:.2} %, {:.2} %, {:.2} %, {:.2} % at 3.75, 7.5, 15 and 30 ips)",
                s3 * 100.0,
                s7 * 100.0,
                s15 * 100.0,
                s30 * 100.0
            );
        }
    }

    /// **Each speed tops out where studio machines are specified.**
    ///
    /// A studio two-track's response is quoted within 2 dB to 10, 15.5, 21
    /// and 25 kHz at 3.75, 7.5, 15 and 30 ips, and each speed's own top is a
    /// one-pole matched to it ([`HEAD_TOP_EDGE_HZ`]). So with every colour
    /// off a quiet 10 kHz tone, against 1 kHz, loses 1.5 to 2.5 dB at
    /// 3.75 ips, 0.5 to 1.4 at 7.5, 0.2 to 0.9 at 15 and 0.1 to 0.7 at 30
    /// (model -2.00, -0.95, -0.54 and -0.39; measured -1.97, -0.93, -0.53
    /// and -0.38).
    #[test]
    fn each_speed_tops_out_where_studio_machines_are_specified() {
        let sr = 48_000.0_f32;
        let amp = 10.0_f32.powf(-30.0 / 20.0);
        for (speed, range) in TapeSpeed::ALL.into_iter().zip([
            -2.5..=-1.5,
            -1.4..=-0.5,
            -0.9..=-0.2,
            -0.7..=-0.1,
        ]) {
            let p = TapeParams {
                speed: speed.dial(),
                ..TapeParams::NEUTRAL
            };
            let top = tone_through(&p, sr, 10_000.0, amp, 0.2, 0.3, None)
                - tone_through(&p, sr, 1_000.0, amp, 0.2, 0.3, None);
            assert!(
                range.contains(&top),
                "at {} a quiet 10 kHz tone reads {top:+.2} dB against 1 kHz",
                speed.label()
            );
        }
    }

    /// The input level, in dB re 0 VU, at which a tone at `hz` through `p`
    /// comes out 1 dB under its own small-signal gain: its 1 dB compression
    /// point, by bisection.
    fn compression_point(p: &TapeParams, sr: f32, hz: f64) -> f64 {
        let gain = |level_db: f64| {
            let amp = zero_vu() * 10.0_f32.powf(level_db as f32 / 20.0);
            tone_through(p, sr, hz, amp, 0.1, 0.05, None)
        };
        let small = gain(-30.0);
        let (mut lo, mut hi) = (-20.0_f64, 30.0_f64);
        for _ in 0..16 {
            let mid = 0.5 * (lo + hi);
            if gain(mid) > small - 1.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    /// **Slower tape saturates its treble first, as the datasheets say.**
    ///
    /// A tape's saturation at 10 kHz comes in sooner than at 1 kHz, and much
    /// sooner on slow tape, where 10 kHz is a short wavelength that
    /// self-erasure takes first: the datasheets put the 10 kHz ceiling 7.5 to
    /// 9.5 dB under the 1 kHz one at 7.5 ips, 1.5 to 2 at 15 and 1 to 1.5 at
    /// 30. So at Drive 0.6 the 10 kHz 1 dB compression point sits 6.5 to
    /// 9.5 dB under 1 kHz's at 7.5 ips, 0.5 to 3 under at 15, at most 2.5
    /// under (and a quarter over) at 30, and at least 8 under at 3.75, in that
    /// order (model -8.26, -1.86, -0.88 and -10.40; measured -8.14, -1.85,
    /// -0.88 and -10.21), which is the self-erasure fit [`ERASE_TOP_HZ`]
    /// states.
    #[test]
    fn slower_tape_saturates_its_treble_first_as_the_datasheets_say() {
        let sr = 48_000.0_f32;
        let gap: Vec<f64> = TapeSpeed::ALL
            .iter()
            .map(|v| {
                let p = TapeParams {
                    drive: 0.6,
                    speed: v.dial(),
                    ..TapeParams::NEUTRAL
                };
                compression_point(&p, sr, 10_000.0) - compression_point(&p, sr, 1_000.0)
            })
            .collect();
        for (k, range) in [(1, -9.5..=-6.5), (2, -3.0..=-0.5), (3, -2.5..=0.25)] {
            assert!(
                range.contains(&gap[k]),
                "at {} the 10 kHz compression point is {:+.2} dB from 1 kHz's",
                TapeSpeed::ALL[k].label(),
                gap[k]
            );
        }
        assert!(gap[0] <= -8.0, "at 3.75 ips the 10 kHz compression point is only {:+.2} dB under 1 kHz's", gap[0]);
        assert!(
            gap[0] < gap[1] && gap[1] < gap[2] && gap[2] <= gap[3],
            "the 10 kHz compression points are not ordered by speed: {gap:.2?} dB"
        );
    }

    /// **The line-up tone comes back at its level at every speed.**
    ///
    /// A machine is lined up with a 1 kHz tone at 0 VU, and the line-up's
    /// whole job is that tone: from the bottom of the dial to the Drive a
    /// fresh tape opens at and a tenth past it, at all four speeds, it comes
    /// back within 0.35 dB of where it went in (model 0.23 at most; measured
    /// -0.24 dB at 3.75 ips at Drive 0.6, and 0.05 or under at the other
    /// three). What is left over is the self-erasure the slow speeds already
    /// do at that level, and the refused flux's lift past where its
    /// correction holds.
    #[test]
    fn the_line_up_tone_comes_back_at_its_level_at_every_speed_and_bias() {
        let sr = 48_000.0_f32;
        let amp = 10.0_f32.powf(-18.0 / 20.0) * std::f32::consts::SQRT_2;
        for speed in TapeSpeed::ALL {
            for step in 0..=6 {
                let p = TapeParams {
                    drive: step as f32 / 10.0,
                    speed: speed.dial(),
                    ..TapeParams::NEUTRAL
                };
                let level = tone_through(&p, sr, 1_000.0, amp, 0.2, 0.2, None);
                assert!(
                    level.abs() < 0.35,
                    "at {} Drive {} the 0 VU line-up tone comes back {level:+.3} dB",
                    speed.label(),
                    step as f32 / 10.0
                );
            }
        }
    }
}
