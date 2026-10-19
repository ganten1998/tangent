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
//! NAB record shelf, and under the aligned bias the under-bias low shelf →
//! record, at four times the session rate: a bias-linearised tanh with the
//! refused flux fed back → the repro line-up → both shelves' exact inverses →
//! the bias's own treble shelf ([`BIAS_RANGE_DB`]) → HF self-erasure (the
//! envelope-driven corner above) → the tape: a short Hermite-interpolated play head swayed by
//! the transport's parts ([`Transport`]: Wow's reel, pinch roller and drift,
//! Flutter's guides, capstan, the tape's own resonance and a whisper of
//! scrape, their depths noise-modulated and the head's travel shared out
//! between them by a governor), and on each track the tape's own modulation
//! noise →
//! the head's own top → the hiss, white behind the speed's playback EQ → the
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
    /// **Wow**: the slow half of the transport's sway, the supply reel, the
    /// pinch roller and the servo's drift ([`ROTOR_HZ_PER_IPS`],
    /// [`DRIFT_MS`]). The symbol is 5.0's `wobble`, which drove wow and
    /// flutter together; it drives the wow alone now, and Flutter the rest.
    /// 0.3 is the specified machine, linear from there. On the deep head
    /// ([`WowRange`]) it is the cassette's depth instead, linear too, 3 % of
    /// the speed at the top. Wow and Flutter at 0 stop the transport, and the
    /// head then moves only by the tape's own modulation noise, which is the
    /// hiss's ([`JITTER_UM`]).
    pub wobble: f32,
    /// Tape hiss, and the modulation noise that rides the music. 0 is silent
    /// tape, which no real reel ever was; see [`HISS_GAIN`] and
    /// [`JITTER_UM`]. The hiss is the tape's, under the record level, so it
    /// falls as Drive rises and rises as Drive falls ([`HISS_DRIVE`]).
    pub hiss: f32,
    /// Output trim. 0.5 is unity; see [`TRIM_DB`].
    pub trim: f32,
    /// The tape speed, a selector of four ([`TapeSpeed`]). A fresh tape runs
    /// at 15 ips.
    pub speed: f32,
    /// **Flutter**: the fast half of the transport's sway, the tension
    /// guides, the capstan with its second harmonic, the tape's own 44 Hz
    /// resonance and the head's scrape. 0.3 is the specified machine, on a
    /// squared taper.
    pub flutter: f32,
    /// **Rate**: how fast every rotating part turns, from half to twice its
    /// own speed across the dial ([`rate_multiplier`]). 0.5 is the machine
    /// as built. The resonance, the scrape and the drift keep their own
    /// time.
    pub rate: f32,
    /// **Wow Range**, a selector of two ([`WowRange`]): the studio
    /// transport's wow on the studio head, or a worn cassette's on the deep
    /// head, [`DEEP_MS`] back. The only control that moves the delay the
    /// host is told. A fresh tape opens on the studio head.
    pub wow_range: f32,
    /// **Bias**: the record bias, from [`BIAS_RANGE_DB`] under the aligned
    /// point to as far over it, the machine lined up again at 1 kHz after
    /// every change, as a real one is. 0.5 is aligned, at the recommended
    /// over-bias. Under-bias brightens the top, saturates the bass sooner
    /// and raises the modulation noise; over-bias dulls the top. See
    /// [`bias_shelf_db`] and [`UNDER_BIAS_UM`].
    pub bias: f32,
    /// **Age**: an old tape on a worn head. The head-to-tape spacing grows
    /// ([`AGE_SPACING_UM`]), so the treble goes first and fastest on slow
    /// tape; the head's gap skews against the tracks ([`AZIMUTH_URAD`]),
    /// pulling the two apart in time; the worn head records as if
    /// over-biased; a magnetised head adds a second harmonic
    /// ([`MAGNETISED_H2`]); and the hiss and the modulation noise rise. 0 is
    /// new tape on new heads, and a fresh tape opens there.
    pub age: f32,
    /// **Damage**: what the tape itself has been through, laid along it per
    /// metre: dropouts where the oxide is shed or dirt lifts the tape off
    /// the head, creases that ripple it and twist its azimuth, and worn
    /// edges that never quite touch ([`DROPOUTS_PER_M`]). A defect is a
    /// stretch of tape, so a slow tape plays each one longer and meets
    /// fewer a second. 0 is an unplayed reel, and a fresh tape opens there.
    pub damage: f32,
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
            // A studio transport inside its published spec: with Wow at
            // 0.3, 0.04 % weighted peak at 15 ips.
            flutter: 0.3,
            // The parts turning at their own speed.
            rate: 0.5,
            // The studio head.
            wow_range: WowRange::Studio.dial(),
            // Aligned: the recommended over-bias, where the datasheets
            // quote the tape.
            bias: 0.5,
            // New tape on new heads.
            age: 0.0,
            // An unplayed reel.
            damage: 0.0,
        }
    }
}

#[cfg(test)]
impl TapeParams {
    /// Every colour off: the record level at the bottom of its travel, no
    /// bump, no roll, a parked head (no wow, no flutter), silent tape, unity
    /// trim, at the studio speed and the parts' own rate, the bias aligned,
    /// new tape, never played. The recipe the neutrality tests here and at the seat spell
    /// out, in one place.
    pub(crate) const NEUTRAL: TapeParams = TapeParams {
        drive: 0.0,
        bump: 0.0,
        roll: 0.0,
        wobble: 0.0,
        hiss: 0.0,
        trim: 0.5,
        speed: TapeSpeed::Ips15.dial(),
        flutter: 0.0,
        rate: 0.5,
        wow_range: WowRange::Studio.dial(),
        bias: 0.5,
        age: 0.0,
        damage: 0.0,
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

/// **The wow range**, a selector of two: which play head the tape plays from,
/// and so which wow it carries.
///
/// The studio head sits [`BASE_MS`] behind the record head and carries a
/// studio transport's wow; the deep head sits [`DEEP_MS`] back and carries a
/// worn cassette's warble instead. Flutter rides both. Switching crossfades
/// the heads over [`HEAD_FADE_MS`]; the latency the host is told follows
/// the selection at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WowRange {
    Studio,
    Deep,
}

/// How many positions the wow range has. A literal so `builtin_fx`'s table
/// can stay `const`; a test there pins it to [`WowRange::ALL`].
pub const WOW_RANGE_STEPS: i32 = 2;

impl WowRange {
    /// Selector order, frozen: append-only, like the ids.
    pub const ALL: [WowRange; 2] = [WowRange::Studio, WowRange::Deep];

    /// What the panel calls it.
    pub fn label(self) -> &'static str {
        match self {
            WowRange::Studio => "studio",
            WowRange::Deep => "deep",
        }
    }

    /// Decode the normalised dial: nearest of the two positions wins.
    pub fn from_dial(x: f32) -> Self {
        let last = (Self::ALL.len() - 1) as f32;
        let at = ((x.clamp(0.0, 1.0) * last).round() as usize).min(Self::ALL.len() - 1);
        Self::ALL[at]
    }

    /// Where this range sits on the 0..=1 dial: the inverse of
    /// [`WowRange::from_dial`].
    pub const fn dial(self) -> f32 {
        self as usize as f32 / (WowRange::ALL.len() - 1) as f32
    }

    /// How far behind the record head this range's play head rests, in
    /// milliseconds.
    fn head_ms(self) -> f32 {
        match self {
            WowRange::Studio => BASE_MS,
            WowRange::Deep => DEEP_MS,
        }
    }
}

/// **The delay a tape built at `sample_rate` tells the host, in
/// milliseconds**, at a wow range: the frames [`Tape::latency_frames`]
/// reports there, its head's rest and the record stage's oversampler
/// together, over the rate. 2 ms on the studio head and 25 ms on the deep
/// one at every rate an engine runs at; a pure function, so a face can print
/// it from the dials alone.
pub fn latency_ms(range: WowRange, sample_rate: f32) -> f32 {
    let frames = head_rest(sample_rate, range.head_ms()) + OS_LATENCY;
    let rate = if sample_rate > 0.0 { sample_rate } else { 8_000.0 };
    frames as f32 * 1_000.0 / rate
}

/// **Rate, as the multiple of its own speed every rotating part turns at**:
/// `2^(2x - 1)`, half at the bottom of the dial, twice at the top, the
/// machine as built at the middle. An octave either way spans a reel from
/// full to empty. Depths stay fractions of the speed, so a slower turn
/// sways the head further for the same pitch.
pub fn rate_multiplier(x: f32) -> f32 {
    (2.0 * norm_or(x, 0.5) - 1.0).exp2()
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

/// The record level a Drive dial position asks for, as `log2 d`: the
/// geometric sweep of [`DRIVE_TANH`], the one place it is spelled.
fn dial_log2_d(drive: f32) -> f64 {
    f64::from(DRIVE_TANH.0).log2() + f64::from(drive) * f64::from(DRIVE_TANH.1 / DRIVE_TANH.0).log2()
}

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

/// **Bias, as decibels of record bias either side of the aligned point**:
/// `b = (2x - 1) 6` across the dial, 6 dB under at the bottom and 6 dB over
/// at the top, the span from beyond the 10 kHz sensitivity's peak to well
/// down its over-bias flank.
///
/// **Bias moves the tape's curve, not the record level.** The datasheets
/// agree on three things: under-bias lowers the tape's maximum output about
/// 1.4 dB for every dB it is under; it lowers it at long wavelengths and
/// leaves the short ones their own headroom, or a little more; and the
/// small-signal treble rises with under-bias and falls with over-bias. So
/// bias drives neither the record curve's `d` nor the self-erasure, which
/// would lower every frequency's ceiling and deepen the treble's loss. It
/// acts through two stages and the line-up instead. Under the aligned
/// point, a low shelf before the curve and its exact inverse after it
/// ([`UNDER_BIAS_UM`]) put the long wavelengths into the curve hotter: a
/// lower ceiling for the bass, and nothing at all at small signal. Either
/// side of it, a first-order shelf after the curve ([`BIAS_SHELF_UM`],
/// [`bias_shelf_db`]) is the small-signal treble law. And the machine is
/// lined up again at 1 kHz after every change, as a real one is: the shelf
/// is unity there, and the line-up reads the record level the 1 kHz tone
/// actually meets through the low shelf.
pub const BIAS_RANGE_DB: f32 = 6.0;

/// The under-bias low shelf: the wavelength of its half-height, in
/// micrometres, and how many decibels of long-wavelength headroom each
/// decibel of under-bias takes.
///
/// A second-order low shelf (Q of a half root two) whose height is 1.4 dB
/// per dB under the aligned point and whose half-height sits at 85 µm, the
/// geometric middle of the two wavelengths the datasheets pin: the whole
/// drop at 190 µm (500 Hz at 3.75 ips) and none at 38.1 µm (10 kHz at
/// 15 ips): 8.0 of its 8.4 dB at 190 µm and 0.4 at 38.1 µm at full
/// under-bias. Second order because a first-order shelf would still lift
/// 38.1 µm by a decibel and a half into the curve, where the treble must
/// keep its own headroom. Before the curve and exactly undone after it,
/// so it is transparent at small signal and a lower ceiling at large.
const UNDER_BIAS_UM: f64 = 85.0;
const UNDER_BIAS_DB_PER_DB: f64 = 1.4;

/// The small-signal bias shelf's corner, as a wavelength in micrometres:
/// 2.5, 5, 10 and 20 kHz at the four speeds, where the published bias
/// ladders put the 10 kHz sensitivity's swing.
const BIAS_SHELF_UM: f64 = 38.1;

/// How far the tape's modulation noise rises for each dB of under-bias, in
/// dB: an under-biased tape's grain is coarser ([`JITTER_UM`]).
const UNDER_BIAS_JITTER_DB: f64 = 2.1;

/// **The bias shelf's height at its corner, in dB, for a bias `b` dB re
/// aligned**: what the treble does against a long wavelength as the bias
/// moves, fitted to the published bias ladders.
///
/// Over the aligned point the treble falls 0.8 dB per dB, a little faster
/// as it goes (`-0.8 b - 0.07 b^2`): at 15 ips 10 kHz loses 0.8 dB per dB
/// and 16 kHz more. Under it, the treble rises to its peak, 3.0 dB at
/// 5.4 dB under, along a cubic that leaves aligned at the same slope and
/// arrives at the peak flat; past the peak it falls again (`3.0 - 0.5 (b +
/// 5.4)^2`), the far flank where tape that far under-biased gives its top
/// back to distortion. Zero at aligned, where the shelf is a wire. The
/// shelf's top is twice this, and its corner is [`BIAS_SHELF_UM`], so the
/// peak-to-aligned swing of 10 kHz against a long wavelength is 5.6, 4.8,
/// 3.0 and 1.2 dB at 3.75, 7.5, 15 and 30 ips, inside every published
/// ladder.
fn bias_shelf_db(b: f64) -> f64 {
    const PEAK_DB: f64 = 3.0;
    const PEAK_AT: f64 = -5.4;
    const SLOPE: f64 = -0.8;
    if b >= 0.0 {
        SLOPE * b - 0.07 * b * b
    } else if b >= PEAK_AT {
        let t = (b - PEAK_AT) / -PEAK_AT;
        PEAK_DB * (2.0 * t * t * t - 3.0 * t * t + 1.0) + SLOPE * -PEAK_AT * (t * t * t - t * t)
    } else {
        PEAK_DB - 0.5 * (b - PEAK_AT).powi(2)
    }
}

/// The bias the dial asks for, in dB re aligned ([`BIAS_RANGE_DB`]); a NaN
/// reads as aligned.
fn bias_db(p: &TapeParams) -> f64 {
    f64::from(BIAS_RANGE_DB) * (2.0 * f64::from(norm_or(p.bias, 0.5)) - 1.0)
}

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

/// The play-head line: its capacity and where the two heads sit in it, in
/// milliseconds. The studio head's two milliseconds are enough for its sway
/// to never run it off either end and short enough that the unit stays an
/// insert rather than a slap; the deep head sits [`DEEP_MS`] back, for a
/// cassette's warble. [`Tape::latency_frames`] reports the one the range
/// selects.
///
/// A head's REST position is this less [`OS_LATENCY`], because the record
/// stage's oversampler is already holding that many samples: the two together
/// come to [`BASE_MS`] (or [`DEEP_MS`]) and that is what the host is told.
/// The studio head may sway [`STUDIO_TRAVEL_MS`] either way of its rest,
/// which is more than a fresh machine asks for at any speed, and the deep
/// head [`DEEP_TRAVEL_MS`]; the line holds both at any rate the tape can be
/// built at.
const LINE_MS: f32 = 50.0;
const BASE_MS: f32 = 2.0;

/// **The deep range: a second play head, 25 ms behind the record head**, its
/// own sway a worn cassette's rather than a studio transport's.
///
/// A studio head two milliseconds long cannot carry a cassette's warble,
/// which swings the pitch by percent, not tenths of one: at full Wow the
/// cassette sways the head by over ten milliseconds. So the
/// warble plays from a head of its own, which may sway 22 ms either way of
/// its rest, and only it carries the cassette's parts; Flutter's parts,
/// governed by the studio head's travel, ride both heads, so either can take
/// over at any moment. Switching the range crossfades the heads over
/// [`HEAD_FADE_MS`], and delay compensation hears the deep head's figure
/// only while the range selects it. Its parts' excursions may change by up
/// to [`DEEP_SLEW`] between them, 3 % of the speed, the deep range's own
/// scale: entering it, the warble builds up over about half a second.
const DEEP_MS: f32 = 25.0;
const DEEP_TRAVEL_MS: f64 = 22.0;
const DEEP_SLEW: f64 = 0.03;

/// **The cassette transport**, the deep head's own parts: the tape pack once
/// around at 0.45 Hz and the pinch roller at 1.5 Hz, both times Rate and
/// neither following the tape speed (a cassette has its own), sharing
/// 0.55 and 0.30 of [`CASSETTE_DEPTH`]; and the belt drifting, a
/// sample-and-hold every 2.5 s through two poles at 0.2 Hz, a delay of up
/// to [`BELT_MS`]. **The belt's drift ignores Rate**, as the studio head's
/// drift does: Rate turns the parts that go round, and a drift is the
/// belt's grip wandering, which no turn paces.
///
/// **At full Wow the deep head swings the pitch 3 % at its peak, and the
/// swing is linear in Wow's dial** (the owner's ruling): 0.9 % at the
/// default 0.3, 1.5 % at 0.5. The depth is the one number that puts it
/// there. The pack and the roller come into step every 6.7 s, three turns
/// of one to ten of the other, and their depths then add, 1.93 and 1.05 %
/// at the top of the wander; the wander seldom stands at its top just then,
/// and the belt's drift and Flutter's parts ride on them, so over two
/// minutes at 44.1 kHz the deep head peaks at 3.01 % at full Wow, 1.51 % at
/// 0.5 and 0.91 % at 0.3, the same to within 0.6 % at 48 and 96 kHz, and
/// at 3.02 % over ten minutes
/// (`the_deep_range_swings_the_pitch_like_a_worn_cassette`). So it holds
/// at every Rate: over ten minutes at full Wow the deep head peaks between
/// 3.00 and 3.07 % from half the parts' speed to twice it, and over two
/// minutes at Wow 1 and 0.5 within 4 % of 3 % times Wow at both ends, at
/// 44.1, 48 and 96 kHz. At the default 0.3 the bottom of Rate reads up to
/// 5.3 % over (0.948 % at 44.1 kHz), where Flutter's shared parts, which do
/// not scale with Wow, happen to land on the cassette's own peak (0.912 %
/// without them). While the belt followed Rate, as 5.2.0's did, twice the
/// parts' speed moved its drift twice as fast and bent the pitch twice as
/// far, and this cassette peaked at 3.2 % over two minutes there at full
/// Wow and 3.4 % over ten.
const CASSETTE_HZ: [f64; 2] = [0.45, 1.5];
const CASSETTE_SHARE: [f64; 2] = [0.55, 0.30];
const CASSETTE_DEPTH: f64 = 0.035;
const BELT_MS: f64 = 3.5;
const BELT_HOLD_S: f64 = 2.5;
const BELT_POLE_HZ: f64 = 0.2;
const BELT_SEED: u32 = 0xCA55_E77E;

/// **How fast Wow's wander may breathe the cassette**: by at most a quarter
/// of the cassette's depth for every radian the pack turns.
///
/// The wander scales the cassette's excursions, and an excursion that moves
/// bends the pitch by how fast it moves. A studio part's excursion is a few
/// samples, and breathing it bends nothing; the pack's is hundreds, and
/// breathed at the wander's own pace, a tenth of a second, its breathing
/// bent the pitch further than its turn did. That is why 5.2.0's deep head
/// was not in proportion to Wow: at 0.3 it peaked at 0.98 % where 0.9 %
/// was asked, and at full Wow, where the deep slew held the breathing
/// back, at only 2.6 %. A drag that varies with the pack's wind varies as
/// the pack turns, so the cassette breathes no faster than this per radian
/// of the pack's turn, at any Rate: the movement then bends the pitch by at
/// most a quarter of the pack's depth, across its turn and so in
/// quadrature with it (the pack peaks within about 3 % of its depth), and
/// it stays inside the deep slew at every setting, so the deep head's
/// swing is in proportion to Wow all the way up the dial.
const CASSETTE_BREATH: f64 = 0.25;

/// The cassette's parts as the deep governor's arrays hold them, slowest
/// first: the belt's drift, the pack, the roller.
const BELT: usize = 0;
const PACK: usize = 1;
const ROLLER: usize = 2;

/// How long a range switch takes, as the length of the equal-power
/// crossfade between the two heads, in milliseconds. A switch arriving
/// mid-fade turns the fade round from where it is.
const HEAD_FADE_MS: f64 = 25.0;

/// **The transport's depth scale**: the unweighted peak speed deviation, as
/// a fraction of the speed, that a part of relative depth one sways the tape
/// by at Wow and Flutter 0.3, Rate 1x and 15 ips.
///
/// One number calibrates the whole transport. Each part below has its own
/// relative depth and its own law with speed, and this scale puts their sum,
/// through the DIN 45507 / IEC 386 weighting and its 2-sigma detector, on
/// the weighted peaks studio two-tracks are specified at: 0.10, 0.06, 0.04
/// and 0.03 % at 3.75, 7.5, 15 and 30 ips, the per-speed medians of
/// published servo-capstan machines (modelled 0.098, 0.061, 0.040 and
/// 0.028 %).
const TRANSPORT_K: f64 = 5.61e-4;

/// Where Wow and Flutter are calibrated on their dials: 0.3 reads the
/// specified machine. Wow's depth is linear in its dial from there, because
/// the head's own travel already bounds how much wow a studio head can
/// carry; Flutter's is the square, so the bottom of its travel has the fine
/// control a fast sway needs.
const DIAL_AT_SPEC: f32 = 0.3;

/// **The transport's rotating parts**, slowest first: the supply reel once
/// around, the pinch roller, the tension guides and the capstan, as hertz
/// per inch per second of tape and depth relative to [`TRANSPORT_K`].
///
/// A rotating part turns once per its own circumference of tape, so its
/// rate is the speed over that length: 0.64, 2.55, 5.78 and 13.3 Hz at
/// 15 ips. Rate ([`rate_multiplier`]) turns all four from half to twice
/// that, as a reel's radius does from full to empty. Their depths are
/// fractions of the speed, so a slower turn sways the head further for the
/// same pitch. The depths fall with speed as `(15 / v)^0.8` ([`SPEED_LADDER`],
/// the ladder that reproduces the per-speed medians), except the supply
/// reel's below 15 ips, which falls with the speed itself, as the published
/// reel figures do. The first two are Wow's, the last two Flutter's.
const ROTOR_HZ_PER_IPS: [f64; 4] = [0.0425, 0.17, 0.385, 0.889];
const ROTOR_DEPTH: [f64; 4] = [1.0, 0.35, 0.30, 0.50];
const SPEED_LADDER: f64 = 0.8;

/// Where each rotating part starts, in turns: apart, so no two start in step.
const ROTOR_PHASE: [f64; 4] = [0.0, 0.21, 0.53, 0.37];

/// The capstan's second harmonic: its depth relative to [`TRANSPORT_K`] and
/// its phase against the capstan's own turn, in radians. An eccentric
/// capstan is never a pure once-around; the harmonic rides the same shaft,
/// so it is part of the capstan and the governor sheds the two together.
const CAPSTAN_2ND: f64 = 0.20;
const CAPSTAN_2ND_RAD: f64 = 0.9;

/// The tape's own mass resonance between its guides: noise narrowed to
/// 44 Hz at a Q of 5, whatever the speed or Rate, at 0.15 of
/// [`TRANSPORT_K`] RMS on the same ladder as the rotating parts. Flutter's,
/// and the governor never sheds it.
const RESONANCE_HZ: f64 = 44.0;
const RESONANCE_Q: f64 = 5.0;
const RESONANCE_DEPTH: f64 = 0.15;

/// The servo and the tape's tension drifting: a sample-and-hold every 4 s
/// through two one-poles of a second each, a delay of up to 0.10 ms at
/// Wow 0.3. So slow that the weighting barely sees it, it is what keeps a
/// long note's pitch from repeating itself. Wow's, and the first part the
/// governor sheds.
const DRIFT_MS: f64 = 0.10;
const DRIFT_HOLD_S: f64 = 4.0;
const DRIFT_POLE_S: f64 = 1.0;

/// **How far the studio head may sway, and how fast its parts may change how
/// far they sway.**
///
/// The head may move [`STUDIO_TRAVEL_MS`] either way of its rest, and the
/// governor shares that between the parts once the per-channel noise has
/// its reserve. And the parts' excursions may grow or shrink, all together,
/// by at most [`STUDIO_SLEW`] of a sample per sample, 0.3 % of the speed,
/// shared equally by the head's [`STUDIO_PARTS`]: so an automated depth, Rate
/// or Speed bends the pitch at most 0.3 % past what either setting does on
/// its own. A Wow thrown from 0 to 1 at 15 ips develops over about a second.
const STUDIO_TRAVEL_MS: f64 = 0.90;
const STUDIO_SLEW: f64 = 0.003;
const STUDIO_PARTS: usize = 7;

/// The studio head's parts, as the governor's arrays hold them: Wow's three
/// slowest first (the drift, the reel, the pinch roller), Flutter's two
/// slowest first (the guides, the capstan with its harmonic), then the two
/// it never sheds (the tape's resonance and the scrape). The four rotating
/// parts are `REEL..=CAPSTAN`, in [`ROTOR_HZ_PER_IPS`] order.
const DRIFT: usize = 0;
const REEL: usize = 1;
const PINCH: usize = 2;
const GUIDES: usize = 3;
const CAPSTAN: usize = 4;
const RESONANCE: usize = 5;
const SCRAPE: usize = 6;

/// How long Rate takes to arrive, as the time constant of its glide in
/// octaves, in milliseconds.
const RATE_GLIDE_MS: f64 = 50.0;

/// The depth wander: how far the noise is allowed to breathe the wow and
/// flutter depths, as a range of scale, and how the noise moves.
///
/// **A transport's drag is not a metronome.** Real wow and flutter spectra
/// are smeared, not lines, because the friction that causes them varies with
/// the wind of the reel. Sampled-and-held uniform noise, slewed, scales the
/// parts' depths, one noise for Wow's and one for Flutter's: held long
/// enough to read as drift, slewed so the depth never steps audibly. The
/// governor plans for the top of the range, so the head always has room for
/// the deepest the wander can breathe.
const DEPTH_WANDER: (f64, f64) = (0.5, 0.95);
const WOW_HOLD_S: f64 = 0.7;
const FLUTTER_HOLD_S: f64 = 0.23;
const WANDER_SLEW_S: f64 = 0.1;

/// Scrape: the head's own whisper, as band-limited noise swaying the tap by
/// fractions of a microsecond. Tape dragging on a fixed head sticks and
/// slips at kilohertz rates; the sidebands it sprays are 50 dB down, felt as
/// a faint graininess rather than heard as anything. Flutter's, on its
/// squared taper, and never shed.
///
/// The depth scales the band noise as [`DESIGN_RATE`] draws it, and the
/// transport scales the noise so every other rate draws the same sway per
/// hertz: at Flutter 0.3, about 0.026 µs RMS of head movement, the scrape
/// 5.0 shipped at its default.
const SCRAPE_MS: f64 = 0.000_5;
const SCRAPE_BAND_HZ: (f64, f64) = (800.0, 3_000.0);

/// The transport's noises' seeds: the depth wander (the one 5.0 shipped),
/// the scrape, the tape's resonance and the drift. Each its own stream, so
/// the slow wander draws the same values at the same moments at every rate,
/// and restored by `clear`.
const WANDER_SEED: u32 = 0x1B0F_5EED;
const SCRAPE_SEED: u32 = 0x5C2A_9E01;
const RESONANCE_SEED: u32 = 0x0D1F_7A11;
const DRIFT_SEED: u32 = 0x0D21_F7ED;

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

/// The tape's bias noise at each speed, slowest first: the gain on uniform
/// white noise that puts it at its datasheet level at Hiss 0.25 at
/// [`DESIGN_RATE`], and the playback equalisation's time constant, in
/// microseconds, that colours it.
///
/// **Tape hiss is white behind the playback equalisation.** The noise a
/// play head reads off the tape is close to white per hertz once NAB
/// playback has taken its record emphasis back off, so the hiss is white
/// noise through one first-order shelf: the speed's own playback time
/// constant against the 50 µs the level is quoted behind. 7.5 and 15 ips
/// are NAB 50 µs, a wire; 3.75 ips is NAB 90 µs, 4 dB brighter at 10 kHz
/// against 1 kHz; 30 ips is AES 17.5 µs, 6.6 dB duller. The pink ladder
/// 5.0 shipped tilted the whole band, which no tape does.
///
/// **The level is a high-output mastering tape's datasheet.** Its bias noise
/// sits 76.0 dB(A) under its 3 % distortion point on a 2.575 mm track, and
/// 1.1 dB nearer it on these 2 mm tracks; with the 3 % point where
/// the hot alignment 5.0 shipped puts it at 15 ips, 5.9 dB over 0 VU, that
/// is -69.0 dB(A) re 0 VU, -87.0 dBFS(A), 0.7 dB over 5.0's hiss at rest.
/// The other speeds take the same flux's noise: 30 ips 3 dB quieter,
/// 7.5 ips 1.5 dB quieter and 3.75 ips 2.5 dB louder, so -72.0, -70.5 and
/// -66.5 dB(A) re 0 VU. Those are the levels at [`HISS_DRIVE`], and the
/// hiss follows Drive from there, as it does on tape: see that constant.
/// The knob's taper is squared, because ears meet noise in decibels, and
/// `process` scales the gain so every rate puts the same hiss in every
/// hertz.
const HISS_GAIN: [f64; 4] = [8.619e-5, 8.613e-5, 1.023_7e-4, 1.133_2e-4];
const HISS_EQ_US: [f64; 4] = [90.0, 50.0, 50.0, 17.5];

/// **The Drive the hiss's level is quoted at, and why the hiss follows
/// Drive from it.**
///
/// The bias noise is on the tape, and the tape does not know the record
/// level. Recording hotter puts the music further above that noise, and
/// the line-up then brings the music back down to 0 VU and the noise down
/// with it; recording cooler does the opposite. So the hiss a person hears
/// is the tape's noise over the record level: it falls as `1 / d` for the
/// dial's `d` ([`DRIVE_TANH`]), the same `d` the record curve and the
/// line-up read, so at small signal the coupling is exact. [`HISS_GAIN`]
/// is quoted at 0.6, the hot alignment 5.0 shipped, so a fresh tape's 0.5,
/// the datasheet alignment, sits 4.6 dB over it: -82.4 dBFS(A) at 15 ips,
/// 64.4 dB(A) under 0 VU, where studio two-tracks at 15 ips measure 63 to
/// 64. The bottom of the dial sits 27.6 dB over, a record level that low
/// being as noisy as it is on any machine, and the top 18.4 dB under. The
/// tape's modulation noise rides the music and is not rescaled
/// ([`JITTER_UM`]). `the_hiss_falls_as_the_record_level_rises_as_it_does_on_tape`
/// holds the law.
const HISS_DRIVE: f32 = 0.6;

/// Modulation noise, as the head's spacing jittering: its depth at Hiss 0.25
/// in micrometres RMS, the most it can reach, and its bandwidth at 15 ips in
/// hertz.
///
/// **The hiss a tone carries with it.** Tape's magnetisation is grainy, and
/// the grain modulates whatever is recorded on it: a 1 kHz tone at 0 VU on a
/// studio machine at 15 ips carries a skirt of noise 72.5 dB under it in
/// 10 Hz at 800 Hz, and nothing of it when the tone stops. Wallace's spacing
/// law, linearised, makes a jitter in the head-to-tape distance a jitter in
/// time, `sigma n / v`, which is what this is: each channel's tap moves by a
/// fraction of a microsecond, 0.51 µs RMS at 15 ips and 2.0 at 3.75 at the
/// default Hiss, through noise band-limited to this bandwidth, which moves
/// with the speed (125 Hz at 3.75 ips to 1 kHz at 30). 0.194 µm is that
/// skirt; the depth follows the hiss's squared taper up to its cap, and
/// Hiss 0 removes it. It replaces the program tracker 5.0's hiss rode on:
/// the noise that follows the music is this, around each note, not a
/// broadband swell. (Its sidebands are phase rather than amplitude, the
/// same power.)
const JITTER_UM: f64 = 0.194;
const JITTER_CAP_UM: f64 = 2.0;
const JITTER_HZ_15: f64 = 500.0;

/// **The modulation noise's depth the dials ask for**, in micrometres RMS:
/// [`JITTER_UM`] on the hiss's squared taper, raised
/// [`UNDER_BIAS_JITTER_DB`] for every dB of under-bias and by an old tape's
/// coarser grain ([`AGE_JITTER`]), and never past [`JITTER_CAP_UM`]. The
/// kernel's glide aims here, and so does the figure a face prints, so the
/// two plan the head's reserve alike.
fn jitter_um(p: &TapeParams) -> f64 {
    let taper = f64::from(norm(p.hiss) / 0.25).powi(2);
    let under = (-bias_db(p)).max(0.0);
    let age = 1.0 + AGE_JITTER * f64::from(norm_or(p.age, 0.0));
    (JITTER_UM * taper * 10.0_f64.powf(UNDER_BIAS_JITTER_DB * under / 20.0) * age).min(JITTER_CAP_UM)
}

/// **What the head keeps in reserve for each track's own movement**, in
/// samples at `sr`, on tape running at `v` ips: the skew the head's
/// azimuth puts between the tracks at its widest, half of it each way
/// ([`skew_samples`] at the widest angle `urad`, [`widest_urad`]), four
/// sigma of modulation noise of
/// `noise_um` µm RMS, which it never reaches, and a sample. The governor
/// shares the head's travel only after this, so a track's own movement can
/// never carry its head off the line. The one place the reserve is worked
/// out: the kernel's governor and the figure a face prints both read it.
fn reserve(noise_um: f64, urad: f64, v: f64, sr: f64) -> f64 {
    skew_samples(urad, v, sr) + 4.0 * noise_um * 1.0e-6 * sr / (0.0254 * v) + 1.0
}

// ── what the years do ───────────────────────────────────────────────────────

/// **How far an old tape rides off a worn head, in micrometres, at full
/// Age.** Wallace's spacing law: a gap of `d` between the head and the
/// oxide costs `54.575 d / wavelength` dB, so it is a treble loss that falls
/// hardest on the short wavelengths of slow tape: at full Age 10 kHz loses
/// about 21 dB at 3.75 ips, 11 at 7.5, 5 at 15 and 2.5 at 30, against new
/// tape. 3 µm is ten times the most a studio will tolerate and inside the
/// 1 to 20 µm the published wear and debris events reach. The filter is
/// [`SPACING_FIT`]'s.
const AGE_SPACING_UM: f64 = 3.0;

/// **Wallace's loss as three one-poles**: `(1 - k HP1(a f0)) LP1(b f0)^2`,
/// `(k, a, b)` here, for `f0 = v / (54.575 d)` (the frequency whose
/// wavelength is 54.575 times the spacing, where the law has lost 1 dB).
/// Within 0.8 dB of the law down to 10 dB of loss and 1.1 dB down to 20,
/// each one-pole matched at 10 kHz ([`matched_b`]).
const SPACING_FIT: (f64, f64, f64) = (0.1328, 0.5046, 7.057);
const WALLACE_DB: f64 = 54.575;

/// The spacing filter's table: matched one-pole coefficients for corners
/// from this many hertz up, [`SPACING_PER_OCTAVE`] to the octave, over
/// [`SPACING_POINTS`] entries, the last 16.8 MHz, past which a one-pole is
/// a wire to a ten-millionth. Every spacing reads its corners here, so no
/// filter is designed on the audio thread.
const SPACING_LOW_HZ: f64 = 16.0;
const SPACING_PER_OCTAVE: f64 = 16.0;
const SPACING_POINTS: usize = 321;

/// How often the head's spacing and the tracks' skew are worked out again,
/// in frames, between which both move in a straight line: often enough to
/// follow a dropout's few milliseconds, seldom enough to cost nothing.
const WEAR_STEP: u64 = 4;

/// **An aged head's azimuth, in microradians at full Age**: the gap's
/// standing error and how far it wanders either way of that, once every
/// [`AZIMUTH_WANDER_M`] of tape.
///
/// A head's gap is never quite square to the tape, and an old tape is
/// never slit quite straight: standing errors are arcminutes, and a
/// slitting's wander is up to 1000 µrad peak to peak every 300 mm. Across
/// [`AZIMUTH_SPAN_MM`] of tape, an angle `theta` puts the two tracks
/// `span tan(theta) / v` apart in time: at full Age on 3.75 ips tape, 42 µs
/// at the standing 1000 µrad, swinging from 21 to 63 µs with the wander,
/// half of it each way of the head. The same angle costs each track its
/// own treble, as `|sin x / x|` across its width ([`TRACK_MM`]), which is
/// folded into the spacing as the gap that costs the same at 10 kHz. At
/// Age 0 the angle is exactly zero, and the two tracks bit for bit alike.
const AZIMUTH_URAD: (f64, f64) = (1_000.0, 500.0);
const AZIMUTH_WANDER_M: f64 = 0.300;
const AZIMUTH_SPAN_MM: f64 = 4.0;
const TRACK_MM: f64 = 2.0;

/// The spacing that costs what an azimuth angle `theta` costs at 10 kHz, in
/// metres, on tape at `v_m` metres a second: `1.448 pi^2 w^2 theta^2 10 kHz
/// / (54.575 v_m)` for the track's width `w`, the `|sin x / x|` loss's
/// small-angle form matched at 10 kHz. 0.110 µm at 1000 µrad on 3.75 ips
/// tape, where the true loss at 10 kHz is 0.64 dB; at the wander's
/// 1500 µrad 1.47 dB. Linear in frequency where the true loss is
/// quadratic, which only the top octave hears.
fn azimuth_gap(theta: f64, v_m: f64) -> f64 {
    let w = TRACK_MM * 1.0e-3;
    1.448 * std::f64::consts::PI.powi(2) * w * w * theta * theta * 1.0e4 / (WALLACE_DB * v_m)
}

/// **What a skewed head puts between the tracks**, as each track's share of
/// it in samples at `sr`, at an angle of `urad` microradians, on tape at
/// `v` ips: half the gap across [`AZIMUTH_SPAN_MM`] in time.
fn skew_samples(urad: f64, v: f64, sr: f64) -> f64 {
    0.5 * AZIMUTH_SPAN_MM * 1.0e-3 * (urad * 1.0e-6).tan() / (0.0254 * v) * sr
}

/// **The widest the head's azimuth can stand off square**, in
/// microradians, at the Age `age` and the Damage `damage`: Age's standing
/// error at the top of its wander, and a crease's twist at its widest on
/// top of it (any Damage at all can crease the tape).
fn widest_urad(age: f64, damage: f64) -> f64 {
    let crease = if damage > 0.0 { CREASE_URAD.0 + CREASE_URAD.1 * damage } else { 0.0 };
    age * (AZIMUTH_URAD.0 + AZIMUTH_URAD.1) + crease
}

/// The worn record head's over-bias at full Age, in dB, on the bias's
/// treble shelf alone: a worn head writes as if biased hotter.
const AGE_OVERBIAS_DB: f64 = 1.5;

/// **A magnetised head's second harmonic**, as the rectifier's share at full
/// Age: `y + c (sqrt(y^2 + e^2) - e)` with `c` this times Age, a smoothed
/// full-wave rectifier, which is 0.5 % of second harmonic at full Age at
/// every level above a hundredth of a millivolt (`4 c / 3 pi`), the
/// level-independent signature a magnetised head has. Its rectified DC is
/// taken off by a one-pole high-pass at [`MAGNETISED_DC_HZ`]. After the
/// line-up and inside the playback equalisation, where the tape's flux is.
const MAGNETISED_H2: f32 = 0.011_78;
const MAGNETISED_KNEE: f32 = 1.0e-4;
const MAGNETISED_DC_HZ: f64 = 5.0;

/// How far Age raises the hiss at its top, in dB, and how much coarser it
/// makes the tape's modulation noise, as a share of it: an old tape is
/// noisier, by up to 3 dB, and its grain modulates twice as hard.
const AGE_HISS_DB: f64 = 3.0;
const AGE_JITTER: f64 = 1.0;

/// How long Age takes to arrive, as the time constant of its glide, in
/// milliseconds: years are not a step.
const AGE_GLIDE_MS: f64 = 100.0;

// ── what the tape has been through ──────────────────────────────────────────

/// **Damage's defects, per metre of tape at full Damage**, on the dial's
/// square, so the lower half of it is a well-kept reel: dropouts at 4 a
/// metre (1.5 a second at 15 ips, 0.38 at 3.75) and creases at half a
/// metre's one. They live on the tape, so the count goes with the metres
/// that pass the head, not with the seconds.
const DROPOUTS_PER_M: f64 = 4.0;
const CREASES_PER_M: f64 = 0.5;

/// **A dropout**: a stretch of tape between these lengths in millimetres,
/// log-uniform (1.3 to 21 ms at 15 ips, 7.1 on average; 5 to 84 ms at
/// 3.75), lifted off the head to a peak spacing log-uniform from the first
/// figure to the first plus the second times Damage, in micrometres, and
/// deeper under-biased (times `10^(-b / 20)` for the bias `b` dB re
/// aligned, the shallower record under-bias leaves). Its spacing rises and
/// falls as `sin^2` along it. It takes both tracks [`DROPOUT_BOTH`] of the
/// time, the left alone [`DROPOUT_LEFT`], the right alone the rest.
const DROPOUT_MM: (f64, f64) = (0.5, 8.0);
const DROPOUT_UM: (f64, f64) = (1.0, 19.0);
const DROPOUT_BOTH: f32 = 0.6;
const DROPOUT_LEFT: f32 = 0.2;

/// **A crease**: a stretch of tape this long in millimetres, rippled at a
/// pitch between these millimetres, lifted to a peak spacing from the first
/// figure to the first plus the second times Damage in micrometres, and
/// twisted off square by an angle of random sign from the first figure to
/// the first plus the second times Damage in microradians, all uniform and
/// all rising and falling as `sin^2` along it. On both tracks.
const CREASE_MM: (f64, f64) = (10.0, 50.0);
const CREASE_PITCH_MM: (f64, f64) = (2.0, 6.0);
const CREASE_UM: (f64, f64) = (2.0, 6.0);
const CREASE_URAD: (f64, f64) = (300.0, 1_200.0);

/// **Worn edges**: each track's edge rides off the head by up to this many
/// micrometres times Damage's square, a level drawn afresh every first
/// figure's millimetres of tape and reached over the second's. Damage is
/// heard at once, before its first dropout.
const EDGE_UM: f64 = 2.0;
const EDGE_MM: (f64, f64) = (30.0, 10.0);

/// The seeds of the damage's draws and of each track's edge, restored by
/// `clear`.
const DAMAGE_SEED: u32 = 0xDA4A_6E01;
const EDGE_SEEDS: [u32; 2] = [0xED6E_0001, 0xED6E_0002];

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

    /// The playback shelf `(1 + s t2) / (1 + s t1)`, unity at DC, by the
    /// bilinear transform warped at 10 kHz (or a quarter of the rate, if that
    /// is lower), so the shelf is exact where its colour is quoted. Equal
    /// time constants are one zero and one pole in the same place: a wire.
    fn playback_shelf(t2_us: f64, t1_us: f64, sample_rate: f64) -> [f64; 3] {
        let fw = 10_000.0_f64.min(0.25 * sample_rate);
        let w = std::f64::consts::TAU * fw;
        let k = w / (w / (2.0 * sample_rate)).tan();
        let (t2, t1) = (t2_us * 1.0e-6, t1_us * 1.0e-6);
        [(1.0 + k * t2) / (1.0 + k * t1), (1.0 - k * t2) / (1.0 + k * t1), (1.0 - k * t1) / (1.0 + k * t1)]
    }

    /// The bias shelf `A (s + w / sqrt A) / (s + w sqrt A)`, unity at DC
    /// and `A` at the top, its corner `w` at `fc` hertz with `db` of height
    /// there (`A` is twice that, in decibels), held to unity at 1 kHz: the
    /// machine lined up again after the bias moved. A first-order section
    /// matched to it in magnitude at DC, at 1 kHz and at 10 kHz (or a
    /// quarter of the rate, if that is lower); `points` are those last two,
    /// each as its hertz and `cos w` at the rate.
    ///
    /// **Matched, not warped.** A bilinear transform is exact at one
    /// frequency, and this shelf is read at three: at DC, where the
    /// published ladders anchor their long wavelength; at the 1 kHz the
    /// machine is lined up at; and at 10 kHz, where every ladder is quoted.
    /// Warped at 10 kHz, the 1 kHz tone met the shelf where the analog one
    /// is at 850 Hz at 48 kHz, and at 3.75 ips, whose corner is 2.5 kHz,
    /// that moved the bass half a decibel, by a different half at every
    /// rate. A first-order section has three numbers, so it can meet the
    /// analog shelf exactly at all three points at any rate: a section's
    /// squared gain is a ratio of two lines in `cos w`, the three points on
    /// it fix its pole, and then its zero and its gain, each the root
    /// inside the unit circle.
    fn bias_shelf(fc: f64, db: f64, points: [(f64, f64); 2]) -> [f64; 3] {
        let a = 10.0_f64.powf(db / 10.0);
        // The analog shelf's squared gain at `f` hertz, `A^2 (r + 1/A) / (r +
        // A)` for `r` the square of `f` over the corner.
        let shelf = |f: f64| {
            let r = (f / fc).powi(2);
            a * a * (r + 1.0 / a) / (r + a)
        };
        let at = |k: usize| if k == 0 { 1.0 } else { points[k - 1].1 };
        let one = shelf(points[0].0);
        let x = [1.0 / one, 1.0, shelf(points[1].0) / one];
        let (d1, d2) = (at(1) - at(0), at(2) - at(0));
        let ap = (x[1] - x[0]) * d2 - (x[2] - x[0]) * d1;
        let bp = (x[1] * at(1) - x[0] * at(0)) * d2 - (x[2] * at(2) - x[0] * at(0)) * d1;
        let a1 = if bp == 0.0 {
            0.0
        } else {
            -ap / (bp + (bp * bp - ap * ap).max(0.0).sqrt().copysign(bp))
        };
        let line = |k: usize| x[k] * (1.0 + a1 * a1 + 2.0 * a1 * at(k));
        let slope = (line(1) - line(0)) / d1;
        let rest = line(0) - slope * at(0);
        let m = (slope / (2.0 * rest)).clamp(-0.5, 0.5);
        let zero = 2.0 * m / (1.0 + (1.0 - 4.0 * m * m).sqrt());
        let g = (rest / (1.0 + zero * zero)).sqrt();
        [g, g * zero, a1]
    }

    fn of(c: [f64; 3]) -> Section {
        Section {
            b0: c[0] as f32,
            b1: c[1] as f32,
            a1: c[2] as f32,
        }
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

/// A low shelf as a trapezoidal state-variable filter, the bell's sections
/// with their low-pass mixed in as well: the analog second-order shelf
/// `A (s^2 + s sqrt(A) / Q + A) / (A s^2 + s sqrt(A) / Q + 1)` at a Q of a
/// half root two, `A^2` of gain at DC, `A` at its corner, unity at the top,
/// warped at the corner.
///
/// **Its inverse is the same shelf at the opposite gain, exactly**: `A` for
/// `1 / A` swaps the analog shelf's numerator and its denominator, and the
/// bilinear transform warped at the same corner keeps them swapped. So what
/// goes onto the tape through one comes off it through the other, as it
/// does through the NAB pair, and the form stays stable for any glide of
/// its gain, which moves with the bias.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shelf {
    a1: f32,
    a2: f32,
    a3: f32,
    m1: f32,
    m2: f32,
}

impl Shelf {
    const WIRE: Shelf = Shelf {
        a1: 1.0,
        a2: 0.0,
        a3: 0.0,
        m1: 0.0,
        m2: 0.0,
    };

    /// The low shelf whose corner is at `t = tan(pi f / rate)`, with `db` of
    /// gain at DC.
    fn low(t: f64, db: f64) -> Self {
        let a = 10.0_f64.powf(db / 40.0);
        let k = std::f64::consts::SQRT_2;
        let g = t / a.sqrt();
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self {
            a1: a1 as f32,
            a2: a2 as f32,
            a3: (g * a2) as f32,
            m1: (k * (a - 1.0)) as f32,
            m2: (a * a - 1.0) as f32,
        }
    }

    /// That shelf's gain at the frequency whose `tan(pi f / rate)` is `at`:
    /// the analog shelf at the frequency the bilinear transform warps it
    /// to, which is the digital section's gain there exactly.
    fn low_gain(t: f64, at: f64, db: f64) -> f64 {
        let a = 10.0_f64.powf(db / 40.0);
        let (r, q) = (at / t, std::f64::consts::SQRT_2 * a.sqrt());
        a * (((a - r * r).powi(2) + (q * r).powi(2)) / ((1.0 - a * r * r).powi(2) + (q * r).powi(2))).sqrt()
    }

    #[inline]
    fn run(&self, st: &mut [f32; 2], x: f32) -> f32 {
        let v3 = x - st[1];
        let v1 = self.a1 * st[0] + self.a2 * v3;
        let v2 = st[1] + self.a2 * st[0] + self.a3 * v3;
        st[0] = flush(2.0 * v1 - st[0]);
        st[1] = flush(2.0 * v2 - st[1]);
        x + self.m1 * v1 + self.m2 * v2
    }
}

/// The hiss's seeds, one per channel (two tracks, two noises), and the
/// modulation noise's. Restored by `clear`, so a cleared tape hisses the same
/// hiss again.
const HISS_SEEDS: [u32; 2] = [0xA5A5_0001, 0x5EED_C0DE];
const JITTER_SEEDS: [u32; 2] = [0x30D0_0001, 0x30D0_0002];

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
    /// The hiss: its playback shelf ([`Section::playback_shelf`]) and its
    /// gain as `log2` ([`HISS_GAIN`]), blended in octaves so a glide moves
    /// it in decibels. The gain carries [`HISS_DRIVE`]'s record level and
    /// this speed's `dmol`, so `process` follows Drive by multiplying it by
    /// the `1 / d` the record curve already has: `dmol` cancels there, and
    /// at [`HISS_DRIVE`] the hiss is [`HISS_GAIN`] at every speed.
    hiss_eq: [f64; 3],
    hiss_gain: f64,
    /// The modulation noise's one-pole at its bandwidth for this speed, and
    /// the factor that keeps its output unit-RMS.
    jitter_a: f64,
    jitter_norm: f64,
}

impl Voicing {
    fn at(speed: TapeSpeed, sample_rate: f64) -> Self {
        let i = speed as usize;
        // The tape's speed in metres per second: every wavelength below is a
        // length on the tape, and its frequency is this over it.
        let v_m = 0.0254 * speed.ips();
        let floor = v_m / (ERASE_FLOOR_UM * 1.0e-6);
        let f_b = v_m / (BUMP_WAVELENGTH_MM * 1.0e-3);
        let jitter_a = f64::from(lp_a(sample_rate, JITTER_HZ_15 * speed.ips() / 15.0));
        let dmol = -DMOL_DB[i] / 20.0 * std::f64::consts::LOG2_10;
        Self {
            dmol,
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
            hiss_eq: Section::playback_shelf(HISS_EQ_US[i], 50.0, sample_rate),
            hiss_gain: HISS_GAIN[i].log2() + dial_log2_d(HISS_DRIVE) + dmol,
            jitter_a,
            // A one-pole `y += a (x - y)` driven by unit white noise has a
            // variance of `a / (2 - a)`; this hands it back.
            jitter_norm: ((2.0 - jitter_a) / jitter_a).sqrt(),
        }
    }
}

/// The rotation a turning part takes in one frame, `(cos w, sin w)` for an
/// increment of `w` radians, by the first terms of their series: within a
/// billionth under 0.1 rad, which every part stays far under at any rate
/// the tape can be built at, so a moving Rate or Speed costs no
/// transcendental on the audio thread.
#[inline]
fn rotation(w: f64) -> [f64; 2] {
    let w2 = w * w;
    [1.0 - w2 * (0.5 - w2 / 24.0), w * (1.0 - w2 * (1.0 / 6.0 - w2 / 120.0))]
}

/// What the capstan's harmonic sways relative to the capstan's own turn:
/// [`CAPSTAN_2ND`] of the depth at twice the rate is a fifth of the
/// excursion.
const CAPSTAN_2ND_EXC: f64 = CAPSTAN_2ND / ROTOR_DEPTH[3] / 2.0;

/// How far each of the studio head's parts can move the head, per unit of
/// what its own state holds: a rotating part's excursion is its amplitude,
/// the capstan's its amplitude and its harmonic's, the drift's its gain on
/// noise held within one, and the two noises' three of their RMS.
const PEAK: [f64; STUDIO_PARTS] = [1.0, 1.0, 1.0, 1.0, 1.0 + CAPSTAN_2ND_EXC, 3.0, 3.0];

/// **What every part of the studio head asks for**, as its excursion at a
/// depth wander of one, in samples at its peak ([`PEAK`]): for a tape at `v`
/// ips whose rotating parts turn at `omega` radians a frame, with Wow's and
/// Flutter's multipliers `m`, at the rate `sr`, where a unit-RMS resonance
/// moves `kappa` a frame and the scrape at Flutter's unit multiplier sways
/// `scrape` samples RMS. A part's depth of pitch is a fraction of the speed
/// ([`TRANSPORT_K`]), so its excursion is that depth over its turn. Shared
/// by the transport and by the figure a face shows, so the two cannot plan
/// differently.
fn studio_wants(v: f64, omega: &[f64; 4], m: [f64; 2], sr: f64, kappa: f64, scrape: f64) -> [f64; STUDIO_PARTS] {
    let ladder = (15.0 / v).powf(SPEED_LADDER);
    let law = [if v <= 15.0 { v / 15.0 } else { ladder }, ladder, ladder, ladder];
    let [m_w, m_f] = m;
    let dial = [m_w, m_w, m_f, m_f];
    let mut want = [0.0_f64; STUDIO_PARTS];
    for k in 0..4 {
        want[REEL + k] = PEAK[REEL + k] * TRANSPORT_K * ROTOR_DEPTH[k] * law[k] * dial[k] / omega[k];
    }
    want[DRIFT] = DRIFT_MS * 1.0e-3 * sr * m_w;
    want[RESONANCE] = PEAK[RESONANCE] * TRANSPORT_K * RESONANCE_DEPTH * ladder * m_f / kappa;
    want[SCRAPE] = PEAK[SCRAPE] * scrape * m_f;
    want
}

/// Every part's ask at the top of its depth wander: the governor plans for
/// the deepest the wander can breathe.
fn at_the_top(want: &[f64; STUDIO_PARTS]) -> [f64; STUDIO_PARTS] {
    let mut asked = *want;
    for (j, a) in asked.iter_mut().enumerate() {
        *a *= Transport::breath(j, DEPTH_WANDER.1, DEPTH_WANDER.1);
    }
    asked
}

/// **The governor**: given what every part asks for at the top of its
/// wander and the `room` the head has after its reserve, the fraction of its
/// ask each part keeps. The resonance and the scrape take theirs first and
/// are never shed; if Wow's three and Flutter's two then ask for more than
/// is left, the rest is shared between the two groups in proportion to what
/// each asked for, and inside each group the slowest part gives way first.
fn govern(asked: &[f64; STUDIO_PARTS], room: f64) -> [f64; STUDIO_PARTS] {
    let room = (room - asked[RESONANCE] - asked[SCRAPE]).max(0.0);
    let wow = asked[DRIFT] + asked[REEL] + asked[PINCH];
    let flutter = asked[GUIDES] + asked[CAPSTAN];
    let mut keep = [1.0_f64; STUDIO_PARTS];
    if wow + flutter > room {
        let mut shed = |order: &[usize], mut over: f64| {
            for &j in order {
                let cut = asked[j].min(over.max(0.0));
                keep[j] = if asked[j] > 0.0 { (asked[j] - cut) / asked[j] } else { 1.0 };
                over -= cut;
            }
        };
        shed(&[DRIFT, REEL, PINCH], wow - room * wow / (wow + flutter));
        shed(&[GUIDES, CAPSTAN], flutter - room * flutter / (wow + flutter));
    }
    keep
}

/// The studio head's travel either way of its rest, in samples, for a tape
/// built at `sr` whose head rests `base` samples back.
fn studio_travel(sr: f64, base: usize) -> f64 {
    (STUDIO_TRAVEL_MS * 1.0e-3 * sr).min(base as f64 - 2.0).max(0.0)
}

/// The resonance's two-pole at `sr`, `[c1, c2]`, and how much its output,
/// made unit-RMS, moves from one frame to the next, RMS: `sqrt(2 (1 - rho))`
/// for its correlation a frame apart, `rho = c1 / (1 - c2)`.
fn resonance_at(sr: f64) -> ([f64; 2], f64) {
    let w0 = std::f64::consts::TAU * RESONANCE_HZ / sr;
    let pole = (-w0 / (2.0 * RESONANCE_Q)).exp();
    let (c1, c2) = (2.0 * pole * w0.cos(), -pole * pole);
    ([c1, c2], (2.0 * (1.0 - c1 / (1.0 - c2))).sqrt())
}

/// The scrape's band at `sr`: its two one-poles' coefficients and the RMS of
/// their difference on uniform noise. A one-pole's energy is `a / (2 - a)`,
/// and two of the same noise share `a1 a2 / (1 - b1 b2)`.
fn scrape_band(sr: f64) -> ([f64; 2], f64) {
    let a = [lp_a(sr, SCRAPE_BAND_HZ.0), lp_a(sr, SCRAPE_BAND_HZ.1)].map(f64::from);
    let one = |a: f64| a / (2.0 - a);
    let both = a[0] * a[1] / (1.0 - (1.0 - a[0]) * (1.0 - a[1]));
    (a, ((one(a[0]) + one(a[1]) - 2.0 * both) / 3.0).sqrt())
}

/// The scrape's sway per unit of Flutter's multiplier at `sr`, in samples
/// per unit of its band noise: 5.0's scrape at its default, with the root
/// that puts the same sway in every hertz at every rate.
fn scrape_per_m(sr: f64) -> f64 {
    SCRAPE_MS * 1.0e-3 * sr * (sr / f64::from(DESIGN_RATE)).sqrt() * f64::from(DIAL_AT_SPEC)
}

/// **The transport**: every part that sways the play head, the governor that
/// shares the head's travel between them, and the slew that keeps a moving
/// dial from bending the pitch. One for both channels: one capstan.
///
/// # The excursion form
///
/// A rotating part is held as how far it moves the head, its excursion `A`
/// in samples, and how fast it turns, `omega` in radians a frame; its sway
/// is `-A sin(phi)` and the pitch it bends is `A omega`. A part's depth of
/// pitch is a fraction of the speed, so its excursion's target is that
/// depth over `omega`, and the excursion moves to its target by at most
/// [`STUDIO_SLEW`]'s share a frame. **A part's turn may fall at once but
/// rise only as fast as its excursion can shrink**, by a factor of
/// `1 + 32 sigma / A` a design, and that is what keeps a Rate or Speed
/// change from overshooting: while `A` shrinks by `sigma` and `omega` rises
/// by `1 + sigma / A`, the depth `A omega` becomes `A omega - sigma^2 omega
/// / A`, and cannot grow. The price is that a part with a long excursion
/// takes up to about a second to reach a much faster turn, with its depth
/// right all the way.
///
/// # The governor
///
/// Once a design, on targets: the head may sway [`STUDIO_TRAVEL_MS`] either
/// way of its rest, less a reserve for the per-channel noise, less the
/// resonance's and the scrape's three sigma, which are never shed. If Wow's
/// parts and Flutter's ask for more than is left, at the top of the depth
/// wander, **the travel is shared in proportion to what each asked for, and
/// inside each the slowest part gives way first**: the drift, then the
/// reel, then the pinch roller; the guides, then the capstan with its
/// harmonic. So both dials stay live where the tape asks for more than the
/// head can hold, and nothing is ever clipped: a part's excursion only ever
/// grows into the room the others leave.
struct Transport {
    sample_rate: f64,
    /// Wow's and Flutter's depth multipliers, and Rate as `log2` of its
    /// multiple, gliding to their dials.
    wow: Glide,
    flutter: Glide,
    rate: Glide,
    a_glide: f64,
    a_rate: f64,
    /// The rotating parts, in [`ROTOR_HZ_PER_IPS`] order: each one's phase
    /// as a rotator (`cos`, `sin`), the rotation it takes a frame, and that
    /// rotation's angle.
    turn: [[f64; 2]; 4],
    step: [[f64; 2]; 4],
    omega: [f64; 4],
    /// Where each rotating part starts, as a rotator.
    start: [[f64; 2]; 4],
    /// The capstan harmonic's phase against the capstan, as `cos` and `sin`.
    harmonic: [f64; 2],
    /// Every part's excursion now and its governed target at a wander of
    /// one, in samples at its peak ([`PEAK`]); what it asked for and what it
    /// was given at the wander's top, from the last design.
    exc: [f64; STUDIO_PARTS],
    aim: [f64; STUDIO_PARTS],
    asked: [f64; STUDIO_PARTS],
    given: [f64; STUDIO_PARTS],
    /// How far a part's excursion may move a frame: [`STUDIO_SLEW`]'s share.
    slew: f64,
    /// The head's travel either way of its rest, and what the parts may use
    /// of it after the reserve, in samples.
    travel: f64,
    room: f64,
    /// The drift: its noise, its hold's countdown and length, the value it
    /// holds, and its two poles.
    drift_rng: u32,
    drift_hold: u32,
    drift_frames: u32,
    drift_held: f64,
    drift_lp: [f64; 2],
    a_drift: f64,
    /// The resonance: its noise, its two-pole's states and coefficients, the
    /// gain that makes it unit-RMS, and how much a unit-RMS resonance moves
    /// from one frame to the next, RMS.
    res_rng: u32,
    res_y: [f64; 2],
    res_c: [f64; 2],
    res_unit: f64,
    res_kappa: f64,
    /// The scrape: its noise, its band's two one-poles (800 Hz, 3 kHz), the
    /// band's RMS, and its sway per unit of Flutter's multiplier.
    scrape_rng: u32,
    scrape_lp: [f64; 2],
    a_scrape: [f64; 2],
    scrape_rms: f64,
    scrape_per_m: f64,
    /// The depth wander, Wow's and Flutter's: its noise, the held targets,
    /// the slewed states, the countdowns and their lengths.
    wander_rng: u32,
    wander: [f64; 2],
    wander_target: [f64; 2],
    wander_hold: [u32; 2],
    wander_frames: [u32; 2],
    a_wander: f64,
    /// The cassette, the deep head's own parts: the pack's and the roller's
    /// rotators, their rotation a frame and its angle, and every cassette
    /// part's excursion, target, ask and gift, slowest first ([`BELT`],
    /// [`PACK`], [`ROLLER`]), with how far each may move a frame; and the
    /// scale the wow wander gives the pack's and the roller's targets,
    /// following the wander at most [`CASSETTE_BREATH`] a radian of the
    /// pack's turn.
    cas_turn: [[f64; 2]; 2],
    cas_step: [[f64; 2]; 2],
    cas_omega: [f64; 2],
    cas_exc: [f64; 3],
    cas_aim: [f64; 3],
    cas_asked: [f64; 3],
    cas_given: [f64; 3],
    cas_slew: f64,
    cas_breath: f64,
    /// The deep head's travel either way of its rest, and what the cassette
    /// may use of it once the reserve and Flutter's shared parts have
    /// theirs, in samples.
    deep_travel: f64,
    deep_room: f64,
    /// The belt's drift: its noise, its hold's countdown and length, the
    /// value it holds, and its two poles with their coefficient, none of
    /// them following Rate.
    belt_rng: u32,
    belt_hold: u32,
    belt_frames: u32,
    belt_held: f64,
    belt_lp: [f64; 2],
    a_belt: f64,
    /// Whether the dials select the deep head, and whether the cassette is
    /// running: it starts from nothing when the deep head is selected and
    /// stops, reset silent, once the deep head has faded out.
    deep: bool,
    cassette: bool,
    /// The two heads' sway this frame, in samples: the studio head's and the
    /// deep head's.
    sway: f64,
    deep_sway: f64,
    /// The cassette's pack alone this frame, for the test that holds it to
    /// its own depth.
    #[cfg(test)]
    pack_sway: f64,
    /// Whether the next design is the first since `new` or `clear`, which
    /// puts every part at its target at once: a fresh tape is a running
    /// machine, not one spinning up.
    fresh: bool,
}

impl Transport {
    fn new(sample_rate: f64, base: usize, base_deep: usize) -> Self {
        let sr = sample_rate;
        let step = |ms: f64| 1.0 - (-1.0 / (sr * ms * 1.0e-3)).exp();
        // The resonance: a two-pole at 44 Hz and Q 5 driven by uniform noise,
        // whose output variance is a third of its impulse response's energy,
        // `(1 - c2) / ((1 + c2) ((1 - c2)^2 - c1^2))`.
        let ([c1, c2], res_kappa) = resonance_at(sr);
        let energy = (1.0 - c2) / ((1.0 + c2) * ((1.0 - c2).powi(2) - c1 * c1));
        let (a_scrape, scrape_rms) = scrape_band(sr);
        let start = ROTOR_PHASE.map(|t| {
            let (s, c) = (std::f64::consts::TAU * t).sin_cos();
            [c, s]
        });
        // The belt's two poles at 0.2 Hz: `1 - e^-w` by its series, `w` being
        // under two ten-thousandths at any rate the tape can be built at, the
        // series 5.2.0 ran on every design while the belt followed Rate, so
        // at Rate 1x the coefficient is the same bits it was.
        let w = std::f64::consts::TAU * BELT_POLE_HZ / sr;
        let mut t = Self {
            sample_rate: sr,
            wow: Glide::new(1.0e-9),
            flutter: Glide::new(1.0e-9),
            rate: Glide::new(1.0e-7),
            a_glide: step(GLIDE_MS),
            a_rate: step(RATE_GLIDE_MS),
            turn: start,
            step: [[1.0, 0.0]; 4],
            omega: [0.0; 4],
            start,
            harmonic: [CAPSTAN_2ND_RAD.cos(), CAPSTAN_2ND_RAD.sin()],
            exc: [0.0; STUDIO_PARTS],
            aim: [0.0; STUDIO_PARTS],
            asked: [0.0; STUDIO_PARTS],
            given: [0.0; STUDIO_PARTS],
            slew: STUDIO_SLEW / STUDIO_PARTS as f64,
            travel: studio_travel(sr, base),
            room: 0.0,
            drift_rng: DRIFT_SEED,
            drift_hold: 0,
            drift_frames: (DRIFT_HOLD_S * sr) as u32,
            drift_held: 0.0,
            drift_lp: [0.0; 2],
            a_drift: step(DRIFT_POLE_S * 1.0e3),
            res_rng: RESONANCE_SEED,
            res_y: [0.0; 2],
            res_c: [c1, c2],
            res_unit: (3.0 / energy).sqrt(),
            res_kappa,
            scrape_rng: SCRAPE_SEED,
            scrape_lp: [0.0; 2],
            a_scrape,
            scrape_rms,
            scrape_per_m: scrape_per_m(sr),
            wander_rng: WANDER_SEED,
            wander: [0.5; 2],
            wander_target: [0.5; 2],
            wander_hold: [0; 2],
            wander_frames: [(WOW_HOLD_S * sr) as u32, (FLUTTER_HOLD_S * sr) as u32],
            a_wander: step(WANDER_SLEW_S * 1.0e3),
            cas_turn: [[1.0, 0.0]; 2],
            cas_step: [[1.0, 0.0]; 2],
            cas_omega: [0.0; 2],
            cas_exc: [0.0; 3],
            cas_aim: [0.0; 3],
            cas_asked: [0.0; 3],
            cas_given: [0.0; 3],
            cas_slew: (DEEP_SLEW - 4.0 * STUDIO_SLEW / STUDIO_PARTS as f64) / 3.0,
            cas_breath: 1.0,
            deep_travel: (DEEP_TRAVEL_MS * 1.0e-3 * sr).min(base_deep as f64 - 2.0).max(0.0),
            deep_room: 0.0,
            belt_rng: BELT_SEED,
            belt_hold: 0,
            belt_frames: (BELT_HOLD_S * sr) as u32,
            belt_held: 0.0,
            belt_lp: [0.0; 2],
            a_belt: w * (1.0 - w * (0.5 - w / 6.0)),
            deep: false,
            cassette: false,
            sway: 0.0,
            deep_sway: 0.0,
            #[cfg(test)]
            pack_sway: 0.0,
            fresh: true,
        };
        t.clear();
        t
    }

    /// The cassette from nothing: its rotators at a phase of zero, no
    /// excursion, the belt's noise back at its seed, and its breath where
    /// Wow's wander is now. Where the deep head starts when it is selected,
    /// and what it is reset to, silent, when it has faded out.
    fn reset_cassette(&mut self) {
        self.cas_turn = [[1.0, 0.0]; 2];
        self.cas_step = [[1.0, 0.0]; 2];
        self.cas_omega = [0.0; 2];
        self.cas_exc = [0.0; 3];
        self.cas_aim = [0.0; 3];
        self.belt_rng = BELT_SEED;
        self.belt_hold = 0;
        self.belt_held = 0.0;
        self.belt_lp = [0.0; 2];
        self.cas_breath = self.breaths().0 / DEPTH_WANDER.1;
    }

    /// Back to the moment the machine was built: every phase, noise and
    /// excursion, and the next design puts every part at its target.
    fn clear(&mut self) {
        self.turn = self.start;
        self.step = [[1.0, 0.0]; 4];
        self.omega = [0.0; 4];
        self.exc = [0.0; STUDIO_PARTS];
        self.aim = [0.0; STUDIO_PARTS];
        self.drift_rng = DRIFT_SEED;
        self.drift_hold = 0;
        self.drift_held = 0.0;
        self.drift_lp = [0.0; 2];
        self.res_rng = RESONANCE_SEED;
        self.res_y = [0.0; 2];
        self.scrape_rng = SCRAPE_SEED;
        self.scrape_lp = [0.0; 2];
        self.wander_rng = WANDER_SEED;
        self.wander = [0.5; 2];
        self.wander_target = [0.5; 2];
        self.wander_hold = [0; 2];
        self.reset_cassette();
        self.deep = false;
        self.cassette = false;
        self.sway = 0.0;
        self.deep_sway = 0.0;
        self.fresh = true;
    }

    /// The dials' targets, once a block: Wow's and Flutter's multipliers on
    /// their tapers, and Rate in octaves. `settle` puts the glides there at
    /// once, for the first block after `new` or `clear`.
    fn aim(&mut self, p: &TapeParams, settle: bool) {
        let at = f64::from(DIAL_AT_SPEC);
        self.wow.target = f64::from(norm(p.wobble)) / at;
        self.flutter.target = (f64::from(norm_or(p.flutter, DIAL_AT_SPEC)) / at).powi(2);
        self.rate.target = 2.0 * f64::from(norm_or(p.rate, 0.5)) - 1.0;
        if settle {
            for g in [&mut self.wow, &mut self.flutter, &mut self.rate] {
                g.settle();
            }
        }
        // The deep head's cassette starts from nothing the moment the deep
        // range is selected, and grows at the deep slew.
        self.deep = WowRange::from_dial(norm_or(p.wow_range, WowRange::Studio.dial())) == WowRange::Deep;
        if self.deep && !self.cassette {
            self.reset_cassette();
            self.cassette = true;
        }
    }

    /// The scale each part's target takes from the depth wander: Wow's for
    /// the reel and the pinch roller, Flutter's for the guides, the capstan
    /// and the resonance; the drift and the scrape do not breathe.
    #[inline]
    fn breath(part: usize, wow: f64, flutter: f64) -> f64 {
        match part {
            REEL | PINCH => wow,
            GUIDES | CAPSTAN | RESONANCE => flutter,
            _ => 1.0,
        }
    }

    /// **Once a design: every part's turn and governed target**, for the
    /// glided speed `v` in ips and a `reserve` of samples kept for the
    /// per-channel noise. See the type's doc for the rise limit and the
    /// governor.
    fn design(&mut self, v: f64, reserve: f64) {
        let sr = self.sample_rate;
        let r = self.rate.cur.exp2();
        for k in 0..4 {
            let target = std::f64::consts::TAU * ROTOR_HZ_PER_IPS[k] * v * r / sr;
            let held = self.exc[REEL + k];
            self.omega[k] = if self.fresh || target <= self.omega[k] || held <= 0.0 {
                target
            } else {
                target.min(self.omega[k] * (1.0 + SUB_BLOCK as f64 * self.slew / held))
            };
            self.step[k] = rotation(self.omega[k]);
            let [c, s] = self.turn[k];
            let g = 1.5 - 0.5 * (c * c + s * s);
            self.turn[k] = [c * g, s * g];
        }
        let want = studio_wants(
            v,
            &self.omega,
            [self.wow.cur, self.flutter.cur],
            sr,
            self.res_kappa,
            self.scrape_rms * self.scrape_per_m,
        );
        let asked = at_the_top(&want);
        self.room = (self.travel - reserve).max(0.0);
        let keep = govern(&asked, self.room);
        for j in 0..STUDIO_PARTS {
            self.aim[j] = want[j] * keep[j];
            self.asked[j] = asked[j];
            self.given[j] = asked[j] * keep[j];
        }
        if self.cassette {
            self.design_cassette(r, reserve);
        }
        if self.fresh {
            let (w, f) = self.breaths();
            for j in 0..STUDIO_PARTS {
                self.exc[j] = self.aim[j] * Self::breath(j, w, f);
            }
            let deep = w / DEPTH_WANDER.1;
            self.cas_breath = deep;
            for (j, e) in self.cas_exc.iter_mut().enumerate() {
                *e = self.cas_aim[j] * if j == BELT { 1.0 } else { deep };
            }
            self.fresh = false;
        }
    }

    /// **Once a design, the cassette's turns and targets**, at Rate's
    /// multiple `r`. Its depths are linear in Wow's own dial and ignore the
    /// tape speed; the wow wander breathes them too, through the cassette's
    /// own breath ([`CASSETTE_BREATH`]), scaled so its top is the full
    /// depth. The deep head's room is its travel less the reserve and less
    /// what Flutter's shared parts were given on the studio head, which ride
    /// this one too; were the cassette ever to ask for more, the slowest of
    /// its parts would give way first, the belt, then the pack, then the
    /// roller. It never does: at full Wow and the bottom of Rate it asks for
    /// 19.3 ms, and the least room the head is ever left, at 3.75 ips with
    /// Flutter, Hiss, Age and Damage at their tops and Bias at its bottom,
    /// is 21.4 ms.
    fn design_cassette(&mut self, r: f64, reserve: f64) {
        let sr = self.sample_rate;
        for k in 0..2 {
            let target = std::f64::consts::TAU * CASSETTE_HZ[k] * r / sr;
            let held = self.cas_exc[PACK + k];
            self.cas_omega[k] = if self.fresh || target <= self.cas_omega[k] || held <= 0.0 {
                target
            } else {
                target.min(self.cas_omega[k] * (1.0 + SUB_BLOCK as f64 * self.cas_slew / held))
            };
            self.cas_step[k] = rotation(self.cas_omega[k]);
            let [c, s] = self.cas_turn[k];
            let g = 1.5 - 0.5 * (c * c + s * s);
            self.cas_turn[k] = [c * g, s * g];
        }
        let dial = self.wow.cur * f64::from(DIAL_AT_SPEC);
        let want = [
            BELT_MS * 1.0e-3 * sr * dial,
            CASSETTE_DEPTH * CASSETTE_SHARE[0] * dial / self.cas_omega[0],
            CASSETTE_DEPTH * CASSETTE_SHARE[1] * dial / self.cas_omega[1],
        ];
        let shared: f64 = [GUIDES, CAPSTAN, RESONANCE, SCRAPE].iter().map(|&j| self.given[j]).sum();
        self.deep_room = (self.deep_travel - reserve - shared).max(0.0);
        let mut keep = [1.0_f64; 3];
        let mut over = want.iter().sum::<f64>() - self.deep_room;
        for j in [BELT, PACK, ROLLER] {
            if over <= 0.0 {
                break;
            }
            let cut = want[j].min(over);
            keep[j] = if want[j] > 0.0 { (want[j] - cut) / want[j] } else { 1.0 };
            over -= cut;
        }
        for j in 0..3 {
            self.cas_aim[j] = want[j] * keep[j];
            self.cas_asked[j] = want[j];
            self.cas_given[j] = want[j] * keep[j];
        }
    }

    /// The depth wander's scales now, Wow's and Flutter's.
    #[inline]
    fn breaths(&self) -> (f64, f64) {
        let span = DEPTH_WANDER.1 - DEPTH_WANDER.0;
        (
            DEPTH_WANDER.0 + span * self.wander[0],
            DEPTH_WANDER.0 + span * self.wander[1],
        )
    }

    /// **One frame of the transport**: the glides, the wander, every part's
    /// excursion moved toward its target (shrinking first, growing only into
    /// the room left), every part turned, and the sway they add up to, in
    /// samples.
    #[inline]
    fn tick(&mut self) -> f64 {
        self.wow.tick(self.a_glide);
        self.flutter.tick(self.a_glide);
        self.rate.tick(self.a_rate);
        for g in 0..2 {
            if self.wander_hold[g] == 0 {
                self.wander_target[g] = f64::from(uniform(&mut self.wander_rng));
                self.wander_hold[g] = self.wander_frames[g];
            }
            self.wander_hold[g] -= 1;
            self.wander[g] += (self.wander_target[g] - self.wander[g]) * self.a_wander;
        }
        let (w, f) = self.breaths();
        let mut used = 0.0;
        let mut asking = 0.0;
        let mut grow = [0.0_f64; STUDIO_PARTS];
        for j in 0..STUDIO_PARTS {
            let target = self.aim[j] * Self::breath(j, w, f);
            if target < self.exc[j] {
                self.exc[j] = (self.exc[j] - self.slew).max(target);
            } else {
                grow[j] = (target - self.exc[j]).min(self.slew);
                asking += grow[j];
            }
            used += self.exc[j];
        }
        if asking > 0.0 {
            let fit = ((self.room - used) / asking).clamp(0.0, 1.0);
            for (e, g) in self.exc.iter_mut().zip(grow) {
                *e += g * fit;
            }
        }
        for (turn, step) in self.turn.iter_mut().zip(self.step) {
            let [c, s] = *turn;
            *turn = [c * step[0] - s * step[1], s * step[0] + c * step[1]];
        }

        // The noises: the drift's hold and its two poles, the resonance, the
        // scrape's band. Drawn every frame whatever their depth, so each
        // stream is the same at any setting.
        if self.drift_hold == 0 {
            self.drift_held = f64::from(white(&mut self.drift_rng));
            self.drift_hold = self.drift_frames;
        }
        self.drift_hold -= 1;
        self.drift_lp[0] += (self.drift_held - self.drift_lp[0]) * self.a_drift;
        self.drift_lp[1] += (self.drift_lp[0] - self.drift_lp[1]) * self.a_drift;
        let x = f64::from(white(&mut self.res_rng));
        let y = x + self.res_c[0] * self.res_y[0] + self.res_c[1] * self.res_y[1];
        self.res_y = [y, self.res_y[0]];
        let x = f64::from(white(&mut self.scrape_rng));
        for (lp, a) in self.scrape_lp.iter_mut().zip(self.a_scrape) {
            *lp += (x - *lp) * a;
        }

        // Flutter's parts ride both heads; Wow's studio parts only the
        // studio head.
        let [c, s] = self.turn[3];
        let harmonic = 2.0 * s * c * self.harmonic[0] + (c * c - s * s) * self.harmonic[1];
        let common = -(self.exc[GUIDES] * self.turn[2][1]
            + self.exc[CAPSTAN] / PEAK[CAPSTAN] * (s + CAPSTAN_2ND_EXC * harmonic))
            + self.exc[RESONANCE] / PEAK[RESONANCE] * self.res_unit * y
            + self.exc[SCRAPE] / (PEAK[SCRAPE] * self.scrape_rms) * (self.scrape_lp[1] - self.scrape_lp[0]);
        let wow = -(self.exc[REEL] * self.turn[0][1] + self.exc[PINCH] * self.turn[1][1])
            + self.exc[DRIFT] * self.drift_lp[1];
        self.sway = common + wow;
        self.deep_sway = if self.cassette { common + self.cassette_tick(w) } else { common };
        self.sway
    }

    /// One frame of the cassette, given Wow's wander scale `w`: its breath
    /// moved toward the wander by at most [`CASSETTE_BREATH`] of the pack's
    /// turn this frame, its parts' excursions moved toward their targets
    /// (shrinking first, growing only into the deep head's room), its parts
    /// turned, the belt's drift, and the sway they add up to, in samples.
    #[inline]
    fn cassette_tick(&mut self, w: f64) -> f64 {
        let most = CASSETTE_BREATH * self.cas_omega[0];
        self.cas_breath += (w / DEPTH_WANDER.1 - self.cas_breath).clamp(-most, most);
        let deep = self.cas_breath;
        let mut used = 0.0;
        let mut asking = 0.0;
        let mut grow = [0.0_f64; 3];
        for j in 0..3 {
            let target = self.cas_aim[j] * if j == BELT { 1.0 } else { deep };
            if target < self.cas_exc[j] {
                self.cas_exc[j] = (self.cas_exc[j] - self.cas_slew).max(target);
            } else {
                grow[j] = (target - self.cas_exc[j]).min(self.cas_slew);
                asking += grow[j];
            }
            used += self.cas_exc[j];
        }
        if asking > 0.0 {
            let fit = ((self.deep_room - used) / asking).clamp(0.0, 1.0);
            for (e, g) in self.cas_exc.iter_mut().zip(grow) {
                *e += g * fit;
            }
        }
        for (turn, step) in self.cas_turn.iter_mut().zip(self.cas_step) {
            let [c, s] = *turn;
            *turn = [c * step[0] - s * step[1], s * step[0] + c * step[1]];
        }
        if self.belt_hold == 0 {
            self.belt_held = f64::from(white(&mut self.belt_rng));
            self.belt_hold = self.belt_frames;
        }
        self.belt_hold -= 1;
        self.belt_lp[0] += (self.belt_held - self.belt_lp[0]) * self.a_belt;
        self.belt_lp[1] += (self.belt_lp[0] - self.belt_lp[1]) * self.a_belt;
        #[cfg(test)]
        {
            self.pack_sway = -self.cas_exc[PACK] * self.cas_turn[0][1];
        }
        -(self.cas_exc[PACK] * self.cas_turn[0][1] + self.cas_exc[ROLLER] * self.cas_turn[1][1])
            + self.cas_exc[BELT] * self.belt_lp[1]
    }

    /// The deep head carries no weight any more: the cassette stops, reset
    /// silent, until the deep range is selected again.
    fn stop_cassette(&mut self) {
        self.reset_cassette();
        self.cassette = false;
    }
}

/// **Where the tape is under the head**: the metres that have passed it
/// since `new` or `clear`, and how many pass it a frame at the speed the
/// voicing is at. What wears the tape lies along it, so it is laid out in
/// metres and not in seconds: the azimuth's wander here, which a faster tape
/// sweeps through faster, and the damage along it.
///
/// **The damage is drawn as the tape passes**: two unit-exponential
/// distances, one to the next dropout and one to the next crease, drawn
/// the first time Damage is up and again after each defect, and spent at
/// Damage's rate per metre, so the count is exact under any glide of the
/// speed or the dial, and at Damage 0 nothing is drawn at all. At most two
/// dropouts and one crease play at once; a third dropout takes the place of
/// the one nearer its end.
struct Wear {
    metres: f64,
    x: f64,
    /// Where the damage was last worked out, along the tape.
    stepped: f64,
    rng: u32,
    /// The distances still to go to the next dropout and the next crease,
    /// in expected defects (NaN until Damage first asks for one).
    drop_due: f64,
    crease_due: f64,
    drops: [Option<Defect>; 2],
    crease: Option<Defect>,
    /// Each track's edge: its draws, where the level it is at came from and
    /// where it is going, and where along the tape that stretch began (NaN
    /// before the first).
    edge_rng: [u32; 2],
    edge: [[f64; 2]; 2],
    edge_at: f64,
    #[cfg(test)]
    log: WearLog,
}

/// One defect on the tape: where it starts and how long it is in metres,
/// its peak spacing in metres, the share of it each track takes, and for a
/// crease its ripple's pitch in metres and its twist in radians.
#[derive(Debug, Clone, Copy)]
struct Defect {
    at: f64,
    len: f64,
    peak: f64,
    mask: [f64; 2],
    pitch: f64,
    twist: f64,
}

impl Defect {
    /// How far along itself the tape at `x` is, 0 to 1, or None once past.
    fn along(&self, x: f64) -> Option<f64> {
        let p = (x - self.at) / self.len;
        (p < 1.0).then_some(p.max(0.0))
    }
}

/// What the wear has drawn, for the tests: dropouts and creases begun, the
/// dropouts' lengths summed in metres, and how many took both tracks, the
/// left alone and the right alone.
#[cfg(test)]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct WearLog {
    pub dropouts: u64,
    pub creases: u64,
    pub dropout_m: f64,
    pub masks: [u64; 3],
}

/// A unit-exponential draw: the distance, in expected events, to the next.
fn exponential(rng: &mut u32) -> f64 {
    -f64::from(uniform(rng)).max(1.0e-9).ln()
}

/// A draw between `lo` and `hi`, uniform, or log-uniform when `log`.
fn between(rng: &mut u32, lo: f64, hi: f64, log: bool) -> f64 {
    let u = f64::from(uniform(rng));
    if log {
        lo * (hi / lo).powf(u)
    } else {
        lo + (hi - lo) * u
    }
}

impl Wear {
    const fn new() -> Self {
        Self {
            metres: 0.0,
            x: 0.0,
            stepped: 0.0,
            rng: DAMAGE_SEED,
            drop_due: f64::NAN,
            crease_due: f64::NAN,
            drops: [None; 2],
            crease: None,
            edge_rng: EDGE_SEEDS,
            edge: [[0.0; 2]; 2],
            edge_at: f64::NAN,
            #[cfg(test)]
            log: WearLog {
                dropouts: 0,
                creases: 0,
                dropout_m: 0.0,
                masks: [0; 3],
            },
        }
    }

    fn clear(&mut self) {
        *self = Self {
            metres: self.metres,
            ..Self::new()
        };
    }

    /// One frame of tape past the head.
    #[inline]
    fn tick(&mut self) {
        self.x += self.metres;
    }

    /// The head's azimuth error here, in radians, at Age `age`: its
    /// standing error and the slitting's wander along the tape
    /// ([`AZIMUTH_URAD`]). Exactly zero at Age 0.
    fn azimuth(&self, age: f64) -> f64 {
        let wander = (std::f64::consts::TAU * self.x / AZIMUTH_WANDER_M).sin();
        age * (AZIMUTH_URAD.0 + AZIMUTH_URAD.1 * wander) * 1.0e-6
    }

    /// **The damage where the tape is now**, at Damage `damage` and with
    /// dropouts `deeper` times their depth (the bias's): each track's
    /// spacing in metres from the dropouts, the crease and the edges, and
    /// the crease's twist of the head's azimuth in radians. Spends the
    /// metres since the last call on the draws first.
    fn damage(&mut self, damage: f64, deeper: f64) -> ([f64; 2], f64) {
        let passed = self.x - self.stepped;
        self.stepped = self.x;
        if damage > 0.0 {
            if self.drop_due.is_nan() {
                self.drop_due = exponential(&mut self.rng);
                self.crease_due = exponential(&mut self.rng);
            }
            let square = damage * damage;
            self.drop_due -= DROPOUTS_PER_M * square * passed;
            while self.drop_due <= 0.0 {
                self.dropout(damage, deeper);
                self.drop_due += exponential(&mut self.rng);
            }
            self.crease_due -= CREASES_PER_M * square * passed;
            while self.crease_due <= 0.0 {
                self.crease_at(damage);
                self.crease_due += exponential(&mut self.rng);
            }
        }

        let mut gap = [0.0; 2];
        let x = self.x;
        for slot in &mut self.drops {
            if let Some(d) = *slot {
                match d.along(x) {
                    Some(p) => {
                        let s = d.peak * (std::f64::consts::PI * p).sin().powi(2);
                        gap[0] += s * d.mask[0];
                        gap[1] += s * d.mask[1];
                    }
                    None => *slot = None,
                }
            }
        }
        let mut twist = 0.0;
        if let Some(c) = self.crease {
            match c.along(x) {
                Some(p) => {
                    let rise = (std::f64::consts::PI * p).sin().powi(2);
                    let ripple = (std::f64::consts::PI * (x - c.at) / c.pitch).sin().powi(2);
                    gap[0] += c.peak * rise * ripple;
                    gap[1] += c.peak * rise * ripple;
                    twist = c.twist * rise;
                }
                None => self.crease = None,
            }
        }

        // The edges, laid along the tape whatever the dial, so turning
        // Damage up finds them where the tape is.
        let (every, slew) = (EDGE_MM.0 * 1.0e-3, EDGE_MM.1 * 1.0e-3);
        if self.edge_at.is_nan() {
            for (e, rng) in self.edge.iter_mut().zip(&mut self.edge_rng) {
                let q = f64::from(uniform(rng));
                *e = [q, q];
            }
            self.edge_at = x;
        }
        while x - self.edge_at >= every {
            self.edge_at += every;
            for (e, rng) in self.edge.iter_mut().zip(&mut self.edge_rng) {
                *e = [e[1], f64::from(uniform(rng))];
            }
        }
        let t = ((x - self.edge_at) / slew).min(1.0);
        let depth = EDGE_UM * 1.0e-6 * damage * damage;
        for (g, e) in gap.iter_mut().zip(&self.edge) {
            *g += depth * (e[0] + (e[1] - e[0]) * t);
        }
        (gap, twist)
    }

    /// A dropout begins here ([`DROPOUT_MM`]).
    fn dropout(&mut self, damage: f64, deeper: f64) {
        let len = between(&mut self.rng, DROPOUT_MM.0, DROPOUT_MM.1, true) * 1.0e-3;
        let peak = between(&mut self.rng, DROPOUT_UM.0, DROPOUT_UM.0 + DROPOUT_UM.1 * damage, true) * 1.0e-6 * deeper;
        let pick = uniform(&mut self.rng);
        let (mask, which) = if pick < DROPOUT_BOTH {
            ([1.0, 1.0], 0)
        } else if pick < DROPOUT_BOTH + DROPOUT_LEFT {
            ([1.0, 0.0], 1)
        } else {
            ([0.0, 1.0], 2)
        };
        self.place(Defect {
            at: self.x,
            len,
            peak,
            mask,
            pitch: 1.0,
            twist: 0.0,
        });
        #[cfg(test)]
        {
            self.log.dropouts += 1;
            self.log.dropout_m += len;
            self.log.masks[which] += 1;
        }
        #[cfg(not(test))]
        let _ = which;
    }

    /// Put a dropout in a free place, or in the place of the one nearer its
    /// end.
    fn place(&mut self, d: Defect) {
        let left = |s: &Option<Defect>| s.and_then(|o| o.along(self.x)).map_or(-1.0, |p| 1.0 - p);
        let k = if left(&self.drops[0]) <= left(&self.drops[1]) { 0 } else { 1 };
        self.drops[k] = Some(d);
    }

    /// A crease begins here ([`CREASE_MM`]).
    fn crease_at(&mut self, damage: f64) {
        let len = between(&mut self.rng, CREASE_MM.0, CREASE_MM.1, false) * 1.0e-3;
        let pitch = between(&mut self.rng, CREASE_PITCH_MM.0, CREASE_PITCH_MM.1, false) * 1.0e-3;
        let peak = between(&mut self.rng, CREASE_UM.0, CREASE_UM.0 + CREASE_UM.1 * damage, false) * 1.0e-6;
        let sign = if uniform(&mut self.rng) < 0.5 { -1.0 } else { 1.0 };
        let twist = sign * between(&mut self.rng, CREASE_URAD.0, CREASE_URAD.0 + CREASE_URAD.1 * damage, false) * 1.0e-6;
        self.crease = Some(Defect {
            at: self.x,
            len,
            peak,
            mask: [1.0, 1.0],
            pitch,
            twist,
        });
        #[cfg(test)]
        {
            self.log.creases += 1;
        }
    }

    /// **The wear alone, for the tests**: `frames` frames of tape at `ips`
    /// at `sample_rate`, at Damage `damage` and the bias `bias_db`, worked
    /// out on the grid the kernel works it on ([`WEAR_STEP`]): the wear as
    /// it is left, its log with it.
    #[cfg(test)]
    fn run(ips: f64, sample_rate: f64, damage: f64, bias_db: f64, frames: u64) -> Wear {
        let mut w = Wear::new();
        w.metres = 0.0254 * ips / sample_rate;
        let deeper = 10.0_f64.powf(-bias_db / 20.0);
        for f in 0..frames {
            if f.is_multiple_of(WEAR_STEP) {
                w.damage(damage, deeper);
            }
            w.tick();
        }
        w
    }
}

/// One stereo pair of tracks through the record and repro chain. See the
/// module header for the signal flow.
pub struct Tape {
    sample_rate: f32,
    /// The tape: one line per channel, because the two tracks are magnetised
    /// independently, but read by one head at one place (two while a range
    /// switch fades between the heads), which is why the transport's sway
    /// below is shared; only the modulation noise, which is each track's own
    /// grain, differs between them.
    line: [Vec<f32>; 2],
    at: usize,
    /// The studio head's rest position in whole samples, and the deep
    /// head's. Whole on purpose: with Wow, Flutter and Hiss at zero the tap
    /// interpolates at exactly zero fraction and hands samples through
    /// untouched. Shortened by [`OS_LATENCY`], for the reason [`BASE_MS`]
    /// gives.
    base: usize,
    base_deep: usize,
    /// The two heads' share of what is heard, as the cosine and sine of an
    /// angle from 0 (the studio head alone) to a quarter turn (the deep head
    /// alone): an equal-power crossfade over [`HEAD_FADE_MS`], turned a step
    /// a frame by the rotation `fade_step` and snapped exact at either end.
    fade: f64,
    fade_w: [f64; 2],
    fade_step: [f64; 2],
    fade_delta: f64,
    /// The input transformer, per channel, and its coefficient.
    transformer: [OnePole; 2],
    g_transformer: f32,
    /// The NAB shelf's state on the way onto the tape and off it, per
    /// channel.
    nab_in: [f32; 2],
    nab_out: [f32; 2],
    /// The under-bias low shelf's states on the way onto the tape and off
    /// it, per channel, and the bias shelf's after the curve ([`BIAS_RANGE_DB`]).
    under_in: [[f32; 2]; 2],
    under_out: [[f32; 2]; 2],
    bias_hf: [f32; 2],
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
    /// The hiss: a noise per channel through its speed's playback shelf.
    hiss_rng: [u32; 2],
    hiss_eq: [f32; 2],
    /// The hiss's level on its dial, as a linear gain gliding to it (see
    /// [`Glide`]); the speed's own gain multiplies it.
    hiss_level: Glide,
    /// The modulation noise: its depth in micrometres, gliding to the dial,
    /// and a noise and its band-limit per channel.
    jitter: Glide,
    jitter_rng: [u32; 2],
    jitter_lp: [f32; 2],
    /// The transport: every part that sways the head, shared by both
    /// channels.
    transport: Transport,
    /// How many times a head had to be held inside the line. The governor
    /// keeps every sway inside the head's travel, so this is a safety net
    /// the tests require never to catch anything.
    clamp_hits: u64,
    /// The furthest each head has been asked to sit from its rest, either
    /// way, while it carried weight, in samples: what the travel test reads.
    #[cfg(test)]
    deepest: [f32; 2],
    /// Holds an aged head's gap square to the tracks in time, so neither
    /// track is read between samples for the skew: what lets a test hear
    /// Age's spacing without the play head's interpolation on top.
    #[cfg(test)]
    square_head: bool,
    // Per-rate coefficients, fixed at construction: the sample rate is, too,
    // per the house law.
    a_att: f32,
    a_rel: f32,
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
    hiss_shelf: Section,
    hiss_gain: f32,
    jitter_a: f32,
    jitter_norm: f32,
    /// Samples of play-head delay per micrometre of spacing jitter, at the
    /// speed the voicing is at: the tape's speed, inverted, at this rate.
    per_um: f32,
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
    /// The bias's stages: the under-bias low shelf and its inverse, the
    /// bias shelf after the curve, whether each is in circuit (each is a
    /// wire at aligned, and skipped), and the `(bias dB, speed, Age)` they
    /// were designed at (NaN until the first design; Age's worn head is the
    /// treble shelf's too).
    ///
    /// **The inverse follows the shelf [`OS_LATENCY`] frames late**, because
    /// the two straddle the record stage's oversampler: what the shelf
    /// shapes at one frame leaves the curve that many frames later, and the
    /// inverse must meet it with the same design, or every design while the
    /// bias glides is a 44-frame step in the bass, a click. So each design's
    /// inverse waits in `inverse_due` with the frame it takes over at (at
    /// most two wait, designs being 32 frames apart).
    under: Shelf,
    under_on: bool,
    under_inv: Shelf,
    under_inv_on: bool,
    inverse_due: [(u64, Shelf, bool); 2],
    inverse_waiting: usize,
    shelf: Section,
    shelf_on: bool,
    bias_for: (f64, f64, f64),
    /// The line-up's reading of the bias, as how many octaves hotter the
    /// 1 kHz alignment tone meets the curve through the under-bias shelf:
    /// where it was at the last design and where it is going, ramped across
    /// the sub-block so the line-up never steps while the bias glides.
    lineup_from: f64,
    lineup_to: f64,
    /// `tan(pi 1 kHz / rate)`, and 1 kHz and 10 kHz (or a quarter of the
    /// rate) as their hertz and `cos w` at this rate: where the bias's
    /// stages are held to the alignment tone and the ladders.
    tan_1k: f64,
    shelf_points: [(f64, f64); 2],
    /// The glides: the speed as its position on [`TapeSpeed::ALL`] (which is
    /// `log2` of the speed, since the speeds are octaves apart), the dial's
    /// record level as `log2 d`, the bump in dB, Roll's corner as `log2` Hz,
    /// and the output trim as a gain.
    speed: Glide,
    drive: Glide,
    bump_db: Glide,
    roll: Glide,
    trim: Glide,
    /// The bias in dB re aligned ([`BIAS_RANGE_DB`]).
    bias: Glide,
    /// Age, and its per-frame step ([`AGE_GLIDE_MS`]).
    age: Glide,
    a_age: f64,
    /// Damage, gliding as the other controls do.
    damage: Glide,
    /// Each track's spacing at the wear's last step, in metres.
    #[cfg(test)]
    gaps: [f64; 2],
    /// Where the tape is under the head ([`Wear`]).
    wear: Wear,
    /// **The wear's grid**, [`WEAR_STEP`] frames: the share of the tracks'
    /// azimuth skew each track takes, in samples (the left's; the right
    /// takes it the other way), and each track's spacing filter, its two
    /// matched one-poles' `b` (the shelf's corner and the double low-pass's),
    /// where the last step put them and where this one is taking them.
    skew_from: f64,
    skew_to: f64,
    spacing_from: [[f32; 2]; 2],
    spacing_to: [[f32; 2]; 2],
    /// The spacing filter: each track's three one-poles, whether it is in
    /// circuit (only while some track has a spacing at all), whether it
    /// has to start from the signal it meets, and the table its
    /// coefficients come from ([`SPACING_POINTS`]).
    spacing: [[f32; 3]; 2],
    spacing_on: bool,
    spacing_fresh: bool,
    spacing_table: [f32; SPACING_POINTS],
    /// The magnetised head: each track's DC blocker, its coefficient, and
    /// whether the stage is in circuit ([`MAGNETISED_H2`]).
    dc: [OnePole; 2],
    g_dc: f32,
    h2_on: bool,
    /// Age's lift on the hiss, as a gain ([`AGE_HISS_DB`]).
    hiss_age: f32,
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
        let base_deep = head_rest(sr, DEEP_MS);
        let fade_delta = std::f64::consts::FRAC_PI_2 / (HEAD_FADE_MS * 1.0e-3 * f64::from(sr)).max(1.0);
        let rate = f64::from(sr);
        let step = |ms: f64| 1.0 - (-1.0 / (rate * ms * 1.0e-3)).exp();
        Self {
            sample_rate: sr,
            line: [vec![0.0; cap], vec![0.0; cap]],
            at: 0,
            base,
            base_deep,
            fade: 0.0,
            fade_w: [1.0, 0.0],
            fade_step: [fade_delta.cos(), fade_delta.sin()],
            fade_delta,
            transformer: [OnePole::default(); 2],
            g_transformer: tpt_g(TRANSFORMER_HZ, rate) as f32,
            nab_in: [0.0; 2],
            nab_out: [0.0; 2],
            under_in: [[0.0; 2]; 2],
            under_out: [[0.0; 2]; 2],
            bias_hf: [0.0; 2],
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
            hiss_rng: HISS_SEEDS,
            hiss_eq: [0.0; 2],
            hiss_level: Glide::new(1.0e-9),
            jitter: Glide::new(1.0e-9),
            jitter_rng: JITTER_SEEDS,
            jitter_lp: [0.0; 2],
            transport: Transport::new(rate, base, base_deep),
            clamp_hits: 0,
            #[cfg(test)]
            deepest: [0.0; 2],
            #[cfg(test)]
            square_head: false,
            a_att: env_a(rate, f64::from(ERASE_ATTACK_MS)),
            a_rel: env_a(rate, f64::from(ERASE_RELEASE_MS)),
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
            hiss_shelf: Section::WIRE,
            hiss_gain: 0.0,
            jitter_a: 0.0,
            jitter_norm: 0.0,
            per_um: 0.0,
            bump_bell: Bell::WIRE,
            dip_bell: Bell::WIRE,
            bells_for: (f64::NAN, f64::NAN, f64::NAN),
            roll_a: 0.0,
            roll_for: f64::NAN,
            roll_on: false,
            under: Shelf::WIRE,
            under_on: false,
            under_inv: Shelf::WIRE,
            under_inv_on: false,
            inverse_due: [(0, Shelf::WIRE, false); 2],
            inverse_waiting: 0,
            shelf: Section::WIRE,
            shelf_on: false,
            bias_for: (f64::NAN, f64::NAN, f64::NAN),
            lineup_from: 0.0,
            lineup_to: 0.0,
            tan_1k: (std::f64::consts::PI * 1_000.0 / rate).tan(),
            shelf_points: [1_000.0, 10_000.0_f64.min(0.25 * rate)]
                .map(|hz| (hz, (std::f64::consts::TAU * hz / rate).cos())),
            speed: Glide::new(1.0e-7),
            drive: Glide::new(1.0e-7),
            bump_db: Glide::new(1.0e-6),
            roll: Glide::new(1.0e-6),
            trim: Glide::new(1.0e-9),
            bias: Glide::new(1.0e-6),
            age: Glide::new(1.0e-9),
            a_age: step(AGE_GLIDE_MS),
            damage: Glide::new(1.0e-9),
            #[cfg(test)]
            gaps: [0.0; 2],
            wear: Wear::new(),
            skew_from: 0.0,
            skew_to: 0.0,
            spacing_from: [[0.0; 2]; 2],
            spacing_to: [[0.0; 2]; 2],
            spacing: [[0.0; 3]; 2],
            spacing_on: false,
            spacing_fresh: false,
            spacing_table: std::array::from_fn(|i| {
                let hz = (SPACING_LOW_HZ.log2() + i as f64 / SPACING_PER_OCTAVE).exp2();
                matched_b(hz, rate) as f32
            }),
            dc: [OnePole::default(); 2],
            g_dc: tpt_g(MAGNETISED_DC_HZ, rate) as f32,
            h2_on: false,
            hiss_age: 1.0,
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
    /// `builtin_fx::DenoiseUnit::set_param`). **Only the wow range moves
    /// it**, the owner's ruling: the studio head reports [`BASE_MS`] and the
    /// deep head [`DEEP_MS`], and every other setting answers the same, the
    /// speed included, because record and playback change speed together. A
    /// range switch re-times the lane at once, while the heads' own
    /// crossfade keeps the tape's output clean.
    pub fn latency_frames(&self, p: &TapeParams) -> usize {
        let head = match WowRange::from_dial(norm_or(p.wow_range, WowRange::Studio.dial())) {
            WowRange::Studio => self.base,
            WowRange::Deep => self.base_deep,
        };
        head + OS_LATENCY
    }

    /// The most a tape built at `sample_rate` can ever report, at any
    /// setting: what a seat sizes a line by when it has to hold the tape's
    /// delay itself, which a bypassed bay does (`builtin_fx::TapeUnit`
    /// keeps the dry signal as late as the tape, so switching it out
    /// compares two sounds and not two timings). The deep head's figure.
    pub fn max_latency_frames(sample_rate: f32) -> usize {
        head_rest(sample_rate, DEEP_MS) + OS_LATENCY
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
        self.drive.target = dial_log2_d(drive);
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
        let taper = f64::from(hiss / 0.25).powi(2);
        self.hiss_level.target = taper * f64::from(noise_rate);
        self.jitter.target = jitter_um(p);
        self.bias.target = bias_db(p);
        self.age.target = f64::from(norm_or(p.age, 0.0));
        self.damage.target = f64::from(norm_or(p.damage, 0.0));
        self.transport.aim(p, self.fresh);
        if self.fresh {
            // And the heads stand where the range is, not fading to it.
            if self.transport.deep {
                (self.fade, self.fade_w) = (std::f64::consts::FRAC_PI_2, [0.0, 1.0]);
            } else {
                (self.fade, self.fade_w) = (0.0, [1.0, 0.0]);
            }
            // The first block after `new` or `clear` starts AT its controls,
            // not gliding to them from wherever the glides were left.
            for g in [
                &mut self.speed,
                &mut self.drive,
                &mut self.bump_db,
                &mut self.roll,
                &mut self.trim,
                &mut self.hiss_level,
                &mut self.jitter,
                &mut self.bias,
                &mut self.age,
                &mut self.damage,
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
            self.hiss_shelf = Section::of([
                mix(a.hiss_eq[0], b.hiss_eq[0]),
                mix(a.hiss_eq[1], b.hiss_eq[1]),
                mix(a.hiss_eq[2], b.hiss_eq[2]),
            ]);
            self.hiss_gain = mix(a.hiss_gain, b.hiss_gain).exp2() as f32;
            self.jitter_a = mix(a.jitter_a, b.jitter_a) as f32;
            self.jitter_norm = mix(a.jitter_norm, b.jitter_norm) as f32;
            let v_m = 0.0254 * TapeSpeed::Ips3_75.ips() * pos.exp2();
            self.per_um = (1.0e-6 * f64::from(self.sample_rate) / v_m) as f32;
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
        self.design_bias(pos);

        // Age's own parts: the hiss's lift, and the magnetised head, which
        // leaves the circuit only once its DC blocker has nothing left in it.
        let age = self.age.cur;
        self.hiss_age = 10.0_f64.powf(AGE_HISS_DB * age / 20.0) as f32;
        if age > 0.0 {
            self.h2_on = true;
        } else if self.h2_on && self.dc.iter().all(|f| f.s.abs() < 1.0e-9) {
            self.h2_on = false;
            self.dc = [OnePole::default(); 2];
        }

        // The transport, at the speed the glide is at. Its reserve is what
        // the per-channel noise can take beyond the sway, four of its sigma
        // and a sample, at the deeper of where the noise and the speed are
        // and where they are going, so it is in place before either arrives.
        let ips = TapeSpeed::Ips3_75.ips();
        let v = ips * pos.exp2();
        let slowest = v.min(ips * self.speed.target.exp2());
        let noise_um = self.jitter.cur.max(self.jitter.target);
        let sr = f64::from(self.sample_rate);
        let widest = widest_urad(
            self.age.cur.max(self.age.target),
            self.damage.cur.max(self.damage.target),
        );
        self.transport.design(v, reserve(noise_um, widest, slowest, sr));
        self.wear.metres = 0.0254 * v / sr;
    }

    /// **One step of the wear's grid**, every [`WEAR_STEP`] frames: the
    /// head's azimuth where the tape is now (Age's, and a crease's twist),
    /// the skew it puts between the tracks, and each track's spacing (Age's,
    /// the gap that costs what the azimuth costs, and the damage's), as the
    /// coefficients the next frames move to. The
    /// spacing filter is in circuit only while some track has a spacing at
    /// all, and starts from the signal it meets when it comes back in: at no
    /// spacing its one-poles are wires that carry nothing.
    fn wear_step(&mut self) {
        let sr = f64::from(self.sample_rate);
        let v_m = self.wear.metres * sr;
        let age = self.age.cur;
        let deeper = 10.0_f64.powf(-self.bias.cur / 20.0);
        let (defects, twist) = self.wear.damage(self.damage.cur, deeper);
        let theta = self.wear.azimuth(age) + twist;
        let skew = if v_m > 0.0 {
            0.5 * AZIMUTH_SPAN_MM * 1.0e-3 * theta.tan() / v_m * sr
        } else {
            0.0
        };
        let gap = AGE_SPACING_UM * 1.0e-6 * age + if v_m > 0.0 { azimuth_gap(theta, v_m) } else { 0.0 };
        let gaps = [gap + defects[0], gap + defects[1]];
        #[cfg(test)]
        {
            self.gaps = gaps;
        }
        // The first frame since `new` or `clear` (`frame` already counts
        // it) starts where it is, not ramping in from nothing.
        let first = self.frame == 1;
        let to = gaps.map(|d| self.spacing_bs(d, v_m));
        let on = gaps.iter().any(|d| *d > 0.0) || self.spacing_to.iter().flatten().any(|b| *b != 0.0);
        if on && !self.spacing_on {
            self.spacing_fresh = true;
        }
        self.spacing_on = on;
        self.skew_from = if first { skew } else { self.skew_to };
        self.skew_to = skew;
        self.spacing_from = if first { to } else { self.spacing_to };
        self.spacing_to = to;
    }

    /// A track's spacing filter for a spacing of `d` metres on tape at `v_m`
    /// metres a second: the `b` of its shelf's one-pole and of its double
    /// low-pass ([`SPACING_FIT`]), read from the table. No spacing is a wire,
    /// and so is one that is not a finite number of metres: no setting
    /// reaches that (the dials read through [`norm_or`], and the seat washes
    /// what it hands in), but a NaN would otherwise fall through to the
    /// table's 16 Hz end and hold the track near silence.
    fn spacing_bs(&self, d: f64, v_m: f64) -> [f32; 2] {
        if !(d.is_finite() && d > 0.0 && v_m > 0.0) {
            return [0.0; 2];
        }
        let f0 = (v_m / (WALLACE_DB * d)).log2();
        let (_, hp, lp) = SPACING_FIT;
        [hp, lp].map(|k| {
            let x = (f0 + k.log2() - SPACING_LOW_HZ.log2()) * SPACING_PER_OCTAVE;
            let last = (SPACING_POINTS - 1) as f64;
            if x >= last {
                return 0.0;
            }
            let x = x.max(0.0);
            let i = x as usize;
            let t = (x - i as f64) as f32;
            self.spacing_table[i] + (self.spacing_table[i + 1] - self.spacing_table[i]) * t
        })
    }

    /// One frame of the wear: the tape moves on, the grid steps when it is
    /// due, and the tracks' skew for this frame, the left's share in
    /// samples, comes back.
    #[inline]
    fn wear_frame(&mut self) -> f32 {
        if (self.frame - 1).is_multiple_of(WEAR_STEP) {
            self.wear_step();
        }
        self.wear.tick();
        #[cfg(test)]
        if self.square_head {
            return 0.0;
        }
        let along = ((self.frame - 1) % WEAR_STEP + 1) as f64 / WEAR_STEP as f64;
        (self.skew_from + (self.skew_to - self.skew_from) * along) as f32
    }

    /// **The bias's stages for where its glide and the speed's are now**,
    /// part of each design: the under-bias low shelf and its inverse at
    /// [`UNDER_BIAS_UM`] on this speed, the bias shelf at [`BIAS_SHELF_UM`],
    /// and the record level the line-up reads for the 1 kHz tone. Each stage
    /// is in circuit only off the aligned point, where it is a wire, and
    /// starts again from rest when it comes back in: a glide through aligned
    /// takes it out at the last sub-block's small gain, and the inverse goes
    /// with that design, its frames later, so nothing steps. Nothing here
    /// runs while neither the bias nor the speed moves.
    fn design_bias(&mut self, pos: f64) {
        let (b, age) = (self.bias.cur, self.age.cur);
        if b == self.bias_for.0 && pos == self.bias_for.1 && age == self.bias_for.2 {
            self.lineup_from = self.lineup_to;
            return;
        }
        let first = self.bias_for.0.is_nan();
        let sr = f64::from(self.sample_rate);
        let v_m = 0.0254 * TapeSpeed::Ips3_75.ips() * pos.exp2();
        let under = (-b).max(0.0);
        let mut lineup = 0.0;
        if under > 0.0 {
            let db = UNDER_BIAS_DB_PER_DB * under;
            let t = bell_g(v_m / (UNDER_BIAS_UM * 1.0e-6), sr);
            self.under = Shelf::low(t, db);
            self.under_on = true;
            self.inverse_after(Shelf::low(t, -db), true, first);
            lineup = Shelf::low_gain(t, self.tan_1k, db).log2();
        } else if self.under_on {
            self.under_on = false;
            self.under_in = [[0.0; 2]; 2];
            self.inverse_after(Shelf::WIRE, false, first);
        }
        // A worn head records as if biased hotter, on the treble shelf only.
        let height = bias_shelf_db(b + AGE_OVERBIAS_DB * age);
        if height != 0.0 {
            self.shelf = Section::of(Section::bias_shelf(v_m / (BIAS_SHELF_UM * 1.0e-6), height, self.shelf_points));
            self.shelf_on = true;
        } else if self.shelf_on {
            self.shelf_on = false;
            self.bias_hf = [0.0; 2];
        }
        self.lineup_from = if first { lineup } else { self.lineup_to };
        self.lineup_to = lineup;
        self.bias_for = (b, pos, age);
    }

    /// Queue the under-bias shelf's inverse to take over [`OS_LATENCY`]
    /// frames from now, when what the shelf just started shaping leaves the
    /// curve; on the first design after `new` or `clear` there is nothing in
    /// the curve yet, and it takes over at once.
    fn inverse_after(&mut self, inverse: Shelf, on: bool, now: bool) {
        if now {
            (self.under_inv, self.under_inv_on, self.inverse_waiting) = (inverse, on, 0);
            return;
        }
        if self.inverse_waiting == self.inverse_due.len() {
            self.inverse_take();
        }
        self.inverse_due[self.inverse_waiting] = (self.frame + OS_LATENCY as u64, inverse, on);
        self.inverse_waiting += 1;
    }

    /// The oldest waiting inverse takes over: in circuit with its design, or
    /// out of it, starting again from rest.
    fn inverse_take(&mut self) {
        let (_, inverse, on) = self.inverse_due[0];
        self.inverse_due[0] = self.inverse_due[1];
        self.inverse_waiting -= 1;
        if !on {
            self.under_out = [[0.0; 2]; 2];
        }
        (self.under_inv, self.under_inv_on) = (inverse, on);
    }

    /// One frame's turn of the waiting inverses: the one due at the frame
    /// now being made (`frame` already counts it) takes over.
    #[inline]
    fn inverse_turn(&mut self) {
        if self.inverse_waiting > 0 && self.frame > self.inverse_due[0].0 {
            self.inverse_take();
        }
    }

    /// **The two heads this frame**: each track's delay on the studio head
    /// and on the deep head, in samples (the transport's sway for that head,
    /// shared, plus the track's own modulation noise and its share of the
    /// azimuth's `skew`, the left late and the right early, held inside the
    /// line and counted when it had to be), and the two heads' shares of what is
    /// heard. The range's crossfade turns a step here every frame; a head
    /// that carries no weight is neither placed nor counted, and once the
    /// deep head has faded out its cassette stops, reset silent.
    #[inline]
    fn heads(&mut self, skew: f32) -> ([[f32; 2]; 2], [f32; 2]) {
        self.transport.tick();
        let deep = self.transport.deep;
        let half = std::f64::consts::FRAC_PI_2;
        let [cos, sin] = self.fade_step;
        if deep && self.fade < half {
            let [c, s] = self.fade_w;
            self.fade_w = [c * cos - s * sin, s * cos + c * sin];
            self.fade += self.fade_delta;
            if self.fade + 0.5 * self.fade_delta >= half {
                (self.fade, self.fade_w) = (half, [0.0, 1.0]);
            }
        } else if !deep && self.fade > 0.0 {
            let [c, s] = self.fade_w;
            self.fade_w = [c * cos + s * sin, s * cos - c * sin];
            self.fade -= self.fade_delta;
            if self.fade - 0.5 * self.fade_delta <= 0.0 {
                (self.fade, self.fade_w) = (0.0, [1.0, 0.0]);
            }
        }
        if !deep && self.fade == 0.0 && self.transport.cassette {
            self.transport.stop_cassette();
        }

        // The modulation noise: each channel's head jitters by its own
        // band-limited noise, unit-RMS, times the depth in samples. At Hiss 0
        // the depth glides to exactly nothing and no noise is drawn, so a
        // still tape sits on whole samples.
        let depth = self.jitter.tick(self.a_glide) as f32 * self.per_um;
        let mut jitter = [0.0_f32; 2];
        if depth > 0.0 {
            for (ch, j) in jitter.iter_mut().enumerate() {
                let w = white(&mut self.jitter_rng[ch]) * 3.0_f32.sqrt();
                self.jitter_lp[ch] = flush(self.jitter_lp[ch] + self.jitter_a * (w - self.jitter_lp[ch]));
                *j = self.jitter_lp[ch] * self.jitter_norm * depth;
            }
        }
        let sways = [self.transport.sway as f32, self.transport.deep_sway as f32];
        let rests = [self.base as f32, self.base_deep as f32];
        let weights = self.fade_w.map(|w| w as f32);
        let last = (self.line[0].len() - 4) as f32;
        let mut delays = [[0.0_f32; 2]; 2];
        for h in 0..2 {
            if weights[h] == 0.0 {
                continue;
            }
            for ((d, j), s) in delays[h].iter_mut().zip(jitter).zip([skew, -skew]) {
                let want = rests[h] + sways[h] + j + s;
                let held = want.clamp(2.0, last);
                if held != want {
                    self.clamp_hits += 1;
                }
                #[cfg(test)]
                {
                    self.deepest[h] = self.deepest[h].max((want - rests[h]).abs());
                }
                *d = held;
            }
        }
        (delays, weights)
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
        self.aim(p);
        let roll_wanted = norm(p.roll) > 1.0e-3;
        let n = self.line[0].len();
        let e_scale = (ERASE_POINTS - 1) as f32 / ERASE_CLAMP as f32;
        for i in 0..frames {
            let f = i * channels;
            if self.frame.is_multiple_of(SUB_BLOCK) {
                self.design(roll_wanted);
            }
            self.frame += 1;
            self.inverse_turn();

            // ── the glides, and the record level they make ─────────────────
            self.speed.tick(self.a_speed);
            self.bump_db.tick(self.a_glide);
            self.roll.tick(self.a_glide);
            self.bias.tick(self.a_glide);
            self.damage.tick(self.a_glide);
            let h2 = MAGNETISED_H2 * self.age.tick(self.a_age) as f32;
            let trim_gain = self.trim.tick(self.a_glide) as f32;
            // The dial's record level, scaled by this speed's headroom: the
            // `d` the curve is asked for, and the line-up for it, read from
            // the table [`Tape::new`] solved (see [`sat_makeup_refused`]) at
            // the level the 1 kHz alignment tone meets the curve, which the
            // under-bias shelf raises ([`BIAS_RANGE_DB`]), ramped across the
            // sub-block.
            let log2_d = self.drive.tick(self.a_glide) + self.dmol;
            let along = ((self.frame - 1) % SUB_BLOCK + 1) as f64 / SUB_BLOCK as f64;
            let lineup = self.lineup_from + (self.lineup_to - self.lineup_from) * along;
            let makeup = makeup_at(&self.makeup, log2_d + lineup);
            let d = log2_d.exp2() as f32;
            let inv_d = 1.0 / d;

            // ── the heads: the transport's sway and each track's noise ───
            let skew = self.wear_frame();
            let (delays, [studio, deep]) = self.heads(skew);
            // The tape's noise over the record level ([`HISS_DRIVE`]): the
            // voicing's gain carries the speed's `dmol`, which `1 / d`
            // takes back out. Age raises it.
            let hiss_gain = self.hiss_level.tick(self.a_glide) as f32 * self.hiss_gain * inv_d * self.hiss_age;
            let wearing = ((self.frame - 1) % WEAR_STEP + 1) as f32 / WEAR_STEP as f32;

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

                // Onto the tape through the NAB record shelf and, under the
                // aligned bias, the under-bias shelf, the record amplifier
                // and the tape at 4× (see [`record`]), the repro amplifier's
                // line-up (not a knob: see [`sat_makeup`]), and off it
                // through both shelves' exact inverses, then the bias's own
                // treble shelf.
                let mut r = self.nab.run(&mut self.nab_in[ch], u);
                if self.under_on {
                    r = self.under.run(&mut self.under_in[ch], r);
                }
                let mut y = record(&mut self.os[ch], &self.os_taps, &mut self.sat_err[ch], d, inv_d, r) * makeup;
                // A magnetised head's second harmonic, on the flux the tape
                // holds, and its rectified DC taken off again.
                if self.h2_on {
                    y += h2 * ((y * y + MAGNETISED_KNEE * MAGNETISED_KNEE).sqrt() - MAGNETISED_KNEE);
                    y = self.dc[ch].high(y, self.g_dc);
                }
                if self.under_inv_on {
                    y = self.under_inv.run(&mut self.under_out[ch], y);
                }
                let mut y = self.nab_inv.run(&mut self.nab_out[ch], y);
                if self.shelf_on {
                    y = self.shelf.run(&mut self.bias_hf[ch], y);
                }

                // HF self-erasure: the corner falls as the level rises, read
                // from this speed's table. At rest `b` is zero: a wire.
                let e = self.erase_env[ch].min(ERASE_CLAMP as f32) * e_scale;
                let k = (e as usize).min(ERASE_POINTS - 2);
                let b = self.erase[k] + (self.erase[k + 1] - self.erase[k]) * (e - k as f32);
                self.erase_lp[ch] = flush(y + b * (self.erase_lp[ch] - y));
                self.line[ch][self.at] = self.erase_lp[ch];

                // The play head, swaying, and the head's own top. Both heads
                // are read only while a range switch fades between them.
                let v = if deep == 0.0 {
                    tap(&self.line[ch], self.at, delays[0][ch])
                } else if studio == 0.0 {
                    tap(&self.line[ch], self.at, delays[1][ch])
                } else {
                    studio * tap(&self.line[ch], self.at, delays[0][ch])
                        + deep * tap(&self.line[ch], self.at, delays[1][ch])
                };
                self.top[ch] = flush(v + self.top_b * (self.top[ch] - v));
                let mut v = self.top[ch];

                // The hiss: white noise behind the speed's playback shelf,
                // which the head contour and Roll then shape as they shape
                // the music. A level glided to exactly nothing skips the
                // stage.
                if hiss_gain > 0.0 {
                    let w = white(&mut self.hiss_rng[ch]);
                    v += self.hiss_shelf.run(&mut self.hiss_eq[ch], w) * hiss_gain;
                }

                // The head-to-tape spacing, Wallace's loss as three
                // one-poles ([`SPACING_FIT`]): after the hiss, so whatever
                // lifts the tape off the head dulls the hiss as well.
                if self.spacing_on {
                    let st = &mut self.spacing[ch];
                    if self.spacing_fresh {
                        *st = [v; 3];
                    }
                    let [from, to] = [self.spacing_from[ch], self.spacing_to[ch]];
                    let hp = from[0] + (to[0] - from[0]) * wearing;
                    let lp = from[1] + (to[1] - from[1]) * wearing;
                    st[0] = flush(v + hp * (st[0] - v));
                    let shelved = v - SPACING_FIT.0 as f32 * (v - st[0]);
                    st[1] = flush(shelved + lp * (st[1] - shelved));
                    st[2] = flush(st[1] + lp * (st[2] - st[1]));
                    v = st[2];
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
            self.spacing_fresh = false;

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
        self.under_in = [[0.0; 2]; 2];
        self.under_out = [[0.0; 2]; 2];
        self.bias_hf = [0.0; 2];
        (self.under_on, self.under_inv_on, self.shelf_on) = (false, false, false);
        self.inverse_waiting = 0;
        self.wear.clear();
        (self.skew_from, self.skew_to) = (0.0, 0.0);
        (self.spacing_from, self.spacing_to) = ([[0.0; 2]; 2], [[0.0; 2]; 2]);
        self.spacing = [[0.0; 3]; 2];
        (self.spacing_on, self.spacing_fresh) = (false, false);
        self.dc = [OnePole::default(); 2];
        self.h2_on = false;
        self.hiss_age = 1.0;
        self.bias_for = (f64::NAN, f64::NAN, f64::NAN);
        (self.lineup_from, self.lineup_to) = (0.0, 0.0);
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
        self.hiss_rng = HISS_SEEDS;
        self.hiss_eq = [0.0; 2];
        self.jitter_rng = JITTER_SEEDS;
        self.jitter_lp = [0.0; 2];
        self.transport.clear();
        (self.fade, self.fade_w) = (0.0, [1.0, 0.0]);
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

    /// **The transport alone, for `frames` frames at `p`**, as one block of
    /// `process` would run it: the same designs on the same frame grid, the
    /// same glides, the heads and their per-channel noise (counted against
    /// the line as `process` counts them), and no audio. Returns the sway of
    /// the head the range selects, in samples, without the per-channel
    /// noise. Called again it carries on, so a test can automate it block by
    /// block.
    #[cfg(test)]
    pub(crate) fn transport_trace(&mut self, p: &TapeParams, frames: usize) -> Vec<f64> {
        self.trace_of(p, frames, |t| if t.deep { t.deep_sway } else { t.sway })
    }

    /// [`Tape::transport_trace`], reading the cassette's pack alone.
    #[cfg(test)]
    pub(crate) fn pack_trace(&mut self, p: &TapeParams, frames: usize) -> Vec<f64> {
        self.trace_of(p, frames, |t| t.pack_sway)
    }

    /// The hooks' one road: the transport run as `process` runs it, `read`
    /// off it every frame.
    #[cfg(test)]
    fn trace_of(&mut self, p: &TapeParams, frames: usize, read: impl Fn(&Transport) -> f64) -> Vec<f64> {
        self.aim(p);
        let roll_wanted = norm(p.roll) > 1.0e-3;
        let mut sway = Vec::with_capacity(frames);
        for _ in 0..frames {
            if self.frame.is_multiple_of(SUB_BLOCK) {
                self.design(roll_wanted);
            }
            self.frame += 1;
            self.inverse_turn();
            self.speed.tick(self.a_speed);
            self.bump_db.tick(self.a_glide);
            self.roll.tick(self.a_glide);
            self.trim.tick(self.a_glide);
            self.bias.tick(self.a_glide);
            self.damage.tick(self.a_glide);
            self.age.tick(self.a_age);
            self.drive.tick(self.a_glide);
            let skew = self.wear_frame();
            self.heads(skew);
            self.hiss_level.tick(self.a_glide);
            sway.push(read(&self.transport));
        }
        sway
    }

    /// What the governor asked of the heads and gave each part at its last
    /// design.
    #[cfg(test)]
    pub(crate) fn governed(&self) -> GovernorView {
        let t = &self.transport;
        GovernorView {
            asked: t.asked,
            given: t.given,
            omega: t.omega,
            travel: t.travel,
            room: t.room,
            cassette_asked: t.cas_asked,
            cassette_given: t.cas_given,
            cassette_omega: t.cas_omega,
            deep_travel: t.deep_travel,
            deep_room: t.deep_room,
        }
    }

    /// The two heads' shares of what is heard now, studio and deep.
    #[cfg(test)]
    pub(crate) fn head_weights(&self) -> (f32, f32) {
        (self.fade_w[0] as f32, self.fade_w[1] as f32)
    }

    /// **Lay a dropout on the tape where it is under the head now**, on
    /// both tracks: `peak_um` micrometres of spacing at its middle, over
    /// `len_mm` millimetres of tape.
    #[cfg(test)]
    pub(crate) fn force_dropout(&mut self, peak_um: f64, len_mm: f64) {
        self.wear.place(Defect {
            at: self.wear.x,
            len: len_mm * 1.0e-3,
            peak: peak_um * 1.0e-6,
            mask: [1.0, 1.0],
            pitch: 1.0,
            twist: 0.0,
        });
    }

    /// **Lay a crease on the tape where it is under the head now**:
    /// `peak_um` micrometres of spacing at its middle rippled every
    /// `pitch_mm`, twisting the head `twist_urad` off square there, over
    /// `len_mm` millimetres of tape.
    #[cfg(test)]
    pub(crate) fn force_crease(&mut self, peak_um: f64, len_mm: f64, pitch_mm: f64, twist_urad: f64) {
        self.wear.crease = Some(Defect {
            at: self.wear.x,
            len: len_mm * 1.0e-3,
            peak: peak_um * 1.0e-6,
            mask: [1.0, 1.0],
            pitch: pitch_mm * 1.0e-3,
            twist: twist_urad * 1.0e-6,
        });
    }

    /// Each track's spacing at the wear's last step, in micrometres.
    #[cfg(test)]
    pub(crate) fn spacing_um(&self) -> [f64; 2] {
        self.gaps.map(|g| g * 1.0e6)
    }

    /// How many times a head has had to be held inside the line.
    #[cfg(test)]
    pub(crate) fn clamp_hits(&self) -> u64 {
        self.clamp_hits
    }

    /// The furthest each head, studio and deep, has been asked to sit from
    /// its rest while it carried weight, in samples.
    #[cfg(test)]
    pub(crate) fn deepest(&self) -> [f32; 2] {
        self.deepest
    }
}

/// **What the governor did at its last design**, for the tests: every
/// studio part's excursion as it asked for it and as it was given it, in
/// samples at the top of the depth wander, in the governor's order
/// (`DRIFT..=SCRAPE`); the rotating parts' turns in radians a frame
/// (`REEL..=CAPSTAN` as 0..4); the studio head's travel either way of its
/// rest, and the room the parts share after the per-channel reserve; and the
/// same for the cassette on the deep head (`BELT..=ROLLER`, the pack's and
/// the roller's turns), its room what is left after Flutter's shared parts.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct GovernorView {
    pub asked: [f64; STUDIO_PARTS],
    pub given: [f64; STUDIO_PARTS],
    pub omega: [f64; 4],
    pub travel: f64,
    pub room: f64,
    pub cassette_asked: [f64; 3],
    pub cassette_given: [f64; 3],
    pub cassette_omega: [f64; 2],
    pub deep_travel: f64,
    pub deep_room: f64,
}

// ── what a face shows ───────────────────────────────────────────────────────
//
// The figures a face prints beside the dials, from the dials alone. The face
// cannot reach this crate, so the host asks these of a bay's values on every
// push and every automated frame, and hands the face the answers. That is
// why each is a pure function of the params and nothing else: no allocation,
// no noise, no tape to run, and a few dozen multiply-adds and a handful of
// roots at most, so a drag that asks sixty times a second costs nothing. The
// weighted wow and flutter is the one that sums a spectrum, a few hundred
// points for each turning part, which sixty times a second is still
// nothing.
// Each reads the speed a dial is heading for, not the glide's position,
// because the face shows where the machine is going.

/// **The centre of the head contour's bell**, in hertz: the speed over the
/// bump's wavelength ([`BUMP_WAVELENGTH_MM`]), 8.74, 17.48, 34.95 and
/// 69.91 Hz at the four speeds. Bump only sizes the bell, so it does not
/// move this.
pub fn bump_hz(p: &TapeParams) -> f32 {
    let speed = TapeSpeed::from_dial(norm_or(p.speed, TapeSpeed::Ips15.dial()));
    (0.0254 * speed.ips() / (BUMP_WAVELENGTH_MM * 1.0e-3)) as f32
}

/// **The tape's hiss on silence, A-weighted, in dBFS**: what an A-weighted
/// meter across the audible band, 20 Hz to 20 kHz, reads at the output with
/// nothing playing. Minus infinity at Hiss 0, which is silent tape.
///
/// The bias noise is white at [`HISS_GAIN`] behind the speed's playback
/// shelf ([`HISS_EQ_US`]), and the rate law in `aim` puts the same noise in
/// every hertz at every rate, so this is the same figure at every rate an
/// engine runs at. The shelf, Roll's one-pole when it is in circuit and the
/// A-weighting are summed over the band's third octaves in octaves of
/// frequency, which agrees with the integral to a hundredth of a decibel,
/// and so is an old tape's spacing, which sits after the hiss and dulls it
/// ([`SPACING_FIT`]'s three matched one-poles at [`DESIGN_RATE`], at Age's
/// gap, the worn edges' mean and the azimuth's standing angle: the
/// digital poles, because at
/// 3.75 ips and full Age the spacing's top octave is a third of a decibel
/// under its analogue form, and the meter hears the poles). Drive, Age and Trim add their own decibels: the record
/// level's, 4.6 dB for every tenth of the dial under [`HISS_DRIVE`] and as
/// much off for every tenth over, an old tape's up to [`AGE_HISS_DB`], and
/// the trim's. The contour leaves it alone: its bell
/// and its low end sit where the A-weighting has already taken thirty
/// decibels. A fresh tape reads -82.4 dBFS(A): the datasheet's -87.0, 0 VU
/// less 69.0 dB(A) at [`HISS_DRIVE`], with the datasheet alignment's 4.6 dB
/// on it.
pub fn hiss_dbfs_a(p: &TapeParams) -> f32 {
    let hiss = norm(p.hiss);
    if hiss <= 0.0 {
        return f32::NEG_INFINITY;
    }
    let i = TapeSpeed::from_dial(norm_or(p.speed, TapeSpeed::Ips15.dial())) as usize;
    let gain = HISS_GAIN[i] * f64::from(hiss / 0.25).powi(2);
    // Uniform noise in -1..1 has a variance of a third, spread over half the
    // design rate's band of hertz.
    let density = gain * gain / 3.0 / (0.5 * f64::from(DESIGN_RATE));
    let roll = norm(p.roll);
    let corner = if roll > 1.0e-3 {
        f64::from(ROLL_HZ.0) * f64::from(ROLL_HZ.1 / ROLL_HZ.0).powf(f64::from(roll))
    } else {
        f64::INFINITY
    };
    let (t2, t1) = (HISS_EQ_US[i] * 1.0e-6, 50.0e-6);
    // An old tape's spacing: the shelf's corner and the double low-pass's,
    // as multiples of the frequency the gap costs a decibel at.
    let age = f64::from(norm_or(p.age, 0.0));
    let v_m = 0.0254 * TapeSpeed::ALL[i].ips();
    let damage = f64::from(norm_or(p.damage, 0.0));
    let gap = AGE_SPACING_UM * 1.0e-6 * age
        + 0.5 * EDGE_UM * 1.0e-6 * damage * damage
        + azimuth_gap(AZIMUTH_URAD.0 * 1.0e-6 * age, v_m);
    let f0 = if gap > 0.0 { v_m / (WALLACE_DB * gap) } else { f64::INFINITY };
    let (k_shelf, at_shelf, at_lp) = SPACING_FIT;
    let rate = f64::from(DESIGN_RATE);
    let (b_shelf, b_lp) = (matched_b(at_shelf * f0, rate), matched_b(at_lp * f0, rate));
    // `(1 - k) + k LP` over the shelf's pole, as a numerator `c0 - c1 z^-1`.
    let (c0, c1) = (1.0 - k_shelf + k_shelf * (1.0 - b_shelf), (1.0 - k_shelf) * b_shelf);
    // Thirty third-octave steps from 20 Hz to 20 kHz, trapezoidal in octaves:
    // the integral of the density over `df` is of `density x f` over `ln f`.
    const STEPS: i32 = 30;
    let step = 1_000.0_f64.ln() / f64::from(STEPS);
    let rise = step.exp();
    let mut f = 20.0_f64;
    let mut sum = 0.0;
    for k in 0..=STEPS {
        let (f2, w2) = (f * f, (std::f64::consts::TAU * f).powi(2));
        let a_weight = 12_194.0_f64.powi(4) * f2 * f2 * f2 * f2
            / ((f2 + 20.6_f64.powi(2)).powi(2)
                * (f2 + 107.7_f64.powi(2))
                * (f2 + 737.9_f64.powi(2))
                * (f2 + 12_194.0_f64.powi(2)).powi(2));
        let shelf = (1.0 + w2 * t2 * t2) / (1.0 + w2 * t1 * t1);
        let roll = 1.0 / (1.0 + f2 / (corner * corner));
        let cos = (std::f64::consts::TAU * f / rate).cos();
        let pole = |b: f64| 1.0 - 2.0 * b * cos + b * b;
        let lp = (1.0 - b_lp).powi(2) / pole(b_lp);
        let spacing = (c0 * c0 + c1 * c1 - 2.0 * c0 * c1 * cos) / pole(b_shelf) * lp * lp;
        let end = if k == 0 || k == STEPS { 0.5 } else { 1.0 };
        sum += end * a_weight * shelf * roll * spacing * f;
        f *= rise;
    }
    // The A curve is normalised to unity at 1 kHz, where its raw form reads
    // 2 dB low.
    let a_norm = 10.0_f64.powf(0.2);
    let trim = f64::from((norm(p.trim) - 0.5) * (TRIM_DB.1 - TRIM_DB.0));
    // The tape's noise over the record level, `d` at [`HISS_DRIVE`] over `d`
    // at the dial.
    let drive = 20.0 * std::f64::consts::LOG10_2 * (dial_log2_d(HISS_DRIVE) - dial_log2_d(norm(p.drive)));
    (10.0 * (density * sum * step * a_norm).log10() + trim + drive + AGE_HISS_DB * age) as f32
}

/// The weighted peaks a fresh transport is calibrated to read at each
/// speed, slowest first, in percent: the per-speed medians of published
/// studio machines ([`TRANSPORT_K`]).
const WEIGHTED_SPEC_PCT: [f64; 4] = [0.10, 0.06, 0.04, 0.03];

/// The flutter weighting's gain at `f` hertz, DIN 45507 / IEC 386 as the
/// spec fits it: unity near 4 Hz, a second-order high-pass at 0.447 Hz and a
/// first-order at 1.27 Hz under it, a first-order low-pass at 11.7 Hz over
/// it, and 0.946 dB of gain.
fn flutter_weight(f: f64) -> f64 {
    const GAIN: f64 = 1.114_98;
    let (f2, q, f1, fl) = (0.4470, 0.5905, 1.2744, 11.722);
    let ff = f * f;
    let hp2 = ff / ((f2 * f2 - ff).powi(2) + (f * f2 / q).powi(2)).sqrt();
    GAIN * hp2 * (f / (ff + f1 * f1).sqrt()) * (fl / (ff + fl * fl).sqrt())
}

/// **What a 2-sigma detector reads of a sum of steady sines, over its
/// RMS**, against the sum's excess kurtosis in tenths from -1.5 to 0. One
/// sine spends most of its time near its peaks, so the level it exceeds
/// 5 % of the time is 1.41 times its RMS; a sum of many is Gaussian, 1.96
/// times. Read off sums of one to four sines of random depths and turns,
/// half of them sharing a slowly wandering depth, some with Gaussian noise
/// added, through the detector's own rule: every sum sat within 3 % of
/// this line.
const SINES_C95: [f64; 16] = [
    1.410, 1.539, 1.614, 1.664, 1.706, 1.752, 1.786, 1.815, 1.846, 1.867, 1.898, 1.908, 1.922, 1.933, 1.944, 1.960,
];

/// **What a 2-sigma detector reads of the parts' breathing, over its
/// RMS.** An excursion that grows or shrinks bends the pitch while it
/// moves, in a burst after each step of its wander, and bursts stand far
/// above their own RMS: ten minutes of the breathing alone, at every
/// setting where it carries a twentieth of the sway or more, read 2.06 to
/// 2.41 times its RMS, 2.20 at the median and 2.33 where it is most of
/// the sway.
const BREATHING_C95: f64 = 2.3;

/// **The sizes of a wander's steps, as three Gauss nodes and weights.** A
/// step is the difference of two uniform draws, so its size `x` is spread
/// as `2 (1 - x)` over `0..1`, and its pulse carries `x^2` of power: these
/// are exact for every polynomial up to the fifth under `x^2 (1 - x)`.
const STEP_NODES: [(f64, f64); 3] = [(0.246_106, 0.161_227), (0.566_150, 0.542_341), (0.854_410, 0.296_432)];

/// Where the breathing's spectrum is summed: every [`BREATH_STEP_HZ`] from
/// minus to plus [`BREATH_SPAN_HZ`] around each part's turn. The step
/// samples each lobe of the slowest wander's spectrum (its hold's `sinc^2`,
/// lobes of 1 / 0.7 s, 1.43 Hz) about six times, and summed out to 40 Hz
/// in steps of 0.05 Hz the figure moves 0.4 % at most (0.03 % on average
/// over the grid of `the_face_reads_the_meter_across_the_whole_grid`).
const BREATH_SPAN_HZ: f64 = 20.0;
const BREATH_STEP_HZ: f64 = 0.25;

/// One turning part as the weighted figure sees it: its turn in hertz, its
/// depth of pitch at a wander scale of one (a fraction of the speed), the
/// wander that breathes it (0 Wow's, 1 Flutter's, 2 the cassette's breath)
/// and how fast its excursion can follow that wander, in wander units a
/// second.
#[derive(Clone, Copy, Default)]
struct Turning {
    hz: f64,
    depth: f64,
    wander: usize,
    limit: f64,
}

/// **The transport's turning parts as the governor plans them**, at
/// [`DESIGN_RATE`]: up to five (the studio head's reel and pinch roller
/// or the cassette's pack and roller, then the guides and the capstan with
/// its harmonic), how many, and the tape resonance's RMS depth at a wander
/// of one. The arithmetic is the kernel's own (the same wants, reserve and
/// governor), so the figure and the tape cannot plan differently. The
/// drifts, the studio head's and the belt's, are left out: they move too
/// slowly for the weighting, under a thousandth of the weighted power at
/// any setting.
fn turning_parts(p: &TapeParams) -> ([Turning; 7], usize, f64) {
    let sr = f64::from(DESIGN_RATE);
    let tau = std::f64::consts::TAU;
    let v = TapeSpeed::from_dial(norm_or(p.speed, TapeSpeed::Ips15.dial())).ips();
    let r = f64::from(rate_multiplier(p.rate));
    let at = f64::from(DIAL_AT_SPEC);
    let m = [
        f64::from(norm(p.wobble)) / at,
        (f64::from(norm_or(p.flutter, DIAL_AT_SPEC)) / at).powi(2),
    ];
    let hz: [f64; 4] = std::array::from_fn(|k| ROTOR_HZ_PER_IPS[k] * v * r);
    let omega = hz.map(|f| tau * f / sr);
    let (_, kappa) = resonance_at(sr);
    let (_, band) = scrape_band(sr);
    let want = studio_wants(v, &omega, m, sr, kappa, band * scrape_per_m(sr));
    let asked = at_the_top(&want);
    let widest = widest_urad(f64::from(norm_or(p.age, 0.0)), f64::from(norm_or(p.damage, 0.0)));
    let reserve = reserve(jitter_um(p), widest, v, sr);
    let keep = govern(&asked, studio_travel(sr, head_rest(sr as f32, BASE_MS)) - reserve);
    let deep = WowRange::from_dial(norm_or(p.wow_range, WowRange::Studio.dial())) == WowRange::Deep;
    // A studio part's excursion follows its aim times the wander's scale by
    // at most its share of the slew a frame: in wander units a second, that
    // share over the scale's span of the aim.
    let span = DEPTH_WANDER.1 - DEPTH_WANDER.0;
    let slew = STUDIO_SLEW / STUDIO_PARTS as f64 * sr / span;
    let mut parts = [Turning::default(); 7];
    let mut n = 0;
    for k in 0..4 {
        let part = REEL + k;
        let aim = want[part] * keep[part];
        // The deep head carries Flutter's parts and its own cassette, not
        // the studio head's wow.
        if (deep && part <= PINCH) || aim <= 0.0 {
            continue;
        }
        let depth = aim / PEAK[part] * omega[k];
        let wander = usize::from(part > PINCH);
        parts[n] = Turning { hz: hz[k], depth, wander, limit: slew / aim };
        n += 1;
        if part == CAPSTAN {
            let second = depth * CAPSTAN_2ND / ROTOR_DEPTH[3];
            parts[n] = Turning { hz: 2.0 * hz[k], depth: second, wander, limit: slew / aim };
            n += 1;
        }
    }
    let ladder = (15.0 / v).powf(SPEED_LADDER);
    let resonance = TRANSPORT_K * RESONANCE_DEPTH * ladder * m[1] * keep[RESONANCE];
    if deep {
        // The cassette, governed by the deep head's own room. Its breath
        // follows the wander over the wander's top by at most
        // CASSETTE_BREATH of the pack's turn a frame.
        let dial = f64::from(norm(p.wobble));
        let cassette_hz = CASSETTE_HZ.map(|f| f * r);
        let want = [
            BELT_MS * 1.0e-3 * sr * dial,
            CASSETTE_DEPTH * CASSETTE_SHARE[0] * dial / (tau * cassette_hz[0] / sr),
            CASSETTE_DEPTH * CASSETTE_SHARE[1] * dial / (tau * cassette_hz[1] / sr),
        ];
        let shared: f64 = [GUIDES, CAPSTAN, RESONANCE, SCRAPE].iter().map(|&j| asked[j] * keep[j]).sum();
        let travel = (DEEP_TRAVEL_MS * 1.0e-3 * sr).min(head_rest(sr as f32, DEEP_MS) as f64 - 2.0);
        let mut over = want.iter().sum::<f64>() - (travel - reserve - shared).max(0.0);
        let limit = CASSETTE_BREATH * tau * cassette_hz[0] * DEPTH_WANDER.1 / span;
        for (j, w) in want.iter().enumerate() {
            let kept = w - w.min(over.max(0.0));
            over -= w - kept;
            if j > BELT && kept > 0.0 {
                let f = cassette_hz[j - PACK];
                parts[n] = Turning { hz: f, depth: kept * tau * f / sr, wander: 2, limit };
                n += 1;
            }
        }
    }
    (parts, n, resonance)
}

/// **The pulse a wander's step of `x` makes in an excursion's speed**, as
/// the time constant, in seconds, of the one-pole pulse with the same area
/// and energy. The wander slews each step through its one-pole
/// ([`WANDER_SLEW_S`]); an excursion that may follow at no more than
/// `limit` wander units a second keeps up with a small step, and slews a
/// large one at its limit until the wander's own slowing lets it catch up,
/// which spreads the same area over a longer, flatter pulse.
fn step_pulse_s(x: f64, limit: f64) -> f64 {
    let tau = WANDER_SLEW_S;
    // The limit over the step's own steepest slope.
    let k = limit * tau / x;
    if k >= 1.0 {
        return tau;
    }
    // It catches up `s` time constants in, where `k s = 1 - e^-s`: Newton
    // from the limited slew's `1 / k`, above the root, where the curve is
    // convex and the steps never overshoot.
    let mut s = 1.0 / k;
    for _ in 0..16 {
        let e = (-s).exp();
        let step = (k * s - 1.0 + e) / (k - e);
        s -= step;
        if step.abs() <= 1.0e-9 * s {
            break;
        }
    }
    // Its energy: the limit held for `s`, then the one-pole's tail.
    tau / (2.0 * k * k * s + (-2.0 * s).exp())
}

/// **A part's breathing, as the meter hears it and as it is.** The
/// wander's scale is a sample-and-hold of uniform draws held `hold`
/// seconds, slewed, which the excursion follows by at most `limit`: its
/// spectrum is the hold's `sinc^2` under the steps' pulses. Carried to the
/// part's turn `hz`, a sideband `u` hertz away bends the pitch at `hz + u`,
/// so it is weighted there and by `(hz + u) / hz`. Returns that weighted
/// power and the scale's own variance, both per unit of the wander's span
/// squared and of the depth squared.
fn breathing(hz: f64, limit: f64, hold: f64) -> (f64, f64) {
    let pulses = STEP_NODES.map(|(x, w)| (step_pulse_s(x, limit), w));
    let points = (2.0 * BREATH_SPAN_HZ / BREATH_STEP_HZ).round() as usize;
    let (mut heard, mut own) = (0.0, 0.0);
    for i in 0..=points {
        let u = -BREATH_SPAN_HZ + i as f64 * BREATH_STEP_HZ;
        let x = std::f64::consts::PI * u * hold;
        let sinc2 = if x == 0.0 { 1.0 } else { (x.sin() / x).powi(2) };
        let shape: f64 = pulses.iter().map(|(t, w)| w / (1.0 + (std::f64::consts::TAU * u * t).powi(2))).sum();
        let power = hold * sinc2 / 12.0 * shape * BREATH_STEP_HZ;
        own += power;
        heard += power * ((hz + u) / hz * flutter_weight((hz + u).abs())).powi(2);
    }
    (heard, own)
}

/// **What the meter reads of the transport**, as a fraction of the speed:
/// the weighted 2-sigma of every turning part, its breathing included, at
/// the speed and Rate the dials are heading for.
///
/// Each wander's parts are steady sines whose depth breathes: their
/// weighted powers and the fourth cumulant of their sum, with the tape's
/// resonance as Gaussian noise among Flutter's, read through
/// [`SINES_C95`]; their depth breathing as its loudest part's does, the
/// scale's moments of a uniform spread of that variance. And each part's
/// breathing bends the pitch on its own wherever the weighting hears its
/// sidebands louder than its turn ([`breathing`]): a slow part's sidebands
/// reach the weighting's band where its turn cannot, which is why the
/// figure needs them once Rate slows the wow under the weighting's 0.45 Hz
/// corner. That excess reads through [`BREATHING_C95`], and the two
/// readings add as squares, which ten-minute meters on every part of the
/// grid bear out.
fn weighted_reading(p: &TapeParams) -> f64 {
    let (parts, n, resonance) = turning_parts(p);
    let (lo, hi) = DEPTH_WANDER;
    let mut breaths = [(0.0, 0.0); 7];
    for (b, t) in breaths.iter_mut().zip(&parts[..n]) {
        let hold = if t.wander == 1 { FLUTTER_HOLD_S } else { WOW_HOLD_S };
        *b = breathing(t.hz, t.limit, hold);
    }
    let (mut steady, mut cumulant, mut excess) = (0.0, 0.0, 0.0);
    for wander in 0..3 {
        // The cassette's breath is the wander's scale over its top.
        let over = if wander == 2 { 1.0 / hi } else { 1.0 };
        let (mean, swing) = (over * 0.5 * (lo + hi), over * (hi - lo));
        let noise = if wander == 1 { (resonance * flutter_weight(RESONANCE_HZ)).powi(2) } else { 0.0 };
        let group = || (0..n).filter(move |&j| parts[j].wander == wander);
        let loud = |j: &usize| parts[*j].depth * flutter_weight(parts[*j].hz);
        // With no part turning, the resonance breathes as the wander itself:
        // a uniform draw's twelfth, less what the slew rounds off each hold.
        let hold = if wander == 1 { FLUTTER_HOLD_S } else { WOW_HOLD_S };
        let slewed = (1.0 - WANDER_SLEW_S / hold * (1.0 - (-hold / WANDER_SLEW_S).exp())) / 12.0;
        let var = group()
            .max_by(|a, b| loud(a).total_cmp(&loud(b)))
            .map_or(slewed, |j| breaths[j].1)
            * swing
            * swing;
        let (m2, m4) = (mean * mean + var, mean.powi(4) + 6.0 * mean * mean * var + 1.8 * var * var);
        let (mut sum, mut fourth) = (noise, 0.0);
        for j in group() {
            let a = loud(&j);
            sum += 0.5 * a * a;
            fourth += a.powi(4);
            let heard = 0.5 * (mean * a).powi(2) + 0.5 * (swing * parts[j].depth).powi(2) * breaths[j].0;
            excess += (heard - 0.5 * m2 * a * a).max(0.0);
        }
        if sum > 0.0 {
            let power = m2 * sum;
            steady += power;
            cumulant += m4 * (3.0 * sum * sum - 0.375 * fourth) - 3.0 * power * power;
        }
    }
    let read = if steady > 0.0 {
        let x = ((cumulant / (steady * steady) + 1.5) * 10.0).clamp(0.0, 15.0);
        let i = (x as usize).min(14);
        (SINES_C95[i] + (SINES_C95[i + 1] - SINES_C95[i]) * (x - i as f64)) * steady.sqrt()
    } else {
        0.0
    };
    read.hypot(BREATHING_C95 * excess.sqrt())
}

/// **The whole transport's weighted flutter**, in percent of the speed: what
/// a DIN 45507 / IEC 386 meter's 2-sigma detector reads off the tape, wow
/// and flutter together, as the governor shares the head, at the speed and
/// Rate the dials are heading for.
///
/// [`weighted_reading`] is the meter's reading, worked out from the parts,
/// and it is anchored at each speed's fresh transport to the machine it is
/// calibrated to be, 0.10, 0.06, 0.04 and 0.03 % ([`WEIGHTED_SPEC_PCT`]).
/// The reading itself is 0.100, 0.062, 0.041 and 0.028 % there, within 2 %
/// of ten minutes of the meter, so the anchor moves the figure by 3.3 % at
/// most at the three slower speeds, and at 30 ips, whose median the 0.03 %
/// rounds up, it stands 8 % over the meter throughout. Away from the fresh
/// transport the figure follows the reading.
/// `the_transport_reads_a_studio_machines_weighted_flutter_at_every_speed`
/// holds it within 15 % of a meter on the head's own sway at every speed
/// and where Wow, Flutter and the governor have all moved it, and
/// `the_face_reads_the_meter_where_rate_slows_the_wow` where Rate slows the
/// parts under the weighting and their breathing is most of what the meter
/// hears. On the deep head the parts are Flutter's and the cassette's, and
/// `the_deep_range_swings_the_pitch_like_a_worn_cassette` holds it within
/// 15 % there at Wow 0.3, 0.5 and 1. The breathing is summed at 161 points
/// of spectrum for each of up to five parts ([`BREATH_SPAN_HZ`]), for the
/// dials and again for the fresh transport, against the dozens of
/// multiply-adds the other figures cost.
pub fn wow_flutter_weighted_pct(p: &TapeParams) -> f32 {
    let i = TapeSpeed::from_dial(norm_or(p.speed, TapeSpeed::Ips15.dial())) as usize;
    let fresh = TapeParams {
        speed: TapeSpeed::ALL[i].dial(),
        ..TapeParams::default()
    };
    (WEIGHTED_SPEC_PCT[i] * weighted_reading(p) / weighted_reading(&fresh)) as f32
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

    /// **Full Wow bends a sine by a believable fraction of a percent.** A
    /// steady 1 kHz through Wow 1 at 15 ips, Flutter at the specified
    /// machine's 0.3: a studio transport's slow sway turned all the way up
    /// moves the pitch a few tenths of a percent peak to peak, and no more,
    /// because the studio head's own travel bounds what wow it can carry; a
    /// cassette's warble is the deep range's, not this head's. Peak to peak
    /// in 0.25 to 0.6 % (model 0.375 to 0.459 % over seven seeds; measured
    /// 0.452 %).
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
            flutter: 0.3,
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
        // the capstan's 13 Hz, whose period is seventy-five of these.
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
            (0.002_5..0.006).contains(&peak_to_peak),
            "peak-to-peak pitch deviation {:.4}% is outside the believable \
             0.25%..0.6% of a studio transport at full wow",
            peak_to_peak * 100.0
        );
    }

    /// **The hiss is white behind the playback EQ, and the modulation noise
    /// rides the signal.**
    ///
    /// (a) On silence at Hiss 1 at 15 ips, whose playback shelf is a wire,
    /// the hiss is flat per hertz: its least-squares slope from 250 Hz to
    /// 8 kHz is within a decibel an octave of level, where 5.0's pink ladder
    /// fell at three. (b) Each speed's colour is its own playback EQ against
    /// the 50 µs its level is quoted behind: per hertz, 10 kHz against 1 kHz
    /// reads +4.0 dB at 3.75 ips (NAB 90 µs), level at 7.5 and 15 ips, and
    /// -6.6 dB at 30 (AES 17.5 µs), each within a decibel. (c) And the noise
    /// a tone carries: a 1 kHz tone at 0 VU at 15 ips, at a fresh tape's
    /// Drive and Hiss, the transport still, carries a skirt at 800 Hz 72.5 dB
    /// under it in 10 Hz, within 3 dB, the figure studio machines measure.
    /// It is eleven 1 Hz bins of a one-second Hann window summed and scaled
    /// by `10 / (11 x 1.5)`: a Hann bin gathers 1.5 bins' worth of noise, so
    /// the eleven unscaled would read 2.2 dB high. Half a second after the
    /// tone stops the same band has fallen by at least 15 dB, because what
    /// is left there is only the hiss, which is still there, above 1e-7 RMS:
    /// the noise that follows the music is around each note, not a swell.
    /// (d) And an under-biased tape's grain is coarser, 2.1 dB more
    /// modulation noise for every dB under: at the bottom of Bias the same
    /// tone's skirt stands 12.6 dB over the aligned tape's, within 1.5.
    /// (e) An old tape's grain modulates twice as hard: at full Age the
    /// skirt stands 6.0 dB over new tape's, within 1. Measured: a slope of -0.20 dB an octave; +4.07, -0.11, -0.11 and
    /// -6.71 dB; a skirt of -70.9 dB that falls to -96.7 once the tone
    /// stops, over a floor of 1.0e-4 RMS; 12.56 dB more of it at the
    /// bottom of Bias, and 5.99 dB more at full Age.
    #[test]
    fn the_hiss_is_white_behind_the_playback_eq_and_the_modulation_noise_rides_the_signal() {
        let sr = 48_000.0_f32;
        let hissing = |speed: TapeSpeed| TapeParams {
            hiss: 1.0,
            speed: speed.dial(),
            ..TapeParams::NEUTRAL
        };

        // (a) The slope, on silence: nothing in the output but the hiss.
        let mut unit = Tape::new(sr);
        let warm = 4_096;
        let windows = 16;
        let len = 8_192;
        let silence = vec![0.0_f32; (warm + windows * len) * 2];
        let out = left(&feed(&mut unit, &silence, &hissing(TapeSpeed::Ips15)));
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
            (-1.0..=1.0).contains(&slope),
            "the hiss slopes at {slope:.2} dB/octave, which is not white"
        );

        // (b) Each speed's colour: the density at 10 kHz against 1 kHz.
        for (speed, want) in TapeSpeed::ALL.into_iter().zip([4.0, 0.0, 0.0, -6.6]) {
            let frames = (sr * 4.0) as usize;
            let out = left(&feed(&mut Tape::new(sr), &vec![0.0; frames * 2], &hissing(speed)));
            let skip = (sr * 0.1) as usize;
            let density = |lo: f64, hi: f64| band_power(&out, sr.into(), lo, hi, skip) / (hi - lo);
            let tilt = 10.0 * (density(9_000.0, 11_000.0) / density(900.0, 1_100.0)).log10();
            assert!(
                (tilt - want).abs() < 1.0,
                "at {} the hiss reads {tilt:+.2} dB per hertz at 10 kHz against 1 kHz, \
                 where its playback EQ gives {want:+.1}",
                speed.label()
            );
        }

        // (c) The skirt a tone carries, and what is left when it stops.
        let p = TapeParams {
            drive: TapeParams::default().drive,
            hiss: 0.25,
            ..TapeParams::NEUTRAL
        };
        let tone_end = (sr * 2.0) as usize;
        let mono: Vec<f32> = (0..(sr * 3.5) as usize)
            .map(|i| {
                if i < tone_end {
                    zero_vu() * (std::f64::consts::TAU * 1_000.0 * i as f64 / f64::from(sr)).sin() as f32
                } else {
                    0.0
                }
            })
            .collect();
        let out = left(&feed(&mut Tape::new(sr), &to_stereo(&mono), &p));
        let second = sr as usize;
        let during = &out[(sr * 0.9) as usize..(sr * 0.9) as usize + second];
        let after = &out[(sr * 2.5) as usize..(sr * 2.5) as usize + second];
        let tone = band(during, sr.into(), 1_000.0).powi(2);
        let skirt_under = |seg: &[f32], tone: f64| {
            let bins: f64 = (795..=805).map(|f| band(seg, sr.into(), f64::from(f)).powi(2)).sum();
            10.0 * (bins * 10.0 / (11.0 * 1.5) / tone).log10()
        };
        let skirt = |seg: &[f32]| skirt_under(seg, tone);
        let (with, without) = (skirt(during), skirt(after));
        assert!(
            (with + 72.5).abs() < 3.0,
            "a 0 VU 1 kHz tone at 15 ips carries {with:+.1} dB in 10 Hz at 800 Hz, \
             where studio machines measure -72.5"
        );
        assert!(
            without < with - 15.0,
            "half a second after the tone stopped the band at 800 Hz reads {without:+.1} dB, \
             against {with:+.1} under the tone: the modulation noise did not leave with it"
        );
        let floor = rms(after);
        assert!(floor > 1.0e-7, "the hiss floor vanished entirely: {floor:.2e}");

        // (d) An under-biased tape's grain is coarser: 2.1 dB more modulation
        // noise for every dB under, so the bottom of Bias, 6 dB under,
        // carries the same tone's skirt 12.6 dB higher.
        let coarse = left(&feed(&mut Tape::new(sr), &to_stereo(&mono), &TapeParams { bias: 0.0, ..p }));
        let during = &coarse[(sr * 0.9) as usize..(sr * 0.9) as usize + second];
        let risen = skirt_under(during, band(during, sr.into(), 1_000.0).powi(2)) - with;
        assert!(
            (risen - 12.6).abs() < 1.5,
            "at the bottom of Bias the tone's skirt stands {risen:+.1} dB over the aligned tape's, \
             where an under-biased tape's coarser grain puts it 12.6 dB higher"
        );

        // (e) An old tape's grain modulates twice as hard ([`AGE_JITTER`]),
        // so full Age carries the tone's skirt 6.0 dB higher.
        let old = left(&feed(&mut Tape::new(sr), &to_stereo(&mono), &TapeParams { age: 1.0, ..p }));
        let during = &old[(sr * 0.9) as usize..(sr * 0.9) as usize + second];
        let risen = skirt_under(during, band(during, sr.into(), 1_000.0).powi(2)) - with;
        assert!(
            (risen - 6.0).abs() < 1.0,
            "at full Age the tone's skirt stands {risen:+.1} dB over new tape's, where an old \
             tape's coarser grain puts it 6.0 dB higher"
        );
    }

    /// **The hiss floor and colour follow each speed's datasheet.**
    ///
    /// On silence, every colour off, the default Hiss and a fresh tape's
    /// Drive, at 44.1 kHz, the rate the levels are designed at. The
    /// datasheet figures are quoted at [`HISS_DRIVE`], and a fresh tape's
    /// record level is a tenth of the dial cooler, which puts its hiss
    /// 4.6 dB over them: so the hiss's density at 1 kHz is each speed's
    /// datasheet noise behind its playback EQ (-128.7, -129.5, -128.0 and
    /// -127.5 dB re full scale squared per hertz at 3.75, 7.5, 15 and
    /// 30 ips) with those 4.6 dB on it, -124.1, -124.9, -123.4 and -122.9,
    /// within a decibel; and at 15 ips its A-weighted level is -82.4
    /// dBFS(A), the datasheet's -87.0 (0 VU less 69.0 dB(A)) with the same
    /// 4.6 dB, within 0.7 dB, measured as the A-weighted sum of the
    /// third-octave bands the output actually carries. Measured: -124.1,
    /// -124.7, -123.2 and -122.6 dB/Hz, and -82.3 dBFS(A).
    ///
    /// **And the figure a face prints is the one a meter reads.**
    /// [`hiss_dbfs_a`] works the same level out from the dials alone, and it
    /// must agree with the measured A-weighted level within half a decibel
    /// at every speed, at Hiss 0.1, 0.5 and 1 as well as 0.25, with Roll at
    /// half travel, with Trim at +6 dB, at the bottom and the top of
    /// Drive, where the hiss sits 23.0 dB over a fresh tape's and 18.4 dB
    /// under the datasheet's, and at Age 0.5 and 1, where an old tape's
    /// noise rises 3 dB at the top and its spacing dulls it again, most of
    /// all on slow tape (so Age 1 is pinned at 3.75 ips too), and at full
    /// Damage, whose worn edges dull it by their mean; it reads
    /// minus infinity at Hiss 0, and a fresh
    /// tape reads -82.4 within a fifth of a decibel. The meter's bands are
    /// each scaled to the noise bandwidth they really pass, or their
    /// overlapping skirts would read a tenth of a decibel hot. Measured: the
    /// meter 0.00 to 0.12 dB over the helper at 3.75, 7.5 and 15 ips, at
    /// every Hiss and Drive and with the Trim (its sum reaches past 20 kHz,
    /// where the helper's band stops), 0.37 over at 30 ips and 0.32 with
    /// Roll at half travel, where the shelf's bilinear warp and Roll's
    /// one-pole pass a little more treble at 44.1 kHz than the analogue
    /// figures the helper sums; at Age 0.5 and 1 at 15 ips the meter 0.09
    /// and 0.06 over, at Age 1 at 3.75 ips 0.36 under, at full Damage 0.02
    /// over; and -82.45
    /// dBFS(A) for a fresh tape.
    #[test]
    fn the_hiss_floor_and_colour_follow_each_speeds_datasheet() {
        let sr = 44_100.0_f32;
        let frames = (sr * 4.0) as usize;
        let skip = (sr * 0.1) as usize;
        // The A-weighting curve, normalised to 0 dB at 1 kHz.
        let a_weight = |f: f64| {
            let f2 = f * f;
            let ra = 12_194.0_f64.powi(2) * f2 * f2
                / ((f2 + 20.6_f64.powi(2))
                    * ((f2 + 107.7_f64.powi(2)) * (f2 + 737.9_f64.powi(2))).sqrt()
                    * (f2 + 12_194.0_f64.powi(2)));
            ra * 10.0_f64.powf(2.0 / 20.0)
        };
        // The third-octave bands, each with the noise bandwidth it really
        // passes: its eighth-order skirts reach into its neighbours, so a
        // band gathers a little more than its edges hold, and summed as they
        // are the bands read white noise a tenth of a decibel hot. The noise
        // bandwidth is the energy of the band's own impulse response through
        // the filters `band_power` runs.
        let bands: Vec<(f64, f64, f64, f64)> = (-17..=13)
            .map(|k| {
                let centre = 1_000.0 * 2.0_f64.powf(f64::from(k) / 3.0);
                let (lo, hi) = (
                    centre * 2.0_f64.powf(-1.0 / 6.0),
                    (centre * 2.0_f64.powf(1.0 / 6.0)).min(0.495 * f64::from(sr)),
                );
                let (mut hp, mut lp) = (
                    Butter::new(8, sr.into(), lo, true),
                    Butter::new(8, sr.into(), hi, false),
                );
                let energy: f64 = (0..sr as usize)
                    .map(|n| lp.run(hp.run(if n == 0 { 1.0 } else { 0.0 })).powi(2))
                    .sum();
                (centre, lo, hi, 0.5 * energy * f64::from(sr))
            })
            .collect();
        let dba = |out: &[f32]| {
            let weighted: f64 = bands
                .iter()
                .map(|&(centre, lo, hi, enbw)| {
                    band_power(out, sr.into(), lo, hi, skip) * (hi - lo) / enbw * a_weight(centre).powi(2)
                })
                .sum();
            10.0 * weighted.log10()
        };
        let silence = vec![0.0; frames * 2];
        let fresh = TapeParams::default().drive;
        let face_agrees = |p: &TapeParams, measured: f64, what: &str| {
            let told = f64::from(hiss_dbfs_a(p));
            assert!(
                (told - measured).abs() < 0.5,
                "{what}: the face is told the hiss reads {told:+.2} dBFS(A), and a meter \
                 reads {measured:+.2}"
            );
        };
        for (speed, want) in TapeSpeed::ALL.into_iter().zip([-124.1, -124.9, -123.4, -122.9]) {
            let p = TapeParams {
                drive: fresh,
                hiss: 0.25,
                speed: speed.dial(),
                ..TapeParams::NEUTRAL
            };
            let out = left(&feed(&mut Tape::new(sr), &silence, &p));
            let density = 10.0 * (band_power(&out, sr.into(), 900.0, 1_100.0, skip) / 200.0).log10();
            assert!(
                (density - want).abs() < 1.0,
                "at {} the hiss reads {density:+.1} dB re FS^2/Hz at 1 kHz, where its \
                 datasheet puts {want:+.1}",
                speed.label()
            );
            let level = dba(&out);
            face_agrees(&p, level, speed.label());
            if speed == TapeSpeed::Ips15 {
                assert!(
                    (level + 82.4).abs() < 0.7,
                    "at 15 ips the hiss reads {level:+.2} dBFS(A) at a fresh tape's Drive, where \
                     the datasheet's -87.0 at Drive {HISS_DRIVE} puts -82.4"
                );
            }
        }
        for (what, p) in [
            ("Hiss 0.1", TapeParams { drive: fresh, hiss: 0.1, ..TapeParams::NEUTRAL }),
            ("Hiss 0.5", TapeParams { drive: fresh, hiss: 0.5, ..TapeParams::NEUTRAL }),
            ("Hiss 1", TapeParams { drive: fresh, hiss: 1.0, ..TapeParams::NEUTRAL }),
            ("Roll at half travel", TapeParams { drive: fresh, hiss: 0.25, roll: 0.5, ..TapeParams::NEUTRAL }),
            ("Trim at +6 dB", TapeParams { drive: fresh, hiss: 0.25, trim: 0.75, ..TapeParams::NEUTRAL }),
            ("Drive 0", TapeParams { drive: 0.0, hiss: 0.25, ..TapeParams::NEUTRAL }),
            ("Drive 1", TapeParams { drive: 1.0, hiss: 0.25, ..TapeParams::NEUTRAL }),
            ("Age 0.5", TapeParams { drive: fresh, hiss: 0.25, age: 0.5, ..TapeParams::NEUTRAL }),
            ("Age 1", TapeParams { drive: fresh, hiss: 0.25, age: 1.0, ..TapeParams::NEUTRAL }),
            (
                "Age 1 at 3.75 ips",
                TapeParams { drive: fresh, hiss: 0.25, age: 1.0, speed: TapeSpeed::Ips3_75.dial(), ..TapeParams::NEUTRAL },
            ),
            ("Damage 1", TapeParams { drive: fresh, hiss: 0.25, damage: 1.0, ..TapeParams::NEUTRAL }),
        ] {
            let out = left(&feed(&mut Tape::new(sr), &silence, &p));
            face_agrees(&p, dba(&out), what);
        }
        assert_eq!(
            hiss_dbfs_a(&TapeParams { hiss: 0.0, ..TapeParams::default() }),
            f32::NEG_INFINITY,
            "silent tape is not silent to the face"
        );
        let told = hiss_dbfs_a(&TapeParams::default());
        assert!(
            (told + 82.4).abs() < 0.2,
            "a fresh tape tells the face its hiss reads {told:+.2} dBFS(A), not the -82.4 its \
             datasheet alignment puts it at"
        );
    }

    /// **The hiss falls as the record level rises, as it does on tape.**
    ///
    /// The bias noise is on the tape, under whatever record level put the
    /// music there, and the line-up brings the music back to 0 VU and the
    /// noise with it: the hiss a person hears is the tape's noise over the
    /// record level `d` ([`HISS_DRIVE`]), so between two Drives it moves by
    /// twenty times the log of their `d`s, the other way. On silence, every
    /// colour off and the default Hiss, Drive 0.2 against 0.8, three fifths
    /// of the dial's 46 dB sweep apart: the hiss reads 27.6 dB louder at 0.2,
    /// within a tenth of a decibel, at 15 ips and at 3.75, where the speed
    /// scales what the curve is asked for and the law must not care, and the
    /// figure a face prints moves by the same. And the tape's modulation
    /// noise, which rides the music rather than sitting under it, moves each
    /// track's head by exactly as much at either Drive. Measured: 27.61 dB
    /// at both speeds, and the head's furthest jitter bit for bit the same.
    #[test]
    fn the_hiss_falls_as_the_record_level_rises_as_it_does_on_tape() {
        let sr = 48_000.0_f32;
        let silence = vec![0.0_f32; (sr * 1.0) as usize * 2];
        let skip = (sr * 0.1) as usize * 2;
        let law = 20.0 * (dial_log2_d(0.8) - dial_log2_d(0.2)) * std::f64::consts::LOG10_2;
        for speed in [TapeSpeed::Ips15, TapeSpeed::Ips3_75] {
            let at = |drive: f32| {
                let p = TapeParams {
                    drive,
                    hiss: 0.25,
                    speed: speed.dial(),
                    ..TapeParams::NEUTRAL
                };
                let mut unit = Tape::new(sr);
                let out = feed(&mut unit, &silence, &p);
                (rms(&out[skip..]), unit.deepest()[0], f64::from(hiss_dbfs_a(&p)))
            };
            let ((cool, cool_jitter, cool_told), (hot, hot_jitter, hot_told)) = (at(0.2), at(0.8));
            let moved = db(cool / hot);
            assert!(
                (moved - law).abs() < 0.1,
                "at {} the hiss reads {moved:+.2} dB louder at Drive 0.2 than at 0.8, where the \
                 record levels are {law:.2} dB apart",
                speed.label()
            );
            assert!(
                (cool_told - hot_told - law).abs() < 0.01,
                "at {} the face is told the hiss moves {:+.2} dB between Drive 0.2 and 0.8, not {law:.2}",
                speed.label(),
                cool_told - hot_told
            );
            assert!(
                cool_jitter > 0.0 && cool_jitter == hot_jitter,
                "at {} the modulation noise moved the head {cool_jitter} samples at Drive 0.2 and \
                 {hot_jitter} at 0.8: it rides the music, and the record level does not scale it",
                speed.label()
            );
        }
    }

    /// **Nothing any control can reach may blow up.** Every combination of
    /// bottom, middle and top on the six 5.0 shipped with at once, the
    /// controls appended since at their defaults, against a signal with a
    /// step in the middle of every block, on one unit whose state rides
    /// through the whole sweep: a knob drag, not a bench test. Then 1024
    /// combinations of all thirteen drawn at random (the selectors on their
    /// positions), and every control at its bottom and at its top at once.
    /// Then NaN params, the end-to-end jumps (the hiss and the damage from
    /// nothing to full and back on silence, then every control), the speed
    /// thrown between 3.75 and 30 ips and the range between the heads every
    /// block, and a clear that must actually clear.
    #[test]
    fn no_setting_any_control_can_reach_makes_it_explode() {
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
        let mut seed = 0x7A9E_C0DE_u32;
        for combo in 0..1_026 {
            let mut x = [0.0_f32; 13];
            for v in &mut x {
                *v = uniform(&mut seed);
            }
            match combo {
                1_024 => x = [0.0; 13],
                1_025 => x = [1.0; 13],
                _ => {}
            }
            let p = TapeParams {
                drive: x[0],
                bump: x[1],
                roll: x[2],
                wobble: x[3],
                hiss: x[4],
                trim: x[5],
                speed: TapeSpeed::from_dial(x[6]).dial(),
                flutter: x[7],
                rate: x[8],
                wow_range: WowRange::from_dial(x[9]).dial(),
                bias: x[10],
                age: x[11],
                damage: x[12],
            };
            let mut buf = block.clone();
            unit.process(&mut buf, 128, 2, &p);
            assert!(
                buf.iter().all(|v| v.is_finite() && v.abs() < 16.0),
                "random combination {combo} ({x:?}) left the audio at {:?}",
                buf.iter().find(|v| !v.is_finite() || v.abs() >= 16.0)
            );
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
            flutter: f32::NEG_INFINITY,
            rate: f32::NAN,
            wow_range: f32::NAN,
            bias: f32::NAN,
            age: f32::NAN,
            damage: f32::NAN,
        };
        let mut buf = block.clone();
        unit.process(&mut buf, 128, 2, &p);
        assert!(
            buf.iter().all(|v| v.is_finite()),
            "NaN params reached the audio"
        );

        // **And a NaN in any control appended since 5.0 is that control at
        // its default**, bit for bit, because each reads through `norm_or`:
        // a file that lost the field, or a host that hands it a NaN, hears
        // the tape the field's default makes. Finite is not enough: Damage
        // read raw passes the leg above, yet its NaN reaches the spacing and
        // holds the tone near silence. Everything else is engaged, so a NaN
        // anywhere in the wear or the transport has something to spoil. (The
        // six 5.0 shipped read a NaN as the middle of their travel, as 5.0
        // did.)
        let heard = |p: &TapeParams| {
            let mut unit = Tape::new(sr);
            (0..40)
                .flat_map(|_| {
                    let mut buf = block.clone();
                    unit.process(&mut buf, 128, 2, p);
                    buf
                })
                .collect::<Vec<f32>>()
        };
        let base = everything_engaged();
        let fresh = TapeParams::default();
        let appended: [(&str, fn(&mut TapeParams, f32), f32); 7] = [
            ("speed", |p, v| p.speed = v, fresh.speed),
            ("flutter", |p, v| p.flutter = v, fresh.flutter),
            ("rate", |p, v| p.rate = v, fresh.rate),
            ("wow range", |p, v| p.wow_range = v, fresh.wow_range),
            ("bias", |p, v| p.bias = v, fresh.bias),
            ("age", |p, v| p.age = v, fresh.age),
            ("damage", |p, v| p.damage = v, fresh.damage),
        ];
        for (name, set, default) in appended {
            let (mut nan, mut unset) = (base, base);
            set(&mut nan, f32::NAN);
            set(&mut unset, default);
            let off = heard(&nan).iter().zip(&heard(&unset)).position(|(x, y)| x.to_bits() != y.to_bits());
            assert_eq!(off.map(|i| i / 2), None, "a NaN {name} played another tape than {name} at its default, from that frame on");
        }
        // The spacing a NaN would have made, had it got there, is no
        // spacing at all.
        let v_m = 0.0254 * TapeSpeed::Ips15.ips();
        for d in [f64::NAN, f64::INFINITY] {
            assert_eq!(Tape::new(sr).spacing_bs(d, v_m), [0.0; 2], "a spacing of {d} m is not a wire");
        }

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
                    damage: end,
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
                    flutter: end,
                    rate: end,
                    wow_range: end,
                    bias: end,
                    age: end,
                    damage: end,
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

        // The speed and the range thrown between their ends every block,
        // slammed and at full Wow: every per-speed table blended mid-glide
        // and the heads turned round mid-fade, two hundred times over.
        let mut thrown = Tape::new(sr);
        for k in 0..200 {
            let p = TapeParams {
                drive: 1.0,
                wobble: 1.0,
                speed: (k % 2) as f32,
                wow_range: (k % 2) as f32,
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
            flutter: 0.0,
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
    /// a fresh tape's Drive. **And under-biased**, where the low shelf
    /// before the curve puts the long wavelengths in hotter: what it adds is
    /// bass, whose harmonics stay far under the 4x Nyquist, and at 3.75 ips
    /// it is a wire at 7 kHz. So 3.75 ips at the bottom of Bias folds no more
    /// than aligned, read against the output tone, within half a decibel
    /// (measured -30.6 against aligned's -29.0). Not against the input tone:
    /// there it reads -47.8, because the bias's own treble shelf, after the
    /// curve, lifts the whole band above its 2.5 kHz corner by up to 5.6 dB,
    /// the folds and the fundamental alike; that is a tone control on what
    /// the curve made, not fold.
    #[test]
    fn full_drive_on_a_high_note_does_not_fold_the_band_back_on_itself() {
        let sr = 48_000.0_f32;
        let frames = (sr * 0.7) as usize;
        // Everything that is not the saturator, off: the transport spreads a
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
        let fold_at = |drive: f32, speed: TapeSpeed, bias: f32| -> (f64, f64) {
            let out = left(&feed(
                &mut Tape::new(sr),
                &to_stereo(&mono),
                &TapeParams {
                    drive,
                    speed: speed.dial(),
                    bias,
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
        let fold = |drive: f32, speed: TapeSpeed| fold_at(drive, speed, 0.5);
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
        let ((under, _), (aligned, _)) = (fold_at(1.0, TapeSpeed::Ips3_75, 0.0), fold(1.0, TapeSpeed::Ips3_75));
        assert!(
            under <= aligned + 0.5,
            "full drive at 3.75 ips at the bottom of Bias folded {under:.1} dB of hash against the \
             tone that came out, where aligned folds {aligned:.1}"
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
    /// the next at any of the four speeds and at the bottom, middle and top
    /// of Bias, where the line-up is the machine's at that bias (model
    /// +0.002 dB at most, measured +0.002 at 30 ips at every bias and none
    /// at the others). Not pinned: the programme at 30 ips, whose last step rises
    /// +0.037 dB in the model, too near the bar to be a fair pin.
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
            for bias in [0.0_f32, 0.5, 1.0] {
                let mut last = f64::INFINITY;
                for step in 0..=10 {
                    let p = TapeParams {
                        drive: step as f32 / 10.0,
                        speed: speed.dial(),
                        bias,
                        ..TapeParams::NEUTRAL
                    };
                    let out = left(&feed(&mut Tape::new(sr), &to_stereo(&hundred), &p));
                    let here = db(band(&out[t_lo..t_hi], sr.into(), 100.0) / tone_in);
                    assert!(
                        here <= last + 0.05,
                        "at {} and Bias {bias} a 0 VU 100 Hz tone read {here:.3} dB at drive {}, \
                         against {last:.3} dB the step before: the dial is a bass volume control",
                        speed.label(),
                        step as f32 / 10.0
                    );
                    last = here;
                }
            }
        }
    }

    /// A biquad cascade in f64, transposed direct form, each section's
    /// numerator and denominator in powers of `z^-1` (the denominator's
    /// first term one): the flutter meters below run it.
    struct Sections(Vec<([f64; 3], [f64; 2], [f64; 2])>);

    impl Sections {
        fn run(&mut self, x: f64) -> f64 {
            let mut v = x;
            for (b, a, z) in &mut self.0 {
                let y = b[0] * v + z[0];
                z[0] = b[1] * v - a[0] * y + z[1];
                z[1] = b[2] * v - a[1] * y;
                v = y;
            }
            v
        }
    }

    /// **The flutter meter's weighting**, DIN 45507 / IEC 386, as the spec
    /// fits it: `10^(0.946/20) s^2 / (s^2 + s w2/0.5905 + w2^2) x s/(s + w1)
    /// x wl/(s + wl)` at 0.4470, 1.2744 and 11.722 Hz, within 0.75 dB of the
    /// published curve from 0.125 to 200 Hz, by the bilinear transform at
    /// the trace's rate (whose warp is nothing at these frequencies).
    fn flutter_weighting(sr: f64) -> Sections {
        let tau = std::f64::consts::TAU;
        let k = 2.0 * sr;
        let (w2, q, w1, wl) = (tau * 0.4470, 0.5905, tau * 1.2744, tau * 11.722);
        let a0 = k * k + k * w2 / q + w2 * w2;
        let g = 10.0_f64.powf(0.946 / 20.0);
        Sections(vec![
            (
                [g * k * k / a0, -2.0 * g * k * k / a0, g * k * k / a0],
                [(2.0 * w2 * w2 - 2.0 * k * k) / a0, (k * k - k * w2 / q + w2 * w2) / a0],
                [0.0; 2],
            ),
            ([k / (k + w1), -k / (k + w1), 0.0], [(w1 - k) / (k + w1), 0.0], [0.0; 2]),
            ([wl / (k + wl), wl / (k + wl), 0.0], [(wl - k) / (k + wl), 0.0], [0.0; 2]),
        ])
    }

    /// The speed deviation a sway makes, `-(D[n] - D[n-1])`, as a fraction
    /// of the speed: a head falling behind is the tape running slow.
    fn speed_of(sway: &[f64]) -> Vec<f64> {
        sway.windows(2).map(|w| -(w[1] - w[0])).collect()
    }

    /// **What a flutter meter reads off a speed deviation** sampled at `sr`,
    /// in percent of the speed: through the weighting or, unweighted, through
    /// a meter's 200 Hz demodulator low-pass (which leaves the scrape's
    /// kilohertz grain out, as a meter's does), into the 2-sigma detector, the
    /// level exceeded 5 % of the time, past `skip` samples of the meter
    /// settling.
    fn meter_2sigma(dev: &[f64], sr: f64, skip: usize, weighted: bool) -> f64 {
        let mut weighting = flutter_weighting(sr);
        let mut demod = Butter::new(2, sr, 200.0, false);
        let mut read: Vec<f64> = dev
            .iter()
            .map(|&s| if weighted { weighting.run(s) } else { demod.run(s) }.abs())
            .skip(skip)
            .collect();
        read.sort_by(f64::total_cmp);
        100.0 * read[(0.95 * read.len() as f64) as usize]
    }

    /// [`meter_2sigma`] of a sway.
    fn flutter_2sigma(sway: &[f64], sr: f64, skip: usize, weighted: bool) -> f64 {
        meter_2sigma(&speed_of(sway), sr, skip, weighted)
    }

    /// The speed deviation a sway makes through the meter's 200 Hz
    /// demodulator, frame by frame, in percent.
    fn demodulated(sway: &[f64], sr: f64) -> Vec<f64> {
        let mut demod = Butter::new(2, sr, 200.0, false);
        speed_of(sway).into_iter().map(|s| 100.0 * demod.run(s)).collect()
    }

    /// The largest speed deviation a sway makes past `skip` frames, in
    /// percent, through the meter's 200 Hz demodulator.
    fn flutter_peak(sway: &[f64], sr: f64, skip: usize) -> f64 {
        demodulated(sway, sr).into_iter().skip(skip).fold(0.0, |m, v| m.max(v.abs()))
    }

    /// **The oversampler is invisible to the host.** The record stage's
    /// filter pair delays the signal by [`OS_LATENCY`] samples, and each play
    /// head gives up exactly that many, so what a host is told to compensate
    /// is still [`BASE_MS`] on the studio head and [`DEEP_MS`] on the deep
    /// one, at every rate an engine runs at. A unit that quietly grew three
    /// quarters of a millisecond would slide against every other track in
    /// the session and nothing would say why.
    ///
    /// The figure is asked of the settings, so it is also pinned against
    /// them: each head and the pair add up to its figure exactly, and of
    /// every position of the thirteen dials only the wow range moves it (the
    /// owner's ruling), the speed included, because a report that moved with
    /// a character dial would re-time the lane in the compensation graph
    /// every time somebody turned it. And the figure a face prints
    /// ([`latency_ms`]) is the same frames, at 22.05 kHz too.
    #[test]
    fn the_oversampler_costs_the_host_no_latency() {
        let positions = [0.0_f32, 0.5, 1.0];
        for sr in [44_100.0_f32, 48_000.0, 96_000.0] {
            let unit = Tape::new(sr);
            for (range, ms, head) in [
                (WowRange::Studio, BASE_MS, unit.base),
                (WowRange::Deep, DEEP_MS, unit.base_deep),
            ] {
                let want = (ms * 1.0e-3 * sr).round() as usize;
                let got = unit.latency_frames(&TapeParams {
                    wow_range: range.dial(),
                    ..TapeParams::default()
                });
                assert_eq!(
                    got,
                    want,
                    "at {sr} Hz the {} head reports {got} frames of latency, not the {want} its \
                     head promises",
                    range.label()
                );
                assert_eq!(
                    head + OS_LATENCY,
                    got,
                    "at {sr} Hz the {} head ({head}) and the pair ({OS_LATENCY}) do not add up \
                     to the {got} frames reported",
                    range.label()
                );
                for combo in 0..3_usize.pow(12) {
                    let at = |k: u32| positions[combo / 3_usize.pow(k) % 3];
                    let p = TapeParams {
                        drive: at(0),
                        bump: at(1),
                        roll: at(2),
                        wobble: at(3),
                        hiss: at(4),
                        trim: at(5),
                        speed: at(6),
                        flutter: at(7),
                        rate: at(8),
                        wow_range: range.dial(),
                        bias: at(9),
                        age: at(10),
                        damage: at(11),
                    };
                    assert_eq!(
                        unit.latency_frames(&p),
                        want,
                        "dial combination {combo} moved the {} head's report at {sr} Hz",
                        range.label()
                    );
                }
            }
        }
        for sr in [22_050.0_f32, 44_100.0, 48_000.0, 96_000.0] {
            let unit = Tape::new(sr);
            for range in WowRange::ALL {
                let p = TapeParams {
                    wow_range: range.dial(),
                    ..TapeParams::default()
                };
                assert_eq!(
                    (latency_ms(range, sr) * sr / 1_000.0).round() as usize,
                    unit.latency_frames(&p),
                    "at {sr} Hz the face is told {} ms for the {} head, and the host {} frames",
                    latency_ms(range, sr),
                    range.label(),
                    unit.latency_frames(&p)
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
    /// with every colour off on either head and at a fresh tape's settings
    /// with the transport (Wow and Flutter) and the hiss still. Minimum-phase
    /// stages add their delay after the peak without moving it.
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
                flutter: 0.0,
                ..TapeParams::default()
            };
            let centred = matches!(speed, TapeSpeed::Ips15 | TapeSpeed::Ips30);
            cases.push((speed.label(), "every colour off", neutral, centred));
            cases.push((speed.label(), "a fresh tape, transport and hiss still", still, false));
            let deep = TapeParams {
                wow_range: WowRange::Deep.dial(),
                ..neutral
            };
            cases.push((speed.label(), "every colour off on the deep head", deep, centred));
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
    ///
    /// **And the transport is the same machine.** Its parts turn in hertz
    /// and sway in seconds, and the slow wander draws its own noise, so it
    /// breathes the same values at the same moments at every rate: a fresh
    /// tape's weighted flutter at 15 ips, 20 s of the head's sway through
    /// the meter, reads within 5 % at 48 and 96 kHz of what it reads at
    /// 44.1; and the tape's 44 Hz resonance, at Flutter 1 with the wow
    /// still, sways the head by the same time within a decibel, 30 s of its
    /// sway in seconds through a 36 to 52 Hz band (its noise is drawn per
    /// frame, so each rate hears its own, which is why it gets a decibel).
    /// Measured: +1.1 and +0.6 %; -0.35 and -0.16 dB.
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

        let transport = |sr: f32| {
            let fresh = Tape::new(sr).transport_trace(&TapeParams::default(), (sr * 20.0) as usize);
            let weighted = flutter_2sigma(&fresh, sr.into(), (sr * 5.0) as usize, true);
            let p = TapeParams {
                wobble: 0.0,
                flutter: 1.0,
                ..TapeParams::default()
            };
            let seconds: Vec<f32> = Tape::new(sr)
                .transport_trace(&p, (sr * 30.0) as usize)
                .into_iter()
                .map(|d| (d / f64::from(sr)) as f32)
                .collect();
            let resonance = 10.0 * band_power(&seconds, sr.into(), 36.0, 52.0, (sr * 1.0) as usize).log10();
            (weighted, resonance)
        };
        let (weighted, resonance) = transport(44_100.0);
        for sr in [48_000.0_f32, 96_000.0] {
            let (w, r) = transport(sr);
            assert!(
                (w / weighted - 1.0).abs() < 0.05,
                "a fresh tape's weighted flutter reads {w:.4} % at {sr} Hz and {weighted:.4} % at 44.1 kHz"
            );
            assert!(
                (r - resonance).abs() < 1.0,
                "the tape's 44 Hz resonance sways the head {:+.2} dB at {sr} Hz against 44.1 kHz",
                r - resonance
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
    /// So a 5 kHz tone through full Flutter, the scrape's dial, everything
    /// else off, is shifted down to 0 Hz and the band 600 Hz to 4 kHz either
    /// side of it measured against the tone. The transport's rotating parts
    /// and its 44 Hz resonance sit within 100 Hz of the tone and the shift's
    /// image twice the tone away, far outside the band, so what is measured
    /// is the scrape's own spray, 43 dB under the tone, and it must read the
    /// same within a quarter of a decibel at 48 and at 96 kHz as at 44.1.
    /// Measured: -0.02 and -0.09 dB; without the root, -0.39 and -3.45.
    #[test]
    fn the_scrape_sprays_the_same_sidebands_at_every_sample_rate() {
        const TONE: f64 = 5_000.0;
        let p = TapeParams {
            flutter: 1.0,
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
        let knobs: [(&str, fn(&mut TapeParams, f32), f64); 13] = [
            // -3.7 dB.
            ("drive", |p, v| p.drive = v, -7.0),
            // -36.5 dB: at 15 ips the contour's bell sits near 35 Hz, a
            // thin slice of broadband noise; the bump's own size is pinned
            // in hertz by its octave test and by the sample-rate test.
            ("bump", |p, v| p.bump = v, -40.0),
            // -2.5 dB.
            ("roll", |p, v| p.roll = v, -6.0),
            // +3.1 dB: Wow alone now, its reel and pinch roller swaying the
            // head by up to twenty samples at full travel, and broadband
            // noise a few samples apart is another noise, so the difference
            // is the programme's own level twice over, which is as loud as
            // this measure can read.
            ("wobble", |p, v| p.wobble = v, 0.0),
            // -9.5 dB, most of it the modulation noise: at Hiss 1 each
            // channel's head jitters by up to 2 µm of spacing, which moves
            // broadband programme far more than the hiss under it does. Its
            // skirt and the hiss's colour are pinned by their own tests.
            ("hiss", |p, v| p.hiss = v, -13.0),
            // +9.5 dB.
            ("trim", |p, v| p.trim = v, 6.0),
            // +2.9 dB: 15 ips to either end of the selector, which moves
            // the transport's parts with it as well as the voicing.
            ("speed", |p, v| p.speed = v, -1.0),
            // +0.9 dB: Flutter's parts sway the head by a few samples at
            // full travel.
            ("flutter", |p, v| p.flutter = v, -3.0),
            // +3.1 dB: the parts turning at half or twice their speed sway
            // the head through other samples.
            ("rate", |p, v| p.rate = v, 0.0),
            // +3.0 dB: the deep head plays the tape 23 ms later, with a
            // cassette's sway on it.
            ("wow_range", |p, v| p.wow_range = v, 0.0),
            // -4.6 dB: the top of the dial dulls 10 kHz by seven decibels at
            // 15 ips, and the bottom lifts it and saturates the bass sooner.
            ("bias", |p, v| p.bias = v, -8.0),
            // -5.2 dB: at 15 ips full Age takes 5 dB off 10 kHz and skews
            // each track a quarter of a sample.
            ("age", |p, v| p.age = v, -9.0),
            // -15.5 dB: full Damage at 15 ips, its worn edges riding the
            // tape up to 2 µm off the head and 1.5 dropouts a second.
            ("damage", |p, v| p.damage = v, -19.0),
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
    ///
    /// The bump's frequency is the one the face prints, [`bump_hz`], so the
    /// figure a face shows is the frequency the bell is measured to lift:
    /// 8.74, 17.48, 34.95 and 69.91 Hz, at any Bump.
    #[test]
    fn the_head_bump_moves_an_octave_with_every_doubling_of_speed() {
        let sr = 48_000.0_f32;
        let amp = 0.1_f32;
        for (speed, want) in TapeSpeed::ALL.into_iter().zip([8.74, 17.48, 34.95, 69.91]) {
            for bump in [0.0, 0.5, 1.0] {
                let p = TapeParams {
                    bump,
                    speed: speed.dial(),
                    ..TapeParams::default()
                };
                let got = f64::from(bump_hz(&p));
                assert!(
                    (got - want).abs() < 0.01,
                    "at {} and Bump {bump} the face is told the bump sits at {got:.3} Hz, \
                     not {want}",
                    speed.label()
                );
            }
        }
        let at = |speed: TapeSpeed| {
            let f_b = f64::from(bump_hz(&TapeParams {
                speed: speed.dial(),
                ..TapeParams::NEUTRAL
            }));
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

    /// **The line-up tone comes back at its level at every speed and bias.**
    ///
    /// A machine is lined up with a 1 kHz tone at 0 VU, and the line-up's
    /// whole job is that tone: from the bottom of the dial to the Drive a
    /// fresh tape opens at and a tenth past it, at all four speeds, and at
    /// the bottom, middle and top of Bias, where the machine is lined up
    /// again (the bias shelf is unity at 1 kHz, and the line-up reads the
    /// record level the tone meets through the under-bias shelf), it comes
    /// back within 0.35 dB of where it went in (model 0.23 at most;
    /// measured, at its worst, -0.25 dB at 3.75 ips and Drive 0.6 at the
    /// bottom of Bias, -0.24 aligned and at the top, and under 0.08 at the
    /// other three speeds). What is left over is the
    /// self-erasure the slow speeds already do at that level, and the
    /// refused flux's lift past where its correction holds.
    #[test]
    fn the_line_up_tone_comes_back_at_its_level_at_every_speed_and_bias() {
        let sr = 48_000.0_f32;
        let amp = 10.0_f32.powf(-18.0 / 20.0) * std::f32::consts::SQRT_2;
        for speed in TapeSpeed::ALL {
            for bias in [0.0_f32, 0.5, 1.0] {
                for step in 0..=6 {
                    let p = TapeParams {
                        drive: step as f32 / 10.0,
                        speed: speed.dial(),
                        bias,
                        ..TapeParams::NEUTRAL
                    };
                    let level = tone_through(&p, sr, 1_000.0, amp, 0.2, 0.2, None);
                    assert!(
                        level.abs() < 0.35,
                        "at {} Drive {} and Bias {bias} the 0 VU line-up tone comes back {level:+.3} dB",
                        speed.label(),
                        step as f32 / 10.0
                    );
                }
            }
        }
    }

    /// The Bias dial's position for a bias of `b` dB re aligned
    /// ([`BIAS_RANGE_DB`]).
    fn bias_at(b: f64) -> f32 {
        ((b / f64::from(BIAS_RANGE_DB) + 1.0) / 2.0) as f32
    }

    /// **Over-bias dulls the top and under-bias lifts it, by the published
    /// slope.**
    ///
    /// A tape's small-signal treble falls about 0.8 dB at 10 kHz for every
    /// dB of over-bias at 15 ips, faster at 16 kHz, and rises toward a peak
    /// about 5.4 dB under the aligned point; the machine is lined up at
    /// 1 kHz after every change, so 1 kHz is what everything is read
    /// against. At 15 ips, Drive 0.2 and -30 dBFS: the 10 kHz slope at
    /// aligned is -0.95 to -0.65 dB per dB and 16 kHz's -1.5 to -0.9 (model
    /// -0.81 and -1.17); 10 kHz at the bottom of the dial stands 2.3 to
    /// 3.4 dB over aligned and at the top 6.3 to 8.3 under (model +2.76 and
    /// -7.16). The published ladders, 10 kHz against a long wavelength from
    /// the peak to aligned, are each speed's: 4.9 to 6.2 dB at 3.75 ips, 4.1
    /// to 5.4 at 7.5, 2.6 to 3.4 at 15 and 0.9 to 1.6 at 30 (model 5.62,
    /// 4.76, 3.00 and 1.24).
    ///
    /// **The bias tilt is pinned, as a decision.** Holding 1 kHz leaves a
    /// long wavelength where the shelf puts it, and at 3.75 ips, where even
    /// 1 kHz is short enough for over-bias to dull, that is audible: a
    /// 100 Hz tone stands 2.5 dB over the aligned machine's at the top of
    /// the dial, within 0.4 dB, and 0.85 under at the bottom, within 0.25;
    /// 0.76 over at 7.5 ips, within 0.25; and within 0.3 at 15 and 30. Each
    /// is read against the same tone through the aligned machine, which is
    /// the bias's own share: the head's own low end takes a third of a
    /// decibel at 100 Hz at 30 ips whatever the bias. And the top of the
    /// dial reads the same 10 kHz at 44.1, 48 and 96 kHz within 0.2 dB.
    /// Measured: -0.79 and -1.13 dB per dB; +2.76 and -7.10 dB; ladders of
    /// 5.61, 4.75, 3.00 and 1.25 dB; the tilt +2.54 and -0.79 at 3.75 ips,
    /// +0.81 at 7.5, +0.22 at 15 and +0.06 at 30; and -7.63 dB at all three
    /// rates within 0.002.
    #[test]
    fn over_bias_dulls_the_top_and_under_bias_lifts_it_by_the_published_slope() {
        let amp = 10.0_f32.powf(-30.0 / 20.0);
        let line = |speed: TapeSpeed, b: f64, sr: f32, hz: f64| {
            let p = TapeParams {
                drive: 0.2,
                speed: speed.dial(),
                bias: bias_at(b),
                ..TapeParams::NEUTRAL
            };
            tone_through(&p, sr, hz, amp, 0.2, 0.2, None)
        };
        let sr = 48_000.0_f32;
        let studio = TapeSpeed::Ips15;
        // 10 and 16 kHz against 1 kHz, a quarter of a decibel either side
        // of aligned.
        let (up, down) = (line(studio, 0.25, sr, 1_000.0), line(studio, -0.25, sr, 1_000.0));
        let slope = |hz: f64| ((line(studio, 0.25, sr, hz) - up) - (line(studio, -0.25, sr, hz) - down)) / 0.5;
        let (s10, s16) = (slope(10_000.0), slope(16_000.0));
        assert!(
            (-0.95..=-0.65).contains(&s10),
            "at 15 ips 10 kHz moves {s10:+.3} dB per dB of bias at aligned"
        );
        assert!(
            (-1.5..=-0.9).contains(&s16),
            "at 15 ips 16 kHz moves {s16:+.3} dB per dB of bias at aligned"
        );
        let top = |b: f64| line(studio, b, sr, 10_000.0) - line(studio, b, sr, 1_000.0);
        let aligned = top(0.0);
        let (lift, dull) = (top(-6.0) - aligned, top(6.0) - aligned);
        assert!(
            (2.3..=3.4).contains(&lift),
            "at 15 ips the bottom of Bias lifts 10 kHz {lift:+.2} dB over aligned"
        );
        assert!(
            (-8.3..=-6.3).contains(&dull),
            "at 15 ips the top of Bias takes 10 kHz {dull:+.2} dB under aligned"
        );

        for (speed, ladder, tilt) in [
            (TapeSpeed::Ips3_75, 4.9..=6.2, 2.1..=2.9),
            (TapeSpeed::Ips7_5, 4.1..=5.4, 0.51..=1.01),
            (TapeSpeed::Ips15, 2.6..=3.4, -0.3..=0.3),
            (TapeSpeed::Ips30, 0.9..=1.6, -0.3..=0.3),
        ] {
            let hundred = |b: f64| line(speed, b, sr, 100.0);
            let held = hundred(0.0);
            let swing = (line(speed, -5.4, sr, 10_000.0) - hundred(-5.4)) - (line(speed, 0.0, sr, 10_000.0) - held);
            assert!(
                ladder.contains(&swing),
                "at {} 10 kHz against 100 Hz swings {swing:+.2} dB from the bias peak to aligned, \
                 outside {ladder:?}",
                speed.label()
            );
            let over = hundred(6.0) - held;
            assert!(
                tilt.contains(&over),
                "at {} the top of Bias leaves a 100 Hz tone {over:+.2} dB from the aligned machine's, \
                 outside {tilt:?}",
                speed.label()
            );
            if speed == TapeSpeed::Ips3_75 {
                let under = hundred(-6.0) - held;
                assert!(
                    (under + 0.85).abs() <= 0.25,
                    "at 3.75 ips the bottom of Bias leaves a 100 Hz tone {under:+.2} dB from the aligned \
                     machine's"
                );
            }
        }

        let rates: Vec<f64> = [44_100.0_f32, 48_000.0, 96_000.0]
            .into_iter()
            .map(|sr| line(studio, 6.0, sr, 10_000.0) - line(studio, 6.0, sr, 1_000.0))
            .collect();
        let spread = rates.iter().fold(f64::MIN, |m, v| m.max(*v)) - rates.iter().fold(f64::MAX, |m, v| m.min(*v));
        assert!(
            spread < 0.2,
            "the top of Bias reads 10 kHz {rates:.2?} dB against 1 kHz at 44.1, 48 and 96 kHz"
        );
    }

    /// **Under-bias saturates sooner without moving the line-up.** An
    /// under-biased tape's bass meets the curve hotter, which is a lower
    /// ceiling and more third harmonic on a tone at the same level, while
    /// the machine lined up again at 1 kHz plays the tone back where it
    /// was. At 15 ips and Drive 0.6, a 220 Hz tone at 0 VU at the bottom of
    /// Bias carries at least three times the aligned tape's HD3 (model
    /// 6.8 times), at a level within half a decibel of the aligned tape's
    /// (model -0.13 dB). Measured: 6.8 times (4.83 against 0.71 %), at
    /// -0.11 dB.
    #[test]
    fn under_bias_saturates_sooner_without_moving_the_line_up() {
        let sr = 48_000.0_f32;
        let at = |bias: f32| TapeParams {
            drive: 0.6,
            bias,
            ..TapeParams::NEUTRAL
        };
        let hd3 = |bias: f32| 10.0_f64.powf(tone_through(&at(bias), sr, 220.0, zero_vu(), 0.3, 0.6, Some(3.0)) / 20.0);
        let level = |bias: f32| tone_through(&at(bias), sr, 220.0, zero_vu(), 0.3, 0.6, None);
        let (under, aligned) = (hd3(0.0), hd3(0.5));
        assert!(
            under >= 3.0 * aligned,
            "under-biased, a 0 VU 220 Hz tone carries {:.2} % HD3 against {:.2} % aligned: only {:.1} times",
            under * 100.0,
            aligned * 100.0,
            under / aligned
        );
        let moved = level(0.0) - level(0.5);
        assert!(
            moved.abs() < 0.5,
            "under-biased, the 0 VU 220 Hz tone comes back {moved:+.2} dB from the aligned tape's"
        );
    }

    /// The tape's maximum output level at 1 kHz through `p`: the output, in
    /// dB re 0 VU, at the input level where the third harmonic reaches 3 %
    /// of the fundamental, found by bisection.
    fn mol(p: &TapeParams, sr: f32) -> f64 {
        let at = |level_db: f64| zero_vu() * 10.0_f32.powf(level_db as f32 / 20.0);
        let (mut lo, mut hi) = (-12.0_f64, 24.0_f64);
        for _ in 0..16 {
            let mid = 0.5 * (lo + hi);
            if tone_through(p, sr, 1_000.0, at(mid), 0.1, 0.05, Some(3.0)) < db(0.03) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let level = 0.5 * (lo + hi);
        level + tone_through(p, sr, 1_000.0, at(level), 0.1, 0.05, None)
    }

    /// **Under-bias takes the bass's headroom first and leaves the treble
    /// its own.**
    ///
    /// The datasheets put an under-biased tape's maximum output about
    /// 1.4 dB lower for every dB it is under, and its 10 kHz ceiling where
    /// it was or a little higher: bias reaches the curve at long
    /// wavelengths only. So at 15 ips and Drive 0.6 the 1 kHz MOL falls
    /// 1.2 to 1.6 dB per dB, from aligned to 3 and to 6 dB under (model 1.45
    /// and 1.37); and a 38.1 µm tone (2.5, 5 and 10 kHz at 3.75, 7.5 and
    /// 15 ips), read as its output at its own 1 dB compression point
    /// against the 1 kHz small-signal level, comes out at least a decibel
    /// higher at the bottom of Bias than aligned (model +1.90, +2.50 and
    /// +2.55): the shelf that lifts its small-signal level is after the
    /// curve, and the low shelf before the curve barely reaches it.
    /// Measured: -1.45 and -1.37 dB per dB; +1.91, +2.52 and +2.63 dB.
    #[test]
    fn under_bias_takes_the_bass_headroom_first_and_leaves_the_treble_its_own() {
        let sr = 48_000.0_f32;
        let at = |speed: TapeSpeed, b: f64| TapeParams {
            drive: 0.6,
            speed: speed.dial(),
            bias: bias_at(b),
            ..TapeParams::NEUTRAL
        };
        let aligned = mol(&at(TapeSpeed::Ips15, 0.0), sr);
        for under in [3.0, 6.0] {
            let slope = (mol(&at(TapeSpeed::Ips15, -under), sr) - aligned) / under;
            assert!(
                (-1.6..=-1.2).contains(&slope),
                "at 15 ips {under} dB of under-bias moves the 1 kHz MOL {slope:+.2} dB per dB"
            );
        }
        let small = zero_vu() * 10.0_f32.powf(-30.0 / 20.0);
        for (speed, hz) in [(TapeSpeed::Ips3_75, 2_500.0), (TapeSpeed::Ips7_5, 5_000.0), (TapeSpeed::Ips15, 10_000.0)] {
            let ceiling = |b: f64| {
                let p = at(speed, b);
                let gain = |f: f64| tone_through(&p, sr, f, small, 0.1, 0.05, None);
                compression_point(&p, sr, hz) + (gain(hz) - 1.0) - gain(1_000.0)
            };
            let kept = ceiling(-6.0) - ceiling(0.0);
            assert!(
                kept >= 1.0,
                "at {} a {hz} Hz tone's ceiling moves {kept:+.2} dB from aligned to the bottom of Bias",
                speed.label()
            );
        }
    }

    /// **Age takes the treble by spacing, and slow tape loses the most.**
    ///
    /// An old tape rides off a worn head, and Wallace's law makes that gap
    /// a loss in decibels proportional to it over the wavelength, so the
    /// same spacing costs a slow tape's short wavelengths far more. At
    /// full Age, against new tape, with Drive 0 and a -20 dBFS tone, 10 kHz
    /// loses 19.0 to 23.5 dB at 3.75 ips, 10.0 to 13.0 at 7.5, 4.3 to 6.5
    /// at 15 and 1.7 to 3.3 at 30 (model -20.8, -11.4, -5.3 and -2.5: the
    /// 3 µm spacing, the azimuth's loss at its angle, and the worn head's
    /// over-bias, the machine lined up again at 1 kHz), each speed losing
    /// more than the one above it; 1 kHz at 15 ips loses 0.2 to 0.8 dB
    /// (model -0.52); and at 3.75 ips the loss at 10 kHz is the same at
    /// 44.1, 48 and 96 kHz within 0.3 dB, every corner being matched at
    /// 10 kHz.
    ///
    /// The head is held square in time for this ([`Tape::square_head`]): an
    /// aged head's skew reads each track between samples, and the play
    /// head's Hermite costs a 10 kHz tone up to half a decibel at 44.1 and
    /// 48 kHz where it reads half way (a hundredth at 96), which the
    /// parked head of Age 0 never pays. That loss is the play head's, not
    /// Age's, and a moving transport pays it at any Age, so a ruler that
    /// counted it would pin the interpolator, not the spacing; with the
    /// skew in, 3.75 ips reads -21.80, -21.79 and -21.30 dB at the three
    /// rates. Measured square: -21.27, -11.54, -5.22 and -2.34 dB at 48 kHz,
    /// 1 kHz -0.53, and 3.75 ips -21.27, -21.27 and -21.28 at the three
    /// rates.
    #[test]
    fn age_takes_the_treble_by_spacing_and_slow_tape_loses_the_most() {
        let amp = 0.1_f32;
        let loss = |speed: TapeSpeed, sr: f32, hz: f64| {
            let at = |age: f32| {
                let p = TapeParams {
                    speed: speed.dial(),
                    age,
                    ..TapeParams::NEUTRAL
                };
                let frames = (sr * 0.7) as usize;
                let mono: Vec<f32> = (0..frames)
                    .map(|i| amp * (std::f64::consts::TAU * hz * i as f64 / f64::from(sr)).sin() as f32)
                    .collect();
                let mut unit = Tape::new(sr);
                unit.square_head = true;
                let out = left(&feed(&mut unit, &to_stereo(&mono), &p));
                let lo = (sr * 0.3) as usize;
                db(band(&out[lo..], sr.into(), hz) / band(&mono[lo..], sr.into(), hz))
            };
            at(1.0) - at(0.0)
        };
        let sr = 48_000.0_f32;
        let tops: Vec<f64> = TapeSpeed::ALL.iter().map(|v| loss(*v, sr, 10_000.0)).collect();
        for ((speed, top), range) in TapeSpeed::ALL.iter().zip(&tops).zip([-23.5..=-19.0, -13.0..=-10.0, -6.5..=-4.3, -3.3..=-1.7]) {
            assert!(
                range.contains(top),
                "at {} full Age takes {top:+.2} dB off 10 kHz, outside {range:?}",
                speed.label()
            );
        }
        assert!(
            tops.windows(2).all(|w| w[0] < w[1]),
            "the loss is not ordered by speed: {tops:.2?} dB at 3.75, 7.5, 15 and 30 ips"
        );
        let low = loss(TapeSpeed::Ips15, sr, 1_000.0);
        assert!(
            (-0.8..=-0.2).contains(&low),
            "at 15 ips full Age takes {low:+.2} dB off 1 kHz"
        );
        let rates: Vec<f64> = [44_100.0_f32, 48_000.0, 96_000.0]
            .into_iter()
            .map(|sr| loss(TapeSpeed::Ips3_75, sr, 10_000.0))
            .collect();
        let spread = rates.iter().fold(f64::MIN, |m, v| m.max(*v)) - rates.iter().fold(f64::MAX, |m, v| m.min(*v));
        assert!(
            spread < 0.3,
            "full Age at 3.75 ips takes {rates:.2?} dB off 10 kHz at 44.1, 48 and 96 kHz"
        );
    }

    /// **Age pulls the two tracks apart in time, like a skewed head.**
    ///
    /// A head's gap a thousand microradians off square, across the 4 mm
    /// between the tracks, reads one track 42 µs after the other on
    /// 3.75 ips tape, and the slitting's wander swings that angle half as
    /// much again either way every 300 mm. So the same noise on both
    /// tracks, through full Age at 3.75 ips and 48 kHz with every colour
    /// off, comes out with the left 2.0 samples behind the right on
    /// average, within half a sample, read window by window off their
    /// cross-correlation; over 4 s, more than one turn of the wander, the
    /// lag swings 2.0 samples peak to peak, within 30 %. And at Age 0 the
    /// two tracks are bit for bit the same. The head keeps each track's
    /// share of the widest skew out of the room its transport may sway, so
    /// the skew can never carry a track off the line. Measured: a mean of
    /// 2.20 samples (4 s is 1.3 turns of the wander, so the mean leans to
    /// the turn it saw twice), swinging from 1.01 to 3.04; the room 1.512
    /// samples less, against a widest share of 1.520 as the lag reads it.
    #[test]
    fn age_pulls_the_two_tracks_apart_in_time_like_a_skewed_head() {
        let sr = 48_000.0_f32;
        let frames = (sr * 4.5) as usize;
        let mut seed = 0x0A6E_5EED_u32;
        let mono: Vec<f32> = (0..frames).map(|_| 0.25 * white(&mut seed)).collect();
        let run = |age: f32| {
            let p = TapeParams {
                speed: TapeSpeed::Ips3_75.dial(),
                age,
                ..TapeParams::NEUTRAL
            };
            let mut unit = Tape::new(sr);
            let out = feed(&mut unit, &to_stereo(&mono), &p);
            (out, unit.governed().room)
        };
        let (new, room_new) = run(0.0);
        assert!(
            new.chunks_exact(2).all(|f| f[0].to_bits() == f[1].to_bits()),
            "at Age 0 the two tracks differ"
        );
        let (aged, room_aged) = run(1.0);
        let (l, r) = (left(&aged), aged.chunks_exact(2).map(|f| f[1]).collect::<Vec<f32>>());
        // Each window's lag of the left behind the right: the peak of their
        // cross-correlation, placed between samples by a parabola.
        let window = 2_048;
        let lags: Vec<f64> = (0..)
            .map(|k| (sr * 0.5) as usize + k * window)
            .take_while(|at| at + window + 8 < frames)
            .map(|at| {
                let corr = |lag: i64| -> f64 {
                    (at..at + window)
                        .map(|n| f64::from(l[n]) * f64::from(r[(n as i64 - lag) as usize]))
                        .sum()
                };
                let c: Vec<f64> = (-6..=6).map(corr).collect();
                let k = (1..c.len() - 1).max_by(|a, b| c[*a].total_cmp(&c[*b])).expect("a peak");
                let (a, b, cc) = (c[k - 1], c[k], c[k + 1]);
                (k as f64 - 6.0) + 0.5 * (a - cc) / (a - 2.0 * b + cc)
            })
            .collect();
        let mean = lags.iter().sum::<f64>() / lags.len() as f64;
        let (lo, hi) = lags.iter().fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        assert!(
            (mean - 2.0).abs() <= 0.5,
            "at full Age on 3.75 ips tape the left runs {mean:.3} samples behind the right"
        );
        assert!(
            ((hi - lo) / 2.0 - 1.0).abs() <= 0.3,
            "at full Age the lag swings {:.3} samples peak to peak over 4 s",
            hi - lo
        );
        // And the head keeps the skew's room: each track's share of the
        // widest lag measured comes out of what the transport may sway,
        // within the few hundredths of a sample the lag's parabola reads to.
        assert!(
            room_new - room_aged >= 0.5 * hi - 0.05,
            "at full Age the head keeps {:.3} samples back for a skew that reached {:.3} a track",
            room_new - room_aged,
            0.5 * hi
        );
    }

    /// **The magnetised head adds a second harmonic that does not follow
    /// level.**
    ///
    /// A head left magnetised records a little of its own field onto every
    /// pass, and what that does is even-order and the same share at every
    /// level, unlike a curve's distortion, which grows with level. So at
    /// full Age, Drive 0, a 220 Hz tone at -30, -18 and -10 dBFS carries
    /// 0.5 % of second harmonic within 0.1 at all three; new tape carries
    /// under 0.005 %; and the rectified DC is gone, under 1e-5 after a
    /// second (the head's own low-end roll-off downstream would take it as
    /// well, so this leg holds the chain, not the blocker alone). Measured:
    /// 0.503 % at all three levels, new tape under a millionth, and 1.1e-6
    /// of DC.
    #[test]
    fn the_magnetised_head_adds_a_second_harmonic_that_does_not_follow_level() {
        let sr = 48_000.0_f32;
        let at = |age: f32, dbfs: f32| {
            let p = TapeParams {
                age,
                ..TapeParams::NEUTRAL
            };
            let amp = 10.0_f32.powf(dbfs / 20.0);
            10.0_f64.powf(tone_through(&p, sr, 220.0, amp, 0.5, 0.5, Some(2.0)) / 20.0)
        };
        for dbfs in [-30.0, -18.0, -10.0] {
            let h2 = at(1.0, dbfs);
            assert!(
                (h2 - 0.005).abs() <= 0.001,
                "at full Age a 220 Hz tone at {dbfs} dBFS carries {:.3} % of second harmonic",
                h2 * 100.0
            );
        }
        let new = at(0.0, -18.0);
        assert!(new < 5.0e-5, "new tape carries {:.4} % of second harmonic", new * 100.0);
        let frames = (sr * 1.5) as usize;
        let mono: Vec<f32> = (0..frames)
            .map(|i| 0.3 * (std::f32::consts::TAU * 220.0 * i as f32 / sr).sin())
            .collect();
        let p = TapeParams {
            age: 1.0,
            ..TapeParams::NEUTRAL
        };
        let out = left(&feed(&mut Tape::new(sr), &to_stereo(&mono), &p));
        let tail = &out[(sr * 1.0) as usize..];
        let periods = (tail.len() as f32 * 220.0 / sr).floor() as usize;
        let whole = (periods as f32 * sr / 220.0) as usize;
        let dc = tail[..whole].iter().map(|v| f64::from(*v)).sum::<f64>() / whole as f64;
        assert!(dc.abs() < 1.0e-5, "a second on, the magnetised head leaves {dc:.2e} of DC");
    }

    /// **Dropouts arrive per metre of tape and last as long as their
    /// stretch.**
    ///
    /// A defect is a place on the tape, so how often one comes is per
    /// metre and how long it plays is its length over the speed. Ten
    /// minutes of the wear alone, at 8 kHz (the wear is laid out in metres,
    /// so the rate is only its grid): at full Damage, 4 dropouts a metre
    /// meet 15 ips tape 914 times (800 to 1030) and creases at half a metre's
    /// one 114 times (85 to 145); 3.75 ips tape a quarter as often, 229 (190
    /// to 270); half Damage at 15 ips, on the dial's square, 229 as well. A
    /// dropout's length is log-uniform from 0.5 to 8 mm, 2.7 mm on average,
    /// which plays 7.10 ms at 15 ips (6.25 to 7.95) and 28.4 at 3.75 (25.0 to
    /// 31.8), four times as long (3.6 to 4.4). They take both tracks 60 % of
    /// the time and each alone 20 %, within five points. At Damage 0 nothing
    /// is drawn: no defect, and the stream still at its seed. And the same
    /// minute of tape draws the same defects at 44.1, 48 and 96 kHz.
    /// Measured: 955 dropouts and 121 creases at 15 ips, 225 dropouts at
    /// 3.75 and at half Damage (the same draws spent at the same expected
    /// count a metre); 7.15 and 27.70 ms, 3.87 times as long; 59.4, 21.2 and
    /// 19.5 % of the masks.
    #[test]
    fn dropouts_arrive_per_metre_of_tape_and_last_as_long_as_their_stretch() {
        let minutes = (600.0 * 8_000.0) as u64;
        let wear = |ips: f64, damage: f64| Wear::run(ips, 8_000.0, damage, 0.0, minutes).log;
        let ms = |log: &WearLog, ips: f64| log.dropout_m / log.dropouts as f64 / (0.0254 * ips) * 1_000.0;
        let full = wear(15.0, 1.0);
        let slow = wear(3.75, 1.0);
        let half = wear(15.0, 0.5);
        for (what, n, range) in [
            ("dropouts at 15 ips", full.dropouts, 800..=1_030),
            ("creases at 15 ips", full.creases, 85..=145),
            ("dropouts at 3.75 ips", slow.dropouts, 190..=270),
            ("dropouts at half Damage", half.dropouts, 190..=270),
        ] {
            assert!(range.contains(&n), "ten minutes of tape met {n} {what}, outside {range:?}");
        }
        let (fast_ms, slow_ms) = (ms(&full, 15.0), ms(&slow, 3.75));
        assert!((6.25..=7.95).contains(&fast_ms), "a dropout plays {fast_ms:.2} ms on average at 15 ips");
        assert!((25.0..=31.8).contains(&slow_ms), "a dropout plays {slow_ms:.2} ms on average at 3.75 ips");
        assert!(
            (3.6..=4.4).contains(&(slow_ms / fast_ms)),
            "a dropout plays {:.2} times as long at 3.75 ips as at 15",
            slow_ms / fast_ms
        );
        for (k, want) in [0.6, 0.2, 0.2].into_iter().enumerate() {
            let share = full.masks[k] as f64 / full.dropouts as f64;
            assert!(
                (share - want).abs() <= 0.05,
                "{:.3} of the dropouts took {}, not {want}",
                share,
                ["both tracks", "the left alone", "the right alone"][k]
            );
        }
        let none = Wear::run(15.0, 8_000.0, 0.0, 0.0, minutes);
        assert_eq!((none.log.dropouts, none.log.creases), (0, 0), "an undamaged tape had defects");
        assert!(
            none.rng == DAMAGE_SEED && none.drop_due.is_nan(),
            "an undamaged tape drew on the damage's stream"
        );
        let counts: Vec<(u64, u64)> = [44_100.0, 48_000.0, 96_000.0]
            .into_iter()
            .map(|sr| {
                let log = Wear::run(15.0, sr, 1.0, 0.0, (60.0 * sr) as u64).log;
                (log.dropouts, log.creases)
            })
            .collect();
        assert!(
            counts.windows(2).all(|w| w[0] == w[1]),
            "a minute of the same tape met {counts:?} dropouts and creases at 44.1, 48 and 96 kHz"
        );
    }

    /// **A dropout takes the treble and leaves the level.**
    ///
    /// Spacing costs `54.575 d / wavelength` dB, so the 20 µm peak of a
    /// deep dropout is a near-silence for 10 kHz and a dip for the bass. An
    /// 8 mm dropout laid on 15 ips tape under a steady tone, every colour
    /// off: at its middle 10 kHz has lost at least 22 dB (the three-pole
    /// fit -26.1), and 500 Hz 0.8 to 2.2 dB (the law -1.43); on 3.75 ips
    /// tape 500 Hz loses 4.5 to 7.0 dB (the law -5.73), and the dropout
    /// lasts its 8 mm, 84 ms, within 2. Measured: -26.06 and -1.46 dB at
    /// 15 ips, -6.13 at 3.75, lasting 83.9 ms.
    #[test]
    fn a_dropout_takes_the_treble_and_leaves_the_level() {
        let sr = 48_000.0_f32;
        // The tone's amplitude over `w` frames from `from`, by quadrature.
        let amp = |x: &[f32], from: usize, w: usize, hz: f64| {
            let (mut c, mut s) = (0.0_f64, 0.0_f64);
            for (n, v) in x[from..from + w].iter().enumerate() {
                let ph = std::f64::consts::TAU * hz * n as f64 / f64::from(sr);
                c += f64::from(*v) * ph.cos();
                s += f64::from(*v) * ph.sin();
            }
            2.0 * (c * c + s * s).sqrt() / w as f64
        };
        // A dropout of 20 µm over 8 mm laid under a tone: the tone at the
        // dropout's middle against the tone just before it, in dB, and how
        // many frames the tape carried a spacing.
        let dip = |speed: TapeSpeed, hz: f64, cycles: usize| {
            let p = TapeParams {
                speed: speed.dial(),
                ..TapeParams::NEUTRAL
            };
            let mut unit = Tape::new(sr);
            let tone = |n: usize| 0.1 * (std::f64::consts::TAU * hz * n as f64 / f64::from(sr)).sin() as f32;
            let settle = (sr * 0.5) as usize;
            let mut before: Vec<f32> = (0..settle).flat_map(|n| [tone(n), tone(n)]).collect();
            unit.process(&mut before, settle, 2, &p);
            unit.force_dropout(20.0, 8.0);
            let len = 8.0e-3 / (0.0254 * speed.ips()) * f64::from(sr);
            let more = len as usize + (sr * 0.05) as usize;
            let mut during = Vec::with_capacity(more * 2);
            let mut spaced = 0;
            for n in settle..settle + more {
                let mut frame = [tone(n), tone(n)];
                unit.process(&mut frame, 1, 2, &p);
                during.extend_from_slice(&frame);
                if unit.spacing_um()[0] > 0.0 {
                    spaced += 1;
                }
            }
            let w = (cycles as f64 * f64::from(sr) / hz).round() as usize;
            let steady = amp(&left(&before), settle - w, w, hz);
            let middle = amp(&left(&during), (0.5 * len) as usize - w / 2, w, hz);
            (db(middle / steady), f64::from(spaced) / f64::from(sr) * 1_000.0)
        };
        let (top, _) = dip(TapeSpeed::Ips15, 10_000.0, 5);
        let (low, _) = dip(TapeSpeed::Ips15, 500.0, 2);
        let (slow, lasted) = dip(TapeSpeed::Ips3_75, 500.0, 2);
        assert!(top <= -22.0, "at 15 ips a 20 µm dropout leaves 10 kHz at {top:+.2} dB");
        assert!((-2.2..=-0.8).contains(&low), "at 15 ips a 20 µm dropout costs 500 Hz {low:+.2} dB");
        assert!((-7.0..=-4.5).contains(&slow), "at 3.75 ips a 20 µm dropout costs 500 Hz {slow:+.2} dB");
        assert!((lasted - 84.0).abs() <= 2.0, "an 8 mm dropout lasted {lasted:.1} ms on 3.75 ips tape");
    }

    /// **A crease twists the head, and the head keeps room for it.**
    ///
    /// A creased stretch of tape stands off square under the head, so for
    /// its length it skews the two tracks in time as an aged head does. A
    /// 50 mm crease twisting the head 1500 µrad, the most full Damage draws,
    /// laid on new 3.75 ips tape at 48 kHz under the same noise on both
    /// tracks: across its middle the left runs 3.02 samples behind the
    /// right (1500 µrad across the 4 mm between the tracks), within half a
    /// sample. And because any Damage can crease the tape, the head keeps
    /// each track's share of that widest twist out of the room its
    /// transport may sway once Damage is up at all. Measured: a lag of
    /// 3.008 samples, and the room 1.512 samples less, the share exactly.
    #[test]
    fn a_crease_twists_the_head_and_the_head_keeps_room_for_it() {
        let sr = 48_000.0_f32;
        let p = TapeParams {
            speed: TapeSpeed::Ips3_75.dial(),
            ..TapeParams::NEUTRAL
        };
        let mut seed = 0xC4EA_5E00_u32;
        let settle = (sr * 0.2) as usize;
        let len = (0.050 / (0.0254 * 3.75) * f64::from(sr)) as usize;
        let mono: Vec<f32> = (0..settle + len).map(|_| 0.25 * white(&mut seed)).collect();
        let stereo = to_stereo(&mono);
        let mut unit = Tape::new(sr);
        let mut out = stereo.clone();
        unit.process(&mut out[..settle * 2], settle, 2, &p);
        unit.force_crease(2.0, 50.0, 4.0, 1_500.0);
        let mut rest = out[settle * 2..].to_vec();
        let n = rest.len() / 2;
        let mut done = 0;
        while done < n {
            let k = (n - done).min(512);
            unit.process(&mut rest[done * 2..(done + k) * 2], k, 2, &p);
            done += k;
        }
        let (l, r) = (left(&rest), rest.chunks_exact(2).map(|f| f[1]).collect::<Vec<f32>>());
        let (at, window) = (len / 2 - 1_024, 2_048);
        let corr = |lag: i64| -> f64 {
            (at..at + window)
                .map(|n| f64::from(l[n]) * f64::from(r[(n as i64 - lag) as usize]))
                .sum()
        };
        let c: Vec<f64> = (-8..=8).map(corr).collect();
        let k = (1..c.len() - 1).max_by(|a, b| c[*a].total_cmp(&c[*b])).expect("a peak");
        let (a, b, cc) = (c[k - 1], c[k], c[k + 1]);
        let lag = (k as f64 - 8.0) + 0.5 * (a - cc) / (a - 2.0 * b + cc);
        let room = |damage: f32| {
            let mut unit = Tape::new(sr);
            unit.transport_trace(&TapeParams { damage, ..p }, 64);
            unit.governed().room
        };
        let kept = room(0.0) - room(1.0);
        let share = skew_samples(1_500.0, 3.75, f64::from(sr));
        assert!(
            (lag - 3.02).abs() <= 0.5,
            "across a 1500 µrad crease the left runs {lag:.3} samples behind the right"
        );
        assert!(
            kept >= share - 1.0e-9,
            "at full Damage the head keeps {kept:.3} samples back for a crease's {share:.3} a track"
        );
    }

    /// Everything engaged at once: the deep head at full Wow and Flutter, an
    /// old, damaged, under-biased tape hissing at the top of the dial, at
    /// 7.5 ips.
    fn everything_engaged() -> TapeParams {
        TapeParams {
            wobble: 1.0,
            flutter: 1.0,
            hiss: 1.0,
            speed: TapeSpeed::Ips7_5.dial(),
            wow_range: WowRange::Deep.dial(),
            bias: 0.2,
            age: 1.0,
            damage: 1.0,
            ..TapeParams::default()
        }
    }

    /// **Every noise starts again from its seed after `clear`.**
    ///
    /// A bounce must be the same bounce: every generator in the machine
    /// (the hiss, the modulation noise, the transport's parts and its
    /// wanders, the cassette's drift, the damage and the edges) draws from
    /// its own seeded stream, and `clear` puts every stream, phase, history
    /// and the tape's own position back. So with everything engaged, a unit
    /// that has played a second of something else and been cleared plays
    /// four seconds bit for bit as a fresh one does, and two fresh units are
    /// bit for bit alike.
    #[test]
    fn every_noise_starts_again_from_its_seed_after_clear() {
        let sr = 44_100.0_f32;
        let p = everything_engaged();
        let mut seed = 0x5EED_0C1E_u32;
        let input: Vec<f32> = (0..(sr * 4.0) as usize).map(|_| 0.3 * white(&mut seed)).collect();
        let input = to_stereo(&input);
        let other: Vec<f32> = (0..(sr * 1.0) as usize * 2).map(|_| 0.5 * white(&mut seed)).collect();
        let fresh = feed(&mut Tape::new(sr), &input, &p);
        let mut used = Tape::new(sr);
        feed(&mut used, &other, &p);
        used.clear();
        let again = feed(&mut used, &input, &p);
        let twin = feed(&mut Tape::new(sr), &input, &p);
        let first_off = |a: &[f32], b: &[f32]| a.iter().zip(b).position(|(x, y)| x.to_bits() != y.to_bits());
        assert_eq!(first_off(&fresh, &again), None, "a cleared unit played something else");
        assert_eq!(first_off(&fresh, &twin), None, "two fresh units played different tapes");
    }

    /// **A tape track sounds the same at any buffer size.**
    ///
    /// Everything in the machine runs on frames counted since `new`: the
    /// designs on their sub-block grid, the wear on its own, every glide and
    /// every draw per frame. So the same two seconds with everything engaged
    /// at 48 kHz come out bit for bit alike through blocks of 7, 64, 333 and
    /// 512 frames. Held still, though, the voicing is designed once and
    /// never again, which cannot tell a design grid counted from `new` from
    /// one counted from each block's start. So the two seconds play again
    /// with the controls the voicing is designed from moving every 4096
    /// frames (the speed through all four, the range between the heads, the
    /// bias across aligned, Age, Damage, Bump and Roll), each block cut at
    /// the moves so every block size meets each one on the same frame:
    /// blocks of 1, 7, 31, 32, 64, 333, 512 and 1000 frames play what blocks
    /// of 4096 do, bit for bit. With the designs counted from each block's
    /// start instead, the still leg passes and this one does not: every
    /// size that is not a whole number of sub-blocks parts from 4096's at
    /// its first block after a move that starts off the grid, frame 4097 in
    /// blocks of 1, 4429 in blocks of 333 and 5096 in blocks of 1000.
    #[test]
    fn a_tape_track_sounds_the_same_at_any_buffer_size() {
        let sr = 48_000.0_f32;
        let p = everything_engaged();
        let mut seed = 0xB10C_5EED_u32;
        let input: Vec<f32> = (0..(sr * 2.0) as usize).map(|_| 0.3 * white(&mut seed)).collect();
        let input = to_stereo(&input);
        let through = |block: usize| {
            let mut unit = Tape::new(sr);
            let mut out = input.clone();
            let total = out.len() / 2;
            let mut done = 0;
            while done < total {
                let n = (total - done).min(block);
                unit.process(&mut out[done * 2..(done + n) * 2], n, 2, &p);
                done += n;
            }
            out
        };
        let at_512 = through(512);
        for block in [7, 64, 333] {
            let other = through(block);
            let off = at_512.iter().zip(&other).position(|(x, y)| x.to_bits() != y.to_bits());
            assert_eq!(off, None, "blocks of {block} frames played another tape");
        }

        // Moving: a new setting every 4096 frames, which no block straddles.
        let moved = |k: usize| TapeParams {
            speed: (k % 4) as f32 / 3.0,
            wow_range: [0.0, 1.0, 1.0, 0.0, 1.0][k % 5],
            bias: [0.5, 0.0, 1.0, 0.3][k % 4],
            age: [0.0, 1.0, 0.5][k % 3],
            damage: [0.0, 1.0][k % 2],
            bump: [0.5, 1.0, 0.0][k % 3],
            roll: [0.0, 0.4, 0.8][k % 3],
            ..everything_engaged()
        };
        let automated = |block: usize| {
            let mut unit = Tape::new(sr);
            let mut out = input.clone();
            let total = out.len() / 2;
            let mut done = 0;
            while done < total {
                let next = (done / 4096 + 1) * 4096;
                let n = block.min(total - done).min(next - done);
                unit.process(&mut out[done * 2..(done + n) * 2], n, 2, &moved(done / 4096));
                done += n;
            }
            out
        };
        let at_4096 = automated(4096);
        for block in [1, 7, 31, 32, 64, 333, 512, 1000] {
            let other = automated(block);
            let off = at_4096.iter().zip(&other).position(|(x, y)| x.to_bits() != y.to_bits());
            assert_eq!(
                off.map(|i| i / 2),
                None,
                "with the controls moving, blocks of {block} frames played another tape from that frame on"
            );
        }
    }

    /// **The tape costs what its budget says.** By hand, in release, never
    /// during a listening test:
    ///
    /// ```text
    /// cargo test --release -p tangent --bin tangent -- the_tape_costs --ignored --nocapture
    /// ```
    ///
    /// One unit, 128-frame blocks at 48 kHz for 10 s of noise, timed block
    /// by block: at the defaults, and fully engaged (Age, Damage and Hiss
    /// at the top, the deep head, the speed thrown between 15 and 3.75 ips
    /// every half second, the range every second, Rate and Bias swept end to
    /// end). Prints the mean, p99 and the worst block, and holds the mean at
    /// the defaults to 26 µs and the p99 fully engaged to 44 µs.
    ///
    /// **Those bars are the measured machine with a margin, not the budget
    /// the design set (21 and 24 µs), and that is an open issue.** On a
    /// quiet machine, warmed, the defaults measure 20.7 to 20.8 µs and the
    /// p99 fully engaged 34.7 to 35.8 µs, against the design's 21 and 24; on
    /// the sixteen-lane `rt_bench` row 5.0's tape cost 23.2 µs an insert
    /// against this one's 29.4. The record stage is most of it (a profile
    /// with it switched off ran at about a fifth): the polyphase pair does
    /// under half the
    /// multiply-adds and still costs what the direct form did, so the code
    /// the compiler makes of that stage is the first lead. The owner shipped
    /// the cost honestly in 5.2 and wants it optimized later;
    /// `docs/PERF-TAPE-5.2.md` has the numbers. Lower the bars as it gets
    /// cheaper, never raise them.
    #[test]
    #[ignore = "a bench: run it by hand in release with --ignored --nocapture"]
    fn the_tape_costs_what_its_budget_says() {
        let sr = 48_000.0_f32;
        let block = 128;
        let blocks = (10.0 * sr) as usize / block;
        let mut seed = 0xBE4C_0001_u32;
        let input: Vec<f32> = (0..block * 2).map(|_| 0.25 * white(&mut seed)).collect();
        let time = |what: &str, at: &dyn Fn(usize) -> TapeParams| {
            // A second untimed first: the first blocks a fresh binary runs
            // read about 5 us dearer (pages and caches still cold), which is
            // the machine starting, not the tape.
            let mut warm = Tape::new(sr);
            for k in 0..blocks / 10 {
                let mut buf = input.clone();
                warm.process(&mut buf, block, 2, &at(k));
                std::hint::black_box(&buf);
            }
            let mut unit = Tape::new(sr);
            let mut us = Vec::with_capacity(blocks);
            for k in 0..blocks {
                let p = at(k);
                let mut buf = input.clone();
                let start = std::time::Instant::now();
                unit.process(&mut buf, block, 2, &p);
                us.push(start.elapsed().as_secs_f64() * 1.0e6);
                std::hint::black_box(&buf);
            }
            let mean = us.iter().sum::<f64>() / us.len() as f64;
            us.sort_by(f64::total_cmp);
            let p99 = us[us.len() * 99 / 100];
            let max = us[us.len() - 1];
            println!("tape, {what}: mean {mean:.2} us, p99 {p99:.2} us, max {max:.2} us per {block}-frame block");
            (mean, p99)
        };
        let (mean, _) = time("defaults", &|_| TapeParams::default());
        let per_second = (sr as usize / block) as f64;
        let (_, p99) = time("fully engaged", &|k| {
            let t = k as f64 / per_second;
            let sweep = (1.0 - (t / 5.0 - 1.0).abs()) as f32;
            TapeParams {
                hiss: 1.0,
                age: 1.0,
                damage: 1.0,
                wobble: 1.0,
                flutter: 1.0,
                speed: if (t * 2.0) as usize % 2 == 0 { TapeSpeed::Ips15.dial() } else { TapeSpeed::Ips3_75.dial() },
                wow_range: if t as usize % 2 == 0 { WowRange::Deep.dial() } else { WowRange::Studio.dial() },
                rate: sweep,
                bias: sweep,
                ..TapeParams::default()
            }
        });
        assert!(mean <= 26.0, "a Tape at its defaults costs {mean:.2} us a block, over its 26 us ceiling");
        assert!(p99 <= 44.0, "a Tape fully engaged costs {p99:.2} us at p99, over its 44 us");
    }

    /// **The transport reads a studio machine's weighted flutter at every
    /// speed.**
    ///
    /// A fresh tape is a servo-capstan two-track inside its published spec,
    /// and the spec is a DIN 45507 / IEC 386 meter's weighted 2-sigma
    /// peak: the per-speed medians of published machines are 0.10, 0.06,
    /// 0.04 and 0.03 % at 3.75, 7.5, 15 and 30 ips. So the head's own sway
    /// over 30 s at 44.1 kHz, through that meter, reads within 15 % of the
    /// model of the same parts (0.098, 0.061, 0.040 and 0.028 %); its
    /// unweighted 2-sigma (the meter's 200 Hz demodulator, no weighting)
    /// stands 1.25 to 2.7 times above it, the spread published machines
    /// show between the two standards; and the figure the face prints for
    /// it ([`wow_flutter_weighted_pct`]) is within 15 % of what the meter
    /// reads, there, at 15 ips with Wow and Flutter at 1, where the governor
    /// has begun to share the head, and at 3.75 ips with them at 1 and Rate
    /// at its bottom, where it has shed most of the head's parts. Measured:
    /// 0.0967, 0.0595, 0.0415 and 0.0279 %, unweighted 1.30, 1.30, 1.65 and
    /// 1.42 times that, the face 3.5, 0.8, 3.7 and 7.5 % off; 0.4 % off at
    /// full Wow and Flutter at 15 ips (0.2959 % measured) and 5.3 % in the
    /// governed corner (0.3470 %).
    #[test]
    fn the_transport_reads_a_studio_machines_weighted_flutter_at_every_speed() {
        let sr = 44_100.0_f32;
        let (frames, skip) = ((sr * 30.0) as usize, (sr * 5.0) as usize);
        let face_agrees = |p: &TapeParams, read: f64, what: &str| {
            let told = f64::from(wow_flutter_weighted_pct(p));
            assert!(
                (told / read - 1.0).abs() < 0.15,
                "{what}: the face is told {told:.4} % weighted, and a meter reads {read:.4} %"
            );
        };
        for (speed, band) in TapeSpeed::ALL.into_iter().zip([
            0.085..=0.115,
            0.051..=0.069,
            0.034..=0.046,
            0.024..=0.033,
        ]) {
            let p = TapeParams {
                speed: speed.dial(),
                ..TapeParams::default()
            };
            let sway = Tape::new(sr).transport_trace(&p, frames);
            let weighted = flutter_2sigma(&sway, sr.into(), skip, true);
            let unweighted = flutter_2sigma(&sway, sr.into(), skip, false);
            assert!(
                band.contains(&weighted),
                "a fresh tape at {} reads {weighted:.4} % weighted, outside {band:?}",
                speed.label()
            );
            assert!(
                (1.25..=2.7).contains(&(unweighted / weighted)),
                "a fresh tape at {} reads {unweighted:.4} % unweighted against {weighted:.4} % \
                 weighted, {:.2} times",
                speed.label(),
                unweighted / weighted
            );
            face_agrees(&p, weighted, speed.label());
        }
        for (what, p) in [
            (
                "Wow and Flutter 1 at 15 ips",
                TapeParams {
                    wobble: 1.0,
                    flutter: 1.0,
                    ..TapeParams::default()
                },
            ),
            (
                "Wow and Flutter 1 at 3.75 ips and the bottom of Rate",
                TapeParams {
                    wobble: 1.0,
                    flutter: 1.0,
                    rate: 0.0,
                    speed: TapeSpeed::Ips3_75.dial(),
                    ..TapeParams::default()
                },
            ),
        ] {
            let sway = Tape::new(sr).transport_trace(&p, frames);
            face_agrees(&p, flutter_2sigma(&sway, sr.into(), skip, true), what);
        }
    }

    /// **The weighted flutter survives the audio path.** The meter is read
    /// off a tone, as a real one is: a 3150 Hz tone at -12 dBFS through a
    /// fresh tape at 15 ips, Drive and Hiss at 0, for 8 s, its frequency
    /// period by period from sub-sample zero crossings, through the same
    /// weighting and detector, reads the 0.040 % the transport is
    /// calibrated to, within 35 % (the per-period reading is a sample of
    /// the speed 3150 times a second, and the scrape's kilohertz grain
    /// folds a little into it). Measured: 0.0411 %.
    #[test]
    fn weighted_flutter_survives_the_audio_path() {
        let sr = 44_100.0_f32;
        let frames = (sr * 8.0) as usize;
        let amp = 10.0_f64.powf(-12.0 / 20.0);
        let mono: Vec<f32> = (0..frames)
            .map(|i| (amp * (std::f64::consts::TAU * 3_150.0 * i as f64 / f64::from(sr)).sin()) as f32)
            .collect();
        let p = TapeParams {
            drive: 0.0,
            hiss: 0.0,
            ..TapeParams::default()
        };
        let out = left(&feed(&mut Tape::new(sr), &to_stereo(&mono), &p));
        let crossings: Vec<f64> = (1..out.len())
            .filter(|&i| out[i - 1] < 0.0 && out[i] >= 0.0)
            .map(|i| (i - 1) as f64 + f64::from(out[i - 1]) / f64::from(out[i - 1] - out[i]))
            .collect();
        let nominal = f64::from(sr) / 3_150.0;
        let dev: Vec<f64> = crossings.windows(2).map(|w| nominal / (w[1] - w[0]) - 1.0).collect();
        let read = meter_2sigma(&dev, 3_150.0, (3_150.0 * 3.0) as usize, true);
        assert!(
            (read / 0.040 - 1.0).abs() < 0.35,
            "a 3150 Hz tone through a fresh tape reads {read:.4} % weighted, not 0.040 %"
        );
    }

    /// `f(0..count)` on three hands, the answers in order: the meters below
    /// are independent traces, and three hands keep a gate that runs this
    /// binary beside other work moving.
    fn on_hands<T: Send>(count: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
        let next = std::sync::atomic::AtomicUsize::new(0);
        let mut got: Vec<(usize, T)> = std::thread::scope(|sc| {
            let hands: Vec<_> = (0..3)
                .map(|_| {
                    sc.spawn(|| {
                        let mut mine = Vec::new();
                        loop {
                            let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            if i >= count {
                                break;
                            }
                            mine.push((i, f(i)));
                        }
                        mine
                    })
                })
                .collect();
            hands.into_iter().flat_map(|h| h.join().unwrap_or_else(|p| std::panic::resume_unwind(p))).collect()
        });
        got.sort_by_key(|(i, _)| *i);
        got.into_iter().map(|(_, t)| t).collect()
    }

    /// **The face reads the meter where Rate slows the wow.**
    ///
    /// A part's depth breathes with its wander, and an excursion that
    /// breathes bends the pitch on its own, in a burst after each of the
    /// wander's steps. Where Rate slows the wow's parts under the
    /// weighting's 0.45 Hz corner, their turns barely reach the meter but
    /// those bursts do: at 3.75 ips with Rate at its bottom, Wow 0.3 and
    /// Flutter 0 the breathing carries four fifths of the power the meter
    /// hears, and 5.2.0's figure, which summed the turns alone, read 73 %
    /// under. So the figure the face prints is held within 15 % of the
    /// meter (the weighted 2-sigma on the head's own sway at 44.1 kHz,
    /// after 8 s of settling) where Rate and Flutter most change what it
    /// reads: the slow
    /// corners of the studio head with Flutter at 0, where its breathing is
    /// most of the sway; a lone part turning at the top of Rate, where one
    /// sine reads lowest over its RMS; full Flutter at 30 ips, where the
    /// figure reads furthest over; and the deep head at the bottom of Rate,
    /// where the cassette's breath is a sixth of the power. Each reads 40 s
    /// (the deep head 120 s), except the slowest corner, whose reel turns
    /// once in 12.5 s: over the nineteen 32 s windows of one ten-minute
    /// trace the meter itself reads 0.018 to 0.029 % there, a spread no
    /// figure can stand within 15 % of, and over the eight 152 s windows of
    /// 21 minutes 0.020 to 0.025 %, so it reads 160 s. Measured, in that
    /// order: -0.3, +4.3, -3.7, -7.3, -9.3,
    /// +1.5, -2.0, +11.4 and -5.7 %, where the turns alone, as 5.2.0's
    /// figure summed them, read -73, -44, -40, -37, -17, -24, +14 and +11 %
    /// on the studio head, which sounds as it did in 5.2.0, and -14 % on the
    /// deep head as it swings now.
    #[test]
    fn the_face_reads_the_meter_where_rate_slows_the_wow() {
        let sr = 44_100.0_f32;
        let at = |speed: TapeSpeed, rate: f32, wobble: f32, flutter: f32| TapeParams {
            speed: speed.dial(),
            rate,
            wobble,
            flutter,
            ..TapeParams::default()
        };
        let cases = [
            ("3.75 ips, Rate 0, Wow 0.3, Flutter 0", at(TapeSpeed::Ips3_75, 0.0, 0.3, 0.0), 160.0),
            ("3.75 ips, Rate 0, Wow 1, Flutter 0", at(TapeSpeed::Ips3_75, 0.0, 1.0, 0.0), 40.0),
            ("3.75 ips, Rate 0.25, Wow 0.3, Flutter 0", at(TapeSpeed::Ips3_75, 0.25, 0.3, 0.0), 40.0),
            ("7.5 ips, Rate 0, Wow 0.3, Flutter 0", at(TapeSpeed::Ips7_5, 0.0, 0.3, 0.0), 40.0),
            ("7.5 ips, Rate 0, Wow 1, Flutter 0", at(TapeSpeed::Ips7_5, 0.0, 1.0, 0.0), 40.0),
            ("15 ips, Rate 0, Wow 0.3, Flutter 0", at(TapeSpeed::Ips15, 0.0, 0.3, 0.0), 40.0),
            ("3.75 ips, Rate 1, Wow 1, Flutter 0", at(TapeSpeed::Ips3_75, 1.0, 1.0, 0.0), 40.0),
            ("30 ips, Rate 0, Wow 0.3, Flutter 1", at(TapeSpeed::Ips30, 0.0, 0.3, 1.0), 40.0),
            (
                "the deep head at 7.5 ips, Rate 0, Wow 1",
                TapeParams {
                    wow_range: WowRange::Deep.dial(),
                    ..at(TapeSpeed::Ips7_5, 0.0, 1.0, 0.3)
                },
                120.0,
            ),
        ];
        let read = on_hands(cases.len(), |i| {
            let (_, p, secs) = &cases[i];
            let sway = Tape::new(sr).transport_trace(p, (sr * secs) as usize);
            flutter_2sigma(&sway, sr.into(), (sr * 8.0) as usize, true)
        });
        for ((what, p, _), read) in cases.iter().zip(read) {
            let told = f64::from(wow_flutter_weighted_pct(p));
            assert!(
                (told / read - 1.0).abs() < 0.15,
                "{what}: the face is told {told:.4} % weighted, and a meter reads {read:.4} %"
            );
        }
    }

    /// **The face reads the meter across the whole grid**: every speed, both
    /// heads, Rate in quarters, Wow 0.3 and 1, Flutter 0, 0.3 and 1, 240
    /// settings at 44.1 kHz, each read by the meter over 40 s (the deep head
    /// 120 s) and over 640 s, after 8 s of settling. By hand, in release:
    ///
    /// ```text
    /// cargo test --release -p tangent --bin tangent -- the_face_reads_the_meter_across --ignored --nocapture
    /// ```
    ///
    /// Prints every setting and holds the face within 15 % of the 640 s
    /// meter, which is what the machine reads. Measured: within 10.9 % of
    /// it everywhere, 3.6 % on average; within 11.4 % of the 40 s meter
    /// everywhere but the one corner the short window cannot read (3.75 ips,
    /// Rate 0, Wow 0.3, Flutter 0: its first 32 s read 0.0294 %, its ten
    /// minutes 0.0236 %, and the face 0.0228 %), and the same at 48 and
    /// 96 kHz. The turns alone, as 5.2.0's figure summed them, missed the
    /// 40 s meter by more than 15 % at 7 of the 240, every one with Flutter
    /// at 0 on the studio head (which sounds as it did in 5.2.0), and read
    /// 79 % under at that corner.
    #[test]
    #[ignore = "slow: run it by hand in release with --ignored --nocapture"]
    fn the_face_reads_the_meter_across_the_whole_grid() {
        let sr = 44_100.0_f32;
        let mut grid = Vec::new();
        for speed in TapeSpeed::ALL {
            for range in WowRange::ALL {
                for rate in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
                    for wobble in [0.3_f32, 1.0] {
                        for flutter in [0.0_f32, 0.3, 1.0] {
                            grid.push(TapeParams {
                                speed: speed.dial(),
                                wow_range: range.dial(),
                                rate,
                                wobble,
                                flutter,
                                ..TapeParams::default()
                            });
                        }
                    }
                }
            }
        }
        let read = on_hands(grid.len(), |i| {
            let p = &grid[i];
            let short = if WowRange::from_dial(p.wow_range) == WowRange::Deep { 120.0 } else { 40.0 };
            let sway = Tape::new(sr).transport_trace(p, (sr * 648.0) as usize);
            let skip = (sr * 8.0) as usize;
            (
                flutter_2sigma(&sway[..(sr * short) as usize], sr.into(), skip, true),
                flutter_2sigma(&sway, sr.into(), skip, true),
            )
        });
        let mut worst = 0.0_f64;
        for (p, (short, long)) in grid.iter().zip(read) {
            let told = f64::from(wow_flutter_weighted_pct(p));
            let off = told / long - 1.0;
            worst = worst.max(off.abs());
            println!(
                "{} {} Rate {} Wow {} Flutter {}: face {told:.5} %, meter {long:.5} % over 640 s ({:+.1} %), {short:.5} % over the short window ({:+.1} %)",
                TapeSpeed::from_dial(p.speed).label(),
                WowRange::from_dial(p.wow_range).label(),
                p.rate,
                p.wobble,
                p.flutter,
                100.0 * off,
                100.0 * (told / short - 1.0)
            );
            assert!(off.abs() < 0.15, "the face is told {told:.5} % and ten minutes of the meter read {long:.5} %");
        }
        println!("worst against 640 s: {:.1} %", 100.0 * worst);
    }

    /// **The play head never runs off its tape at any setting.**
    ///
    /// (a) Every speed, on either head, at the ends and middle of Rate, with
    /// Wow, Flutter, Hiss, Age and Damage at their tops and the bias at its
    /// bottom, whose coarser grain asks the most of the modulation noise,
    /// whose aged head skews the two tracks apart and whose creases twist it
    /// further, ten seconds of the transport
    /// at 44.1 kHz each: no head is ever held inside the line
    /// (the safety net catches nothing), and no track's head, its sway and
    /// its own modulation noise together, ever sits further from its rest
    /// than that head's travel. Where the parts ask for more than that, the governor shares
    /// it; nothing is clipped. (b) And the switch between the heads, which is
    /// where a cassette's excursion could land on a studio head: the range
    /// thrown every half second while Rate and Speed jump every second, from
    /// the bottom of Rate at full Wow, starting at every speed, at 44.1, 48
    /// and 96 kHz, five seconds each, under the same two bars. The cassette
    /// stays on the deep head until that head has faded out, and starts
    /// from nothing when it fades back in. Measured at worst: 32.2 of the
    /// studio head's 39.7 samples (15 ips, Rate at its bottom) and 719 of
    /// the deep head's 970 (7.5 ips, Rate at its bottom) at 44.1 kHz;
    /// thrown, 35.4 of 43.2 and 422.4 of 2112 at 48 and 96 kHz.
    #[test]
    fn the_play_head_never_runs_off_its_tape_at_any_setting() {
        let held = |unit: &Tape, what: &str| {
            let g = unit.governed();
            assert_eq!(unit.clamp_hits(), 0, "{what}: a head was held inside the line");
            for (h, travel) in [(0, g.travel), (1, g.deep_travel)] {
                assert!(
                    f64::from(unit.deepest()[h]) <= travel + 1.0e-3,
                    "{what}: the {} head sat {:.3} samples from its rest, past its {travel:.3} \
                     of travel",
                    ["studio", "deep"][h],
                    unit.deepest()[h]
                );
            }
        };
        let sr = 44_100.0_f32;
        for speed in TapeSpeed::ALL {
            for range in WowRange::ALL {
                for rate in [0.0, 0.5, 1.0] {
                    let p = TapeParams {
                        wobble: 1.0,
                        flutter: 1.0,
                        hiss: 1.0,
                        rate,
                        speed: speed.dial(),
                        wow_range: range.dial(),
                        bias: 0.0,
                        age: 1.0,
                        damage: 1.0,
                        ..TapeParams::default()
                    };
                    let mut unit = Tape::new(sr);
                    unit.transport_trace(&p, (sr * 10.0) as usize);
                    held(&unit, &format!("{} on the {} head at Rate {rate}", speed.label(), range.label()));
                }
            }
        }
        for sr in [44_100.0_f32, 48_000.0, 96_000.0] {
            for (k, start) in TapeSpeed::ALL.into_iter().enumerate() {
                let mut unit = Tape::new(sr);
                let half = (sr * 0.5) as usize;
                for step in 0..10 {
                    let second = step / 2;
                    let p = TapeParams {
                        wobble: 1.0,
                        rate: (second % 2) as f32,
                        speed: TapeSpeed::ALL[(k + second) % 4].dial(),
                        wow_range: (step % 2) as f32,
                        ..TapeParams::default()
                    };
                    unit.transport_trace(&p, half);
                }
                held(&unit, &format!("the range thrown from {} at {sr} Hz", start.label()));
            }
        }
    }

    /// **The head is shared when the tape asks for more than it holds.**
    ///
    /// At 3.75 ips and the bottom of Rate, Wow and Flutter at their tops ask
    /// for about four and a half milliseconds of sway, and the studio head
    /// has under one. The governor shares what it has in proportion to what
    /// Wow's parts and Flutter's asked for, and inside each the slowest part
    /// gives way first: the drift and the reel are shed whole before the
    /// pinch roller is touched, and the guides before the capstan; the
    /// tape's 44 Hz resonance and the scrape are never shed. So over 20 s at
    /// 44.1 kHz the head swings to between 70 % and all of the room it has
    /// and never past it, the safety net catches nothing, and Wow stays a
    /// live dial there: the wow the head carries at Wow 1 is at least 1.5
    /// times what it carries at Wow 0.3, with Flutter at 1 and at 0.3 (model
    /// 2.2 and 2.3 times). And a fresh tape asks for nothing to be shed at
    /// any speed. Measured: 79 % of the room, and 2.18 and 2.27 times.
    #[test]
    fn the_head_is_shared_when_the_tape_asks_for_more_than_it_holds() {
        let sr = 44_100.0_f32;
        let corner = |wobble: f32, flutter: f32| TapeParams {
            wobble,
            flutter,
            rate: 0.0,
            speed: TapeSpeed::Ips3_75.dial(),
            ..TapeParams::default()
        };
        let mut unit = Tape::new(sr);
        let sway = unit.transport_trace(&corner(1.0, 1.0), (sr * 20.0) as usize);
        let g = unit.governed();
        let peak = sway.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        assert!(
            (0.70..=1.0).contains(&(peak / g.room)),
            "the governed head swung {peak:.2} samples with {:.2} of room",
            g.room
        );
        assert_eq!(unit.clamp_hits(), 0, "the governed head was held inside the line");
        let name = ["drift", "reel", "pinch roller", "guides", "capstan", "resonance", "scrape"];
        for j in [DRIFT, REEL, GUIDES] {
            assert_eq!(g.given[j], 0.0, "the {} was kept ({:.3} samples) before a faster part gave way", name[j], g.given[j]);
        }
        for j in [PINCH, CAPSTAN] {
            assert!(
                g.given[j] > 0.0 && g.given[j] < g.asked[j],
                "the {} kept {:.3} of the {:.3} samples it asked for",
                name[j],
                g.given[j],
                g.asked[j]
            );
        }
        for j in [RESONANCE, SCRAPE] {
            assert_eq!(g.given[j], g.asked[j], "the {} was shed", name[j]);
        }
        let wow_carried = |wobble: f32, flutter: f32| {
            let mut unit = Tape::new(sr);
            unit.transport_trace(&corner(wobble, flutter), 64);
            let g = unit.governed();
            [REEL, PINCH].iter().map(|&j| g.given[j] * g.omega[j - REEL]).sum::<f64>()
        };
        for flutter in [1.0, 0.3] {
            let (top, spec) = (wow_carried(1.0, flutter), wow_carried(0.3, flutter));
            assert!(
                spec > 0.0 && top >= 1.5 * spec,
                "with Flutter at {flutter} the head carries {:.4} % of wow at Wow 1 and {:.4} % \
                 at Wow 0.3: the dial went dead",
                top * 100.0,
                spec * 100.0
            );
        }
        for speed in TapeSpeed::ALL {
            let mut unit = Tape::new(sr);
            unit.transport_trace(&TapeParams { speed: speed.dial(), ..TapeParams::default() }, 64);
            let g = unit.governed();
            assert_eq!(g.given, g.asked, "a fresh tape at {} is governed", speed.label());
        }
    }

    /// **A Rate or Speed change never bends the pitch past the setting's own
    /// depth.**
    ///
    /// A rotating part's sway is its excursion times its turn, so a part
    /// that sped up before its excursion had shrunk would bend the pitch
    /// past anything either setting does: a pinch roller's wow at 3.75 ips
    /// carried into 30 ips eight times faster. A part's turn may only rise as
    /// fast as its excursion shrinks, and the head's parts may change their
    /// excursions by 0.3 % of the speed between them, so through any change
    /// the largest speed deviation (through the meter's 200 Hz demodulator)
    /// never exceeds the larger of the two settings' own largest, over 20 s
    /// of each, by more than that 0.3 %: Wow 1 from 3.75 to 30 ips and back,
    /// and Wow and Flutter thrown from 0 to 1 at once at 15 ips. Measured:
    /// 0.231 and 0.292 % through the speed changes against their settings'
    /// 0.310, and 0.841 through the jump, which is the largest of the
    /// setting it settles into; with a part's turn free to rise at once the
    /// first reads 0.772.
    #[test]
    fn a_rate_or_speed_change_never_bends_the_pitch_past_the_settings_own_depth() {
        let sr = 44_100.0_f32;
        let srf = f64::from(sr);
        let steady = |p: &TapeParams| {
            let mut unit = Tape::new(sr);
            let sway = unit.transport_trace(p, (sr * 20.0) as usize);
            flutter_peak(&sway, srf, (sr * 0.5) as usize)
        };
        let at = |wobble: f32, flutter: f32, speed: TapeSpeed| TapeParams {
            wobble,
            flutter,
            speed: speed.dial(),
            ..TapeParams::default()
        };
        for (what, from, to, slew) in [
            ("Wow 1, 3.75 to 30 ips", at(1.0, 0.3, TapeSpeed::Ips3_75), at(1.0, 0.3, TapeSpeed::Ips30), 0.3),
            ("Wow 1, 30 to 3.75 ips", at(1.0, 0.3, TapeSpeed::Ips30), at(1.0, 0.3, TapeSpeed::Ips3_75), 0.3),
            ("Wow and Flutter 0 to 1 at 15 ips", at(0.0, 0.0, TapeSpeed::Ips15), at(1.0, 1.0, TapeSpeed::Ips15), 0.3),
        ] {
            let mut unit = Tape::new(sr);
            let mut sway = unit.transport_trace(&from, (sr * 3.0) as usize);
            let switch = sway.len();
            sway.extend(unit.transport_trace(&to, (sr * 4.0) as usize));
            let moved = demodulated(&sway, srf)[switch - 1..]
                .iter()
                .fold(0.0_f64, |m, v| m.max(v.abs()));
            let bound = steady(&from).max(steady(&to));
            assert!(
                moved <= bound + slew,
                "{what}: the pitch bent {moved:.3} % through the change, past the settings' own \
                 {bound:.3} % and the head's {slew} % of slew"
            );
        }

        // The deep head: Rate from its bottom to its top at full Wow, under
        // the deep head's own 3 % of slew; and the cassette's pack alone,
        // whose excursion is the largest any part carries, never bends the
        // pitch past 1.2 times its own depth, `CASSETTE_DEPTH` of Wow's
        // dial times its share (model 1.11). Measured: 2.70 % against the
        // settings' 2.82 (3.25 against 2.87 while the belt's drift followed
        // Rate), and the pack 0.96 times its depth.
        let deep = |rate: f32| TapeParams {
            wobble: 1.0,
            rate,
            wow_range: WowRange::Deep.dial(),
            ..TapeParams::default()
        };
        let mut unit = Tape::new(sr);
        let mut sway = unit.transport_trace(&deep(0.0), (sr * 6.0) as usize);
        let switch = sway.len();
        sway.extend(unit.transport_trace(&deep(1.0), (sr * 6.0) as usize));
        let moved = demodulated(&sway, srf)[switch - 1..]
            .iter()
            .fold(0.0_f64, |m, v| m.max(v.abs()));
        let bound = steady(&deep(0.0)).max(steady(&deep(1.0)));
        assert!(
            moved <= bound + 3.0,
            "the deep head through Rate 0 to 1: the pitch bent {moved:.3} % past the settings' own \
             {bound:.3} % and the deep head's 3 % of slew"
        );
        let mut unit = Tape::new(sr);
        let mut pack = unit.pack_trace(&deep(0.0), (sr * 6.0) as usize);
        pack.extend(unit.pack_trace(&deep(1.0), (sr * 6.0) as usize));
        let own = 100.0 * CASSETTE_DEPTH * CASSETTE_SHARE[0];
        let worst = flutter_peak(&pack, srf, (sr * 0.5) as usize);
        assert!(
            worst <= 1.2 * own,
            "the cassette's pack bent the pitch {worst:.3} %, past 1.2 times its own {own:.3} %"
        );
    }


    /// **The deep range swings the pitch like a worn cassette.**
    ///
    /// The deep head carries a cassette's warble, 3 % of the speed at its
    /// peak at full Wow and linear in Wow's dial (the owner's ruling). So at
    /// Rate as built, 120 s of its sway at 44.1 kHz peaks through the
    /// meter's demodulator within 5 % of 3 % times Wow: 2.85 to 3.15 % at
    /// full Wow, 1.425 to 1.575 % at 0.5, and 0.855 to 0.945 % at the
    /// default 0.3, a plainly cassette-like swing already. 5.2.0's deep head
    /// peaked at 2.615, 1.618 and 0.981 %, outside all three, because the
    /// wander's own movement on the pack's long excursion bent the pitch
    /// further than the pack's turn did, until the deep slew held it back
    /// ([`CASSETTE_BREATH`]). It reads a third of that weighted, within 15 %
    /// of 1 % times Wow, and the figure the face prints is within 15 % of
    /// the meter at all three. And the pack alone, at the default Wow, peaks
    /// within 5 % of its own depth: its breathing may bend the pitch by at
    /// most a quarter of that depth, and does so across its turn, so it
    /// adds about 3 % at most, while the breathing still carries the pack
    /// to the top of its depth (5.2.0's took it to 1.6 times). The swing
    /// holds at Rate's ends too, half the parts' speed and twice it: at
    /// Wow 1 and 0.5 it peaks within the same 5 % of 3 % times Wow, because
    /// the belt's drift ignores Rate ([`CASSETTE_HZ`]); while the drift
    /// followed Rate, twice the speed peaked 7 and 9 % over. With Wow and
    /// Flutter at 0 the deep head does not move at all. The cassette
    /// ignores the tape speed: its pack turns at 0.45 Hz within 5 % at
    /// 3.75 and at 30 ips. And nothing is ever held inside the line.
    /// Measured: peaks of 3.011, 1.508 and 0.911 %, 1.002, 0.500 and
    /// 0.301 % weighted, the face 1.8 to 2.2 % under the meter; the pack
    /// alone 1.026 times its depth; at Rate's ends 3.068 and 1.555 % at
    /// half speed, 3.015 and 1.530 % at twice (3.216 and 1.630 % while the
    /// drift followed Rate); the pack's line at 0.4485 Hz at both speeds.
    #[test]
    fn the_deep_range_swings_the_pitch_like_a_worn_cassette() {
        let sr = 44_100.0_f32;
        let srf = f64::from(sr);
        let skip = (sr * 10.0) as usize;
        let deep = |wobble: f32| TapeParams {
            wobble,
            wow_range: WowRange::Deep.dial(),
            ..TapeParams::default()
        };
        let wow = TapeParams::default().wobble;
        let own = 100.0 * CASSETTE_DEPTH * CASSETTE_SHARE[0] * f64::from(wow);
        let pack = flutter_peak(&Tape::new(sr).pack_trace(&deep(wow), (sr * 120.0) as usize), srf, skip);
        assert!(
            (pack / own - 1.0).abs() <= 0.05,
            "the cassette's pack alone peaks at {pack:.3} % at the default Wow, not within 5 % of its \
             own {own:.3} %"
        );
        for wobble in [1.0_f32, 0.5, 0.3] {
            let p = deep(wobble);
            let mut unit = Tape::new(sr);
            let sway = unit.transport_trace(&p, (sr * 120.0) as usize);
            let peak = flutter_peak(&sway, srf, skip);
            let weighted = flutter_2sigma(&sway, srf, skip, true);
            let told = f64::from(wow_flutter_weighted_pct(&p));
            let ruled = 3.0 * f64::from(wobble);
            assert!(
                (peak / ruled - 1.0).abs() <= 0.05,
                "the deep head at Wow {wobble} peaks at {peak:.3} %, not within 5 % of the ruled \
                 {ruled:.2} %"
            );
            let third = ruled / 3.0;
            assert!(
                (weighted / third - 1.0).abs() <= 0.15,
                "the deep head at Wow {wobble} reads {weighted:.4} % weighted, not within 15 % of \
                 {third:.2} %"
            );
            assert!(
                (told / weighted - 1.0).abs() < 0.15,
                "the deep head at Wow {wobble}: the face is told {told:.4} % weighted, and a meter \
                 reads {weighted:.4} %"
            );
            assert_eq!(unit.clamp_hits(), 0, "the deep head at Wow {wobble} was held inside the line");
        }

        // Rate's ends, a hand each: the four traces are independent.
        let ends: Vec<(f32, f32)> = [0.0_f32, 1.0].into_iter().flat_map(|rate| [(rate, 1.0_f32), (rate, 0.5)]).collect();
        let peaks: Vec<(f64, u64)> = std::thread::scope(|sc| {
            let hands: Vec<_> = ends
                .iter()
                .map(|&(rate, wobble)| {
                    sc.spawn(move || {
                        let mut unit = Tape::new(sr);
                        let sway = unit.transport_trace(&TapeParams { rate, ..deep(wobble) }, (sr * 120.0) as usize);
                        (flutter_peak(&sway, srf, skip), unit.clamp_hits())
                    })
                })
                .collect();
            hands.into_iter().map(|h| h.join().unwrap_or_else(|p| std::panic::resume_unwind(p))).collect()
        });
        for (&(rate, wobble), &(peak, clamped)) in ends.iter().zip(&peaks) {
            let ruled = 3.0 * f64::from(wobble);
            let x = rate_multiplier(rate);
            assert!(
                (peak / ruled - 1.0).abs() <= 0.05,
                "the deep head at Wow {wobble} and Rate {x:.1}x peaks at {peak:.3} %, not within 5 % of \
                 the ruled {ruled:.2} %"
            );
            assert_eq!(clamped, 0, "the deep head at Wow {wobble} and Rate {x:.1}x was held inside the line");
        }

        let still = TapeParams {
            wobble: 0.0,
            flutter: 0.0,
            ..deep(0.0)
        };
        let sway = Tape::new(sr).transport_trace(&still, (sr * 5.0) as usize);
        assert!(sway.iter().all(|d| *d == 0.0), "the deep head moved with Wow and Flutter at 0");

        // The pack's line: the deep head's sway at full Wow with Flutter
        // still, averaged to a millisecond and scanned from 0.3 to 0.6 Hz.
        for speed in [TapeSpeed::Ips3_75, TapeSpeed::Ips30] {
            let p = TapeParams {
                flutter: 0.0,
                speed: speed.dial(),
                ..deep(1.0)
            };
            let sway = Tape::new(sr).transport_trace(&p, (sr * 40.0) as usize);
            let ms = (sr / 1_000.0) as usize;
            let slow: Vec<f64> = sway.chunks_exact(ms).map(|c| c.iter().sum::<f64>() / ms as f64).collect();
            // A Goertzel's power at `hz`, then a coarse scan and a fine one
            // around its best.
            let power = |hz: f64| {
                let coeff = 2.0 * (std::f64::consts::TAU * hz / 1_000.0).cos();
                let (mut s1, mut s2) = (0.0_f64, 0.0_f64);
                for x in &slow {
                    let s0 = x + coeff * s1 - s2;
                    s2 = s1;
                    s1 = s0;
                }
                s1 * s1 + s2 * s2 - coeff * s1 * s2
            };
            let best = |from: f64, step: f64, n: i32| {
                (0..=n)
                    .map(|k| from + step * f64::from(k))
                    .max_by(|a, b| power(*a).total_cmp(&power(*b)))
                    .expect("a line")
            };
            let coarse = best(0.3, 0.005, 60);
            let line = best(coarse - 0.005, 0.000_5, 20);
            assert!(
                (line / 0.45 - 1.0).abs() < 0.05,
                "at {} the cassette's pack turns at {line:.3} Hz, not 0.45",
                speed.label()
            );
        }
    }

    /// **A moving control never clicks.**
    ///
    /// A 1 kHz tone at -18 dBFS through a fresh tape, and each control
    /// thrown from one end of its travel to the other at a block boundary
    /// and back: Speed from 15 to 3.75 ips and back, the wow range from the
    /// studio head to the deep one and back, every other control 0 to 1 and
    /// 1 to 0. A click is energy the band above 12 kHz did not carry before
    /// or after, so the 20 ms after each throw may carry no more there than
    /// the louder of the two settings' own steady state, plus 6 dB. And the
    /// range switch fades the heads with equal power, two copies of the tone
    /// 23 ms apart, so its level never rises past the steady tone's by more
    /// than 3.2 dB. Bias's
    /// under-bias shelf and its inverse straddle the record stage's
    /// oversampler, so the inverse takes each design over 44 frames after
    /// the shelf does; designed together, each design while the bias glided
    /// was a 44-frame step in the bass, +9.3 dB above 12 kHz.
    /// Measured: at most +3.9 dB above 12 kHz, Bias thrown from its bottom
    /// to its top, its shelves redesigned a sub-block at a time through the
    /// glide, and +0.6 or under for every other throw; the heads' fade
    /// swells the tone +2.48 dB into the deep head, whose cassette starts
    /// from nothing so the two copies nearly agree, and not at all on the
    /// way back.
    #[test]
    fn a_moving_control_never_clicks() {
        let sr = 48_000.0_f32;
        let srf = f64::from(sr);
        let block = 480;
        let amp = 10.0_f64.powf(-18.0 / 20.0);
        let tone = |i: usize| (amp * (std::f64::consts::TAU * 1_000.0 * i as f64 / srf).sin()) as f32;
        let settle = (sr * 1.0) as usize;
        let after = (sr * 1.5) as usize;
        let window = (sr * 0.02) as usize;
        // The tone through a fresh tape at `from`, thrown to `to` on the
        // block at one second: the left channel.
        let thrown = |from: &TapeParams, to: &TapeParams| {
            let mut unit = Tape::new(sr);
            let total = settle + after;
            let mut heard = Vec::with_capacity(total);
            for b in 0..total / block {
                let mut buf: Vec<f32> = (b * block..(b + 1) * block).flat_map(|i| [tone(i), tone(i)]).collect();
                let p = if b * block < settle { from } else { to };
                unit.process(&mut buf, block, 2, p);
                heard.extend(buf.chunks_exact(2).map(|f| f[0]));
            }
            heard
        };
        // The power above 12 kHz, frame by frame, through an eighth-order
        // high-pass run over the whole render so it has settled by the
        // throw.
        let treble = |heard: &[f32]| {
            let mut hp = Butter::new(8, srf, 12_000.0, true);
            heard.iter().map(|v| hp.run(f64::from(*v)).powi(2)).collect::<Vec<f64>>()
        };
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let set = |k: usize, v: f32| {
            let mut p = TapeParams::default();
            match k {
                0 => p.drive = v,
                1 => p.bump = v,
                2 => p.roll = v,
                3 => p.wobble = v,
                4 => p.hiss = v,
                5 => p.trim = v,
                6 => p.speed = v,
                7 => p.flutter = v,
                8 => p.rate = v,
                9 => p.wow_range = v,
                10 => p.bias = v,
                11 => p.age = v,
                _ => p.damage = v,
            }
            p
        };
        let ends = |k: usize| match k {
            6 => (TapeSpeed::Ips15.dial(), TapeSpeed::Ips3_75.dial()),
            _ => (0.0, 1.0),
        };
        let names = ["drive", "bump", "roll", "wow", "hiss", "trim", "speed", "flutter", "rate", "wow range", "bias", "age", "damage"];
        for k in 0..names.len() {
            let (a, b) = ends(k);
            for (x, y) in [(a, b), (b, a)] {
                let heard = thrown(&set(k, x), &set(k, y));
                let hf = treble(&heard);
                let before = mean(&hf[settle - window..settle]);
                let steady = mean(&hf[heard.len() - window..]);
                let at = mean(&hf[settle..settle + window]);
                let rise = 10.0 * (at / before.max(steady).max(1.0e-30)).log10();
                assert!(
                    rise <= 6.0,
                    "{} thrown from {x} to {y}: the 20 ms after carried {rise:+.1} dB above 12 kHz \
                     over the louder steady state",
                    names[k]
                );
                if k == 9 {
                    let rms = |v: &[f32]| (v.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
                    let steady = rms(&heard[settle - 4 * window..settle]).max(rms(&heard[heard.len() - 4 * window..]));
                    let ms = (sr / 1_000.0) as usize;
                    let worst = heard[settle..settle + 2 * window]
                        .chunks_exact(ms)
                        .map(|c| rms(c))
                        .fold(0.0_f64, f64::max);
                    let swell = 20.0 * (worst / steady).log10();
                    assert!(
                        swell <= 3.2,
                        "the wow range thrown from {x} to {y} swelled the tone {swell:+.2} dB through \
                         the heads' fade"
                    );
                }
            }
        }
    }
}
