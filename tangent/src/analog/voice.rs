//! One voice: three oscillators and a noise source into the mixer, the
//! mixer's overload, the filter of the moment, two contours, and the
//! component tolerance that makes this voice not quite the one beside it.
//!
//! **Where the nonlinearity sits, and why.** The signal path is oscillators
//! → mixer → overload → filter → amplifier, and the drive knob is the
//! amplifier's output patched back INTO the mixer — the cable every owner
//! of the 1970 machine ran from the headphone jack to the external input.
//! That loop is closed at 4× (see `filter`), so the feedback is a sample
//! late at 192 kHz rather than at 48, which is the difference between a
//! growl and a comb filter. The overload is a tanh at [`MIX_CLIP`], at
//! 4×, memoryless: the suite's ADAA knee was the first choice and cost
//! four f64 transcendentals per 4× sample — a quarter of the whole voice
//! — to suppress a fold that a soft knee under a band-limited mix does
//! not produce (the screamer needed it at 41 dB of gain into a hard
//! clip; this is a sum of three oscillators leaning on a ceiling). The
//! ladder's own differential pair saturates after it, INSIDE the
//! resonance loop.
//!
//! **The five inexactnesses** (brief §5): tolerance is drawn once per voice
//! index from a fixed seed ([`Tolerance::draw`]) — cutoff, envelope timing,
//! level and tune — and scaled by the Drift knob; oscillator wander is a
//! filtered random walk per oscillator ([`Voice::wander`]) scaled by Osc
//! Drift; phase free-runs (`osc`); the ladder's bass loss is left alone
//! (`filter`); and the drive path is above. **The brief's numbers are the
//! knob's MIDPOINT.** ±3 % on cutoff is a calibrated board; the top of the
//! knob is double that, a board that has not seen a trimmer in a decade,
//! and the shipped default of a quarter is half a calibrated board. The
//! seeds are constants: a bounce is the playback, twice, on every machine.
//!
//! **Modulation** (brief §2, the arrangement that is deliberately not a
//! matrix). One bus, two switches. The left side of Mod Mix is oscillator
//! three when its keyboard switch is OFF — the 1970 machine's trick, an
//! oscillator become a modulator — and the dedicated LFO when it is ON,
//! because a polysynth cannot spare an oscillator and cannot do without a
//! vibrato. The right side is noise. The bus reaches pitch through OSC MOD
//! and cutoff through FLT MOD, scaled by the wheel up to the Depth
//! ceiling. Pressure adds a little LFO vibrato and cutoff of its own. PWM
//! is the LFO on the pulse edges, always, unswitched.

use super::env::{Env, EnvCoeffs};
use super::filter::{fast_tanh, ladder_k, sem_damp, Cutoff, FilterModel, Ladder, Oversampler, Sem, Taps, CUTOFF_HZ};
use super::osc::{Noise, Osc, Rng, Wave, PWM_SWING};

/// One oscillator at full mixer level, at the filter's input, in the units
/// the saturators are set in. Three at full add to 1.2 and lean on the
/// overload; one does not.
pub const OSC_GAIN: f32 = 0.4;
/// Noise at full mixer level. White noise's peak is its full swing, so a
/// little under an oscillator's.
pub const NOISE_GAIN: f32 = 0.3;
/// The mixer overload's ceiling: where the sum stops adding.
pub const MIX_CLIP: f32 = 1.0;
/// How much of the voice's output the drive knob at full feeds back into
/// the mixer. Two is a loop that growls; past about three it latches.
pub const DRIVE_GAIN: f32 = 2.0;

/// The contour amount at full, in octaves of cutoff at the envelope's
/// peak. The 1970 machine's top setting swept most of the band.
pub const CONTOUR_OCT: f32 = 6.0;
/// Velocity to cutoff at full, octaves, at full velocity, unipolar.
pub const VEL_FILTER_OCT: f32 = 4.0;
/// Velocity to amplitude at full: the softest note sits this far under
/// the hardest, in decibels.
pub const VEL_AMP_DB: f32 = 36.0;
/// The modulation bus to pitch, at wheel and depth both full, in
/// semitones for a full-swing source. Square-law across Depth so the
/// vibrato range lives in the lower half of the knob.
pub const MOD_PITCH_SEMIS: f32 = 12.0;
/// The modulation bus to cutoff, octaves, likewise.
pub const MOD_FILTER_OCT: f32 = 4.0;
/// Pressure at full: cents of LFO vibrato, and octaves of cutoff.
pub const PRESSURE_VIB_CENTS: f32 = 40.0;
pub const PRESSURE_FILTER_OCT: f32 = 1.5;

/// The tolerances at the Drift knob's midpoint: the brief's calibrated
/// board.
pub const TOL_CUTOFF: f32 = 0.03;
pub const TOL_ENV: f32 = 0.05;
pub const TOL_LEVEL_DB: f32 = 0.4;
pub const TOL_TUNE_CENTS: f32 = 2.0;
/// Oscillator wander at the Osc Drift knob's midpoint, cents, and the
/// corner of the filter that shapes it.
pub const WANDER_CENTS: f32 = 3.0;
pub const WANDER_HZ: f32 = 0.1;

/// The key an untracked oscillator three sits at: middle C, so its range
/// switch and tune knob read in the same octaves as the tracked ones.
pub const UNTRACKED_KEY: f32 = 60.0;

/// Frames per chunk at the base rate: how often the voice recomputes its
/// per-chunk coefficients and the synth its smoothed dials. Two thirds of a
/// millisecond at 48 kHz, under any knob's audible step.
pub const CHUNK: usize = 32;

/// The component tolerance a voice was built with, as unit draws in −1..1;
/// the knob scales them at use.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tolerance {
    cutoff: f32,
    env: f32,
    level: f32,
    tune: f32,
    osc: [f32; 3],
}

impl Tolerance {
    /// Drawn from the voice index, and only from it.
    pub fn draw(voice: usize) -> Self {
        let mut r = Rng::new(0x5EED_0000 + voice as u32 * 7919);
        Self {
            cutoff: r.bipolar(),
            env: r.bipolar(),
            level: r.bipolar(),
            tune: r.bipolar(),
            osc: [r.bipolar(), r.bipolar(), r.bipolar()],
        }
    }
}

/// What the synth tells every voice once per chunk: the dials, already
/// smoothed and already mapped where the mapping is per instrument rather
/// than per voice.
#[derive(Clone, Copy, Debug)]
pub struct VoiceParams {
    pub wave: [Wave; 3],
    /// Octaves from 8′, per oscillator, from the range switches.
    pub range_oct: [f32; 3],
    /// Semitones from the tune knobs (oscillator one has none).
    pub tune_semis: [f32; 3],
    pub level: [f32; 3],
    pub o3_tracks: bool,
    pub sync: bool,
    pub pwm: f32,
    pub noise_level: f32,
    pub pink: bool,
    pub drive: f32,
    pub model: FilterModel,
    /// Cutoff in octaves above [`CUTOFF_HZ`]`.0`; modulation adds octaves.
    pub cutoff_oct: f32,
    pub emphasis: f32,
    pub sem_mode: f32,
    pub contour: f32,
    pub keytrack: f32,
    pub vel_filter: f32,
    pub vel_amp: f32,
    /// The two contours' dials: attack, decay, sustain, release.
    pub f_adsr: [f32; 4],
    pub a_adsr: [f32; 4],
    /// The Drift and Osc Drift knobs, 0..1.
    pub drift: f32,
    pub osc_drift: f32,
    /// Master tune, semitones, plus the bend, semitones.
    pub tune: f32,
    pub bend: f32,
    /// The wheel and Depth, already multiplied, 0..1, and the pressure
    /// amount likewise.
    pub wheel_amt: f32,
    pub pressure_amt: f32,
    pub mod_mix: f32,
    pub osc_mod: bool,
    pub filter_mod: bool,
    /// Glide as a per-sample one-pole coefficient; 1.0 is no glide.
    pub glide_coef: f32,
    /// LFO delay as a per-sample fade increment; 1.0 is no delay.
    pub lfo_fade_inc: f32,
    /// How far across the field the voice's `pan` place is taken: the
    /// Spread dial for poly, the unison Spread for a stack.
    pub spread: f32,
}

/// One voice of the instrument. Plain data with fixed-size state, so a
/// stolen voice can be copied into a corpse slot with one assignment.
#[derive(Clone, Copy, Debug)]
pub struct Voice {
    osc: [Osc; 3],
    noise: Noise,
    wander: [f32; 3],
    wander_rng: Rng,
    ladder: Ladder,
    sem: Sem,
    os: Oversampler,
    f_env: Env,
    a_env: Env,
    fb: f32,
    /// The last oscillator-three and noise samples, for the bus.
    o3_last: f32,
    noise_last: f32,
    // ── carried between the phases of one sample ──
    up4: [f32; 4],
    y4: [f32; 4],
    cut: Cutoff,
    ae: f32,
    tol: Tolerance,
    // ── the note ──
    pub key: u8,
    pub vel: f32,
    /// Semitones, gliding toward `target`.
    pitch: f32,
    target: f32,
    /// Detune and pan given by the allocator (unison, spread).
    pub detune_cents: f32,
    pub pan: f32,
    lfo_fade: f32,
    /// Allocation order, for the steal.
    pub age: u32,
    /// The key is down (or held by the pedal).
    pub gate: bool,
    pub sustained: bool,
    // ── per-chunk coefficients ──
    f_coeffs: EnvCoeffs,
    a_coeffs: EnvCoeffs,
    osc_mult: [f32; 3],
    vel_gain: f32,
    level_gain: f32,
    k: f32,
    damp: f32,
    cutoff_base: f32,
    pan_l: f32,
    pan_r: f32,
}

impl Voice {
    pub fn new(index: usize, rate: f32) -> Self {
        let seed = index as u32 * 16 + 1;
        Self {
            osc: [Osc::new(seed), Osc::new(seed + 1), Osc::new(seed + 2)],
            noise: Noise::new(seed + 3),
            wander: [0.0; 3],
            wander_rng: Rng::new(seed + 4),
            ladder: Ladder::new(),
            sem: Sem::new(),
            os: Oversampler::new(),
            f_env: Env::new(),
            a_env: Env::new(),
            fb: 0.0,
            o3_last: 0.0,
            noise_last: 0.0,
            up4: [0.0; 4],
            y4: [0.0; 4],
            cut: Cutoff::default(),
            ae: 0.0,
            tol: Tolerance::draw(index),
            key: 60,
            vel: 0.0,
            pitch: 60.0,
            target: 60.0,
            detune_cents: 0.0,
            pan: 0.0,
            lfo_fade: 0.0,
            age: 0,
            gate: false,
            sustained: false,
            f_coeffs: EnvCoeffs::new(rate, 0.0, 0.5, 0.5, 0.5, 1.0),
            a_coeffs: EnvCoeffs::new(rate, 0.0, 0.5, 0.5, 0.5, 1.0),
            osc_mult: [1.0; 3],
            vel_gain: 1.0,
            level_gain: 1.0,
            k: 0.0,
            damp: 1.0,
            cutoff_base: 0.0,
            pan_l: 0.707,
            pan_r: 0.707,
        }
    }

    /// Sounding at all: either contour still moving.
    #[inline]
    pub fn active(&self) -> bool {
        self.a_env.active()
    }

    /// The amplifier contour's level: how loud this voice is, for the
    /// steal's "quietest".
    #[inline]
    pub fn level(&self) -> f32 {
        self.a_env.level()
    }

    #[inline]
    pub fn releasing(&self) -> bool {
        !self.gate && self.a_env.active()
    }

    /// A note on this voice. **Superposition, never a reset**: the
    /// contours attack from where they are, the oscillators keep running,
    /// the filter keeps ringing. `retrigger` false is single-trigger
    /// legato — the pitch moves and nothing restarts.
    pub fn note_on(&mut self, key: u8, vel: f32, age: u32, retrigger: bool, glide: bool) {
        let was_active = self.active();
        self.key = key;
        self.vel = vel.clamp(0.0, 1.0);
        self.age = age;
        self.gate = true;
        self.sustained = false;
        self.target = f32::from(key);
        if !glide || !was_active {
            self.pitch = self.target;
        }
        if retrigger || !was_active {
            self.f_env.gate_on();
            self.a_env.gate_on();
            if !was_active {
                self.lfo_fade = 0.0;
            }
        }
    }

    /// The key came up. With the pedal down the note is held instead.
    pub fn note_off(&mut self, pedal: bool) {
        if pedal {
            self.sustained = true;
            return;
        }
        self.gate = false;
        self.sustained = false;
        self.f_env.gate_off();
        self.a_env.gate_off();
    }

    /// The pedal came up on a note whose key already had.
    pub fn pedal_up(&mut self) {
        if self.sustained {
            self.note_off(false);
        }
    }

    /// Slide to another pitch without a new note: mono legato.
    pub fn legato_to(&mut self, key: u8, glide: bool) {
        self.key = key;
        self.target = f32::from(key);
        if !glide {
            self.pitch = self.target;
        }
    }

    /// Reset everything but the oscillator phases — for a slot whose old
    /// tenant has just been copied out to a corpse. Silent by
    /// construction: the contours start from zero.
    pub fn vacate(&mut self) {
        for o in &mut self.osc {
            o.flush();
        }
        self.noise.clear();
        self.ladder.clear();
        self.sem.clear();
        self.os.clear();
        self.f_env.kill();
        self.a_env.kill();
        self.fb = 0.0;
        self.gate = false;
        self.sustained = false;
    }

    /// Everything off, phases included left alone: the instrument's clear.
    pub fn clear(&mut self) {
        self.vacate();
    }

    /// Once per chunk: the coefficients that do not move within it.
    pub fn prepare(&mut self, p: &VoiceParams, rate: f32) {
        let drift = p.drift * 2.0; // the brief's numbers at the midpoint
        let env_scale = 1.0 + TOL_ENV * drift * self.tol.env;
        self.f_coeffs = EnvCoeffs::new(rate, p.f_adsr[0], p.f_adsr[1], p.f_adsr[2], p.f_adsr[3], env_scale);
        self.a_coeffs = EnvCoeffs::new(rate, p.a_adsr[0], p.a_adsr[1], p.a_adsr[2], p.a_adsr[3], env_scale);
        // Wander: a filtered random walk per oscillator, one step per
        // chunk. The one-pole at WANDER_HZ turns white steps into a slow
        // drift; the scale is set so the midpoint's swing is about
        // WANDER_CENTS.
        let a = 1.0 - (-std::f32::consts::TAU * WANDER_HZ * CHUNK as f32 / rate).exp();
        let wander_scale = p.osc_drift * 2.0 * WANDER_CENTS;
        let voice_cents = TOL_TUNE_CENTS * drift * self.tol.tune + self.detune_cents;
        for i in 0..3 {
            // The white step is scaled up so the filtered walk's peaks
            // land near ±1 rather than the tiny value a one-pole leaves.
            let target = self.wander_rng.bipolar() * 6.0;
            self.wander[i] += a * (target - self.wander[i]);
            let w = self.wander[i].clamp(-1.0, 1.0) * wander_scale;
            let cents = voice_cents + TOL_TUNE_CENTS * drift * self.tol.osc[i] + w;
            let oct = p.range_oct[i] + p.tune_semis[i] / 12.0 + cents / 1200.0;
            self.osc_mult[i] = oct.exp2();
        }
        self.vel_gain = 10.0_f32.powf(-p.vel_amp * VEL_AMP_DB * (1.0 - self.vel) / 20.0);
        self.level_gain = 10.0_f32.powf(TOL_LEVEL_DB * drift * self.tol.level / 20.0);
        self.k = ladder_k(p.emphasis);
        self.damp = sem_damp(p.emphasis);
        // ±3 % of cutoff is ±0.043 octaves, added in the log domain.
        self.cutoff_base = p.cutoff_oct
            + (1.0 + TOL_CUTOFF * drift * self.tol.cutoff).log2()
            + p.keytrack * (f32::from(self.key) - 60.0) / 12.0
            + p.vel_filter * self.vel * VEL_FILTER_OCT;
        let angle = ((self.pan * p.spread).clamp(-1.0, 1.0) + 1.0) * 0.25 * std::f32::consts::PI;
        self.pan_l = angle.cos();
        self.pan_r = angle.sin();
    }

    /// **One sample, in three phases**, so the frame can interleave the
    /// voices: [`Voice::pre`] for every voice, then each of the four
    /// substeps ([`Voice::substep`]) for every voice, then
    /// [`Voice::post`]. The filter's substep is a chain of dependent
    /// operations a hundred cycles long; run voice by voice those chains
    /// queue up, run substep by substep across the voices the CPU
    /// overlaps them. Same arithmetic, a third of the time.
    ///
    /// `pre`: contours, glide, the modulation bus, the oscillators, the
    /// mixer, the cutoff, and the upsample into `up4`.
    #[inline]
    pub fn pre(&mut self, p: &VoiceParams, taps: &Taps, ctx: &TickCtx, lfo_raw: f32) {
        let fe = self.f_env.tick(&self.f_coeffs);
        self.ae = self.a_env.tick(&self.a_coeffs);
        if self.lfo_fade < 1.0 {
            self.lfo_fade = (self.lfo_fade + p.lfo_fade_inc).min(1.0);
        }
        let lfo_v = lfo_raw * self.lfo_fade;
        self.pitch += p.glide_coef * (self.target - self.pitch);

        // The modulation bus.
        let left = if p.o3_tracks { lfo_v } else { self.o3_last };
        let bus = left + p.mod_mix * (self.noise_last - left);
        let mut pitch_mod = lfo_v * p.pressure_amt * PRESSURE_VIB_CENTS / 100.0;
        if p.osc_mod {
            pitch_mod += bus * ctx.pitch_depth;
        }
        let mut cutoff_mod = p.pressure_amt * PRESSURE_FILTER_OCT;
        if p.filter_mod {
            cutoff_mod += bus * ctx.filter_depth;
        }

        // Oscillator frequencies: one exp2 for everything the voice
        // shares, the per-oscillator multipliers from `prepare`.
        let semis = self.pitch + p.tune + p.bend + pitch_mod;
        let common = 440.0 * ((semis - 69.0) / 12.0).exp2();
        let pw_mod = lfo_raw * p.pwm * PWM_SWING;
        let inc2 = common * self.osc_mult[1] * ctx.inv_rate;
        let o2 = self.osc[1].tick(p.wave[1], inc2, pw_mod, None);
        let sync = if p.sync { self.osc[1].wrap() } else { None };
        let inc1 = common * self.osc_mult[0] * ctx.inv_rate;
        let o1 = self.osc[0].tick(p.wave[0], inc1, pw_mod, sync);
        let f3 = if p.o3_tracks { common } else { ctx.untracked_hz };
        let inc3 = f3 * self.osc_mult[2] * ctx.inv_rate;
        let o3 = self.osc[2].tick(p.wave[2], inc3, pw_mod, None);
        let n = self.noise.tick(p.pink);
        self.o3_last = o3;
        self.noise_last = n;

        let mix = OSC_GAIN * (p.level[0] * o1 + p.level[1] * o2 + p.level[2] * o3)
            + NOISE_GAIN * p.noise_level * n;

        // The cutoff for this sample, in octaves then hertz, then the
        // coefficients the four substeps share.
        let oct = (self.cutoff_base + ctx.contour_oct * fe + cutoff_mod).clamp(0.0, ctx.top_oct);
        let hz = CUTOFF_HZ.0 * oct.exp2();
        self.cut = Cutoff::new(hz, ctx.rate4, self.damp);
        self.up4 = self.os.up(taps, mix);
    }

    /// Substep `j` of four at 4×: the feedback into the mixer, the
    /// overload, the filter, and the amplifier on the fed-back copy.
    #[inline]
    pub fn substep(&mut self, p: &VoiceParams, ctx: &TickCtx, j: usize) {
        let m = self.up4[j] + ctx.drive * self.fb;
        let clipped = MIX_CLIP * fast_tanh(m * (1.0 / MIX_CLIP));
        let y = match p.model {
            FilterModel::Ladder => self.ladder.tick(clipped, &self.cut, self.k),
            FilterModel::Sem => self.sem.tick(clipped, &self.cut, self.damp, p.sem_mode),
        };
        self.fb = y * self.ae;
        self.y4[j] = y;
    }

    /// The downsample, the amplifier, the pan: the sample's stereo output.
    #[inline]
    pub fn post(&mut self, taps: &Taps) -> (f32, f32) {
        let y = self.os.down(taps, self.y4);
        let v = y * self.ae * self.vel_gain * self.level_gain;
        (v * self.pan_l, v * self.pan_r)
    }

    /// One sample, the three phases in a row: the bench's road.
    #[inline]
    pub fn tick(&mut self, p: &VoiceParams, taps: &Taps, ctx: &TickCtx, lfo_raw: f32) -> (f32, f32) {
        self.pre(p, taps, ctx, lfo_raw);
        for j in 0..4 {
            self.substep(p, ctx, j);
        }
        self.post(taps)
    }

    /// Render `frames` (≤ [`CHUNK`]) into `out`, interleaved stereo,
    /// ADDING to what is there, with a fade of `gain` stepping by
    /// `gain_step` per sample. The bench's one-voice road; the frame
    /// itself calls [`Voice::tick`] sample-major.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        p: &VoiceParams,
        taps: &Taps,
        lfo: &[f32],
        rate: f32,
        out: &mut [f32],
        frames: usize,
        gain: f32,
        gain_step: f32,
    ) {
        let ctx = TickCtx::new(p, rate);
        for i in 0..frames {
            let (l, r) = self.tick(p, taps, &ctx, lfo[i]);
            let g = gain + gain_step * i as f32;
            out[2 * i] += l * g;
            out[2 * i + 1] += r * g;
        }
    }
}

/// The constants a chunk's samples share, derived once from the dials.
#[derive(Clone, Copy, Debug)]
pub struct TickCtx {
    pub inv_rate: f32,
    pub rate4: f32,
    pub untracked_hz: f32,
    pub drive: f32,
    pub contour_oct: f32,
    pub top_oct: f32,
    pub pitch_depth: f32,
    pub filter_depth: f32,
}

impl TickCtx {
    pub fn new(p: &VoiceParams, rate: f32) -> Self {
        Self {
            inv_rate: 1.0 / rate,
            rate4: 4.0 * rate,
            untracked_hz: 440.0 * ((UNTRACKED_KEY - 69.0) / 12.0).exp2(),
            drive: p.drive * DRIVE_GAIN,
            contour_oct: p.contour * CONTOUR_OCT,
            top_oct: (CUTOFF_HZ.1 / CUTOFF_HZ.0).log2(),
            pitch_depth: p.wheel_amt * MOD_PITCH_SEMIS,
            filter_depth: p.wheel_amt * MOD_FILTER_OCT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tolerance draws are the voice index's and nothing else's: the same
    /// index twice is the same board, two indices are two boards.
    #[test]
    fn tolerance_is_drawn_from_the_index_alone() {
        let a = Tolerance::draw(3);
        let b = Tolerance::draw(3);
        let c = Tolerance::draw(4);
        assert_eq!(a.cutoff, b.cutoff);
        assert_eq!(a.osc, b.osc);
        assert_ne!(a.cutoff, c.cutoff);
        for v in 0..16 {
            let t = Tolerance::draw(v);
            for x in [t.cutoff, t.env, t.level, t.tune, t.osc[0], t.osc[1], t.osc[2]] {
                assert!((-1.0..=1.0).contains(&x));
            }
        }
    }
}
