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

pub mod clav;
pub mod reed;
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
    /// The concert instrument: longer wire, longer ring, sweeter treble,
    /// wider dynamics, quieter action. The hall grand to Piano 1's club one.
    Piano2,
    /// The same tine piano through its own amplifier: the stereo throb of
    /// the twin speakers, and a preamp that rounds the top a little.
    EpSuitcase,
    /// A struck-and-held string with a sliding pickup: the funk machine,
    /// taken dry. See the `clav` module for what a tangent is.
    Clav,
    /// The same instrument through an envelope filter that chases your
    /// touch — the pedal it spent the seventies plugged into.
    ClavWah,
    /// A reed piano, transistor era: hollow when feathered, growling when
    /// leaned on, with the amp's own tremolo.
    Wurli200,
    /// The earlier tube reed piano: darker, never entirely clean, and the
    /// growl comes up like a dimmer rather than a switch.
    Wurli140,
}

/// Which physics answers a patch — the four mechanisms behind eight sounds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Piano,
    Ep,
    Clav,
    Wurli,
}

impl Patch {
    fn family(self) -> Family {
        match self {
            Patch::Piano1 | Patch::Piano2 => Family::Piano,
            Patch::EpMk1 | Patch::EpSuitcase => Family::Ep,
            Patch::Clav | Patch::ClavWah => Family::Clav,
            Patch::Wurli200 | Patch::Wurli140 => Family::Wurli,
        }
    }
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
    /// APPENDED, never reordered: the first two indices were written into
    /// settings files for months before the other six existed.
    pub const ALL: [Patch; 8] = [
        Patch::Piano1,
        Patch::EpMk1,
        Patch::Piano2,
        Patch::EpSuitcase,
        Patch::Clav,
        Patch::ClavWah,
        Patch::Wurli200,
        Patch::Wurli140,
    ];

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
            Patch::Piano1 | Patch::Piano2 => &PIANO_PARAMS,
            Patch::EpMk1 => &EP_PARAMS,
            Patch::EpSuitcase => &EP_SUITCASE_PARAMS,
            Patch::Clav => &CLAV_PARAMS,
            Patch::ClavWah => &CLAV_WAH_PARAMS,
            Patch::Wurli200 => &WURLI_PARAMS,
            Patch::Wurli140 => &WURLI140_PARAMS,
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
/// speaks under the note. `render` reads it directly for the pianos' board
/// send, which is why it is named here rather than left to the two
/// knob-mapping structs; the electric patches' tonebar takes it at the
/// strike through `EpKnobs::body` instead, because their `Body` is a hard
/// zero and the bar is per voice.
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
        about: "the bar the tine is bolted to: its ring and the slow beat",
        default: 0.5,
    },
];

/// The suitcase wears the dry instrument's rack with one seat changed: the
/// CHIME slot becomes the TREMOLO, because the amp's throb is the reason to
/// choose this patch and a knob you cannot reach is not a knob.
const EP_SUITCASE_PARAMS: [ParamSpec; MAX_PARAMS] = [
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
        name: "Tremolo",
        about: "the stereo throb of the twin speakers",
        default: 0.45,
    },
    ParamSpec {
        name: "Release",
        about: "how fast the damper stops the tine",
        default: 0.5,
    },
    ParamSpec {
        name: "Tonebar",
        about: "the bar the tine is bolted to: its ring and the slow beat",
        default: 0.5,
    },
];

const CLAV_PARAMS: [ParamSpec; MAX_PARAMS] = [
    ParamSpec {
        name: "Voicing",
        about: "tangent tip: worn soft to fresh hard",
        default: 0.5,
    },
    ParamSpec {
        name: "Width",
        about: "how far bass and treble sit apart",
        default: WIDTH_DEFAULT,
    },
    ParamSpec {
        name: "Decay",
        about: "how long a held string rings",
        default: 0.5,
    },
    ParamSpec {
        name: "Pickup",
        about: "where the coil sits under the string",
        default: 0.35,
    },
    ParamSpec {
        name: "Tone",
        about: "the filter rockers, folded to one sweep",
        default: 0.75,
    },
    ParamSpec {
        name: "Snap",
        about: "the spit of the string when the key lifts",
        default: 0.5,
    },
    ParamSpec {
        name: "Release",
        about: "how fast the yarn chokes the string",
        default: 0.5,
    },
    ParamSpec {
        name: "Mute",
        about: "the sliding damper pad: funk at the top",
        default: 0.0,
    },
];

/// The wah patch trades the TONE rocker for the pedal's one knob — a static
/// filter under a moving one is a fight the moving one always wins.
const CLAV_WAH_PARAMS: [ParamSpec; MAX_PARAMS] = [
    ParamSpec {
        name: "Voicing",
        about: "tangent tip: worn soft to fresh hard",
        default: 0.5,
    },
    ParamSpec {
        name: "Width",
        about: "how far bass and treble sit apart",
        default: WIDTH_DEFAULT,
    },
    ParamSpec {
        name: "Decay",
        about: "how long a held string rings",
        default: 0.5,
    },
    ParamSpec {
        name: "Pickup",
        about: "where the coil sits under the string",
        default: 0.35,
    },
    ParamSpec {
        name: "Wah",
        about: "how hard the filter chases your touch",
        default: 0.55,
    },
    ParamSpec {
        name: "Snap",
        about: "the spit of the string when the key lifts",
        default: 0.5,
    },
    ParamSpec {
        name: "Release",
        about: "how fast the yarn chokes the string",
        default: 0.5,
    },
    ParamSpec {
        name: "Mute",
        about: "the sliding damper pad: funk at the top",
        default: 0.0,
    },
];

const WURLI_PARAMS: [ParamSpec; MAX_PARAMS] = [
    ParamSpec {
        name: "Voicing",
        about: "hammer felt: worn soft to packed hard",
        default: 0.5,
    },
    ParamSpec {
        name: "Width",
        about: "how far bass and treble sit apart",
        default: WIDTH_DEFAULT,
    },
    ParamSpec {
        name: "Decay",
        about: "how long the reed rings",
        default: 0.5,
    },
    ParamSpec {
        name: "Reed",
        about: "how much of the bar's own metal speaks",
        default: 0.5,
    },
    ParamSpec {
        name: "Growl",
        about: "how hard the reed leans into the plate",
        default: 0.5,
    },
    ParamSpec {
        name: "Tremolo",
        about: "the amp's throb",
        default: 0.4,
    },
    ParamSpec {
        name: "Release",
        about: "how fast the damper stops the reed",
        default: 0.5,
    },
    ParamSpec {
        name: "Thump",
        about: "the hammer's knock against the reed",
        default: 0.5,
    },
];

/// The tube instrument ships dirtier and stiller: growl up, tremolo off —
/// the 140's trem was a service-menu afterthought and nobody used it.
const WURLI140_PARAMS: [ParamSpec; MAX_PARAMS] = [
    ParamSpec {
        name: "Voicing",
        about: "hammer felt: worn soft to packed hard",
        default: 0.5,
    },
    ParamSpec {
        name: "Width",
        about: "how far bass and treble sit apart",
        default: WIDTH_DEFAULT,
    },
    ParamSpec {
        name: "Decay",
        about: "how long the reed rings",
        default: 0.5,
    },
    ParamSpec {
        name: "Reed",
        about: "how much of the bar's own metal speaks",
        default: 0.5,
    },
    ParamSpec {
        name: "Growl",
        about: "how hard the reed leans into the plate",
        default: 0.62,
    },
    ParamSpec {
        name: "Tremolo",
        about: "the amp's throb",
        default: 0.0,
    },
    ParamSpec {
        name: "Release",
        about: "how fast the damper stops the reed",
        default: 0.5,
    },
    ParamSpec {
        name: "Thump",
        about: "the hammer's knock against the reed",
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

    /// The suitcase patch: index 5 belongs to the amp's TREMOLO, so the
    /// chime holds at the tine as built — the same seat-swap discipline as
    /// `ClavKnobs::from_params_wah`. Before this existed the suitcase went
    /// through `from_params` and the tremolo depth leaked into the chime
    /// mapping: turning the throb up retuned the bell partial nine
    /// semitones at the strike, and the shipped 0.45 default meant the
    /// suitcase's bell never even matched the Mk.1's.
    pub fn from_params_suitcase(p: &[f32; MAX_PARAMS]) -> Self {
        let mut k = Self::from_params(p);
        k.chime = 0.0;
        k
    }
}

impl Default for EpKnobs {
    fn default() -> Self {
        Self::from_params(&Patch::EpMk1.defaults())
    }
}

/// The clav's knobs in the units `ClavVoice` uses.
#[derive(Clone, Copy)]
pub struct ClavKnobs {
    /// 0 worn soft, 1 fresh hard.
    pub voicing: f32,
    /// Multiplier on the string's t60.
    pub decay: f32,
    /// The coil's position, 0 tight to the bridge, 1 out under the middle.
    pub pickup: f32,
    /// The rockers, 0 soft to 1 brilliant.
    pub tone: f32,
    /// Scale on the release spit.
    pub snap: f32,
    /// Multiplier on the yarn's choke time.
    pub release: f32,
    /// The damper slider, 0 free to 1 pressed.
    pub mute: f32,
}

impl ClavKnobs {
    /// The dry patch: TONE lives at index 4.
    pub fn from_params(p: &[f32; MAX_PARAMS]) -> Self {
        ClavKnobs {
            voicing: p[P_VOICING].clamp(0.0, 1.0),
            decay: bipolar(p[2], 1.4),
            pickup: p[3].clamp(0.0, 1.0),
            tone: p[4].clamp(0.0, 1.0),
            snap: (p[5] * 2.0).clamp(0.0, 2.0),
            release: bipolar(p[6], 1.7),
            mute: p[7].clamp(0.0, 1.0),
        }
    }

    /// The wah patch: index 4 belongs to the pedal, so the rockers hold at
    /// their shipped setting — the pedal is doing the filtering.
    pub fn from_params_wah(p: &[f32; MAX_PARAMS]) -> Self {
        let mut k = Self::from_params(p);
        k.tone = 0.75;
        k
    }
}

impl Default for ClavKnobs {
    fn default() -> Self {
        Self::from_params(&Patch::Clav.defaults())
    }
}

/// The reed piano's knobs in the units `WurliVoice` uses. TREMOLO is absent
/// on purpose: it belongs to the AMP, one per instrument, and the frame
/// reads it straight off the store each block.
#[derive(Clone, Copy)]
pub struct WurliKnobs {
    /// 0 worn soft, 1 packed hard.
    pub voicing: f32,
    /// Multiplier on the reed's t60.
    pub decay: f32,
    /// Scale on the bar overtones' weight.
    pub reed: f32,
    /// Scale on the plate's asymmetry.
    pub growl: f32,
    /// Multiplier on the damper's time constant.
    pub release: f32,
    /// Scale on the hammer knock.
    pub thump: f32,
}

impl WurliKnobs {
    pub fn from_params(p: &[f32; MAX_PARAMS]) -> Self {
        WurliKnobs {
            voicing: p[P_VOICING].clamp(0.0, 1.0),
            decay: bipolar(p[2], 1.4),
            reed: (p[3] * 2.0).clamp(0.0, 2.0),
            growl: (p[4] * 2.0).clamp(0.0, 2.0),
            release: bipolar(p[6], 1.7),
            thump: (p[7] * 2.0).clamp(0.0, 2.0),
        }
    }
}

impl Default for WurliKnobs {
    fn default() -> Self {
        Self::from_params(&Patch::Wurli200.defaults())
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
    clav: clav::ClavVoice,
    wurli: reed::WurliVoice,
    /// Which mechanism is speaking, and which base it wears. The FAMILY is
    /// asked once per voice per sample; the patch only at the strike.
    family: Family,
    patch: Patch,
}

impl Mechanism {
    fn new(rate: f32) -> Self {
        Mechanism {
            piano: string::PianoVoice::new(rate),
            ep: tine::EpVoice::new(rate),
            clav: clav::ClavVoice::new(rate),
            wurli: reed::WurliVoice::new(rate),
            family: Family::Piano,
            patch: Patch::Piano1,
        }
    }

    /// Become another instrument. Silences all four, because a tine ringing
    /// on under a piano would be the previous patch leaking into the new one.
    ///
    /// Silenced outright, not damped: a mechanism that is not the family
    /// being ticked never runs its fade, so `damp` on it was a promise
    /// nobody kept — the wave sat in the buffer until the next strike
    /// cleared it. Now that a strike LANDS on a voice that is not finished
    /// (see `strike`), a stale wave would be landed on, and the patch it
    /// came from would speak through the new one.
    fn set_kind(&mut self, patch: Patch) {
        self.family = patch.family();
        self.patch = patch;
        self.silence();
    }

    /// Every mechanism to silence, now: what a voice must be before a first
    /// strike, and what the frame makes of one it has faded out.
    fn silence(&mut self) {
        self.piano.silence();
        self.ep.silence();
        self.clav.silence();
        self.wurli.silence();
    }

    /// Whether a strike of `pitch` with these knobs lands on what this
    /// voice is ringing, or would have to replace it. The modal instruments
    /// always land — a resonator takes new coefficients under its motion
    /// without a step — and the waveguides land when the loop's design is
    /// the one ringing; see `string::PianoVoice::lands`.
    fn lands(&self, pitch: u8, params: &[f32; MAX_PARAMS]) -> bool {
        match self.patch {
            Patch::Piano1 => self.piano.lands(
                pitch,
                &PianoKnobs::from_params(params),
                &string::PianoBase::JAZZ,
            ),
            Patch::Piano2 => self.piano.lands(
                pitch,
                &PianoKnobs::from_params(params),
                &string::PianoBase::CONCERT,
            ),
            Patch::EpMk1 | Patch::EpSuitcase => !self.ep.finished(),
            Patch::Clav => self.clav.lands(pitch, &ClavKnobs::from_params(params)),
            Patch::ClavWah => self.clav.lands(pitch, &ClavKnobs::from_params_wah(params)),
            Patch::Wurli200 | Patch::Wurli140 => !self.wurli.finished(),
        }
    }

    /// Strike, with **the whole knob store** rather than one number: each
    /// mechanism maps the shared 0..1 values into its own units, because
    /// knob 4 is an unison on one instrument and a pickup on the other.
    ///
    /// On a voice that is still audible this LANDS — the hammer's energy
    /// is added to what is ringing — when `lands` says it can, and starts
    /// over from a cleared state when it cannot. The frame never lets the
    /// second case happen to an audible voice; see `Keys::note_on`.
    fn strike(&mut self, pitch: u8, velocity: f32, params: &[f32; MAX_PARAMS]) {
        match self.patch {
            Patch::Piano1 => self.piano.strike_based(
                pitch,
                velocity,
                &PianoKnobs::from_params(params),
                &string::PianoBase::JAZZ,
            ),
            Patch::Piano2 => self.piano.strike_based(
                pitch,
                velocity,
                &PianoKnobs::from_params(params),
                &string::PianoBase::CONCERT,
            ),
            Patch::EpMk1 => {
                self.ep
                    .strike_knobs(pitch, velocity, &EpKnobs::from_params(params));
            }
            Patch::EpSuitcase => self.ep.strike_knobs(
                pitch,
                velocity,
                &EpKnobs::from_params_suitcase(params),
            ),
            Patch::Clav => {
                self.clav
                    .strike_knobs(pitch, velocity, &ClavKnobs::from_params(params));
            }
            Patch::ClavWah => self.clav.strike_knobs(
                pitch,
                velocity,
                &ClavKnobs::from_params_wah(params),
            ),
            Patch::Wurli200 => self.wurli.strike_knobs(
                pitch,
                velocity,
                &WurliKnobs::from_params(params),
                &reed::WurliBase::TWO_HUNDRED,
            ),
            Patch::Wurli140 => self.wurli.strike_knobs(
                pitch,
                velocity,
                &WurliKnobs::from_params(params),
                &reed::WurliBase::ONE_FORTY,
            ),
        }
    }

    fn release(&mut self, pedal: bool) {
        match self.family {
            Family::Piano => self.piano.release(pedal),
            Family::Ep => self.ep.release(pedal),
            Family::Clav => self.clav.release(pedal),
            Family::Wurli => self.wurli.release(pedal),
        }
    }

    fn damp(&mut self) {
        match self.family {
            Family::Piano => self.piano.damp(),
            Family::Ep => self.ep.damp(),
            Family::Clav => self.clav.damp(),
            Family::Wurli => self.wurli.damp(),
        }
    }

    fn finished(&self) -> bool {
        match self.family {
            Family::Piano => self.piano.finished(),
            Family::Ep => self.ep.finished(),
            Family::Clav => self.clav.finished(),
            Family::Wurli => self.wurli.finished(),
        }
    }

    /// This voice's loudness right now, for the stealing policy. Cheap: it is
    /// a running envelope the voice already keeps, not a measurement.
    fn level(&self) -> f32 {
        match self.family {
            Family::Piano => self.piano.level(),
            Family::Ep => self.ep.level(),
            Family::Clav => self.clav.level(),
            Family::Wurli => self.wurli.level(),
        }
    }

    fn tick(&mut self) -> f32 {
        match self.family {
            Family::Piano => self.piano.tick(),
            Family::Ep => self.ep.tick(),
            Family::Clav => self.clav.tick(),
            Family::Wurli => self.wurli.tick(),
        }
    }
}

struct Note {
    /// Boxed, so that moving a voice out of the pool — into a corpse slot,
    /// see `Keys::bury` — is a pointer swap and not a copy of the four
    /// mechanisms' hundred-odd kilobytes on the audio thread. Allocated in
    /// `fill_pool`, once.
    mech: Box<Mechanism>,
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

/// A voice that had to leave the pool while it was still audible, finishing
/// on its own: the mechanism, where it sat in the field, and how much of
/// its fade is left.
///
/// **Why a voice cannot simply be restruck.** A strike LANDS on a voice of
/// the same design — see `Mechanism::strike` — but a stolen voice is a
/// different pitch, and nothing superposes across a pitch change; the loop
/// has to start from silence, and clearing a loop that is still audible is
/// a step, which is a pop. So the audible mechanism is swapped out here,
/// faded to nothing over a few milliseconds, and the slot starts fresh on a
/// mechanism that is silent — clearing silence pops nothing.
struct Corpse {
    mech: Box<Mechanism>,
    pan: f32,
    /// Samples of fade left. Zero is a free slot.
    left: u32,
    total: u32,
}

/// How many voices may be fading out at once. Stealing is rare on a pool
/// of sixty-four, and a fade is four milliseconds; three is more than a
/// hand can steal in that time. When it is not, the corpse nearest the end
/// of its fade is cut — at a gain already close to zero.
const CORPSES: usize = 3;

/// How long a stolen voice takes to fade, in seconds. Long enough that the
/// slope is not an edge at any pitch the instrument plays, short enough
/// that the new note is not heard arriving under an old one.
const CORPSE_FADE: f32 = 0.004;

/// The instrument.
pub struct Keys {
    notes: Vec<Note>,
    /// Voices on their way out; see `Corpse`.
    corpses: Vec<Corpse>,
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
    /// The AMP side of the patches that have one — per instrument, not per
    /// voice, because the suitcase has one pair of speakers and the reed
    /// piano one output stage however many keys are down. A shared LFO
    /// phase for the tremolos; a follower and a state-variable filter, one
    /// per channel, for the wah.
    trem_phase: f32,
    wah_env: f32,
    wah_low: [f32; 2],
    wah_band: [f32; 2],
}

impl Keys {
    /// Build the instrument. **Allocates**, and is called from the engine's
    /// constructor rather than from the callback.
    pub fn new(sample_rate: f32) -> Self {
        let rate = if sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        let mut keys = Keys {
            notes: Vec::new(),
            corpses: Vec::new(),
            patch: Patch::default(),
            rate,
            params: Patch::default().defaults(),
            pedal: false,
            sounding: 0,
            strikes: 0,
            body: string::Body::new(rate),
            trem_phase: 0.0,
            wah_env: 0.0,
            wah_low: [0.0; 2],
            wah_band: [0.0; 2],
        };
        keys.fill_pool();
        keys
    }

    /// Fill the pool. Called once, from [`Keys::new`], and never again.
    fn fill_pool(&mut self) {
        self.notes.clear();
        self.notes.reserve_exact(VOICES);
        for _ in 0..VOICES {
            let mut mech = Box::new(Mechanism::new(self.rate));
            mech.set_kind(self.patch);
            self.notes.push(Note {
                mech,
                pitch: None,
                held: false,
                pan: 0.0,
                age: 0,
            });
        }
        self.corpses.clear();
        self.corpses.reserve_exact(CORPSES);
        for _ in 0..CORPSES {
            let mut mech = Box::new(Mechanism::new(self.rate));
            mech.set_kind(self.patch);
            self.corpses.push(Corpse {
                mech,
                pan: 0.0,
                left: 0,
                total: 1,
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
        for c in &mut self.corpses {
            c.left = 0;
            c.mech.set_kind(patch);
        }
        self.sounding = 0;
        self.body.set_patch(patch);
        self.trem_phase = 0.0;
        self.wah_env = 0.0;
        self.wah_low = [0.0; 2];
        self.wah_band = [0.0; 2];
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
        //
        // **What the strike then does to that voice is the mechanism's
        // business — a hammer LANDS on a string that is still moving — with
        // one exception the frame has to handle: a voice that is audible
        // and cannot be landed on.** Two cases. A stolen voice is another
        // pitch, and nothing superposes across a pitch change. A re-struck
        // key whose loop was re-designed under it — a knob moved the
        // string's length between two strikes of one held note — is the
        // rare case of the same thing. Both would need the loop cleared
        // while it is audible, which is a pop, so both go to `bury`: the
        // audible mechanism fades out on its own and the slot starts from
        // silence.
        let slot = match self.notes.iter().position(|n| n.pitch == Some(pitch)) {
            Some(s) => {
                if !self.notes[s].mech.finished() && !self.notes[s].mech.lands(pitch, &self.params) {
                    self.bury(s);
                }
                s
            }
            None => match self.free_slot() {
                Some(s) => s,
                None => {
                    let Some(s) = self.steal() else {
                        return;
                    };
                    self.bury(s);
                    s
                }
            },
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

    /// Move the voice in `slot` out to a corpse, fading, and leave the slot
    /// holding a silent mechanism. See `Corpse` for why. Zero allocation:
    /// the corpse pool was filled in `fill_pool`, and the mechanisms change
    /// places by pointer.
    fn bury(&mut self, slot: usize) {
        // A free corpse, or failing that the one nearest the end of its
        // fade: its gain is already closest to zero, so cutting it is the
        // smallest cut on offer.
        let Some(c) = (0..self.corpses.len()).min_by_key(|&i| self.corpses[i].left) else {
            return;
        };
        let corpse = &mut self.corpses[c];
        if corpse.left > 0 {
            corpse.mech.silence();
        }
        std::mem::swap(&mut self.notes[slot].mech, &mut corpse.mech);
        corpse.pan = self.notes[slot].pan;
        corpse.total = ((CORPSE_FADE * self.rate) as u32).max(1);
        corpse.left = corpse.total;
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
        // CUBED, with the centre still exactly 1.0: the board is voiced to
        // sit well under the strings, so a linear 0..2 could only whisper —
        // the bench measured a tenth of a decibel of total RMS across the
        // knob's whole travel. The cube leaves the shipped default
        // bit-identical and gives the top of the knob a board you can hear
        // open, 18 dB above the old ceiling, with a true zero at the bottom.
        // This feeds `Body`, which the electric patches hard-zero — their
        // knob 7 is the tonebar, applied at the strike in `EpKnobs`.
        let body_amt = {
            let b = (self.params[P_BODY] * 2.0).clamp(0.0, 2.0);
            b * b * b
        };
        // The amp side, block-constant. The tremolo and wah knobs are read
        // HERE rather than at the strike, because an amp's knobs act on
        // whatever is already ringing — turn the throb up under a held
        // chord and the chord throbs, which is what the real switch does.
        let patch = self.patch;
        let (trem_depth, trem_hz) = match patch {
            Patch::EpSuitcase => ((self.params[5] * 0.85).clamp(0.0, 0.85), 5.0),
            Patch::Wurli200 | Patch::Wurli140 => {
                (self.params[5].clamp(0.0, 1.0), 5.5)
            }
            _ => (0.0, 0.0),
        };
        let trem_inc = std::f32::consts::TAU * trem_hz / self.rate;
        let wah_sens = self.params[4].clamp(0.0, 1.0);
        let wah_atk = 1.0 - (-1.0 / (0.003 * self.rate)).exp();
        let wah_rel = 1.0 - (-1.0 / (0.140 * self.rate)).exp();
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
            // The voices on their way out: a straight line to zero, which
            // at four milliseconds is a slope no pitch hears as an edge.
            // They still drive the body — a fading string is a string.
            for c in &mut self.corpses {
                if c.left == 0 {
                    continue;
                }
                let s = c.mech.tick() * (c.left as f32 / c.total as f32);
                c.left -= 1;
                drive += s;
                let (gl, gr) = pan_gains(c.pan);
                left += s * gl;
                right += s * gr;
            }
            // The soundboard/tonebar knob rides here rather than inside the
            // body, so the board's own decay is untouched and only how much
            // of it reaches the bus moves.
            let (bl, br) = self.body.tick(drive);
            left += bl * body_amt;
            right += br * body_amt;
            // **The amp pass.** What the cabinet does to the sum that the
            // mechanism cannot do to a voice.
            match patch {
                Patch::EpSuitcase => {
                    // The stereo vibrato: the two speakers trade the signal,
                    // equal-power, unity at rest. The one effect every
                    // suitcase record is wearing. Normalised by the loud
                    // side's own peak: equal POWER swings the PEAK up by
                    // root two at full throw, and the bench caught the dry
                    // instrument's forte chords — already voiced to the
                    // ceiling — clipping the moment the throb was on. The
                    // scale is 1.0 at rest, so depth zero IS the dry patch.
                    self.trem_phase = (self.trem_phase + trem_inc)
                        % std::f32::consts::TAU;
                    let pan = trem_depth * self.trem_phase.sin();
                    let a = std::f32::consts::FRAC_PI_4 * (1.0 + pan);
                    let peak = std::f32::consts::SQRT_2
                        * (std::f32::consts::FRAC_PI_4 * (1.0 + trem_depth)).sin();
                    let norm = std::f32::consts::SQRT_2 / peak.max(1.0);
                    left *= a.cos() * norm;
                    right *= a.sin() * norm;
                }
                Patch::Wurli200 | Patch::Wurli140 => {
                    // The reed amp's tremolo throbs LEVEL, in mono — one
                    // output stage, both channels together.
                    self.trem_phase = (self.trem_phase + trem_inc)
                        % std::f32::consts::TAU;
                    let g = 1.0
                        - trem_depth * 0.30 * (1.0 + self.trem_phase.sin());
                    left *= g;
                    right *= g;
                }
                Patch::ClavWah => {
                    // The touch filter: a follower rides the playing and
                    // drags a resonant sweep up with it. Fast up, slow down
                    // - the QUACK is the asymmetry.
                    let lvl = (left.abs() + right.abs()) * 0.5;
                    let coeff = if lvl > self.wah_env { wah_atk } else { wah_rel };
                    self.wah_env += (lvl - self.wah_env) * coeff;
                    let drive = (self.wah_env * 5.5).clamp(0.0, 1.0);
                    let oct = 1.0 + 2.6 * wah_sens;
                    let fc = (240.0 * 2.0_f32.powf(oct * drive))
                        .clamp(180.0, 4_800.0);
                    let fcoef = (2.0
                        * (std::f32::consts::PI * fc / self.rate).sin())
                    .clamp(0.0, 0.7);
                    for (c, x) in [left, right].into_iter().enumerate() {
                        self.wah_low[c] += fcoef * self.wah_band[c];
                        let high = x - self.wah_low[c] - 0.40 * self.wah_band[c];
                        self.wah_band[c] += fcoef * high;
                        if !self.wah_low[c].is_finite()
                            || !self.wah_band[c].is_finite()
                        {
                            self.wah_low[c] = 0.0;
                            self.wah_band[c] = 0.0;
                        }
                        let wet = self.wah_band[c] * 1.15
                            + self.wah_low[c] * 0.30;
                        if c == 0 {
                            left = wet;
                        } else {
                            right = wet;
                        }
                    }
                }
                _ => {}
            }
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
        // A corpse that reached the end of its fade this block is silenced
        // HERE, once — the clearing a strike used to do on every voice —
        // so the next `bury` hands its slot a mechanism a first strike can
        // land on as if from `new`.
        for c in &mut self.corpses {
            if c.left > 0 {
                live += 1;
            } else if !c.mech.finished() {
                c.mech.silence();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Strike a handful of keys at mixed velocities and roll the block loop
    /// the way the engine does. The shape every per-patch test shares.
    fn play(patch: Patch, secs: f32, tweak: &[(usize, f32)]) -> Vec<f32> {
        let mut k = Keys::new(48_000.0);
        k.set_patch(patch);
        for (i, v) in tweak {
            k.set_param(*i, *v);
        }
        for (n, vel) in [(36u8, 0.9), (48, 0.5), (60, 0.75), (64, 0.3), (79, 1.0)] {
            k.note_on(n, vel);
        }
        let frames = (secs * 48_000.0) as usize;
        let mut out = vec![0.0f32; frames * 2];
        let mut at = 0;
        while at < frames {
            let n = 128.min(frames - at);
            k.render(&mut out[at * 2..(at + n) * 2], n, 2);
            if at > frames / 2 {
                k.note_off(60);
            }
            at += n;
        }
        out
    }

    /// **The append-only pin.** The first two indices were written into
    /// settings files before the other six existed; a reorder silently
    /// loads the wrong instrument into every old session.
    #[test]
    fn the_first_two_patch_indices_are_forever() {
        assert_eq!(Patch::ALL[0], Patch::Piano1);
        assert_eq!(Patch::ALL[1], Patch::EpMk1);
        assert_eq!(Patch::ALL.len(), 8);
    }

    /// Every patch speaks, stays finite, and stays inside the headroom its
    /// mechanisms promise — at rest AND with its amp-side knobs pinned to
    /// the top, which is where a runaway filter or tremolo would show.
    #[test]
    fn every_patch_renders_bounded_and_audible() {
        for &patch in &Patch::ALL {
            for tweak in [&[][..], &[(4, 1.0), (5, 1.0)][..]] {
                let out = play(patch, 2.0, tweak);
                let mut peak = 0.0f32;
                let mut energy = 0.0f64;
                for s in &out {
                    assert!(s.is_finite(), "{patch:?} went non-finite");
                    peak = peak.max(s.abs());
                    energy += f64::from(*s) * f64::from(*s);
                }
                assert!(peak < 4.0, "{patch:?} peaked at {peak}");
                let rms = (energy / out.len() as f64).sqrt();
                assert!(rms > 1.0e-4, "{patch:?} rendered near-silence, rms {rms}");
            }
        }
    }

    /// A patch change may not carry the old instrument's ringing into the
    /// new one: the flip damps everything, and what remains after a couple
    /// of blocks is the damper fade dying, not a note.
    #[test]
    fn a_patch_change_silences_what_was_ringing() {
        let mut k = Keys::new(48_000.0);
        k.note_on(48, 1.0);
        let mut buf = vec![0.0f32; 4096 * 2];
        k.render(&mut buf, 4096, 2);
        k.set_patch(Patch::Clav);
        // A quarter second of nothing played on the new patch.
        let mut peak = 0.0f32;
        for _ in 0..94 {
            buf[..256].fill(0.0);
            k.render(&mut buf[..256], 128, 2);
            peak = peak.max(buf[..256].iter().fold(0.0f32, |a, s| a.max(s.abs())));
        }
        assert!(peak < 1.0e-3, "the old patch leaked through at {peak}");
    }

    /// Defaults are the ParamSpec table, verbatim, for every patch — the
    /// panel's Initialise, the untouched strip and the bench agree by
    /// construction, and this keeps it that way.
    #[test]
    fn defaults_are_the_spec_table_for_every_patch() {
        for &patch in &Patch::ALL {
            let d = patch.defaults();
            for (i, spec) in patch.params().iter().enumerate() {
                assert_eq!(d[i], spec.default, "{patch:?} knob {i} ({})", spec.name);
            }
        }
    }

    /// Magnitude at one frequency through a Hann window — the bench's
    /// measuring stick, duplicated here because the tests below pin SPECTRA
    /// and a rectangular window's leakage floor would pin noise instead.
    fn goertzel_hann(x: &[f32], freq: f32, rate: f32) -> f32 {
        if x.is_empty() || freq <= 0.0 || freq >= rate * 0.5 {
            return 0.0;
        }
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

    /// One voice of `mech`, struck once, ticked for `secs` — no pool, no
    /// pan, no body: the mechanism alone, which is what the spectrum tests
    /// are about.
    fn run_voice(tick: &mut dyn FnMut() -> f32, secs: f32) -> Vec<f32> {
        (0..(secs * 48_000.0) as usize).map(|_| tick()).collect()
    }

    /// **The clav has no bell.** Its brightness is combs over a stretched
    /// HARMONIC series; for one release the shared dispersion map was
    /// inverted against stiffness and parked a long-ringing upper series up
    /// to 122 cents sharp — an enharmonic chime this instrument does not
    /// own. Every component loud enough to matter, at every velocity, must
    /// sit within a fraction of a semitone of the stretched ideal.
    #[test]
    fn the_clavs_ringing_partials_sit_on_the_stretched_string_series() {
        let f0 = 261.6256_f32;
        // The B schedule at C4, exactly as clav.rs computes it.
        let b = 1.2e-4 * 2.0_f32.powf((60.0 - 52.0) / 26.0);
        for vel in [0.3_f32, 0.7, 1.0] {
            let mut v = clav::ClavVoice::new(48_000.0);
            v.strike_knobs(60, vel, &ClavKnobs::default());
            let out = run_voice(&mut || v.tick(), 0.85);
            // Skip the strike transient: broadband by nature, innocent here.
            let w = &out[(0.05 * 48_000.0) as usize..];
            // Scan the whole region above the fundamental in 10-cent steps.
            let mut mags = Vec::new();
            let mut f = f0 * 1.5;
            while f < f0 * 8.5 {
                mags.push((f, goertzel_hann(w, f, 48_000.0)));
                f *= 1.005_79; // ten cents
            }
            let top = mags.iter().fold(0.0_f32, |m, &(_, g)| m.max(g));
            let floor = top * 0.031_6; // -30 dB: quieter than this is tail
            for &(f, g) in &mags {
                if g < floor {
                    continue;
                }
                // Distance to the nearest stretched partial, in cents.
                let nearest = (2..=9)
                    .map(|n| {
                        let ideal =
                            n as f32 * f0 * (1.0 + b * (n * n) as f32).sqrt();
                        (1_200.0 * (f / ideal).log2()).abs()
                    })
                    .fold(f32::MAX, f32::min);
                assert!(
                    nearest < 40.0,
                    "clav at vel {vel}: {f:.0} Hz rings {nearest:.0} cents \
                     off the stretched series — that is a bell"
                );
            }
        }
    }

    /// **The two electric mechanisms may never converge.** The reed's upper
    /// energy is the plate's GROWL — second and third harmonics, integer
    /// ratios, with its one bar overtone buried — while the tine carries a
    /// loud BELL far from any integer. One release shipped them swapped:
    /// the reed's overtone sat half a semitone from the tine's bell, rang
    /// into the sustain, and the reed out-Rhodesed the Rhodes. This pins
    /// each mechanism to its own signature: what dominates the reed's top,
    /// how far down its inharmonic content lies, and how loud and how
    /// off-integer the tine's bell is.
    #[test]
    fn the_reed_speaks_harmonics_where_the_tine_speaks_a_bell() {
        let f0 = 261.6256_f32;
        let peak_near = |x: &[f32], around: f32| -> (f32, f32) {
            let mut best = (0.0_f32, 0.0_f32);
            let mut f = around * 0.95;
            while f < around * 1.05 {
                let g = goertzel_hann(x, f, 48_000.0);
                if g > best.1 {
                    best = (f, g);
                }
                f += around * 0.001;
            }
            best
        };

        let mut reed = reed::WurliVoice::new(48_000.0);
        reed.strike_knobs(60, 1.0, &WurliKnobs::default(), &reed::WurliBase::TWO_HUNDRED);
        let out = run_voice(&mut || reed.tick(), 0.25);
        // The loudest thing above the fundamental must be a harmonic.
        let mut best = (0.0_f32, 0.0_f32);
        let mut f = f0 * 1.5;
        while f < f0 * 8.2 {
            let g = goertzel_hann(&out, f, 48_000.0);
            if g > best.1 {
                best = (f, g);
            }
            f *= 1.005_79; // ten cents
        }
        let r = best.0 / f0;
        assert!(
            (r - r.round()).abs() < 0.08 && (1.8..=3.2).contains(&r),
            "the reed's loudest upper partial sits at {r:.2} x f0 — growl \
             is harmonic, this is not"
        );
        // And its one inharmonic voice — the solder-dropped bar overtone —
        // stays attack colour: well under the fundamental even in the
        // attack window, where it is at its loudest.
        let fund = goertzel_hann(&out, f0, 48_000.0).max(1.0e-12);
        let drop = 1.0 - 0.06 * ((60.0_f32 - 33.0) / 40.0).clamp(0.0, 1.0);
        let over1 = peak_near(&out, f0 * 4.4 * drop).1;
        let over_db = 20.0 * (over1 / fund).max(1.0e-12).log10();
        assert!(
            over_db < -35.0,
            "the reed's bar overtone reads {over_db:.0} dB under the \
             fundamental; louder than -35 and it starts sounding like a bell"
        );

        let mut tine = tine::EpVoice::new(48_000.0);
        tine.strike_knobs(60, 1.0, &EpKnobs::default());
        let out = run_voice(&mut || tine.tick(), 0.25);
        let fund = goertzel_hann(&out, f0, 48_000.0).max(1.0e-12);
        // The bell mode as tine.rs places it at C4, chime centred.
        let bell_ratio = 6.6 - 0.030 * (60.0 - 28.0);
        let (bf, bg) = peak_near(&out, f0 * bell_ratio);
        let bell_db = 20.0 * (bg / fund).max(1.0e-12).log10();
        let t = bf / f0;
        assert!(
            bell_db > -30.0,
            "the tine's bell reads {bell_db:.0} dB under the fundamental at \
             fortissimo — that is the under-belled Rhodes coming back"
        );
        assert!(
            (t - t.round()).abs() > 0.25,
            "the tine's bell sits at {t:.2} x f0 — a bell lives OFF the \
             harmonic series, this one converged onto it"
        );
    }

    /// **STRETCH turns the way it says.** The inverted dispersion map made
    /// knob zero the stiffest wire on the panel; this holds the direction:
    /// more knob, sharper upper partials.
    #[test]
    fn the_stretch_knob_pulls_the_partials_sharper_not_flatter() {
        let f0 = 261.6256_f32;
        let n6_at = |stretch: f32| -> f32 {
            let mut k = PianoKnobs::default();
            k.stretch = stretch;
            k.unison = 0.0; // measure the design, not the beat geometry
            let mut v = string::PianoVoice::new(48_000.0);
            v.strike_based(60, 0.7, &k, &string::PianoBase::JAZZ);
            let out = run_voice(&mut || v.tick(), 1.0);
            let mut best = (0.0_f32, 0.0_f32);
            let mut f = 6.0 * f0 * 0.98;
            while f < 6.0 * f0 * 1.10 {
                let g = goertzel_hann(&out, f, 48_000.0);
                if g > best.1 {
                    best = (f, g);
                }
                f += f0 * 0.006;
            }
            best.0
        };
        let lo = n6_at(bipolar(0.0, 1.6));
        let hi = n6_at(bipolar(1.0, 1.6));
        let cents = 1_200.0 * (hi / lo).log2();
        assert!(
            cents > 20.0,
            "full stretch travel moved n6 by {cents:.0} cents; it must pull \
             clearly SHARP with the knob, not sit still or fall"
        );
    }

    /// **The dispersion budget never detunes a short string.** The all-pass
    /// cascade eats loop delay, and at the top of the keyboard there is
    /// almost none to eat: the per-note cap must leave the fundamental on
    /// pitch at both stretch extremes, or C8 plays flat and no test below
    /// A5 would ever notice.
    #[test]
    fn the_treble_stays_on_pitch_at_either_stretch_extreme() {
        for pitch in [105_u8, 108] {
            let f0 = 440.0 * 2.0_f32.powf((f32::from(pitch) - 69.0) / 12.0);
            for stretch in [bipolar(0.0, 1.6), bipolar(1.0, 1.6)] {
                let mut k = PianoKnobs::default();
                k.stretch = stretch;
                k.unison = 0.0;
                let mut v = string::PianoVoice::new(48_000.0);
                v.strike_based(pitch, 0.7, &k, &string::PianoBase::JAZZ);
                let out = run_voice(&mut || v.tick(), 0.4);
                let mut best = (0.0_f32, 0.0_f32);
                let mut f = f0 * 0.97;
                while f < f0 * 1.03 {
                    let g = goertzel_hann(&out, f, 48_000.0);
                    if g > best.1 {
                        best = (f, g);
                    }
                    f += f0 * 0.000_5;
                }
                let cents = 1_200.0 * (best.0 / f0).log2();
                assert!(
                    cents.abs() < 15.0,
                    "pitch {pitch} at stretch {stretch:.2} sounds {cents:.0} \
                     cents off — the dispersion cap failed to protect it"
                );
            }
        }
    }

    /// The two tremolos and the wah live on the SUM, so they must engage
    /// without a strike-time knob read: turn the knob mid-note and the
    /// held sound changes. Rendering the suitcase with tremolo hard on and
    /// hard off must produce audibly different left channels.
    #[test]
    fn the_amp_knobs_act_on_a_note_already_ringing() {
        let render_with = |depth: f32| -> Vec<f32> {
            let mut k = Keys::new(48_000.0);
            k.set_patch(Patch::EpSuitcase);
            k.note_on(60, 0.8);
            k.set_param(5, depth);
            let mut out = vec![0.0f32; 48_000 * 2];
            let mut at = 0;
            while at < 48_000 {
                k.render(&mut out[at * 2..(at + 128) * 2], 128, 2);
                at += 128;
            }
            out
        };
        let still = render_with(0.0);
        let throb = render_with(1.0);
        let diff: f64 = still
            .iter()
            .zip(&throb)
            .map(|(a, b)| f64::from((a - b).abs()))
            .sum();
        assert!(diff > 1.0, "the tremolo knob did nothing to a held note");
    }
}
