//! The electric piano: a steel tine, a tonebar, and a magnet listening to it.
//!
//! **Why this is not the piano with different numbers.** A piano string is
//! flexible and under enormous tension, so its partials are very nearly
//! whole-number multiples and a delay line models it beautifully. A tine is a
//! stiff steel rod clamped at one end — a tuning fork's arm — and a stiff bar's
//! modes are nowhere near harmonic: the textbook clamped-free ratios run
//! 1 : 6.27 : 17.55. A waveguide cannot make those, because a delay line
//! produces harmonics by construction. So this file is a bank of resonators
//! instead, one per mode, each free to sit wherever the metal actually puts it.
//! That single decision is the difference between an electric piano and a
//! detuned piano with the tone knob down.
//!
//! **Where the sound actually comes from.**
//! - The TINE gives the pitch and the bell. Its high mode is the glassy chime
//!   that a light touch is almost entirely made of, and it dies in well under
//!   a second while the fundamental rings for many — that ratio IS the sound.
//! - The TONEBAR is a lump of metal tuned near the tine, coupled to it. It
//!   lengthens the sustain and puts a slow beat in it.
//! - The PICKUP is a magnet, and it is not linear. Flux through the coil goes
//!   roughly as the inverse square of the gap, so a tine swinging a long way
//!   generates a lopsided waveform full of even harmonics. **That is the
//!   bark**, and the reason it must be modelled here rather than as an
//!   overdrive after the fact: the distortion depends on how far the tine is
//!   moving, so it arrives when the player hits hard and it is simply absent
//!   when they do not. A waveshaper on the output cannot do that — it would
//!   dirty the quiet notes too, and quiet is where this instrument is supposed
//!   to be at its cleanest.

/// How far the tine swings, as a fraction of the pickup gap, at full velocity.
/// The whole nonlinearity is a function of this number: well inside the gap
/// the pickup is a wire, and close to it a fuzzbox.
///
/// **It is this small for a measured reason.** At 0.86 the bench reported a
/// waveform asymmetry of 0.995 at full velocity — very nearly one-sided, which
/// is not a bark but a spike, and it reads to the ear as clipping rather than
/// as an instrument being driven. At this value asymmetry runs from about 0.01
/// feathered to about 0.45 hard, which is a Rhodes leaning on the pickup.
const PICKUP_SWING: f32 = 0.17;

/// The tine cannot reach the magnet, and neither may the arithmetic — one
/// sample at the pole is an infinity that never leaves the output.
///
/// **Set far above anything a strike reaches, on purpose.** When the clamp
/// actually engages it stops being a safety net and starts being part of the
/// transfer function, and a compensated gain that saturates INVERTS: the bench
/// caught full-velocity notes coming out 4 dB quieter than velocity 107, which
/// is the instrument getting quieter the harder it is played. This is a guard
/// against arithmetic, not a limiter.
const PICKUP_LIMIT: f32 = 0.55;

/// The damper's time constant as the bench tuned it. Longer than the piano's
/// because a Rhodes damper is a felt pad on a steel bar, not on wire; the
/// RELEASE knob scales this and nothing else.
const DAMP_SECS: f32 = 0.085;

/// Modes per voice. Six covers the fundamental, the low bar modes, and the
/// bell; more are inaudible under the pickup's own colouring.
const MODES: usize = 6;

use super::EpKnobs;

/// One mode of the metal, as a decaying sinusoid.
///
/// **Kicked, not driven.** The obvious way to build a modal bank is a row of
/// resonant filters with the excitation poured through them, and it is a trap:
/// a two-pole resonator sharp enough to ring for nine seconds has a gain at
/// its own frequency of about a hundred thousand, so the first version of this
/// file put out ninety decibels over full scale. The fix is not to normalise
/// that gain. It is to stop driving the filter at all — a mode of a struck bar
/// IS a decaying sinusoid, so the strike sets its amplitude directly and the
/// recursion below just plays it out.
///
/// Two multiplies and two adds per mode per sample, no input, no gain to get
/// wrong, and — the part that matters for this instrument — the amplitude of
/// every partial is a number the strike chooses rather than the accidental
/// product of a filter's response. The whole velocity design depends on being
/// able to say exactly how loud the bell is relative to the fundamental.
#[derive(Clone, Copy, Default)]
pub(super) struct Mode {
    y1: f32,
    y2: f32,
    /// 2 * r * cos(w)
    b1: f32,
    /// -r^2
    b2: f32,
}

impl Mode {
    /// Start this mode ringing at `amp`, decaying 60 dB in `t60` seconds.
    ///
    /// The seed values are the first two samples of `amp * r^n * sin(w n)`;
    /// the recursion generates every sample after that exactly.
    pub(super) fn kick(&mut self, freq: f32, t60: f32, rate: f32, amp: f32) {
        if freq <= 0.0 || freq >= rate * 0.47 || amp == 0.0 {
            self.b1 = 0.0;
            self.b2 = 0.0;
            self.y1 = 0.0;
            self.y2 = 0.0;
            return;
        }
        let w = std::f32::consts::TAU * freq / rate;
        let r = 10.0_f32
            .powf(-3.0 / (t60.max(0.01) * rate))
            .clamp(0.0, 0.999_99);
        self.b1 = 2.0 * r * w.cos();
        self.b2 = -(r * r);
        self.y2 = 0.0;
        self.y1 = amp * r * w.sin();
    }

    pub(super) fn tick(&mut self) -> f32 {
        let y = self.b1 * self.y1 + self.b2 * self.y2;
        let y = if y.is_finite() { y } else { 0.0 };
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub(super) fn clear(&mut self) {
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// One key of the electric piano.
pub struct EpVoice {
    rate: f32,
    modes: [Mode; MODES],
    /// The tonebar: near-unison with the fundamental, lightly coupled, and the
    /// source of the slow beat under a held note.
    bar: Mode,
    /// What the pickup's own gain has to be divided by, so that hitting
    /// harder gets DIRTIER rather than simply enormous. Fixed at the strike,
    /// because the swing the note is going to reach is known then.
    pk_comp: f32,
    /// How far a unit of tine displacement swings toward the magnet — the
    /// stock `PICKUP_SWING` scaled by the BARK knob, stashed at the strike.
    /// It has to be a field: the first build scaled only `pk_comp` and left
    /// the tick reading the bare constant, which made BARK an inverse volume
    /// knob — the compensation for a nonlinearity that never changed — and
    /// at the top of its travel a fortissimo came out QUIETER than a
    /// mezzo-forte while adding no dirt at all.
    pk_swing: f32,
    damp: f32,
    damp_to: f32,
    /// The damper's time constant in seconds, set at strike time from the
    /// RELEASE knob.
    damp_secs: f32,
    env: f32,
    done: bool,
    /// Random sign per strike, so repeated notes are not bit-identical.
    seed: u32,
    /// The damper pad landing: a soft dud that plays when the key is let go.
    /// The tick's comment always promised it; this is the promise kept.
    thump: Mode,
    /// The passive pickup and cable: a gentle top rolloff, one pole. A real
    /// harp's coil into a real lead has nothing left above the bell, and the
    /// raw modal sum was the one place this instrument still sounded like
    /// arithmetic.
    pickup_lp: f32,
    pickup_a: f32,
}

impl EpVoice {
    pub fn new(sample_rate: f32) -> Self {
        EpVoice {
            rate: sample_rate,
            modes: [Mode::default(); MODES],
            bar: Mode::default(),
            pk_comp: 1.0,
            pk_swing: PICKUP_SWING,
            damp: 1.0,
            damp_to: 1.0,
            damp_secs: DAMP_SECS,
            env: 0.0,
            done: true,
            seed: 0x9E37_79B9,
            thump: Mode::default(),
            pickup_lp: 0.0,
            // Corner near 7.5 kHz: high enough to leave every bell partial
            // alone, low enough to shave the synthetic edge off the top.
            pickup_a: 1.0 - (-std::f32::consts::TAU * 7_500.0
                / sample_rate.max(8_000.0))
                .exp(),
        }
    }

    pub fn strike(&mut self, pitch: u8, velocity: f32) {
        self.strike_knobs(pitch, velocity, &EpKnobs::default());
    }

    /// `strike` with the whole panel: every knob this mechanism answers to.
    pub fn strike_knobs(&mut self, pitch: u8, velocity: f32, k: &EpKnobs) {
        self.strike_full(pitch, velocity, k);
    }

    /// `strike` with the panel's VOICING knob: it scales the hammer tip's
    /// contact time, 1.3x at full mellow to 0.77x at full bright, the same
    /// lever velocity itself pulls. The exponent form lands 0.5 on exactly
    /// 1.0, so the shipped default is this file's tuning untouched.
    pub fn strike_voiced(&mut self, pitch: u8, velocity: f32, voicing: f32) {
        let mut k = EpKnobs::default();
        k.voicing = voicing;
        self.strike_full(pitch, velocity, &k);
    }

    fn strike_full(&mut self, pitch: u8, velocity: f32, knobs: &EpKnobs) {
        let voicing = knobs.voicing;
        let f0 = 440.0 * 2.0_f32.powf((f32::from(pitch) - 69.0) / 12.0);
        let v = velocity.clamp(0.0, 1.0);
        let k = f32::from(pitch);
        // The tip's compound, worn soft to fresh hard, as a scale on the
        // contact time. The 1.9 span is measured, not felt-out: at the old
        // 1.3 the bell moved under 3 dB across the knob's whole travel,
        // because the contact rolloff only starts biting the bell partial
        // when the width can actually double. 0.5 still lands on exactly 1.
        let felt = 1.9_f32.powf(1.0 - 2.0 * voicing.clamp(0.0, 1.0));

        // **The bell partial's position moves with the key.** On a real
        // instrument the tine gets shorter and its clamp stiffer as you go up,
        // so the chime sits at a lower multiple in the treble than it does in
        // the bass. Down low it is up around the sixth; at the top it has
        // collapsed towards the fourth and is nearly gone.
        // CHIME slides that whole curve up or down the series — the same
        // instrument with a longer or shorter tine, not a different tuning.
        let bell = (6.6 - 0.030 * (k - 28.0) + knobs.chime).clamp(2.2, 9.0);

        // Decay. The fundamental rings for many seconds; the bell is gone in
        // under one, and shorter still in the treble. If those two numbers are
        // the same the instrument sounds like an organ.
        let t60_f = (9.0 * (-(k - 28.0) / 42.0).exp()).clamp(0.9, 11.0) * knobs.decay;
        let t60_f = t60_f.clamp(0.1, 40.0);
        // The bell follows DECAY too, but at its own much shorter scale: if
        // it did not, a long-decay setting would leave the chime ringing at
        // the same length while everything under it grew, which is a bell,
        // not a Rhodes.
        // 1.05 base with the ceiling at 1.2: the earlier 0.75/0.9 pair left
        // the chime at a third of a second in the middle of the keyboard,
        // which reads as a tick riding the note rather than a bell over it —
        // the reed piano's lone bar overtone was outliving it, and the two
        // instruments were trading characters. A chime that hangs on for
        // half a second is still gone twenty times faster than the
        // fundamental, which is the ratio the sound IS.
        let t60_bell =
            ((1.05 * (-(k - 28.0) / 46.0).exp()).clamp(0.2, 1.2) * knobs.decay).clamp(0.03, 4.0);

        // **Velocity, mechanism one: contact time.** A hard strike is short and
        // therefore wide-band, so far more of the energy lands in the high
        // modes. The excitation's spectrum is the transform of its contact
        // pulse, which for a raised cosine of length T rolls off from about
        // 1/T — so rather than filter anything, each mode simply takes the
        // amount of energy that pulse actually contains at its frequency.
        // Contact runs about 2.6 ms feathered down to 0.7 ms hard.
        let contact = (2.2 - 1.5 * v.powf(0.7)) * 0.001 * felt;
        let fc = 1.0 / contact;

        // The bell's own weight on top of that, because the tine's high mode
        // is also driven harder by a stiffer, more compressed tip. Between the
        // two, the bell sits about thirty decibels below the fundamental at
        // pianissimo and level with it at fortissimo, which is the entire
        // reason this instrument can whisper and shout.
        // BELL scales that weight: 0 is a tine with no chime at all, 2 is a
        // glass bell over the note.
        // The 0.16 floor is what keeps the promise above honest at the soft
        // end: at the old 0.045, the contact rolloff and the chime's own
        // decay stacked up to fifty-six decibels under the fundamental by the
        // bench's measure — not a soft bell, no bell — and a feathered Rhodes
        // note is supposed to be almost nothing but glass. The floor cannot
        // buy the whole distance back (a 2 ms contact genuinely has little
        // to give a 1.5 kHz mode — that part is physics, not voicing); this
        // lands pianissimo in the high minus-thirties re the fundamental,
        // which is a bell you hear, from a bench column that used to say
        // there was none.
        let bell_w = (0.16 + 0.89 * v.powf(2.0)) * knobs.bell;

        let ratios = [1.0, 2.0, 3.0, bell, bell * 1.62, bell * 2.71];
        let weights = [1.0, 0.20, 0.09, bell_w, bell_w * 0.34, bell_w * 0.11];

        // Each mode's share of the strike: its own weight, shaped by what the
        // contact pulse has at that frequency.
        let mut amps = [0.0_f32; MODES];
        let mut norm = 0.0_f32;
        for i in 0..MODES {
            let f = f0 * ratios[i];
            // First order in AMPLITUDE. Squaring it here (the obvious
            // 1/(1 + (f/fc)^2)) is a twelve-decibel-per-octave weighting, and
            // the bench showed what that costs: at pianissimo it put the bell
            // fifty-two decibels under the fundamental, which is not a soft
            // bell, it is no bell. A Rhodes played gently is still glassy.
            let roll = 1.0 / (1.0 + (f / fc) * (f / fc)).sqrt();
            amps[i] = weights[i] * roll;
            norm += amps[i].abs();
        }
        // Normalise so the tine's swing is a known quantity. It has to be, or
        // the pickup below cannot be told how close to the magnet it is.
        let scale = if norm > 1e-9 { 1.0 / norm } else { 0.0 };
        // **The overall swing is what velocity buys.** The exponent is the
        // neoprene tip: rubber gets stiffer as it is compressed, so the
        // amplitude curve is steeper than the velocity that caused it.
        let swing = 0.92 * v.powf(1.25);

        let t60s = [
            t60_f,
            t60_f * 0.62,
            t60_f * 0.40,
            t60_bell,
            t60_bell * 0.68,
            t60_bell * 0.45,
        ];
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        let jitter = 1.0 + ((self.seed >> 8) as f32 / 16_777_216.0 - 0.5) * 0.03;

        for i in 0..MODES {
            self.modes[i].kick(
                f0 * ratios[i],
                t60s[i],
                self.rate,
                amps[i] * scale * swing * jitter,
            );
        }

        // The tonebar: a few cents off the fundamental and ringing longer than
        // the tine does. Two near-unison partials beat at their difference
        // frequency, and that slow swell is why a held Rhodes note sounds
        // alive rather than sustained.
        // The TONEBAR knob is this mode, weight and ring together: 0 is a
        // tine screwed to dead wood — no beat, and the note dies at the
        // tine's own rate — and 2 is a bar that outlasts the tine by half
        // again and swells against it. 1 is the instrument as tuned. (The
        // knob spent one release wired to the PIANO's soundboard send, which
        // the electric instruments hand a hard zero: a placebo. This is the
        // real bar.)
        self.bar.kick(
            f0 * 1.0018,
            t60_f * (1.0 + 0.45 * knobs.body),
            self.rate,
            amps[0] * scale * swing * 0.42 * knobs.body,
        );

        // How close to the magnet this note is going to swing, and therefore
        // how much of the pickup's exponential gain it is going to pick up.
        // Taking exactly that back out leaves the ASYMMETRY — the positive
        // half still grows against the negative one — while the note's
        // loudness stays a function of how hard it was hit.
        // BARK is how far the tine swings against the magnet, which IS the
        // pickup's nonlinearity: turning it up is leaning on the pickup, not
        // a distortion after the fact. The scaled swing is STASHED for the
        // tick — the compensation below and the transfer it compensates must
        // read the same number, or the knob turns into a volume control.
        self.pk_swing = PICKUP_SWING * knobs.bark;
        let u_max = (swing * self.pk_swing).clamp(0.0, PICKUP_LIMIT);
        let gm = 1.0 - u_max;
        self.pk_comp = gm * gm * gm;

        self.damp = 1.0;
        self.damp_to = 1.0;
        self.damp_secs = (DAMP_SECS * knobs.release).clamp(0.004, 1.5);
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

    /// The pad lands. Scaled by how loudly the tine was still swinging: a
    /// note that has already died gets no thump, because the pad has nothing
    /// to catch.
    fn land_damper(&mut self) {
        if self.done || self.damp_to == 0.0 {
            return;
        }
        self.thump.kick(88.0, 0.045, self.rate, self.env * 0.35);
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
        disp += self.bar.tick();

        // **Velocity, mechanism two: the pickup.** Flux through the coil goes
        // as the inverse square of the gap, so to first order in the swing the
        // transfer is
        //     out = x / (1 - x/d)^3
        // Below about a third of the gap that is indistinguishable from a
        // wire. Approaching it, the positive half grows and the negative half
        // compresses, which puts even harmonics in — a lopsided waveform, and
        // lopsided is what a bark is. Because it is a function of DISPLACEMENT
        // it arrives when the player hits hard and is simply not there when
        // they do not, which a waveshaper on the output could never manage:
        // that would dirty the quiet notes too, and quiet is where this
        // instrument is supposed to be at its cleanest.
        //
        // The clamp is not cosmetic. A real tine cannot reach the magnet; the
        // arithmetic can, and one sample at the pole is an infinity that lives
        // in the output for the rest of the session.
        // `pk_swing`, not the bare constant: the BARK knob moved the gap at
        // the strike, and reading the constant here while compensating for
        // the knob there is exactly the bug that made BARK a volume knob.
        let u = (disp * self.pk_swing).clamp(-PICKUP_LIMIT, PICKUP_LIMIT);
        let g = 1.0 - u;
        let out = disp * self.pk_comp / (g * g * g);

        // The damper: felt, and slow. A Rhodes does not stop dead, and the
        // little thump as the pad lands is part of why it sounds played.
        let coeff = 1.0 - (-1.0 / (self.damp_secs * self.rate)).exp();
        self.damp += (self.damp_to - self.damp) * coeff;
        // **Headroom for chords, not for one note.** A single note at full
        // velocity peaks near -7 dBFS, which looks quiet on a meter and is
        // not: five voices of a comped voicing sum, and at a trim that made
        // one note hit 0 dBFS the bench put a four-note chord plus a bass
        // note four decibels INTO the ceiling. An instrument that clips its
        // own output on an ordinary voicing is broken; the channel fader is
        // downstream and cannot undo it.
        // The pickup's own rolloff, then the damper's thump OUTSIDE the
        // damp fade — the pad landing is the damper, it cannot be silenced
        // by it.
        self.pickup_lp += self.pickup_a * (out - self.pickup_lp);
        let out = (self.pickup_lp * self.damp + self.thump.tick()) * 0.45;
        let out = if out.is_finite() { out.clamp(-4.0, 4.0) } else { 0.0 };

        let a = out.abs();
        self.env += (a - self.env) * if a > self.env { 0.4 } else { 0.0006 };
        if self.env < 2.0e-5 {
            self.done = true;
            for m in &mut self.modes {
                m.clear();
            }
            self.bar.clear();
            self.thump.clear();
            self.pickup_lp = 0.0;
        }
        out
    }
}
