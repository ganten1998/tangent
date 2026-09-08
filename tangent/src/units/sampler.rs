//! The sampler kernel: one file, played back polyphonically from a key.
//!
//! # What this module is, and what it deliberately is not
//!
//! It is the arithmetic of playing a piece of recorded audio at a rate the
//! keyboard chooses, thirty-two times at once, with an envelope on each and a
//! loop that does not click. It is NOT the thing that owns the audio. The
//! samples arrive as a [`SampleRef`] borrowed for the length of one
//! [`Sampler::render`] call and are gone again when it returns — the kernel
//! holds no `Arc`, no `Vec` of audio, no path and no file handle.
//!
//! That is not tidiness, it is the allocation contract. The engine keeps the
//! decoded clip in ONE place (`instrument::Renderer`'s per-lane store, swapped
//! in over a ring exactly as a drum kit is) so that the displaced sample is
//! freed on the host thread and never in the callback. If this kernel held its
//! own `Arc<Clip>`, every reload would put a `drop` — and therefore a `free` of
//! tens of megabytes — on the audio thread, which is the one place that must
//! not happen. Borrowing costs nothing and makes that mistake unspellable.
//!
//! It is also what keeps this file in the Minimal build. `units/mod.rs`'s law
//! is "nothing in here may assume an engine"; `tangent_record::decode::Clip`
//! IS the engine, and it is optional besides. A slice of `f32` is neither.
//!
//! # The four things worth reading before changing anything
//!
//! 1. **Pitch is a ratio, and glide moves it geometrically.** A semitone is a
//!    multiply, not an add, so portamento walks the ratio by a constant factor
//!    per sample rather than walking semitones and calling `exp2` on every
//!    frame of every voice. Thirty-two voices at 48 kHz is 1.5 million
//!    transcendental calls a second saved, for identical audio: a geometric
//!    walk in ratio IS a linear walk in semitones.
//! 2. **The loop crossfade is forward, not backward.** See [`Sampler::render`]
//!    — a backward crossfade needs material BEFORE the loop start, and a
//!    sample cropped to start at frame 0 has none, which is precisely the
//!    case somebody loops first.
//! 3. **Every buffer is cut in [`Sampler::new`]** at [`MAX_VOICES`], because
//!    the voice cap is a dial and a dial that reallocated would be a malloc on
//!    the callback. Raising the cap steals; it never grows.
//! 4. **The dials arrive normalised 0..=1**, the house contract, and the maps
//!    between that and seconds, cents and semitones live here beside the DSP —
//!    with their INVERSES exported, so the face can print a number and read one
//!    back without a second copy of the taper. Two copies of a taper is how a
//!    knob comes to disagree with its own readout.

// Nothing reaches this module in a Minimal build: `builtin_fx` is
// recorder-gated and it is the only caller. The kernel compiles and tests
// itself there regardless, which is the point of the suite's layout.
#![allow(dead_code)]

// ── how far the dials travel ────────────────────────────────────────────────

/// The most voices this kernel will ever hold, and therefore the most it ever
/// allocates. The Max Voices dial caps how many of these may SOUND; it does
/// not decide how many exist. See [`Sampler::new`].
pub const MAX_VOICES: usize = 32;

/// Where the volume dial travels, as linear gain. 1.0 — unity — is the middle
/// of the travel, so the shipped default changes nothing.
pub const VOL_MAX: f32 = 2.0;

/// How far Transpose reaches, in semitones each way. Two octaves is the range
/// that covers "this was sampled an octave out" without turning the dial into
/// a tuning fork.
pub const TRANSPOSE_SEMIS: f32 = 24.0;

/// How far Fine reaches, in CENTS each way. The owner asked for "pitch change
/// (hz)" and the face prints the Hz this lands on beside the cents — but what
/// is STORED is cents, because a fixed Hz offset is a different interval at
/// every key and a sampler is played across a keyboard.
pub const FINE_CENTS: f32 = 100.0;

/// The envelope's longest attack, decay and release, in seconds.
pub const ATTACK_SECS: f32 = 5.0;
pub const DECAY_SECS: f32 = 5.0;
pub const RELEASE_SECS: f32 = 10.0;

/// The longest loop crossfade, in milliseconds. Half a second is long enough
/// to hide a seam in a sustained string note and short enough that the dial
/// still has resolution where drums need it.
pub const XFADE_MS: f32 = 500.0;

/// The longest portamento, in seconds.
pub const PORTA_SECS: f32 = 2.0;

/// The widest bend range the stepped dial offers, in semitones. Twenty-four
/// because whammy-bar and pitch-ribbon patches ask for two octaves, and the
/// step is a whole semitone so every position is a real interval.
pub const BEND_MAX_SEMIS: i32 = 24;

/// What the three Sample Mode positions are called, in step order. The face
/// prints these and `builtin_fx::choices` hands them out; one list, so the
/// panel and the sound cannot disagree about what position 2 means.
pub const MODE_LABELS: [&str; 3] = ["pitched", "fixed", "one-shot"];

/// The three positions, by name.
pub const MODE_PITCHED: usize = 0;
pub const MODE_FIXED: usize = 1;
pub const MODE_ONESHOT: usize = 2;

/// The parameter ids, in the order the published table declares them. Both
/// halves of the feature index by these, so they are as append-only as the
/// symbols beside them in `builtin_fx::SAMPLER_FACE_PARAMS`.
pub const P_VOL: u32 = 0;
pub const P_MODE: u32 = 1;
pub const P_TRANSPOSE: u32 = 2;
pub const P_FINE: u32 = 3;
pub const P_ATTACK: u32 = 4;
pub const P_DECAY: u32 = 5;
pub const P_SUSTAIN: u32 = 6;
pub const P_RELEASE: u32 = 7;
pub const P_START: u32 = 8;
pub const P_END: u32 = 9;
pub const P_NOTEOFF: u32 = 10;
pub const P_LOOP: u32 = 11;
pub const P_XFADE: u32 = 12;
pub const P_VOICES: u32 = 13;
pub const P_PORTA: u32 = 14;
pub const P_BEND: u32 = 15;

/// How many dials the sampler publishes.
pub const PARAM_COUNT: usize = 16;

// ── the dial maps, and their inverses ───────────────────────────────────────

/// A time dial's seconds, from its normalised position: a CUBE law.
///
/// **Why not linear.** The published range for Attack is 0..5 s and its rest
/// is 5 ms, which on a linear knob is one part in a thousand of the travel —
/// a default that sits invisibly against the end stop and a first millisecond
/// nobody can dial. The cube puts 5 ms at exactly a tenth of the way round,
/// gives the whole first tenth of a second most of the first half of the
/// sweep, and still reaches the published maximum at the top. Every published
/// default in the table lands on a round-ish position because of it.
#[inline]
pub fn secs(v: f32, max: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    max * v * v * v
}

/// [`secs`] backwards: where a knob must stand to read `s` seconds. Exported
/// so the face can turn a typed number into a position without keeping a
/// second copy of the taper — two copies is how a knob comes to disagree with
/// its own readout.
#[inline]
pub fn secs_norm(s: f32, max: f32) -> f32 {
    if max <= 0.0 {
        return 0.0;
    }
    (s.max(0.0) / max).cbrt().clamp(0.0, 1.0)
}

/// The index a stepped dial sits on, out of `steps` positions. The analog's
/// own rounding, restated here so this module stands alone with its bench —
/// a test holds the two equal.
#[inline]
pub fn step_index(v: f32, steps: i32) -> usize {
    let last = (steps.max(2) - 1) as f32;
    ((v.clamp(0.0, 1.0) * last).round() as usize).min(steps.max(2) as usize - 1)
}

/// Where a stepped dial must stand to sit on `index` of `steps`.
#[inline]
pub fn step_norm(index: usize, steps: i32) -> f32 {
    let last = (steps.max(2) - 1) as f32;
    (index as f32 / last).clamp(0.0, 1.0)
}

/// A dial that travels symmetrically about a centre — Transpose and Fine —
/// read out in its own units.
#[inline]
pub fn bipolar(v: f32, half: f32) -> f32 {
    (v.clamp(0.0, 1.0) - 0.5) * 2.0 * half
}

/// [`bipolar`] backwards.
#[inline]
pub fn bipolar_norm(x: f32, half: f32) -> f32 {
    if half <= 0.0 {
        return 0.5;
    }
    (x / half * 0.5 + 0.5).clamp(0.0, 1.0)
}

/// A knob value with the wire's dirt washed off. A settings file somebody has
/// edited, an automation curve that overshot, a NaN out of a plugin bridge:
/// all three arrive here, and none of them may reach the arithmetic.
#[inline]
fn dial(v: f32, rest: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        rest
    }
}

// ── what the kernel is handed ───────────────────────────────────────────────

/// The audio a sampler bay is playing, borrowed for one render call.
///
/// Interleaved by `channels`, at `rate`. The engine decodes at the device's
/// own rate so `rate` is nearly always the render rate and the correction
/// below is exactly 1.0 — it is carried anyway because "nearly always" is not
/// a thing to write a playback rate on, and an offline bounce at 96 k with a
/// cache decoded at 48 k is one hot-plug away.
#[derive(Clone, Copy)]
pub struct SampleRef<'a> {
    pub data: &'a [f32],
    pub channels: usize,
    pub rate: f32,
    /// The key at which this file plays at unity speed.
    pub root_key: u8,
}

impl SampleRef<'_> {
    /// How many frames the file holds. A ragged tail — a buffer whose length
    /// is not a whole number of frames — is truncated rather than indexed
    /// past.
    #[inline]
    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.data.len() / self.channels
        }
    }
}

/// One thing that happens inside a block, at `at` frames from its start.
/// Delivered to [`Sampler::render`] sorted by `at`, the analog's contract.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    NoteOn { at: usize, key: u8, vel: f32 },
    NoteOff { at: usize, key: u8 },
    /// Pitch bend, −1..1 of the Bend Range dial.
    Bend { at: usize, amount: f32 },
    /// The sustain pedal. A held pedal defers every note off until it lifts.
    Sustain { at: usize, down: bool },
    /// Everything off, now — the wire's two panic buttons.
    AllNotesOff { at: usize },
    /// One automation point, normalised, as the host delivers it.
    Param { at: usize, id: u32, value: f32 },
}

impl Event {
    #[inline]
    fn at(&self) -> usize {
        match *self {
            Event::NoteOn { at, .. }
            | Event::NoteOff { at, .. }
            | Event::Bend { at, .. }
            | Event::Sustain { at, .. }
            | Event::AllNotesOff { at }
            | Event::Param { at, .. } => at,
        }
    }
}

// ── one voice ───────────────────────────────────────────────────────────────

/// Where a voice's envelope is. `Off` is the free-slot marker: a voice is
/// dead when and only when its stage is `Off`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Stage {
    #[default]
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// One note sounding: a position in the file, a rate, and an envelope.
#[derive(Clone, Copy, Debug)]
struct Voice {
    stage: Stage,
    key: u8,
    /// Whether the key is still down. A voice whose key is up but which is
    /// still sounding is either pedalled, one-shot, or in its release.
    held: bool,
    /// Held down by the SUSTAIN pedal after its key came up.
    pedalled: bool,
    /// Captured at note-on: a one-shot ignores note off for its whole life,
    /// even if the dial moves while it sounds. Changing what a note obeys
    /// half way through it is a dial that reaches backwards in time.
    oneshot: bool,
    velocity: f32,
    /// Fractional frame position into the file. Fractional because the step is.
    pos: f64,
    /// This voice's own pitch ratio from the key alone — root-relative, and
    /// 1.0 in the two fixed modes.
    base: f64,
    /// The glide's remaining factor, multiplying `base`. 1.0 when settled.
    glide: f64,
    /// What `glide` is multiplied by each frame while it walks home.
    glide_mul: f64,
    env: f32,
    /// The level the release started from, so a note let go during its
    /// attack falls from where it actually was.
    rel_from: f32,
    /// Start order, for the steal: the OLDEST sounding voice loses.
    age: u64,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            stage: Stage::Off,
            key: 0,
            held: false,
            pedalled: false,
            oneshot: false,
            velocity: 0.0,
            pos: 0.0,
            base: 1.0,
            glide: 1.0,
            glide_mul: 1.0,
            env: 0.0,
            rel_from: 0.0,
            age: 0,
        }
    }
}

impl Voice {
    #[inline]
    fn sounding(&self) -> bool {
        self.stage != Stage::Off
    }
}

// ── the kernel ──────────────────────────────────────────────────────────────

/// The sampler: thirty-two voices, one file, sixteen dials.
pub struct Sampler {
    sample_rate: f32,
    /// Every dial, normalised, indexed by id. Written by `set_param` and by
    /// [`Event::Param`]; read through the maps above.
    params: [f32; PARAM_COUNT],
    /// Cut once, at [`MAX_VOICES`], and never resized. The Max Voices dial
    /// caps how many of these may sound.
    voices: Vec<Voice>,
    /// Rising, so "oldest" is a comparison and not a timestamp.
    next_age: u64,
    /// The last key played, in root-relative semitones, for portamento to
    /// glide FROM. `None` until the first note, so the first note of a
    /// session does not slide in from the root.
    last_semis: Option<f32>,
    /// The wheel's position, −1..1 of the Bend Range dial.
    bend: f32,
    /// The sustain pedal.
    pedal: bool,
}

impl Sampler {
    /// Every allocation this kernel will ever make, made here.
    pub fn new(sample_rate: f32) -> Self {
        let rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let mut params = [0.0f32; PARAM_COUNT];
        for (id, slot) in params.iter_mut().enumerate() {
            *slot = default_norm(id as u32);
        }
        Self {
            sample_rate: rate,
            params,
            voices: vec![Voice::default(); MAX_VOICES],
            next_age: 1,
            last_semis: None,
            bend: 0.0,
            pedal: false,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// One dial, normalised, by id. Out of range is ignored rather than
    /// panicking: an automation curve addressed at a parameter this build
    /// does not have is a stale project, not a crash.
    pub fn set_param(&mut self, id: u32, value: f32) {
        let Some(slot) = self.params.get_mut(id as usize) else {
            return;
        };
        *slot = dial(value, default_norm(id));
    }

    pub fn param(&self, id: u32) -> f32 {
        self.params.get(id as usize).copied().unwrap_or(0.0)
    }

    /// Every voice off, the dials left where they stand — the house `clear`.
    /// A load path is `create`-then-`clear`, so this must not touch the
    /// settings it was just given.
    pub fn clear(&mut self) {
        for v in self.voices.iter_mut() {
            *v = Voice::default();
        }
        self.last_semis = None;
        self.bend = 0.0;
        self.pedal = false;
    }

    /// How many voices are sounding. The steal's evidence, and the face's
    /// voice count.
    pub fn active(&self) -> usize {
        self.voices.iter().filter(|v| v.sounding()).count()
    }

    /// Every byte of `Vec` capacity in the kernel, for the test that proves
    /// `render` never allocates: equal before and after means no buffer so
    /// much as considered growing. The convolver's and the denoise's own
    /// accessor, for the same contract.
    #[cfg(test)]
    fn footprint(&self) -> usize {
        self.voices.capacity() * size_of::<Voice>()
    }

    // ── the dials, read ─────────────────────────────────────────────────────

    fn mode(&self) -> usize {
        step_index(self.params[P_MODE as usize], 3)
    }

    fn max_voices(&self) -> usize {
        step_index(self.params[P_VOICES as usize], MAX_VOICES as i32) + 1
    }

    fn bend_semis(&self) -> f32 {
        step_index(self.params[P_BEND as usize], BEND_MAX_SEMIS + 1) as f32
    }

    /// The cropped region, in frames, as `(start, end)` with `end > start`
    /// guaranteed whenever the file has a frame in it at all.
    ///
    /// **The two dials are sorted, not trusted in order.** Dragging End below
    /// Start is one mouse gesture; a region whose end is before its beginning
    /// is a negative length, and a negative length reaches the audio thread as
    /// a loop that never ends or a subtraction that wraps. Whichever is
    /// smaller is the start.
    fn region(&self, frames: usize) -> (f64, f64) {
        if frames == 0 {
            return (0.0, 0.0);
        }
        let a = self.params[P_START as usize].clamp(0.0, 1.0);
        let b = self.params[P_END as usize].clamp(0.0, 1.0);
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let n = frames as f64;
        let start = (f64::from(lo) * n).floor().clamp(0.0, n - 1.0);
        let end = (f64::from(hi) * n).ceil().clamp(start + 1.0, n);
        (start, end)
    }

    // ── notes ───────────────────────────────────────────────────────────────

    /// Where a key sits relative to the root, in semitones, under the current
    /// mode. Fixed and one-shot answer zero: every key plays the file at the
    /// speed it was recorded at, which is what makes a sampler a drum machine.
    fn key_semis(&self, key: u8, root: u8) -> f32 {
        if self.mode() == MODE_PITCHED {
            f32::from(key) - f32::from(root)
        } else {
            0.0
        }
    }

    /// Start a note, stealing if the cap is full.
    ///
    /// **The steal takes the OLDEST SOUNDING voice, and it takes it whole.**
    /// Fading the loser out over a few milliseconds would be kinder and is
    /// what the drum pads do; it is not done here because a sampler's voices
    /// are already enveloped and a stolen voice at the cap is, by
    /// construction, the one that has been decaying longest. Growing the
    /// pool instead is the thing that must never happen: that is a `Vec`
    /// push on the audio callback.
    fn note_on(&mut self, key: u8, vel: f32, root: u8, frames: usize) {
        let cap = self.max_voices().min(self.voices.len());
        let (start, _) = self.region(frames);
        let semis = self.key_semis(key, root);
        let base = f64::from(semis_ratio(semis));

        // Portamento: begin at the last key's pitch and walk home. In ratio
        // space, so the per-frame cost is one multiply — see the module note.
        let porta = secs(self.params[P_PORTA as usize], PORTA_SECS);
        let (glide, glide_mul) = match self.last_semis {
            Some(prev) if porta > 0.0 && (prev - semis).abs() > 1e-6 => {
                let n = (porta * self.sample_rate).max(1.0);
                let delta = f64::from(prev - semis);
                let g = f64::from(semis_ratio(delta as f32));
                // The factor that takes `g` to 1.0 in exactly `n` frames.
                (g, (1.0f64 / g).powf(1.0 / f64::from(n)))
            }
            _ => (1.0, 1.0),
        };
        self.last_semis = Some(semis);

        // A free slot first; the oldest sounding voice only if there is none,
        // or if the cap has been turned DOWN under a chord that was already
        // playing. Both roads end in the same assignment.
        let mut chosen = None;
        let mut live = 0usize;
        for (i, v) in self.voices.iter().enumerate() {
            if v.sounding() {
                live += 1;
            } else if chosen.is_none() {
                chosen = Some(i);
            }
        }
        let slot = if live >= cap || chosen.is_none() {
            // The oldest sounding voice. `age` rises for ever, so this is a
            // comparison and never a clock read.
            let mut oldest = 0usize;
            let mut best = u64::MAX;
            for (i, v) in self.voices.iter().enumerate() {
                if v.sounding() && v.age < best {
                    best = v.age;
                    oldest = i;
                }
            }
            oldest
        } else {
            chosen.unwrap_or(0)
        };

        let attack = secs(self.params[P_ATTACK as usize], ATTACK_SECS);
        let oneshot = self.mode() == MODE_ONESHOT;
        let age = self.next_age;
        self.next_age = self.next_age.wrapping_add(1);
        let Some(v) = self.voices.get_mut(slot) else {
            return;
        };
        *v = Voice {
            stage: if attack > 0.0 {
                Stage::Attack
            } else {
                Stage::Decay
            },
            key,
            held: true,
            pedalled: false,
            oneshot,
            velocity: vel.clamp(0.0, 1.0),
            pos: start,
            base,
            glide,
            glide_mul,
            env: if attack > 0.0 { 0.0 } else { 1.0 },
            rel_from: 0.0,
            age,
        };
    }

    /// Let a key go. What that MEANS is the Obey Note Off dial's answer, and
    /// a one-shot's answer is always "nothing" — the mode is the whole point
    /// of the mode.
    fn note_off(&mut self, key: u8) {
        let obey = self.params[P_NOTEOFF as usize] >= 0.5;
        let pedal = self.pedal;
        for v in self.voices.iter_mut() {
            if !v.sounding() || v.key != key || !v.held {
                continue;
            }
            v.held = false;
            if v.oneshot || !obey {
                continue;
            }
            if pedal {
                v.pedalled = true;
                continue;
            }
            release(v);
        }
    }

    fn lift_pedal(&mut self) {
        for v in self.voices.iter_mut() {
            if v.sounding() && v.pedalled {
                v.pedalled = false;
                release(v);
            }
        }
    }

    // ── the block ───────────────────────────────────────────────────────────

    /// Render `frames` of `channels`-wide interleaved audio, REPLACING what
    /// was in `out`.
    ///
    /// `events` must be sorted by offset — the seat above sorts them, exactly
    /// as the analog's does — and `sample` is the file the bay is loaded with,
    /// borrowed for this call only. With no file the notes are still tracked
    /// (so a sample arriving mid-chord is not met by a kernel that has
    /// forgotten what is held) and the output is silence.
    ///
    /// # The loop seam
    ///
    /// The crossfade is FORWARD: the last `xfade` of the region is mixed,
    /// equal-power, with the same length taken from the region's head, and the
    /// read position lands just past that head material when it wraps. The
    /// obvious alternative — fade the region's head against the material
    /// BEFORE it — is what hardware samplers with a pre-roll do, and it is
    /// unavailable here for the case people meet first: a file cropped to
    /// start at frame 0 has nothing before it, so the seam would be exactly as
    /// bare as it was without the dial. Forward costs the loop `xfade` of its
    /// period, which is a length the dial names.
    ///
    /// Equal power (cosine/sine) rather than linear, because the two sides of
    /// a loop seam are the same instrument at nearly the same phase: a linear
    /// crossfade of two correlated signals dips about 3 dB in the middle, and
    /// a periodic 3 dB dip is a tremolo at the loop rate.
    pub fn render(
        &mut self,
        out: &mut [f32],
        frames: usize,
        channels: usize,
        events: &[Event],
        sample: Option<SampleRef<'_>>,
    ) {
        if channels == 0 {
            return;
        }
        let n = frames.min(out.len() / channels);
        for s in out.iter_mut().take(n * channels) {
            *s = 0.0;
        }
        let file_frames = sample.map_or(0, |s| s.frames());
        // **The dials are read ONCE per block, not once per frame.** Every
        // number in `Derived` costs a `powf`, a `cbrt` or a rounding, and a
        // sampler runs them for thirty-two voices; doing that per frame is
        // 48,000 transcendental calls a second for values that change at
        // human speed. Automation still lands sample-accurately, because an
        // [`Event::Param`] re-derives on the spot — the only frames that pay
        // are the ones a curve actually moved something on.
        let mut d = self.derive(sample, file_frames);
        let mut ev = 0usize;
        for f in 0..n {
            // Everything that happens at or before this frame, in order. `<=`
            // and not `==`: an offset past the block's end has been clamped
            // into it by the seat, and an unsorted stray must still be heard
            // rather than silently held for ever.
            while ev < events.len() && events[ev].at() <= f {
                let e = events[ev];
                ev += 1;
                self.apply(e, sample, file_frames);
                if matches!(e, Event::Param { .. } | Event::Bend { .. }) {
                    d = self.derive(sample, file_frames);
                }
            }
            self.frame(out, f, channels, sample, &d);
        }
        // Events past the last frame of a block — which a zero-length block
        // makes all of them — are applied rather than dropped. A note-on lost
        // because the callback asked for no frames is a note that never plays.
        while ev < events.len() {
            let e = events[ev];
            ev += 1;
            self.apply(e, sample, file_frames);
        }
    }

    fn apply(&mut self, e: Event, sample: Option<SampleRef<'_>>, file_frames: usize) {
        let root = sample.map_or(60, |s| s.root_key);
        match e {
            Event::NoteOn { key, vel, .. } => {
                if vel <= 0.0 {
                    // A zero-velocity note on IS a note off on the wire, and
                    // some controllers send nothing else.
                    self.note_off(key);
                } else {
                    self.note_on(key, vel, root, file_frames);
                }
            }
            Event::NoteOff { key, .. } => self.note_off(key),
            Event::Bend { amount, .. } => {
                self.bend = if amount.is_finite() {
                    amount.clamp(-1.0, 1.0)
                } else {
                    0.0
                };
            }
            Event::Sustain { down, .. } => {
                self.pedal = down;
                if !down {
                    self.lift_pedal();
                }
            }
            Event::AllNotesOff { .. } => {
                for v in self.voices.iter_mut() {
                    *v = Voice::default();
                }
                self.pedal = false;
            }
            Event::Param { id, value, .. } => self.set_param(id, value),
        }
    }

    /// Everything the block's dials say, worked out once. See [`Sampler::render`].
    fn derive(&self, sample: Option<SampleRef<'_>>, file_frames: usize) -> Derived {
        let sr = self.sample_rate.max(1.0);
        let (start, end) = self.region(file_frames);
        let looping = self.params[P_LOOP as usize] >= 0.5;
        let region = end - start;
        // The crossfade, in frames, clamped so it can never be longer than
        // half the region it has to fit inside — a fade as long as the loop is
        // a loop with no period at all.
        let xfade = if looping {
            let want =
                f64::from(secs(self.params[P_XFADE as usize], XFADE_MS / 1000.0)) * f64::from(sr);
            want.min((region - 1.0).max(0.0) * 0.5).max(0.0)
        } else {
            0.0
        };
        // The block-constant half of the pitch: the dials and the wheel, plus
        // the correction for a file decoded at some other rate. Transpose is
        // rounded because the brief calls it whole semitones — a knob that
        // reads "+7" and plays +6.91 is a knob that is lying.
        let semis = bipolar(self.params[P_TRANSPOSE as usize], TRANSPOSE_SEMIS).round()
            + bipolar(self.params[P_FINE as usize], FINE_CENTS) / 100.0
            + self.bend * self.bend_semis();
        let rate_fix = match sample {
            Some(s) if s.rate > 0.0 => f64::from(s.rate) / f64::from(sr),
            _ => 1.0,
        };
        let sustain = self.params[P_SUSTAIN as usize].clamp(0.0, 1.0);
        let attack = secs(self.params[P_ATTACK as usize], ATTACK_SECS);
        let decay = secs(self.params[P_DECAY as usize], DECAY_SECS);
        Derived {
            start,
            end,
            region,
            looping,
            xfade,
            global: f64::from(semis_ratio(semis)) * rate_fix,
            vol: self.params[P_VOL as usize].clamp(0.0, 1.0) * VOL_MAX,
            sustain,
            a_step: if attack > 0.0 { 1.0 / (attack * sr) } else { 1.0 },
            d_step: if decay > 0.0 {
                (1.0 - sustain) / (decay * sr)
            } else {
                1.0
            },
            release_frames: secs(self.params[P_RELEASE as usize], RELEASE_SECS) * sr,
        }
    }

    /// One frame, every voice.
    fn frame(
        &mut self,
        out: &mut [f32],
        f: usize,
        channels: usize,
        sample: Option<SampleRef<'_>>,
        d: &Derived,
    ) {
        let Some(sample) = sample else {
            return;
        };
        let file_frames = sample.frames();
        if file_frames == 0 || sample.channels == 0 {
            return;
        }
        let (mut l, mut r) = (0.0f32, 0.0f32);
        for v in self.voices.iter_mut() {
            if !v.sounding() {
                continue;
            }
            // **The envelope first, so a voice that ends this frame does not
            // also read a sample.** Order matters only for the last frame of
            // a note, and only by one sample — which is exactly the sample a
            // click is made of.
            match v.stage {
                Stage::Attack => {
                    v.env += d.a_step;
                    if v.env >= 1.0 {
                        v.env = 1.0;
                        v.stage = Stage::Decay;
                    }
                }
                Stage::Decay => {
                    v.env -= d.d_step;
                    if v.env <= d.sustain {
                        v.env = d.sustain;
                        v.stage = Stage::Sustain;
                    }
                }
                Stage::Sustain => {
                    v.env = d.sustain;
                    // A zero sustain that has finished decaying IS the end of
                    // the note; holding a silent voice would spend a slot the
                    // cap has to give somebody.
                    if d.sustain <= 0.0 {
                        v.stage = Stage::Off;
                        continue;
                    }
                }
                Stage::Release => {
                    let step = if d.release_frames > 0.0 {
                        v.rel_from / d.release_frames
                    } else {
                        v.rel_from.max(1.0)
                    };
                    v.env -= step;
                    if v.env <= 0.0 {
                        v.env = 0.0;
                        v.stage = Stage::Off;
                        continue;
                    }
                }
                Stage::Off => continue,
            }

            // Out of the region — which a moved crop can do to a voice that
            // was already sounding — or off the end. Checked BEFORE the read,
            // so nothing is ever fetched from past the crop.
            if v.pos >= d.end || v.pos < d.start {
                if d.looping {
                    v.pos = d.start;
                } else {
                    v.stage = Stage::Off;
                    continue;
                }
            }

            let gain = v.env * v.velocity * d.vol;
            let (mut sl, mut sr_) = read(&sample, v.pos, file_frames);
            if d.xfade > 0.0 && v.pos >= d.end - d.xfade {
                // The seam. `t` walks 0..1 across the fade; the head is read
                // at the same distance in from `start`.
                let into = v.pos - (d.end - d.xfade);
                let t = (into / d.xfade).clamp(0.0, 1.0) as f32;
                let (fo, fi) = equal_power(t);
                let (hl, hr) = read(&sample, d.start + into, file_frames);
                sl = sl * fo + hl * fi;
                sr_ = sr_ * fo + hr * fi;
            }
            l += sl * gain;
            r += sr_ * gain;

            // Advance. The glide walks home geometrically; the compare is
            // against 1.0 from whichever side it started.
            if v.glide_mul != 1.0 {
                v.glide *= v.glide_mul;
                if (v.glide_mul > 1.0 && v.glide >= 1.0) || (v.glide_mul < 1.0 && v.glide <= 1.0) {
                    v.glide = 1.0;
                    v.glide_mul = 1.0;
                }
            }
            v.pos += d.global * v.base * v.glide;
            if d.looping && v.pos >= d.end {
                // Land where the head material the crossfade already played
                // has reached, so the loop is continuous rather than repeating
                // the fade's worth of audio.
                v.pos -= d.region - d.xfade;
                if v.pos < d.start {
                    v.pos = d.start;
                }
            }
        }

        let base = f * channels;
        if let Some(dst) = out.get_mut(base) {
            *dst = l;
        }
        if channels > 1 {
            if let Some(dst) = out.get_mut(base + 1) {
                *dst = r;
            }
        }
    }
}

/// The block's dials, worked out once — see [`Sampler::derive`]. Every field
/// is a number the per-frame loop would otherwise have recomputed for each of
/// thirty-two voices.
#[derive(Clone, Copy)]
struct Derived {
    start: f64,
    end: f64,
    region: f64,
    looping: bool,
    xfade: f64,
    /// The pitch every voice shares: transpose, fine, bend and the file's own
    /// rate against the render rate.
    global: f64,
    vol: f32,
    sustain: f32,
    a_step: f32,
    d_step: f32,
    release_frames: f32,
}

/// Put a voice into its release from wherever its envelope actually stands.
#[inline]
fn release(v: &mut Voice) {
    v.rel_from = v.env;
    v.stage = Stage::Release;
}

/// A semitone offset as a playback ratio.
#[inline]
fn semis_ratio(semis: f32) -> f32 {
    (semis / 12.0).exp2()
}

/// The two sides of an equal-power crossfade at `t` in 0..1: `(out, in)`.
/// Their squares sum to one, which is what keeps a seam between two
/// correlated signals from dipping in the middle.
#[inline]
fn equal_power(t: f32) -> (f32, f32) {
    let a = t.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
    (a.cos(), a.sin())
}

/// One stereo pair from the file at a fractional position, by **cubic Hermite
/// (Catmull-Rom) interpolation**.
///
/// **What was chosen and why.** Linear is the floor the brief allows, and it
/// is audibly the floor: interpolating a 48 kHz file linearly is a first-order
/// lowpass whose corner moves with the playback rate, so a sample played an
/// octave down loses its top and one played up gains a hash of images. Hermite
/// is four taps, five multiplies and no table, it is exact on the samples
/// themselves, and it costs about a dozen instructions a frame per voice —
/// which is nothing beside the cache miss that fetching those four taps
/// already is. Sinc is the next step up and is a resampler, not an
/// interpolator; it is not worth a kernel in a bay.
///
/// Taps are CLAMPED into the file rather than wrapped or zeroed, so the first
/// and last frames degrade to a held value instead of to a step — a step at
/// the ends is a click, and the ends are where a crop puts every voice.
#[inline]
fn read(sample: &SampleRef<'_>, pos: f64, frames: usize) -> (f32, f32) {
    if frames == 0 {
        return (0.0, 0.0);
    }
    let i = pos.floor();
    let t = (pos - i) as f32;
    let i = i as isize;
    let ch = sample.channels;
    let tap = |k: isize, c: usize| -> f32 {
        let k = k.clamp(0, frames as isize - 1) as usize;
        sample.data.get(k * ch + c.min(ch - 1)).copied().unwrap_or(0.0)
    };
    let l = hermite(tap(i - 1, 0), tap(i, 0), tap(i + 1, 0), tap(i + 2, 0), t);
    let r = if ch > 1 {
        hermite(tap(i - 1, 1), tap(i, 1), tap(i + 1, 1), tap(i + 2, 1), t)
    } else {
        l
    };
    (l, r)
}

/// Catmull-Rom between `y1` and `y2`, with `y0` and `y3` as the slopes.
#[inline]
fn hermite(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c0 = y1;
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + c0
}

/// Where each dial rests, normalised — the one place the published defaults
/// are spelled as arithmetic on the ranges above rather than as decimals, so
/// moving a range moves its default with it. `builtin_fx`'s table reads this,
/// and a test holds the two equal.
pub fn default_norm(id: u32) -> f32 {
    match id {
        P_VOL => 1.0 / VOL_MAX,
        P_MODE => step_norm(MODE_PITCHED, 3),
        P_TRANSPOSE => bipolar_norm(0.0, TRANSPOSE_SEMIS),
        P_FINE => bipolar_norm(0.0, FINE_CENTS),
        P_ATTACK => secs_norm(0.005, ATTACK_SECS),
        P_DECAY => secs_norm(0.20, DECAY_SECS),
        P_SUSTAIN => 1.0,
        P_RELEASE => secs_norm(0.15, RELEASE_SECS),
        P_START => 0.0,
        P_END => 1.0,
        P_NOTEOFF => 1.0,
        P_LOOP => 0.0,
        P_XFADE => secs_norm(0.010, XFADE_MS / 1000.0),
        P_VOICES => step_norm(15, MAX_VOICES as i32),
        P_PORTA => 0.0,
        P_BEND => step_norm(2, BEND_MAX_SEMIS + 1),
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// A ramp from 0 to 1 across `frames`, in both channels. Chosen because
    /// Catmull-Rom reproduces a straight line EXACTLY, so any reading of a
    /// ramp is a direct measurement of where the read head is — which is what
    /// every pitch, crop and loop test below actually wants to know.
    fn ramp(frames: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; frames * 2];
        for i in 0..frames {
            let x = i as f32 / frames as f32;
            v[i * 2] = x;
            v[i * 2 + 1] = x;
        }
        v
    }

    fn sample(data: &[f32]) -> SampleRef<'_> {
        SampleRef {
            data,
            channels: 2,
            rate: SR,
            root_key: 60,
        }
    }

    /// A kernel with the envelope out of the way — instant on, full sustain,
    /// instant off — so a test measures the thing it names and not the ADSR.
    fn flat() -> Sampler {
        let mut s = Sampler::new(SR);
        s.set_param(P_ATTACK, 0.0);
        s.set_param(P_DECAY, 0.0);
        s.set_param(P_SUSTAIN, 1.0);
        s.set_param(P_RELEASE, 0.0);
        s
    }

    fn on(key: u8) -> Event {
        Event::NoteOn { at: 0, key, vel: 1.0 }
    }

    fn off(key: u8) -> Event {
        Event::NoteOff { at: 0, key }
    }

    fn render(s: &mut Sampler, n: usize, ev: &[Event], data: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0f32; n * 2];
        s.render(&mut out, n, 2, ev, Some(sample(data)));
        out
    }

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// The measured playback ratio: how far the read head travelled per
    /// output frame, read off the ramp the voice is playing.
    fn measured_ratio(out: &[f32], frames: usize, file_frames: usize) -> f32 {
        let first = out[0];
        let last = out[(frames - 1) * 2];
        (last - first) * file_frames as f32 / (frames - 1) as f32
    }

    /// **A note sounds, and letting it go stops it.** The whole feature in one
    /// line: without this the other twelve are testing silence.
    #[test]
    fn a_note_sounds_and_a_note_off_stops_it() {
        let data = ramp(48_000);
        let mut s = flat();
        let out = render(&mut s, 512, &[on(60)], &data);
        assert!(peak(&out) > 0.0, "a note on produced nothing at all");
        assert_eq!(s.active(), 1, "one note on is one voice");
        let out = render(&mut s, 512, &[off(60)], &data);
        assert_eq!(s.active(), 0, "the note off left a voice sounding");
        // The release is zero here, so the tail is one frame at most.
        assert!(
            peak(&out[2..]) == 0.0,
            "a released voice kept making sound: {}",
            peak(&out[2..])
        );
    }

    /// **The release DECAYS — it does not cut, and it does not hold.** A
    /// release that jumps to zero is a click on every note; one that never
    /// reaches zero is a voice the cap can never reclaim. Both were the
    /// shapes to rule out, so this measures monotonic descent AND arrival.
    #[test]
    fn the_release_actually_decays_rather_than_cutting() {
        let data = vec![1.0f32; 48_000 * 2];
        let mut s = flat();
        // A tenth of a second of release, on a DC file so the output IS the
        // envelope with nothing else in it.
        s.set_param(P_RELEASE, secs_norm(0.1, RELEASE_SECS));
        let _ = render(&mut s, 64, &[on(60)], &data);
        let out = render(&mut s, 9_600, &[off(60)], &data);
        let first = out[0];
        assert!(first > 0.5, "the release started from {first}, not from the note");
        let mut worst_up = 0.0f32;
        for f in 1..9_600 {
            worst_up = worst_up.max(out[f * 2] - out[(f - 1) * 2]);
        }
        assert!(
            worst_up <= 1e-6,
            "the release went UP by {worst_up} somewhere — it is not a decay"
        );
        // A tenth of a second is 4,800 frames; half way down it must still be
        // sounding, and by the end it must be gone.
        assert!(out[2_400 * 2] > 0.2, "the release was over far too early");
        assert!(
            out[9_599 * 2] == 0.0 && s.active() == 0,
            "the release never reached zero — the voice is stuck"
        );
    }

    /// **A looped voice crosses the seam without a click.**
    ///
    /// The file is a ramp, so its two ends are as far apart as audio gets: a
    /// bare loop steps 1.0 in one sample at every wrap, which is the loudest
    /// click a sampler can make. With the crossfade dialled in, no two
    /// consecutive output samples may differ by more than 0.02 — about
    /// −34 dBFS, and two orders of magnitude under the bare seam. The bare
    /// case is asserted too, so this cannot pass by looping silently.
    #[test]
    fn a_looped_voice_crosses_the_seam_without_a_click() {
        const STEP_CEILING: f32 = 0.02;
        let file = 4_800usize;
        let data = ramp(file);
        let worst = |xfade_norm: f32| -> f32 {
            let mut s = flat();
            s.set_param(P_LOOP, 1.0);
            s.set_param(P_XFADE, xfade_norm);
            let out = render(&mut s, 20_000, &[on(60)], &data);
            let mut worst = 0.0f32;
            for f in 1..20_000 {
                worst = worst.max((out[f * 2] - out[(f - 1) * 2]).abs());
            }
            worst
        };
        let bare = worst(0.0);
        assert!(
            bare > 0.5,
            "the bare seam only stepped {bare} — the loop is not wrapping, so \
             the crossfade below is being praised for nothing"
        );
        let faded = worst(secs_norm(0.020, XFADE_MS / 1000.0));
        assert!(
            faded <= STEP_CEILING,
            "the crossfaded seam stepped {faded}, past the {STEP_CEILING} ceiling \
             (the bare seam steps {bare})"
        );
    }

    /// **The crop binds at BOTH ends.** Start is where the note begins and
    /// End is where it stops — reading before the one or past the other is
    /// how a trimmed sample plays somebody else's silence, or the drum hit
    /// they cut off.
    #[test]
    fn the_crop_is_respected_at_both_ends() {
        let file = 4_800usize;
        let data = ramp(file);
        let mut s = flat();
        s.set_param(P_START, 0.25);
        s.set_param(P_END, 0.5);
        let out = render(&mut s, 4_000, &[on(60)], &data);
        assert!(
            (out[0] - 0.25).abs() < 0.01,
            "the note began at {} of the file, not at the Start dial's 0.25",
            out[0]
        );
        // 0.25 of 4,800 frames is 1,200 frames of region; past that the voice
        // is over and every sample must be silence, not the rest of the file.
        for f in 1_300..4_000 {
            assert!(
                out[f * 2] == 0.0,
                "frame {f} still played {} — the End dial did not stop it",
                out[f * 2]
            );
        }
        assert_eq!(s.active(), 0, "the cropped voice never ended");
        let top = out[..1_190 * 2].iter().fold(0.0f32, |m, s| m.max(*s));
        assert!(
            top <= 0.51,
            "the voice read {top} of the file, past the End dial's 0.5"
        );
    }

    /// **A crop dragged backwards is still a region.** Pulling End below
    /// Start is one mouse gesture, and a negative length reaches the audio
    /// thread as a subtraction that wraps.
    #[test]
    fn a_crop_dragged_backwards_still_names_a_region() {
        let data = ramp(4_800);
        let mut s = flat();
        s.set_param(P_START, 0.75);
        s.set_param(P_END, 0.25);
        let out = render(&mut s, 3_000, &[on(60)], &data);
        assert!(peak(&out) > 0.2, "a backwards crop played nothing");
        assert!(out.iter().all(|x| x.is_finite()));
    }

    /// **The voice cap STEALS; it never grows.** Growing is a `Vec` push on
    /// the audio callback. The footprint is checked in the same breath,
    /// because "it stole" and "it did not allocate" are the same claim.
    #[test]
    fn the_voice_cap_steals_rather_than_growing() {
        let data = ramp(48_000);
        let mut s = flat();
        s.set_param(P_VOICES, step_norm(3, MAX_VOICES as i32)); // four voices
        let before = s.footprint();
        for key in 60..72u8 {
            let _ = render(&mut s, 64, &[on(key)], &data);
        }
        assert_eq!(s.active(), 4, "the cap of four held {} voices", s.active());
        assert_eq!(s.footprint(), before, "the voice pool grew — that is a malloc");
        // The OLDEST goes: the last four keys played must be the survivors.
        let mut keys: Vec<u8> = s.voices.iter().filter(|v| v.sounding()).map(|v| v.key).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec![68, 69, 70, 71], "the steal took the wrong voices");
    }

    /// **A one-shot ignores note off, whatever the Obey dial says.** The mode
    /// is the whole point of the mode: a hit triggered from a pad plays to its
    /// end even though the key was down for four milliseconds.
    #[test]
    fn a_one_shot_ignores_note_off() {
        let data = ramp(48_000);
        let mut s = flat();
        s.set_param(P_MODE, step_norm(MODE_ONESHOT, 3));
        let _ = render(&mut s, 64, &[on(60)], &data);
        let out = render(&mut s, 4_000, &[off(60)], &data);
        assert_eq!(s.active(), 1, "the one-shot obeyed a note off");
        assert!(peak(&out) > 0.0, "the one-shot went silent on the note off");
        // And it does end, at the end of the file — a one-shot that never
        // finished would spend a voice for ever.
        let mut left = 48_000usize;
        while left > 0 && s.active() > 0 {
            let n = left.min(8_000);
            let _ = render(&mut s, n, &[], &data);
            left -= n;
        }
        assert_eq!(s.active(), 0, "the one-shot never reached the end of the file");
    }

    /// **Obey Note Off, off, holds the note through the key lift.** The dial
    /// the drum lanes want: strike, let go, and hear the whole sample.
    #[test]
    fn obey_note_off_off_holds_the_note_past_the_key_lift() {
        let data = ramp(48_000);
        let mut s = flat();
        s.set_param(P_NOTEOFF, 0.0);
        let _ = render(&mut s, 64, &[on(60)], &data);
        let _ = render(&mut s, 64, &[off(60)], &data);
        assert_eq!(s.active(), 1, "the note stopped even with Obey Note Off off");
        // And back on, the same gesture stops it.
        s.set_param(P_NOTEOFF, 1.0);
        let _ = render(&mut s, 64, &[on(62)], &data);
        let _ = render(&mut s, 64, &[off(62)], &data);
        assert_eq!(s.active(), 1, "obeying the note off did not stop the second note");
    }

    /// **Transpose and bend land on the ratio they name.** An octave up is
    /// exactly twice the speed and a two-semitone bend is exactly
    /// 2^(2/12) — measured off the ramp, not asserted about the dial.
    #[test]
    fn transpose_and_bend_land_on_the_ratio_they_name() {
        let file = 48_000usize;
        let data = ramp(file);
        let at = |setup: &dyn Fn(&mut Sampler), ev: &[Event]| -> f32 {
            let mut s = flat();
            setup(&mut s);
            let mut all = vec![Event::NoteOn { at: 0, key: 60, vel: 1.0 }];
            all.extend_from_slice(ev);
            let out = render(&mut s, 4_000, &all, &data);
            measured_ratio(&out, 4_000, file)
        };
        let unity = at(&|_| {}, &[]);
        assert!((unity - 1.0).abs() < 0.002, "the root key played at {unity}, not unity");
        let octave = at(&|s| s.set_param(P_TRANSPOSE, bipolar_norm(12.0, TRANSPOSE_SEMIS)), &[]);
        assert!((octave - 2.0).abs() < 0.004, "+12 semitones played at {octave}, not 2.0");
        let down = at(&|s| s.set_param(P_TRANSPOSE, bipolar_norm(-12.0, TRANSPOSE_SEMIS)), &[]);
        assert!((down - 0.5).abs() < 0.002, "-12 semitones played at {down}, not 0.5");
        // The shipped bend range is two semitones; the wheel at full travel
        // must be exactly that interval.
        let want = 2.0f32.powf(2.0 / 12.0);
        let bent = at(&|_| {}, &[Event::Bend { at: 0, amount: 1.0 }]);
        assert!((bent - want).abs() < 0.003, "a full bend played at {bent}, not {want}");
        // And a key an octave up in Pitched mode is the same ratio as the
        // dial's octave — the mode's own claim.
        let mut s = flat();
        let out = render(&mut s, 4_000, &[on(72)], &data);
        let keyed = measured_ratio(&out, 4_000, file);
        assert!((keyed - 2.0).abs() < 0.004, "key 72 played at {keyed}, not 2.0");
    }

    /// **Fixed plays every key at the root pitch.** The mode a drum sample
    /// is loaded in: the keyboard chooses WHICH note fires, never how fast.
    #[test]
    fn fixed_mode_plays_every_key_at_the_root_pitch() {
        let file = 48_000usize;
        let data = ramp(file);
        for mode in [MODE_FIXED, MODE_ONESHOT] {
            for key in [36u8, 60, 96] {
                let mut s = flat();
                s.set_param(P_MODE, step_norm(mode, 3));
                let out = render(&mut s, 4_000, &[on(key)], &data);
                let r = measured_ratio(&out, 4_000, file);
                assert!(
                    (r - 1.0).abs() < 0.002,
                    "mode {mode} played key {key} at {r}, not at the root pitch"
                );
            }
        }
    }

    /// **Portamento starts at the last note's pitch and ARRIVES.** A glide
    /// that never lands is a synth permanently out of tune; one that lands
    /// instantly is a dial that does nothing.
    #[test]
    fn portamento_glides_from_the_last_note_and_arrives() {
        let file = 48_000usize;
        let data = ramp(file);
        let mut s = flat();
        s.set_param(P_PORTA, secs_norm(0.1, PORTA_SECS));
        // The first note sets where the glide starts from; it is not itself
        // glided, because there is nowhere to glide from.
        let _ = render(&mut s, 480, &[on(60)], &data);
        let _ = render(&mut s, 64, &[off(60)], &data);
        // An octave up, over a tenth of a second — 4,800 frames.
        let out = render(&mut s, 480, &[on(72)], &data);
        let early = measured_ratio(&out, 480, file);
        assert!(
            early < 1.3,
            "the glide began at {early} — it jumped to the new pitch instead of sliding"
        );
        let _ = render(&mut s, 5_000, &[], &data);
        let out = render(&mut s, 2_000, &[], &data);
        let late = measured_ratio(&out, 2_000, file);
        assert!(
            (late - 2.0).abs() < 0.01,
            "the glide settled at {late}, not at the octave it was going to"
        );
    }

    /// **Nothing in `render` allocates.** Every `Vec` capacity in the kernel
    /// is byte-identical before and after ten seconds of audio with every
    /// dial that can move machinery moving underneath it — the convolver's
    /// and the denoise's own contract test, for the same reason: a `resize`
    /// nobody noticed is a malloc on the audio callback, and a malloc on the
    /// audio callback is a dropout on somebody else's machine.
    #[test]
    fn nothing_on_the_audio_thread_allocates() {
        let data = ramp(24_000);
        let mut s = Sampler::new(SR);
        let before = s.footprint();
        let mut out = vec![0.0f32; 1_024 * 2];
        let mut step = 0usize;
        let mut pos = 0usize;
        let total = (SR * 10.0) as usize;
        while pos < total {
            let n = [480usize, 512, 1, 333, 1_024][step % 5].min(total - pos);
            let ev = [
                Event::NoteOn { at: 0, key: (48 + (step % 30)) as u8, vel: 0.9 },
                Event::Param {
                    at: n / 2,
                    id: [P_LOOP, P_MODE, P_VOICES, P_XFADE, P_PORTA][step % 5],
                    value: (step % 7) as f32 / 6.0,
                },
                Event::NoteOff { at: n.saturating_sub(1), key: (48 + (step % 30)) as u8 },
            ];
            s.render(&mut out, n, 2, &ev, Some(sample(&data)));
            assert!(
                out[..n * 2].iter().all(|x| x.is_finite()),
                "a non-finite sample at step {step}"
            );
            pos += n;
            step += 1;
        }
        assert_eq!(
            s.footprint(),
            before,
            "render grew a buffer somewhere — the allocation contract is broken"
        );
    }

    /// **A bay with no sample loaded is silent, and still remembers.** The
    /// window between loading the unit and the file landing is a real one,
    /// and a kernel that lost the chord held across it would drop the notes
    /// somebody is already playing.
    #[test]
    fn an_empty_bay_is_silent_and_keeps_its_notes() {
        let mut s = flat();
        let mut out = vec![0.0f32; 256 * 2];
        s.render(&mut out, 256, 2, &[on(60)], None);
        assert_eq!(peak(&out), 0.0, "a bay with no sample made a sound");
        let data = ramp(48_000);
        let out = render(&mut s, 256, &[], &data);
        assert!(
            peak(&out) > 0.0,
            "the note held while the file was loading was forgotten when it landed"
        );
    }

    /// **Every dial map inverts.** The face turns a typed number into a knob
    /// position with these; a taper whose inverse disagreed would put the
    /// readout and the sound one gesture apart.
    #[test]
    fn every_dial_map_inverts_itself() {
        for s in [0.0f32, 0.005, 0.05, 0.5, 2.0, 5.0] {
            let back = secs(secs_norm(s, ATTACK_SECS), ATTACK_SECS);
            assert!((back - s).abs() < 1e-4, "{s} s came back as {back}");
        }
        for x in [-24.0f32, -7.0, 0.0, 3.0, 24.0] {
            let back = bipolar(bipolar_norm(x, TRANSPOSE_SEMIS), TRANSPOSE_SEMIS);
            assert!((back - x).abs() < 1e-3, "{x} semitones came back as {back}");
        }
        for i in 0..MAX_VOICES {
            assert_eq!(step_index(step_norm(i, MAX_VOICES as i32), MAX_VOICES as i32), i);
        }
        // And the published defaults are the numbers the brief names.
        assert!((secs(default_norm(P_ATTACK), ATTACK_SECS) - 0.005).abs() < 1e-5);
        assert!((secs(default_norm(P_DECAY), DECAY_SECS) - 0.20).abs() < 1e-4);
        assert!((secs(default_norm(P_RELEASE), RELEASE_SECS) - 0.15).abs() < 1e-4);
        assert_eq!(step_index(default_norm(P_VOICES), MAX_VOICES as i32) + 1, 16);
        assert_eq!(step_index(default_norm(P_BEND), BEND_MAX_SEMIS + 1), 2);
        assert_eq!(step_index(default_norm(P_MODE), 3), MODE_PITCHED);
    }

    /// **The stepped rounding is the analog's rounding.** Two copies of one
    /// rule is how a face comes to name position 3 while the kernel plays
    /// position 2.
    #[test]
    fn the_step_rounding_matches_the_house_one() {
        for steps in [2i32, 3, 13, 16, 25, 32] {
            for k in 0..=100 {
                let v = k as f32 / 100.0;
                assert_eq!(
                    step_index(v, steps),
                    crate::analog::step_index(v, steps),
                    "step {v} of {steps} disagreed with the analog's"
                );
            }
        }
    }

    /// **The sustain pedal holds notes whose keys are up, and lifting it
    /// releases exactly those.** A pedal that released the keys still DOWN
    /// is the fault: it cuts the chord somebody is holding.
    #[test]
    fn the_pedal_holds_the_lifted_keys_and_lets_them_go_together() {
        let data = ramp(48_000);
        let mut s = flat();
        let _ = render(&mut s, 64, &[Event::Sustain { at: 0, down: true }, on(60)], &data);
        let _ = render(&mut s, 64, &[on(64)], &data);
        let _ = render(&mut s, 64, &[off(60)], &data);
        assert_eq!(s.active(), 2, "the pedal did not hold the lifted key");
        let _ = render(&mut s, 64, &[Event::Sustain { at: 0, down: false }], &data);
        assert_eq!(s.active(), 1, "lifting the pedal took the key that is still down");
    }
}
