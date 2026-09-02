//! The clav: a string struck and HELD, and two pickups arguing about it.
//!
//! **Why this is neither the piano nor a harpsichord.** A piano's hammer
//! throws and leaves; a harpsichord's plectrum plucks and leaves. The clav's
//! tangent — a rubber tip on the back of the key — strikes the string and
//! then STAYS, pinning it against its anvil for as long as the finger is
//! down. That one fact is the whole instrument: the strike is percussive
//! like a hammer, the sustain is a fretted string like a guitarist's
//! hammer-on, and the note ends not by fading but by the tangent LIFTING —
//! the string's far end is wound with yarn, so the release is a fast choke
//! with an audible spit of the string letting go. Playing one is as much
//! about the lift as the press, which is why the release spit is modelled
//! and knobbed rather than cleaned away.
//!
//! **Where the funk lives.** Not in the string — in the PICKUP. The string
//! is read by a coil under one point of its length, and a point reading of
//! a travelling wave is a comb filter: everything whose half-wavelength
//! divides the pickup's distance from the bridge is simply not heard. The
//! PICKUP knob slides that point, which is what the C/D rocker on the real
//! instrument does, and the hollow bark it makes is the sound every record
//! reaches for. It is a first-order difference off the same delay line the
//! string already runs — almost free, and impossible to fake downstream.
//!
//! The string itself is [`super::string::Str`] — the piano's loop, borrowed
//! whole. Same bridge, different cabinet.

use super::string::{design_loss, loop_phase_delay, Str};
use super::ClavKnobs;

/// How long the yarn takes to choke the string once the tangent lifts.
/// Yarn is fast — that is what it is FOR — but it is not a gate.
const CHOKE_SECS: f32 = 0.014;

/// The pickup comb's own delay store. Sized for the lowest string at 96 kHz
/// with the pickup at its furthest position; anything longer clamps, which
/// only a sub-contra note at an extreme setting could notice.
const COMB_MAX: usize = 2048;

/// One key of the clav.
pub struct ClavVoice {
    rate: f32,
    string: Str,
    /// The tangent's strike pulse, incremental-Gaussian exactly as the
    /// piano's hammer runs it — two multiplies a sample.
    pulse: u32,
    pulse_amp: f32,
    pulse_g: f32,
    pulse_r: f32,
    pulse_rstep: f32,
    /// Where along the string the tangent lands, in samples back.
    strike_at: usize,
    /// The pickup comb: y = x - g * x[d].
    comb: [f32; COMB_MAX],
    comb_at: usize,
    comb_d: usize,
    comb_g: f32,
    /// The tone rocker: two matched poles over the comb's output. Two, not
    /// one — a clav's spectrum is over by 2 kHz, and one pole's 6 dB an
    /// octave measured under 4 dB of change on anything that was actually
    /// sounding. Twelve is a rocker you can hear land.
    tone_z: f32,
    tone_z2: f32,
    tone_a: f32,
    /// The choke fade, and the spit that rides out through it.
    damp: f32,
    damp_to: f32,
    damp_secs: f32,
    /// Release spit still to pour, in samples; reuses the pulse registers'
    /// idiom at a fixed tiny width.
    spit: u32,
    spit_amp: f32,
    spit_g: f32,
    spit_r: f32,
    spit_rstep: f32,
    /// The SNAP knob as read at the strike, kept apart from `spit_amp` —
    /// which is the spit IN FLIGHT — so a strike landing on a string still
    /// spitting from its last release does not retune the spit halfway.
    snap: f32,
    env: f32,
    done: bool,
}

/// What a strike decides about the loop and the pickup before it touches
/// the string. See `ClavVoice::design`.
struct Design {
    len: f32,
    gain: f32,
    loss_a: f32,
    disp_a: f32,
    strike_at: usize,
    comb_d: usize,
}

impl ClavVoice {
    pub fn new(sample_rate: f32) -> Self {
        ClavVoice {
            rate: sample_rate,
            string: Str::new(),
            pulse: 0,
            pulse_amp: 0.0,
            pulse_g: 0.0,
            pulse_r: 0.0,
            pulse_rstep: 0.0,
            strike_at: 8,
            comb: [0.0; COMB_MAX],
            comb_at: 0,
            comb_d: 16,
            comb_g: 0.92,
            tone_z: 0.0,
            tone_z2: 0.0,
            tone_a: 1.0,
            damp: 1.0,
            damp_to: 1.0,
            damp_secs: CHOKE_SECS,
            spit: 0,
            spit_amp: 0.0,
            spit_g: 0.0,
            spit_r: 0.0,
            spit_rstep: 0.0,
            snap: 0.0,
            env: 0.0,
            done: true,
        }
    }

    /// Whether a strike of `pitch` with these knobs would land on the
    /// string ringing now rather than have to replace it — the same
    /// question as `PianoVoice::lands`, with one more condition: the pickup
    /// comb's tap. Moving that tap under a ringing string jumps the comb
    /// term by up to the whole raw signal, so a PICKUP knob moved between
    /// two strikes of one held note is the rare case the frame retires
    /// through a fade instead.
    pub fn lands(&self, pitch: u8, knobs: &ClavKnobs) -> bool {
        !self.done && self.fits(&self.design(pitch, knobs))
    }

    fn fits(&self, d: &Design) -> bool {
        (self.string.len - d.len).abs() <= 1.0 && self.comb_d == d.comb_d
    }

    /// Everything a strike decides about the loop and the comb, for this
    /// pitch and these knobs. Pure: reads the rate, writes nothing.
    fn design(&self, pitch: u8, k: &ClavKnobs) -> Design {
        let f0 = 440.0 * 2.0_f32.powf((f32::from(pitch) - 69.0) / 12.0);
        let total = (self.rate / f0).clamp(4.0, 4000.0);
        let key = f32::from(pitch);

        // Thin plain-steel wire, short scale: stiffer for its pitch than a
        // grand's tenor but without the grand's wound bass, so one gentle
        // schedule serves the whole board.
        let b = (1.2e-4 * 2.0_f32.powf((key - 52.0) / 26.0)).clamp(2.0e-5, 2.0e-3);

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
        let w1 = std::f32::consts::TAU * f0 / self.rate;
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
        // every velocity, and a test in mod.rs now pins that.
        let disp_tau = (1.8625 * b.sqrt() * total)
            .clamp(1.0, (0.14 * total).min(19.0).max(1.0));
        let disp_a = (1.0 - disp_tau) / (1.0 + disp_tau);
        let phase_delay = loop_phase_delay(w1, p, disp_a);
        let len = (total - phase_delay).max(4.0);

        // The tangent lands close to the anvil end — a tenth of the string,
        // a touch nearer in the treble — which is why even the strike is
        // toppy before the pickup has said anything.
        let ratio = if key < 76.0 { 10.0 } else { 9.0 };
        let strike_at = ((total / ratio).round() as usize).clamp(1, 4000);

        // **The pickup.** Its distance from the bridge, as a fraction of the
        // string, sets the comb: close is glassy (notches high and out of
        // the way), far is hollow (the low harmonics start folding). The
        // knob's travel covers the C/D rockers and everything between.
        let frac = 0.06 + 0.30 * k.pickup.clamp(0.0, 1.0);
        let comb_d = ((total * frac).round() as usize).clamp(1, COMB_MAX - 1);

        Design {
            len,
            gain,
            loss_a: 1.0 - p,
            disp_a,
            strike_at,
            comb_d,
        }
    }

    pub fn strike_knobs(&mut self, pitch: u8, velocity: f32, k: &ClavKnobs) {
        let v = velocity.clamp(0.0, 1.0);
        let d = self.design(pitch, k);

        // **A tangent landing on a string that is still moving adds to the
        // motion** — the piano's rule, borrowed with the loop; see
        // `PianoVoice::strike_full` for the argument. The comb and the tone
        // rockers are part of the signal path and keep their memory; only a
        // string of another design is cleared, and the frame keeps that
        // from being audible.
        let landing = !self.done && self.fits(&d);
        if landing && self.damp < 1.0 {
            // The yarn's take so far, made real in the string, the comb and
            // the rockers — all linear, so exactly.
            self.string.scale(self.damp);
            for c in &mut self.comb {
                *c *= self.damp;
            }
            self.tone_z *= self.damp;
            self.tone_z2 *= self.damp;
        }
        for ap in &mut self.string.disp {
            ap.a = d.disp_a;
        }
        self.string.loss.a = d.loss_a;
        self.string.gain = d.gain;
        self.string.len = d.len;
        self.strike_at = d.strike_at;
        if !landing {
            self.string.clear();
        }

        // Rubber on steel: contact a fraction of the piano's felt. VOICING
        // is the tip's compound, worn soft to fresh hard; velocity shortens
        // contact exactly as the piano's felt does, just faster and from a
        // harder start.
        let tip = 1.30_f32.powf(1.0 - 2.0 * k.voicing.clamp(0.0, 1.0));
        let sigma_ms = (0.20 - 0.13 * v.powf(0.8)) * tip;
        let sigma_s = (sigma_ms * 0.001 * self.rate).max(0.8);
        let half = 3.5 * sigma_s;
        self.pulse = (2.0 * half) as u32;
        self.pulse_g = (-half * half / (2.0 * sigma_s * sigma_s)).exp();
        self.pulse_r = ((2.0 * half - 1.0) / (2.0 * sigma_s * sigma_s)).exp();
        self.pulse_rstep = (-1.0 / (sigma_s * sigma_s)).exp();
        self.pulse_amp = 0.30 * v.powf(1.3);

        self.comb_d = d.comb_d;
        self.comb_g = 0.92;
        if !landing {
            self.comb.fill(0.0);
            self.comb_at = 0;
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
        if !landing {
            self.spit = 0;
        }
        self.snap = k.snap;
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
        self.tone_z = 0.0;
        self.tone_z2 = 0.0;
        self.pulse = 0;
        self.spit = 0;
        self.damp = 1.0;
        self.damp_to = 1.0;
        self.env = 0.0;
        self.done = true;
    }

    /// The tangent lifts. The yarn chokes the string AND the string spits —
    /// the far segment, pinned all this time, gets its one say. The spit is
    /// poured into the still-ringing loop so it comes out coloured by the
    /// string and the comb rather than as a naked click.
    pub fn release(&mut self, pedal: bool) {
        if pedal || self.done || self.damp_to == 0.0 {
            return;
        }
        self.damp_to = 0.0;
        let sigma_s = (0.10 * 0.001 * self.rate).max(0.8);
        let half = 3.5 * sigma_s;
        self.spit = (2.0 * half) as u32;
        self.spit_g = (-half * half / (2.0 * sigma_s * sigma_s)).exp();
        self.spit_r = ((2.0 * half - 1.0) / (2.0 * sigma_s * sigma_s)).exp();
        self.spit_rstep = (-1.0 / (sigma_s * sigma_s)).exp();
        // Scaled by how loudly the string was still going: lifting off a
        // dead note is silent, lifting mid-bark clacks.
        self.spit_amp = self.snap * self.env * 1.6;
    }

    pub fn damp(&mut self) {
        // The panic path and the patch change: choke without the spit.
        self.damp_to = 0.0;
        self.spit = 0;
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
        let mut inject = 0.0_f32;
        if self.pulse > 0 {
            inject = self.pulse_amp * self.pulse_g;
            self.pulse_g *= self.pulse_r;
            self.pulse_r *= self.pulse_rstep;
            self.pulse -= 1;
        }
        if self.spit > 0 {
            inject += self.spit_amp * self.spit_g;
            self.spit_g *= self.spit_r;
            self.spit_r *= self.spit_rstep;
            self.spit -= 1;
        }

        // The tap pair, exactly the piano's: the difference is the strike
        // comb, and summing to zero keeps the loop DC-free.
        if inject != 0.0 {
            self.string.excite(self.strike_at, -inject);
        }
        let raw = self.string.tick(inject);

        // The pickup's point reading: now minus a while ago.
        let behind =
            self.comb[(self.comb_at + COMB_MAX - self.comb_d) % COMB_MAX];
        self.comb[self.comb_at] = raw;
        self.comb_at = (self.comb_at + 1) % COMB_MAX;
        let picked = raw - self.comb_g * behind;

        // Tone rocker — both poles — then the choke.
        self.tone_z += self.tone_a * (picked - self.tone_z);
        self.tone_z2 += self.tone_a * (self.tone_z - self.tone_z2);
        let coeff = 1.0 - (-1.0 / (self.damp_secs * self.rate)).exp();
        self.damp += (self.damp_to - self.damp) * coeff;
        let out = self.tone_z2 * self.damp;
        let out = if out.is_finite() { out.clamp(-4.0, 4.0) } else { 0.0 };

        let a = out.abs();
        self.env += (a - self.env) * if a > self.env { 0.4 } else { 0.0006 };
        if self.env < 2.0e-5 {
            self.done = true;
            self.string.clear();
            self.comb.fill(0.0);
            self.tone_z = 0.0;
            self.tone_z2 = 0.0;
        }
        out
    }
}
