//! Tangent Keys: a piano and an electric piano, computed rather than recorded.
//!
//! **Why this and not samples, again.** The DX7 module makes the argument for
//! a patch that is 128 bytes; this one makes a stronger version of it. A
//! convincing sampled piano is gigabytes, and it is gigabytes because a
//! recording is a still photograph of one hammer hitting one string at one
//! speed: every velocity needs its own take, every take needs a loop point,
//! and the seams between them are what the ear finds first. A model has no
//! seams. There is no velocity layer to cross because there are no layers —
//! hit the string harder and the felt compresses further, the contact gets
//! shorter, and the tone brightens because that is what the arithmetic does.
//!
//! Two patches ship: [`Patch::Piano1`], a jazz grand voiced for solo playing,
//! and [`Patch::EpMk1`], a tine electric piano taken dry.
//!
//! **The layout, and who owns what.** This file is the FRAME — the public
//! face, the patch list, the voice pool and the block loop. It deliberately
//! contains no physics. The physics live one module down, and the two
//! instruments share nothing but the frame, because they do not share a
//! mechanism: a piano string is flexible and its partials are nearly
//! harmonic, a Rhodes tine is a stiff bar and its partials are not. Modelling
//! them with one algorithm and different numbers is the mistake that makes
//! every electric piano preset sound like a detuned piano.
//!
//! - [`string`] — the piano: a digital waveguide, its hammer, its soundboard.
//! - [`tine`] — the electric piano: a modal bar, its tonebar, its pickup.
//!
//! **Real-time contract.** Everything the audio thread touches is allocated in
//! [`Keys::new`], which runs while the engine is being built and not from the
//! callback. `render` allocates nothing, locks nothing, and calls into no
//! system service. See `crate::instrument` for who calls it and when.

pub mod string;
pub mod tine;

/// How many notes can ring at once.
///
/// **Larger than it looks like it needs to be, on purpose.** A pianist with
/// the sustain pedal down is not playing sixteen notes, they are holding
/// every note they have played since they last lifted their foot, and a
/// jazz pianist pedals through a whole phrase. Sixty-four is the point where
/// a bar of pedalled block chords stops stealing from itself; the cost of the
/// unused ones is zero, because a voice that is not sounding is skipped by
/// the block loop rather than run and multiplied by silence.
pub const VOICES: usize = 64;

/// The instruments this module knows how to be.
///
/// Plain data, `Copy`, and an enum rather than a string: a patch is chosen in
/// the UI, saved into a session and read back on the audio thread, and every
/// one of those layers is happier with something that cannot be misspelt.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Patch {
    /// A jazz grand, voiced for solo playing: mid-forward, woody, a long
    /// singing sustain and a dynamic window that lives at mezzo-piano.
    #[default]
    Piano1,
    /// A tine electric piano, dry — no amplifier, no tremolo, no chorus.
    /// Bell-like when it is played lightly, and it barks when it is not.
    EpMk1,
}

impl Patch {
    /// Every patch, in the order the picker shows them.
    ///
    /// **This order is a contract with the settings file.** The chosen patch
    /// is stored as a NUMBER, because a settings file may be read by a build
    /// older than the one that wrote it and an unknown name would have to fail
    /// somewhere. Reordering this list without reordering
    /// `tangent_ui::dialogs::KEYS_PATCH_NAMES` does not fail to compile and
    /// does not fail to run: it silently loads the piano when the electric
    /// piano was asked for. A test in that module pins both.
    ///
    /// The NAMES live over there rather than here, and deliberately only
    /// there: the picker, the settings clamp and the strip label all need
    /// them, none of those can see this crate, and two copies of a display
    /// string are two things that can disagree.
    pub const ALL: [Patch; 2] = [Patch::Piano1, Patch::EpMk1];
}

/// One key, sounding.
///
/// **Both mechanisms, always, in every voice.** The obvious build has a
/// `Box<dyn Voice>` or an enum holding one or the other, and rebuilds the pool
/// when the patch changes. Both are wrong here for the same reason: the patch
/// is chosen in the UI, but the instrument lives on the audio thread — the
/// `Renderer` is MOVED into the callback, so the host cannot reach in and swap
/// anything. The choice has to travel as an atomic and be applied by the
/// callback itself, and a callback may not allocate. So the allocation happens
/// once, in [`Keys::new`], and changing patch is a flag and a damp.
///
/// It costs about 3 MB for a pool of 64 to carry a piano it is not currently
/// being, against a stem ring the engine already allocates in the tens of
/// megabytes. It buys a patch change that cannot glitch, cannot allocate, and
/// needs no queue, no lock and no `&mut` reaching across a thread boundary.
struct Mechanism {
    piano: string::PianoVoice,
    ep: tine::EpVoice,
    /// Which of the two is speaking. Not `Patch`, because this is asked once
    /// per voice per sample and a bool is the whole question.
    is_ep: bool,
}

impl Mechanism {
    fn new(rate: f32) -> Self {
        Mechanism {
            piano: string::PianoVoice::new(rate),
            ep: tine::EpVoice::new(rate),
            is_ep: false,
        }
    }

    /// Become the other instrument. Silences both, because a tine ringing on
    /// under a piano would be the previous patch leaking into the new one.
    fn set_kind(&mut self, patch: Patch) {
        self.is_ep = matches!(patch, Patch::EpMk1);
        self.piano.damp();
        self.ep.damp();
    }

    fn strike(&mut self, pitch: u8, velocity: f32, voicing: f32) {
        if self.is_ep {
            self.ep.strike_voiced(pitch, velocity, voicing);
        } else {
            self.piano.strike_voiced(pitch, velocity, voicing);
        }
    }

    fn release(&mut self, pedal: bool) {
        if self.is_ep {
            self.ep.release(pedal);
        } else {
            self.piano.release(pedal);
        }
    }

    fn damp(&mut self) {
        if self.is_ep {
            self.ep.damp();
        } else {
            self.piano.damp();
        }
    }

    fn finished(&self) -> bool {
        if self.is_ep {
            self.ep.finished()
        } else {
            self.piano.finished()
        }
    }

    /// This voice's loudness right now, for the stealing policy. Cheap: it is
    /// a running envelope the voice already keeps, not a measurement.
    fn level(&self) -> f32 {
        if self.is_ep {
            self.ep.level()
        } else {
            self.piano.level()
        }
    }

    fn tick(&mut self) -> f32 {
        if self.is_ep {
            self.ep.tick()
        } else {
            self.piano.tick()
        }
    }
}

struct Note {
    mech: Mechanism,
    /// Which key this is. `None` when the voice is free.
    pitch: Option<u8>,
    /// Whether the key is still down. A voice with the key up but the pedal
    /// down is still sounding, and is the FIRST thing stealing takes.
    held: bool,
    /// Where in the stereo field. A piano is not a point source: the bass is
    /// to the left of a player's ears and the treble to the right, and that
    /// spread is most of why a modelled piano stops sounding like it is
    /// inside a box.
    pan: f32,
    /// Bumped on every strike, so the oldest voice is identifiable without a
    /// clock. Wraps harmlessly — only the ORDER matters, and a wrap needs
    /// four billion notes.
    age: u32,
}

/// The instrument.
pub struct Keys {
    notes: Vec<Note>,
    patch: Patch,
    rate: f32,
    /// The panel's VOICING knob: 0 is felt and mellow, 1 is close and bright,
    /// 0.5 is the instrument exactly as it was tuned on the bench. It scales
    /// the hammer's contact width (see `string::PianoVoice::strike_voiced`),
    /// which is the honest way to voice either mechanism — the same thing a
    /// technician does with a needle and felt, not a filter over the output.
    /// Read at strike time, so it may be set from the audio thread.
    voicing: f32,
    /// The panel's WIDTH knob: a scale on the per-key stereo spread. 0 folds
    /// the instrument to a point source, 1.0 is the stock seating, and a
    /// little more leans the bass and treble further apart. Applied where the
    /// pan is chosen, so held notes keep their seat and only new strikes move.
    width: f32,
    /// Sustain pedal, from CC 64.
    pedal: bool,
    /// How many voices are not finished. `render` returns immediately at zero,
    /// which is what makes an unloaded-but-present Keys cost nothing.
    sounding: usize,
    strikes: u32,
    /// The shared body: the soundboard for the piano, the harp of undamped
    /// strings that answers a pedalled note. One per instrument, not one per
    /// voice, because in the real thing there is one soundboard.
    body: string::Body,
}

impl Keys {
    /// Build the instrument. **Allocates**, and is called from the engine's
    /// constructor rather than from the callback.
    pub fn new(sample_rate: f32) -> Self {
        let rate = if sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        let mut keys = Keys {
            notes: Vec::new(),
            patch: Patch::default(),
            rate,
            voicing: 0.5,
            width: 1.0,
            pedal: false,
            sounding: 0,
            strikes: 0,
            body: string::Body::new(rate),
        };
        keys.fill_pool();
        keys
    }

    /// Fill the pool. Called once, from [`Keys::new`], and never again.
    fn fill_pool(&mut self) {
        self.notes.clear();
        self.notes.reserve_exact(VOICES);
        for _ in 0..VOICES {
            let mut mech = Mechanism::new(self.rate);
            mech.set_kind(self.patch);
            self.notes.push(Note {
                mech,
                pitch: None,
                held: false,
                pan: 0.0,
                age: 0,
            });
        }
        self.sounding = 0;
    }

    /// Load a patch.
    ///
    /// **Real-time safe, and it has to be**: the choice arrives as an atomic
    /// and is applied by the audio callback, so this allocates nothing, locks
    /// nothing and takes no unbounded time. Every voice already owns both
    /// instruments; this flips which one answers and silences what was
    /// ringing.
    pub fn set_patch(&mut self, patch: Patch) {
        if patch == self.patch {
            return;
        }
        self.patch = patch;
        for n in &mut self.notes {
            n.pitch = None;
            n.held = false;
            n.mech.set_kind(patch);
        }
        self.sounding = 0;
        self.body.set_patch(patch);
    }

    /// Whether anything is making a sound.
    pub fn active(&self) -> bool {
        self.sounding > 0
    }

    /// The VOICING knob, 0 mellow to 1 bright; 0.5 is the bench tuning.
    /// A plain store read at the next strike — real-time safe by having
    /// nothing to be unsafe with.
    pub fn set_voicing(&mut self, v: f32) {
        self.voicing = v.clamp(0.0, 1.0);
    }

    /// The WIDTH knob: 0 mono, 1 stock seating, up to 1.5 leaning wider.
    pub fn set_width(&mut self, w: f32) {
        self.width = w.clamp(0.0, 1.5);
    }

    /// Strike a key.
    ///
    /// `velocity` is MIDI velocity over 127, so 0.0 to 1.0 — the same units
    /// the DX7 takes and the same units `instrument.rs` hands out.
    pub fn note_on(&mut self, pitch: u8, velocity: f32) {
        if pitch > 127 {
            return;
        }
        let velocity = velocity.clamp(0.0, 1.0);
        // A re-struck key takes its own voice back, sounding or merely held
        // by the pedal. The DX7 learned this the hard way: matching only the
        // held flag let a repeated note under the pedal allocate a fresh
        // voice every time until the pool ran dry. A real damper cannot do
        // that, because there is only one string.
        let slot = self
            .notes
            .iter()
            .position(|n| n.pitch == Some(pitch))
            .or_else(|| self.free_slot())
            .or_else(|| self.steal());
        let Some(slot) = slot else {
            return;
        };
        self.strikes = self.strikes.wrapping_add(1);
        let age = self.strikes;
        let pan = pan_for(pitch) * self.width;
        let note = &mut self.notes[slot];
        let was_done = note.pitch.is_none() || note.mech.finished();
        note.pitch = Some(pitch);
        note.held = true;
        note.pan = pan;
        note.age = age;
        note.mech.strike(pitch, velocity, self.voicing);
        if was_done {
            self.sounding += 1;
        }
    }

    fn free_slot(&self) -> Option<usize> {
        self.notes
            .iter()
            .position(|n| n.pitch.is_none() || n.mech.finished())
    }

    /// **Take the quietest thing that is already dying, never the loudest
    /// thing being held.** A synth that steals by age is fine for a synth; on
    /// a piano it chops the bass note out from under a chord, because the bass
    /// note is the oldest and it is also the one holding the harmony up. So
    /// released voices are candidates before held ones, and within each group
    /// the quietest goes first — a voice nobody can hear any more is free.
    fn steal(&self) -> Option<usize> {
        let mut best: Option<(usize, bool, f32)> = None;
        for (i, n) in self.notes.iter().enumerate() {
            let cand = (i, n.held, n.mech.level());
            best = Some(match best {
                None => cand,
                Some(b) => {
                    // held=false sorts before held=true; then quieter first.
                    let better = match (cand.1, b.1) {
                        (false, true) => true,
                        (true, false) => false,
                        _ => cand.2 < b.2,
                    };
                    if better {
                        cand
                    } else {
                        b
                    }
                }
            });
        }
        best.map(|b| b.0)
    }

    /// Let a key up. Under the pedal the string keeps ringing, which is the
    /// whole point of the pedal.
    pub fn note_off(&mut self, pitch: u8) {
        let pedal = self.pedal;
        for n in &mut self.notes {
            if n.pitch == Some(pitch) && n.held {
                n.held = false;
                n.mech.release(pedal);
            }
        }
    }

    /// The sustain pedal, CC 64.
    ///
    /// Lifting it damps everything whose key is already up — and only that.
    /// Notes still held by a finger keep ringing, which is the difference
    /// between a pedal and a panic button and the thing that is missing from
    /// every model where the pedal "does not seem to do anything".
    pub fn set_pedal(&mut self, down: bool) {
        if down == self.pedal {
            return;
        }
        self.pedal = down;
        if !down {
            for n in &mut self.notes {
                if !n.held {
                    n.mech.damp();
                }
            }
        }
        self.body.set_pedal(down);
    }

    /// Everything off, immediately. Not a pedal lift: this damps held keys too.
    pub fn all_notes_off(&mut self) {
        for n in &mut self.notes {
            n.held = false;
            n.mech.damp();
        }
        self.body.reset();
    }

    /// Add `frames` of interleaved audio into `out`.
    ///
    /// Adds rather than overwrites, exactly as `Dx7::render` does, because
    /// that is the contract `instrument.rs` renders against — it clears the
    /// scratch buffer itself and then lets the instrument sum into it.
    pub fn render(&mut self, out: &mut [f32], frames: usize, channels: usize) {
        if self.sounding == 0 || channels == 0 {
            return;
        }
        let n = frames.min(out.len() / channels);
        for f in 0..n {
            let mut left = 0.0_f32;
            let mut right = 0.0_f32;
            // What the ringing strings put INTO the body this sample, and what
            // the body gives back. One shared path, so a pedalled chord lights
            // up the strings nobody struck.
            let mut drive = 0.0_f32;
            for note in &mut self.notes {
                if note.pitch.is_none() || note.mech.finished() {
                    continue;
                }
                let s = note.mech.tick();
                drive += s;
                // Equal power, so a note does not get quieter for being
                // panned. Centre is unity.
                let (gl, gr) = pan_gains(note.pan);
                left += s * gl;
                right += s * gr;
            }
            let (bl, br) = self.body.tick(drive);
            left += bl;
            right += br;
            let base = f * channels;
            if channels == 1 {
                out[base] += (left + right) * 0.5;
            } else {
                out[base] += left;
                out[base + 1] += right;
                for c in 2..channels {
                    out[base + c] += (left + right) * 0.5;
                }
            }
        }
        // Recount once per block rather than per sample: `finished` is cheap
        // but sixty-four of them per sample is not, and a voice that ends
        // mid-block costs at most one block of being counted.
        let mut live = 0usize;
        for n in &mut self.notes {
            if n.pitch.is_some() && n.mech.finished() {
                n.pitch = None;
                n.held = false;
            } else if n.pitch.is_some() {
                live += 1;
            }
        }
        self.sounding = live;
        if self.body.ringing() {
            self.sounding += 1;
        }
    }
}

/// Where a key sits across the stereo field, as the player hears it: bass to
/// the left, treble to the right, and not all the way to either. A0 is -0.55,
/// C8 is +0.55, middle C is very slightly left of centre, which is where it
/// actually is on a keyboard.
fn pan_for(pitch: u8) -> f32 {
    let t = (f32::from(pitch) - 21.0) / 66.0;
    (t.clamp(0.0, 1.0) - 0.5) * 1.1
}

/// Equal-power pan, normalised so centre is 0 dB rather than -3.
fn pan_gains(pan: f32) -> (f32, f32) {
    let p = (pan.clamp(-1.0, 1.0) + 1.0) * 0.5;
    let a = std::f32::consts::FRAC_PI_2 * p;
    (a.cos() * std::f32::consts::SQRT_2, a.sin() * std::f32::consts::SQRT_2)
}
