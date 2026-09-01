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

    /// The knobs this sound answers to, in the order the panel draws them.
    ///
    /// **Per SOUND, not per instrument.** A piano and a tine electric piano
    /// are not the same machine with different numbers in it — one has an
    /// unison to detune and a soundboard to open, the other has a bell to
    /// weigh and a pickup to lean on — so the panel asks the patch what it
    /// has rather than showing a fixed rack with half of it greyed out.
    ///
    /// Indices are a CONTRACT with the settings file, exactly as `ALL` is:
    /// values are stored per strip per patch by POSITION. Append; never
    /// reorder, and never reuse a retired slot for something else.
    pub const fn params(self) -> &'static [ParamSpec] {
        match self {
            Patch::Piano1 => &PIANO_PARAMS,
            Patch::EpMk1 => &EP_PARAMS,
        }
    }

    /// Every knob of this sound at the value the bench signed off — what the
    /// panel's Initialise button restores, and what a strip that has never
    /// been touched holds.
    pub fn defaults(self) -> [f32; MAX_PARAMS] {
        let mut out = [0.0; MAX_PARAMS];
        for (slot, spec) in out.iter_mut().zip(self.params()) {
            *slot = spec.default;
        }
        out
    }
}

/// The most knobs any one sound exposes. The store is a fixed array so that
/// nothing on the audio thread has a length to get wrong.
pub const MAX_PARAMS: usize = 8;

/// One knob, as the panel needs to draw it and the file needs to keep it.
pub struct ParamSpec {
    /// What the panel calls it.
    pub name: &'static str,
    /// The one line under the name. What it does to the INSTRUMENT, not what
    /// it does to the number.
    pub about: &'static str,
    /// Where it sits when the sound is initialised.
    ///
    /// **Everything that travels is 0..=1** — the file, the atomic and the
    /// slider all speak the same range, so a knob has exactly one definition
    /// of "middle" and reset is an assignment rather than a conversion. The
    /// mapping out of that range into seconds, cents and multipliers lives in
    /// `PianoKnobs`/`EpKnobs`, next to the physics it feeds.
    pub default: f32,
}

/// Voicing is knob 0 and Width is knob 1 in EVERY patch, because they are the
/// two that mean the same thing whatever the mechanism is: how hard the
/// contact is, and how far apart the keys sit.
pub const P_VOICING: usize = 0;
pub const P_WIDTH: usize = 1;
/// The body knob — a soundboard on one instrument and a tonebar on the other,
/// but the same slot and the same meaning: how much of the shared resonator
/// reaches the bus. `render` reads it directly, which is why it is named here
/// rather than left to the two knob-mapping structs.
pub const P_BODY: usize = 7;

/// The stereo spread at the top of the WIDTH knob: a little wider than the
/// stock seating, which is what 1.0 in the old two-knob panel meant.
const WIDTH_FULL: f32 = 1.5;
/// Where WIDTH sits for the stock seating — 1.0 of 1.5, and the reason this
/// one default is not 0.5.
const WIDTH_DEFAULT: f32 = 1.0 / WIDTH_FULL;

const PIANO_PARAMS: [ParamSpec; MAX_PARAMS] = [
    ParamSpec {
        name: "Voicing",
        about: "hammer felt: needled soft to lacquered bright",
        default: 0.5,
    },
    ParamSpec {
        name: "Width",
        about: "how far bass and treble sit apart",
        default: WIDTH_DEFAULT,
    },
    ParamSpec {
        name: "Decay",
        about: "how long a struck string rings",
        default: 0.5,
    },
    ParamSpec {
        name: "Brilliance",
        about: "how long the upper partials outlast the fundamental",
        default: 0.5,
    },
    ParamSpec {
        name: "Unison",
        about: "detune between the strings of a course: the beating",
        default: 0.5,
    },
    ParamSpec {
        name: "Stretch",
        about: "string stiffness, which pulls the partials sharp",
        default: 0.5,
    },
    ParamSpec {
        name: "Release",
        about: "how fast the damper stops a string",
        default: 0.5,
    },
    ParamSpec {
        name: "Soundboard",
        about: "the body under everything, and the pedalled wash",
        default: 0.5,
    },
];

const EP_PARAMS: [ParamSpec; MAX_PARAMS] = [
    ParamSpec {
        name: "Voicing",
        about: "hammer tip: soft to hard against the tine",
        default: 0.5,
    },
    ParamSpec {
        name: "Width",
        about: "how far bass and treble sit apart",
        default: WIDTH_DEFAULT,
    },
    ParamSpec {
        name: "Decay",
        about: "how long the tine rings",
        default: 0.5,
    },
    ParamSpec {
        name: "Bell",
        about: "the glassy chime on the attack",
        default: 0.5,
    },
    ParamSpec {
        name: "Bark",
        about: "how hard the tine leans on the pickup",
        default: 0.5,
    },
    ParamSpec {
        name: "Chime",
        about: "where the bell partial sits above the note",
        default: 0.5,
    },
    ParamSpec {
        name: "Release",
        about: "how fast the damper stops the tine",
        default: 0.5,
    },
    ParamSpec {
        name: "Tonebar",
        about: "the metal the tine is bolted to",
        default: 0.5,
    },
];

/// A knob mapped so that **0.5 is exactly the instrument the bench signed
/// off**: every curve below passes through its own tuned constant at the
/// centre, so a strip that has never been touched, a strip reset to defaults
/// and the two-knob build all make the identical sound.
fn bipolar(v: f32, octaves: f32) -> f32 {
    2.0_f32.powf((v.clamp(0.0, 1.0) - 0.5) * 2.0 * octaves)
}

/// The piano's knobs in the units `PianoVoice` actually uses.
#[derive(Clone, Copy)]
pub struct PianoKnobs {
    /// 0 mellow, 1 bright: the hammer's contact width.
    pub voicing: f32,
    /// Multiplier on every partial's t60. 0.38 to 2.6.
    pub decay: f32,
    /// The reference partial's t60 as a fraction of the fundamental's — the
    /// tuned 0.28 at centre, half that when dull, double when singing.
    pub brilliance: f32,
    /// Scale on the unison's cents. 0 collapses the course to one string,
    /// which is a harpsichord's stillness, and 2 is honky-tonk.
    pub unison: f32,
    /// Scale on the inharmonicity coefficient.
    pub stretch: f32,
    /// Multiplier on the damper's time constant: bigger is slower.
    pub release: f32,
    /// How much soundboard. 1.0 is the board as built.
    pub body: f32,
}

impl PianoKnobs {
    pub fn from_params(p: &[f32; MAX_PARAMS]) -> Self {
        PianoKnobs {
            voicing: p[P_VOICING].clamp(0.0, 1.0),
            decay: bipolar(p[2], 1.4),
            brilliance: (0.28 * bipolar(p[3], 1.0)).clamp(0.05, 0.9),
            unison: (p[4] * 2.0).clamp(0.0, 2.0),
            stretch: bipolar(p[5], 1.6),
            release: bipolar(p[6], 1.7),
            body: (p[7] * 2.0).clamp(0.0, 2.0),
        }
    }
}

impl Default for PianoKnobs {
    fn default() -> Self {
        Self::from_params(&Patch::Piano1.defaults())
    }
}

/// The electric piano's knobs, in the units `EpVoice` uses.
#[derive(Clone, Copy)]
pub struct EpKnobs {
    /// 0 soft tip, 1 hard.
    pub voicing: f32,
    /// Multiplier on the tine's t60.
    pub decay: f32,
    /// Scale on the bell mode's weight against the fundamental.
    pub bell: f32,
    /// Scale on how far the tine swings, which is the pickup's nonlinearity
    /// and therefore the bark.
    pub bark: f32,
    /// Shifts the bell partial up or down the series, in partials.
    pub chime: f32,
    /// Multiplier on the damper's time constant.
    pub release: f32,
    /// How much tonebar body.
    pub body: f32,
}

impl EpKnobs {
    pub fn from_params(p: &[f32; MAX_PARAMS]) -> Self {
        EpKnobs {
            voicing: p[P_VOICING].clamp(0.0, 1.0),
            decay: bipolar(p[2], 1.4),
            bell: (p[3] * 2.0).clamp(0.0, 2.0),
            bark: bipolar(p[4], 1.2),
            chime: (p[5] - 0.5) * 3.0,
            release: bipolar(p[6], 1.7),
            body: (p[7] * 2.0).clamp(0.0, 2.0),
        }
    }
}

impl Default for EpKnobs {
    fn default() -> Self {
        Self::from_params(&Patch::EpMk1.defaults())
    }
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

    /// Strike, with **the whole knob store** rather than one number: each
    /// mechanism maps the shared 0..1 values into its own units, because
    /// knob 4 is an unison on one instrument and a pickup on the other.
    fn strike(&mut self, pitch: u8, velocity: f32, params: &[f32; MAX_PARAMS]) {
        if self.is_ep {
            self.ep.strike_knobs(pitch, velocity, &EpKnobs::from_params(params));
        } else {
            self.piano
                .strike_knobs(pitch, velocity, &PianoKnobs::from_params(params));
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
    /// **The panel's knobs, all of them, normalised 0..=1**, indexed by the
    /// current patch's `ParamSpec` list. Every one is read at STRIKE time —
    /// never per sample — so they may be set from the audio thread and a
    /// held note keeps the instrument it was struck on, which is what a real
    /// one does when a technician voices a hammer under your hands.
    ///
    /// One array rather than a field per knob: the host pushes the whole
    /// store every block, and a fixed array has no length for the callback
    /// to get wrong.
    params: [f32; MAX_PARAMS],
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
            params: Patch::default().defaults(),
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
        // **The knobs belong to the SOUND.** A piano's Unison is an electric
        // piano's Bark at the same index, so carrying the numbers across
        // would apply one instrument's settings to another's mechanism. The
        // new patch starts at its own defaults; the host, which remembers a
        // set per strip per patch, pushes the stored ones on the very next
        // block.
        self.params = patch.defaults();
    }

    /// Whether anything is making a sound.
    pub fn active(&self) -> bool {
        self.sounding > 0
    }

    /// Set one knob, by its index in the current patch's `ParamSpec` list.
    ///
    /// A plain store, read at the next strike — real-time safe by having
    /// nothing to be unsafe with, which is why the host may call it from the
    /// callback every block without a queue.
    pub fn set_param(&mut self, index: usize, value: f32) {
        if let Some(slot) = self.params.get_mut(index) {
            *slot = value.clamp(0.0, 1.0);
        }
    }

    /// Every knob at once, in the order the patch lists them. Shorter slices
    /// leave the rest alone, which is what a build reading a file written by
    /// an older one hands over.
    pub fn set_params(&mut self, values: &[f32]) {
        for (i, v) in values.iter().enumerate() {
            self.set_param(i, *v);
        }
    }

    /// The knobs as they stand.
    pub fn params(&self) -> &[f32; MAX_PARAMS] {
        &self.params
    }

    /// **Initialise the sound**: every knob back to the value the bench
    /// signed off for the patch that is loaded. What the panel's button does.
    pub fn reset_params(&mut self) {
        self.params = self.patch.defaults();
    }

    /// The VOICING knob, 0 mellow to 1 bright; 0.5 is the bench tuning.
    /// Knob 0 under an older name, kept because the bench calls it.
    pub fn set_voicing(&mut self, v: f32) {
        self.set_param(P_VOICING, v);
    }

    /// The WIDTH knob in its OLD units — 0 mono, 1.0 stock seating, up to
    /// 1.5 leaning wider — normalised into knob 1. The panel speaks 0..1
    /// like every other knob; this exists for the bench and for the two-knob
    /// call sites that predate the rack.
    pub fn set_width(&mut self, w: f32) {
        self.set_param(P_WIDTH, w.clamp(0.0, WIDTH_FULL) / WIDTH_FULL);
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
        let pan = pan_for(pitch) * self.params[P_WIDTH] * WIDTH_FULL;
        let note = &mut self.notes[slot];
        let was_done = note.pitch.is_none() || note.mech.finished();
        note.pitch = Some(pitch);
        note.held = true;
        note.pan = pan;
        note.age = age;
        note.mech.strike(pitch, velocity, &self.params);
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
        // Once per block, not per sample: the knob cannot move inside a
        // block, and the mapping is a multiply the loop should not repeat.
        let body_amt = (self.params[P_BODY] * 2.0).clamp(0.0, 2.0);
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
            // The soundboard/tonebar knob rides here rather than inside the
            // body, so the board's own decay is untouched and only how much
            // of it reaches the bus moves.
            let (bl, br) = self.body.tick(drive);
            left += bl * body_amt;
            right += br * body_amt;
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
