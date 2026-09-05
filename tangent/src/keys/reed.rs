//! The reed piano: a flat steel reed, and a charged plate listening to it.
//!
//! **Why this is not the tine with different numbers.** The tine instrument
//! is a tuning fork read by a MAGNET: its nonlinearity is the flux curve of
//! a tip swinging past a pole, and its voice is a pure fundamental with a
//! glassy bell riding on top. This one is a flat spring-steel reed, tuned by
//! a blob of solder at its tip, vibrating INSIDE a charged plate — the reed
//! and the plate are the two halves of a capacitor, and what the amplifier
//! hears is the current the motion induces, `i = V · dC/dt` (Pfeifle 2017,
//! eq. 11; 160–170 V of bias per the 200/200A service manual). Played gently
//! it is hollow and woody — almost all fundamental, the overtones of a stiff
//! bar too high and too weak to carry. Played hard the reed swings THROUGH
//! the plane of the plate, the capacitance passes its peak twice a cycle,
//! and the tone GROWLS: a second harmonic and a third arriving together,
//! which is what Pfeifle's forte direct-out shows (two unequal events per
//! mechanical cycle) and what no monotonic saturator can make. That growl is
//! the whole reason anyone has ever carried one up a flight of stairs, and it
//! is a function of how far the reed is swinging and where it rests — which
//! is why it lives here, on the displacement, and not in an overdrive after
//! the fact.
//!
//! **What the rework changed, and what decided it.** With GROWL at zero the
//! old instrument was a pure sine — every harmonic under −103 dB, measured —
//! so all of its harmonic content came from the plate, which was the right
//! architecture wearing the wrong transfer: a square-and-cube waveshaper on
//! the displacement that held H3 32 dB under H2 at every velocity and made
//! nothing above H3 at all. Three things are different now, each with its
//! measurement beside its constant:
//!
//! 1. **The plate is a [`super::pickup::Transducer`]** on the capacitive
//!    curve `C(x) = 1 / (1 + (x/gap)²)`, differentiated: the reed's
//!    displacement in gap units goes in and the plate's current comes out.
//!    At the 200's regulation of half a gap that puts H3 within a dB of H2
//!    from a mezzo-forte swing up (`pickup.rs` reproduces the brief's table
//!    to a dB), and the derivative's 6 dB/octave tilt is why a measured
//!    Wurlitzer output is spiky where the reed's motion is a clean sine.
//! 2. **GROWL is the reed's rest offset from the plate plane** — the
//!    service manual's centring adjustment, on a knob. Nearer the plane is
//!    dirtier; at the plane itself the fundamental vanishes into an octave
//!    (Trap 1, `pickup::OFFSET_FLOOR`). See [`GROWL_REACH`] for the
//!    direction and the reach, both read off the plate's own map.
//! 3. **The bar's overtones come from the beam.** [`super::beam::clamped_free`]
//!    with the reed's taper and its solder blob replaces two hand-picked
//!    ratios, `4.4` and `14.6`, whose comment had the blob's direction
//!    backwards: a tip mass RAISES a clamped bar's ratios and a taper lowers
//!    them, the blob is biggest in the bass, so the ratio is highest in the
//!    bass and falls toward the treble. They stay attack colour — Pfeifle's
//!    high-speed camera found the higher modes "comparably small or
//!    non-existent even under extreme playing conditions" — so the rasp is
//!    the plate's, not the metal's, and the `t60 · 0.09` treatment stays.
//!
//! Two things a derivative demands that a memoryless waveshaper never asked
//! for, both handled here because the transducer does not know what struck
//! the metal. A mode seeded in one sample has a velocity STEP at the strike,
//! and `d/dt` prints a step as a click at full amplitude on every note-on —
//! so the hammer's contact is real now: the strike is a raised-cosine pulse
//! of sub-strikes spread over the contact time (see [`WurliVoice::tick`]),
//! which also supplies the contact low-pass this file used to fake with a
//! one-pole. And the damper takes its energy out of the METAL, sample by
//! sample, rather than scaling the output: a landing strike used to fold the
//! damper's take into the modes in one sample, and through a derivative that
//! fold is a spike of the whole take over one `ω₀`.
//!
//! The bank of decaying sinusoids is [`super::tine::Mode`] — kicked, not
//! driven, for the reasons written on that struct.

use super::beam::{self, BeamRatios};
use super::pickup::{Curve, Transducer};
use super::tine::Mode;
use super::WurliKnobs;

/// Modes per voice: the fundamental, two bar overtones, and two thump
/// partials for the hammer's knock. A stiff bar's overtones sit an octave
/// and a half of nothing away from the fundamental; five is already more
/// than the plate lets through.
const MODES: usize = 5;

/// The first three modes are the reed and pass through the plate. The last
/// two are the knock — hammer on reed, key on rail: wood and felt, not
/// vibrating steel between two electrodes — and they join the signal AFTER
/// the plate. Through the derivative a fixed 165 Hz thud would have come
/// out 9 dB louder under a bass note than the bench signed it and 20 dB
/// quieter under a treble one, because the plate's tilt is referred to the
/// note's own fundamental; the knock is the same knock on every key.
const REED_MODES: usize = 3;

/// How far the reed swings at fortissimo, in gaps, at VOICING centre.
///
/// **0.7, from the plate's own map** (`pickup.py`'s curve driven over
/// offsets 0.2–2.0 and swings 0.05–1.0; this file's velramp test
/// re-measures the rows that matter on the real voice). At the 200's
/// regulation of half a gap a swing of 0.7 puts H2 at −8.6 dB and H3 at
/// −8.2 in the attack — level with each other, which is the brief's
/// verdict on the forte tone, and a bark the waveshaper's −8.4 of second
/// harmonic alone never had a third to go with. Under the note's own
/// decay that is about −22/−24 by the 0.12–0.52 s window the evidence
/// tables use, against the waveshaper's −16.5 and −48.6 there: a touch
/// less octave in the sustain, a third that exists, and a fourth and a
/// fifth at −25/−28 where there was nothing.
///
/// The CEILING on this number is Trap 1 at the knob's dirtiest end. With
/// GROWL at 2 the reed rests 0.42 gap from the plane (0.40 on the 140),
/// VOICING packed adds 13 % to the throw, and there H2 must still sit
/// under H1: the map reads −2.7 dB at 0.42 and −1.8 at 0.40 for a swing
/// of 0.79, and the octave overtakes the fundamental a tenth of a gap
/// nearer. The FLOOR is that the growl must arrive with the player's
/// weight: mezzo-forte (a swing of 0.29) reads H2 −22, forte (0.48) −15,
/// fortissimo −9 — the dimmer, not the switch.
const REED_SWING: f32 = 0.7;

/// Velocity to throw, as a power.
///
/// The old chain scaled the displacement as `v^1.3` and then divided the
/// loud end by its own growl compensation, `1 / (1 + 0.45 · asym)`, which
/// took 4.5 dB off fortissimo and nothing off pianissimo — a knee, not a
/// law. The plate has no such gain to hand back: its normalisation is the
/// largest slope over the excursion, which on the shoulder of the curve
/// where the reach sits is the same number at every swing. One exponent
/// cannot bend the knee exactly; 1.36 leaves the worst step 1.5 dB from
/// the bench-signed table (velocity 63), where 1.3 and 1.42 are each
/// 2.4 dB out at one end, measured in
/// `the_velocity_table_keeps_the_shape_the_bench_signed`.
const THROW_CURVE: f32 = 1.36;

/// GROWL's reach: how far one unit of the knob moves the reed's rest
/// position, in gaps. The knob runs 0..2 with the base's regulation at 1,
/// so the 200 spans 0.58 gap at GROWL 0 to 0.42 at GROWL 2.
///
/// **Which way is dirtier, and why the reach is what it is.** Nearer the
/// plane is dirtier, and it is the service manual's own fault direction:
/// "extra loud or harsh … check to see if the reed involved is slightly
/// off center". On the map at the fortissimo swing an offset of 0.42
/// reads H2 −4.5 / H3 −7.0 dB, the centre reads −8.6 / −8.2, and 0.58
/// reads −14.0 / −9.8 — the knob's low end sits on the curve's inflection
/// at `1/√3 = 0.577`, where the even term of the transfer changes sign and
/// the octave nulls (−86 dB at a pianissimo swing, measured), so GROWL at
/// zero is not a cleaner centre but a different voicing: odd-led and
/// hollow, the "hollow bark" of the owner's brief. The upper end is where
/// Trap 1 lives (see [`REED_SWING`]): 0.08 per unit is the most reach the
/// knob can have without a setting on the panel that plays an octave on
/// the harshest note it can be asked for. `pickup::OFFSET_FLOOR` still
/// guards the centre underneath this, for a base regulated nearer than
/// these two.
const GROWL_REACH: f32 = 0.08;

/// The hammer never stays on a reed longer than one period of the note.
///
/// The contact schedule below — 1.6 ms feathered to 0.5 ms flat out, the
/// felt's wear on top — was written for the middle of the keyboard and is
/// the same on every key. Now that the contact is a real pulse the
/// fundamental is excited THROUGH it, and a raised cosine's transfer has
/// its first null at two periods of the pulse: at the top of the keyboard
/// 1.6 ms is three periods of the note, the pulse would excite almost none
/// of the fundamental, and the normalisation that keeps the level would be
/// dividing by nothing. At one period the pulse still passes half the
/// fundamental, so the normalisation never exceeds 2; physically a hammer
/// that is on a reed for more than one of its periods is holding it, not
/// striking it.
const CONTACT_MAX_PERIODS: f32 = 1.0;

/// The damper's time constant. Reed dampers are small felt pads on a stiff
/// arm — quicker than the tine's, slower than yarn.
const DAMP_SECS: f32 = 0.050;

/// The keyboard the schedules are drawn between: A0 to C8.
const KEY_BASS: f32 = 21.0;
const KEY_TREBLE: f32 = 108.0;

/// The one gain the rework moved. The old chain put 0.44 on a displacement
/// bus that ran to 0.9 at fortissimo through a waveshaper and its
/// compensation, and chose the number by the bench's forte comp chords on
/// the tube instrument against the ceiling. This is the gain on the plate's
/// output, which runs to the throw of 0.7 gap, chosen the same way: 0.48
/// puts the 140's comp chords back at the −0.4 dBFS they peaked at before
/// (0.56, which matched the single-note attack peak instead, put 79 of
/// their samples into the ceiling). The plate's waveform at a mezzo swing
/// is a sine with a sine's crest factor where the waveshaper's was spiky,
/// so the two cannot both be matched: with this gain a fortissimo E4's
/// attack lands 1.3 dB under the signed table and its sustain 3.4 dB over
/// it, and the velocity table's SHAPE is held to 2 dB regardless.
const OUT_GAIN: f32 = 0.48;

/// The knock, relative to the fortissimo throw: 0.25 on the old bus of 0.9
/// put the knock about thirty decibels under the tone at forte, and the
/// plate's output sits at the throw, so the same knock is `0.25 · 0.7/0.9`.
const KNOCK: f32 = 0.194;

/// The damper's pad landing, relative to the level it lands on: 0.22 on the
/// old bus, through the same `0.7/0.9`.
const DAMPER_THUD: f32 = 0.171;

/// What separates the two shipped reed pianos before any knob moves: the
/// reeds themselves, and then the amp on top.
#[derive(Clone, Copy)]
pub struct WurliBase {
    /// Where the reed rests from the plane of the plate at GROWL centre, in
    /// gaps. Half a gap is the brief's reference regulation and the offset
    /// every table in `pickup.py` was measured at.
    pub offset: f32,
    /// The tube stage's own dirt, and it is NOT a floor under the growl:
    /// the growl is the plate's curve and vanishes at a feather — the bench
    /// measured the "never entirely clean" instrument dead clean at
    /// velocity 8. What a warm output stage actually does is favour one
    /// half of the wave by a fixed FRACTION whatever the level, so this
    /// scales an `|x|` term whose second harmonic holds a constant ratio to
    /// the fundamental all the way down: −31.9 dB at 0.06, measured. Zero
    /// for the transistor one.
    pub haze: f32,
    /// The amp's top, one pole.
    pub tone_hz: f32,
    /// Scale on the hammer knock.
    pub thump: f32,
    /// The reed's width at the tip as a fraction of its root. A Wurlitzer
    /// reed is cut from flat strip with parallel sides; the taper is in
    /// the grind, below.
    pub width_taper: f32,
    /// The reed's thickness at the tip as a fraction of its root, at the
    /// bass end of the keyboard and at the treble end, linear in the key
    /// between. Long bass reeds are ground well down toward the tip; the
    /// short treble reeds are nearly uniform stock.
    pub thickness_taper: [f32; 2],
    /// The solder blob as a fraction of the reed's own mass, `β` to the
    /// beam, at the bass end and at the treble end, geometric in the key
    /// between: a bass reed carries a lump that weighs about what the reed
    /// does, a treble reed a dab. This is the schedule that makes a bass
    /// reed and a treble reed different instruments, and its direction is
    /// guarded by name in the tests.
    pub blob: [f32; 2],
}

impl WurliBase {
    /// The classroom-and-stage classic: transistor amp, clean until leaned
    /// on, then the bark arrives all at once. Bright for a reed. Its reeds
    /// are the reference set: half-gap regulation, a grind from 0.6 in the
    /// bass to 0.85 in the treble, a blob from one reed-mass to 0.08.
    pub const TWO_HUNDRED: WurliBase = WurliBase {
        offset: 0.5,
        haze: 0.0,
        tone_hz: 9_000.0,
        thump: 1.0,
        width_taper: 1.0,
        thickness_taper: [0.6, 0.85],
        blob: [1.0, 0.08],
    };
    /// The earlier tube instrument: darker, warmer, never quite clean —
    /// there is a little even-harmonic haze on every note, and the growl
    /// comes up like a dimmer rather than a switch. Its reeds are regulated
    /// a shade nearer the plate, 0.48 gap against the 200's 0.5, which is
    /// what the old base's "growl 1.15" comes to once the growl is a
    /// length: on the map that is H2 −7.5 / H3 −7.9 dB at the fortissimo
    /// swing where the 200 reads −8.6 / −8.2, the dimmer one step up — and
    /// it is as near as this knob may start, because its top end at 0.40
    /// gap leaves H2 only 1.8 dB under H1 on the harshest note the panel
    /// can ask for (see [`REED_SWING`]). Its reeds carry more solder — the
    /// older, heavier reed stock tuned with a bigger lump — so its
    /// overtones sit higher, and less of a grind.
    pub const ONE_FORTY: WurliBase = WurliBase {
        offset: 0.48,
        haze: 0.06,
        tone_hz: 6_200.0,
        thump: 1.2,
        width_taper: 1.0,
        thickness_taper: [0.7, 0.9],
        blob: [1.3, 0.1],
    };

    /// Where `key` sits between the bass end and the treble end, 0 to 1.
    fn span(key: f32) -> f32 {
        ((key - KEY_BASS) / (KEY_TREBLE - KEY_BASS)).clamp(0.0, 1.0)
    }

    /// The solder blob on the reed of `key`, as a fraction of the reed's
    /// own mass. Geometric between the two ends, because the keyboard is.
    pub fn blob(&self, key: f32) -> f32 {
        let lo = self.blob[0].max(1e-4);
        let hi = self.blob[1].max(1e-4);
        lo * (hi / lo).powf(Self::span(key))
    }

    /// The thickness taper of the reed of `key`.
    pub fn thickness_taper(&self, key: f32) -> f32 {
        let [lo, hi] = self.thickness_taper;
        lo + (hi - lo) * Self::span(key)
    }

    /// The reed's overtones at `key`, relative to its fundamental: the
    /// clamped-free beam with this base's taper and blob. A few
    /// microseconds (beam.rs prints the figure), called once per strike.
    pub fn overtones(&self, key: f32) -> BeamRatios {
        beam::clamped_free(self.blob(key), self.width_taper, self.thickness_taper(key))
    }
}

/// One mode's share of a strike, waiting to be delivered through the
/// hammer's contact.
#[derive(Clone, Copy, Default)]
struct Pending {
    freq: f32,
    t60: f32,
    amp: f32,
}

/// One key of the reed piano.
pub struct WurliVoice {
    rate: f32,
    modes: [Mode; MODES],
    /// The plate. Regulated and calibrated at a strike from silence; a
    /// landing strike keeps what the ringing reed was struck with.
    plate: Transducer,
    /// This note's fundamental in radians per sample: what the plate's
    /// derivative is referred to.
    omega: f32,
    /// The strike still to be delivered, and how much of the contact is
    /// left to deliver it in.
    pending: [Pending; MODES],
    onset_n: usize,
    onset_left: usize,
    /// The tube stage's standing lopsidedness — see `WurliBase::haze`.
    haze: f32,
    /// DC blocker: the haze's rectified term has a mean, and the mean must
    /// not reach the bus as a per-note thud of offset.
    dc_x1: f32,
    dc_y1: f32,
    /// The amp's one-pole top.
    tone_z: f32,
    tone_a: f32,
    /// The damper is down, and what it takes from the metal every sample
    /// while it is.
    damping: bool,
    damp_step: f32,
    env: f32,
    done: bool,
    seed: u32,
}

impl WurliVoice {
    pub fn new(sample_rate: f32) -> Self {
        WurliVoice {
            rate: sample_rate,
            modes: [Mode::default(); MODES],
            plate: Transducer::new(Curve::Capacitive { gap: 1.0 }),
            omega: 1.0e-3,
            pending: [Pending::default(); MODES],
            onset_n: 0,
            onset_left: 0,
            haze: 0.0,
            dc_x1: 0.0,
            dc_y1: 0.0,
            tone_z: 0.0,
            tone_a: 1.0,
            damping: false,
            damp_step: 1.0,
            env: 0.0,
            done: true,
            seed: 0x2545_F491,
        }
    }

    pub fn strike_knobs(
        &mut self,
        pitch: u8,
        velocity: f32,
        k: &WurliKnobs,
        base: &WurliBase,
    ) {
        let f0 = 440.0 * 2.0_f32.powf((f32::from(pitch) - 69.0) / 12.0);
        let v = velocity.clamp(0.0, 1.0);
        let key = f32::from(pitch);

        // **The bar's overtones**, from the beam with this base's reeds:
        // highest in the bass where the blob is heaviest, falling toward
        // the treble. They are colour, not chord: the plate reads
        // displacement near the tip and barely sees them, and they are gone
        // before the sustain starts.
        let bar = base.overtones(key);

        // Decay: a reed outlasts a tine's bell but not its fundamental —
        // there is no tonebar here to bank energy in, which is also why 5.0
        // and not the tine's tonebar-fed nine-plus.
        let t60_f = (5.0 * (-(key - 33.0) / 40.0).exp()).clamp(0.7, 8.0) * k.decay;
        let t60_f = t60_f.clamp(0.1, 30.0);

        // Contact: felt over a wooden hammer, on a sprung key — shorter and
        // harder than the tine's neoprene. VOICING is the felt's wear, and
        // `hard` is its other face: packed felt does not just shorten the
        // contact, it knocks louder against the reed and throws it further
        // through the gap, so the knob reaches the attack and the growl
        // rather than only the two bar overtones nobody could hear move.
        // The contact is a pulse of that many samples, capped at one period
        // of the note (see `CONTACT_MAX_PERIODS`).
        let felt = 1.28_f32.powf(1.0 - 2.0 * k.voicing.clamp(0.0, 1.0));
        let hard = 1.0 / felt;
        let contact = ((1.6 - 1.1 * v.powf(0.7)) * 0.001 * felt).min(CONTACT_MAX_PERIODS / f0);
        let onset = ((contact * self.rate).round() as usize).max(1);
        // What the pulse passes at the fundamental: the fundamental is
        // normalised through it so the throw is the throw on every key at
        // every velocity, and the overtones get exactly what a hammer that
        // stays on the reed that long leaves them — the roll-off this file
        // used to approximate with a one-pole at `1/contact`.
        let gain0 = pulse_gain(onset, std::f32::consts::TAU * f0 / self.rate).max(0.5);

        // The REED knob is how much of the bar's own metal reaches the
        // plate: 0 is a sine in a box, 2 is a reed with its clank showing.
        // The weights are the ones the bench signed, referred to the
        // OUTPUT: the plate's derivative lifts a partial at `r · f0` by `r`,
        // so the metal is seeded with `w / r` and comes out at `w`.
        let w1 = 0.035 * k.reed / bar.f2;
        let w2 = 0.012 * k.reed / bar.f3;

        // **The knock.** A wurli's attack has a woody tick the tine never
        // had — hammer meeting reed meets key meeting rail. Two fixed
        // partials, gone in thirty milliseconds, scaled by velocity harder
        // than the tone is.
        let knock = base.thump * k.thump * KNOCK * v.powf(1.6) * hard * hard;

        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        let jitter = 1.0 + ((self.seed >> 8) as f32 / 16_777_216.0 - 0.5) * 0.03;

        // The throw, in gaps. Packed felt throws the reed further: the
        // square root of the felt's hardness, which is the `hard^0.49` the
        // old growl coefficient carried into its curve.
        let throw = REED_SWING * v.powf(THROW_CURVE) * hard.sqrt();
        let norm = 1.0 / (1.0 + w1 + w2);
        let amp = throw * norm * jitter;

        // **A hammer landing on a reed that is still moving adds to the
        // motion** — the same rule, for the same pop, as the tine's; see
        // `EpVoice::strike_full` for the argument, and `Mode::kick_add` for
        // the arithmetic. The fundamental — the one mode that keeps energy
        // — is capped at a little over this strike's own throw so rapid
        // repeats cannot stack it through the plate. The overtones and the
        // knock are attack colour, gone in tens of milliseconds, and take
        // the whole seed.
        //
        // **The plate belongs to the note that is ringing.** From silence
        // it forgets its last sample, is regulated by GROWL and calibrated
        // for this throw. On a landing strike it is left exactly as it is:
        // re-regulating it moves `f(x₀)` under a reed that has not moved,
        // which the derivative prints as a spike of the whole flux
        // difference over one `ω₀` — measured at 2.8 full scale for a tenth
        // of a gap at middle C — and re-calibrating it is a gain step on the
        // residue. What that costs is nothing at the centre and under a dB
        // at the knob's ends: over the whole reach the steepest slope inside
        // any excursion is within 0.7 dB of the one at fortissimo, because
        // the reach sits on the shoulder of the curve where the slope is
        // flattest. A GROWL moved between two strikes of one held note
        // reaches the next note struck from silence, which is how every
        // strike-time knob on this panel already behaves.
        let landing = !self.done;
        if !landing {
            self.plate.clear();
            self.plate
                .set_offset(base.offset + (1.0 - k.growl.clamp(0.0, 2.0)) * GROWL_REACH);
            self.plate.calibrate(throw);
            // The amp's one pole and the blocker hold the last sample of
            // what was ringing; from silence that is nothing.
            self.dc_x1 = 0.0;
            self.dc_y1 = 0.0;
            self.tone_z = 0.0;
        }
        self.omega = std::f32::consts::TAU * f0 / self.rate;
        // **The cap is judged where the hammer will actually land.** The
        // pulse is symmetric, so what it delivers is exactly one seed of
        // this size at its centre sample, and the reed will have moved on
        // by then: at middle C a fortissimo contact's centre is 24° of the
        // cycle away. Judged at the strike instant, a landing that was
        // benign became dead against the motion by the time it arrived —
        // measured: the fundamental fell from 0.477 to 0.132 with `allowed`
        // at 1.0, a −13 dB hole in the pedalled rapid-repeat bench where
        // the floor promises −3. So the seed is judged against a copy of
        // the mode ticked forward to the centre of the pulse, which for a
        // linear mode is the state it will meet, to the sample.
        let allowed = if landing {
            let mut met = self.modes[0];
            for _ in 0..onset / 2 {
                met.tick();
            }
            met.seed_within(f0, self.rate, amp, amp * 1.25)
        } else {
            1.0
        };

        // The overtone rings a tenth of the fundamental's life, not a
        // quarter: it is attack colour, and the old 0.22 left it singing
        // into the sustain where it read as a chime. Everything below is
        // delivered by `tick` through the contact pulse; a strike landing
        // inside the previous strike's contact — under two milliseconds —
        // takes the pulse over, and what the first had not yet delivered is
        // not delivered.
        let seed = amp / gain0;
        self.pending = [
            Pending { freq: f0, t60: t60_f, amp: seed * allowed },
            Pending { freq: f0 * bar.f2, t60: t60_f * 0.09, amp: seed * w1 },
            Pending { freq: f0 * bar.f3, t60: t60_f * 0.10, amp: seed * w2 },
            Pending { freq: 165.0, t60: 0.035, amp: knock },
            Pending { freq: 610.0, t60: 0.014, amp: knock * 0.5 },
        ];
        self.onset_n = onset;
        self.onset_left = onset;

        self.haze = base.haze;
        let corner = base.tone_hz.min(self.rate * 0.45);
        self.tone_a = 1.0 - (-std::f32::consts::TAU * corner / self.rate).exp();

        self.damping = false;
        let damp_secs = (DAMP_SECS * k.release).clamp(0.004, 1.5);
        self.damp_step = (-1.0 / (damp_secs * self.rate)).exp();
        self.env = if landing { self.env.max(throw) } else { throw };
        self.done = false;
    }

    /// Silence now: every state cleared, `finished` true — the voice a
    /// first strike can land on as if from `new`. See `EpVoice::silence`.
    pub fn silence(&mut self) {
        for m in &mut self.modes {
            m.clear();
        }
        self.plate.clear();
        self.onset_left = 0;
        self.dc_x1 = 0.0;
        self.dc_y1 = 0.0;
        self.tone_z = 0.0;
        self.damping = false;
        self.env = 0.0;
        self.done = true;
    }

    pub fn release(&mut self, pedal: bool) {
        if !pedal {
            self.land_damper();
            self.damping = true;
        }
    }

    pub fn damp(&mut self) {
        self.land_damper();
        self.damping = true;
    }

    /// The pad lands on the reed — softer than the tine's thump, because the
    /// arm is shorter and the reed lighter. Added to the knock mode, not
    /// seeded over it: a staccato note's knock may still be ringing.
    fn land_damper(&mut self) {
        if self.done || self.damping {
            return;
        }
        self.modes[3].kick_add(140.0, 0.030, self.rate, self.env * DAMPER_THUD);
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

        // **The hammer's contact.** The strike is delivered as a raised
        // cosine of sub-strikes over the contact time, so the reed's
        // VELOCITY rises over the contact the way a felt-faced hammer makes
        // it, instead of stepping in one sample. The modes are linear, so
        // the sum of the sub-strikes is exactly the one strike convolved
        // with the pulse: the same steady sinusoid, the contact's own
        // low-pass on every partial, and no step for the plate to print.
        if self.onset_left > 0 {
            let n = self.onset_n as f32;
            let i = (self.onset_n - self.onset_left) as f32;
            let w = (1.0 - (std::f32::consts::TAU * (i + 0.5) / n).cos()) / n;
            for (m, p) in self.modes.iter_mut().zip(&self.pending) {
                m.kick_add(p.freq, p.t60, self.rate, p.amp * w);
            }
            self.onset_left -= 1;
        }

        // **The damper takes from the metal.** Scaling the state of a
        // linear mode by a constant every sample is exactly a faster decay,
        // which is what a felt pad on a reed is — and the plate then reads
        // a reed that is actually slowing down, so the dirt cleans up as the
        // damper takes hold, which is what the real instrument does.
        if self.damping {
            for m in &mut self.modes {
                m.scale(self.damp_step);
            }
        }

        let mut reed = 0.0;
        let mut thud = 0.0;
        for (i, m) in self.modes.iter_mut().enumerate() {
            let y = m.tick();
            if i < REED_MODES {
                reed += y;
            } else {
                thud += y;
            }
        }

        // **The plate.** The reed's displacement in gaps goes in; the
        // current comes out, referred to this note's fundamental. Because
        // the displacement decays with the note, the dirt fades with it — a
        // chord's tail cleans itself up, which is exactly what the real
        // instrument does and no post-effect can. The knock joins after
        // the plate (see `REED_MODES`).
        let sig = self.plate.tick(reed, self.omega) + thud;
        // The `|sig|` term is the tube stage: a fixed favouring of one half
        // of the wave, so its second harmonic keeps a constant RATIO to the
        // note however softly it is played — the 140's standing haze. The DC
        // it carries (a rectified wave has a mean) dies in the blocker below.
        let shaped = sig + self.haze * sig.abs();

        let dc = shaped - self.dc_x1 + 0.9995 * self.dc_y1;
        self.dc_x1 = shaped;
        self.dc_y1 = dc;

        self.tone_z += self.tone_a * (dc - self.tone_z);
        let out = self.tone_z * OUT_GAIN;
        let out = if out.is_finite() { out.clamp(-4.0, 4.0) } else { 0.0 };

        let a = out.abs();
        self.env += (a - self.env) * if a > self.env { 0.4 } else { 0.0006 };
        if self.env < 2.0e-5 {
            self.done = true;
            for m in &mut self.modes {
                m.clear();
            }
            self.plate.clear();
            self.onset_left = 0;
            self.dc_x1 = 0.0;
            self.dc_y1 = 0.0;
            self.tone_z = 0.0;
        }
        out
    }
}

/// What a raised-cosine pulse of `n` samples passes at `w` radians per
/// sample: the magnitude of its transfer, 1 at DC. The pulse `tick`
/// delivers a strike through, evaluated the same way it is delivered.
fn pulse_gain(n: usize, w: f32) -> f32 {
    let nf = n as f32;
    let (mut re, mut im) = (0.0_f32, 0.0_f32);
    for i in 0..n {
        let wi = (1.0 - (std::f32::consts::TAU * (i as f32 + 0.5) / nf).cos()) / nf;
        let (s, c) = (w * i as f32).sin_cos();
        re += wi * c;
        im -= wi * s;
    }
    re.hypot(im)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;
    const E4: f32 = 329.627_56;
    const C4: f32 = 261.625_6;

    /// The same Hann-windowed Goertzel the bench and mod.rs measure with; a
    /// rectangular window lies about anything under the fundamental's
    /// sidelobes, which is where the soft end of this instrument lives.
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

    fn knobs(growl: f32, voicing: f32) -> WurliKnobs {
        let mut k = WurliKnobs::default();
        k.growl = growl;
        k.voicing = voicing;
        k
    }

    /// One strike of `key` from silence, ticked for `secs`.
    fn strike(key: u8, vel: f32, k: &WurliKnobs, base: &WurliBase, secs: f32) -> Vec<f32> {
        let mut v = WurliVoice::new(RATE);
        v.strike_knobs(key, vel, k, base);
        (0..(secs * RATE) as usize).map(|_| v.tick()).collect()
    }

    fn samples(secs: f32) -> usize {
        (secs * RATE) as usize
    }

    /// The peak of the attack, the first 250 ms from 4 ms in — the bench's
    /// window for the velocity table.
    fn attack_peak(x: &[f32]) -> f32 {
        x[samples(0.004)..samples(0.254)]
            .iter()
            .fold(0.0_f32, |m, s| m.max(s.abs()))
    }

    /// H1 absolute, and H2..=H8 in dB re H1, over `len` seconds from
    /// `from`.
    fn harmonics_in(x: &[f32], f0: f32, from: f32, len: f32) -> (f32, [f32; 7]) {
        let seg = &x[samples(from)..samples(from + len)];
        let h1 = goertzel_hann(seg, f0, RATE).max(1e-12);
        let mut out = [0.0; 7];
        for (i, o) in out.iter_mut().enumerate() {
            *o = db(goertzel_hann(seg, f0 * (i + 2) as f32, RATE) / h1);
        }
        (h1, out)
    }

    /// The window `evidence/harmonics-today.txt` was measured in, 0.12–0.52 s
    /// — a third of the way down the note's decay at E4.
    fn harmonics(x: &[f32], f0: f32) -> (f32, [f32; 7]) {
        harmonics_in(x, f0, 0.12, 0.40)
    }

    /// The attack, the first 100 ms: where the swing is largest and so
    /// where the plate is at its dirtiest.
    fn attack_harmonics(x: &[f32], f0: f32) -> (f32, [f32; 7]) {
        harmonics_in(x, f0, 0.004, 0.10)
    }

    fn largest_step(x: &[f32]) -> f32 {
        x.windows(2).fold(0.0_f32, |m, w| m.max((w[1] - w[0]).abs()))
    }

    const VELS: [u8; 12] = [8, 19, 30, 41, 52, 63, 74, 85, 96, 107, 118, 127];

    /// **The ratio schedule, both bases**, printed for the record at every
    /// octave, and held to the beam: a bass reed's `f2/f1` is above a treble
    /// reed's, and every ratio sits inside the band the beam's two direction
    /// guards define — no lower than the same taper with no blob, no higher
    /// than the same blob on a uniform bar.
    #[test]
    fn a_bass_reeds_overtone_sits_higher_than_a_treble_reeds() {
        for (name, base) in [("200", WurliBase::TWO_HUNDRED), ("140", WurliBase::ONE_FORTY)] {
            println!("  Wurli {name}: key   beta   th     f2/f1    f3/f1");
            for key in [21.0_f32, 33.0, 45.0, 57.0, 69.0, 81.0, 93.0] {
                let r = base.overtones(key);
                println!(
                    "             {key:3.0}   {:5.3}  {:4.2}   {:6.3}   {:6.3}",
                    base.blob(key),
                    base.thickness_taper(key),
                    r.f2,
                    r.f3
                );
                let no_blob = beam::clamped_free(0.0, base.width_taper, base.thickness_taper(key));
                let no_taper = beam::clamped_free(base.blob(key), 1.0, 1.0);
                assert!(
                    no_blob.f2 <= r.f2 && r.f2 <= no_taper.f2,
                    "{name} key {key}: f2/f1 {:.3} is outside the beam's band [{:.3}, {:.3}]",
                    r.f2,
                    no_blob.f2,
                    no_taper.f2
                );
                assert!(
                    no_blob.f3 <= r.f3 && r.f3 <= no_taper.f3,
                    "{name} key {key}: f3/f1 {:.3} is outside the beam's band [{:.3}, {:.3}]",
                    r.f3,
                    no_blob.f3,
                    no_taper.f3
                );
            }
            let bass = base.overtones(33.0);
            let treble = base.overtones(73.0);
            assert!(
                bass.f2 > treble.f2 && bass.f3 > treble.f3,
                "{name}: the ratio must fall toward the treble: key 33 {:.3}/{:.3}, key 73 {:.3}/{:.3}",
                bass.f2,
                bass.f3,
                treble.f2,
                treble.f3
            );
        }
    }

    /// **The solder blob is biggest in the bass.** This is the direction the
    /// old file had backwards; if it goes red a base has its blob schedule
    /// the wrong way round, and the overtone ratio will rise toward the
    /// treble with it.
    #[test]
    fn the_solder_blob_shrinks_toward_the_treble() {
        for (name, base) in [("200", WurliBase::TWO_HUNDRED), ("140", WurliBase::ONE_FORTY)] {
            let (lo, mid, hi) = (base.blob(21.0), base.blob(60.0), base.blob(108.0));
            assert!(
                lo > mid && mid > hi,
                "{name}: the blob must shrink up the keyboard: {lo:.3} at A0, {mid:.3} at C4, {hi:.3} at C8"
            );
            assert!(lo > 0.5 && hi < 0.2, "{name}: a bass reed carries a lump, a treble reed a dab: {lo:.3} / {hi:.3}");
        }
    }

    /// **Acceptance B1.** At GROWL centre, forte E4, H2 and H3 within 6 dB
    /// of each other on the real voice — the waveshaper held H3 32 dB under
    /// H2 at every velocity. Measured at forte (107) and fortissimo (127),
    /// in the attack and in the evidence's sustain window, and held in
    /// both; and at fortissimo the attack's octave must be a growl, not a
    /// hint — above −12 dB, where the map puts a 0.7 swing at −8.6 before
    /// the window's hundred milliseconds of decay.
    #[test]
    fn at_growl_centre_forte_e4_speaks_h2_and_h3_together() {
        for vel in [107_u8, 127] {
            let x = strike(64, f32::from(vel) / 127.0, &WurliKnobs::default(), &WurliBase::TWO_HUNDRED, 0.6);
            for (window, (_, h)) in [("attack ", attack_harmonics(&x, E4)), ("sustain", harmonics(&x, E4))] {
                println!(
                    "  200, growl centre, E4 vel {vel}, {window}: H2 {:+.1}  H3 {:+.1}  H4 {:+.1}  H5 {:+.1} dB re H1",
                    h[0], h[1], h[2], h[3]
                );
                assert!(
                    (h[0] - h[1]).abs() <= 6.0,
                    "vel {vel} {window}: H2 {:+.1} and H3 {:+.1} are {:.1} dB apart",
                    h[0],
                    h[1],
                    (h[0] - h[1]).abs()
                );
            }
            if vel == 127 {
                let (_, h) = attack_harmonics(&x, E4);
                assert!(h[0] > -12.0, "vel {vel}: H2 at {:+.1} dB in the attack is not a growl", h[0]);
            }
        }
    }

    /// **Acceptance B2, Trap 2's own check.** The peak of the attack rises
    /// monotonically with velocity at GROWL min, centre and max, both
    /// bases. This is what caught BARK turning into an inverse volume knob
    /// twice on the tine.
    #[test]
    fn peak_output_rises_monotonically_with_velocity_at_every_growl() {
        for (name, base) in [("200", WurliBase::TWO_HUNDRED), ("140", WurliBase::ONE_FORTY)] {
            for growl in [0.0_f32, 1.0, 2.0] {
                let peaks: Vec<f32> = VELS
                    .iter()
                    .map(|&v| attack_peak(&strike(64, f32::from(v) / 127.0, &knobs(growl, 0.5), &base, 0.3)))
                    .collect();
                for (i, w) in peaks.windows(2).enumerate() {
                    assert!(
                        w[1] > w[0],
                        "{name} growl {growl}: peak fell from {} to {} between velocity {} and {}",
                        w[0],
                        w[1],
                        VELS[i],
                        VELS[i + 1]
                    );
                }
            }
        }
    }

    /// **Acceptance B3, Trap 1.** H2 never exceeds H1 — the note keeps its
    /// pitch — at any GROWL, any VOICING, any velocity, on either base,
    /// measured in the ATTACK, where the swing is largest and the plate
    /// dirtiest (a third of the way down the decay the same note reads
    /// 10 dB cleaner, and would flatter this). The worst case the panel can
    /// reach is the 140 with GROWL and VOICING at the top and the key
    /// floored; its margin is printed, and the map says 1.8 dB.
    #[test]
    fn the_octave_never_overtakes_the_fundamental() {
        let mut worst = (f32::MIN, String::new());
        for (name, base) in [("200", WurliBase::TWO_HUNDRED), ("140", WurliBase::ONE_FORTY)] {
            for growl in [0.0_f32, 0.5, 1.0, 1.5, 2.0] {
                for voicing in [0.0_f32, 0.5, 1.0] {
                    for &vel in &VELS {
                        let x = strike(64, f32::from(vel) / 127.0, &knobs(growl, voicing), &base, 0.12);
                        let (_, h) = attack_harmonics(&x, E4);
                        if h[0] > worst.0 {
                            worst = (h[0], format!("{name} growl {growl} voicing {voicing} vel {vel}"));
                        }
                    }
                }
            }
        }
        println!("  closest the octave comes to the fundamental: {:+.1} dB at {}", worst.0, worst.1);
        assert!(worst.0 < 0.0, "H2 reached {:+.1} dB re H1 at {}: that note plays an octave", worst.0, worst.1);
    }

    /// **Acceptance B4.** The E4 velramp harmonic table of
    /// `evidence/harmonics-today.txt`, re-measured on the voice at this
    /// HEAD for both bases at their shipped knobs — the bench's own render
    /// is in MERGE-ME §5 beside it. Held loosely, because it is a record,
    /// not a gate: the growl arrives with velocity, and at the top there is
    /// a third harmonic at all — the waveshaper had it at −48 dB on both
    /// bases. (The 140's haze is an `|x|` term, second harmonic by
    /// construction, so its H2 leads its H3; the 200's B1 test holds the
    /// two together.)
    #[test]
    fn the_e4_velramp_harmonic_table() {
        for (name, base, patch) in [
            ("200", WurliBase::TWO_HUNDRED, super::super::Patch::Wurli200),
            ("140", WurliBase::ONE_FORTY, super::super::Patch::Wurli140),
        ] {
            let k = WurliKnobs::from_params(&patch.defaults());
            println!("  Wurli {name}, E4, voice, 0.12-0.52 s");
            println!("  vel    H1 dBFS |  H2     H3     H4     H5     H6     H7     H8   (dB re H1)");
            let mut h2 = Vec::new();
            let mut h3 = Vec::new();
            for vel in [8_u8, 41, 74, 107, 127] {
                let x = strike(64, f32::from(vel) / 127.0, &k, &base, 0.6);
                let (h1, h) = harmonics(&x, E4);
                println!(
                    "  {vel:3}   {:8.1} | {:5.1}  {:5.1}  {:5.1}  {:5.1}  {:5.1}  {:5.1}  {:5.1}",
                    db(h1), h[0], h[1], h[2], h[3], h[4], h[5], h[6]
                );
                h2.push(h[0]);
                h3.push(h[1]);
            }
            assert!(h2[4] > h2[0] + 10.0, "{name}: the growl did not arrive with velocity: H2 {:+.1} at 8, {:+.1} at 127", h2[0], h2[4]);
            assert!(h3[4] > -30.0, "{name}: at fortissimo H3 sits at {:+.1} dB; the waveshaper's −48 is back", h3[4]);
        }
    }

    /// **Acceptance B5**, the assertion mod.rs makes, made here too so it
    /// cannot be lost in the merge: the reed's loudest upper partial at C4
    /// fortissimo sits ON the harmonic series, in 1.8..3.2 — growl is
    /// harmonic — and its bar overtone, at the ratio the beam gives this
    /// base at C4, stays attack colour, under −35 dB even in the attack
    /// window where it is loudest.
    #[test]
    fn the_reeds_loudest_upper_partial_sits_on_the_harmonic_series() {
        let x = strike(60, 1.0, &WurliKnobs::default(), &WurliBase::TWO_HUNDRED, 0.25);
        let mut best = (0.0_f32, 0.0_f32);
        let mut f = C4 * 1.5;
        while f < C4 * 8.2 {
            let g = goertzel_hann(&x, f, RATE);
            if g > best.1 {
                best = (f, g);
            }
            f *= 1.005_79;
        }
        let r = best.0 / C4;
        println!("  loudest partial above the fundamental at C4 ff: {r:.3} x f0");
        assert!(
            (r - r.round()).abs() < 0.08 && (1.8..=3.2).contains(&r),
            "the reed's loudest upper partial sits at {r:.2} x f0 — growl is harmonic, this is not"
        );
        let fund = goertzel_hann(&x, C4, RATE).max(1e-12);
        let over = C4 * WurliBase::TWO_HUNDRED.overtones(60.0).f2;
        let mut peak = 0.0_f32;
        let mut f = over * 0.95;
        while f < over * 1.05 {
            peak = peak.max(goertzel_hann(&x, f, RATE));
            f += over * 0.001;
        }
        let over_db = db(peak / fund);
        println!("  bar overtone at {:.2} x f0 in the attack: {over_db:+.1} dB re f0", over / C4);
        assert!(over_db < -35.0, "the reed's bar overtone reads {over_db:.0} dB under the fundamental; louder than -35 and it starts sounding like a bell");
    }

    /// **The velocity table keeps the shape the bench signed.** The bare
    /// voice's attack peak at every velocity, relative to fortissimo, as it
    /// measured at 898cbfb with the waveshaper — GROWL centre, both bases —
    /// held to 2 dB at every step after the one gain `OUT_GAIN` documents —
    /// and that gain itself within 2 dB of the signed level, which it is by
    /// 1.3. A physics change may move the timbre; it may not re-map the
    /// keyboard under the player's hands.
    #[test]
    fn the_velocity_table_keeps_the_shape_the_bench_signed() {
        const SIGNED_200: [f32; 12] = [-39.8, -30.6, -25.8, -22.6, -20.0, -17.8, -15.8, -14.1, -12.4, -10.8, -9.2, -8.0];
        const SIGNED_140: [f32; 12] = [-39.5, -30.3, -25.6, -22.3, -19.7, -17.5, -15.6, -13.8, -12.1, -10.4, -8.9, -7.6];
        for (name, base, signed) in [
            ("200", WurliBase::TWO_HUNDRED, SIGNED_200),
            ("140", WurliBase::ONE_FORTY, SIGNED_140),
        ] {
            let peaks: Vec<f32> = VELS
                .iter()
                .map(|&v| db(attack_peak(&strike(64, f32::from(v) / 127.0, &knobs(1.0, 0.5), &base, 0.3))))
                .collect();
            println!("  Wurli {name} attack peak, dBFS on the voice: vel / now / signed");
            let mut worst = 0.0_f32;
            for (i, &v) in VELS.iter().enumerate() {
                let now = peaks[i] - peaks[11];
                let was = signed[i] - signed[11];
                worst = worst.max((now - was).abs());
                println!("    {v:3}  {:+6.1}  {:+6.1}   re ff {now:+6.1} vs {was:+6.1}", peaks[i], signed[i]);
            }
            println!("    fortissimo now {:+.1} dBFS, signed {:+.1}; worst shape drift {worst:.2} dB", peaks[11], signed[11]);
            assert!(worst <= 2.0, "{name}: the velocity table's shape drifted {worst:.1} dB from the bench-signed one");
            assert!((peaks[11] - signed[11]).abs() <= 2.0, "{name}: fortissimo moved from {:+.1} to {:+.1} dBFS", signed[11], peaks[11]);
        }
    }

    /// **A strike is a hammer, not a click.** Through a derivative, a mode
    /// seeded in one sample is a full-amplitude step on the first sample;
    /// the contact pulse spreads it over the felt's contact time. The
    /// largest one-sample step in the attack of a fortissimo E4 must be a
    /// small fraction of its peak — a sinusoid at E4 moves 4 % of its
    /// amplitude per sample, and a fortissimo contact is 24 samples.
    /// Break the pulse (`onset = 1`) and the step is the peak itself.
    #[test]
    fn a_strike_is_a_hammer_not_a_click() {
        for (name, base) in [("200", WurliBase::TWO_HUNDRED), ("140", WurliBase::ONE_FORTY)] {
            for vel in [0.3_f32, 1.0] {
                let x = strike(64, vel, &WurliKnobs::default(), &base, 0.05);
                let peak = x.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
                let step = largest_step(&x);
                println!("  {name} vel {vel}: largest one-sample step {:.4} against a peak of {:.4} ({:.2} of it)", step, peak, step / peak);
                assert!(step < 0.2 * peak, "{name} vel {vel}: a step of {step:.3} on a peak of {peak:.3} is a click");
            }
        }
    }

    /// **A restrike on a damping reed adds a hammer, not a spike.** The
    /// damper is down on a released note and has taken a third of it when
    /// the key is struck again; what the strike adds must be no sharper
    /// than a strike from silence. The old landing path folded the damper's
    /// take into the modes in one sample — a displacement step the plate
    /// would print as a spike of the whole take over one `ω₀`, measured at
    /// forty times a fresh strike's step with the fold put back. Twice a
    /// fresh strike is the bound, because two hammers' velocities add:
    /// a strike landing in phase on a ringing reed legitimately moves it
    /// faster than one from silence, and the growl-change test below
    /// measures that at 1.5.
    #[test]
    fn a_restrike_on_a_damping_reed_adds_a_hammer_not_a_spike() {
        let k = WurliKnobs::default();
        let base = WurliBase::TWO_HUNDRED;
        let fresh = largest_step(&strike(64, 0.8, &k, &base, 0.05));
        let mut v = WurliVoice::new(RATE);
        v.strike_knobs(64, 0.8, &k, &base);
        for _ in 0..samples(0.1) {
            v.tick();
        }
        v.release(false);
        for _ in 0..samples(0.02) {
            v.tick();
        }
        v.strike_knobs(64, 0.8, &k, &base);
        let after: Vec<f32> = (0..samples(0.005)).map(|_| v.tick()).collect();
        let step = largest_step(&after);
        println!("  largest step after a restrike on a damping reed {:.4}; a fresh strike's {:.4}", step, fresh);
        assert!(step <= fresh * 2.0, "the restrike stepped {step:.4} where a fresh strike steps {fresh:.4}: that is the damper fold coming back");
    }

    /// **GROWL moved between two strikes of a ringing note does not step
    /// it.** The plate is left as the ringing note was struck; regulating it
    /// under the reed moves `f(x₀)` and the derivative prints the move —
    /// measured at over a hundred times a fresh strike's step with the
    /// regulation applied on landing. Two hammers' velocities add, so the
    /// bound is twice a fresh strike; measured 1.5.
    #[test]
    fn a_growl_change_under_a_ringing_reed_does_not_step_it() {
        let base = WurliBase::TWO_HUNDRED;
        let fresh = largest_step(&strike(64, 0.8, &knobs(1.0, 0.5), &base, 0.05));
        let mut v = WurliVoice::new(RATE);
        v.strike_knobs(64, 0.8, &knobs(1.0, 0.5), &base);
        for _ in 0..samples(0.1) {
            v.tick();
        }
        v.strike_knobs(64, 0.8, &knobs(2.0, 0.5), &base);
        let after: Vec<f32> = (0..samples(0.005)).map(|_| v.tick()).collect();
        let step = largest_step(&after);
        println!("  largest step after a restrike at a new GROWL {:.4}; a fresh strike's {:.4}", step, fresh);
        assert!(step <= fresh * 2.0, "the restrike stepped {step:.4} where a fresh strike steps {fresh:.4}: the plate was re-regulated under a ringing reed");
    }

    /// **The restrike bench's STEAL verdict on this instrument is the bed's
    /// own phase, not the steal.** The scenario fills the pool with 64
    /// pedalled notes from A0, releases one, and lands a C7 on it; its
    /// `dip` compares the quietest 4 ms in the 12 after the strike with the
    /// 4 before. A 4 ms window on a bed whose energy is in 30–100 Hz notes
    /// measures where those notes' cycles happen to be, and the plate's
    /// derivative moved every one of them a quarter cycle — so the same
    /// windows on the same bed with the newcomer LEFT OUT dip −8.9 dB on
    /// the 200 and −8.8 on the 140, and with it in, −7.0 and −6.8: the
    /// strike adds two decibels to the hole it is accused of digging. Held
    /// here so the verdict cannot be mistaken for a mechanism fault again:
    /// the newcomer may not deepen the bed's own dip.
    #[test]
    fn the_steal_benchs_dip_is_the_beds_own() {
        use super::super::{Keys, Patch, VOICES};
        let block = 128usize;
        let at = |secs: f32| (secs * RATE) as usize / block * block;
        let render = |patch: Patch, newcomer: bool| -> Vec<f32> {
            let mut k = Keys::new(RATE);
            k.set_patch(patch);
            k.set_pedal(true);
            for i in 0..VOICES {
                k.note_on(24 + i as u8, 102.0 / 127.0);
            }
            let frames = samples(0.8);
            let mut out = vec![0.0_f32; frames * 2];
            let mut scratch = vec![0.0_f32; block * 2];
            let mut f = 0;
            while f < frames {
                if f == at(0.1) {
                    k.note_off(30);
                }
                if newcomer && f == at(0.5) {
                    k.note_on(100, 102.0 / 127.0);
                }
                scratch.fill(0.0);
                k.render(&mut scratch, block, 2);
                out[f * 2..(f + block) * 2].copy_from_slice(&scratch);
                f += block;
            }
            (0..frames).map(|i| (out[i * 2] + out[i * 2 + 1]) * 0.5).collect()
        };
        let dip_of = |x: &[f32]| -> f32 {
            let onset = at(0.5);
            let rms = |a: usize, b: usize| {
                (x[a..b].iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / (b - a) as f64).sqrt() as f32
            };
            let before = rms(onset - samples(0.004), onset);
            let mut worst = f32::MAX;
            let mut a = onset;
            while a + samples(0.004) <= onset + samples(0.012) {
                worst = worst.min(rms(a, a + samples(0.004)));
                a += samples(0.001);
            }
            db(worst / before)
        };
        for patch in [Patch::Wurli200, Patch::Wurli140] {
            let bed = dip_of(&render(patch, false));
            let stolen = dip_of(&render(patch, true));
            println!("  {patch:?}: the bench's steal window dips {bed:+.1} dB on the bed alone, {stolen:+.1} dB with the newcomer");
            assert!(
                stolen >= bed - 0.5,
                "{patch:?}: the newcomer deepened the bed's dip from {bed:+.1} to {stolen:+.1} dB — that is the steal, not the bed"
            );
        }
    }
}
