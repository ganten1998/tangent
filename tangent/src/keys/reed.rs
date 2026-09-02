//! The reed piano: a flat steel reed, and a charged plate listening to it.
//!
//! **Why this is not the tine with different numbers.** The tine instrument
//! is a tuning fork read by a MAGNET: its nonlinearity is the flux curve,
//! and its voice is a pure fundamental with a glassy bell riding on top. This
//! one is a flat spring-steel reed, tuned by a blob of solder at its tip,
//! vibrating INSIDE a charged plate — the reed and the plate are the two
//! halves of a capacitor, and the current the motion induces is lopsided
//! from the first millimetre of swing, because the reed swings THROUGH the
//! plate's gap, not past a pole piece. Played gently it is hollow and
//! woody — almost all fundamental, the overtones of a stiff bar too high
//! and too weak to carry. Played hard, the capacitance curve hands the
//! signal a growing second harmonic, and the tone GROWLS. That growl is the
//! whole reason anyone has ever carried one up a flight of stairs, and it
//! is a function of how far the reed is swinging — which is why it lives
//! here, on the displacement, and not in an overdrive after the fact.
//!
//! The bank of decaying sinusoids is [`super::tine::Mode`] — kicked, not
//! driven, for the reasons written on that struct.

use super::tine::Mode;
use super::WurliKnobs;

/// Modes per voice: the fundamental, two bar overtones, and two thump
/// partials for the hammer's knock. A stiff bar's overtones sit an octave
/// and a half of nothing away from the fundamental; five is already more
/// than the plate lets through.
const MODES: usize = 5;

/// How far the reed swings at full velocity, as a fraction of the plate
/// gap. Sets where the growl arrives; the GROWL knob scales it.
const REED_SWING: f32 = 0.45;

/// The damper's time constant. Reed dampers are small felt pads on a stiff
/// arm — quicker than the tine's, slower than yarn.
const DAMP_SECS: f32 = 0.050;

/// What separates the two shipped reed pianos before any knob moves.
#[derive(Clone, Copy)]
pub struct WurliBase {
    /// Scale on the pickup's asymmetry — how eager the growl is.
    pub growl: f32,
    /// A floor under it: the tube instrument is never entirely clean, even
    /// feathered. Zero for the transistor one.
    pub growl_floor: f32,
    /// The amp's top, one pole.
    pub tone_hz: f32,
    /// Scale on the hammer knock.
    pub thump: f32,
}

impl WurliBase {
    /// The classroom-and-stage classic: transistor amp, clean until leaned
    /// on, then the bark arrives all at once. Bright for a reed.
    pub const TWO_HUNDRED: WurliBase = WurliBase {
        growl: 1.0,
        growl_floor: 0.0,
        tone_hz: 9_000.0,
        thump: 1.0,
    };
    /// The earlier tube instrument: darker, warmer, never quite clean —
    /// there is a little even-harmonic haze on every note, and the growl
    /// comes up like a dimmer rather than a switch.
    pub const ONE_FORTY: WurliBase = WurliBase {
        growl: 1.5,
        growl_floor: 0.20,
        tone_hz: 6_200.0,
        thump: 1.2,
    };
}

/// One key of the reed piano.
pub struct WurliVoice {
    rate: f32,
    modes: [Mode; MODES],
    /// The pickup's asymmetry this note, set at the strike: knob, base and
    /// swing folded to one coefficient on the square term.
    asym: f32,
    /// Compensation so the growl changes the SHAPE, not the loudness.
    comp: f32,
    /// DC blocker: a squared term has a mean, and the mean must not reach
    /// the bus as a per-note thud of offset.
    dc_x1: f32,
    dc_y1: f32,
    /// The amp's one-pole top.
    tone_z: f32,
    tone_a: f32,
    damp: f32,
    damp_to: f32,
    damp_secs: f32,
    env: f32,
    done: bool,
    seed: u32,
}

impl WurliVoice {
    pub fn new(sample_rate: f32) -> Self {
        WurliVoice {
            rate: sample_rate,
            modes: [Mode::default(); MODES],
            asym: 0.0,
            comp: 1.0,
            dc_x1: 0.0,
            dc_y1: 0.0,
            tone_z: 0.0,
            tone_a: 1.0,
            damp: 1.0,
            damp_to: 1.0,
            damp_secs: DAMP_SECS,
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

        // **The bar's overtones.** A clamped-free bar wants 6.27 and 17.55;
        // the solder blob that tunes the reed drags both DOWN, further in
        // the bass where the blob is biggest. They are colour, not chord:
        // the plate reads displacement near the tip and barely sees them.
        let drop = 1.0 - 0.06 * ((key - 33.0) / 40.0).clamp(0.0, 1.0);
        let over1 = 5.6 * drop;
        let over2 = 14.6 * drop;

        // Decay: a reed outlasts a tine's bell but not its fundamental —
        // there is no tonebar here to bank energy in.
        let t60_f = (6.5 * (-(key - 33.0) / 40.0).exp()).clamp(0.7, 8.0) * k.decay;
        let t60_f = t60_f.clamp(0.1, 30.0);

        // Contact: felt over a wooden hammer, on a sprung key — shorter and
        // harder than the tine's neoprene. VOICING is the felt's wear.
        let felt = 1.28_f32.powf(1.0 - 2.0 * k.voicing.clamp(0.0, 1.0));
        let contact = (1.6 - 1.1 * v.powf(0.7)) * 0.001 * felt;
        let fc = 1.0 / contact;
        let roll = |f: f32| 1.0 / (1.0 + (f / fc) * (f / fc)).sqrt();

        // The REED knob is how much of the bar's own metal reaches the
        // plate: 0 is a sine in a box, 2 is a reed with its clank showing.
        let w1 = 0.05 * k.reed * roll(f0 * over1) / roll(f0).max(1e-9);
        let w2 = 0.012 * k.reed * roll(f0 * over2) / roll(f0).max(1e-9);

        // **The knock.** A wurli's attack has a woody tick the tine never
        // had — hammer meeting reed meets key meeting rail. Two fixed
        // partials, gone in thirty milliseconds, scaled by velocity harder
        // than the tone is.
        let knock = base.thump * k.thump * 0.10 * v.powf(1.6);

        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        let jitter = 1.0 + ((self.seed >> 8) as f32 / 16_777_216.0 - 0.5) * 0.03;

        let swing = 0.9 * v.powf(1.3);
        let norm = 1.0 / (1.0 + w1 + w2);
        let amp = swing * norm * jitter;

        self.modes[0].kick(f0, t60_f, self.rate, amp);
        self.modes[1].kick(f0 * over1, t60_f * 0.22, self.rate, amp * w1);
        self.modes[2].kick(f0 * over2, t60_f * 0.10, self.rate, amp * w2);
        self.modes[3].kick(165.0, 0.035, self.rate, knock);
        self.modes[4].kick(610.0, 0.014, self.rate, knock * 0.5);

        // **The growl coefficient.** The square term's size against the
        // linear one, from how far this note will swing through the gap.
        // The floor is the tube instrument's standing haze; the knob and
        // velocity bring the rest. The compensation divides the level the
        // asymmetry would otherwise add, so GROWL turns dirt up, not volume.
        // The 2.2 is calibrated on the bench: with it, the second harmonic
        // rides from under -70 dB feathered to the high -teens flat out,
        // which is a reed amp doing what reed amps are loved for. Without
        // it the growl topped out near -32 dB - a polite instrument, and
        // polite is the one thing this must never be.
        let u = swing * REED_SWING;
        self.asym = (base.growl_floor + base.growl * k.growl * u * 2.2)
            .clamp(0.0, 1.6);
        // 0.45, not a smaller figure: the bench's forte comp chords on the
        // tube instrument put 24 samples into the ceiling at 0.35.
        self.comp = 1.0 / (1.0 + 0.45 * self.asym);

        let corner = base.tone_hz.min(self.rate * 0.45);
        self.tone_a = 1.0 - (-std::f32::consts::TAU * corner / self.rate).exp();
        self.tone_z = 0.0;

        self.damp = 1.0;
        self.damp_to = 1.0;
        self.damp_secs = (DAMP_SECS * k.release).clamp(0.004, 1.5);
        self.env = swing;
        self.done = false;
    }

    pub fn release(&mut self, pedal: bool) {
        if !pedal {
            self.land_damper();
            self.damp_to = 0.0;
        }
    }

    pub fn damp(&mut self) {
        self.land_damper();
        self.damp_to = 0.0;
    }

    /// The pad lands on the reed — softer than the tine's thump, because the
    /// arm is shorter and the reed lighter.
    fn land_damper(&mut self) {
        if self.done || self.damp_to == 0.0 {
            return;
        }
        self.modes[3].kick(140.0, 0.030, self.rate, self.env * 0.22);
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
        let mut disp = 0.0;
        for m in &mut self.modes {
            disp += m.tick();
        }

        // **The plate.** Linear term plus the asymmetric square, and a whiff
        // of cube so the hardest notes crack rather than merely thicken.
        // Because `disp` decays with the note, the dirt fades with it —
        // a chord's tail cleans itself up, which is exactly what the real
        // amp does and no post-effect can.
        let u = (disp * REED_SWING).clamp(-0.9, 0.9);
        let shaped = disp * (1.0 + self.asym * u + 0.30 * self.asym * u * u);

        // The squared term's mean walks with the envelope; block it.
        let dc = shaped - self.dc_x1 + 0.9995 * self.dc_y1;
        self.dc_x1 = shaped;
        self.dc_y1 = dc;

        self.tone_z += self.tone_a * (dc - self.tone_z);
        let coeff = 1.0 - (-1.0 / (self.damp_secs * self.rate)).exp();
        self.damp += (self.damp_to - self.damp) * coeff;
        let out = self.tone_z * self.comp * self.damp * 0.44;
        let out = if out.is_finite() { out.clamp(-4.0, 4.0) } else { 0.0 };

        let a = out.abs();
        self.env += (a - self.env) * if a > self.env { 0.4 } else { 0.0006 };
        if self.env < 2.0e-5 {
            self.done = true;
            for m in &mut self.modes {
                m.clear();
            }
            self.dc_x1 = 0.0;
            self.dc_y1 = 0.0;
            self.tone_z = 0.0;
        }
        out
    }
}
