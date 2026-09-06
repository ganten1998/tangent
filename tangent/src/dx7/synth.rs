//! Six-operator FM: the part that makes the sound.
//!
//! # How faithful is this
//!
//! **Structurally exact, musically calibrated, not a hardware emulation.**
//!
//! Exact: the thirty-two algorithms, the operator ratios including fixed mode
//! and detune, feedback, four-stage envelopes per operator and one for pitch,
//! keyboard level scaling with all four curves, rate scaling, velocity
//! sensitivity, and an LFO with six waveforms feeding pitch and amplitude.
//! Those are the things that decide what a patch IS, and a patch loaded here is
//! the same instrument as on the hardware.
//!
//! Calibrated rather than copied: the envelope rate and level curves. The DX7's
//! are lookup tables baked into a 1983 chip; these are continuous functions
//! fitted to the published times and levels. A patch will sit a shade brighter
//! or decay a shade differently than the original. It will not be a different
//! sound.
//!
//! Not implemented: portamento and the mono modes, which are performance
//! controls rather than part of a patch, and pitch bend, which nothing sends
//! here yet.
//!
//! # The rules this file lives by
//!
//! **It runs on the audio thread.** No allocation, no locks, no panics. Voices
//! are a fixed array and are stolen, never grown.
//!
//! **Everything expensive happens at note-on.** Ratios, envelope increments,
//! keyboard scaling and velocity are resolved once per note and cached per
//! operator; the per-sample loop is six sines, six multiplies and six adds.

use super::algorithms::{DEST, FEEDBACK_OP, OUT};
use super::voice::Voice;

/// How many notes can sound at once.
///
/// The DX7 itself managed sixteen and it defined the sound of a decade, so
/// sixteen is not a compromise. It is also six operators each, which is
/// ninety-six oscillators, and that is the real reason not to go higher.
pub const VOICES: usize = 16;

/// The dynamic range an envelope travels, in decibels. Below this a note is
/// past the least significant bit of anything it will be played through.
const RANGE_DB: f32 = 96.0;

/// Seconds for a full-range sweep at rate 99, the fastest the DX7 has.
///
/// **Fitted to the hardware, and the fit is audible.** These two constants are
/// the whole rate curve, and getting them wrong does not sound like a bug: it
/// sounds like every patch in every cartridge has the wrong decay. An earlier
/// pair here put rate 99 at 1.5ms and rate 0 at twenty seconds, which made a
/// held electric piano note die in a third of a second.
///
/// The DX7 sweeps its full range in about 5.5ms at rate 99 and about 38
/// seconds at rate 0, with roughly 8 rate steps per halving between them.
const FASTEST_SWEEP: f32 = 0.0055;

/// How many rate steps halve the time. See [`FASTEST_SWEEP`]: together they put
/// rate 0 near 38 seconds and rate 50 near a third of one.
const RATE_HALVING: f32 = 7.76;

/// Headroom before the limiter. Six carriers at full level is the loudest a
/// DX7 patch can be, and that has to leave room rather than clip.
const GAIN: f32 = 0.11;

// ── the parts of a voice ────────────────────────────────────────────────────

/// One operator's four-stage envelope, in the attenuation domain.
///
/// Attenuation rather than amplitude: an FM envelope is heard logarithmically,
/// and a linear one has an audible knee at the bottom of every decay. Zero is
/// full level and [`RANGE_DB`] is silence.
#[derive(Clone, Copy)]
struct Eg {
    /// Where each stage is heading, in dB of attenuation.
    target: [f32; 4],
    /// dB per sample for each stage.
    speed: [f32; 4],
    stage: usize,
    level: f32,
    /// Held at stage 3 until the key is released, which is what a sustain is.
    holding: bool,
    done: bool,
    /// An amplitude envelope is finished when it reaches silence. A PITCH
    /// envelope has no silence to reach: its rest position is the middle, and
    /// treating it like an attenuation is what made every note 1.4 semitones
    /// sharp — level 50 is neutral, and neutral is not quiet.
    bipolar: bool,
}

impl Eg {
    const SILENT: Self = Self {
        target: [RANGE_DB; 4],
        speed: [1.0; 4],
        stage: 3,
        level: RANGE_DB,
        holding: false,
        done: true,
        bipolar: false,
    };

    /// Build from a patch's four rates and levels, with `rate_scale` in
    /// 0.0..=1.0 added by the note's pitch.
    fn new(rates: [u8; 4], levels: [u8; 4], sample_rate: f32, rate_scale: f32) -> Self {
        let mut target = [0.0_f32; 4];
        let mut speed = [0.0_f32; 4];
        for i in 0..4 {
            target[i] = level_to_db(levels[i]);
            let r = f32::from(rates[i]).clamp(0.0, 99.0) + rate_scale * 24.0;
            speed[i] = rate_to_db_per_sample(r.min(99.0), sample_rate);
        }
        Self {
            target,
            speed,
            stage: 0,
            // A note starts silent and the first stage brings it up, which is
            // what makes a slow attack a slow attack rather than a click.
            level: RANGE_DB,
            holding: false,
            done: false,
            bipolar: false,
        }
    }

    /// The pitch envelope: four levels in SEMITONES, not in attenuation.
    ///
    /// Level 50 is the note as written. It starts at its own fourth level,
    /// which is where the DX7 rests, so a patch with a flat pitch envelope
    /// displaces nothing at all.
    fn pitch(rates: [u8; 4], levels: [u8; 4], sample_rate: f32) -> Self {
        let mut target = [0.0_f32; 4];
        let mut speed = [0.0_f32; 4];
        for i in 0..4 {
            target[i] = pitch_level_to_semitones(levels[i]);
            // The same rate scale as an amplitude envelope, expressed over the
            // pitch envelope's own span rather than over ninety-six decibels.
            let dbps = rate_to_db_per_sample(f32::from(rates[i]).clamp(0.0, 99.0), sample_rate);
            speed[i] = dbps / RANGE_DB * PITCH_SPAN;
        }
        Self {
            target,
            speed,
            stage: 0,
            level: target[3],
            holding: false,
            done: false,
            bipolar: true,
        }
    }

    /// Advance one sample and return the current attenuation.
    #[inline]
    fn tick(&mut self) -> f32 {
        if self.done {
            return RANGE_DB;
        }
        let t = self.target[self.stage];
        let s = self.speed[self.stage];
        if self.level < t {
            self.level = (self.level + s).min(t);
        } else if self.level > t {
            self.level = (self.level - s).max(t);
        }
        if (self.level - t).abs() < 1.0e-3 {
            // Stage 3 is the sustain: it is where a held note waits.
            if self.stage < 2 {
                self.stage += 1;
            } else if self.stage == 2 {
                self.holding = true;
            } else if !self.bipolar && self.level >= RANGE_DB - 0.01 {
                self.done = true;
            }
        }
        self.level
    }

    /// Where the envelope is right now, in the same units as its targets.
    fn level(&self) -> f32 {
        self.level
    }

    /// This envelope, starting its first stage from `level` instead of from
    /// silence. **What the hardware does when a voice is re-keyed**: the
    /// DX7's envelope generator is given a new target and a new rate, and
    /// it goes there from wherever it is — it has no way to jump. An attack
    /// that started from silence on a voice that was still sounding is a
    /// hole cut in the note, and the edges of the hole are the click.
    fn from_level(mut self, level: f32) -> Self {
        self.level = level.clamp(0.0, RANGE_DB);
        self
    }

    /// The key came up: run the fourth stage.
    fn release(&mut self) {
        self.stage = 3;
        self.holding = false;
    }

    /// Whether this envelope is down past `floor` dB of attenuation AND is
    /// not going to come back up on its own.
    ///
    /// **The level alone is half the question.** An envelope that has not
    /// started its attack is sitting at silence too, and it is about to be
    /// the note; reading the level alone would call every note dead on the
    /// sample it was struck. And a patch is free to write a level of zero
    /// into a MIDDLE stage — a gated sound, silent for a moment and then
    /// back — so "quiet now" is not "quiet from now on". Every stage it has
    /// left to visit has to be down there too.
    fn decayed_past(&self, floor: f32) -> bool {
        self.done
            || (self.level >= floor && self.target[self.stage..].iter().all(|t| *t >= floor))
    }

    /// Nothing left: this envelope has reached silence and will not come
    /// back up.
    ///
    /// **Not "the release ran".** This asked for stage 3, and a key
    /// released under the pedal never reaches it: `note_off` leaves the
    /// envelope in its SUSTAIN stage, so a patch whose sustain level is zero
    /// — every percussive patch ever written — decays to nothing and sits
    /// there, silent, with the flag false. Its slot was then never reclaimed
    /// and `sounding` never decremented, so a re-struck key kept finding the
    /// husk and re-keying it: for a sync patch that meant the repeat kept
    /// the dead note's oscillator phase and lost the aligned attack the
    /// patch exists for, measured on the bench at ten decibels of second
    /// difference over what a fresh strike is allowed.
    fn finished(&self) -> bool {
        !self.bipolar && self.decayed_past(RANGE_DB - 0.01)
    }

    /// Past hearing, if not yet past arithmetic: down [`SPENT_DB`] on its own
    /// strike. See `Dx7::note_on` for what a repeat does about it.
    fn spent(&self) -> bool {
        self.decayed_past(SPENT_DB)
    }
}

/// How far an operator has to have decayed before a repeat of its key counts
/// as a NEW note rather than another hammer on the same one: 40 dB, a
/// hundredth of the amplitude the strike put there.
///
/// **This threshold cannot cut anything**, which is why it can be this
/// generous. A spent voice is not silenced, re-seeded or faded — it is left
/// exactly as it is, ringing itself out, while the repeat goes to a slot of
/// its own. The only thing a too-eager reading costs is a voice out of
/// sixteen for as long as the husk takes to reach the floor.
const SPENT_DB: f32 = 40.0;

/// One sounding note.
#[derive(Clone, Copy)]
struct Note {
    on: bool,
    pitch: u8,
    age: u64,
    /// Per operator, in OP1..OP6 order.
    phase: [f32; 6],
    step: [f32; 6],
    eg: [Eg; 6],
    /// Peak amplitude for each operator, after output level, keyboard scaling
    /// and velocity. Resolved once, at note-on.
    peak: [f32; 6],
    /// How much the LFO's amplitude modulation reaches each operator.
    am_depth: [f32; 6],
    /// The two samples of feedback history the loop reads.
    fb: [f32; 2],
    pitch_eg: Eg,
    /// Semitones the pitch envelope is currently displacing the note by.
    pitch_bias: f32,
}

impl Note {
    const SILENT: Self = Self {
        on: false,
        pitch: 0,
        age: 0,
        phase: [0.0; 6],
        step: [0.0; 6],
        eg: [Eg::SILENT; 6],
        peak: [0.0; 6],
        am_depth: [0.0; 6],
        fb: [0.0; 2],
        pitch_eg: Eg::SILENT,
        pitch_bias: 0.0,
    };
}

// ── the instrument ──────────────────────────────────────────────────────────

pub struct Dx7 {
    voice: Voice,
    notes: [Note; VOICES],
    sample_rate: f32,
    clock: u64,
    pedal: bool,
    sounding: usize,
    /// LFO phase in 0.0..1.0, shared by every note, which is what the hardware
    /// does and why a patch with a fast LFO sounds coherent across a chord.
    lfo_phase: f32,
    lfo_step: f32,
    lfo_value: f32,
    /// Sample-and-hold's current value, and the phase it last changed at.
    lfo_sh: f32,
    lfo_rng: u32,
    feedback_gain: f32,
    feedback_op: usize,
}

impl Dx7 {
    pub fn new(sample_rate: f32) -> Self {
        let mut d = Self {
            voice: Voice::default(),
            notes: [Note::SILENT; VOICES],
            sample_rate: sample_rate.max(8_000.0),
            clock: 0,
            pedal: false,
            sounding: 0,
            lfo_phase: 0.0,
            lfo_step: 0.0,
            lfo_value: 0.0,
            lfo_sh: 0.0,
            lfo_rng: 0x1234_5678,
            feedback_gain: 0.0,
            feedback_op: 5,
        };
        d.set_voice(Voice::default());
        d
    }

    /// Load a patch. Sounding notes keep the patch they started with for their
    /// envelopes, which is what stops a bank change from clicking.
    pub fn set_voice(&mut self, v: Voice) {
        let a = usize::from(v.algorithm.min(31));
        self.feedback_op = usize::from(FEEDBACK_OP[a]).clamp(1, 6) - 1;
        // Feedback 0..=7 is a doubling scale; 7 is where an operator goes to
        // noise, which is what it is for.
        self.feedback_gain = if v.feedback == 0 {
            0.0
        } else {
            (1u32 << (v.feedback - 1)) as f32 / 128.0 * 6.0
        };
        self.lfo_step = lfo_hz(v.lfo_speed) / self.sample_rate;
        self.voice = v;
    }

    pub fn active(&self) -> bool {
        self.sounding > 0
    }

    pub fn note_on(&mut self, pitch: u8, velocity: f32) {
        if pitch > 127 {
            return;
        }
        // **A repeat of a pitch reuses that pitch's voice — sounding OR held
        // by the damper.** With the pedal down, `note_off` leaves the voice
        // ringing with `on = false` and the damper bit set. Matching only
        // `on` here meant a re-struck key under the pedal allocated a FRESH
        // voice every time, so rapid repeats stacked a dozen identical notes
        // until the pool ran out and stealing chopped ringing voices — the
        // owner's "many note glitches". Retriggering the held voice keeps one
        // voice per pitch, which is what a real damper does.
        // **A slot is free when EVERY operator is silent, not when OP1 is.**
        // This used to ask `eg[0].finished()` alone, and a patch with a fast
        // OP1 release beside a slower-ringing carrier — an ordinary electric
        // piano — had eg[0] done milliseconds after the key came up while OP3
        // still rang. Slot 0 then looked free, the very next note-on
        // hard-overwrote the audible tail with fifteen genuinely silent slots
        // sitting right there, and the `sounding` ledger below counted the
        // stolen slot as newly occupied: one phantom voice per steal, and
        // `active()` never false again until a flush.
        // **And a repeat of a pitch whose voice has already died away takes a
        // NEW voice, which is what the hardware does with all sixteen of
        // them.** Re-keying is for a string still under the damper; a note
        // that has decayed past hearing is finished, and starting the repeat
        // on top of its husk is what made a rapid pedalled repeat sound
        // unlike the patch — the envelope generator picks up from wherever
        // the husk left it, so the attack is as much shorter as the husk is
        // loud, and a percussive patch's attack is the patch. Nothing is cut
        // to do this: the husk is left alone to ring itself out, and the
        // repeat is a first strike, to the bit.
        //
        // Only ever into a slot that is genuinely free — never a steal — so
        // that a full pool degrades to re-keying rather than to chopping
        // somebody's held chord.
        let damper_held = |n: &Note| !n.on && n.age & (1 << 63) != 0;
        let dest = DEST[usize::from(self.voice.algorithm.min(31))];
        // A voice speaks through its CARRIERS. A modulator at silence still
        // shapes; a carrier at silence is the note being over.
        let spent = |n: &Note| {
            n.age == 0 || (0..6).all(|i| dest[i] != OUT || n.eg[i].spent())
        };
        let mine = |n: &Note| n.pitch == pitch && (n.on || damper_held(n));
        let slot = self
            .notes
            .iter()
            .position(|n| mine(n) && !spent(n))
            .or_else(|| {
                self.notes
                    .iter()
                    .position(|n| !n.on && n.eg.iter().all(Eg::finished))
            })
            .or_else(|| self.notes.iter().position(mine))
            .unwrap_or_else(|| {
                let mut best = 0;
                for (i, n) in self.notes.iter().enumerate() {
                    if n.age < self.notes[best].age {
                        best = i;
                    }
                }
                best
            });

        let v = self.voice;
        let sr = self.sample_rate;
        // Transpose is stored 0..=48 with 24 meaning none.
        let sounding_pitch = f32::from(pitch) + f32::from(v.transpose) - 24.0;
        let hz = 440.0 * 2.0_f32.powf((sounding_pitch - 69.0) / 12.0);
        let vel = velocity.clamp(0.0, 1.0);

        // **The slot is re-keyed, not replaced.** Whatever is in it — a
        // repeat of this pitch under the pedal, a release tail, a stolen
        // voice at another pitch — is still sounding, and this used to
        // overwrite it wholesale: every envelope back to silence, every
        // phase re-seeded, the feedback history zeroed. A ringing voice cut
        // mid-waveform, audible as a pop on every consecutive strike and
        // worst under the pedal, where the old note is always still loud.
        // The rules below are the hardware's, which re-keys a voice by
        // giving its generators new targets and letting them run on.
        let old = self.notes[slot];
        let silent = old.age == 0 || old.eg.iter().all(Eg::finished);

        let mut n = Note {
            on: true,
            pitch,
            age: self.clock.wrapping_add(1),
            // The two samples of feedback history stay: the self-modulating
            // operator keeps its own recent past, and zeroing it is a kink
            // in whatever it was making. From silence there is nothing to
            // keep, and starting at exactly zero is what makes a repeat on
            // a dead slot the same sound as a first strike, to the bit.
            fb: if silent { [0.0; 2] } else { old.fb },
            ..Note::SILENT
        };
        self.clock = n.age;

        for i in 0..6 {
            let op = v.ops[i];
            // How fast this operator's envelope runs at this pitch.
            let rs =
                f32::from(op.rate_scaling) / 7.0 * ((sounding_pitch - 21.0) / 87.0).clamp(0.0, 1.0);
            // Each envelope starts its attack from where the OLD one is —
            // from silence exactly when the slot was silent, which is the
            // first strike as it always was.
            let from = if silent { RANGE_DB } else { old.eg[i].level() };
            n.eg[i] = Eg::new(op.rate, op.level, sr, rs).from_level(from);
            n.step[i] = std::f32::consts::TAU * op_hz(&op, hz) / sr;
            // A patch with oscillator sync starts every operator at zero, which
            // is what makes a percussive attack repeatable. Without it they run
            // free and each note is slightly different, which is what makes a
            // pad breathe.
            //
            // Both rules are for a voice starting from SILENCE, where the
            // phase is inaudible until the envelope brings it up. A voice
            // that is still sounding keeps its phase whatever the patch
            // says: the sine runs on at the new step — continuous through a
            // pitch change, which is a frequency step and not a click —
            // where a reset to zero is a step of up to the whole waveform.
            // The hardware never resets an audible oscillator on a repeat
            // of the same key, because it gives the repeat a fresh silent
            // voice; with one voice per pitch here, running on is the same
            // sound. (It does reset one when it steals, and that is a
            // known click of the instrument, not one to keep.)
            n.phase[i] = if !silent {
                old.phase[i]
            } else if v.osc_sync {
                0.0
            } else {
                self.next_random() * std::f32::consts::TAU
            };

            let db = f32::from(op.output_level).min(99.0);
            let scaled = db + keyboard_scaling(&op, sounding_pitch) + velocity_offset(&op, vel);
            n.peak[i] = level_to_amp(scaled);
            n.am_depth[i] = f32::from(op.amp_mod_sens) / 3.0;
        }
        // The pitch envelope restarts: its rest is the note as written, and
        // a step in frequency is not a step in the waveform.
        n.pitch_eg = Eg::pitch(v.pitch_rate, v.pitch_level, sr);

        // The ledger counts occupied SLOTS. A retired slot is `Note::SILENT`,
        // whose age is 0 — the retire loop in `render` is the only decrement,
        // and it only fires on slots it resets to exactly that. Taking one of
        // those is a new voice; overwriting anything else destroys a note that
        // is still counted, so its count transfers to the note replacing it.
        if self.notes[slot].age == 0 {
            self.sounding += 1;
        }
        self.notes[slot] = n;
    }

    pub fn note_off(&mut self, pitch: u8) {
        for n in &mut self.notes {
            if n.on && n.pitch == pitch {
                if self.pedal {
                    // Held by the damper: no longer keyed down, but still
                    // ringing. The damper bit (1<<63) marks it so the pedal
                    // lift releases it AND a repeat of this pitch reuses this
                    // very voice rather than stacking a new one — see the
                    // slot search in `note_on`.
                    n.on = false;
                    n.age |= 1 << 63;
                } else {
                    n.on = false;
                    for e in &mut n.eg {
                        e.release();
                    }
                    n.pitch_eg.release();
                }
            }
        }
    }

    pub fn set_pedal(&mut self, down: bool) {
        let lifting = self.pedal && !down;
        self.pedal = down;
        if lifting {
            for n in &mut self.notes {
                if !n.on && n.age & (1 << 63) != 0 {
                    n.age &= !(1 << 63);
                    for e in &mut n.eg {
                        e.release();
                    }
                    n.pitch_eg.release();
                }
            }
        }
    }

    pub fn all_notes_off(&mut self) {
        self.notes = [Note::SILENT; VOICES];
        self.sounding = 0;
        self.pedal = false;
    }

    fn next_random(&mut self) -> f32 {
        // xorshift, so an unsynced patch is different per note without a
        // dependency and without touching the OS.
        self.lfo_rng ^= self.lfo_rng << 13;
        self.lfo_rng ^= self.lfo_rng >> 17;
        self.lfo_rng ^= self.lfo_rng << 5;
        (self.lfo_rng >> 8) as f32 / 16_777_216.0
    }

    /// Add `frames` of interleaved audio into `out`.
    pub fn render(&mut self, out: &mut [f32], frames: usize, channels: usize) {
        if self.sounding == 0 || channels == 0 {
            return;
        }
        let n = frames.min(out.len() / channels);
        let algo = usize::from(self.voice.algorithm.min(31));
        let dest = DEST[algo];
        let pms = f32::from(self.voice.pitch_mod_sens) / 7.0;
        let pmd = f32::from(self.voice.lfo_pitch_depth) / 99.0;
        let amd = f32::from(self.voice.lfo_amp_depth) / 99.0;

        for f in 0..n {
            self.advance_lfo();
            // Pitch modulation is in semitones, and seven is the most a DX7
            // will bend with the wheel wide open.
            let pitch_lfo = self.lfo_value * pmd * pms * 7.0;
            let amp_lfo = (1.0 - amd * (0.5 - 0.5 * self.lfo_value)).clamp(0.0, 1.0);
            let pitch_mul = 2.0_f32.powf(pitch_lfo / 12.0);

            let mut sum = 0.0_f32;
            for note in &mut self.notes {
                if note.eg[0].finished() && note.eg.iter().all(Eg::finished) {
                    continue;
                }
                // Each operator's output this sample, indexed by operator
                // number so a destination can read it directly.
                let mut outs = [0.0_f32; 7];
                let mut heard = 0.0_f32;
                note.pitch_bias = note.pitch_eg.tick();
                let bias = 2.0_f32.powf(note.pitch_bias / 12.0) * pitch_mul;

                // From 6 down to 1: a modulator is always a higher number than
                // what it modulates, so this order has every input ready.
                for i in (0..6).rev() {
                    let atten = note.eg[i].tick();
                    let env = db_to_amp(atten);
                    let modulation = if i == self.feedback_op {
                        // Feedback reads the mean of the last two samples,
                        // which is the standard way to keep a self-modulating
                        // operator from oscillating at Nyquist.
                        (note.fb[0] + note.fb[1]) * 0.5 * self.feedback_gain
                    } else {
                        outs[i + 1]
                    };
                    let s = fast_sin(note.phase[i] + modulation);
                    note.phase[i] += note.step[i] * bias;
                    if note.phase[i] >= std::f32::consts::TAU {
                        note.phase[i] -= std::f32::consts::TAU;
                    }
                    let am = 1.0 - note.am_depth[i] * (1.0 - amp_lfo);
                    let value = s * env * note.peak[i] * am;
                    if i == self.feedback_op {
                        note.fb[1] = note.fb[0];
                        note.fb[0] = value;
                    }
                    let d = dest[i];
                    if d == OUT {
                        heard += value;
                    } else {
                        // A destination collects every modulator aimed at it.
                        // The index is the destination's operator NUMBER, which
                        // is why `outs` has seven slots and slot 0 is unused.
                        outs[usize::from(d)] += value * std::f32::consts::TAU;
                    }
                }
                sum += heard;
            }

            let s = soft_clip(sum * GAIN);
            let base = f * channels;
            for c in 0..channels {
                out[base + c] += s;
            }
        }

        // Retire finished notes once per block.
        for note in &mut self.notes {
            if !note.on && note.eg.iter().all(Eg::finished) && note.age != 0 {
                *note = Note::SILENT;
                self.sounding = self.sounding.saturating_sub(1);
            }
        }
    }

    fn advance_lfo(&mut self) {
        let prev = self.lfo_phase;
        self.lfo_phase += self.lfo_step;
        if self.lfo_phase >= 1.0 {
            self.lfo_phase -= 1.0;
        }
        let p = self.lfo_phase;
        self.lfo_value = match self.voice.lfo_wave {
            0 => 1.0 - 4.0 * (p - 0.5).abs(), // triangle
            1 => 1.0 - 2.0 * p,               // saw down
            2 => 2.0 * p - 1.0,               // saw up
            3 => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            } // square
            4 => fast_sin(p * std::f32::consts::TAU), // sine
            _ => {
                if p < prev {
                    self.lfo_sh = self.next_random() * 2.0 - 1.0;
                }
                self.lfo_sh
            } // sample and hold
        };
    }
}

// ── the arithmetic ──────────────────────────────────────────────────────────

/// An operator's frequency, from its ratio or its fixed setting.
fn op_hz(op: &super::voice::Op, note_hz: f32) -> f32 {
    // Detune is 0..=14 with 7 meaning none, and spans about a seventh of a
    // semitone either way: enough to thicken, never enough to sound out of tune.
    let detune = (f32::from(op.detune) - 7.0) * 0.015;
    if op.fixed {
        // Coarse picks the decade, fine walks through it: 1 Hz to 9772 Hz.
        let decade = f32::from(op.coarse & 0x03);
        10.0_f32.powf(decade + f32::from(op.fine) / 99.0 * 0.9) * (1.0 + detune * 0.1)
    } else {
        // Coarse 0 means a half, not a zero; the rest are whole multiples,
        // with fine interpolating to the next.
        let coarse = if op.coarse == 0 {
            0.5
        } else {
            f32::from(op.coarse)
        };
        note_hz * (coarse * (1.0 + f32::from(op.fine) / 100.0)) * 2.0_f32.powf(detune / 12.0)
    }
}

/// Keyboard level scaling, in dB to add to an operator's level.
fn keyboard_scaling(op: &super::voice::Op, pitch: f32) -> f32 {
    // Break point 0 is A-1; the scale runs up from there in semitones.
    let bp = f32::from(op.break_point) + 21.0;
    let (depth, curve, dist) = if pitch < bp {
        (op.left_depth, op.left_curve, bp - pitch)
    } else {
        (op.right_depth, op.right_curve, pitch - bp)
    };
    if depth == 0 {
        return 0.0;
    }
    let d = f32::from(depth);
    // Curves 0 and 1 take level away going out from the break point; 2 and 3
    // add it. Within each pair, one is linear in semitones and the other bends.
    let t = (dist / 48.0).clamp(0.0, 1.0);
    let shape = match curve {
        0 | 3 => t,
        _ => t * t,
    };
    let magnitude = shape * d * 0.75;
    if curve < 2 {
        -magnitude
    } else {
        magnitude
    }
}

/// How much velocity moves an operator's level, in the same dB units.
fn velocity_offset(op: &super::voice::Op, velocity: f32) -> f32 {
    if op.vel_sens == 0 {
        return 0.0;
    }
    // Sensitivity 7 takes a whisper down by about the full range of the
    // parameter; 1 barely touches it.
    let sens = f32::from(op.vel_sens) / 7.0;
    (velocity - 1.0) * sens * 60.0
}

/// An envelope level, 0..=99, as attenuation in dB.
fn level_to_db(level: u8) -> f32 {
    let l = f32::from(level.min(99));
    if l <= 0.0 {
        return RANGE_DB;
    }
    // Roughly three quarters of a dB per step at the top, steepening below,
    // which is where the DX7's own table bends.
    let db = (99.0 - l) * 0.75;
    if l >= 20.0 { db } else { db + (20.0 - l) * 1.6 }.min(RANGE_DB)
}

/// An operator level in the 0..=99 domain, as a linear amplitude.
fn level_to_amp(level: f32) -> f32 {
    if level <= 0.0 {
        return 0.0;
    }
    db_to_amp(level_to_db(level.min(99.0) as u8))
}

fn db_to_amp(atten_db: f32) -> f32 {
    if atten_db >= RANGE_DB {
        0.0
    } else {
        10.0_f32.powf(-atten_db / 20.0)
    }
}

/// An envelope rate, 0..=99, as dB per sample.
fn rate_to_db_per_sample(rate: f32, sample_rate: f32) -> f32 {
    let seconds = FASTEST_SWEEP * 2.0_f32.powf((99.0 - rate) / RATE_HALVING);
    RANGE_DB / (seconds * sample_rate).max(1.0)
}

/// How far the pitch envelope reaches, in semitones either side of neutral.
///
/// An octave up and an octave down. The hardware goes further at the extremes
/// of the parameter, and nothing musical lives out there.
const PITCH_SPAN: f32 = 24.0;

/// A pitch envelope level as semitones from the written note. **50 is zero**,
/// which is the whole point.
fn pitch_level_to_semitones(level: u8) -> f32 {
    (f32::from(level.min(99)) - 50.0) / 50.0 * (PITCH_SPAN / 2.0)
}

/// LFO speed 0..=99 as Hz. Slow enough to be a swell, fast enough to be a
/// buzz, which is the range the hardware covers.
fn lfo_hz(speed: u8) -> f32 {
    let s = f32::from(speed.min(99)) / 99.0;
    0.06 + s * s * 46.0
}

/// A sine, cheaply. See `builtin.rs`: peak error around a thousandth, and this
/// runs six times per voice per sample.
fn fast_sin(x: f32) -> f32 {
    const TAU: f32 = std::f32::consts::TAU;
    const PI: f32 = std::f32::consts::PI;
    let mut x = x % TAU;
    if x > PI {
        x -= TAU;
    } else if x < -PI {
        x += TAU;
    }
    let ax = x.abs();
    (16.0 * x * (PI - ax)) / (5.0 * PI * PI - 4.0 * ax * (PI - ax))
}

/// The ceiling, as in `builtin.rs`. Six carriers at full level is loud.
fn soft_clip(x: f32) -> f32 {
    const KNEE: f32 = 0.6;
    let a = x.abs();
    if a <= KNEE {
        return x;
    }
    let over = a - KNEE;
    let shaped = KNEE + (1.0 - KNEE) * (over / (over + (1.0 - KNEE)));
    if x < 0.0 {
        -shaped
    } else {
        shaped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(b: &[f32]) -> f32 {
        if b.is_empty() {
            return 0.0;
        }
        (b.iter().map(|s| s * s).sum::<f32>() / b.len() as f32).sqrt()
    }

    fn organ() -> Voice {
        // Algorithm 32: six carriers, no modulation. The one patch whose sound
        // can be predicted without a DX7 in the room. Built on the tine piano
        // rather than on `Voice::default()`, as every fixture here is: the
        // default became INIT VOICE, and these were written against the
        // piano's detunes, scalings and velocities. Their pins must not move.
        let mut v = Voice::electric_piano();
        v.algorithm = 31;
        v.feedback = 0;
        for op in &mut v.ops {
            op.output_level = 99;
            op.coarse = 1;
            op.fine = 0;
            op.rate = [99, 99, 99, 99];
            op.level = [99, 99, 99, 0];
        }
        v
    }

    #[test]
    fn an_idle_instrument_does_not_touch_the_buffer() {
        let mut d = Dx7::new(48_000.0);
        let mut out = vec![3.0_f32; 256];
        d.render(&mut out, 128, 2);
        assert!(out.iter().all(|s| *s == 3.0));
        assert!(!d.active());
    }

    #[test]
    fn a_note_sounds_and_a_release_ends_it() {
        let mut d = Dx7::new(48_000.0);
        d.set_voice(organ());
        d.note_on(60, 1.0);
        assert!(d.active());
        let mut out = vec![0.0_f32; 2 * 2048];
        d.render(&mut out, 2048, 2);
        assert!(
            rms(&out) > 0.01,
            "an organ patch was inaudible: {}",
            rms(&out)
        );

        d.note_off(60);
        for _ in 0..400 {
            let mut b = vec![0.0_f32; 2 * 2048];
            d.render(&mut b, 2048, 2);
        }
        assert!(!d.active(), "the note never released");
    }

    /// **Six carriers at full level must not clip the bus.** This is the
    /// loudest a DX7 patch can be, and sixteen of them at once is a pedalled
    /// cluster rather than a pathological case.
    #[test]
    fn the_loudest_possible_patch_stays_inside_the_rails() {
        let mut d = Dx7::new(48_000.0);
        d.set_voice(organ());
        for i in 0..VOICES {
            d.note_on(36 + i as u8 * 3, 1.0);
        }
        let mut peak = 0.0_f32;
        for _ in 0..8 {
            let mut out = vec![0.0_f32; 2 * 1024];
            d.render(&mut out, 1024, 2);
            for s in out {
                peak = peak.max(s.abs());
            }
        }
        assert!(peak <= 1.0, "clipped at {peak}");
        assert!(peak > 0.05, "sixteen voices came out at {peak}");
    }

    /// A carrier at ratio 1 has to come out at the note's own frequency, or
    /// every patch is in the wrong key.
    #[test]
    fn a_ratio_of_one_sounds_the_note_it_was_given() {
        let sr = 48_000.0;
        let mut d = Dx7::new(sr);
        d.set_voice(organ());
        d.note_on(69, 1.0); // A440
        let mut out = vec![0.0_f32; 2 * 24_000];
        d.render(&mut out, 24_000, 2);
        let mono: Vec<f32> = out.chunks_exact(2).map(|f| f[0]).collect();
        // Count rising zero crossings over the second half, past the attack.
        let tail = &mono[8_000..];
        let crossings = tail
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        let hz = crossings as f32 / (tail.len() as f32 / sr);
        assert!((hz - 440.0).abs() < 12.0, "A440 came out at {hz} Hz");
    }

    /// Every algorithm has to render without panicking and without silence,
    /// which is the cheap way to catch an index that walks off the table.
    #[test]
    fn all_thirty_two_algorithms_render() {
        for a in 0..32u8 {
            let mut v = organ();
            v.algorithm = a;
            v.feedback = 4;
            let mut d = Dx7::new(48_000.0);
            d.set_voice(v);
            d.note_on(60, 1.0);
            let mut out = vec![0.0_f32; 2 * 4096];
            d.render(&mut out, 4096, 2);
            let r = rms(&out);
            assert!(r.is_finite(), "algorithm {} produced NaN", a + 1);
            assert!(r > 0.0, "algorithm {} was silent", a + 1);
            assert!(
                out.iter().all(|s| s.abs() <= 1.0),
                "algorithm {} left the rails",
                a + 1
            );
        }
    }

    /// Render the default patch, and a cartridge patch if one is offered, to a
    /// `.wav` so a human can hear it.
    ///
    ///   IVORY_SYX_DEMO=/path/to/bank.syx cargo test -p tangent --bins dx7_demo \
    ///     -- --ignored --nocapture
    #[test]
    #[ignore = "writes a file for a person to listen to"]
    fn dx7_demo_wav() {
        const SR: u32 = 48_000;
        let mut d = Dx7::new(SR as f32);
        let mut left = Vec::<f32>::new();

        let mut banks: Vec<(String, Voice)> = vec![("default".into(), Voice::default())];
        if let Ok(path) = std::env::var("IVORY_SYX_DEMO") {
            match super::super::Cartridge::load(std::path::Path::new(&path)) {
                Ok(c) => {
                    println!(
                        "bank {} ({} checksum)",
                        c.name,
                        if c.checksum_ok { "good" } else { "bad" }
                    );
                    for (i, v) in c.voices.iter().enumerate().take(4) {
                        banks.push((format!("{i}: {}", v.display_name()), *v));
                    }
                }
                Err(e) => println!("could not load {path}: {e}"),
            }
        }

        for (label, v) in &banks {
            println!("  playing {label}");
            d.all_notes_off();
            d.set_voice(*v);
            // A ii-V-I, then a low note alone, per patch.
            let phrase: [(&[u8], f32); 4] = [
                (&[50, 57, 60, 65, 69], 1.2),
                (&[43, 53, 57, 62, 64], 1.2),
                (&[48, 55, 59, 64, 67], 1.8),
                (&[36], 2.0),
            ];
            for (notes, secs) in phrase {
                for n in notes {
                    d.note_on(*n, 0.85);
                }
                let frames = (secs * SR as f32) as usize;
                let mut block = vec![0.0_f32; frames * 2];
                d.render(&mut block, frames, 2);
                left.extend(block.chunks_exact(2).map(|f| f[0]));
                for n in notes {
                    d.note_off(*n);
                }
            }
        }

        let path = std::env::temp_dir().join(format!("tangent-dx7-{}.wav", std::process::id()));
        let mut wav = Vec::<u8>::new();
        let data_len = (left.len() * 2) as u32;
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&SR.to_le_bytes());
        wav.extend_from_slice(&(SR * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for s in &left {
            wav.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        std::fs::write(&path, wav).expect("write the demo");
        println!(
            "wrote {} ({:.1}s)",
            path.display(),
            left.len() as f32 / SR as f32
        );
    }

    /// **The rate curve, against the hardware.** Three points off a real DX7,
    /// which is the only thing that says whether a decay is right. Wide
    /// tolerances: the exact figures vary by unit and by who measured them, and
    /// what this catches is a curve off by a factor, not by a few percent.
    #[test]
    fn the_envelope_rate_curve_matches_a_real_dx7() {
        let secs = |rate: f32| RANGE_DB / (rate_to_db_per_sample(rate, 48_000.0) * 48_000.0);
        for (rate, want) in [(99.0, 0.0055_f32), (50.0, 0.33), (0.0, 38.0)] {
            let got = secs(rate);
            assert!(
                got > want * 0.6 && got < want * 1.7,
                "rate {rate} sweeps in {got:.4}s; the hardware is near {want}s"
            );
        }
        // Monotonic, or a higher rate number would somewhere be slower.
        for r in 0..99 {
            let (a, b) = (r as f32, (r + 1) as f32);
            assert!(secs(a) > secs(b), "rate {a} is not slower than {b}");
        }
    }

    /// **The pedal does not stack a repeated pitch into new voices.** With
    /// the damper down, releasing and re-striking the same key many times
    /// used to allocate a fresh voice each time, exhaust the sixteen-voice
    /// pool, and then steal ringing voices — the owner's rapid-note glitches.
    /// One pitch must occupy at most one voice, and distinct pitches struck
    /// afterward must still find room.
    #[test]
    fn the_pedal_does_not_stack_repeated_pitches() {
        const SR: f32 = 48_000.0;
        let mut d = Dx7::new(SR);
        d.set_pedal(true);
        // Re-strike middle C forty times under the pedal, releasing between.
        for _ in 0..40 {
            d.note_on(60, 0.8);
            d.note_off(60);
        }
        let c_voices = d
            .notes
            .iter()
            .filter(|n| n.pitch == 60 && !n.eg[0].finished())
            .count();
        assert!(
            c_voices <= 1,
            "one pitch took {c_voices} voices under the pedal — it should reuse one"
        );
        // Room remains for a chord: eight distinct pitches all get a voice.
        for p in 61u8..69 {
            d.note_on(p, 0.8);
        }
        for p in 61u8..69 {
            let live = d.notes.iter().any(|n| n.pitch == p && n.on);
            assert!(
                live,
                "pitch {p} found no voice — the pool was clogged by repeats"
            );
        }
    }

    /// **A ringing release tail is not stolen while silent voices sit free,
    /// and the `sounding` ledger comes back to zero.** OP1's envelope alone
    /// used to decide whether a slot was free: with a fast OP1 release beside
    /// a slow-ringing OP3, eg[0] was done milliseconds after the key came up,
    /// the next note-on hard-overwrote the still-audible tail in slot 0 with
    /// an otherwise empty pool — and counted the stolen slot as new, so
    /// `sounding` crept up by one and `active()` stayed true for the rest of
    /// the session.
    #[test]
    fn a_release_tail_is_not_stolen_while_silent_voices_sit_free() {
        const SR: f32 = 48_000.0;
        let mut v = organ();
        v.ops[0].rate[3] = 99; // OP1 dies within milliseconds of release…
        v.ops[2].rate[3] = 40; // …while OP3 rings on for about a second.
        let mut d = Dx7::new(SR);
        d.set_voice(v);

        d.note_on(60, 0.9);
        // Let the attack arrive before the key comes up: a release that
        // starts from silence is over before it begins, and proves nothing.
        let mut b = vec![0.0_f32; 2 * 2048];
        d.render(&mut b, 2048, 2);
        d.note_off(60);
        // Long enough for OP1's release to finish; OP3 is still ringing.
        let mut b = vec![0.0_f32; 2 * 2048];
        d.render(&mut b, 2048, 2);
        let ringing = |d: &Dx7| {
            d.notes
                .iter()
                .any(|n| n.pitch == 60 && !n.eg.iter().all(Eg::finished))
        };
        assert!(
            ringing(&d),
            "the tail died before the test could threaten it"
        );

        // A new note must take one of the fifteen silent slots, not the tail.
        d.note_on(62, 0.9);
        assert!(
            ringing(&d),
            "the new note stole the ringing tail with an empty pool"
        );

        // And once everything ends, the ledger has to say so.
        d.note_off(62);
        for _ in 0..400 {
            let mut b = vec![0.0_f32; 2 * 2048];
            d.render(&mut b, 2048, 2);
        }
        assert!(
            !d.active(),
            "`sounding` leaked: active() with nothing ringing"
        );
    }

    /// **The tine piano has to still be sounding when the note is.** A patch
    /// whose decay outruns the key is the bug this whole rate fit came from, and
    /// it is invisible in every test that only asks whether a sample is nonzero.
    ///
    /// It was "the default patch" when this was written. The default is INIT
    /// VOICE now — a sine that holds for exactly as long as the key does, which
    /// the last assertion here would rightly call a drone — so this strikes the
    /// piano it was written against, and the test below it holds INIT VOICE to
    /// its own contract.
    #[test]
    fn the_tine_piano_sustains_a_held_note_and_still_dies() {
        const SR: f32 = 48_000.0;
        let mut d = Dx7::new(SR);
        d.set_voice(Voice::electric_piano());
        d.note_on(60, 0.85);
        let peak_over = |d: &mut Dx7, secs: f32| {
            let frames = (secs * SR) as usize;
            let mut b = vec![0.0_f32; frames * 2];
            d.render(&mut b, frames, 2);
            b.iter().fold(0.0_f32, |m, s| m.max(s.abs()))
        };
        let attack = peak_over(&mut d, 0.05);
        assert!(attack > 0.05, "the attack is {attack}, which is inaudible");
        let after_1s = peak_over(&mut d, 1.0);
        let after_2s = peak_over(&mut d, 1.0);
        // A struck note decays, so this is a floor and not an equality.
        assert!(
            after_1s > attack * 0.1,
            "a second in, the note is at {:.0}% of its attack",
            after_1s / attack * 100.0
        );
        assert!(
            after_2s > attack * 0.02,
            "two seconds in, the note is at {:.1}% of its attack",
            after_2s / attack * 100.0
        );
        // And it does eventually stop, or it is a drone.
        let _ = peak_over(&mut d, 8.0);
        let late = peak_over(&mut d, 1.0);
        assert!(late < attack * 0.01, "still at {late} after eleven seconds");
    }

    /// **INIT VOICE holds while the key is down and stops when it is up.**
    /// Envelope levels 99/99/99/0 at rate 99 everywhere: the sine is at full
    /// level two seconds in, and gone within a twentieth of a second of the
    /// release. This is the contract the "(built in)" row now makes, and the
    /// opposite of the piano's — which is why the two have a test each.
    ///
    /// Revert-check performed: OP1's fourth level at 99 (`level: [99; 4]`)
    /// makes the release never come and the last assertion reddens.
    #[test]
    fn the_init_voice_holds_while_the_key_is_down_and_stops_when_it_is_up() {
        const SR: f32 = 48_000.0;
        let mut d = Dx7::new(SR);
        assert_eq!(d.voice, Voice::init(), "the engine does not start on INIT VOICE");
        d.note_on(60, 0.85);
        let peak_over = |d: &mut Dx7, secs: f32| {
            let frames = (secs * SR) as usize;
            let mut b = vec![0.0_f32; frames * 2];
            d.render(&mut b, frames, 2);
            b.iter().fold(0.0_f32, |m, s| m.max(s.abs()))
        };
        let attack = peak_over(&mut d, 0.05);
        assert!(attack > 0.05, "the attack is {attack}, which is inaudible");
        let _ = peak_over(&mut d, 1.0);
        let held = peak_over(&mut d, 1.0);
        assert!(
            held > attack * 0.9,
            "two seconds in, a held INIT VOICE is at {:.0}% of its attack",
            held / attack * 100.0
        );
        d.note_off(60);
        let _ = peak_over(&mut d, 0.05);
        let released = peak_over(&mut d, 0.05);
        assert!(
            released < attack * 0.01,
            "a tenth of a second after the release, INIT VOICE is still at {released}"
        );
    }

    /// Velocity has to matter, or every patch plays like a machine.
    #[test]
    fn velocity_changes_the_level_when_the_patch_asks_for_it() {
        let mut v = organ();
        for op in &mut v.ops {
            op.vel_sens = 7;
        }
        let loud = {
            let mut d = Dx7::new(48_000.0);
            d.set_voice(v);
            d.note_on(60, 1.0);
            let mut o = vec![0.0_f32; 2 * 2048];
            d.render(&mut o, 2048, 2);
            rms(&o)
        };
        let soft = {
            let mut d = Dx7::new(48_000.0);
            d.set_voice(v);
            d.note_on(60, 0.2);
            let mut o = vec![0.0_f32; 2 * 2048];
            d.render(&mut o, 2048, 2);
            rms(&o)
        };
        assert!(
            soft < loud * 0.6,
            "soft {soft} was not quieter than loud {loud}"
        );
    }

    // ── consecutive strikes ─────────────────────────────────────────────

    const SR: f32 = 48_000.0;
    const BLOCK: usize = 128;

    #[derive(Clone, Copy)]
    enum Ev {
        On(u8),
        Off(u8),
        Pedal(bool),
    }

    fn at(secs: f32) -> usize {
        (secs * SR) as usize / BLOCK * BLOCK
    }

    fn ms(m: f32) -> usize {
        (m * 0.001 * SR) as usize
    }

    /// Mono render of a scripted performance in the callback's own block
    /// size.
    fn script(v: Voice, events: &[(usize, Ev)], frames: usize) -> Vec<f32> {
        let mut d = Dx7::new(SR);
        d.set_voice(v);
        let mut events = events.to_vec();
        events.sort_by_key(|e| e.0);
        let mut out = Vec::with_capacity(frames);
        let mut scratch = [0.0_f32; BLOCK * 2];
        let mut next = 0;
        let mut frame = 0;
        while frame < frames {
            while next < events.len() && events[next].0 < frame + BLOCK {
                match events[next].1 {
                    Ev::On(p) => d.note_on(p, 0.8),
                    Ev::Off(p) => d.note_off(p),
                    Ev::Pedal(x) => d.set_pedal(x),
                }
                next += 1;
            }
            scratch.fill(0.0);
            d.render(&mut scratch, BLOCK, 2);
            out.extend(scratch.chunks_exact(2).map(|f| (f[0] + f[1]) * 0.5));
            frame += BLOCK;
        }
        out
    }

    fn d1(x: &[f32], a: usize, b: usize) -> f32 {
        let b = b.min(x.len());
        (a.max(1)..b).fold(0.0_f32, |m, n| m.max((x[n] - x[n - 1]).abs()))
    }

    fn d2(x: &[f32], a: usize, b: usize) -> f32 {
        let b = b.min(x.len());
        (a.max(2)..b).fold(0.0_f32, |m, n| {
            m.max((x[n] - 2.0 * x[n - 1] + x[n - 2]).abs())
        })
    }

    /// A percussive patch with oscillator sync and feedback: the shape whose
    /// whole character is six operators starting together, and therefore the
    /// one a re-key can most obviously get wrong.
    fn sync_bell() -> Voice {
        let mut v = Voice::electric_piano();
        v.osc_sync = true;
        v.feedback = 6;
        for op in &mut v.ops {
            op.rate = [99, 72, 50, 80];
            op.level = [99, 60, 0, 0];
        }
        v.ops[1].coarse = 3;
        v.ops[1].output_level = 86;
        v.ops[3].coarse = 7;
        v.ops[3].output_level = 70;
        v.ops[5].coarse = 11;
        v.ops[5].output_level = 75;
        v
    }

    /// A free-running patch that sustains: no sync, slow envelopes, so a
    /// repeat lands on a voice that has NOT yet come up. It must keep
    /// climbing, not start over.
    fn free_pad() -> Voice {
        let mut v = Voice::electric_piano();
        v.osc_sync = false;
        v.feedback = 0;
        for op in &mut v.ops {
            op.rate = [48, 60, 40, 55];
            op.level = [99, 95, 90, 0];
        }
        v
    }

    /// **A repeat of a key whose note has already died is a FIRST STRIKE, to
    /// the bit.** The hardware gives a repeat one of its sixteen voices and
    /// never re-zeroes a sounding one; here a spent voice is left to ring
    /// itself out and the repeat gets a slot of its own, so a sync patch's
    /// repeat is the sound the patch is for and not a re-key of a husk.
    ///
    /// This is where the ten-decibel second difference lived: the envelopes
    /// of a key released under the pedal never reach the RELEASE stage, so
    /// the slot looked occupied forever and the repeat inherited a dead
    /// note's oscillator phase.
    #[test]
    fn a_repeat_of_a_dead_note_is_a_first_strike() {
        let v = sync_bell();
        let frames = at(0.2);
        let fresh = script(v, &[(0, Ev::Pedal(true)), (0, Ev::On(60))], frames);
        // Struck, key up under the pedal, and struck again a second later —
        // by which time the bell has long since decayed to nothing.
        let onset = at(1.0);
        let again = script(
            v,
            &[
                (0, Ev::Pedal(true)),
                (0, Ev::On(60)),
                (at(0.25), Ev::Off(60)),
                (onset, Ev::On(60)),
            ],
            onset + frames,
        );
        let tail = &again[onset..];
        assert_eq!(tail.len(), fresh.len());
        let worst = fresh
            .iter()
            .zip(tail)
            .fold(0.0_f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(
            worst < 1.0e-6,
            "the repeat differs from a first strike by {worst}"
        );
    }

    /// A voice that decayed to silence under the pedal gives its slot back.
    /// It never did: `finished` asked whether the release stage had run, and
    /// under the pedal it does not — sixteen percussive notes and the pool
    /// was gone.
    #[test]
    fn a_percussive_note_held_by_the_pedal_frees_its_slot() {
        let mut d = Dx7::new(SR);
        d.set_voice(sync_bell());
        d.set_pedal(true);
        d.note_on(60, 0.8);
        d.note_off(60);
        let mut out = vec![0.0_f32; BLOCK * 2];
        for _ in 0..(at(1.0) / BLOCK) {
            out.fill(0.0);
            d.render(&mut out, BLOCK, 2);
        }
        assert!(
            !d.active(),
            "a bell that decayed to silence under the pedal still holds its voice"
        );
    }

    /// **A hammer is not a step, and the difference is the SHAPE.** What a
    /// landing strike adds may be louder or quieter than a fresh one — an
    /// envelope re-keyed from part way up is a shorter attack, and that is
    /// the generator doing what the hardware's does — but a gain moves the
    /// first and second differences together. A state cut and re-seeded does
    /// not: a step is broadband, so it lands in the second difference many
    /// times harder than in the first.
    fn assert_same_shape(what: &str, v: Voice, base: &[(usize, Ev)], onset: usize, frames: usize) {
        let fresh = script(v, &[(0, Ev::Pedal(true)), (at(0.01), Ev::On(100))], at(0.06));
        let (ff1, ff2) = (d1(&fresh, 0, fresh.len()), d2(&fresh, 0, fresh.len()));
        let bed = script(v, base, frames);
        let mut with = base.to_vec();
        with.push((onset, Ev::On(100)));
        let got = script(v, &with, frames);
        let diff: Vec<f32> = got.iter().zip(&bed).map(|(a, b)| a - b).collect();
        let (a, b) = (onset.saturating_sub(ms(2.0)), onset + ms(20.0));
        let r1 = d1(&diff, a, b) / ff1.max(1e-12);
        let r2 = d2(&diff, a, b) / ff2.max(1e-12);
        assert!(
            r2 <= r1 * 1.413,
            "{what}: what the strike added is {r1:.2}x a fresh strike's first difference \
             but {r2:.2}x its second — that ratio is a step, not a key going down"
        );
    }

    /// A voice taken out from under a full pool, and the newcomer that took
    /// it: what the newcomer adds is what a fresh strike adds.
    #[test]
    fn a_steal_adds_what_a_fresh_strike_adds() {
        for (name, v) in [("bell", sync_bell()), ("pad", free_pad())] {
            let mut base = vec![(0usize, Ev::Pedal(true))];
            for i in 0..VOICES {
                base.push((0, Ev::On(24 + i as u8)));
            }
            base.push((at(0.02), Ev::Off(30)));
            assert_same_shape(name, v, &base, at(0.1), at(0.14));
        }
    }

    /// A key struck again while its own note is still ringing under the
    /// pedal: same rule.
    #[test]
    fn a_restrike_under_the_pedal_adds_what_a_fresh_strike_adds() {
        for (name, v) in [("bell", sync_bell()), ("pad", free_pad())] {
            let base = [
                (0usize, Ev::Pedal(true)),
                (0, Ev::On(100)),
                (at(0.05), Ev::Off(100)),
            ];
            assert_same_shape(name, v, &base, at(0.1), at(0.14));
        }
    }

    /// **Eight strikes at sixty milliseconds, pedal down.** A repeat that
    /// allocates rather than re-keys must not stack the pool's output either.
    #[test]
    fn eight_pedalled_repeats_do_not_stack() {
        for (name, v) in [("bell", sync_bell()), ("pad", free_pad())] {
            let fresh = script(v, &[(0, Ev::Pedal(true)), (0, Ev::On(60))], at(0.5));
            let fresh_peak = fresh.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
            let mut ev = vec![(0usize, Ev::Pedal(true))];
            for k in 0..8 {
                ev.push((at(k as f32 * 0.060), Ev::On(60)));
                ev.push((at(k as f32 * 0.060 + 0.030), Ev::Off(60)));
            }
            let x = script(v, &ev, at(0.52));
            let peak = x.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
            assert!(
                peak <= fresh_peak * 1.585,
                "{name} stacked to {peak} against a fresh strike's {fresh_peak}"
            );
        }
    }

    /// **The first-strike pin.** One hash per shape of a strike from
    /// silence — the INIT VOICE that is the default, the tine piano that
    /// used to be, a percussive sync patch with feedback, a free-running pad
    /// — so a change meant for CONSECUTIVE strikes cannot quietly retune the
    /// engine on its way past. The piano's hash is the one it has always
    /// had: it lost its name, not its sound.
    #[test]
    fn a_first_strike_from_silence_is_unchanged() {
        fn fnv(x: &[f32]) -> u64 {
            let mut h = 0xcbf2_9ce4_8422_2325_u64;
            for s in x {
                for b in s.to_bits().to_le_bytes() {
                    h ^= u64::from(b);
                    h = h.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
            h
        }
        for (name, v, want) in [
            ("INIT VOICE", Voice::default(), 16_651_629_613_891_984_392),
            ("TANGENT EP", Voice::electric_piano(), 15_203_663_575_639_289_799),
            ("SYNC BELL", sync_bell(), 5_675_424_527_636_297_463),
            ("FREE PAD", free_pad(), 4_397_977_957_930_456_023),
        ] {
            let x = script(
                v,
                &[(0, Ev::Pedal(true)), (0, Ev::On(60)), (at(0.15), Ev::Off(60))],
                at(0.25),
            );
            assert_eq!(fnv(&x), want, "{name} first strike moved");
        }
    }
}
