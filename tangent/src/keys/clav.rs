//! The clav: a string struck and HELD, and two pickups arguing about it.
//!
//! **Why this is neither the piano nor a harpsichord.** A piano's hammer
//! throws and leaves; a harpsichord's plectrum plucks and leaves. The clav's
//! tangent — a rubber tip on the back of the key — strikes the string and
//! then STAYS, pinning it against its anvil for as long as the finger is
//! down. That one fact is the whole instrument: the strike is percussive
//! like a hammer, the sustain is a fretted string like a guitarist's
//! hammer-on, and the note ends not by fading but by the tangent LIFTING.
//! While it is down the string is two strings — the speaking length between
//! anvil and bridge, and the far length wound in yarn and pinned silent —
//! and when it lifts they become one string again. The loop grows by the
//! yarn's length, the pitch falls, and the yarn chokes what is left.
//! Gabrielli et al. measured the fall at three semitones across the whole
//! keyboard, and that drop IS the release of a clavinet; the click that
//! used to stand in for it is gone. Playing one is as much about the lift as
//! the press, which is why the rejoin is knobbed rather than fixed.
//!
//! **Where the funk lives.** In the PICKUPS. Two of them — a bridge coil
//! and a centre coil — each a point reading of the string, and a point
//! reading of a travelling wave is a comb filter: everything whose
//! half-wavelength divides the coil's distance from the bridge is simply
//! not heard. The rocker takes one coil, the other, their sum, or their
//! DIFFERENCE, which cancels the fundamental the two nearly share and
//! leaves the thin sound. And each coil is a magnet whose flux against the
//! string's distance is an exponential — monotonic, because the string
//! never crosses the magnet's axis — read through a derivative, so a hard
//! note comes out with a second harmonic a soft one does not have. That is
//! the only nonlinearity in the instrument, and it is a gentler colour than
//! the electric pianos' on purpose: it cannot make a second event per cycle.
//!
//! The string itself is [`super::string::Str`] — the piano's loop, borrowed
//! whole — under a stiffness schedule of its own. Same bridge, different
//! cabinet, different wire.

use super::pickup::{Curve, Transducer};
use super::string::{design_loss, loop_phase_delay, Ap, Lp, Str};
use super::ClavKnobs;

/// How long the yarn takes to choke the string once the tangent lifts.
/// Yarn is fast — that is what it is FOR — but it is not a gate.
const CHOKE_SECS: f32 = 0.014;

/// The pickup combs' shared delay store. Sized for the lowest string at
/// 96 kHz with a coil at the far end of any base's geometry; anything longer
/// clamps, which only a sub-contra note could notice.
const COMB_MAX: usize = 2048;

/// The non-speaking line's store. At SNAP's top the rejoin is six semitones
/// — 0.414 of the loop — and the loop is at most 4000 samples, so 1657 is
/// the most that is ever in flight.
const NS_MAX: usize = 2048;

/// The rejoin the paper measured: "the pitch decrease after release is three
/// semitones for the whole keyboard" (Gabrielli et al. 2013). SNAP at its
/// centre is exactly this; the knob scales the semitones.
const REJOIN_SEMITONES: f32 = 3.0;

/// How long the tangent takes to lift clear — the loop reaches its rejoined
/// length over this. A few milliseconds: the rubber tip decompresses and
/// the key rises under the finger; the yarn's choke (five times longer)
/// then takes the note, so what is heard is a dive to the new pitch and a
/// tail at it, not a slide.
const REJOIN_SECS: f32 = 0.003;

/// The fastest the loop may grow, in samples per sample. The wave has to
/// cross the new length: a line lengthening faster than one sample per
/// sample would read samples written BEFORE the tangent lifted, which is
/// silence or the previous note, and either is a hole. Half is a Doppler of
/// one octave at most during the lift, which the choke covers; a low string
/// simply takes longer to rejoin than [`REJOIN_SECS`], as it should.
const REJOIN_MAX_STEP: f32 = 0.5;

/// **The tangent's pulse.** Gabrielli et al. extracted the excitation from
/// recordings and fitted a sixth-order polynomial to its attack, joined to
/// its own reverse; these are their coefficients, highest power first, in
/// samples at their 44.1 kHz. The fit peaks at [`RAMP_PEAK_AT`] and dives
/// negative past 30, so the ramp is the fit from zero to its peak.
///
/// **Referred to its onset.** The fit's value at zero is −0.035 on a peak of
/// 0.158: a boundary residual, since a rubber tip's force on a string it
/// has just reached starts at nothing. Used verbatim that residual is a
/// step of 22 % of the peak at both ends of every pulse, and it was
/// measured, not argued: the fresh strike's first difference tripled
/// (0.040 → 0.120), the click became the loudest thing in the note, and
/// with the level set on that click the ring sat 6 dB under the old
/// table and the wah's follower stayed shut on a single strike. The same
/// polynomial with its onset value subtracted starts and ends at zero,
/// carries 0.39 of its peak as area (the Gaussian it replaces carried
/// 0.35; the verbatim shape 0.23), and keeps the paper's shape between.
const RAMP: [f32; 7] = [-2.69e-8, 2.53e-6, -9.54e-5, 1.74e-3, -1.44e-2, 4.50e-2, -3.50e-2];

/// Where the fit peaks: the ramp's length in the paper's samples. Measured
/// by evaluating the polynomial at every integer from 0 to 30; the test
/// below holds it.
const RAMP_PEAK_AT: f32 = 23.0;

/// The fit's rise from its onset value to its peak — 0.157931 + 0.035 — so
/// the ramp reads one there. A test pins it.
const RAMP_PEAK: f32 = 0.192_931;

/// The tangent-to-anvil distance, in metres: the pulse lasts `d / v`.
///
/// **1.0 mm, and it is the null table that chose it.** A compact pulse has
/// spectral zeros near multiples of its own reciprocal length, and this
/// module has already shipped one pulse whose zero sat on C4's fifth and
/// sixth partials at every velocity. The paper's pulse is measured here
/// (the test below prints the table) at every velocity from MIDI 1 to 127
/// and every VOICING: at 1.0 mm no zero lands on any of C4's first eight
/// partials anywhere — the first one, at the softest voicing and MIDI 1,
/// sits on the ninth, 30 dB under its neighbours. At 1.3 mm it reaches
/// the eighth at MIDI 1; at 2.0 mm the sixth at MIDI 1 and the seventh at
/// MIDI 8. And 1.0 mm is self-consistent with the source: the paper's
/// native 46-sample pulse at 44.1 kHz is 1.04 ms, which is this distance
/// at the slow end of their 1–4 m/s range.
const TANGENT_GAP_M: f32 = 1.0e-3;

/// The tangent's speed at MIDI 1 and MIDI 127, in m/s: the paper's "normally
/// varies linearly in the range 1 to 4 m/s".
const V_SLOW: f32 = 1.0;
const V_FAST: f32 = 4.0;

/// Where the string swings at a mezzo-forte — MIDI 76, 0.6 of full
/// velocity — in gaps of the magnet's exponential: the brief's swing axis,
/// and its reference row, the one that reads H2 −16.6 dB and H3 −37.1 dB
/// on the flux. `DRIVE` puts the instrument there; a test measures the
/// swing at the coil and holds it — which is the only reader, because the
/// number IS the specification and `DRIVE` below is what the voice runs on.
/// Fortissimo then measures 0.8 gaps, not
/// the `0.6 / 0.6^1.3 = 1.2` the level law alone would give, because the
/// pulse SHORTENS as it strengthens — `N = fs·d/v` — and the loop's energy
/// goes as `v^1.3 / (1 + 3v)`; between the 0.6 row and the 1.6 one, H3
/// twelve to twenty dB under the fundamental, which is the brief's own
/// description of what a clavinet's dirt should be.
#[allow(dead_code)]
const SWING_MF: f32 = 0.6;

/// Loop amplitude to gaps. Measured at E4, MIDI 76, with `DRIVE = 1`: the
/// bridge coil's integrator peaks at 0.0572, and `SWING_MF / 0.0572` is
/// this. The chain is transparent to the linear part of the signal
/// whatever this is (see `Pickup::tick`), so it sets the DISTORTION and
/// not the level.
///
/// It was 8.3 against an integrator that peaked at 0.091, which was the
/// excitation before the paper's polynomial pulse replaced the Gaussian:
/// a shorter, differently-shaped pulse puts less amplitude into the loop
/// for the same key speed, and the constant did not follow it, so the
/// string swung 0.475 gaps where this file's own axis says 0.6 and the
/// instrument sat a fifth of the way down its dirt curve from where it
/// was voiced. `the_string_swings_the_reference_gaps_at_the_coil` is what
/// caught it and is what will catch the next excitation change.
const DRIVE: f32 = 10.5;

/// What the whole transducer chain weighs, so that the instrument's LEVEL
/// did not move when its physics did.
///
/// The brief's rule for this wave is that a knob's level at a given
/// velocity must not change — the rework is meant to alter what the
/// clavinet sounds like, not how loud it is, because every preset, every
/// mix and the wah patch's own envelope follower are calibrated against
/// the old one. But the gain of `exp(-x/d)` differentiated and normalised
/// over its swing is an arbitrary number with no reason to land where the
/// old waveshaper's did, and it landed three decibels above it. This is
/// the one constant that puts it back: fitted so a fresh mezzo strike at
/// E4 peaks where the signed-off instrument peaked, measured through
/// `Keys` and not through the voice alone, because that is the level the
/// rest of the engine sees.
const LEVEL: f32 = 0.696;

/// The integrator's leak, in Hz. A pure integrator would hold the strike's
/// displacement step for ever; a leak well under the lowest string (F1,
/// 43.7 Hz) lets it go while leaving the fundamental's integration within
/// two per cent of exact. The loop is DC-free by the tap pair, so the leak
/// is a guard and not a tone control.
const INT_LEAK_HZ: f32 = 8.0;

/// What each coil weighs when the rocker takes both. The average of the
/// two, so the rocker changes the colour and not the fader: two coils that
/// read almost the same fundamental would otherwise land six decibels up
/// on the sum and the wah patch, already at −2 dBFS in `comp`, would clip.
/// The anti-phase pair is the same weight with one sign flipped, so the
/// acceptance measurement — the difference's fundamental against the
/// sum's — is independent of this number.
const PAIR_GAIN: f32 = 0.5;

/// The measured inharmonicity schedule, Gabrielli et al. 2013, eight tones
/// across the keyboard: F1 5e−4, A1 2e−4, D3 9e−5 and 1e−4, F5 9e−5, E6
/// 8e−5. B FALLS with pitch, by six times — the wound bass set is stiff for
/// its pitch and the plain treble wire is not — and it flattens out above
/// the second octave. Fitted by least squares in log B as
/// `B = B_TREBLE · 10^(B_BASS_DEX · exp(−(key − 29) / B_KEYS))`: the treble
/// plateau, the extra decades at F1, and how many keys it takes to shed
/// them. Residuals +1 %, −2 %, −1 %/+10 %, +3 %, −9 %, the two D3 tones
/// straddling the curve. A single log-linear slope misses F1 by a factor of
/// two. The schedule this replaced ROSE with pitch — 6.5e−5 at F1, 3.1e−4
/// at E6 — eight times too low in the bass, four too high at the top; it
/// was the piano's V borrowed for a wire that does not have one.
const B_TREBLE: f32 = 8.76e-5;
const B_BASS_DEX: f32 = 0.7524;
const B_KEYS: f32 = 5.6;
/// The lowest string on the instrument (F1). Below it the schedule holds
/// at the bottom string's stiffness rather than extrapolating the bass
/// rise into a wire the instrument does not have.
const B_KEY_LO: f32 = 29.0;

/// The physical instrument under a pair of patches: where the coils sit
/// and how heavy the wire is. The wah is an effect on top, exactly as the
/// suitcase's tremolo is; this is what differs underneath.
pub struct ClavBase {
    /// The bridge coil's distance from the bridge as a fraction of the
    /// speaking length.
    pub bridge: f32,
    /// The centre coil's, further along the string.
    pub centre: f32,
    /// Multiplier on the measured stiffness schedule: string gauge. At a
    /// fixed pitch and length B goes as the wire's diameter squared, so a
    /// heavier set is a stiffer set.
    pub gauge: f32,
}

impl ClavBase {
    /// The instrument the paper measured. The bridge coil at 21.4 %:
    /// their A2 tone shows notches at 544 Hz and its multiples, every 4.7th
    /// partial, and 1/4.7 is this. The centre coil at 30 % — the two coils
    /// on a D6 are a few centimetres apart at the bridge end, not a coil at
    /// the string's middle: a coil at the middle would notch every even
    /// partial, and its difference with the bridge coil would leave the
    /// fundamental only 6 dB down instead of the 15 measured here, which
    /// is not "damped".
    pub const DRY: ClavBase = ClavBase {
        bridge: 0.165,
        centre: 0.25,
        gauge: 1.0,
    };
    /// The wah instrument: both coils a hair nearer the bridge, so the
    /// notches sit higher and the pedal has more top to sweep through, and
    /// a heavier set of strings — a stiffer wire's top sits a few cents
    /// sharper, which a resonant sweep makes audible where a dry amp does
    /// not.
    pub const WAH: ClavBase = ClavBase {
        bridge: 0.19,
        centre: 0.27,
        gauge: 1.2,
    };
}

/// The measured schedule — see [`B_TREBLE`] — scaled by the base's wire.
fn stiffness(key: f32, gauge: f32) -> f32 {
    (B_TREBLE
        * 10.0_f32.powf(B_BASS_DEX * (-(key.max(B_KEY_LO) - B_KEY_LO) / B_KEYS).exp())
        * gauge)
        .clamp(2.0e-5, 2.0e-3)
}

/// The ramp: the paper's fit referred to its onset value (`RAMP[6]` is
/// the polynomial at zero) and normalised to one at its peak. Six
/// multiplies by Horner, for the length of the pulse only.
fn ramp(u: f32) -> f32 {
    let mut v = 0.0_f32;
    for c in RAMP {
        v = v * u + c;
    }
    (v - RAMP[6]) * (1.0 / RAMP_PEAK)
}

/// The rocker under the PICKUP knob, as `(bridge, centre)` weights. Four
/// positions, hard-edged as the switch is: centre alone, bridge alone,
/// both, both in anti-phase. The knob is read at the strike, and a change
/// of position under a held note is a design change `lands` refuses, so
/// there is nothing for a crossfade to smooth. The shipped 0.35 is the
/// bridge coil — the sound the old sliding coil made at its default, and
/// the sound every record reaches for.
fn rocker(pickup: f32) -> (f32, f32) {
    let p = pickup.clamp(0.0, 1.0);
    if p < 0.25 {
        (0.0, 1.0)
    } else if p < 0.5 {
        (1.0, 0.0)
    } else if p < 0.75 {
        (PAIR_GAIN, PAIR_GAIN)
    } else {
        (PAIR_GAIN, -PAIR_GAIN)
    }
}

/// One coil and the arithmetic in front of it.
///
/// **The order matters, and the brief's §4 says why.** The loop's wave
/// variable is a velocity — the tangent's pulse is a bump, and a struck
/// string's velocity at a point is a train of bumps — so the derivative a
/// coil takes is free. But the magnet's flux is a function of DISTANCE,
/// and a nonlinear function of velocity is not a pickup, it is a
/// waveshaper. So: integrate to the displacement, read the flux there, and
/// let the transducer differentiate. For the linear part the integration
/// and the derivative cancel exactly — `k` carries the note's own `ω₀` in
/// and [`Transducer::tick`] divides it back out — so the chain hands the
/// loop's signal through at unit gain and adds only what the curve adds.
#[derive(Clone, Copy)]
struct Pickup {
    coil: Transducer,
    /// The string's displacement from rest, in gaps.
    x: f32,
    /// The integrator's pole.
    leak: f32,
    /// The integrator's gain, `ω₀ · DRIVE`.
    k: f32,
}

impl Pickup {
    fn new() -> Self {
        let mut coil = Transducer::new(Curve::Exponential { scale: 1.0 });
        // Normalised at zero swing: `1 / |f'(x₀)|`, so a small signal comes
        // through at exactly the loop's level and a large one comes out
        // LOUDER, by as much as the near side of the exponential is
        // steeper. That expansion is the instrument — a real coil has no
        // gain riding on it — and it is never re-measured, so no landing
        // strike can step the level under a ringing note. The brief's
        // Trap 2 guards a compensation computed at the wrong point; a
        // constant has no wrong point, and the test that peak output rises
        // with velocity holds it anyway.
        coil.calibrate(0.0);
        Pickup {
            coil,
            x: 0.0,
            leak: 1.0,
            k: 0.0,
        }
    }

    /// Regulate for a note: the integrator's leak for this rate and its
    /// gain for this pitch.
    fn design(&mut self, omega: f32, rate: f32) {
        self.leak = (-std::f32::consts::TAU * INT_LEAK_HZ / rate).exp();
        self.k = omega * DRIVE;
    }

    /// Silence only. See [`Transducer::clear`].
    fn clear(&mut self) {
        self.x = 0.0;
        self.coil.clear();
    }

    fn tick(&mut self, v: f32, omega: f32) -> f32 {
        self.x = self.x * self.leak + v * self.k;
        if !self.x.is_finite() {
            self.x = 0.0;
        }
        // Minus: the flux falls as the string moves AWAY, so the coil's
        // polarity is the string's inverted, and the loop's polarity is
        // the one every level and asymmetry figure was measured on.
        -self.coil.tick(self.x, omega) * (1.0 / DRIVE)
    }
}

/// One key of the clav.
pub struct ClavVoice {
    rate: f32,
    string: Str,
    /// The loop's whole period at the strike, in samples: what the rejoin
    /// lengthens.
    total: f32,
    omega: f32,
    /// The tangent's pulse: samples left to pour, its length, and the
    /// step through the ramp's argument per sample.
    pulse: u32,
    pulse_n: u32,
    pulse_dt: f32,
    pulse_amp: f32,
    /// Where along the string the tangent lands, in samples back.
    strike_at: usize,
    /// The coils' shared delay store: one history, two taps.
    comb: [f32; COMB_MAX],
    comb_at: usize,
    tap_bridge: usize,
    tap_centre: usize,
    comb_g: f32,
    /// The rocker's weights, `(bridge, centre)`.
    mix: (f32, f32),
    bridge: Pickup,
    centre: Pickup,
    /// The tone rocker: two matched poles over the coils' output. Two, not
    /// one — a clav's spectrum is over by 2 kHz, and one pole's 6 dB an
    /// octave measured under 4 dB of change on anything that was actually
    /// sounding. Twelve is a rocker you can hear land.
    tone_z: f32,
    tone_z2: f32,
    tone_a: f32,
    /// The yarn's choke: a gain on the string as the coils read it.
    damp: f32,
    damp_to: f32,
    damp_secs: f32,
    /// The non-speaking length, spliced into the loop between release and
    /// silence. While the tangent is down it is not ticked at all.
    ns: [f32; NS_MAX],
    ns_at: usize,
    /// How much of it is in the loop now, in samples, and where it is going.
    ns_len: f32,
    ns_to: f32,
    ns_step: f32,
    /// The loop's own filters, taken over from the string while the line
    /// is spliced in. See `release`.
    ns_loss: Lp,
    ns_disp: [Ap; 4],
    ns_gain: f32,
    rejoining: bool,
    /// The SNAP knob as read at the strike: how many of the paper's three
    /// semitones the rejoin is.
    rejoin: f32,
    env: f32,
    done: bool,
}

/// What a strike decides about the loop and the coils before it touches
/// the string. See `ClavVoice::design`.
struct Design {
    total: f32,
    omega: f32,
    len: f32,
    gain: f32,
    loss_a: f32,
    disp_a: f32,
    strike_at: usize,
    tap_bridge: usize,
    tap_centre: usize,
    mix: (f32, f32),
}

impl ClavVoice {
    pub fn new(sample_rate: f32) -> Self {
        ClavVoice {
            rate: sample_rate,
            string: Str::new(),
            total: 100.0,
            omega: 0.01,
            pulse: 0,
            pulse_n: 0,
            pulse_dt: 0.0,
            pulse_amp: 0.0,
            strike_at: 8,
            comb: [0.0; COMB_MAX],
            comb_at: 0,
            tap_bridge: 16,
            tap_centre: 24,
            comb_g: 0.92,
            mix: (1.0, 0.0),
            bridge: Pickup::new(),
            centre: Pickup::new(),
            tone_z: 0.0,
            tone_z2: 0.0,
            tone_a: 1.0,
            damp: 1.0,
            damp_to: 1.0,
            damp_secs: CHOKE_SECS,
            ns: [0.0; NS_MAX],
            ns_at: 0,
            ns_len: 0.0,
            ns_to: 0.0,
            ns_step: 0.0,
            ns_loss: Lp::default(),
            ns_disp: [Ap::default(); 4],
            ns_gain: 0.0,
            rejoining: false,
            rejoin: 1.0,
            env: 0.0,
            done: true,
        }
    }

    /// Whether a strike of `pitch` with these knobs would land on the
    /// string ringing now rather than have to replace it — the same
    /// question as `PianoVoice::lands`, with two more conditions. The
    /// coils' taps and the rocker: moving a tap or a weight under a
    /// ringing string jumps the output by up to the whole raw signal, so a
    /// PICKUP knob moved between two strikes of one held note is the rare
    /// case the frame retires through a fade instead. And the rejoin: a
    /// string whose non-speaking length is back in the loop is a longer
    /// string than the one being asked for, and taking that length out
    /// again would drop everything in flight in it. The frame fades the
    /// rejoining string out and starts the new note on a fresh voice,
    /// which is what a tangent landing on a string still settling from
    /// its lift does anyway.
    pub fn lands(&self, pitch: u8, knobs: &ClavKnobs) -> bool {
        self.lands_based(pitch, knobs, &ClavBase::DRY)
    }

    pub fn lands_based(&self, pitch: u8, knobs: &ClavKnobs, base: &ClavBase) -> bool {
        !self.done && self.fits(&self.design(pitch, knobs, base))
    }

    fn fits(&self, d: &Design) -> bool {
        !self.rejoining
            && (self.string.len - d.len).abs() <= 1.0
            && self.tap_bridge == d.tap_bridge
            && self.tap_centre == d.tap_centre
            && self.mix == d.mix
    }

    /// Everything a strike decides about the loop and the coils, for this
    /// pitch, these knobs and this base. Pure: reads the rate, writes
    /// nothing.
    fn design(&self, pitch: u8, k: &ClavKnobs, base: &ClavBase) -> Design {
        let f0 = 440.0 * 2.0_f32.powf((f32::from(pitch) - 69.0) / 12.0);
        let total = (self.rate / f0).clamp(4.0, 4000.0);
        let key = f32::from(pitch);
        let omega = std::f32::consts::TAU * f0 / self.rate;

        let b = stiffness(key, base.gauge);

        // Sustain. Shorter than a piano — the anvil end is a rubber tip, not
        // an agraffe — but BRIGHT for its length: steel on steel with no felt
        // anywhere, the top of the series hangs on at nearly half the
        // fundamental's life, which is most of why a held clav note zings
        // where a held piano note sings. The MUTE knob is the sliding damper
        // pad on the real one: it rides the whole curve down to staccato.
        let t60_f = (3.6 * (-(key - 40.0) / 34.0).exp()).clamp(0.35, 4.5)
            * k.decay
            * (1.0 - 0.88 * k.mute.powf(0.8));
        let t60_f = t60_f.clamp(0.06, 12.0);
        let f_hi = (7.0 * f0).min(0.40 * self.rate);
        let t60_h = (t60_f * 0.45).clamp(0.03, t60_f);
        let w1 = omega;
        let w2 = std::f32::consts::TAU * f_hi / self.rate;
        let rho1 = 10.0_f32.powf(-3.0 / (t60_f * f0));
        let rho2 = 10.0_f32.powf(-3.0 / (t60_h * f0));
        let (p, gain) = design_loss(w1, w2, rho1, rho2);

        // The same sqrt(B)-in-delay dispersion fit as the piano's — see
        // string.rs for the design and for the inversion this replaced. It
        // matters MORE here: the old map turned this string's small B into a
        // huge coefficient, which parked a long-ringing upper series 25 to
        // 122 cents sharp — an enharmonic bell on an instrument whose whole
        // brightness is supposed to be combs over a harmonic series. With the
        // fit the partials sit within a few cents of the stretched ideal at
        // every velocity, and a test in mod.rs pins that.
        //
        // The piano's two caps, kept, and the 19 now BINDS: the measured
        // schedule asks 46 samples of per-section delay at F1 and 23 at
        // A1, so the bottom fifth of the keyboard rides the cap. Measured
        // (`the_bass_partials_report_against_the_measured_stretch`): at F1
        // the partials sit within 2 cents of the stretch the cap realises
        // and 2 to 20 cents FLAT of Gabrielli's series at n2..n8; at A1 the
        // miss is 3 cents at n8. Raising the cap to 64 does not fix it — it
        // lands n8 41 cents SHARP at F1 and 11 at A1 — because the
        // sqrt(B)-in-delay map is a small-tau fit made at C4, and at tau·ω
        // near one the all-pass's delay falls off inside the first few
        // partials of a 44 Hz string. The map is the string lane's; the
        // correction for large tau is theirs to make, and until it is made
        // a cap that undershoots by twenty cents is preferred to a free
        // coefficient that overshoots by forty.
        let disp_tau = (1.8625 * b.sqrt() * total)
            .clamp(1.0, (0.14 * total).min(19.0).max(1.0));
        let disp_a = (1.0 - disp_tau) / (1.0 + disp_tau);
        let phase_delay = loop_phase_delay(w1, p, disp_a);
        let len = (total - phase_delay).max(4.0);

        // The tangent lands close to the anvil end — a tenth of the string,
        // a touch nearer in the treble — which is why even the strike is
        // toppy before the coils have said anything.
        let ratio = if key < 76.0 { 10.0 } else { 9.0 };
        let strike_at = ((total / ratio).round() as usize).clamp(1, 4000);

        // **The coils.** Each one's distance from the bridge, as a fraction
        // of the string, sets its comb: close is glassy (notches high and
        // out of the way), far is hollow (the low harmonics start folding).
        // Where they sit is the instrument's, not the player's; the rocker
        // is the player's.
        let tap_bridge = ((total * base.bridge).round() as usize).clamp(1, COMB_MAX - 1);
        let tap_centre = ((total * base.centre).round() as usize).clamp(1, COMB_MAX - 1);
        let mix = rocker(k.pickup);

        Design {
            total,
            omega,
            len,
            gain,
            loss_a: 1.0 - p,
            disp_a,
            strike_at,
            tap_bridge,
            tap_centre,
            mix,
        }
    }

    /// The dry patch: `strike_based` through [`ClavBase::DRY`].
    ///
    /// One of the strike ladder's rungs that the instrument does not use:
    /// `mod.rs` calls the base-picking one. This shorter spelling is what
    /// `strike_knobs_is_the_dry_base` below and `mod.rs`'s own partial test
    /// strike with.
    #[allow(dead_code)]
    pub fn strike_knobs(&mut self, pitch: u8, velocity: f32, k: &ClavKnobs) {
        self.strike_based(pitch, velocity, k, &ClavBase::DRY);
    }

    pub fn strike_based(&mut self, pitch: u8, velocity: f32, k: &ClavKnobs, base: &ClavBase) {
        let v = velocity.clamp(0.0, 1.0);
        let d = self.design(pitch, k, base);

        // **A tangent landing on a string that is still moving adds to the
        // motion** — the piano's rule, borrowed with the loop; see
        // `PianoVoice::strike_full` for the argument. The combs, the coils
        // and the tone rockers are part of the signal path and keep their
        // memory; only a string of another design is cleared, and the frame
        // keeps that from being audible.
        let landing = !self.done && self.fits(&d);
        if landing && self.damp < 1.0 {
            // The yarn's take so far, made real in the string and the comb
            // store — everything the choke gain sits AFTER — so that the
            // coils, which are not linear and cannot be scaled, see the
            // same displacement on the next sample as on the last.
            self.string.scale(self.damp);
            for c in &mut self.comb {
                *c *= self.damp;
            }
        }
        for ap in &mut self.string.disp {
            ap.a = d.disp_a;
        }
        self.string.loss.a = d.loss_a;
        self.string.gain = d.gain;
        self.string.len = d.len;
        self.strike_at = d.strike_at;
        self.total = d.total;
        self.omega = d.omega;
        if !landing {
            self.string.clear();
            self.rejoining = false;
            self.ns_len = 0.0;
            self.ns_to = 0.0;
        }

        // **The pulse.** Rubber flattening against metal, the paper's shape,
        // lasting the time the tangent takes to cross the gap at its speed:
        // `N = fs · d / v`, `v` linear from 1 m/s at MIDI 1 to 4 m/s at 127.
        // VOICING is the tip's compound, worn soft to fresh hard — a softer
        // tip is a longer flattening — and its 1.3× span is the range the
        // null table was measured over.
        let tip = 1.30_f32.powf(1.0 - 2.0 * k.voicing.clamp(0.0, 1.0));
        let v_ms = V_SLOW + (V_FAST - V_SLOW) * v;
        let n = ((self.rate * TANGENT_GAP_M * tip / v_ms).round() as u32).max(3);
        self.pulse = n;
        self.pulse_n = n;
        self.pulse_dt = 1.0 / (n - 1) as f32;
        // The level law is the one the bench signed off; the constant is
        // re-levelled for this pulse's area — the ring of a C4 at MIDI 100
        // measured against the Gaussian's, the sustained level rather than
        // the attack's peak, because the ring is the note and it is what
        // the wah's follower has to open on. 0.52 matched the ring to the
        // old table within a decibel; it is 0.41 because the paper's
        // 0.25 ms fortissimo pulse carries a click 7 dB more prominent
        // than the Gaussian's, and at 0.52 that click put the wah patch's
        // `comp` render at +0.6 dBFS (the old table: −2.0). Two decibels
        // of ring bought the headroom back; the click is the clack the
        // owner asked for and stays.
        self.pulse_amp = 0.41 * v.powf(1.3);

        self.tap_bridge = d.tap_bridge;
        self.tap_centre = d.tap_centre;
        self.comb_g = 0.92;
        self.mix = d.mix;
        self.bridge.design(d.omega, self.rate);
        self.centre.design(d.omega, self.rate);
        if !landing {
            self.comb.fill(0.0);
            self.comb_at = 0;
            self.bridge.clear();
            self.centre.clear();
        }

        // The tone rockers: brilliant leaves the wire alone, soft is the
        // mellow switch pair down. 8010 Hz is precisely where the old
        // 1500..14000 sweep put the shipped 0.75 default, so an untouched
        // strip keeps its corner to the hertz; the five-octave span around
        // it reaches 600 Hz at the bottom — INTO the sounding series, where
        // the old 1500 floor stopped a full octave above a spectrum that is
        // over by 2 kHz — and an effectively-open 19 kHz at the top.
        let corner = 8_010.0 * 2.0_f32.powf((k.tone.clamp(0.0, 1.0) - 0.75) * 5.0);
        self.tone_a = 1.0 - (-std::f32::consts::TAU * corner / self.rate).exp();
        if !landing {
            self.tone_z = 0.0;
            self.tone_z2 = 0.0;
        }

        self.damp = 1.0;
        self.damp_to = 1.0;
        self.damp_secs = (CHOKE_SECS * k.release).clamp(0.003, 0.25);
        self.rejoin = k.snap;
        // The follower keeps whichever is louder: it is the stealing
        // policy's view of the voice, and a soft strike on a loud note has
        // not made the note soft.
        self.env = if landing { self.env.max(self.pulse_amp) } else { self.pulse_amp };
        self.done = false;
    }

    /// Silence now: every state cleared, `finished` true — the voice a
    /// first strike can land on as if from `new`. See `PianoVoice::silence`.
    pub fn silence(&mut self) {
        self.string.clear();
        self.comb.fill(0.0);
        self.comb_at = 0;
        self.bridge.clear();
        self.centre.clear();
        self.tone_z = 0.0;
        self.tone_z2 = 0.0;
        self.pulse = 0;
        self.damp = 1.0;
        self.damp_to = 1.0;
        self.rejoining = false;
        self.ns_len = 0.0;
        self.ns_to = 0.0;
        self.env = 0.0;
        self.done = true;
    }

    /// The tangent lifts. The yarn chokes the string AND the string gets
    /// its far length back: the two halves rejoin and the loop grows by
    /// `2^(snap · 3 / 12)`.
    ///
    /// **How a frozen loop grows.** `Str` is a closed loop — read, filter,
    /// write — and its length is a read position; the paper's model adds a
    /// second delay line, and this one does too, without touching the
    /// string. From here on the string's own gain is zero and its all-pass
    /// memory is empty, so `Str::tick` writes exactly what it is handed and
    /// nothing else; the loss and the dispersion, state and all, move into
    /// this voice, and they feed the non-speaking line, whose output goes
    /// into the string's head. At a length of zero that is bit-for-bit
    /// what the string was about to write itself — the copy has the same
    /// state and sees the same sample — and then the length glides up over
    /// [`REJOIN_SECS`]. A resonator taking new coefficients: nothing in the
    /// loop is dropped, nothing silent is spliced in, and the output is
    /// continuous through the lift; the test below holds the first
    /// millisecond of the release to the ringing signal's own slope.
    pub fn release(&mut self, pedal: bool) {
        if pedal || self.done || self.damp_to == 0.0 {
            return;
        }
        self.damp_to = 0.0;
        let ratio = 2.0_f32.powf(self.rejoin.clamp(0.0, 2.0) * REJOIN_SEMITONES / 12.0);
        let extra = (self.total * (ratio - 1.0)).min((NS_MAX - 2) as f32);
        // SNAP at zero: the tangent does not lift clear — a string choked at
        // pitch, which is what a felt mute under the strings would do.
        if extra < 0.5 {
            return;
        }
        self.ns_loss = self.string.loss;
        self.ns_disp = self.string.disp;
        self.ns_gain = self.string.gain;
        self.string.gain = 0.0;
        for d in &mut self.string.disp {
            d.x1 = 0.0;
            d.y1 = 0.0;
        }
        self.ns_len = 0.0;
        self.ns_to = extra;
        self.ns_step = (extra / (REJOIN_SECS * self.rate).max(1.0)).min(REJOIN_MAX_STEP);
        self.rejoining = true;
    }

    pub fn damp(&mut self) {
        // The panic path and the patch change: choke without the lift.
        self.damp_to = 0.0;
    }

    pub fn finished(&self) -> bool {
        self.done
    }

    pub fn level(&self) -> f32 {
        self.env
    }

    /// The non-speaking line `d` samples behind the sample just written,
    /// linearly interpolated — the same read the string does.
    fn ns_read(&self, d: f32) -> f32 {
        let i = d.floor();
        let frac = d - i;
        let back = i as usize;
        let a = self.ns[(self.ns_at + NS_MAX - back) % NS_MAX];
        let b = self.ns[(self.ns_at + NS_MAX - back - 1) % NS_MAX];
        a + (b - a) * frac
    }

    pub fn tick(&mut self) -> f32 {
        if self.done {
            return 0.0;
        }
        let mut inject = 0.0_f32;
        if self.pulse > 0 {
            let i = self.pulse_n - self.pulse;
            let t = i as f32 * self.pulse_dt;
            // Up the ramp and back down its reverse.
            let u = RAMP_PEAK_AT * (1.0 - (2.0 * t - 1.0).abs());
            inject = self.pulse_amp * ramp(u);
            self.pulse -= 1;
        }

        // The tap pair, exactly the piano's: the difference is the strike
        // comb, and summing to zero keeps the loop DC-free.
        if inject != 0.0 {
            self.string.excite(self.strike_at, -inject);
        }
        let raw = if self.rejoining {
            // The loop's filters, run here on the sample the string is
            // about to hand out, into the line and back to the head.
            let out = self.string.read(self.string.len);
            let mut v = self.ns_loss.tick(out) * self.ns_gain;
            for d in &mut self.ns_disp {
                v = d.tick(v);
            }
            if !v.is_finite() {
                v = 0.0;
            }
            self.ns[self.ns_at] = v;
            let fed = self.ns_read(self.ns_len);
            self.ns_at = (self.ns_at + 1) % NS_MAX;
            if self.ns_len < self.ns_to {
                self.ns_len = (self.ns_len + self.ns_step).min(self.ns_to);
            }
            self.string.tick(inject + fed)
        } else {
            self.string.tick(inject)
        };

        // Each coil's point reading: now minus a while ago. The yarn's
        // choke is a gain on the string here, before the coils, because it
        // is the STRING the yarn takes and the coil reads what is left.
        let coeff = 1.0 - (-1.0 / (self.damp_secs * self.rate)).exp();
        self.damp += (self.damp_to - self.damp) * coeff;
        let behind_b = self.comb[(self.comb_at + COMB_MAX - self.tap_bridge) % COMB_MAX];
        let behind_c = self.comb[(self.comb_at + COMB_MAX - self.tap_centre) % COMB_MAX];
        self.comb[self.comb_at] = raw;
        self.comb_at = (self.comb_at + 1) % COMB_MAX;
        let seen_b = (raw - self.comb_g * behind_b) * self.damp;
        let seen_c = (raw - self.comb_g * behind_c) * self.damp;

        // The magnets, then the rocker.
        let ob = self.bridge.tick(seen_b, self.omega);
        let oc = self.centre.tick(seen_c, self.omega);
        let picked = self.mix.0 * ob + self.mix.1 * oc;

        // Tone rocker — both poles.
        self.tone_z += self.tone_a * (picked - self.tone_z);
        self.tone_z2 += self.tone_a * (self.tone_z - self.tone_z2);
        let out = self.tone_z2 * LEVEL;
        let out = if out.is_finite() { out.clamp(-4.0, 4.0) } else { 0.0 };

        let a = out.abs();
        self.env += (a - self.env) * if a > self.env { 0.4 } else { 0.0006 };
        if self.env < 2.0e-5 {
            self.done = true;
            self.string.clear();
            self.comb.fill(0.0);
            self.bridge.clear();
            self.centre.clear();
            self.tone_z = 0.0;
            self.tone_z2 = 0.0;
            self.rejoining = false;
            self.ns_len = 0.0;
            self.ns_to = 0.0;
        }
        out
    }

    /// The two coils' displacements, in gaps — for the test that pins the
    /// swing to the brief's axis.
    #[cfg(test)]
    fn displacement(&self) -> (f32, f32) {
        (self.bridge.x, self.centre.x)
    }

    /// The pulse as it would be poured for this velocity and voicing, for
    /// the null table. Pure.
    #[cfg(test)]
    fn pulse_shape(rate: f32, velocity: f32, voicing: f32) -> Vec<f32> {
        let tip = 1.30_f32.powf(1.0 - 2.0 * voicing.clamp(0.0, 1.0));
        let v_ms = V_SLOW + (V_FAST - V_SLOW) * velocity.clamp(0.0, 1.0);
        let n = ((rate * TANGENT_GAP_M * tip / v_ms).round() as u32).max(3);
        let dt = 1.0 / (n - 1) as f32;
        (0..n)
            .map(|i| {
                let t = i as f32 * dt;
                ramp(RAMP_PEAK_AT * (1.0 - (2.0 * t - 1.0).abs()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const RATE: f32 = 48_000.0;
    const C4: f32 = 261.625_55;

    /// Magnitude at one frequency through a Hann window — the bench's
    /// measuring stick, because a rectangular window's leakage floor pins
    /// noise instead of partials.
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

    /// The strongest magnitude within 5 % of `around`: dispersion moves a
    /// real partial sharp of the ideal, and a pinned probe reads the gap.
    fn search_peak(x: &[f32], around: f32) -> (f32, f32) {
        let mut best = (0.0_f32, 0.0_f32);
        let mut f = around * 0.95;
        while f < around * 1.05 {
            let g = goertzel_hann(x, f, RATE);
            if g > best.1 {
                best = (f, g);
            }
            f += around * 0.001;
        }
        best
    }

    fn db(x: f32) -> f32 {
        20.0 * x.max(1e-30).log10()
    }

    fn knobs() -> ClavKnobs {
        ClavKnobs::default()
    }

    /// One voice, struck once, ticked for `secs`, the mechanism alone.
    fn strike_and_run(pitch: u8, vel: f32, k: &ClavKnobs, base: &ClavBase, secs: f32) -> Vec<f32> {
        let mut v = ClavVoice::new(RATE);
        v.strike_based(pitch, vel, k, base);
        (0..(secs * RATE) as usize).map(|_| v.tick()).collect()
    }

    /// The paper's fit, held to its own arithmetic: one at the peak, the
    /// peak at 23, and zero at the origin — the onset residual referred
    /// out, see `RAMP`.
    #[test]
    fn the_ramp_is_the_papers_polynomial_normalised_at_its_peak() {
        assert!((ramp(RAMP_PEAK_AT) - 1.0).abs() < 1e-4, "ramp(23) = {}", ramp(RAMP_PEAK_AT));
        let argmax = (0..=30)
            .map(|u| (u, ramp(u as f32)))
            .fold((0, f32::MIN), |m, c| if c.1 > m.1 { c } else { m })
            .0;
        assert_eq!(argmax, 23, "the fit does not peak where RAMP_PEAK_AT says");
        assert!(ramp(0.0).abs() < 1e-6, "the onset is {}, not zero", ramp(0.0));
    }

    /// The spectrum of the pulse itself at C4's partials, in dB re its DC.
    fn pulse_spectrum_db(pulse: &[f32], n_max: usize) -> Vec<f64> {
        let dc: f64 = pulse.iter().map(|&v| v as f64).sum();
        (1..=n_max)
            .map(|n| {
                let w = TAU * (n as f64) * C4 as f64 / RATE as f64;
                let (mut re, mut im) = (0.0_f64, 0.0_f64);
                for (i, &v) in pulse.iter().enumerate() {
                    re += v as f64 * (w * i as f64).cos();
                    im -= v as f64 * (w * i as f64).sin();
                }
                20.0 * (re.hypot(im) / dc.abs().max(1e-30)).log10()
            })
            .collect()
    }

    /// **The null table.** The Gaussian this pulse replaced was there for a
    /// measured reason — a raised cosine once put a spectral zero on C4's
    /// fifth and sixth partials — so the paper's pulse is measured the same
    /// way before it is trusted: its spectrum at C4's first ten partials,
    /// at every velocity from MIDI 1 to 127 and at both ends of VOICING.
    /// A null is a partial more than 8 dB under BOTH its neighbours, which
    /// is what a zero landing on a partial looks like and what a smooth
    /// roll-off never does. None on the first eight, anywhere; the first
    /// zero to reach the series — softest voicing, MIDI 1 — sits on the
    /// ninth at −49 dB. `TANGENT_GAP_M` is the number that makes this true,
    /// and the revert is one digit: 2.0 mm puts a zero on the sixth.
    #[test]
    fn the_pulse_has_no_null_on_c4s_first_eight_partials_at_any_velocity() {
        println!("pulse spectrum at C4's partials, dB re DC (n1..n10)");
        for voicing in [0.0_f32, 0.5, 1.0] {
            for midi in [1u8, 4, 8, 16, 32, 64, 100, 127] {
                let pulse = ClavVoice::pulse_shape(RATE, f32::from(midi) / 127.0, voicing);
                let s = pulse_spectrum_db(&pulse, 10);
                let row: Vec<String> = s.iter().map(|v| format!("{v:6.1}")).collect();
                println!("  voicing {voicing:.1} midi {midi:3} N={:3}  {}", pulse.len(), row.join(" "));
                for n in 1..=8usize {
                    let here = s[n - 1];
                    let lo = if n == 1 { 0.0 } else { s[n - 2] };
                    let hi = s[n];
                    assert!(
                        !(here < lo - 8.0 && here < hi - 8.0),
                        "voicing {voicing} MIDI {midi}: partial {n} at {here:.1} dB sits in a null \
                         ({lo:.1} below, {hi:.1} above)"
                    );
                }
            }
        }
    }

    /// The measured schedule, held to the paper's six tones within the
    /// fit's own worst residual (10 %, one of the two D3 tones), and
    /// falling — never rising — from the bottom of the keyboard to the top.
    #[test]
    fn the_b_schedule_falls_with_pitch_as_gabrielli_measured() {
        for (key, want) in [(29.0_f32, 5e-4_f32), (33.0, 2e-4), (50.0, 9e-5), (50.0, 1e-4), (77.0, 9e-5), (88.0, 8e-5)] {
            let got = stiffness(key, 1.0);
            let ratio = got / want;
            println!("  key {key}: B {got:.2e} measured {want:.1e} ({:+.0} %)", (ratio - 1.0) * 100.0);
            assert!(
                (0.88..=1.12).contains(&ratio),
                "key {key}: B {got:.2e} against the measured {want:.1e}"
            );
        }
        let mut prev = f32::MAX;
        for key in 29..=88u8 {
            let b = stiffness(f32::from(key), 1.0);
            assert!(b <= prev * 1.000_01, "B rises from {prev:.3e} to {b:.3e} at key {key}");
            prev = b;
        }
        assert!(stiffness(29.0, 1.0) / stiffness(88.0, 1.0) > 4.0, "the schedule spans less than the measured six times");
        assert!(stiffness(60.0, 1.2) > stiffness(60.0, 1.0), "a heavier gauge is not stiffer");
    }

    /// Where the bass partials actually land against the measured
    /// stretch, and against the stretch the dispersion cap lets through.
    /// Printed for the record at F1, A1, F2 and D3 — the schedule asks for
    /// a per-section delay of 46 samples at F1 and the cap allows 19, so
    /// the bottom fifth rides the cap (see `design`) — and HELD on the
    /// realised stretch: every partial to n8 within 8 cents of the series
    /// the design's own coefficient makes, which is the loop's tuning.
    /// Revert-checked by leaving the filters' delay out of `len`.
    #[test]
    fn the_bass_partials_report_against_the_measured_stretch() {
        for key in [29u8, 33, 41, 50] {
            let f0 = 440.0 * 2.0_f32.powf((f32::from(key) - 69.0) / 12.0);
            let v = ClavVoice::new(RATE);
            let d = v.design(key, &knobs(), &ClavBase::DRY);
            let tau = (1.0 - d.disp_a) / (1.0 + d.disp_a);
            let b_real = (tau / (1.8625 * d.total)).powi(2);
            let b_want = stiffness(f32::from(key), 1.0);
            let out = strike_and_run(key, 0.7, &knobs(), &ClavBase::DRY, 1.2);
            let w = &out[(0.1 * RATE) as usize..];
            let mut line = format!(
                "  key {key}: tau {tau:.1} (asked {:.1}), B realised {b_real:.1e} of {b_want:.1e}; cents vs measured / realised:",
                1.8625 * b_want.sqrt() * d.total
            );
            for n in 2..=8u32 {
                let (f, g) = search_peak(w, n as f32 * f0);
                if db(g) < -100.0 {
                    line.push_str(&format!("  n{n} -"));
                    continue;
                }
                let want = n as f32 * f0 * (1.0 + b_want * (n * n) as f32).sqrt();
                let real = n as f32 * f0 * (1.0 + b_real * (n * n) as f32).sqrt();
                let off_real = 1200.0 * (f / real).log2();
                line.push_str(&format!("  n{n} {:+.0}/{off_real:+.0}", 1200.0 * (f / want).log2()));
                assert!(
                    off_real.abs() <= 8.0,
                    "key {key} partial {n} sits {off_real:+.0} cents off the stretch the design realises"
                );
            }
            println!("{line}");
        }
    }

    /// **Acceptance D.** "When the pickups signal is summed in anti-phase
    /// the fundamental is damped and the resulting timbre is thin." At A2,
    /// the difference's fundamental against the sum's: ≥ 10 dB down, and
    /// with the coils where `ClavBase::DRY` puts them it measures −15.
    #[test]
    fn the_anti_phase_sum_damps_the_fundamental_at_a2() {
        let f0 = 110.0_f32;
        let level = |pickup: f32| {
            let mut k = knobs();
            k.pickup = pickup;
            let out = strike_and_run(45, 0.7, &k, &ClavBase::DRY, 0.7);
            let w = &out[(0.1 * RATE) as usize..];
            db(search_peak(w, f0).1)
        };
        let both = level(0.6);
        let anti = level(0.9);
        println!("  A2 fundamental: in phase {both:+.1} dB, anti-phase {anti:+.1} dB, {:+.1} dB", anti - both);
        assert!(
            anti <= both - 10.0,
            "the anti-phase fundamental is only {:.1} dB under the in-phase one",
            both - anti
        );
    }

    /// The rejoin's first millisecond has no step larger than the ringing
    /// signal's own slope: the largest one-sample difference in the 1 ms
    /// after the lift against the largest over two periods before it, at
    /// three pitches and both ends of SNAP. A lengthening line plays its history
    /// SLOWER, so an honest rejoin can only flatten the slope; a splice —
    /// the line at full length at once, revert-checked — reads a step.
    #[test]
    fn the_release_rejoins_without_a_step() {
        for pitch in [36u8, 60, 84] {
            for snap in [1.0_f32, 2.0] {
                let mut k = knobs();
                k.snap = snap;
                let mut v = ClavVoice::new(RATE);
                v.strike_based(pitch, 0.8, &k, &ClavBase::DRY);
                let hold = (0.3 * RATE) as usize;
                let mut out: Vec<f32> = (0..hold).map(|_| v.tick()).collect();
                v.release(false);
                assert!(v.rejoining, "SNAP {snap} did not splice the far length in");
                out.extend((0..(0.020 * RATE) as usize).map(|_| v.tick()));
                let d1 = |a: usize, b: usize| {
                    (a.max(1)..b).fold(0.0_f32, |m, n| m.max((out[n] - out[n - 1]).abs()))
                };
                // The ringing signal's own slope needs a whole waveform to
                // show itself: two periods, never less than 8 ms.
                let f0 = 440.0 * 2.0_f32.powf((f32::from(pitch) - 69.0) / 12.0);
                let span = ((2.0 * RATE / f0) as usize).max((0.008 * RATE) as usize);
                let before = d1(hold - span, hold);
                let after = d1(hold, hold + (0.001 * RATE) as usize);
                println!("  key {pitch} snap {snap}: slope before {before:.5}, first ms after {after:.5}");
                assert!(before > 1e-4, "key {pitch}: nothing was ringing to lift off");
                assert!(
                    after <= before,
                    "key {pitch} snap {snap}: the release stepped {after:.5} against a slope of {before:.5}"
                );
            }
        }
    }

    /// The drop is the paper's three semitones at SNAP centre, none at
    /// SNAP zero, and six at the top — measured on the tail with the choke
    /// slowed (RELEASE up) so there is a tail to measure: the fundamental
    /// found between 30 and 80 ms after the lift, in cents from the held
    /// note's.
    #[test]
    fn the_release_drops_three_semitones_at_snap_centre() {
        let f0 = C4;
        for (snap, want_cents) in [(0.0_f32, 0.0_f32), (1.0, -300.0), (2.0, -600.0)] {
            let mut k = knobs();
            k.snap = snap;
            k.release = 3.0;
            let mut v = ClavVoice::new(RATE);
            v.strike_based(60, 0.8, &k, &ClavBase::DRY);
            for _ in 0..(0.3 * RATE) as usize {
                v.tick();
            }
            v.release(false);
            let tail: Vec<f32> = (0..(0.10 * RATE) as usize).map(|_| v.tick()).collect();
            let w = &tail[(0.030 * RATE) as usize..(0.080 * RATE) as usize];
            // Search a wide band: the whole drop is in play.
            let mut best = (0.0_f32, 0.0_f32);
            let mut f = f0 * 0.6;
            while f < f0 * 1.1 {
                let g = goertzel_hann(w, f, RATE);
                if g > best.1 {
                    best = (f, g);
                }
                f *= 1.001;
            }
            let cents = 1200.0 * (best.0 / f0).log2();
            println!("  snap {snap}: tail fundamental {:.1} Hz, {cents:+.0} cents", best.0);
            assert!(
                (cents - want_cents).abs() < 25.0,
                "snap {snap}: the tail sits {cents:+.0} cents off, wanted {want_cents:+.0}"
            );
        }
    }

    /// A string in its rejoin refuses to land — the frame fades it and the
    /// new note starts fresh — while a string choked at pitch (SNAP zero)
    /// lands as it always did.
    #[test]
    fn a_strike_does_not_land_on_a_rejoining_string() {
        for (snap, want) in [(1.0_f32, false), (0.0, true)] {
            let mut k = knobs();
            k.snap = snap;
            let mut v = ClavVoice::new(RATE);
            v.strike_based(60, 0.8, &k, &ClavBase::DRY);
            for _ in 0..(0.2 * RATE) as usize {
                v.tick();
            }
            assert!(v.lands(60, &k), "a held note does not land on itself");
            v.release(false);
            for _ in 0..(0.002 * RATE) as usize {
                v.tick();
            }
            assert_eq!(v.lands(60, &k), want, "snap {snap}: lands() during the release");
        }
    }

    /// Harmonics 1..=k of `f0` in `sig`, dB re the first, Hann window.
    fn harmonics_db(sig: &[f32], f0: f32, k: usize) -> Vec<f32> {
        let h1 = goertzel_hann(sig, f0, RATE).max(1e-30);
        (1..=k).map(|h| db(goertzel_hann(sig, f0 * h as f32, RATE) / h1)).collect()
    }

    /// **The pickup chain reads the brief's table.** A `Pickup` driven the
    /// way the voice drives it — a sinusoidal velocity at 220 Hz whose
    /// integrated displacement swings 0.6 gaps — puts out the §4.1 clavinet
    /// row TILTED by the derivative: the row is `pickup.py` reading the
    /// flux with no derivative (H2 −16.6, H3 −37.1), and a coil's output
    /// is the flux's time derivative, which adds `20·log10(h)`: H2 −10.6,
    /// H3 −27.6, held to 1 dB. The displacement itself, read through the
    /// flux alone, is the row as printed, held to 1 dB — that is the proof
    /// the swing really is 0.6 gaps and the curve really is the table's.
    #[test]
    fn the_pickup_chain_reads_the_brief_table_at_swing_0_6() {
        let f0 = 220.0_f32;
        let omega = std::f32::consts::TAU * f0 / RATE;
        let mut p = Pickup::new();
        p.design(omega, RATE);
        // A velocity of amplitude a integrates to a displacement of
        // a · DRIVE at f0; ask for 0.6 gaps.
        let amp = 0.6 / DRIVE;
        let n = 33_600; // 154 whole periods of 220 Hz at 48 kHz
        let mut out = Vec::with_capacity(n);
        let mut x = Vec::with_capacity(n);
        let mut flux = Vec::with_capacity(n);
        for i in 0..n {
            let v = amp * (TAU * f0 as f64 * i as f64 / RATE as f64).sin() as f32;
            out.push(p.tick(v, omega));
            x.push(p.x);
            flux.push(p.coil.flux(p.x));
        }
        // Over the settled half: a sinusoid starting from rest integrates
        // to `1 − cos`, a transient the leak takes a few hundred samples
        // to let go of.
        let swing = x[n / 2..].iter().fold(0.0_f32, |m, &v| m.max(v.abs()));
        println!("  displacement swing {swing:.3} gaps");
        assert!((swing - 0.6).abs() < 0.03, "the swing is {swing}, not 0.6");

        let fl = harmonics_db(&flux[n / 2..], f0, 3);
        let ou = harmonics_db(&out[n / 2..], f0, 3);
        println!("  flux  H2 {:+.1} H3 {:+.1}   brief −16.6 −37.1", fl[1], fl[2]);
        println!("  out   H2 {:+.1} H3 {:+.1}   tilted −10.6 −27.6", ou[1], ou[2]);
        assert!((fl[1] + 16.6).abs() <= 1.0, "flux H2 {:+.1}", fl[1]);
        assert!((fl[2] + 37.1).abs() <= 1.0, "flux H3 {:+.1}", fl[2]);
        assert!((ou[1] + 10.6).abs() <= 1.0, "out H2 {:+.1}", ou[1]);
        assert!((ou[2] + 27.6).abs() <= 1.0, "out H3 {:+.1}", ou[2]);
        // And the linear part is handed through at unit gain: a tiny
        // swing comes out at the velocity's own amplitude.
        let mut q = Pickup::new();
        q.design(omega, RATE);
        let tiny = 0.001;
        let peak = (0..4800)
            .map(|i| q.tick(tiny * (TAU * f0 as f64 * i as f64 / RATE as f64).sin() as f32, omega))
            .skip(2400)
            .fold(0.0_f32, |m, v| m.max(v.abs()));
        assert!((peak / tiny - 1.0).abs() < 0.03, "unit gain: {tiny} in, {peak} out");
    }

    /// The swing the instrument makes is the brief's axis: at E4, MIDI 76,
    /// the bridge coil sees `SWING_MF` gaps. This pins `DRIVE` to a
    /// measurement; fortissimo is printed and held loosely, because it is
    /// the level law's business and not the coil's.
    #[test]
    fn the_string_swings_the_reference_gaps_at_the_coil() {
        let swing_at = |vel: f32| {
            let mut v = ClavVoice::new(RATE);
            v.strike_based(64, vel, &knobs(), &ClavBase::DRY);
            (0..(0.25 * RATE) as usize)
                .map(|_| {
                    v.tick();
                    v.displacement().0.abs()
                })
                .fold(0.0_f32, f32::max)
        };
        let mf = swing_at(76.0 / 127.0);
        let ff = swing_at(1.0);
        let pp = swing_at(8.0 / 127.0);
        println!("  swing at MIDI 8: {pp:.3} gaps; MIDI 76: {mf:.3}; MIDI 127: {ff:.3}");
        assert!((mf / SWING_MF - 1.0).abs() < 0.10, "MIDI 76 swings {mf:.3} gaps, not {SWING_MF}");
        assert!(ff > mf && ff < 1.6, "MIDI 127 swings {ff:.3} gaps");
    }

    /// Trap 2's bench assertion: peak output rises monotonically with
    /// velocity, on both bases, over the velocity table's twelve strikes.
    /// Also prints, for the record, the second and third harmonics against
    /// the fundamental at E4 in the brief's §1.2 window, soft and hard.
    #[test]
    fn peak_output_rises_monotonically_with_velocity() {
        for (label, base) in [("dry", &ClavBase::DRY), ("wah", &ClavBase::WAH)] {
            let mut prev = 0.0_f32;
            for i in 0..12 {
                let midi = (8 + i * 11).min(127);
                let out = strike_and_run(64, midi as f32 / 127.0, &knobs(), base, 0.25);
                let peak = out.iter().fold(0.0_f32, |m, &v| m.max(v.abs()));
                assert!(peak > prev, "{label}: peak fell to {peak} at MIDI {midi} from {prev}");
                prev = peak;
            }
        }
        let f0 = 440.0 * 2.0_f32.powf((64.0 - 69.0) / 12.0);
        for midi in [8u8, 127] {
            let out = strike_and_run(64, f32::from(midi) / 127.0, &knobs(), &ClavBase::DRY, 0.55);
            let w = &out[(0.12 * RATE) as usize..(0.52 * RATE) as usize];
            let h = harmonics_db(w, f0, 5);
            println!(
                "  E4 MIDI {midi:3}: H2 {:+.1}  H3 {:+.1}  H4 {:+.1}  H5 {:+.1} dB re H1",
                h[1], h[2], h[3], h[4]
            );
        }
    }

    /// **Eight pedalled repeats, on the metal and through the wah.** The
    /// mechanism's own stacking — the dry patch, whose fresh peak is its
    /// own click — is held to the frame's 4 dB margin (mod.rs
    /// `STACK_MARGIN`). Through the wah the same eight strikes read far
    /// over it, and this test says why in numbers: the wah's follower
    /// opens 3 ms after a strike from silence, so a fresh strike's peak
    /// through it is the RING with the click filtered away, while in the
    /// stack every later click lands on an open filter AND a ringing
    /// string, and two things that superpose add. The stack's peak is held
    /// to the sum of a fresh strike's click through an open wah and its
    /// ring — one decibel of slack — which is the bound superposition
    /// gives, and the number the frame's margin should be read against.
    #[test]
    fn eight_repeats_stack_within_the_margin_on_the_metal_and_add_click_to_ring_through_the_wah() {
        use super::super::{Keys, Patch};
        const BLOCK: usize = 128;
        let at = |secs: f32| (secs * RATE) as usize / BLOCK * BLOCK;
        // (frame, 0 on / 1 off / 2 pedal, pitch)
        let run = |patch: Patch, ev: &[(usize, u8, u8)], frames: usize| -> Vec<f32> {
            let mut k = Keys::new(RATE);
            k.set_patch(patch);
            let mut out = Vec::new();
            let mut scratch = vec![0.0f32; BLOCK * 2];
            let mut ev = ev.to_vec();
            ev.sort_by_key(|e| e.0);
            let (mut next, mut frame) = (0, 0);
            while frame < frames {
                while next < ev.len() && ev[next].0 < frame + BLOCK {
                    match ev[next].1 {
                        0 => k.note_on(ev[next].2, 102.0 / 127.0),
                        1 => k.note_off(ev[next].2),
                        _ => k.set_pedal(true),
                    }
                    next += 1;
                }
                scratch.fill(0.0);
                k.render(&mut scratch, BLOCK, 2);
                out.extend(scratch.chunks_exact(2).map(|f| (f[0] + f[1]) * 0.5));
                frame += BLOCK;
            }
            out
        };
        let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
        let mut rapid = vec![(0usize, 2u8, 0u8)];
        for k in 0..8 {
            rapid.push((at(k as f32 * 0.060), 0, 60));
            rapid.push((at(k as f32 * 0.060 + 0.030), 1, 60));
        }
        for patch in [Patch::Clav, Patch::ClavWah] {
            let fresh = peak(&run(patch, &[(0, 2, 0), (at(0.01), 0, 60)], (0.6 * RATE) as usize));
            let stack = peak(&run(patch, &rapid, (0.6 * RATE) as usize));
            // A strike after a choked note: the follower is still open
            // (140 ms release) over a silent string, so this peak is the
            // click through an open wah; the ring's own peak is read after
            // the click has passed.
            let opened = run(patch, &[(at(0.01), 0, 60), (at(0.04), 1, 60), (at(0.12), 0, 60)], (0.6 * RATE) as usize);
            assert!(peak(&opened[at(0.10)..at(0.12)]) < 0.01, "{patch:?}: the opener was still ringing");
            let click = peak(&opened[at(0.12)..at(0.12) + (0.006 * RATE) as usize]);
            let ring = peak(&opened[at(0.12) + (0.006 * RATE) as usize..]);
            println!(
                "  {patch:?}: fresh {fresh:.3}, stack {stack:.3} ({:+.1} dB); click through an open wah {click:.3}, ring {ring:.3}, sum {:.3}",
                20.0 * (stack / fresh).log10(),
                click + ring
            );
            if patch == Patch::Clav {
                assert!(stack <= fresh * 1.585, "the metal stacked to {stack} against a fresh {fresh}");
            }
            assert!(
                stack <= (click + ring) * 1.122,
                "{patch:?}: the stack's {stack} exceeds a click on a ring ({click} + {ring})"
            );
        }
    }

    /// `strike_knobs` is `strike_based` through the dry base, sample for
    /// sample, so mod.rs's untouched dispatch is the dry instrument.
    #[test]
    fn strike_knobs_is_the_dry_base() {
        let mut a = ClavVoice::new(RATE);
        let mut b = ClavVoice::new(RATE);
        a.strike_knobs(60, 0.7, &knobs());
        b.strike_based(60, 0.7, &knobs(), &ClavBase::DRY);
        for _ in 0..4800 {
            assert_eq!(a.tick(), b.tick());
        }
    }
}
