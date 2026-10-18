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

pub mod beam;
pub mod board;
pub mod clav;
pub mod clav_wah;
pub mod egrand;
pub mod egrand_hammer;
pub mod grand;
pub mod grand_noise;
pub mod guitar;
pub mod hammer;
pub mod modal;
pub mod pickup;
pub mod reed;
pub mod reed_amp;
pub mod string;
pub mod symp;
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
    /// An intimate grand, heard close: soft felt, a warm low end, a quiet
    /// action you can hear, and a dynamic window that lives at mezzo-piano.
    /// Voiced for playing alone in a small room (`GrandBase::INTIMATE`).
    #[default]
    Piano1,
    /// A tine electric piano, dry — no amplifier, no tremolo, no chorus.
    /// Bell-like when it is played lightly, and it barks when it is not.
    EpMk1,
    /// The concert instrument: longer wire, longer ring, a deep extended
    /// bass, wider dynamics, a quieter action. The hall grand to Piano 1's
    /// small room (`GrandBase::CONCERT`).
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
    /// A reed piano, taken dry: hollow when feathered, growling when leaned
    /// on. Knob 5 is the old amp throb, off by default.
    Wurli200,
    /// The reed piano through its amp: a slap, a cabinet, a 4 Hz throb and
    /// a small room (`reed_amp`). Shown as Reed Amp.
    Wurli140,
    /// A stage electric grand: short, stiff piano strings heard through
    /// pickups under the bridge, no soundboard — see the `egrand` module.
    ElectricGrand,
    /// A nylon-strung classical guitar: six strings, first position, a
    /// fingertip's pluck, the open strings ringing along. See `guitar`.
    Guitar,
}

/// Which physics answers a patch: one mechanism per family, shared by the
/// sounds built on it (two grands, two tine pianos, two clavs, two reeds).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Piano,
    Ep,
    Clav,
    Wurli,
    EGrand,
    Guitar,
}

impl Patch {
    fn family(self) -> Family {
        match self {
            Patch::Piano1 | Patch::Piano2 => Family::Piano,
            Patch::EpMk1 | Patch::EpSuitcase => Family::Ep,
            Patch::Clav | Patch::ClavWah => Family::Clav,
            Patch::Wurli200 | Patch::Wurli140 => Family::Wurli,
            Patch::ElectricGrand => Family::EGrand,
            Patch::Guitar => Family::Guitar,
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
    /// settings files for months before the rest existed.
    pub const ALL: [Patch; 10] = [
        Patch::Piano1,
        Patch::EpMk1,
        Patch::Piano2,
        Patch::EpSuitcase,
        Patch::Clav,
        Patch::ClavWah,
        Patch::Wurli200,
        Patch::Wurli140,
        Patch::ElectricGrand,
        Patch::Guitar,
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
            Patch::ElectricGrand => &EGRAND_PARAMS,
            Patch::Guitar => &GUITAR_PARAMS,
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

    /// **What the frame lifts this sound by on its way out**, a factor: one,
    /// exactly, for six of the ten, which are the instrument they were to
    /// the bit.
    ///
    /// The two grands, the electric grand and the guitar were each set by
    /// their loudest playing to peak a few dB under full scale (the grands'
    /// pedalled fortissimo chords at -2.9 dBFS, the guitar's six-string
    /// strum at full velocity at -3.6), where the tine and reed pianos and
    /// the clavs had been voiced up to it. So at forte they sat 7 to 8 dB
    /// under the other six: comping, -27.1 to -28.0 dB against -18.8 to
    /// -23.4 (the reference study's listening scores, 300 ms windows within
    /// 40 dB of the loudest, both channels), and a six-note fortissimo
    /// chord's RMS -25.3 to -26.2 dBFS against -16.6 to -20.7. `LIFT` puts
    /// them in the middle of the other six on both; their six-note
    /// fortissimo chords then peak at -1.0 to +1.3 dBFS, the other six's at
    /// -0.9 to +3.7.
    ///
    /// **At the output, after everything the instrument does**, so nothing
    /// it measures of itself moves: its culls, its sleep and the harp's are
    /// where they were, and only what the host hears is louder.
    pub fn lift(self) -> f32 {
        match self {
            Patch::Piano1 | Patch::Piano2 | Patch::ElectricGrand | Patch::Guitar => LIFT,
            _ => 1.0,
        }
    }
}

/// 7 dB, `10^(7/20)`: see `Patch::lift`.
const LIFT: f32 = 2.238_721;

/// The most knobs any one sound exposes. The store is a fixed array so that
/// nothing on the audio thread has a length to get wrong.
pub const MAX_PARAMS: usize = 8;

/// One knob, as the panel needs to draw it and the file needs to keep it.
///
/// **The panel does not read `name` and `about` from here** — `tangent-ui`
/// cannot see this crate, so `KEYS_PARAM_SPECS` carries its own copy of both
/// and `the_panel_knobs_and_the_engine_knobs_are_the_same_knobs` asserts the
/// two tables word for word. These fields are the side that test measures
/// against, which is the whole reason they are written down twice.
pub struct ParamSpec {
    /// What the panel calls it.
    #[allow(dead_code)]
    pub name: &'static str,
    /// The one line under the name. What it does to the INSTRUMENT, not what
    /// it does to the number.
    #[allow(dead_code)]
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
        about: "the tine-height screw: how near the tip rests to the magnet",
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
        about: "the tine-height screw: how near the tip rests to the magnet",
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
        default: 0.6,
    },
    ParamSpec {
        name: "Tone",
        about: "the filter rockers, folded to one sweep",
        default: 0.75,
    },
    ParamSpec {
        name: "Snap",
        about: "how much of the muted length rejoins the string at release",
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
        default: 0.6,
    },
    ParamSpec {
        name: "Wah",
        about: "how hard the filter chases your touch",
        default: 0.55,
    },
    ParamSpec {
        name: "Snap",
        about: "how much of the muted length rejoins the string at release",
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
        default: 0.0,
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
        about: "the reed's regulation: where it rests in the plate's field",
        default: 0.5,
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

/// The reed piano through its amp: the same knobs as the dry one, with
/// Tremolo now the amp's throb in dB (`reed_amp::WET_DB_PER_KNOB`; 0.5 is
/// the reference's 3 dB swing) and Growl at the base's own regulation.
const WURLI140_PARAMS: [ParamSpec; MAX_PARAMS] = [
    ParamSpec {
        name: "Voicing",
        about: "hammer felt: worn soft to packed hard",
        default: 0.5,
    },
    ParamSpec {
        name: "Width",
        about: "how far bass and treble sit apart",
        default: 0.0,
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
        about: "the reed's regulation: where it rests in the plate's field",
        default: 0.5,
    },
    ParamSpec {
        name: "Tremolo",
        about: "the amp's throb",
        default: 0.5,
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

/// The electric grand's knobs (`egrand::EGrandKnobs`). VOICING and WIDTH sit
/// where every patch has them; knob 7 is the PICKUP, not a soundboard — the
/// frame's `Body` is silent for this patch (`string::Body::tick` answers only
/// the two pianos).
const EGRAND_PARAMS: [ParamSpec; MAX_PARAMS] = [
    ParamSpec {
        name: "Voicing",
        about: "hammer felt: needled soft to lacquered hard",
        default: 0.5,
    },
    ParamSpec {
        name: "Width",
        about: "how differently the two pickup rails hear a string",
        default: WIDTH_DEFAULT,
    },
    ParamSpec {
        name: "Decay",
        about: "how long a struck string rings",
        default: 0.5,
    },
    ParamSpec {
        name: "Brilliance",
        about: "how long the upper partials ring: the wire's friction",
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
        name: "Pickup",
        about: "where the pickups read the strings: round to thin",
        default: 0.5,
    },
];

/// The guitar's knobs: every one a physical quantity of the instrument and
/// its player, mapped in `guitar::GuitarKnobs`. WIDTH sits where every
/// patch has it.
const GUITAR_PARAMS: [ParamSpec; MAX_PARAMS] = [
    ParamSpec {
        name: "Voicing",
        about: "the plucking finger: flesh to nail",
        default: guitar::DEFAULTS[0],
    },
    ParamSpec {
        name: "Width",
        about: "how far apart the two close microphones hear it",
        default: guitar::DEFAULTS[1],
    },
    ParamSpec {
        name: "Decay",
        about: "how long a plucked string rings",
        default: guitar::DEFAULTS[2],
    },
    ParamSpec {
        name: "Position",
        about: "where the finger plucks: near the bridge or over the soundhole",
        default: guitar::DEFAULTS[3],
    },
    ParamSpec {
        name: "Body",
        about: "how much the hollow body's air speaks",
        default: guitar::DEFAULTS[4],
    },
    ParamSpec {
        name: "Pick",
        about: "how long the finger holds the string before letting go",
        default: guitar::DEFAULTS[5],
    },
    ParamSpec {
        name: "Release",
        about: "how fast a lifted finger stops the string",
        default: guitar::DEFAULTS[6],
    },
    ParamSpec {
        name: "Strings",
        about: "the open strings that ring in sympathy",
        default: guitar::DEFAULTS[7],
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
    /// 0 mellow, 1 bright: the hammer's felt, needled soft to lacquered
    /// hard — a stiffness, `string::VOICING_STIFFNESS` at either end, and
    /// the contact width follows from it. Not a width curve any more: the
    /// felt is a spring the string pushes back on, and how long the two
    /// touch is what that argument settles, per strike.
    pub voicing: f32,
    /// How far the two microphones of the modal grand hear a partial
    /// differently, and how far the key's place along the bridge pans it:
    /// 1.0 is the stock seating (knob 1 at its default, 1/1.5 of the knob),
    /// 0 is one microphone, 1.5 the top of the knob. The waveguide piano
    /// (`string::PianoVoice`, which no patch plays now) is a mono voice and
    /// never reads this.
    pub width: f32,
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
    ///
    /// **The piano's board send does NOT come through here** — `render`
    /// takes knob 7 straight off `params[P_BODY]` and cubes it, for the
    /// reason `P_BODY`'s own doc gives. This is the mapping DECLARED, so
    /// that `an_initialised_sound_is_the_bench_tuning` can hold the piano's
    /// knob 7 to the same 1.0 centre as every other knob and as `EpKnobs`'
    /// tonebar, which does read its copy.
    #[allow(dead_code)]
    pub body: f32,
}

impl PianoKnobs {
    pub fn from_params(p: &[f32; MAX_PARAMS]) -> Self {
        PianoKnobs {
            voicing: p[P_VOICING].clamp(0.0, 1.0),
            width: (p[P_WIDTH] * WIDTH_FULL).clamp(0.0, WIDTH_FULL),
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
    /// How far apart the two channels hear the tine: the phase difference
    /// the voice's two all-passes put on the note, and a level pan of at
    /// most ±1.4 dB, bass left. 1.0 is the stock image (knob 1 at its
    /// default, 1/1.5 of the knob), 0 is mono to the bit, 1.5 the top of
    /// the knob. The tine voices own their image, as every voice does: the
    /// frame pans none.
    pub width: f32,
    /// Multiplier on the tine's t60.
    pub decay: f32,
    /// The bell, on 0..2: up to 1 the reference tine piano's own quiet
    /// bell (40-60 dB under the fundamental at the strike) scaled linearly
    /// to nothing; from 1 to 2 raised to +25 dB over it, the old glass bell
    /// over the note.
    pub bell: f32,
    /// The tine-height screw: a scale on the harp's per-key schedule of rest
    /// positions, `x₀ = rest(key) · bark^−0.7`, on the knob's 0.43..2.3
    /// around a centre of 1 that is the base's own regulation (the dry harp
    /// rests 0.23-0.28 gap below the axis from F1 to C4 and 0.5 at C6; the
    /// suitcase's climbs toward the bass, 0.56 at C2). Bigger is nearer the
    /// pole and dirtier, because the flux curve's flank makes more
    /// harmonics out of the same swing there; the knob's bottom puts middle
    /// C on the curve's inflection, the cleanest regulation it has. It is
    /// the adjustment a technician makes with a screwdriver, held at the
    /// transducer's floor, away from the axis itself, where the transfer
    /// goes even and the note would play an octave up. Read at the strike,
    /// and a note already ringing glides to the new regulation rather than
    /// stepping to it.
    pub bark: f32,
    /// Shifts the bell partial up or down the series, in partials.
    pub chime: f32,
    /// Multiplier on the damper's time constant.
    pub release: f32,
    /// The tonebar's coupling to the tine, on 0..2: up to 1 the factory's
    /// light coupling (the reference rings without a beat) scaled to a tine
    /// on dead wood; from 1 to 2 rising to the old regulation, where the
    /// slow beat and the two-stage aftersound live.
    pub body: f32,
}

impl EpKnobs {
    pub fn from_params(p: &[f32; MAX_PARAMS]) -> Self {
        EpKnobs {
            voicing: p[P_VOICING].clamp(0.0, 1.0),
            width: (p[P_WIDTH] * WIDTH_FULL).clamp(0.0, WIDTH_FULL),
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
    /// The rocker, four positions in the order the knob has always
    /// stored them: the lower coil alone, the upper coil alone, both in
    /// phase (the shipped 0.6), both in anti-phase. See `clav::rocker`.
    pub pickup: f32,
    /// The rockers, 0 soft to 1 brilliant.
    pub tone: f32,
    /// How much of the non-speaking length rejoins the string when the
    /// tangent lifts, on the knob's 0..2 around a centre of 1 that is the
    /// paper's measured three semitones for the whole keyboard. The
    /// release is not a click any more: the two halves of the string
    /// become one longer string, the pitch drops, and the yarn takes the
    /// whole thing over a few milliseconds later. Zero releases at pitch,
    /// which is a damper and not a clavinet; two drops the better part of
    /// an octave.
    pub snap: f32,
    /// Multiplier on the yarn's choke time.
    pub release: f32,
    /// The damper slider, 0 free to 1 pressed.
    pub mute: f32,
    /// The voice's own image (every voice owns its image: the frame pans
    /// none): 1.0 is the stock seating, knob 1 at its default, 0 is mono,
    /// 1.5 the top of the knob.
    pub width: f32,
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
            width: (p[P_WIDTH] * WIDTH_FULL).clamp(0.0, WIDTH_FULL),
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
    /// How far bass and treble sit apart: the reed voice owns its stereo
    /// image (every voice does: the frame pans none), so WIDTH reaches it
    /// here. 0 is mono, both channels the same sample, and the knob's top
    /// is `WIDTH_FULL`.
    pub width: f32,
    /// Multiplier on the reed's t60.
    pub decay: f32,
    /// Scale on the bar overtones' weight.
    pub reed: f32,
    /// The reed's rest offset from the plate plane, on the knob's 0..2:
    /// 1 is the base's own per-key regulation (0.15 gap in the bass on
    /// both patches), and each unit scales the rest distance by 2^-0.4
    /// (top 0.76x, bottom 1.32x). UP is nearer the plane and dirtier, with
    /// more octave; down is hollower and odd-led. Read at the strike; a
    /// ringing note keeps the regulation it was struck with. See
    /// `reed::GROWL_REACH`.
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
            width: (p[P_WIDTH] * WIDTH_FULL).clamp(0.0, WIDTH_FULL),
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
/// **Every mechanism, always, in every voice.** The obvious build has a
/// `Box<dyn Voice>` or an enum holding one of them, and rebuilds the pool
/// when the patch changes. Both are wrong here for the same reason: the patch
/// is chosen in the UI, but the instrument lives on the audio thread — the
/// `Renderer` is MOVED into the callback, so the host cannot reach in and swap
/// anything. The choice has to travel as an atomic and be applied by the
/// callback itself, and a callback may not allocate. So the allocation happens
/// once, in [`Keys::new`], and changing patch is a flag and a damp.
///
/// It costs about 47 KB a voice held in place (`size_of`, 2026-10-17; the
/// clav's loop is half of it), so about 3 MB for a pool of 64 to carry the
/// instruments it is not currently being, plus the mode banks the modal
/// voices allocate once in `new` — against a stem ring the engine already
/// allocates in the tens of megabytes. (The old waveguide piano, 49 KB a
/// voice, left the pool when the modal grand took Piano 1 and Piano 2.) It buys a patch change that cannot glitch, cannot allocate, and
/// needs no queue, no lock and no `&mut` reaching across a thread boundary.
struct Mechanism {
    /// The modal grand: what Piano 1 and Piano 2 play. Rendered a block at
    /// a time (`render_block`), never ticked.
    grand: grand::GrandVoice,
    ep: tine::EpVoice,
    clav: clav::ClavVoice,
    wurli: reed::WurliVoice,
    /// The electric grand: block-rendered like the modal grand.
    egrand: egrand::EGrandVoice,
    /// The nylon guitar: one string, plucked, rendered a block at a time.
    guitar: guitar::GuitarVoice,
    /// Which mechanism is speaking, and which base it wears. The FAMILY is
    /// asked once per voice per block; the patch only at the strike.
    family: Family,
    patch: Patch,
}

impl Mechanism {
    fn new(rate: f32) -> Self {
        Mechanism {
            grand: grand::GrandVoice::new(rate),
            ep: tine::EpVoice::new(rate),
            clav: clav::ClavVoice::new(rate),
            wurli: reed::WurliVoice::new(rate),
            egrand: egrand::EGrandVoice::new(rate),
            guitar: guitar::GuitarVoice::new(rate),
            family: Family::Piano,
            patch: Patch::Piano1,
        }
    }

    /// Become another instrument. Silences every one, because a tine ringing
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
        self.grand.silence();
        self.ep.silence();
        self.clav.silence();
        self.wurli.silence();
        self.egrand.silence();
        self.guitar.silence();
    }

    /// Whether a strike of `pitch` with these knobs lands on what this
    /// voice is ringing, or would have to replace it. The modal instruments
    /// always land — a resonator takes new coefficients under its motion
    /// without a step — and the clav's loop lands when its design is the
    /// one ringing; see `clav::ClavVoice::lands`.
    fn lands(&self, pitch: u8, params: &[f32; MAX_PARAMS]) -> bool {
        match self.patch {
            Patch::Piano1 => self.grand.lands(
                pitch,
                &PianoKnobs::from_params(params),
                &grand::GrandBase::INTIMATE,
            ),
            Patch::Piano2 => self.grand.lands(
                pitch,
                &PianoKnobs::from_params(params),
                &grand::GrandBase::CONCERT,
            ),
            Patch::EpMk1 | Patch::EpSuitcase => !self.ep.finished(),
            Patch::Clav => self.clav.lands(pitch, &ClavKnobs::from_params(params)),
            Patch::ClavWah => self.clav.lands(pitch, &ClavKnobs::from_params_wah(params)),
            Patch::Wurli200 | Patch::Wurli140 => !self.wurli.finished(),
            Patch::ElectricGrand => self.egrand.lands(
                pitch,
                &egrand::EGrandKnobs::from_params(params),
                &egrand::EGrandBase::ELECTRIC,
            ),
            // The guitar's landing depends on the string it is played on,
            // which only the frame's fretboard knows: `lands_guitar`.
            Patch::Guitar => false,
        }
    }

    /// The guitar's `lands`: the same note on the same string and fret (a
    /// re-pluck after a knob move re-voices the ringing string itself, so the
    /// frame never buries a guitar voice; `guitar::Design`).
    fn lands_guitar(&self, place: guitar::Place, pitch: u8, params: &[f32; MAX_PARAMS]) -> bool {
        self.guitar.lands(place, pitch, &guitar::GuitarKnobs::from_params(params))
    }

    /// The guitar's `strike`: `place` is the string the frame's selector chose.
    fn strike_guitar(&mut self, place: guitar::Place, pitch: u8, velocity: f32, params: &[f32; MAX_PARAMS]) {
        self.guitar.strike(place, pitch, velocity, &guitar::GuitarKnobs::from_params(params));
    }

    /// Another note took this voice's string. Only a guitar has strings to
    /// take; every other mechanism is simply damped.
    fn choke(&mut self) {
        match self.family {
            Family::Guitar => self.guitar.choke(),
            _ => self.damp(),
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
    fn strike(&mut self, pitch: u8, velocity: f32, params: &[f32; MAX_PARAMS], designs: &grand::GrandDesigns) {
        match self.patch {
            // A fresh strike copies its strings from the frame's solved
            // table when it holds them for these knobs (`grand::GrandDesigns`).
            Patch::Piano1 => self.grand.strike_designed(
                pitch,
                velocity,
                &PianoKnobs::from_params(params),
                &grand::GrandBase::INTIMATE,
                designs,
            ),
            Patch::Piano2 => self.grand.strike_designed(
                pitch,
                velocity,
                &PianoKnobs::from_params(params),
                &grand::GrandBase::CONCERT,
                designs,
            ),
            Patch::EpMk1 => self.ep.strike_based(
                pitch,
                velocity,
                &EpKnobs::from_params(params),
                &tine::TineBase::MK1,
            ),
            Patch::EpSuitcase => self.ep.strike_based(
                pitch,
                velocity,
                &EpKnobs::from_params_suitcase(params),
                &tine::TineBase::SUITCASE,
            ),
            Patch::Clav => self.clav.strike_based(
                pitch,
                velocity,
                &ClavKnobs::from_params(params),
                &clav::ClavBase::DRY,
            ),
            Patch::ClavWah => self.clav.strike_based(
                pitch,
                velocity,
                &ClavKnobs::from_params_wah(params),
                &clav::ClavBase::WAH,
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
            Patch::ElectricGrand => self.egrand.strike(
                pitch,
                velocity,
                &egrand::EGrandKnobs::from_params(params),
                &egrand::EGrandBase::ELECTRIC,
            ),
            // Struck through `strike_guitar`, with the string the selector
            // chose; a strike without one is the first-position string (or,
            // off the fingerboard, the key's own).
            Patch::Guitar => self.strike_guitar(guitar::Place::home(pitch), pitch, velocity, params),
        }
    }

    fn release(&mut self, pedal: bool) {
        match self.family {
            Family::Piano => self.grand.release(pedal),
            Family::Ep => self.ep.release(pedal),
            Family::Clav => self.clav.release(pedal),
            Family::Wurli => self.wurli.release(pedal),
            Family::EGrand => self.egrand.release(pedal),
            Family::Guitar => self.guitar.release(pedal),
        }
    }

    /// Key up at a sustain pedal position (CC 64 as 0..1). The modal grand's
    /// dampers follow a pedal that is part way down (half pedalling); the
    /// other mechanisms know a pedal that is down or up.
    fn release_at(&mut self, sustain: f32) {
        match self.family {
            Family::Piano => self.grand.release_at(sustain),
            _ => self.release(sustain >= 0.5),
        }
    }

    /// The sustain pedal moved (CC 64 as 0..1); `held`: the key is down, or
    /// the sostenuto holds its damper. Only the grand follows the pedal
    /// continuously; for the others the frame's pedal-up damping
    /// (`Keys::set_pedals`) is the whole story.
    fn set_sustain(&mut self, sustain: f32, held: bool) {
        if self.family == Family::Piano {
            self.grand.set_sustain(sustain, held);
        }
    }

    /// Whether this voice's dampers follow the sustain pedal's level
    /// (`set_sustain`): the modal grand's do, and the frame must not then
    /// drop them with `damp` when the pedal crosses 64, which would land a
    /// half-pedalled damper with its full weight.
    fn follows_sustain(&self) -> bool {
        self.family == Family::Piano
    }

    /// The soft pedal (CC 67 as 0..1), for this voice's strikes to come.
    fn set_soft(&mut self, soft: f32) {
        if self.family == Family::Piano {
            self.grand.set_soft(soft);
        }
    }

    fn damp(&mut self) {
        match self.family {
            Family::Piano => self.grand.damp(),
            Family::Ep => self.ep.damp(),
            Family::Clav => self.clav.damp(),
            Family::Wurli => self.wurli.damp(),
            Family::EGrand => self.egrand.damp(),
            Family::Guitar => self.guitar.damp(),
        }
    }

    fn finished(&self) -> bool {
        match self.family {
            Family::Piano => self.grand.finished(),
            Family::Ep => self.ep.finished(),
            Family::Clav => self.clav.finished(),
            Family::Wurli => self.wurli.finished(),
            Family::EGrand => self.egrand.finished(),
            Family::Guitar => self.guitar.finished(),
        }
    }

    /// This voice's loudness right now, for the stealing policy. Cheap: it is
    /// a running envelope the voice already keeps, not a measurement.
    fn level(&self) -> f32 {
        match self.family {
            Family::Piano => self.grand.level(),
            Family::Ep => self.ep.level(),
            Family::Clav => self.clav.level(),
            Family::Wurli => self.wurli.level(),
            Family::EGrand => self.egrand.level(),
            Family::Guitar => self.guitar.level(),
        }
    }

    /// One block of a block-rendered voice, ADDED into the three slices:
    /// the grand is a mode bank, and a bank pays for itself only when its
    /// loop runs over a whole block (see `Keys::render_blocks`).
    fn render_block(&mut self, l: &mut [f32], r: &mut [f32], drive: &mut [f32]) {
        match self.family {
            Family::Piano => self.grand.render(l, r, drive),
            // The tine piano owns its stereo image and adds nothing to
            // `drive` (it has no body).
            Family::Ep => self.ep.render(l, r, drive),
            Family::Clav => self.clav.render(l, r, drive),
            Family::EGrand => self.egrand.render(l, r, drive),
            // The reed piano too: it owns its stereo image by the frame's own
            // pan law (`WurliKnobs::width`) and adds nothing to `drive`.
            Family::Wurli => self.wurli.render(l, r, drive),
            // The guitar's two close microphones are its own; its drive is
            // its string's bus (`Keys::render_blocks`, which renders a
            // sounding note's voice itself, to hand its string back).
            Family::Guitar => {
                self.guitar.render(l, r, drive);
            }
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
    /// Bumped on every strike, so the oldest voice is identifiable without a
    /// clock. Wraps harmlessly — only the ORDER matters, and a wrap needs
    /// four billion notes.
    age: u32,
    /// Stopped by `all_notes_off`: the pedals no longer reach it, until its
    /// key is struck again. A cut voice kept its key, and the next sustain
    /// level lifted its dampers again (review).
    cut: bool,
}

/// A voice that had to leave the pool while it was still audible, finishing
/// on its own: the mechanism, and how much of its fade is left.
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
    /// Samples of fade left. Zero is a free slot.
    left: u32,
    total: u32,
}

/// How many voices may be fading out at once. Stealing is rare on a pool
/// of sixty-four, and a fade is four milliseconds, but a chord the host
/// delivers in one block steals all its keys in the same instant, before
/// any corpse has faded at all: with three, an eight-key chord on a full
/// pool cut five voices at full gain (review). Eight is that chord, at one
/// `Mechanism` each, 245 KB of heap (counted: `Keys::new` 18.63 MB with
/// three, 19.82 MB with eight). When even eight are not enough, the corpse
/// nearest the end of its fade is cut, at the smallest gain on offer.
const CORPSES: usize = 8;

/// How long a stolen voice takes to fade, in seconds. Long enough that the
/// slope is not an edge at any pitch the instrument plays, short enough
/// that the new note is not heard arriving under an old one.
const CORPSE_FADE: f32 = 0.004;

/// How long all notes off takes to fade the shared side out, and to fade it
/// back in after the clear, seconds (see `Keys::hush_left`). Ten
/// milliseconds rather than a corpse's four: what fades here is a pedalled
/// chord's whole wash with its bass strings, A0's period alone is 36 ms, and
/// nothing new is waiting to be heard under it.
const HUSH_FADE: f32 = 0.010;

/// The keys of the three patches with a harp, A0 to C8: the compass the
/// grands and the electric grand clamp a strike to, and their harps'
/// strings (`symp::KEYS` of them, from A0).
const HARP_KEYS: std::ops::RangeInclusive<u8> = 21..=108;

/// The sympathetic harp's send for each piano patch: the measured
/// references' "sympathetic resonance" amounts, 0.40 on the intimate grand
/// and 1.00 on the concert grands, as one gain on what the bridge drives
/// the harp with (`symp::StringSet::send`). Each grand's strings are voiced
/// at its own send against its own reference (`symp::GrandHarp`): the
/// intimate's halo, tail and silent-key answers at 0.40, the concert's
/// wider halo and shorter tail at 1.00.
const HARP_SEND_INTIMATE: f32 = 0.40;
const HARP_SEND_CONCERT: f32 = 1.00;
/// The electric grand's: its reference's sympathetic resonance is 1.00, and
/// its own couplings (`egrand::EGrandBase::harp_couple`, `harp_own`) are
/// voiced at this send.
const HARP_SEND_ELECTRIC: f32 = 1.00;

/// The electric grand's string set (`egrand::harp_lines`): its stretched
/// scale on its own B, its own losses and couplings, dampers up to its last
/// damper (93), no aliquots.
fn egrand_strings() -> symp::StringSet {
    let lines = egrand::harp_lines();
    let keys: [symp::HarpString; symp::KEYS] = std::array::from_fn(|i| egrand_string(&lines[i]));
    // A string with no coupling is one the harp does not have.
    let none = symp::HarpString { couple: 0.0, own: 0.0, ..keys[0] };
    symp::StringSet { keys, aliquots: [none; symp::ALIQUOTS], send: HARP_SEND_ELECTRIC }
}

/// One of the electric grand's strings as the shared harp takes it: the
/// fields of `egrand::HarpLine` are the harp string's.
fn egrand_string(s: &egrand::HarpLine) -> symp::HarpString {
    symp::HarpString { f0: s.f0, b: s.b, t60: s.t60, t60_hi: s.t60_hi, f_hi: s.f_hi, t60_damped: s.t60_damped, couple: s.couple, own: s.own, damper: s.damper }
}

/// What pays the frame's following of the knobs (`Keys::follow_designs`):
/// the audio that went by, told by whoever saw it go.
#[derive(Clone, Copy, Debug)]
enum Audio {
    /// Frames the frame rendered (`Keys::render`), or that went by while
    /// nothing sounded and the host said so in place of a render
    /// (`Keys::idle`): paid now.
    Went(usize),
    /// The host's every-block visit, a block of this many frames
    /// (`Keys::set_params`). It pays for the visit before it if neither of
    /// the above did (a lane the host does not render at all: parked, or fed
    /// by nothing), by that visit's frames, and never more than its own.
    Visit(usize),
}

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
    /// Sustain pedal, from CC 64: down or up (at 64), what the mechanisms
    /// without a continuous damper, the body and the harp hear.
    pedal: bool,
    /// The pedals as levels, 0..1: CC 64 (the grand's dampers follow it
    /// continuously: half pedalling) and CC 67 (una corda). And the
    /// sostenuto (CC 66): down or up, and which keys it caught — the keys
    /// that were down when it went down keep their dampers up until it comes
    /// up, whatever happens to their keys.
    sustain: f32,
    soft: f32,
    sostenuto: bool,
    sost_keys: [bool; 128],
    /// The grand's pedal noise (the trapwork, heard through the case), one
    /// per instrument like the body; and whether the dampers are thrown, so
    /// the thump comes once per movement and the landing's once per return
    /// (a pedal fluttering about the throw point is one throw).
    pedal_noise: grand_noise::PedalNoise,
    pedal_thrown: bool,
    /// The grand's key release for keys whose voices have finished
    /// (`grand::KeyNoise`), and which keys are down, so that a key-up with
    /// no voice left to sound it still makes its noise, and a stray note-off
    /// none.
    key_noise: grand::KeyNoise,
    key_down: [bool; 128],
    /// The grand's every key solved ahead of its strikes, for both
    /// instruments, at the knobs as they stand (`grand::GrandDesigns`):
    /// built in `new`, brought to a moved knob a key for every 64 frames of
    /// audio, rendered or not, each block paid by its own frames.
    designs: grand::GrandDesigns,
    /// The host's last visit's frames, and whether audio that went by has
    /// paid since it (`Audio`), so the next visit owes nothing for it.
    visit: usize,
    paid: bool,
    /// How many voices are not finished. `render` returns immediately at zero,
    /// which is what makes an unloaded-but-present Keys cost nothing.
    sounding: usize,
    strikes: u32,
    /// The shared body: the soundboard for the piano, the harp of undamped
    /// strings that answers a pedalled note. One per instrument, not one per
    /// voice, because in the real thing there is one soundboard.
    body: string::Body,
    /// The strings nobody struck: the sympathetic harp that answers the
    /// bridge sum under the pedal. One per instrument, like the board,
    /// because there is one harp. See `symp`.
    harp: symp::Harp,
    /// The guitar's shared side: which string each note is on, the open
    /// strings nobody holds, and the air in the body. See `guitar::Fretboard`.
    fretboard: guitar::Fretboard,
    /// The AMP side of the patches that have one — per instrument, not per
    /// voice, because the suitcase has one pair of speakers and the reed
    /// piano one output stage however many keys are down. A shared LFO
    /// phase for the tremolos; a follower and a state-variable filter, one
    /// per channel, for the wah.
    trem_phase: f32,
    /// The suitcase's TREMOLO knob as its depth uses it: through the reed
    /// amp's smoother (`reed_amp::ReedAmp::smooth`), whether a value has
    /// come since the last patch change (the first is taken as it is, so a
    /// steady knob is exactly the knob), and the one-pole's coefficient. The
    /// depth was applied as the knob stood, and a jump under a held note was
    /// a step (38 times the steady second difference; review).
    trem_knob: f32,
    trem_knob_set: bool,
    trem_knob_a: f32,
    /// The reed piano's amp: the dry patch's throb, and the wet patch's
    /// slap, cabinet, throb and room. One per instrument, like the board.
    reed_amp: reed_amp::ReedAmp,
    wah: clav_wah::Wah,
    /// One block of the block-rendered voices, summed: the two channels and
    /// the bridge drive, and a scratch the fading corpses render into before
    /// their fade is applied. Sized in `new`, `modal::BLOCK` frames each.
    acc_l: Vec<f32>,
    acc_r: Vec<f32>,
    acc_d: Vec<f32>,
    tmp_l: Vec<f32>,
    tmp_r: Vec<f32>,
    tmp_d: Vec<f32>,
    /// The harp's return for the same block, rendered once from `acc_d`.
    acc_h: Vec<f32>,
    /// **The hush**: all notes off's fade of the shared side (the board, the
    /// harp, the guitar's open strings and air, the action's noises) over
    /// `HUSH_FADE`, which is then cleared under the silence, where it used to
    /// be cleared in one sample (6.4 times the steady second difference on
    /// a pedalled Piano 1 chord; review), and faded back in over another
    /// `HUSH_FADE`: the voices it choked still sound, and drive it again from
    /// the clear, and heard at once at full gain they stepped it (the
    /// guitar's air 4 to 9 times the steady second difference ten
    /// milliseconds after the stop; verifier). Frames of the fade left of
    /// `hush_total`, whether it has run out with the clear still to make,
    /// frames of the rise left, the gain of either for each frame of the
    /// block being rendered, and whether the last recount counted the hush
    /// as sounding (what all notes off takes out of `sounding` to see
    /// whether anything else is).
    hush_left: u32,
    hush_total: u32,
    hush_due: bool,
    hush_rise: u32,
    hush_g: Vec<f32>,
    hush_counted: bool,
    /// **A repeated all notes off holds the shared side out.** Set when its
    /// fade was begun by an all notes off with nothing struck since the last
    /// one (`stopped`): at the fade's end the clear waits, the shared side
    /// heard at nothing and fed nothing, until the voices the first one
    /// stopped are done or a key is struck (`note_on`), and then the clear
    /// and the rise come as after any fade.
    hush_held: bool,
    /// Whether every key has been let up and every voice stopped by an all
    /// notes off since the last strike, so that the next has nothing of its
    /// own to do.
    stopped: bool,
    /// The strike count when the hush stopped the keys or its fade ended: a
    /// voice struck since is a new note, not one the hush is fading, and
    /// drives the shared side at full (`render_blocks`).
    ///
    /// **A key struck during the fade itself loses its shared side.** In
    /// the ten milliseconds between the stop and the clear its harp strings
    /// and its soundboard are the old wash's, faded and cleared with it, and
    /// it comes back with the rise, its drive faded in with the choked
    /// voices': struck 5 ms into the fade, over its first 60 ms it is -28.3
    /// / -28.0 / -31.0 dB off a strike from silence on Piano 1 / Piano 2 /
    /// the electric grand (4122d312, cleared and not faded back in: -29.7 /
    /// -30.2 / -31.2; 787cdb89, which cleared at the stop and stepped there,
    /// -37.7 / -40.2 / -36.2 struck with the stop, verifier finding 3).
    /// The later in the fade, the more it loses against 4122d312, which lost
    /// only the rest of the fade: struck 479 frames in, -31.0 / -31.4 / -33.6
    /// dB here against -37.7 / -40.4 / -37.1 there. That take stepped where
    /// this one does not: up to 3.7 times a fresh strike's second difference
    /// on Piano 1 (this rule: 1.05 at most; `FADE_STRUCK_STEP`).
    /// Driving it at full from the clear gives 1.3 to 2.5 dB of that back
    /// (-30.5 / -30.6 / -32.2, this rule -28.3 / -28.1 / -30.9 on the same
    /// takes), but its drive then starts at the clear in mid-swing and the
    /// network plays that edge back a line later: 1.40 times a fresh
    /// strike's second difference at the same age 33 ms after the clear, on
    /// Piano 1 struck 8 ms in, where the faded drive reads 1.01.
    /// Keeping it whole would take a second shared side to ring the old
    /// wash out in for the length of the fade, two harps' strings and two
    /// soundboards; the case is a key played within ten milliseconds of a
    /// transport stop or locate while keys are held, and it is decided
    /// here. A key struck on the frame of the clear or after it keeps its
    /// shared side (`a_key_struck_in_the_rise_keeps_its_shared_side`).
    hush_from: u32,
    /// The harp's string sets for the patches that have one, designed here
    /// once (`symp::Strung`, a solve per string): `set_patch` loads one,
    /// which is a copy, where stringing it cost up to 0.66 ms of the
    /// callback (the harp lane's verifier, finding 8). Piano 1's, Piano 2's, then
    /// the electric grand's. Each follows the STRETCH knob as its voices do
    /// (`follow_harp`), so the set a patch change loads is strung where
    /// that patch's knob left it.
    harp_sets: [symp::Strung; 3],
    /// What a string re-strung for another stretch is tuned from: the two
    /// grands' stretched tunings and Bs (`grand::tuning`), the electric
    /// grand's tuning (`egrand::harp_tune`). The audio the strings may still
    /// spend on following, frames, and what one string costs of it
    /// (`symp::RESTRING_SECS`).
    harp_tune: [([f32; symp::KEYS], [f32; symp::KEYS]); 2],
    egrand_tune: [f32; symp::KEYS],
    harp_credit: usize,
    harp_per: usize,
    /// Frames to the harp's next payment on its grid, 0 when one is due
    /// before the next frame. STRETCH re-strings the strings a block's
    /// frames' worth at a time at every `modal::BLOCK` frames of the
    /// timeline, rendered or told of (`render`, `idle`), and not a call's
    /// worth at each call: a string re-strung under a ringing chord glides
    /// from the sample it is re-strung on, and paid by the call that sample
    /// moved with the host's call size (up to 896 frames early at
    /// 1024-frame calls: the electric grand -65 dB re peak off the
    /// 128-frame take, `every_patch_renders_the_same_at_any_host_call`).
    harp_due: usize,
    /// How many times the shared side has been cleared (`clear_shared`):
    /// what the tests hold a repeated all notes off's cost to.
    #[cfg(test)]
    shared_clears: u32,
}

impl Keys {
    /// Build the instrument. **Allocates**, and is called from the engine's
    /// constructor rather than from the callback.
    pub fn new(sample_rate: f32) -> Self {
        let rate = if sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        let harp = symp::Harp::new(rate);
        let harp_sets = [
            harp.strung(&symp::StringSet::grand(&grand::GrandBase::INTIMATE, HARP_SEND_INTIMATE, &symp::GrandHarp::INTIMATE)),
            harp.strung(&symp::StringSet::grand(&grand::GrandBase::CONCERT, HARP_SEND_CONCERT, &symp::GrandHarp::CONCERT)),
            harp.strung(&egrand_strings()),
        ];
        let mut keys = Keys {
            notes: Vec::new(),
            corpses: Vec::new(),
            patch: Patch::default(),
            rate,
            params: Patch::default().defaults(),
            pedal: false,
            sustain: 0.0,
            soft: 0.0,
            sostenuto: false,
            sost_keys: [false; 128],
            pedal_noise: grand_noise::PedalNoise::new(rate),
            pedal_thrown: false,
            key_noise: grand::KeyNoise::new(rate),
            key_down: [false; 128],
            designs: grand::GrandDesigns::new(rate),
            visit: 0,
            paid: true,
            sounding: 0,
            strikes: 0,
            body: string::Body::new(rate),
            harp,
            fretboard: guitar::Fretboard::new(rate),
            trem_phase: 0.0,
            trem_knob: 0.0,
            trem_knob_set: false,
            trem_knob_a: 1.0 - (-1.0 / (reed_amp::KNOB_SMOOTH_S * rate)).exp(),
            reed_amp: reed_amp::ReedAmp::new(rate),
            wah: clav_wah::Wah::new(rate),
            acc_l: vec![0.0; modal::BLOCK],
            acc_r: vec![0.0; modal::BLOCK],
            acc_d: vec![0.0; modal::BLOCK],
            tmp_l: vec![0.0; modal::BLOCK],
            tmp_r: vec![0.0; modal::BLOCK],
            tmp_d: vec![0.0; modal::BLOCK],
            acc_h: vec![0.0; modal::BLOCK],
            hush_left: 0,
            hush_total: ((HUSH_FADE * rate) as u32).max(1),
            hush_due: false,
            hush_rise: 0,
            hush_g: vec![1.0; modal::BLOCK],
            hush_counted: false,
            hush_held: false,
            stopped: false,
            hush_from: 0,
            harp_sets,
            harp_tune: [grand::tuning(&grand::GrandBase::INTIMATE), grand::tuning(&grand::GrandBase::CONCERT)],
            egrand_tune: egrand::harp_tune(),
            harp_credit: 0,
            harp_per: ((rate * symp::RESTRING_SECS).round() as usize).max(1),
            harp_due: 0,
            #[cfg(test)]
            shared_clears: 0,
        };
        keys.fill_pool();
        keys.string_harp();
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
                age: 0,
                cut: false,
            });
        }
        self.corpses.clear();
        self.corpses.reserve_exact(CORPSES);
        for _ in 0..CORPSES {
            let mut mech = Box::new(Mechanism::new(self.rate));
            mech.set_kind(self.patch);
            self.corpses.push(Corpse {
                mech,
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
        // The harp is strung for the patch's instrument and starts silent;
        // its pedal is re-synced because the electric patches did not
        // forward the pedal while they were loaded.
        self.harp.clear();
        self.pedal_noise.clear();
        self.key_noise.clear();
        self.key_down = [false; 128];
        // And the keys the sostenuto caught: their voices are gone, and a
        // catch that outlived them held the new instrument's next note of
        // the same key (review).
        self.sost_keys = [false; 128];
        self.string_harp();
        if self.has_harp() {
            self.harp.set_sustain(self.harp_sustain(self.sustain));
        }
        self.fretboard.clear();
        self.fretboard.set_pedal(self.pedal);
        // Everything the hush would have faded is cleared already.
        self.hush_left = 0;
        self.hush_due = false;
        self.hush_rise = 0;
        self.hush_counted = false;
        self.hush_held = false;
        self.stopped = false;
        self.trem_phase = 0.0;
        self.trem_knob_set = false;
        self.reed_amp.reset();
        self.wah.reset();
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
        // **A knob that is not a number is the patch's default.** `clamp`
        // passes NaN straight through, and from here it reaches every strike,
        // the reed's spread by the frame's pan law among them, which
        // multiplies by WIDTH (`WurliKnobs::width`); the piano's
        // BODY knob reaches the output through `body_amt` with nothing in a
        // voice to catch it.
        let default = self.patch.defaults();
        if let Some(slot) = self.params.get_mut(index) {
            *slot = if value.is_nan() { default[index] } else { value.clamp(0.0, 1.0) };
        }
    }

    /// Every knob at once, in the order the patch lists them, from the
    /// host's every-block walk, and the `frames` of the block it is visiting.
    /// Shorter slices leave the rest alone, which is what a build reading a
    /// file written by an older one hands over.
    ///
    /// The host renders a silent instrument nothing, and tells it `idle`
    /// instead where it renders the lane at all; a lane it leaves out of the
    /// render altogether (parked, or fed by nothing) is told only this. Its
    /// visit then pays the knobs' following (`follow_designs`) for the one
    /// before it, which nothing else paid, by that visit's frames: so a knob
    /// turned in a long silence still follows, and no call pays more than
    /// its own frames. The one block that pays more is the first a lane
    /// renders after visits alone (armed, or the transport started): its
    /// visit pays the visit before it and its render its own span, twice its
    /// frames once (verifier: 515 to 530 µs of a 512-frame block, 5 %).
    pub fn set_params(&mut self, values: &[f32], frames: usize) {
        for (i, v) in values.iter().enumerate() {
            self.set_param(i, *v);
        }
        self.follow_designs(Audio::Visit(frames));
    }

    /// **`frames` went by, and nothing sounded**: what the host says in place
    /// of `render` while `active` is false. Real-time safe, and nothing moves
    /// but what follows the knobs (the grand's table, `follow_designs`, and
    /// the harp's strings, `pass_harp_grid`), which they pay as a render's
    /// frames do: here, on the lane's own render, rather than at the host's
    /// next visit, which is its serial walk over every lane.
    pub fn idle(&mut self, frames: usize) {
        self.follow_designs(Audio::Went(frames));
        self.pass_harp_grid(frames);
    }

    /// What the knobs make of the strings, brought to them as they move and
    /// paid for by the audio (`Audio`): by the frames `render` renders and
    /// `idle` is told of, and at `set_params` for a visit neither paid. The
    /// grand's solved strings (`grand::GrandDesigns::follow`), at most a
    /// call's frames' worth; the harp's on a grid of their own
    /// (`pass_harp_grid`), and here only for a visit, which has no audio.
    fn follow_designs(&mut self, audio: Audio) {
        let frames = match audio {
            Audio::Went(n) => {
                self.paid = true;
                n
            }
            Audio::Visit(n) => {
                // The visit before this one, which nothing rendered and
                // nothing said went by: the frames it spanned, held to this
                // block's own when the host's blocks have shrunk since.
                let owed = if self.paid { 0 } else { self.visit.min(n) };
                (self.visit, self.paid) = (n, false);
                owed
            }
        };
        let base = match self.patch {
            Patch::Piano1 => Some(&grand::GrandBase::INTIMATE),
            Patch::Piano2 => Some(&grand::GrandBase::CONCERT),
            _ => None,
        };
        if let Some(base) = base {
            self.designs.follow(base, &PianoKnobs::from_params(&self.params), frames);
        }
        // A visit nothing rendered: no sample went by for a string to be
        // re-strung on, so the strings take its frames at once.
        if let Audio::Visit(_) = audio {
            self.follow_harp(frames);
        }
    }

    /// The harp's strings brought `frames` of audio nearer STRETCH, which
    /// tunes them as it tunes the voices they shadow: the patch's set
    /// (`symp::Strung::follow`), re-strung in the harp that has it loaded.
    fn follow_harp(&mut self, frames: usize) {
        let set = match self.patch {
            Patch::Piano1 => 0,
            Patch::Piano2 => 1,
            Patch::ElectricGrand => 2,
            _ => return,
        };
        let stretch = if set == 2 { egrand::EGrandKnobs::from_params(&self.params).stretch } else { PianoKnobs::from_params(&self.params).stretch };
        self.harp_credit = self.harp_credit.saturating_add(frames);
        let string_at = |i: usize, s: f32| match set {
            0 => symp::grand_string(&grand::GrandBase::INTIMATE, &symp::GrandHarp::INTIMATE, &self.harp_tune[0], i, s),
            1 => symp::grand_string(&grand::GrandBase::CONCERT, &symp::GrandHarp::CONCERT, &self.harp_tune[1], i, s),
            // The 88: its aliquots' loops have no coupling and are never
            // re-strung.
            _ => egrand_string(&egrand::harp_line(&self.egrand_tune, i.min(symp::KEYS - 1), s)),
        };
        self.harp_sets[set].follow(stretch, &mut self.harp_credit, self.harp_per, Some(&mut self.harp), string_at);
    }

    /// `frames` of the timeline with none of the harp's samples rendered in
    /// them (the instrument silent, or `idle`): the payments its grid has in
    /// them (`harp_due`), at once, since no string sounds to be re-strung on
    /// one sample of them rather than another.
    fn pass_harp_grid(&mut self, frames: usize) {
        let mut left = frames;
        while left > 0 {
            if self.harp_due == 0 {
                self.follow_harp(modal::BLOCK);
                self.harp_due = modal::BLOCK;
            }
            let s = left.min(self.harp_due);
            self.harp_due -= s;
            left -= s;
        }
    }

    /// The wah patch with its pedal out of circuit: a bench hook, so the
    /// wah can be measured as an effect against the same events rendered
    /// with it in. The app never calls it.
    #[allow(dead_code)]
    pub fn set_wah_bypass(&mut self, on: bool) {
        self.wah.set_bypass(on);
    }

    /// The knobs as they stand.
    ///
    /// The desk pushes knobs down through `set_param`; reading them back is
    /// what the panel's Initialise test does, so the shipped path never
    /// asks. Same for `reset_params` below.
    #[allow(dead_code)]
    pub fn params(&self) -> &[f32; MAX_PARAMS] {
        &self.params
    }

    /// **Initialise the sound**: every knob back to the value the bench
    /// signed off for the patch that is loaded. What the panel's button does.
    #[allow(dead_code)]
    pub fn reset_params(&mut self) {
        self.params = self.patch.defaults();
    }

    /// Strike a key.
    ///
    /// `velocity` is MIDI velocity over 127, so 0.0 to 1.0 — the same units
    /// the DX7 takes and the same units `instrument.rs` hands out.
    pub fn note_on(&mut self, pitch: u8, velocity: f32) {
        if pitch > 127 {
            return;
        }
        // **The patches with a harp have its eighty-eight keys** and no
        // others, and a key they do not have plays nothing. Each voice
        // clamped one onto its edge key while the frame and the harp took it
        // as it came: MIDI 15 rang A0 with A0's string asleep, and beside a
        // struck A0 it was a second voice on that one string (review).
        // Dropped rather than clamped: a clamped key would share A0's voice
        // and string, and the two keys' note-offs would have to agree on
        // which of them lets it go.
        if self.has_harp() && !HARP_KEYS.contains(&pitch) {
            return;
        }
        // **A velocity that is not a number is the softest touch**, a silent
        // press on every patch. `clamp` passes NaN straight through, and the
        // tine pianos made of it a voice whose level was NaN, which never
        // finished and held the instrument awake for good (review).
        let velocity = if velocity.is_nan() { 0.0 } else { velocity.clamp(0.0, 1.0) };
        // **The guitar decides the string before the voice.** One note per
        // string: the selector picks the string (`guitar::Fretboard::place`),
        // and a key off the fingerboard is a string of its own.
        let place = (self.patch.family() == Family::Guitar).then(|| self.fretboard.place(pitch));
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
                let lands = match place {
                    Some(pl) => self.notes[s].mech.lands_guitar(pl, pitch, &self.params),
                    None => self.notes[s].mech.lands(pitch, &self.params),
                };
                if !self.notes[s].mech.finished() && !lands {
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
        // **The slot's last string goes back to the harp when another key
        // takes the slot.** The end-of-block recount hands back a voice that
        // ends inside a block, but a slot can be handed out again before any
        // recount sees it: a steal (its voice now fading as a corpse), or a
        // voice that finished at its own strike (a silent press). Three
        // silent presses in one block once took one slot in turn, and only
        // the last key's string ever rang; a stolen pitch's string stayed on
        // its halo's loss and coupling under the pedal for as long as its key
        // was free (the harp lane's verifier, finding 12). The guitar's string goes
        // back to the fretboard the same way (a stolen or finished voice's
        // string is free and open).
        if let Some(old) = self.notes[slot].pitch {
            if old != pitch && self.has_harp() {
                self.harp.voice_done(old);
            }
            if old != pitch && place.is_some() {
                self.fretboard.voice_done(old);
            }
        }
        self.strikes = self.strikes.wrapping_add(1);
        let age = self.strikes;
        // A strike: the next all notes off has a key to stop, and a hush
        // holding the shared side out for a repeated one lets it back in.
        self.stopped = false;
        self.hush_held = false;
        self.key_down[usize::from(pitch)] = true;
        let note = &mut self.notes[slot];
        let was_done = note.pitch.is_none() || note.mech.finished();
        note.pitch = Some(pitch);
        note.held = true;
        note.age = age;
        note.cut = false;
        note.mech.set_soft(self.soft);
        match place {
            Some(pl) => note.mech.strike_guitar(pl, pitch, velocity, &self.params),
            None => note.mech.strike(pitch, velocity, &self.params, &self.designs),
        }
        if was_done {
            self.sounding += 1;
        }
        // Whatever else was on that string stops: the finger is on it now.
        // After the newcomer's strike, so a chord of more keys than strings
        // chokes notes still in their contact phase, which then never pluck
        // (`guitar::GuitarVoice::choke`): only the last note on each string
        // sounds.
        if let Some(pl) = place {
            if let Some(old) = self.fretboard.take(pl, pitch) {
                for n in &mut self.notes {
                    if n.pitch == Some(old) {
                        n.mech.choke();
                    }
                }
            }
        }
        if self.has_harp() {
            self.harp.key_down(pitch);
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
        // A key the sostenuto caught keeps its damper up: released as if the
        // sustain pedal were down, until the sostenuto comes up.
        let caught = self.sost_keys.get(usize::from(pitch)).copied().unwrap_or(false);
        let sustain = if caught { 1.0 } else { self.sustain };
        let was_down = self.key_down.get_mut(usize::from(pitch)).map(|d| std::mem::replace(d, false)).unwrap_or(false);
        let mut sounded = false;
        for n in &mut self.notes {
            if n.pitch == Some(pitch) && n.held {
                n.held = false;
                n.mech.release_at(sustain);
                // **Asked after the key-up, not before.** The key-up ends the
                // grand's control block on its frame, and the block's end may
                // find the voice finished there: a soft note that died in the
                // block, or a silent press. Asked first, the frame left the
                // key's release to a voice that could no longer ring it, and
                // neither rang it: a soft treble key let up in the last 3 to
                // 125 frames before its voice would have finished under the
                // finger, or a silent press let up within a block of it, came
                // up without its release (verifier,
                // `a_key_comes_up_with_its_noise_wherever_its_voice_ends`).
                sounded |= !n.mech.finished();
            }
        }
        // The key's own noise on the way up, which the grand's voice rings
        // itself — unless it has finished while the key was held (a soft
        // treble note) or at the key-up, and then the frame does
        // (`grand::KeyNoise`).
        // The two grands only: the electric grand has a harp but no key noise
        // of its own yet (`egrand`'s release-noise hook), and must not borrow
        // the acoustic action's.
        if was_down && !sounded && matches!(self.patch, Patch::Piano1 | Patch::Piano2) {
            let base = if self.patch == Patch::Piano2 { &grand::GrandBase::CONCERT } else { &grand::GrandBase::INTIMATE };
            self.key_noise.key_up(pitch, &PianoKnobs::from_params(&self.params), base);
            self.sounding = self.sounding.max(1);
        }
        // **The strings hear the KEY**, and a key that was not down has
        // nothing to let go of: a stray or doubled note-off reached the harp
        // and re-armed a string's felt (review). A caught key's string is
        // let go to the sustain when the sostenuto comes up (`set_pedals`),
        // and until then each side keeps it as what it is to that side: the
        // harp's leaves its halo and keeps its damper up (`key_up_held`); a
        // guitar string has no halo, and to the fretboard a string the
        // sostenuto holds is one a key holds, so it hears nothing yet. Told
        // the key was up, it gave the string to the next note, which choked
        // the caught one; and marked as the sustain's instead, the string
        // went free when the sustain lifted under the sostenuto.
        if !was_down {
            return;
        }
        if self.has_harp() {
            if caught {
                self.harp.key_up_held(pitch);
            } else {
                self.harp.key_up(pitch);
            }
        }
        if self.patch.family() == Family::Guitar && !caught {
            self.fretboard.key_up(pitch);
        }
    }

    /// String the harp for the patch that is loaded: each modal grand from
    /// its own scale and stretch (`symp::StringSet::grand`), at its own
    /// send and voicing (`symp::GrandHarp`), the intimate with aliquots over
    /// its top courses. Designed in `new` and re-strung as each patch's
    /// STRETCH moves (`harp_sets`), loaded here: a copy of coefficients,
    /// real-time safe, as `set_patch` must be.
    fn string_harp(&mut self) {
        match self.patch {
            Patch::Piano1 => self.harp.load(&self.harp_sets[0]),
            Patch::Piano2 => self.harp.load(&self.harp_sets[1]),
            Patch::ElectricGrand => self.harp.load(&self.harp_sets[2]),
            _ => {}
        }
    }

    /// What the harp's dampers hear of the sustain pedal (CC 64 as 0..1): on
    /// the two grands its level, which their strings' dampers follow (half
    /// pedalling); on the electric grand the switch at 64, all its strings'
    /// dampers know (`Mechanism::release_at`), so that a string and its
    /// sympathetic copy are damped by one rule. Its harp followed the level
    /// while its strings followed the switch: at CC 30 to 63 the struck
    /// string was damped and its copy and the wash rang up to 29 dB over the
    /// damped tail (review).
    fn harp_sustain(&self, sustain: f32) -> f32 {
        match self.patch {
            Patch::ElectricGrand => f32::from(u8::from(sustain >= 0.5)),
            _ => sustain,
        }
    }

    /// Only the pianos have undamped strings to answer the bridge. The
    /// electric patches' harp stays cleared and is never asked anything,
    /// so a loop it woke could never be left ticking with nobody to tick
    /// it, holding the instrument awake.
    fn has_harp(&self) -> bool {
        matches!(self.patch, Patch::Piano1 | Patch::Piano2 | Patch::ElectricGrand)
    }

    /// Whether the patch has a shared side for the hush to fade: the grands'
    /// soundboard, harp and action noises, the electric grand's harp, the
    /// guitar's open strings and air. The other patches' voices are the
    /// whole of what all notes off stops (their amp is not the hush's).
    fn has_shared(&self) -> bool {
        self.has_harp() || self.patch.family() == Family::Guitar
    }

    /// The sustain pedal, CC 64, as a switch: `set_pedals` with the pedal
    /// all the way down or up and the other two pedals as they are.
    #[allow(dead_code)] // keysrig's scenes and the frame tests switch the sustain; the app hands set_pedals all three levels.
    pub fn set_pedal(&mut self, down: bool) {
        let s = if down { 127 } else { 0 };
        let sost = if self.sostenuto { 127 } else { 0 };
        let soft = (self.soft * 127.0).round() as u8;
        self.set_pedals([s, sost, soft]);
    }

    /// The three pedals, as the controllers 64 (sustain), 66 (sostenuto) and
    /// 67 (soft) carry them, 0..127 — what `instrument.rs` reads every block
    /// (`pedal_now`), so a repeated level is a compare and nothing else.
    ///
    /// The sustain is a LEVEL: the grand's dampers follow it part of the
    /// way (`grand::GrandBase::pedal_lift`), which is half pedalling. Lifting
    /// it past 64 damps everything whose key is already up — and only that.
    /// Notes still held by a finger keep ringing, which is the difference
    /// between a pedal and a panic button and the thing that is missing from
    /// every model where the pedal "does not seem to do anything". The
    /// sostenuto catches the keys that are down as it goes down and holds
    /// their dampers up until it comes up; the soft pedal shifts the grand's
    /// action for the strikes to come.
    pub fn set_pedals(&mut self, cc: [u8; 3]) {
        let sustain = f32::from(cc[0].min(127)) / 127.0;
        let sost = cc[1] >= 64;
        let soft = f32::from(cc[2].min(127)) / 127.0;
        if soft != self.soft {
            self.soft = soft;
        }
        if sost != self.sostenuto {
            self.sostenuto = sost;
            if sost {
                // **The keys that are down, sounding or not.** A silent
                // press holds its damper off as a struck key does, and a
                // held key's note may have died under the finger long before
                // the pedal went down; the catch read the voices and caught
                // neither (review).
                self.sost_keys = self.key_down;
            } else {
                // The caught keys whose keys are up now fall to whatever the
                // sustain pedal says, and the KEY says whether it is up: a
                // voice may have died under a finger that is still down, and
                // asking the voices damped that key's string under it.
                let caught = std::mem::replace(&mut self.sost_keys, [false; 128]);
                for n in &mut self.notes {
                    if let Some(p) = n.pitch {
                        if caught[usize::from(p)] && !self.key_down[usize::from(p)] && !n.cut {
                            n.mech.set_sustain(self.sustain, false);
                            if !self.pedal && !n.mech.follows_sustain() {
                                n.mech.damp();
                            }
                        }
                    }
                }
                // Whatever the sustain: the harp knows its own pressure, and
                // with the pedal down `key_up` leaves the string free (a caught
                // key let go under the sustain would otherwise stay held in
                // the harp for good); the fretboard's `key_up` hands the
                // string to the sustain as a key-up under it would.
                let guitar = self.patch.family() == Family::Guitar;
                for (p, c) in caught.iter().enumerate() {
                    if *c && !self.key_down[p] {
                        if self.has_harp() {
                            self.harp.key_up(p as u8);
                        }
                        if guitar {
                            self.fretboard.key_up(p as u8);
                        }
                    }
                }
            }
        }
        if sustain == self.sustain {
            return;
        }
        let was = self.sustain;
        self.sustain = sustain;
        // The harp's dampers follow the level too (`symp::Harp::set_sustain`),
        // or the switch, as the strings' own do (`harp_sustain`).
        if self.has_harp() {
            self.harp.set_sustain(self.harp_sustain(sustain));
        }
        // The grand's dampers follow the level; a caught key's stays up, and
        // a cut voice's stays down (`Note::cut`).
        for n in &mut self.notes {
            if let Some(p) = n.pitch {
                if n.cut {
                    continue;
                }
                let held = n.held || self.sost_keys[usize::from(p)];
                n.mech.set_sustain(sustain, held);
            }
        }
        // The trapwork's thump, when the dampers are thrown off the strings
        // and when they land (the intimate grand's: `GrandBase::pedal_noise`).
        // Rung on Piano 1 only, and followed on every patch: the throw is
        // the pedal's, and one followed only on Piano 1 came back from
        // another patch stale, losing the next throw or landing (review).
        let [lo, hi, _] = grand::GrandBase::INTIMATE.pedal_lift;
        let rings = self.patch == Patch::Piano1;
        let gain = grand::GrandBase::INTIMATE.gain * 10.0_f32.powf(grand::GrandBase::INTIMATE.pedal_noise / 20.0);
        if !self.pedal_thrown && was < hi && sustain >= hi {
            self.pedal_thrown = true;
            if rings {
                self.pedal_noise.ring(gain);
                self.sounding = self.sounding.max(1);
            }
        } else if self.pedal_thrown && was > lo && sustain <= lo {
            self.pedal_thrown = false;
            if rings {
                self.pedal_noise.ring(gain * 10.0_f32.powf(-1.3 / 20.0));
                self.sounding = self.sounding.max(1);
            }
        }
        let down = sustain >= 0.5;
        if down == self.pedal {
            return;
        }
        self.pedal = down;
        if !down {
            // The grand's dampers are already where this level puts them
            // (`set_sustain` above): half way down they bear lightly, and a
            // `damp` here would land them with their full weight — which was
            // half pedalling that depended on how the pedal got there
            // (verifier: C4 let go under the full pedal, then one CC message
            // to 40, fell at -586 dB/s where a key let go at 40 falls at
            // -178).
            for n in &mut self.notes {
                let caught = n.pitch.is_some_and(|p| self.sost_keys[usize::from(p)]);
                if !n.held && !caught && !n.mech.follows_sustain() {
                    n.mech.damp();
                }
            }
        }
        self.body.set_pedal(down);
        self.fretboard.set_pedal(down);
    }

    /// Everything off, immediately. Not a pedal lift: this stops held keys
    /// too, each the way its instrument stops a note for good (`choke`: a
    /// guitar's string by the next finger, which drops a pluck still in its
    /// finger's contact; every other mechanism by its damper), and the keys
    /// the sostenuto caught are up with the rest. The harp and the fretboard
    /// let every key up, the fretboard keeping who is on which string until
    /// the choked voices are done, and the shared side's sound fades out, is
    /// cleared and fades back in (`hush_left`).
    ///
    /// **A fade already running runs on.** The host's force-quiet calls this
    /// every block for as long as the instrument is active, and a fade that
    /// started over at every call would hold the shared side at full level
    /// for as long as the calls kept coming. One that comes during the rise
    /// fades out from wherever the rise has reached. With nothing but the
    /// hush itself sounding there is nothing to fade, and what little is
    /// left is cleared at once, as it always was: counted, the hush kept
    /// the force-quiet's next call fading it again, and the rise kept the
    /// instrument awake for good.
    ///
    /// **A repeat costs nothing more than the first.** With nothing struck
    /// since the last all notes off there is no key to let up and no voice
    /// to stop, and a fade a repeat begins holds the shared side out at its
    /// end until the voices are done (`hush_held`), where it was cleared and
    /// faded back in to be turned by the next call: the force-quiet cleared
    /// the shared side about every second call. With it stopping the
    /// pedalled six-note chord at 1.2 s, a 3 s take at 1-frame calls cost
    /// the Reed Amp 180 ms of CPU, Piano 2 337 and the electric grand 269,
    /// where one all notes off costs 44, 256 and 131; it costs 43, 261 and
    /// 132 now. A patch with no shared side runs no hush at all
    /// (`has_shared`).
    pub fn all_notes_off(&mut self) {
        // With nothing struck since the last all notes off, every key is up
        // and every voice stopped already. Choked again, a guitar voice ended
        // its control block wherever the host's call fell
        // (`guitar::GuitarVoice::choke`).
        let repeat = self.stopped;
        if !repeat {
            for n in &mut self.notes {
                n.held = false;
                n.cut = true;
                n.mech.choke();
            }
            self.harp.release_all();
            self.fretboard.release_all();
            self.key_down = [false; 128];
            self.sost_keys = [false; 128];
            self.stopped = true;
        }
        // Only a patch with a shared side has anything for the hush to fade.
        if !self.has_shared() {
            return;
        }
        if self.sounding.saturating_sub(usize::from(self.hush_counted)) == 0 {
            self.clear_shared();
            self.hush_left = 0;
            self.hush_due = false;
            self.hush_rise = 0;
            self.hush_held = false;
        } else if self.hush_left == 0 && !self.hush_due {
            // The fade's gain at `hush_left` frames to go is the rise's at as
            // many frames done, so the turn has no step.
            self.hush_left = self.hush_total - self.hush_rise.min(self.hush_total);
            self.hush_rise = 0;
            self.hush_due = self.hush_left == 0;
            self.hush_from = self.strikes;
            self.hush_held = repeat;
        }
    }

    /// The shared side's sound cleared, what the keys left of the strings
    /// kept: the end of the hush.
    fn clear_shared(&mut self) {
        #[cfg(test)]
        {
            self.shared_clears += 1;
        }
        self.body.reset();
        self.harp.hush();
        self.fretboard.hush();
        self.pedal_noise.clear();
        self.key_noise.clear();
    }

    /// Every voice for the next `m` frames (at most `modal::BLOCK`): every
    /// sounding note straight into the accumulators, every fading corpse into
    /// the scratch first and then in under its smoothstep fade, one sample of
    /// fade per frame. Corpses still drive the body: a fading string is a
    /// string.
    ///
    /// Under the hush it also fills `hush_g` with the fade's or the rise's
    /// gain for each frame (nothing, held out: `hush_held`) and fades by it
    /// the shared side it renders here (the guitar's open strings and air,
    /// the key noise) and the bridge drive the harp and the soundboard hear;
    /// `render` fades what they give back, and the return says whether to.
    fn render_blocks(&mut self, m: usize) -> bool {
        // The hush ran out in the last block: what it faded is silent now,
        // and is cleared before any of it can be heard again; and faded back
        // in from there, since the voices it choked drive it from the clear.
        // Held out for a repeated all notes off (`hush_held`), it waits for
        // the clear until the voices it stopped are done (`render`).
        if self.hush_due && !self.hush_held {
            self.hush_due = false;
            self.clear_shared();
            self.hush_rise = self.hush_total;
        }
        let hushed = self.hush_left > 0 || self.hush_rise > 0 || self.hush_due;
        if self.hush_left > 0 {
            for (k, g) in self.hush_g[..m].iter_mut().enumerate() {
                let t = self.hush_left.saturating_sub(k as u32) as f32 / self.hush_total as f32;
                *g = t * t * (3.0 - 2.0 * t);
            }
            self.hush_left = self.hush_left.saturating_sub(m as u32);
            self.hush_due = self.hush_left == 0;
        } else if self.hush_rise > 0 {
            // The fade's curve run back: frame k of the rise at k / total,
            // and one, exactly, from the end of the rise.
            for (k, g) in self.hush_g[..m].iter_mut().enumerate() {
                let t = 1.0 - self.hush_rise.saturating_sub(k as u32) as f32 / self.hush_total as f32;
                *g = t * t * (3.0 - 2.0 * t);
            }
            self.hush_rise = self.hush_rise.saturating_sub(m as u32);
        } else if self.hush_due {
            self.hush_g[..m].fill(0.0);
        }
        self.acc_l[..m].fill(0.0);
        self.acc_r[..m].fill(0.0);
        self.acc_d[..m].fill(0.0);
        // The guitar's voices drive their OWN string's bus, so an open
        // string is never driven by the note it is (`guitar::Fretboard`).
        let guitar = self.patch.family() == Family::Guitar;
        if guitar {
            self.fretboard.set_knobs(&guitar::GuitarKnobs::from_params(&self.params));
            self.fretboard.begin(m);
        }
        // Under the hush a key struck since it stopped the keys, or since its
        // fade ended, is rendered after the rest (`hush_from`), so that the
        // drive faded below is only what the hush is fading.
        let from = self.hush_from;
        let struck_since = |n: &Note| hushed && (n.age.wrapping_sub(from) as i32) > 0;
        for note in &mut self.notes {
            if note.pitch.is_none() {
                continue;
            }
            if note.mech.finished() {
                // **A voice that ended at an event hands its string back
                // here.** A key let up, the pedal lifted or all notes off
                // closes the voice's control block where it falls, and the
                // cull at the cut can empty its bank: the voice is done before
                // any block renders it, so the end the render below reports is
                // never seen. Its string stayed owned by nobody until another
                // key took it, and the next of its pitch went back on it
                // (`a_voice_that_ends_at_an_event_gives_its_string_back`).
                // The block after such an event starts on its frame at every
                // host call; for a voice that ended in an earlier block of
                // this call the string is free already, and this finds no
                // owner.
                if let (true, Some(p)) = (guitar, note.pitch) {
                    self.fretboard.voice_ended(p, 0);
                }
                continue;
            }
            if struck_since(note) {
                continue;
            }
            if guitar {
                // A voice that ends in this block hands its string back from
                // the frame it ended on, on the fretboard's own grid.
                let s = note.mech.guitar.bus();
                let sounded = note.mech.guitar.render(&mut self.acc_l[..m], &mut self.acc_r[..m], self.fretboard.bus(s, m));
                if let (true, Some(p)) = (note.mech.guitar.finished(), note.pitch) {
                    self.fretboard.voice_ended(p, sounded);
                }
            } else {
                note.mech.render_block(&mut self.acc_l[..m], &mut self.acc_r[..m], &mut self.acc_d[..m]);
            }
        }
        for c in &mut self.corpses {
            if c.left == 0 {
                continue;
            }
            self.tmp_l[..m].fill(0.0);
            self.tmp_r[..m].fill(0.0);
            self.tmp_d[..m].fill(0.0);
            c.mech.render_block(&mut self.tmp_l[..m], &mut self.tmp_r[..m], &mut self.tmp_d[..m]);
            let bus = if guitar { Some(c.mech.guitar.bus()) } else { None };
            for i in 0..m {
                if c.left == 0 {
                    break;
                }
                // **Not a straight line: a straight line has two corners.**
                // The fade starts at full gain and ends at zero, and a ramp's
                // ends are slope discontinuities: small in the first
                // difference, louder in the second, and the Clav patches have
                // a resonant filter downstream that rings whatever corner it
                // is handed. `t^2 (3 - 2t)` leaves and arrives with zero
                // slope, so there is no corner at either end. Worth two
                // multiplies: it is not what the bench's steal figures were
                // about (those are the pickups' own gain, and this moved them
                // by a fraction of a percent), but a corner is a corner and
                // there is no reason to hand one to a filter.
                let t = c.left as f32 / c.total as f32;
                let g = t * t * (3.0 - 2.0 * t);
                self.acc_l[i] += self.tmp_l[i] * g;
                self.acc_r[i] += self.tmp_r[i] * g;
                match bus {
                    Some(s) => self.fretboard.bus(s, m)[i] += self.tmp_d[i] * g,
                    None => self.acc_d[i] += self.tmp_d[i] * g,
                }
                c.left -= 1;
            }
        }
        if hushed {
            // **The drive too, not only what comes back.** The harp's strings
            // and the soundboard's network are delay lines: fed at full level
            // from the clear and only heard through the rise, they played the
            // clear's edge back once the rise was over, a line or a bass
            // string's period after it (Piano 2 at 64-frame calls, 30 to 50
            // ms after the stop: 1.42 times the steady second difference,
            // where BODY at 0 reads 0.40). A key struck since the stop or the
            // fade's end drives them at full: a note of its own, which the
            // hush is not fading (`hush_from`).
            for i in 0..m {
                self.acc_d[i] *= self.hush_g[i];
            }
            // Only a key struck since the stop can be one, and none is if the
            // strike count has not moved. Held out for a repeated all notes
            // off, the shared side is under the hush for as long as the voices
            // sound, and a walk of every voice a block for none put the
            // electric grand's force-quiet 3.3 % over its one all notes off at
            // 1-frame calls (1.9 % without it).
            if self.strikes != from {
                for note in &mut self.notes {
                    if note.pitch.is_none() || note.mech.finished() || !struck_since(note) {
                        continue;
                    }
                    if guitar {
                        let s = note.mech.guitar.bus();
                        let sounded = note.mech.guitar.render(&mut self.acc_l[..m], &mut self.acc_r[..m], self.fretboard.bus(s, m));
                        if let (true, Some(p)) = (note.mech.guitar.finished(), note.pitch) {
                            self.fretboard.voice_ended(p, sounded);
                        }
                    } else {
                        note.mech.render_block(&mut self.acc_l[..m], &mut self.acc_r[..m], &mut self.acc_d[..m]);
                    }
                }
            }
            // The voices' share, kept aside so that only what the shared side
            // adds to it below is faded.
            self.tmp_l[..m].copy_from_slice(&self.acc_l[..m]);
            self.tmp_r[..m].copy_from_slice(&self.acc_r[..m]);
        }
        if guitar {
            self.fretboard.render(&mut self.acc_l[..m], &mut self.acc_r[..m], m);
        }
        if self.key_noise.active() {
            let base = if self.patch == Patch::Piano2 { &grand::GrandBase::CONCERT } else { &grand::GrandBase::INTIMATE };
            self.key_noise.render(base, &mut self.acc_l[..m], &mut self.acc_r[..m]);
        }
        if hushed {
            for i in 0..m {
                let g = self.hush_g[i];
                self.acc_l[i] = self.tmp_l[i] + (self.acc_l[i] - self.tmp_l[i]) * g;
                self.acc_r[i] = self.tmp_r[i] + (self.acc_r[i] - self.tmp_r[i]) * g;
            }
        }
        // The fade ends with this block: a key struck from here on, on the
        // frame of the clear among them, never drove what it faded.
        if self.hush_due {
            self.hush_from = self.strikes;
        }
        hushed
    }

    /// Add `frames` of interleaved audio into `out`.
    ///
    /// Adds rather than overwrites, exactly as `Dx7::render` does, because
    /// that is the contract `instrument.rs` renders against — it clears the
    /// scratch buffer itself and then lets the instrument sum into it.
    pub fn render(&mut self, out: &mut [f32], frames: usize, channels: usize) {
        // The grand's solved strings follow the knobs a key per 64 frames of
        // the call, and the harp's a string per 64 frames of the timeline,
        // paid on its grid (`harp_due`), whether or not anything sounds (a
        // compare when nothing moved); the blocks the host does not render
        // pay in `idle`, or else in `set_params`.
        self.follow_designs(Audio::Went(frames));
        if self.sounding == 0 || channels == 0 {
            self.pass_harp_grid(frames);
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
        let harp = self.has_harp();
        let (trem_knob, trem_hz) = match patch {
            Patch::EpSuitcase => (self.params[5].clamp(0.0, 1.0), 5.0),
            _ => (0.0, 0.0),
        };
        // The reed amp's throb knob: the dry patch's depth, the wet one's
        // dB (see `reed_amp`).
        let reed_knob = self.params[5].clamp(0.0, 1.0);
        let trem_inc = std::f32::consts::TAU * trem_hz / self.rate;
        let wah_sens = self.params[4].clamp(0.0, 1.0);
        let lift = patch.lift();
        // Whether this block is under the hush (`render_blocks`).
        let mut hushed = false;
        let (mut at, mut m) = (0, 0);
        for f in 0..n {
            // **Every voice is rendered a block at a time**: into the
            // accumulators once per `modal::BLOCK` frames (`render_blocks`),
            // read back here a sample at a time through the shared side and
            // the amp. The voices own their stereo (the grand's two
            // microphones, the tine's all-passes, the clav's seating): no
            // pan. A per-sample path for voices that had no block one lived
            // beside this until every family had one, and a patch left out
            // of its list would have played silence.
            if f == at + m {
                at = f;
                // The harp's payment falls on its grid (`harp_due`), before
                // the block's voices and strings, and a block ends where the
                // next one is due.
                if self.harp_due == 0 {
                    self.follow_harp(modal::BLOCK);
                    self.harp_due = modal::BLOCK;
                }
                m = (n - f).min(modal::BLOCK).min(self.harp_due);
                // A block ends where the hush's fade or rise does, so its
                // clear and its rise's end fall on the same frame at every
                // host call (they waited for the next block: up to a block
                // later at longer calls).
                for to_go in [self.hush_left, self.hush_rise] {
                    if to_go > 0 {
                        m = m.min(to_go as usize);
                    }
                }
                self.harp_due -= m;
                hushed = self.render_blocks(m);
                // The harp hears the block's bridge drive at once, string
                // by string: `symp::Harp::render` is its samples ticked
                // one at a time, to the bit, at a block's cost.
                self.acc_h[..m].fill(0.0);
                if harp {
                    self.harp.render(&self.acc_d[..m], &mut self.acc_h[..m]);
                }
            }
            let i = f - at;
            let mut left = self.acc_l[i];
            let mut right = self.acc_r[i];
            // What the ringing strings put INTO the body this sample, and what
            // the body gives back. One shared path, so a pedalled chord lights
            // up the strings nobody struck.
            let mut drive = self.acc_d[i];
            // The soundboard/tonebar knob rides here rather than inside the
            // body, so the board's own decay is untouched and only how much
            // of it reaches the bus moves.
            // The harp: the undamped strings hear the bridge sum and give
            // back what they carry. Heard like any string — on the bus at
            // centre, and into the board through the same drive — and
            // rendered from the voices' and corpses' drive alone, so it
            // hears the bridge and not itself: eighty-eight strings feeding
            // each other through a one-sample loop would be a resonator with
            // nothing to bound it.
            // The rest of the shared side under the hush: one, exactly,
            // when there is none.
            let g = if hushed { self.hush_g[i] } else { 1.0 };
            if harp {
                let h = self.acc_h[i] * g;
                drive += h;
                left += h;
                right += h;
            }
            let (bl, br) = self.body.tick(drive);
            left += bl * body_amt * g;
            right += br * body_amt * g;
            if self.pedal_noise.active() {
                let (pl, pr) = self.pedal_noise.tick();
                left += pl * g;
                right += pr * g;
            }
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
                    let knob = reed_amp::ReedAmp::smooth(&mut self.trem_knob, &mut self.trem_knob_set, self.trem_knob_a, trem_knob);
                    let trem_depth = (knob * 0.85).clamp(0.0, 0.85);
                    let pan = trem_depth * self.trem_phase.sin();
                    let a = std::f32::consts::FRAC_PI_4 * (1.0 + pan);
                    let peak = std::f32::consts::SQRT_2
                        * (std::f32::consts::FRAC_PI_4 * (1.0 + trem_depth)).sin();
                    let norm = std::f32::consts::SQRT_2 / peak.max(1.0);
                    left *= a.cos() * norm;
                    right *= sine_apart(a) * norm;
                }
                Patch::Wurli200 => {
                    // The dry reed piano's throb, LEVEL, in mono — one
                    // output stage, both channels together (`ReedAmp::dry`).
                    let (l, r) = self.reed_amp.dry(left, right, reed_knob);
                    left = l;
                    right = r;
                }
                Patch::Wurli140 => {
                    // The reed piano through its amp (`ReedAmp::wet`).
                    let (l, r) = self.reed_amp.wet(left, right, reed_knob);
                    left = l;
                    right = r;
                }
                Patch::ClavWah => {
                    // The touch filter: `clav_wah`, voiced to the
                    // reference's envelope wah.
                    (left, right) = self.wah.process(left, right, wah_sens);
                }
                _ => {}
            }
            // The sound's level (`Patch::lift`): one, exactly, for six.
            left *= lift;
            right *= lift;
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
        // Frames asked for that `out` had no room for still went by.
        self.pass_harp_grid(frames - n);
        if patch == Patch::Wurli140 {
            self.reed_amp.end_block();
        }
        // Recount once per block rather than per sample: `finished` is cheap
        // but sixty-four of them per sample is not, and a voice that ends
        // mid-block costs at most one block of being counted.
        let mut live = 0usize;
        let guitar = patch == Patch::Guitar;
        for n in &mut self.notes {
            if n.pitch.is_some() && n.mech.finished() {
                // The struck string was the voice's from the strike; the
                // harp takes it back now that nothing is sounding it. A
                // guitar's string is the fretboard's again already, from the
                // frame its voice ended on (`render_blocks`).
                if let (true, Some(p)) = (harp, n.pitch) {
                    self.harp.voice_done(p);
                }
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
        // The voices a repeated all notes off held the shared side out for
        // are done: it is cleared under the silence, and rises, in the next
        // block (`hush_held`).
        if live == 0 {
            self.hush_held = false;
        }
        if self.body.ringing() {
            self.sounding += 1;
        }
        if self.harp.active() {
            self.sounding += 1;
        }
        if self.pedal_noise.active() {
            self.sounding += 1;
        }
        if self.key_noise.active() {
            self.sounding += 1;
        }
        // The amp's room rings on after the last reed has stopped.
        if patch == Patch::Wurli140 && self.reed_amp.ringing() {
            self.sounding += 1;
        }
        // The open strings and the air ring on after the last pluck.
        if guitar && self.fretboard.active() {
            self.sounding += 1;
        }
        // And the hush runs to its clear and its rise to the end.
        self.hush_counted = self.hush_left > 0 || self.hush_due || self.hush_rise > 0;
        if self.hush_counted {
            self.sounding += 1;
        }
        // **Silent, the amp side is a new one's.** Nothing sounds: no voice
        // and no tail (the Reed Amp's room is one: `ReedAmp::ringing`), and
        // the host stops calling until something does.
        // - The knob smoothers are there for a knob moved under a sound, and
        //   a knob moved in the silence must be heard at its new value from
        //   the next note's first sample. Both kept their state, and the new
        //   depth glided in over that note's first 100 ms (verifier: the
        //   suitcase's left-right swing over its first 60 ms read 16.6 dB
        //   where the depth gives 21.6). Unset, each takes the next value as
        //   it comes; a steady knob was that value already.
        // - The Reed Amp's lines still held -87 to -101 dBFS of the room
        //   when the frame stopped rendering it, and that came back under
        //   the next note.
        // - The tremolos' phases ran on from wherever the last call stopped,
        //   which is up to a call past the moment the sound ended, and the
        //   wah's follower fell as far: the next phrase's throb and opening
        //   moved with the host's call size. The frame cannot know how long
        //   the silence lasts, and a new amp's phase and a rested pedal are
        //   as good as any and the same at every call size.
        if self.sounding == 0 {
            self.trem_phase = 0.0;
            self.trem_knob_set = false;
            match patch {
                Patch::Wurli200 | Patch::Wurli140 => self.reed_amp.reset(),
                Patch::ClavWah => self.wah.reset(),
                _ => {}
            }
        }
    }
}

/// Where a key sits across the stereo field, as the player hears it: bass to
/// the left, treble to the right, and not all the way to either. A straight
/// line over sixty-six keys, A0 at -0.55 up to +0.55 at D#6 (key 87), where
/// it saturates: every key above D#6 sits at +0.55 with it. Its centre is
/// F#3 (key 54), so middle C sits a little right of it, at +0.10. The reed
/// piano's WIDTH spreads its voices by this law (`WurliKnobs::width`, voiced
/// and measured against it), which is why it stays the law it is rather than
/// one over the keyboard's eighty-seven.
fn pan_for(pitch: u8) -> f32 {
    let t = (f32::from(pitch) - 21.0) / 66.0;
    (t.clamp(0.0, 1.0) - 0.5) * 1.1
}

/// Equal-power pan, normalised so centre is 0 dB rather than -3.
fn pan_gains(pan: f32) -> (f32, f32) {
    let p = (pan.clamp(-1.0, 1.0) + 1.0) * 0.5;
    let a = std::f32::consts::FRAC_PI_2 * p;
    (a.cos() * std::f32::consts::SQRT_2, sine_apart(a) * std::f32::consts::SQRT_2)
}

/// **The sine of an angle whose cosine is taken beside it, never inlined**,
/// so a debug build and a release one give the same bits. Inlined beside
/// the cosine of the same angle, an optimised build fuses the two into one
/// `sincos` call (`__sincosf_stret` on Apple), which rounds a last bit its
/// own way (the engine's `instrument::pan_gains` has the measurement): the
/// suitcase's vibrato made its first strike hash one way under `cargo test`
/// and another under `cargo test --release`, which is the profile that
/// ships. `noinline` is an attribute LLVM's inliner never overrides, so both
/// profiles make the two plain calls. Every f32 sine taken beside its own
/// angle's cosine in this module goes through here. That is not every way
/// the two profiles part: on the study's twenty listening takes, debug
/// against release, six sounds are the same to the bit on both and the
/// clavs' comping and both reed pianos still differ, by -108 to -132 dB re
/// peak (cause not traced; the f64 designs still take their pairs whole).
/// Every patch the first-strike pin strikes hashes the same in both.
#[inline(never)]
pub(crate) fn sine_apart(a: f32) -> f32 {
    a.sin()
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
        assert_eq!(Patch::ALL.len(), 10);
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

    /// A knob that is not a number lands on the patch's default, on every
    /// patch and every knob, and the patch still plays finite: a NaN would
    /// pass a clamp and reach the strike, and the reed's spread by the
    /// frame's pan law.
    #[test]
    fn a_nan_knob_is_the_patch_default() {
        for &patch in &Patch::ALL {
            let mut k = Keys::new(48_000.0);
            k.set_patch(patch);
            for i in 0..MAX_PARAMS {
                k.set_param(i, f32::NAN);
            }
            assert_eq!(*k.params(), patch.defaults(), "{patch:?}");
            k.note_on(60, 0.8);
            let mut buf = vec![0.0f32; 1024 * 2];
            k.render(&mut buf, 1024, 2);
            assert!(buf.iter().all(|s| s.is_finite()), "{patch:?} went non-finite");
        }
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
        // And its one inharmonic voice — the bar's second mode, where the
        // beam puts it for this base's taper and solder blob at C4 —
        // stays attack colour: well under the fundamental even in the
        // attack window, where it is at its loudest.
        let fund = goertzel_hann(&out, f0, 48_000.0).max(1.0e-12);
        let over = f0 * reed::WurliBase::TWO_HUNDRED.overtones(60.0).f2;
        let over1 = peak_near(&out, over).1;
        let over_db = 20.0 * (over1 / fund).max(1.0e-12).log10();
        assert!(
            over_db < -35.0,
            "the reed's bar overtone reads {over_db:.0} dB under the \
             fundamental; louder than -35 and it starts sounding like a bell"
        );

        // The tine's bell, isolated: the same fortissimo strike with BELL
        // at zero, subtracted (and WIDTH at zero, so the mid is the voice).
        // It is attack colour now and quiet — the reference tine piano's own
        // bell, isolated the same way, is 63 dB under the fundamental at
        // velocity 127 and 41 at 70 — so what keeps the two mechanisms apart
        // is not how loud it is but WHERE it is: the beam's second mode at
        // the spring the base puts on middle C, 7.36·f0, a third of a
        // partial off the series, where the reed's loudest upper partial is
        // a harmonic.
        let bell_run = |bell: f32| {
            let mut tine = tine::EpVoice::new(48_000.0);
            let k = EpKnobs {
                bell,
                width: 0.0,
                ..EpKnobs::default()
            };
            tine.strike_knobs(60, 1.0, &k);
            run_voice(&mut || tine.tick(), 0.25)
        };
        let (with, without) = (bell_run(1.0), bell_run(0.0));
        let only: Vec<f32> = with.iter().zip(&without).map(|(a, b)| a - b).collect();
        let fund = goertzel_hann(&with, f0, 48_000.0).max(1.0e-12);
        let mut best = (0.0_f32, 0.0_f32);
        let mut f = f0 * 3.5;
        while f < f0 * 12.0 {
            let g = goertzel_hann(&only, f, 48_000.0);
            if g > best.1 {
                best = (f, g);
            }
            f *= 1.001;
        }
        let (bf, bg) = best;
        let bell_db = 20.0 * (bg / fund).max(1.0e-12).log10();
        let t = bf / f0;
        println!("the tine's bell at {t:.3} x f0, {bell_db:.1} dB re the fundamental at fortissimo");
        assert!(
            (6.9..=7.8).contains(&t),
            "the tine's bell sits at {t:.2} x f0; the reference's middle C has it at 7.36"
        );
        assert!(
            (t - t.round()).abs() > 0.25,
            "the tine's bell sits at {t:.2} x f0 — a bell lives OFF the \
             harmonic series, this one converged onto it"
        );
        assert!(
            (-75.0..=-50.0).contains(&bell_db),
            "the tine's bell reads {bell_db:.0} dB re the fundamental at \
             fortissimo; the reference's is -63"
        );
    }

    /// The shipped pianos and the stretch they are tuned with, for the
    /// STRETCH tests: each patch, its stretched tuning in cents from equal
    /// temperament at STRETCH centre and its B, both per key from A0.
    fn stretched() -> [(Patch, [f32; 88], [f32; 88]); 3] {
        let (c1, b1) = grand::tuning(&grand::GrandBase::INTIMATE);
        let (c2, b2) = grand::tuning(&grand::GrandBase::CONCERT);
        let (ce, be) = egrand::tuning();
        [(Patch::Piano1, c1, b1), (Patch::Piano2, c2, b2), (Patch::ElectricGrand, ce, be)]
    }

    /// `pitch` struck at MIDI 90 through the frame with UNISON at zero (the
    /// course one string, so no beat moves a line) and STRETCH at `knob`:
    /// the strongest line within `span` of `around` over 0.05 s to `secs`,
    /// mono, searched in tenth-of-a-cent steps.
    fn stretched_line(patch: Patch, pitch: u8, knob: f32, around: f32, span: f32, secs: f32) -> f32 {
        let (l, r) = pedal_script(patch, &[(0.0, PedalEv::Knob(4, 0.0)), (0.0, PedalEv::Knob(5, knob)), (0.0, PedalEv::On(pitch, 90))], secs);
        let (a, b) = ((0.05 * SR) as usize, l.len());
        let mono: Vec<f32> = l[a..b].iter().zip(&r[a..b]).map(|(x, y)| 0.5 * (x + y)).collect();
        let mut best = (0.0_f32, around);
        let mut f = around * (1.0 - span);
        while f < around * (1.0 + span) {
            let g = goertzel_hann(&mono, f, SR);
            if g > best.0 {
                best = (g, f);
            }
            f *= 1.000_057_8;
        }
        best.1
    }

    /// **STRETCH turns the way it says, by as much as the strings say.** C4
    /// on each shipped piano, STRETCH from 0 to 1: its sixth partial moves
    /// sharp by what the instrument's own tuning puts there, within a cent.
    /// STRETCH scales B, and the tuning with it (a stretched octave is one
    /// whose coincident partials agree), so partial n of key k sits at
    /// `n F sqrt(1 + n^2 B s)` with `F sqrt(1 + B s)` the fundamental at
    /// `c s` cents from equal temperament, `s` the knob's multiplier: from 0
    /// to 1 that is +26.9 cents on Piano 1, +21.7 on Piano 2 and +26.4 on the
    /// electric grand, and each measures within 0.1 of it. The test used to
    /// strike the waveguide piano, which left the pool when the modal grand
    /// took both pianos. Measured: printed.
    #[test]
    fn the_stretch_knob_pulls_the_partials_sharper_not_flatter() {
        let f0 = 261.6256_f32;
        let (s0, s1) = (bipolar(0.0, 1.6), bipolar(1.0, 1.6));
        for (patch, cents, b) in stretched() {
            let (c, b) = (cents[60 - 21], b[60 - 21]);
            let n6 = |s: f32| c * s + 600.0 * ((1.0 + 36.0 * b * s) / (1.0 + b * s)).log2();
            let want = n6(s1) - n6(s0);
            let moved = 1_200.0 * (stretched_line(patch, 60, 1.0, 6.0 * f0, 0.06, 1.0) / stretched_line(patch, 60, 0.0, 6.0 * f0, 0.06, 1.0)).log2();
            println!("{patch:?}: STRETCH 0 to 1 moves C4's n6 {moved:+.1} cents; its tuning says {want:+.1}");
            assert!(want > 10.0 && (moved - want).abs() <= 1.0, "{patch:?}: STRETCH moved n6 {moved:+.1} cents, where its tuning says {want:+.1}");
        }
    }

    /// **The treble sounds where its stretched tuning puts it, at either end
    /// of the knob.** A7 and C8 on each shipped piano, STRETCH at 0 and at 1:
    /// the fundamental within a cent of `c s` cents from equal temperament,
    /// the instrument's own stretch at that knob (above; `grand::tuning`),
    /// measured within 0.3. At STRETCH 1 that is far sharp of equal
    /// temperament (A7 +104, +101 and +61 cents, C8 +128, +120 and +80 on
    /// Piano 1, Piano 2 and the electric grand), so an equal-temperament
    /// bound would accuse the tuning; the review's +27 to +50 were read in a
    /// window of 3 % and found the key's own harp string, which was strung at
    /// STRETCH centre then, not the voice. Through the frame, on the instruments
    /// that ship: the test used to strike the waveguide piano, whose all-pass
    /// cascade could eat a short string's delay. Measured: printed.
    #[test]
    fn the_treble_stays_on_pitch_at_either_stretch_extreme() {
        for (patch, cents, _) in stretched() {
            for pitch in [105_u8, 108] {
                let equal = 440.0 * 2.0_f32.powf((f32::from(pitch) - 69.0) / 12.0);
                for knob in [0.0_f32, 1.0] {
                    let want = cents[usize::from(pitch - 21)] * bipolar(knob, 1.6);
                    let got = 1_200.0 * (stretched_line(patch, pitch, knob, equal, 0.10, 0.4) / equal).log2();
                    println!("{patch:?} key {pitch} at STRETCH {knob}: {got:+.1} cents from equal temperament, its tuning {want:+.1}");
                    assert!((got - want).abs() <= 1.0, "{patch:?} key {pitch} at STRETCH {knob}: {got:+.1} cents, its tuning {want:+.1}");
                }
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

    /// **The suitcase's tremolo knob moves without a step.** C4 at MIDI 100
    /// held, the knob thrown from 0 to 1 at a block's edge at eight moments
    /// of the throb: the right channel's largest second difference over the
    /// four samples across the edge stays within twice the largest over the
    /// 40 ms before (0.97 measured). The depth was applied as the knob stood,
    /// and the edge read 106 times it here at 787cdb89 (38 at the review's
    /// moments; F9). A steady knob is the old throb to the bit
    /// (`a_first_strike_from_silence_is_unchanged`, EpSuitcase).
    #[test]
    fn the_suitcase_tremolo_knob_moves_without_a_step() {
        let mut worst = 0.0_f32;
        for j in 0..8 {
            let edge = at(0.4 + 0.0257 * j as f32);
            let (_, r) = pedal_script(Patch::EpSuitcase, &[(0.0, PedalEv::Knob(5, 0.0)), (0.01, PedalEv::On(60, 100)), (edge as f32 / SR, PedalEv::Knob(5, 1.0))], 0.8);
            let (before, across) = (d2(&r, edge - ms(40.0), edge - 2), d2(&r, edge - 1, edge + 3));
            worst = worst.max(across / before);
        }
        println!("the suitcase's tremolo thrown 0 to 1 under a held C4: at worst {worst:.2} times the steady second difference");
        assert!(worst <= 2.0, "the tremolo knob stepped the output ({worst:.1} times the steady second difference)");
    }

    // ── consecutive strikes ─────────────────────────────────────────────

    /// One event in a scripted performance, at a fixed velocity: these tests
    /// are about WHEN a key goes down, never how hard.
    #[derive(Clone, Copy)]
    enum Ev {
        On(u8),
        Off(u8),
        Pedal(bool),
    }

    const SR: f32 = 48_000.0;
    const BLOCK: usize = 128;
    const VEL: f32 = 0.8;

    /// Seconds to a block boundary at or before that instant — where the
    /// loop below will actually apply an event, and therefore where a
    /// measurement window has to be centred.
    fn at(secs: f32) -> usize {
        (secs * SR) as usize / BLOCK * BLOCK
    }

    fn ms(m: f32) -> usize {
        (m * 0.001 * SR) as usize
    }

    /// Mono render of a scripted performance, driven in the same block size
    /// the callback uses so that a fault at a block boundary happens here too.
    fn script(patch: Patch, events: &[(usize, Ev)], frames: usize) -> Vec<f32> {
        let mut k = Keys::new(SR);
        k.set_patch(patch);
        let mut events = events.to_vec();
        events.sort_by_key(|e| e.0);
        let mut out = Vec::with_capacity(frames);
        let mut scratch = [0.0f32; BLOCK * 2];
        let mut next = 0;
        let mut frame = 0;
        while frame < frames {
            while next < events.len() && events[next].0 < frame + BLOCK {
                match events[next].1 {
                    Ev::On(p) => k.note_on(p, VEL),
                    Ev::Off(p) => k.note_off(p),
                    Ev::Pedal(d) => k.set_pedal(d),
                }
                next += 1;
            }
            scratch.fill(0.0);
            k.render(&mut scratch, BLOCK, 2);
            out.extend(scratch.chunks_exact(2).map(|f| (f[0] + f[1]) * 0.5));
            frame += BLOCK;
        }
        out
    }

    /// Largest one-sample step in a window: the plain measure of an edge.
    fn d1(x: &[f32], a: usize, b: usize) -> f32 {
        let b = b.min(x.len());
        (a.max(1)..b).fold(0.0f32, |m, n| m.max((x[n] - x[n - 1]).abs()))
    }

    /// Largest second difference. A sinusoid's is its first scaled by the
    /// tiny angle per sample, so a STEP — whose second difference is the
    /// whole step — stands clear of a loud low bed that the first difference
    /// alone would let it hide under.
    fn d2(x: &[f32], a: usize, b: usize) -> f32 {
        let b = b.min(x.len());
        (a.max(2)..b).fold(0.0f32, |m, n| {
            m.max((x[n] - 2.0 * x[n - 1] + x[n - 2]).abs())
        })
    }

    fn rms(x: &[f32], a: usize, b: usize) -> f32 {
        let b = b.min(x.len());
        if b <= a {
            return 0.0;
        }
        let e: f64 = x[a..b].iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
        (e / (b - a) as f64).sqrt() as f32
    }

    /// How far a landing strike's shape may drift from a fresh strike's
    /// before it is called a pop, as a ratio. Three decibels — the same
    /// figure the bench uses.
    const EDGE_TOL: f32 = 1.413;

    /// How far above a single fresh strike's peak eight of them at sixty
    /// milliseconds may stack, as a ratio: four decibels.
    ///
    /// **Derived, not chosen.** The strike's energy cap holds a re-struck
    /// mode to a quarter over one strike's own throw — 1.9 dB of
    /// DISPLACEMENT — and the pickups are expansive, so at the output that
    /// reads as more than it does on the metal. The worst measured is the
    /// reed piano at 3.2 dB, which is the plate's curve doing exactly what
    /// the growl is; four is the round number above it, and it is a long way
    /// under the clamp every mechanism ends its tick with.
    const STACK_MARGIN: f32 = 1.585;

    /// A fresh strike of `pitch` from silence: what every landing strike is
    /// measured against.
    fn fresh_edge(patch: Patch, pitch: u8, secs: f32) -> (f32, f32, f32) {
        let x = script(
            patch,
            &[(0, Ev::Pedal(true)), (at(0.01), Ev::On(pitch))],
            (secs * SR) as usize,
        );
        let peak = x.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        (d1(&x, 0, x.len()), d2(&x, 0, x.len()), peak)
    }

    /// **What a landing strike ADDED, against the same strike from silence.**
    ///
    /// The performance is rendered twice, once with the event and once
    /// without, and the difference is exactly what that strike contributed
    /// through whatever the amp pass was doing at the time.
    fn added(patch: Patch, base: &[(usize, Ev)], event: (usize, Ev), frames: usize) -> (f32, f32) {
        let bed = script(patch, base, frames);
        let mut with = base.to_vec();
        with.push(event);
        let got = script(patch, &with, frames);
        let diff: Vec<f32> = got.iter().zip(&bed).map(|(a, b)| a - b).collect();
        let (a, b) = (event.0.saturating_sub(ms(2.0)), event.0 + ms(20.0));
        (d1(&diff, a, b), d2(&diff, a, b))
    }

    /// **A hammer is not a step, and the difference is the SHAPE.**
    ///
    /// A magnet leaned on, a wah wide open, a plate the reed is deep inside:
    /// each is a gain the strike arrives through, and a gain multiplies the
    /// first and second differences by the SAME number. A state cut and
    /// re-seeded does not — a step is broadband, so it lands in the second
    /// difference many times harder than in the first. So the property to
    /// hold a landing strike to is not "no louder than a fresh one", which
    /// no nonlinear pickup can promise; it is "the same shape as a fresh
    /// one", which every honest superposition can, whatever gain it is read
    /// through. Measured: a straight sum reads 1.00 against 1.00, the reed
    /// deep in its plate reads 1.63 against 1.75, and the faults this module
    /// was written for read tenfold.
    fn assert_same_shape(what: &str, patch: Patch, added: (f32, f32), fresh: (f32, f32)) {
        let r1 = added.0 / fresh.0.max(1e-12);
        let r2 = added.1 / fresh.1.max(1e-12);
        assert!(
            r2 <= r1 * EDGE_TOL,
            "{patch:?} {what}: what the strike added is {r1:.2}x a fresh strike's first \
             difference but {r2:.2}x its second — that ratio is a step, not a hammer"
        );
    }

    /// A key struck again while its own note is still ringing under the
    /// pedal adds a hammer, not a step — on every patch.
    #[test]
    fn a_restrike_under_the_pedal_adds_what_a_fresh_strike_adds() {
        for &patch in &Patch::ALL {
            let (f1, f2, _) = fresh_edge(patch, 60, 0.1);
            let base = [
                (0usize, Ev::Pedal(true)),
                (0, Ev::On(60)),
                (at(0.1), Ev::Off(60)),
            ];
            let a = added(patch, &base, (at(0.2), Ev::On(60)), at(0.25));
            assert_same_shape("restrike", patch, a, (f1, f2));
        }
    }

    /// The pool full and one voice buried: the newcomer landing on the slot
    /// the corpse vacated adds a hammer, not a step.
    #[test]
    fn a_steal_adds_what_a_fresh_strike_adds() {
        for &patch in &Patch::ALL {
            let (f1, f2, _) = fresh_edge(patch, 100, 0.06);
            let mut base = vec![(0usize, Ev::Pedal(true))];
            for i in 0..VOICES {
                base.push((0, Ev::On(24 + i as u8)));
            }
            // A released voice is what the stealing policy takes first, and
            // under the pedal it is still at full level: the worst case.
            base.push((at(0.02), Ev::Off(30)));
            let a = added(patch, &base, (at(0.1), Ev::On(100)), at(0.14));
            assert_same_shape("steal", patch, a, (f1, f2));
        }
    }

    /// **A chord that steals eight voices at once fades all eight out.** The
    /// pool filled under the pedal (64 keys a block apart, each let go at
    /// once), then an eight-key chord in one block: what the chord added has
    /// the shape of the same chord struck from silence on every patch that
    /// can steal (the guitar's forty-four keys cannot fill the pool). With
    /// three corpses the fourth to eighth steals each cut a corpse at full
    /// gain, its whole fade still to run (192 of 192 samples), and on the
    /// Reed 200 the step stood clear of the chord's attack (review, alloc 7).
    #[test]
    fn a_chord_that_steals_eight_voices_fades_all_eight() {
        for &patch in &Patch::ALL {
            if patch == Patch::Guitar {
                continue;
            }
            let chord: Vec<(usize, Ev)> = (101..109).map(|p| (at(0.6), Ev::On(p))).collect();
            let mut base = vec![(0usize, Ev::Pedal(true))];
            for i in 0..VOICES {
                let t = (i + 1) * BLOCK;
                base.extend([(t, Ev::On(24 + i as u8)), (t, Ev::Off(24 + i as u8))]);
            }
            let frames = at(0.64);
            let bed = script(patch, &base, frames);
            let mut with = base.clone();
            with.extend(chord.iter().copied());
            let got = script(patch, &with, frames);
            let diff: Vec<f32> = got.iter().zip(&bed).map(|(a, b)| a - b).collect();
            let (a, b) = (at(0.6) - ms(2.0), at(0.6) + ms(20.0));
            let mut alone = vec![(0usize, Ev::Pedal(true))];
            alone.extend(chord.iter().map(|&(_, e)| (at(0.01), e)));
            let fresh = script(patch, &alone, at(0.05));
            assert_same_shape("an eight-voice steal", patch, (d1(&diff, a, b), d2(&diff, a, b)), (d1(&fresh, 0, fresh.len()), d2(&fresh, 0, fresh.len())));
        }
    }

    /// **Eight strikes at sixty milliseconds, pedal down.** Superposition
    /// that stacks without bound is one way to be wrong and a strike that
    /// cancels the note it landed on is the other: neither may the peak run
    /// away nor may a repeat leave a hole where the note was.
    #[test]
    fn eight_pedalled_repeats_neither_stack_nor_leave_a_hole() {
        for &patch in &Patch::ALL {
            let (_, _, fresh_peak) = fresh_edge(patch, 60, 0.1);
            let mut ev = vec![(0usize, Ev::Pedal(true))];
            let mut onsets = Vec::new();
            for k in 0..8 {
                let t = at(k as f32 * 0.060);
                onsets.push(t);
                ev.push((t, Ev::On(60)));
                ev.push((at(k as f32 * 0.060 + 0.030), Ev::Off(60)));
            }
            let x = script(patch, &ev, at(0.52));
            let peak = x.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(
                peak <= fresh_peak * STACK_MARGIN,
                "{patch:?} stacked to {peak} against a fresh strike's {fresh_peak}"
            );
            // The quietest four milliseconds in the twelve after each strike,
            // against the four before it. A landing hammer mostly adds; one
            // landing against the motion takes energy out, which is why this
            // floor is not zero. A voice cut to silence and re-attacked is
            // tens of decibels down, and one capped into cancellation — the
            // fault this test was written for — was ten.
            //
            // **The floor moved from six decibels to eight in the physics
            // rework, and the reason is the rework itself.** Before it, the
            // two electric pianos carried modes at fixed multiples of the
            // fundamental, oscillators of their own; a restrike that
            // cancelled the fundamental left those ringing on, and they
            // filled the hole. Now the harmonics are made by the pickup out
            // of the fundamental's own motion, which is where a real
            // instrument's harmonics come from — so when the fundamental
            // cancels they cancel with it, and nothing is left to mask it.
            // Measured here, deepest dip of the seven restrikes per patch:
            //
            //   Rhodes suitcase  -6.5    Wurlitzer 140  -5.5
            //   Rhodes Mk.1      -6.4    Wurlitzer 200  -5.1
            //   both pianos, both clavs: nothing past -3.4
            //
            // It is exactly the four instruments that got a transducer, and
            // the two that did not are untouched — which is why this reads
            // as the transducer's signature and not as one mechanism's bug.
            // Eight decibels keeps a decibel and a half of margin under the
            // measurement and stays two clear of the fault. The number is
            // empirical, as the six was; it is written down here with what
            // it was measured against so the next person does not have to
            // guess which it is.
            for &o in &onsets[1..] {
                let before = rms(&x, o.saturating_sub(ms(4.0)), o);
                if before <= 1.0e-3 {
                    continue;
                }
                let mut worst = f32::MAX;
                let mut a = o;
                while a + ms(4.0) <= o + ms(12.0) {
                    worst = worst.min(rms(&x, a, a + ms(4.0)));
                    a += ms(1.0);
                }
                assert!(
                    worst >= before * 0.40,
                    "{patch:?} lost {:.1} dB of the note it was struck on",
                    20.0 * (worst / before).log10()
                );
            }
        }
    }

    /// **The first-strike pin.** One hash per mechanism of a strike from
    /// silence, so a change meant for CONSECUTIVE strikes cannot quietly
    /// retune the instruments on its way past.
    ///
    /// A hash rather than a tolerance: "unchanged" is the claim, and a
    /// tolerance lets a retune through one commit at a time. If this fails
    /// and the change to the sound was meant, `keysrig pin` prints the same
    /// figure for every patch at both ends of every knob, so the diff of two
    /// printouts says exactly what moved.
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
        // One patch per mechanism: piano, tine, tangent, reed.
        //
        // All four were re-pinned in the physics rework's wave B, which is
        // what §5.3 of its brief asks for: the piano's hammer became a mass
        // in contact with the string, its soundboard grew a modal plate,
        // the sympathetic harp now answers this very render (it is pedalled
        // from the first block), the reed's plate and the tine's magnet and
        // the tangent's coil all became real transducers, and the tine's
        // overtones and the tangent's inharmonicity came off a beam and a
        // measured table instead of two schedules that ran the wrong way.
        // Four hashes rather than one because a change meant for a single
        // mechanism must not move the other three, and through this whole
        // wave each moved only when its own lane landed.
        //
        // Piano1 re-pinned by lane/modal: Piano 1 and Piano 2 now play
        // `grand::GrandVoice`, the modal grand, so its first strike is a
        // different instrument's by design (MERGE-ME.md §5). Re-pinned again
        // by the lane's fix pass (implicit contact, refitted voicing), and by
        // the grand lane (the midpoint contact, the felt's core, the action, the
        // knock and the key's release, the pedals; its MERGE-ME), and by its
        // fix pass (the action's noises in their own resonators, the intimate's
        // hammer refitted with its felt's rate hardening, its knock, its
        // regulation and its level; its MERGE-ME).
        //
        // Piano1 re-pinned again by the harp lane: this strike is pedalled, so the
        // harp answers it, and the harp is now strung from the grand's own
        // tuning with a lossless read and the struck string's halo, fed flat
        // and shaped and heard as it sounds (`symp.rs`; the harp lane's MERGE-ME).
        // The grand's voice is untouched.
        //
        // EpMk1 re-pinned by the tine lane: the tine pianos were re-voiced
        // against the reference tine piano (regulation, swing table, level
        // law, decays, bell, knock, damper, stereo image) and are
        // block-rendered with their own stereo image, so the Mk.1's first
        // strike is a different voicing by design (its MERGE-ME.md §5).
        //
        // Clav re-pinned by the clav lane: the clav was rebuilt on the
        // reference analysis (a lossless loop read at the coils, a new
        // contact, the quadratic term, the copy, the noise, the voice's own
        // stereo through the block path, its output stage's spread and
        // two-microphone image), so its first strike is a new instrument's
        // by design (the lane's MERGE-ME.md).
        //
        // Wurli200 re-pinned by the reed lane: the reed voice was rebuilt against
        // the reference study (a charge-read plate, a per-key swing and level,
        // a two-stage decay, a key-tracking knock, block rendering), so its
        // first strike is a different voice by design (lane reed MERGE-ME.md);
        // and again by its fix pass (the knock the key's own at every strike,
        // a fortissimo slap, middle C in one slope, the level 2.5 dB under the
        // reference's).
        //
        // EpSuitcase pinned by the frame-fix lane, at the hash 787cdb89 renders: its
        // tremolo's depth is now smoothed (`Keys::render`), and a steady knob
        // must be the old throb to the bit.
        //
        // EpSuitcase re-pinned in release when its vibrato's sine was taken
        // out of line (`sine_apart`): an optimised build had fused the
        // vibrato's sine and cosine of one angle into a single `sincos` call,
        // which rounds a last bit its own way, so the shipped build hashed
        // 9_528_229_155_151_203_875 where a debug one hashed this. Both now
        // make the two plain calls and hash the same, as every patch here
        // does.
        //
        // Piano1 re-pinned for 5.1.0 by its level alone: the frame lifts it
        // 7 dB at its output (`Patch::lift`), every sample the old one times
        // `LIFT` to the bit (4_764_576_450_495_230_413 before).
        for (patch, want) in [
            (Patch::Piano1, 17_994_698_811_216_702_661),
            (Patch::EpMk1, 3_987_617_924_517_340_302),
            (Patch::Clav, 5_576_748_566_300_753_751),
            (Patch::Wurli200, 18_381_627_961_271_600_470),
            (Patch::EpSuitcase, 6_077_776_810_017_057_856),
        ] {
            let x = script(
                patch,
                &[
                    (0, Ev::Pedal(true)),
                    (0, Ev::On(60)),
                    (at(0.15), Ev::Off(60)),
                ],
                at(0.25),
            );
            assert_eq!(fnv(&x), want, "{patch:?} first strike moved");
        }
    }

    /// Render `Keys` on a grand patch through a list of (seconds, action)
    /// events, the pedals as the three controller levels `instrument.rs`
    /// hands `set_pedals`: (left, right).
    fn pedal_script(patch: Patch, events: &[(f32, PedalEv)], secs: f32) -> (Vec<f32>, Vec<f32>) {
        let mut k = Keys::new(SR);
        k.set_patch(patch);
        let mut cc = [0u8; 3];
        let frames = (secs * SR) as usize / BLOCK * BLOCK;
        let (mut l, mut r) = (Vec::with_capacity(frames), Vec::with_capacity(frames));
        let mut scratch = [0.0f32; BLOCK * 2];
        let mut next = 0;
        let mut frame = 0;
        while frame < frames {
            while next < events.len() && ((events[next].0 * SR) as usize) < frame + BLOCK {
                match events[next].1 {
                    PedalEv::On(p, v) => k.note_on(p, f32::from(v) / 127.0),
                    PedalEv::Off(p) => k.note_off(p),
                    PedalEv::Cc(slot, v) => {
                        cc[slot] = v;
                        k.set_pedals(cc);
                    }
                    PedalEv::Patch(p) => k.set_patch(p),
                    PedalEv::AllOff => k.all_notes_off(),
                    PedalEv::Knob(i, v) => k.set_param(i, v),
                }
                next += 1;
            }
            scratch.fill(0.0);
            k.render(&mut scratch, BLOCK, 2);
            for [a, b] in scratch.as_chunks::<2>().0 {
                l.push(*a);
                r.push(*b);
            }
            frame += BLOCK;
        }
        (l, r)
    }

    #[derive(Clone, Copy)]
    enum PedalEv {
        On(u8, u8),
        Off(u8),
        /// Pedal slot (0 sustain, 1 sostenuto, 2 soft) to a CC level.
        Cc(usize, u8),
        /// A patch change, all notes off and a knob, as the host sends them
        /// between blocks.
        Patch(Patch),
        AllOff,
        Knob(usize, f32),
    }

    /// `pedal_script` with the events in any order: the script reads them
    /// in time order.
    fn pedal_sorted(patch: Patch, events: &[(f32, PedalEv)], secs: f32) -> (Vec<f32>, Vec<f32>) {
        let mut ev = events.to_vec();
        ev.sort_by(|a, b| a.0.total_cmp(&b.0));
        pedal_script(patch, &ev, secs)
    }

    /// The level of a partial near `f` Hz over `len` seconds from `from`,
    /// both channels' power, dB (Hann, searched within half a percent).
    fn tone_db(l: &[f32], r: &[f32], f: f32, from: f32, len: f32) -> f32 {
        let (a, b) = ((from * SR) as usize, ((from + len) * SR) as usize);
        let mag = |x: &[f32], g: f32| {
            let w = std::f32::consts::TAU * g / SR;
            let n = x.len() as f32;
            let (mut re, mut im, mut ws) = (0.0f64, 0.0f64, 0.0f64);
            for (i, &s) in x.iter().enumerate() {
                let h = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n).cos();
                re += f64::from(s * h * (w * i as f32).cos());
                im += f64::from(s * h * (w * i as f32).sin());
                ws += f64::from(h);
            }
            2.0 * (re * re + im * im).sqrt() / ws
        };
        let best = (0..11)
            .map(|i| {
                let g = f * (0.9975 + 0.0005 * i as f32);
                0.5 * (mag(&l[a..b], g).powi(2) + mag(&r[a..b], g).powi(2))
            })
            .fold(0.0f64, f64::max);
        (10.0 * best.max(1e-30).log10()) as f32
    }

    /// **The sostenuto holds the keys it caught, and only those.** On the
    /// grand: C4 down, the sostenuto down (CC 66), then E4 down; both keys up
    /// at 0.6 s with the sustain pedal up. C4's damper stays off its string
    /// (its fundamental falls at the undamped string's rate), E4's lands
    /// (falls at the damper's); the sostenuto up at 1.4 s drops C4's damper
    /// too. Measured: printed.
    #[test]
    fn the_sostenuto_holds_only_the_keys_it_caught() {
        for patch in [Patch::Piano1, Patch::Piano2] {
            let (l, r) = pedal_script(
                patch,
                &[
                    (0.0, PedalEv::On(60, 80)),
                    (0.1, PedalEv::Cc(1, 127)),
                    (0.2, PedalEv::On(64, 80)),
                    (0.6, PedalEv::Off(60)),
                    (0.6, PedalEv::Off(64)),
                    (1.4, PedalEv::Cc(1, 0)),
                ],
                2.0,
            );
            let (c4, e4) = (261.6, 329.6);
            // dB per second over 0.7..1.2 s, after each key came up.
            let fall = |f: f32, a: f32, b: f32| (tone_db(&l, &r, f, a, 0.1) - tone_db(&l, &r, f, b, 0.1)) / (b - a);
            let (held, damped, dropped) = (fall(c4, 0.7, 1.2), fall(e4, 0.7, 1.2), fall(c4, 1.45, 1.75));
            println!("{patch:?}: caught C4 falls {held:.0} dB/s, uncaught E4 {damped:.0} dB/s, C4 after the sostenuto {dropped:.0} dB/s");
            assert!(held < 25.0, "{patch:?}: the caught C4 was damped ({held:.0} dB/s)");
            assert!(damped > 60.0, "{patch:?}: the uncaught E4 rang on ({damped:.0} dB/s)");
            assert!(dropped > 60.0, "{patch:?}: the sostenuto came up and C4 rang on ({dropped:.0} dB/s)");
        }
    }

    /// **The sostenuto catches the keys that are down, sounding or not.** C4,
    /// E4 and G4 pressed silently (MIDI 1, under the let-off) on each harp
    /// patch, the sostenuto 0.3 s later, long after their silent voices have
    /// finished, the keys up at 0.4 s and a staccato C2 at 0.6 s: the three
    /// strings answer the C2 as they do with the keys still held, their
    /// partials over 1-2 s within half a decibel of the held take. The catch
    /// read the voices and caught none of the three: they read
    /// -122.8/-136.6/-98.0 dB on Piano 1 against the held keys'
    /// -83.7/-88.9/-82.1 (review, F1). Measured: printed.
    #[test]
    fn the_sostenuto_catches_a_silently_held_key() {
        for patch in [Patch::Piano1, Patch::Piano2, Patch::ElectricGrand] {
            let mut held = vec![(0.6, PedalEv::On(36, 100)), (0.75, PedalEv::Off(36))];
            for p in [60, 64, 67] {
                held.push((0.0, PedalEv::On(p, 1)));
            }
            let mut caught = held.clone();
            caught.push((0.3, PedalEv::Cc(1, 127)));
            for p in [60, 64, 67] {
                caught.push((0.4, PedalEv::Off(p)));
            }
            let (hl, hr) = pedal_sorted(patch, &held, 2.2);
            let (cl, cr) = pedal_sorted(patch, &caught, 2.2);
            for f in [261.6, 329.6, 392.0] {
                let (h, c) = (tone_db(&hl, &hr, f, 1.0, 1.0), tone_db(&cl, &cr, f, 1.0, 1.0));
                println!("{patch:?}: the string at {f} Hz answers a C2 at {h:.1} dB held, {c:.1} dB caught and let go");
                assert!((h - c).abs() <= 0.5, "{patch:?}: the sostenuto missed the silent key at {f} Hz ({c:.1} dB against {h:.1})");
            }
        }
    }

    /// **And asks the KEY whether it is up when it comes up.** C6 at MIDI 12
    /// on the two grands, held throughout: its note dies under the finger
    /// (4.4 s on Piano 1, 5.5 s on Piano 2). With the sostenuto down at 0.1 s
    /// and up at 6 s, the key still held, the take is the one without the
    /// sostenuto to the bit: the release asked for a held voice, found none,
    /// and damped the string under the finger (its answer to a later C4 fell
    /// 10 to 15 dB; review, alloc 1). And caught only after its note died
    /// (the sostenuto at 6 s, the key up at 6.2 s), C6's string is held as
    /// the key held it: its answer to a C5 within half a decibel of the take
    /// with the key down, where it read the key let go (review, F1).
    #[test]
    fn the_sostenuto_asks_the_key_whether_it_is_up() {
        for patch in [Patch::Piano1, Patch::Piano2] {
            let tail = [(6.4, PedalEv::On(60, 100)), (6.5, PedalEv::Off(60))];
            let mut plain = vec![(0.0, PedalEv::On(84, 12))];
            plain.extend(tail);
            let mut with = plain.clone();
            with.extend([(0.1, PedalEv::Cc(1, 127)), (6.0, PedalEv::Cc(1, 0))]);
            let (a, b) = (pedal_sorted(patch, &plain, 7.5), pedal_sorted(patch, &with, 7.5));
            assert!(a == b, "{patch:?}: the sostenuto came up over a held key and changed the take");
            // Caught with its note already gone.
            let late = [(0.0, PedalEv::On(84, 12)), (6.0, PedalEv::Cc(1, 127)), (6.2, PedalEv::Off(84)), (6.4, PedalEv::On(72, 100)), (6.5, PedalEv::Off(72))];
            let (ll, lr) = pedal_sorted(patch, &late, 7.6);
            let held = [(0.0, PedalEv::On(84, 12)), (6.4, PedalEv::On(72, 100)), (6.5, PedalEv::Off(72))];
            let (hl, hr) = pedal_sorted(patch, &held, 7.6);
            let (h, c) = (tone_db(&hl, &hr, 1046.5, 6.7, 0.8), tone_db(&ll, &lr, 1046.5, 6.7, 0.8));
            println!("{patch:?}: C6's string answers a C5 at {h:.1} dB with its key down, {c:.1} dB caught after its note died");
            assert!((h - c).abs() <= 0.5, "{patch:?}: the sostenuto missed a held key whose note had died ({c:.1} dB against {h:.1})");
        }
    }

    /// **A caught key let go leaves its halo; its damper stays up.** C4 at
    /// MIDI 80 on each harp patch, the sostenuto down under it, the key up:
    /// its harp string is held clear of its felt and rung (the harp's own,
    /// free, ringing with its voice), and is heading for exactly the
    /// coupling and loop gain of the same key let go under the sustain
    /// pedal. The frame told the harp nothing, and the string stayed its
    /// voice's halo, broad by design, for as long as the sostenuto was down
    /// (review, alloc 8).
    #[test]
    fn a_caught_keys_string_leaves_its_halo() {
        for patch in [Patch::Piano1, Patch::Piano2, Patch::ElectricGrand] {
            let let_go = |cc: [u8; 3]| {
                let mut k = Keys::new(SR);
                k.set_patch(patch);
                k.note_on(60, 80.0 / 127.0);
                let mut out = [0.0f32; BLOCK * 2];
                for _ in 0..4 {
                    k.render(&mut out, BLOCK, 2);
                }
                k.set_pedals(cc);
                k.note_off(60);
                k.harp.string_state(60).unwrap()
            };
            let (caught, sustained) = (let_go([0, 127, 0]), let_go([127, 0, 0]));
            assert_eq!(caught.0, [true, false, true], "{patch:?}: the caught string (held, voiced, rung)");
            assert_eq!(caught.1, sustained.1, "{patch:?}: the caught string is not the sustained one's");
        }
    }

    /// **A stray or doubled note-off does nothing.** C3 at MIDI 100 let go at
    /// 1 s on each patch with a harp or a fretboard, the sustain up and
    /// down: a second note-off for it at 1.1 s and one for a key never
    /// pressed at 1.2 s leave the take as it is, to the bit. The harp heard
    /// every note-off: a second one re-armed the string's felt and took its
    /// ring back from it (the C3 residual on Piano 2 fell from -136 to -158
    /// dB; review, alloc 9).
    #[test]
    fn a_stray_or_doubled_note_off_does_nothing() {
        for patch in [Patch::Piano1, Patch::Piano2, Patch::ElectricGrand, Patch::Guitar] {
            for sus in [0u8, 127] {
                let once = [(0.0, PedalEv::Cc(0, sus)), (0.0, PedalEv::On(48, 100)), (1.0, PedalEv::Off(48))];
                let mut twice = once.to_vec();
                twice.extend([(1.1, PedalEv::Off(48)), (1.2, PedalEv::Off(50))]);
                assert!(pedal_script(patch, &once, 2.5) == pedal_script(patch, &twice, 2.5), "{patch:?}, sustain {sus}: a stray note-off changed the take");
            }
        }
    }

    /// **All notes off is not undone by the pedal.** A chord under the sustain
    /// on the two grands (C3 E3 G3 C4 at MIDI 100), the keys up, all notes
    /// off at 0.8 s, and the pedal's level then moved by a hair (to 126, 20
    /// ms later), or to 120 or 100: the half second from 0.2 s after the move
    /// is no louder than with the pedal left where it was. The cut voices kept
    /// their keys, and the next level lifted their dampers again: -43.5 dB
    /// against -83.6 at 126 (review, alloc 2). Measured: printed.
    #[test]
    fn all_notes_off_is_not_undone_by_the_pedal() {
        for patch in [Patch::Piano1, Patch::Piano2] {
            let take = |moved: &[(f32, PedalEv)]| {
                let mut ev = vec![(0.0, PedalEv::Cc(0, 127)), (0.8, PedalEv::AllOff)];
                for p in [48, 52, 55, 60] {
                    ev.extend([(0.0, PedalEv::On(p, 100)), (0.5, PedalEv::Off(p))]);
                }
                ev.extend(moved);
                pedal_sorted(patch, &ev, 1.8)
            };
            let (sl, sr) = take(&[]);
            for (t, cc) in [(0.82, 126u8), (0.86, 120), (0.95, 100)] {
                let (ml, mr) = take(&[(t, PedalEv::Cc(0, cc))]);
                let (moved, still) = (span_db(&ml, &mr, t + 0.2, 0.5), span_db(&sl, &sr, t + 0.2, 0.5));
                println!("{patch:?}: all notes off, the pedal to {cc} at {t} s: {moved:+.1} dBFS, left at 127 {still:+.1}");
                assert!(moved <= still + 1.0, "{patch:?}: the pedal to {cc} undid all notes off ({moved:+.1} dBFS against {still:+.1})");
            }
        }
    }

    /// **All notes off fades the shared side out; it does not cut it.** A
    /// pedalled six-note chord (E2 B2 E3 G#3 B3 E4 at MIDI 100) on Piano 1,
    /// the electric grand and the guitar, stopped by all notes off at 1.2 s:
    /// the largest second difference from the stop to the end of the fade is
    /// within 1.5 times the largest of the 100 ms before, and measures 0.73,
    /// 0.85 and 0.68 times it. The board, the harp and the open strings were
    /// cleared in one sample: 1.66, 10.5 and 5.2 times it here at 787cdb89
    /// (the review's four-sample window read 6.4, 3.2 and 2.2; F10). The fade
    /// is bounded: called every block from the stop on, as the host's
    /// force-quiet calls it, it holds the instrument awake no longer than a
    /// fade and two blocks past its last voice (measured: 2, 3 and 5 ms).
    /// And a key struck in the stop's own block keeps its strings: on Piano 1
    /// the harp is awake under it once the fade has run.
    #[test]
    fn all_notes_off_fades_the_shared_side_out() {
        let chord = |ev: &mut Vec<(f32, PedalEv)>| {
            ev.push((0.0, PedalEv::Cc(0, 127)));
            for (i, p) in [40u8, 47, 52, 56, 59, 64].into_iter().enumerate() {
                ev.push((0.01 + 0.012 * i as f32, PedalEv::On(p, 100)));
            }
        };
        let s = at(1.2);
        for patch in [Patch::Piano1, Patch::ElectricGrand, Patch::Guitar] {
            let mut ev = Vec::new();
            chord(&mut ev);
            ev.push((s as f32 / SR, PedalEv::AllOff));
            let (l, r) = pedal_script(patch, &ev, 1.4);
            let fade = (HUSH_FADE * SR) as usize;
            let d2_lr = |a: usize, b: usize| d2(&l, a, b).max(d2(&r, a, b));
            let (before, across) = (d2_lr(s - ms(100.0), s - 1), d2_lr(s - 1, s + fade + 4));
            println!("{patch:?}: all notes off under a pedalled chord: second difference over the fade {across:.5}, the 100 ms before {before:.5} ({:.2}x)", across / before);
            assert!(across <= 1.5 * before, "{patch:?}: all notes off stepped the output ({:.2} times the steady second difference)", across / before);
            // The force-quiet: all notes off every block from the stop on.
            let mut k = Keys::new(SR);
            k.set_patch(patch);
            k.set_pedals([127, 0, 0]);
            for p in [40u8, 47, 52, 56, 59, 64] {
                k.note_on(p, 100.0 / 127.0);
            }
            let mut out = [0.0f32; BLOCK * 2];
            for _ in 0..at(1.2) / BLOCK {
                k.render(&mut out, BLOCK, 2);
            }
            let (mut blocks, mut voices_done) = (0, None);
            while k.active() && blocks < at(5.0) / BLOCK {
                k.all_notes_off();
                k.render(&mut out, BLOCK, 2);
                blocks += 1;
                if voices_done.is_none() && k.notes.iter().all(|n| n.pitch.is_none()) {
                    voices_done = Some(blocks);
                }
            }
            let secs = |b: usize| (b * BLOCK) as f32 / SR;
            let done = voices_done.map_or(f32::NAN, secs);
            println!("{patch:?}: the force-quiet's voices done {done:.3} s on, the instrument asleep at {:.3} s", secs(blocks));
            assert!(!k.active() && secs(blocks) - done <= HUSH_FADE + secs(2), "{patch:?}: the hush held the instrument awake {:.3} s past its voices", secs(blocks) - done);
        }
        let mut k = Keys::new(SR);
        k.set_pedals([127, 0, 0]);
        for p in [40u8, 47, 52] {
            k.note_on(p, 100.0 / 127.0);
        }
        let mut out = [0.0f32; BLOCK * 2];
        for _ in 0..40 {
            k.render(&mut out, BLOCK, 2);
        }
        k.all_notes_off();
        k.note_on(64, 0.8);
        for _ in 0..12 {
            k.render(&mut out, BLOCK, 2);
        }
        assert!(k.harp.active(), "a key struck with all notes off lost its strings to the fade");
    }

    /// **All notes off leaves no late click.** The pedalled six-note chord on
    /// the two grands, stopped at 1.2 s by all notes off, or by every key
    /// let go and the pedal lifted at the same moment: from 25 to 56 ms after
    /// the stop (where a leftover in the soundboard's network comes back, a
    /// line's length after the hush's clear) the largest second difference
    /// is no larger than the natural stop's. The network's one-pole states
    /// survived `Body::reset`: -56.6 and -52.2 dBFS here at 4122d312 against
    /// the natural stop's -61.0 and -58.6 (verifier). Measured: printed.
    #[test]
    fn all_notes_off_leaves_no_late_click() {
        let s = at(1.2);
        for patch in [Patch::Piano1, Patch::Piano2] {
            let mut stopped = vec![(0.0, PedalEv::Cc(0, 127))];
            for (i, p) in [40u8, 47, 52, 56, 59, 64].into_iter().enumerate() {
                stopped.push((0.01 + 0.012 * i as f32, PedalEv::On(p, 100)));
            }
            let mut lifted = stopped.clone();
            stopped.push((s as f32 / SR, PedalEv::AllOff));
            for p in [40u8, 47, 52, 56, 59, 64] {
                lifted.push((s as f32 / SR, PedalEv::Off(p)));
            }
            lifted.push((s as f32 / SR, PedalEv::Cc(0, 0)));
            let late = |ev: &[(f32, PedalEv)]| {
                let (l, r) = pedal_sorted(patch, ev, 1.4);
                d2(&l, s + ms(25.0), s + ms(56.0)).max(d2(&r, s + ms(25.0), s + ms(56.0)))
            };
            let (hush, natural) = (late(&stopped), late(&lifted));
            let db = |v: f32| 20.0 * v.max(1e-30).log10();
            println!("{patch:?}: second difference 25-56 ms after the stop: all notes off {:+.1} dBFS, keys up and pedal lifted {:+.1} dBFS", db(hush), db(natural));
            assert!(hush <= natural, "{patch:?}: all notes off clicked {:+.1} dB over the natural stop 25-56 ms on", db(hush) - db(natural));
        }
    }

    /// **The hush fades the shared side back in, and steps no more than the
    /// natural stop at any host call.** The pedalled six-note chord on the
    /// four patches with a shared side, stopped at 1.2 s by all notes off,
    /// at host calls of 1, 16, 32, 64, 96 and 128 frames: over the 80 ms
    /// after the stop the largest second difference in any 5 ms window, re
    /// the largest of the 100 ms before, is no larger than the same take's
    /// with every key let go and the pedal lifted at the stop (or than the
    /// steady level, where that stop is quieter still). Measured: 0.68 to
    /// 0.95 times (the natural stop: 0.70 to 1.82). The shared side was
    /// cleared and heard again at once at full gain, its delay lines fed at
    /// full level from the clear, and the network's one-poles left set:
    /// 4122d312 read 2.29 to 4.23 times on Piano 1, 1.20 to 3.81 on Piano 2
    /// (the electric grand 0.86 to 0.95 at both) and 0.73 to 6.71 on the
    /// guitar, its air (verifier, finding 2). Measured: printed.
    #[test]
    fn the_hush_steps_no_more_than_the_natural_stop_at_any_call() {
        let s_at = |call: usize| at(1.2) / BLOCK * BLOCK / call * call;
        let take = |patch: Patch, call: usize, natural: bool| {
            let mut k = Keys::new(SR);
            k.set_patch(patch);
            k.set_pedals([127, 0, 0]);
            let keys = [40u8, 47, 52, 56, 59, 64];
            let n = at(1.4) / call * call;
            let mut out = vec![0.0_f32; 2 * n];
            let mut struck = 0;
            for (c, chunk) in out.chunks_mut(2 * call).enumerate() {
                let f = c * call;
                while struck < keys.len() && f + call > at(0.01 + 0.012 * struck as f32) {
                    k.note_on(keys[struck], 100.0 / 127.0);
                    struck += 1;
                }
                if f == s_at(call) {
                    if natural {
                        for p in keys {
                            k.note_off(p);
                        }
                        k.set_pedals([0, 0, 0]);
                    } else {
                        k.all_notes_off();
                    }
                }
                k.render(chunk, call, 2);
            }
            let (l, r): (Vec<f32>, Vec<f32>) = out.chunks_exact(2).map(|f| (f[0], f[1])).unzip();
            let d = |a: usize, b: usize| d2(&l, a, b).max(d2(&r, a, b));
            let s = s_at(call);
            let before = d(s - ms(100.0), s - 1);
            (0..16).map(|w| d(s + ms(5.0) * w, s + ms(5.0) * (w + 1)) / before).fold(0.0_f32, f32::max)
        };
        for patch in [Patch::Piano1, Patch::Piano2, Patch::ElectricGrand, Patch::Guitar] {
            for call in [1usize, 16, 32, 64, 96, 128] {
                let (hush, natural) = (take(patch, call, false), take(patch, call, true));
                println!("{patch:?} at {call}-frame calls: the worst step after the stop {hush:.2} times the steady second difference, the natural stop's {natural:.2}");
                assert!(hush <= natural.max(1.0), "{patch:?} at {call}-frame calls: all notes off stepped {hush:.2} times the steady second difference, the natural stop {natural:.2}");
            }
        }
    }

    /// **All notes off during the rise turns it where it stands.** The
    /// pedalled chord on Piano 1 stopped by all notes off, then all notes off
    /// again at every block from a third of the way into the rise (the host's
    /// force-quiet): while the choked voices sound, the hush's gain, frame by
    /// frame, never moves by more than its curve's steepest step (1.5 /
    /// `hush_total` a frame) and stays within 0 and 1; once they are done the
    /// shared side is cleared with nothing to drive it, the hush ends and the
    /// instrument sleeps within two blocks. Restarted from full gain at the
    /// turn instead, the gain jumped by 0.83 there.
    #[test]
    fn all_notes_off_during_the_rise_turns_it_where_it_stands() {
        let mut k = Keys::new(SR);
        k.set_pedals([127, 0, 0]);
        for p in [40u8, 47, 52, 56, 59, 64] {
            k.note_on(p, 100.0 / 127.0);
        }
        let mut out = [0.0_f32; BLOCK * 2];
        for _ in 0..at(1.2) / BLOCK {
            k.render(&mut out, BLOCK, 2);
        }
        k.all_notes_off();
        let mut gains = Vec::new();
        let mut turned = false;
        let sounding = |k: &Keys| k.notes.iter().any(|n| n.pitch.is_some() && !n.mech.finished());
        while sounding(&k) && gains.len() < at(3.0) {
            if k.hush_rise > 0 && k.hush_rise <= 2 * k.hush_total / 3 {
                turned = true;
            }
            if turned {
                k.all_notes_off();
            }
            // A block's frames one at a time, so each frame's gain is the
            // one `hush_g` holds after it.
            for _ in 0..BLOCK {
                let hushed = k.hush_left > 0 || k.hush_due || k.hush_rise > 0;
                k.render(&mut out[..2], 1, 2);
                gains.push(if hushed { k.hush_g[0] } else { 1.0 });
            }
        }
        assert!(turned && !sounding(&k), "the rise was never turned, or the voices never ended");
        for _ in 0..2 {
            k.all_notes_off();
            k.render(&mut out, BLOCK, 2);
        }
        assert!(!k.active() && k.hush_left == 0 && !k.hush_due && k.hush_rise == 0, "the hush held on past the voices");
        let steepest = 1.5 / k.hush_total as f32 + 1e-6;
        let jump = gains.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0_f32, f32::max);
        assert!(jump <= steepest, "the hush's gain jumped {jump:.3} in a frame (its curve's steepest step is {steepest:.4})");
        assert!(gains.iter().all(|g| (0.0..=1.0).contains(g)), "the hush's gain left 0..1");
    }

    /// **A repeated all notes off costs nothing more than the first.** The
    /// host's force-quiet, all notes off before every call for as long as
    /// the instrument is active: the pedalled six-note chord on every patch,
    /// stopped that way at 1.2 s, at calls of 1, 7, 37 and 128 frames. A
    /// patch with a shared side clears it at most three times (the hush's
    /// clear; the clear when the voices it then held out for are done; and
    /// the hush's end, which finds nothing left on the guitar: measured 2,
    /// 2, 2 and 3 on Piano 1, Piano 2, the electric grand and the guitar),
    /// and one with none never does. At 0d20e3d1 each call in the rise
    /// turned it into a fade that cleared the shared side and rose again,
    /// about every second call, on every patch: at 1-frame calls 12659 /
    /// 21129 / 40273 / 2862 clears on those four, 46740 on the Reed Amp,
    /// which has nothing to clear, and 200 to 764 at 128-frame calls (a
    /// clear is 2.5 us). The instrument falls asleep within 2 ms of when it
    /// did, or earlier with no hush to keep it awake: the Reed Amp 96 ms
    /// earlier at 128-frame calls (printed).
    #[test]
    fn a_repeated_all_notes_off_costs_nothing_more_than_the_first() {
        for patch in Patch::ALL {
            let mut line = format!("{patch:?}: the force-quiet clears the shared side");
            for call in [1usize, 7, 37, 128] {
                let mut k = Keys::new(SR);
                k.set_patch(patch);
                k.set_pedals([127, 0, 0]);
                for p in [40u8, 47, 52, 56, 59, 64] {
                    k.note_on(p, 100.0 / 127.0);
                }
                let mut out = vec![0.0_f32; 2 * call];
                for _ in 0..at(1.2) / call {
                    k.render(&mut out, call, 2);
                }
                let before = k.shared_clears;
                let mut calls = 0;
                while k.active() && calls * call < at(5.0) {
                    k.all_notes_off();
                    k.render(&mut out, call, 2);
                    calls += 1;
                }
                let clears = k.shared_clears - before;
                line += &format!(" {clears} times at {call}-frame calls ({:.3} s to sleep);", (calls * call) as f32 / SR);
                assert!(!k.active(), "{patch:?} at {call}-frame calls: the force-quiet held the instrument awake 5 s");
                let most = if k.has_shared() { 3 } else { 0 };
                assert!(clears <= most, "{patch:?} at {call}-frame calls: the force-quiet cleared the shared side {clears} times (at most {most})");
            }
            println!("{line}");
        }
    }

    /// The largest step a key struck in the hush's fade may put on its shared
    /// side, as a multiple of a fresh strike's second difference at the same
    /// age: the decision at `Keys::hush_from`, held. Measured struck 5.3 and
    /// 8.0 ms into the fade: 1.02 / 1.02 on Piano 1, 1.05 / 1.03 on Piano 2,
    /// 1.03 / 1.03 on the electric grand. Its drive kept at full from the
    /// clear (the alternative the decision weighed) read 1.11 / 1.40, 1.03 /
    /// 1.93 and 1.02 / 1.02.
    const FADE_STRUCK_STEP: f32 = 1.2;

    /// **A key struck in the rise keeps its shared side.** The pedalled
    /// chord on each harp patch stopped by all notes off at 1.2 s, and C5
    /// struck five blocks on, after the clear and inside the rise: over its
    /// first 60 ms what the strike added is as near a strike from silence
    /// (within a decibel) as it is on the same take with the shared side
    /// cleared at the stop and never hushed (the rule before the hush, which
    /// stepped there; it leaves the new key's strings untouched). Measured
    /// -38.0 / -45.5 / -40.5 dB on Piano 1 / Piano 2 / the electric grand,
    /// cleared at the stop -38.2 / -44.5 / -39.7. Faded in with the rest,
    /// its drive and its strings' answer, it read -34.2 / -36.5 / -36.8.
    ///
    /// The same on the frame of the clear itself, the call cut there: the
    /// fade is over and the key never drove what it faded. Measured -37.2 /
    /// -42.7 / -40.7 dB, cleared at the stop -37.8 / -43.4 / -39.2. Taken
    /// for a key struck in the fade, whose drive the rise fades in, it read
    /// -31.0 / -31.4 / -33.5.
    ///
    /// A key struck during the fade itself is the case decided at
    /// `Keys::hush_from` (printed). The guitar's open strings and air are
    /// faded back in on what they give, since the air hears the strings and
    /// the choked voices alike, and a key struck in its rise is -63.9 dB off
    /// its own (printed): the open strings deaf after the clear listen again
    /// from its strike, to the choked voices' last too (-65.0 when they
    /// listened throughout; `guitar::Fretboard::hush`).
    #[test]
    fn a_key_struck_in_the_rise_keeps_its_shared_side() {
        let s = at(1.2);
        // The key's frame cuts the call there, struck or not, so that the
        // take with it and the take without are rendered alike.
        let take = |patch: Patch, at_: usize, strike: bool, hushed: bool| {
            let mut k = Keys::new(SR);
            k.set_patch(patch);
            k.set_pedals([127, 0, 0]);
            let keys = [40u8, 47, 52, 56, 59, 64];
            let key = if patch == Patch::Guitar { 69 } else { 72 };
            let mut out = vec![0.0_f32; 2 * at(1.5)];
            for (b, chunk) in out.chunks_mut(2 * BLOCK).enumerate() {
                let f = b * BLOCK;
                for (i, &p) in keys.iter().enumerate() {
                    if f == at(0.01 + 0.012 * i as f32) / BLOCK * BLOCK {
                        k.note_on(p, 100.0 / 127.0);
                    }
                }
                if f == s {
                    k.all_notes_off();
                    if !hushed {
                        k.clear_shared();
                        (k.hush_left, k.hush_due) = (0, false);
                    }
                }
                let cut = if (f + 1..f + BLOCK).contains(&at_) { at_ - f } else { 0 };
                let (head, tail) = chunk.split_at_mut(2 * cut);
                if cut > 0 {
                    k.render(head, cut, 2);
                }
                if strike && f + cut == at_ {
                    k.note_on(key, 0.8);
                }
                k.render(tail, BLOCK - cut, 2);
            }
            out
        };
        for patch in [Patch::Piano1, Patch::Piano2, Patch::ElectricGrand, Patch::Guitar] {
            let fresh = {
                let mut k = Keys::new(SR);
                k.set_patch(patch);
                k.set_pedals([127, 0, 0]);
                k.note_on(if patch == Patch::Guitar { 69 } else { 72 }, 0.8);
                let mut out = vec![0.0_f32; 2 * at(0.1)];
                for chunk in out.chunks_mut(2 * BLOCK) {
                    k.render(chunk, BLOCK, 2);
                }
                out
            };
            let error = |at_: usize, hushed: bool| {
                let (bed, with) = (take(patch, at_, false, hushed), take(patch, at_, true, hushed));
                let (mut e, mut r) = (0.0f64, 0.0f64);
                for i in 0..2 * ms(60.0) {
                    let d = f64::from(with[2 * at_ + i] - bed[2 * at_ + i]) - f64::from(fresh[i]);
                    e += d * d;
                    r += f64::from(fresh[i]).powi(2);
                }
                (10.0 * (e / r).log10()) as f32
            };
            let (fade, rise, cleared) = (error(s + 2 * BLOCK, true), error(s + 5 * BLOCK, true), error(s + 5 * BLOCK, false));
            let clear = s + (HUSH_FADE * SR) as usize;
            let (edge, edge_cleared) = (error(clear, true), error(clear, false));
            // What the decision at `hush_from` buys: the worst 5 ms second
            // difference of what a key struck in the fade added, 5 to 80 ms
            // on, re a strike from silence's at the same age, either channel.
            let step = |at_: usize| {
                let (bed, with) = (take(patch, at_, false, true), take(patch, at_, true, true));
                let mut worst = 0.0_f32;
                for c in 0..2 {
                    let added: Vec<f32> = (0..ms(80.0)).map(|i| with[2 * (at_ + i) + c] - bed[2 * (at_ + i) + c]).collect();
                    let alone: Vec<f32> = (0..ms(80.0)).map(|i| fresh[2 * i + c]).collect();
                    for w in 1..16 {
                        let (a, b) = (ms(5.0) * w, ms(5.0) * (w + 1));
                        worst = worst.max(d2(&added, a, b) / d2(&alone, a, b).max(1e-9));
                    }
                }
                worst
            };
            let (fade_step, late_step) = (step(s + 2 * BLOCK), step(s + 3 * BLOCK));
            println!("{patch:?}: what C5 added re a strike from silence, 60 ms: struck in the fade {fade:+.1} dB, in the rise {rise:+.1} dB, at that moment on the take cleared at the stop {cleared:+.1} dB; on the frame of the clear {edge:+.1} dB, cleared at the stop {edge_cleared:+.1} dB; struck in the fade, its worst step {fade_step:.2} and {late_step:.2} times a fresh strike's (5.3 and 8.0 ms in)");
            if patch != Patch::Guitar {
                assert!(rise <= cleared + 1.0, "{patch:?}: a key struck in the rise lost its shared side ({rise:+.1} dB against {cleared:+.1} dB never hushed)");
                assert!(edge <= edge_cleared + 1.0, "{patch:?}: a key struck on the frame of the clear lost its shared side ({edge:+.1} dB against {edge_cleared:+.1} dB never hushed)");
                assert!(fade_step.max(late_step) <= FADE_STRUCK_STEP, "{patch:?}: a key struck in the fade stepped {:.2} times a fresh strike's (`hush_from`)", fade_step.max(late_step));
            }
        }
    }

    /// A performance rendered in host calls of `call` frames, each call cut
    /// at an event's frame so that every call size applies every event on
    /// the same sample: interleaved stereo.
    fn at_calls(patch: Patch, events: &[(usize, PedalEv)], frames: usize, call: usize) -> Vec<f32> {
        let mut k = Keys::new(SR);
        k.set_patch(patch);
        let mut cc = [0u8; 3];
        let mut out = vec![0.0_f32; 2 * frames];
        let (mut next, mut f) = (0, 0);
        while f < frames {
            while next < events.len() && events[next].0 <= f {
                match events[next].1 {
                    PedalEv::On(p, v) => k.note_on(p, f32::from(v) / 127.0),
                    PedalEv::Off(p) => k.note_off(p),
                    PedalEv::Cc(slot, v) => {
                        cc[slot] = v;
                        k.set_pedals(cc);
                    }
                    PedalEv::Knob(i, v) => k.set_param(i, v),
                    PedalEv::Patch(p) => k.set_patch(p),
                    PedalEv::AllOff => k.all_notes_off(),
                }
                next += 1;
            }
            let mut end = ((f / call + 1) * call).min(frames);
            if next < events.len() {
                end = end.min(events[next].0);
            }
            k.render(&mut out[2 * f..2 * end], end - f, 2);
            f = end;
        }
        out
    }

    /// The six performances the call-size property is held on, for a
    /// patch: (name, events on their frames, frames). One note held and let
    /// go; six strikes 50 ms apart (the frame-fix verifier's, knob 5 up); a
    /// pedalled chord with a knob moved, a restrike, half a pedal and the
    /// pedal lifted; a phrase, silence and a phrase; a pedalled chord
    /// stopped by all notes off off every grid, a key struck in the rise,
    /// all notes off again, a note after; and three keys struck as the soft
    /// pedal moves and let up within ten frames of one another, the guitar's
    /// hand then coming to rest on its open strings (the frame-2
    /// verifier's).
    fn call_scores(patch: Patch) -> Vec<(&'static str, Vec<(usize, PedalEv)>, usize)> {
        let key = |k: u8| if patch == Patch::Guitar { k.clamp(40, 83) } else { k };
        let f = |t: f32| (t * SR).round() as usize;
        let mut repeats = vec![(0, PedalEv::Knob(5, 1.0))];
        for i in 0..6 {
            let t = 0.1 + 0.0503 * i as f32;
            repeats.extend([(f(t), PedalEv::On(key(62), 100)), (f(t + 0.0251), PedalEv::Off(key(62)))]);
        }
        let mut chord = vec![(0, PedalEv::Cc(0, 127))];
        for (i, k) in [48u8, 55, 64, 67].into_iter().enumerate() {
            chord.push((f(0.013 + 0.0037 * i as f32), PedalEv::On(key(k), 60 + 15 * i as u8)));
        }
        chord.extend([
            (f(0.3107), PedalEv::Off(key(48))),
            (f(0.4019), PedalEv::On(key(60), 110)),
            (f(0.4733), PedalEv::Knob(2, 0.8)),
            (f(0.5521), PedalEv::On(key(60), 60)),
            (f(0.8307), PedalEv::Cc(0, 70)),
            (f(0.9513), PedalEv::Cc(0, 0)),
            (f(1.0501), PedalEv::Off(key(60))),
        ]);
        let gap = vec![
            (0, PedalEv::Knob(5, 1.0)),
            (f(0.0103), PedalEv::On(key(60), 102)),
            (f(0.2011), PedalEv::Off(key(60))),
            (f(3.0007), PedalEv::On(key(60), 102)),
            (f(3.3013), PedalEv::Off(key(60))),
        ];
        let mut stop = vec![(0, PedalEv::Cc(0, 127))];
        for (i, k) in [40u8, 47, 52, 56, 59, 64].into_iter().enumerate() {
            stop.push((f(0.0103 + 0.012 * i as f32), PedalEv::On(key(k), 100)));
        }
        stop.extend([
            (f(1.20037), PedalEv::AllOff),
            (f(1.2151), PedalEv::On(key(72), 102)),
            (f(1.2183), PedalEv::AllOff),
            (f(1.3001), PedalEv::On(key(60), 89)),
            (f(1.5007), PedalEv::Off(key(60))),
        ]);
        let up = vec![
            (0, PedalEv::Cc(2, 127)),
            (f(0.0103), PedalEv::On(key(60), 89)),
            (f(0.2003), PedalEv::Cc(2, 40)),
            (f(0.2507), PedalEv::On(key(62), 102)),
            (f(0.4001), PedalEv::Cc(2, 0)),
            (f(0.4103), PedalEv::On(key(64), 114)),
            (f(0.9001), PedalEv::Off(key(60))),
            (f(0.9003), PedalEv::Off(key(62))),
            (f(0.9005), PedalEv::Off(key(64))),
        ];
        let mut scores = vec![
            ("one note", vec![(f(0.0101), PedalEv::On(key(60), 102)), (f(0.7013), PedalEv::Off(key(60)))], f(1.2)),
            ("repeats", repeats, f(0.8)),
            ("a chord", chord, f(1.6)),
            ("a gap", gap, f(3.6)),
            ("a stop", stop, f(2.2)),
            ("three keys up", up, f(1.4)),
        ];
        for s in &mut scores {
            s.1.sort_by_key(|e| e.0);
        }
        scores
    }

    /// The largest difference over every performance between the take at
    /// each host call size and the 128-frame take, re that take's peak, dB,
    /// with the performance and the call size it was at (`None`: identical
    /// to the bit).
    fn call_size_difference(patch: Patch, calls: &[usize]) -> Option<(f32, &'static str, usize)> {
        let mut worst: Option<(f32, &'static str, usize)> = None;
        for (name, events, frames) in call_scores(patch) {
            let reference = at_calls(patch, &events, frames, 128);
            let peak = reference.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
            for &call in calls {
                let take = at_calls(patch, &events, frames, call);
                if take != reference {
                    let d = take.iter().zip(&reference).fold(0.0_f32, |m, (a, b)| m.max((a - b).abs()));
                    let db = 20.0 * (d / peak).log10();
                    if worst.is_none_or(|w| db > w.0) {
                        worst = Some((db, name, call));
                    }
                }
            }
        }
        worst
    }

    /// **Every patch renders the same at any host call.** Six performances
    /// (`call_scores`) on each patch at calls of 1, 16, 37 and 1024 frames,
    /// every event on its own frame, against the take at 128: the same to the
    /// bit, or within a bound stated with what differs. The control a voice
    /// runs once a block ran once a CALL, so a host's calls were part of the
    /// sound: the reed's six strikes were -4.2 dB re peak off the 128-frame
    /// take at 1-frame calls (frame-fix verifier, finding 5) and the clav's
    /// release, the electric grand's phantoms and damper and the guitar's
    /// damper -20 to -40; the tremolos' phases and the wah's follower, frozen
    /// wherever the last call left them, moved the next phrase by up to -5.9
    /// dB; and the hush's clear and its rise's end waited for the next block,
    /// up to a block later at longer calls (the guitar -63 dB, the electric
    /// grand -72, through all notes off). The guitar's fretboard ran its own
    /// control once a call too: three keys let up within ten frames of one
    /// another, the hand then coming to rest on the open strings a staircase
    /// a call at a time, were -56.3 dB re peak off the 128-frame take at
    /// 1-frame calls at 0d20e3d1 (the frame-2 verifier's performance, the
    /// sixth here; the other five -75.5), and across a gap the air's last
    /// 1e-9, left wherever the call ended, put -135.6 dB re peak under the
    /// next note at 1024-frame calls.
    /// Now every voice runs its control on a grid of its own (each one's
    /// `Ctl`), so does the fretboard (`guitar::BoardCtl`), and a block ends
    /// where the hush's fade or rise does. The bounds, against these
    /// measurements:
    /// - the tine pianos, the dry clav, the Reed 200 and the guitar:
    ///   identical.
    /// - the Clav Wah: -90 dB (measured -100.8), and the Reed Amp: -80
    ///   (-87.1): the frame falls silent at the end of a call, so a longer
    ///   call renders more of the wah's resonance or the room's tail, both
    ///   already under the frame's floor.
    /// - the electric grand: -85 (-91.1, the six strikes at 1-frame calls):
    ///   the hammer's contact is solved a block at a time by superposition,
    ///   and a block a call cuts sums the same forces in another order; the
    ///   bank flushes what is under 1e-20 a part at a time; and the harp
    ///   takes a string back at the end of the call its voice ended in.
    /// These performances do not reach every residual: the electric grand's
    /// velocity ladder differs by -68 at 4096-frame calls (-72 at 1024 and
    /// under; the frame-2 verifier's, not held here).
    /// The two grands are held apart, at 64-frame calls too:
    /// `the_grands_render_the_same_at_any_host_call`.
    #[test]
    fn every_patch_renders_the_same_at_any_host_call() {
        let calls = [1usize, 16, 37, 1024];
        for (patch, bound) in [
            (Patch::EpMk1, None),
            (Patch::EpSuitcase, None),
            (Patch::Clav, None),
            (Patch::Wurli200, None),
            (Patch::ClavWah, Some(-90.0)),
            (Patch::Wurli140, Some(-80.0)),
            (Patch::ElectricGrand, Some(-85.0)),
            (Patch::Guitar, None),
        ] {
            let worst = call_size_difference(patch, &calls);
            println!("{patch:?}: at calls of 1, 16, 37 and 1024 frames, {}", worst.map_or("identical to the 128-frame takes".to_string(), |(w, name, call)| format!("at most {w:+.1} dB re peak off the 128-frame takes ({name}, {call}-frame calls)")));
            match (worst, bound) {
                (None, _) => {}
                (Some((w, name, call)), None) => panic!("{patch:?}: {name} at {call}-frame calls moved {w:+.1} dB re peak off the 128-frame take; it was the same to the bit"),
                (Some((w, name, call)), Some(b)) => assert!(w <= b, "{patch:?}: {name} at {call}-frame calls moved {w:+.1} dB re peak off the 128-frame take (bound {b:+.1})"),
            }
        }
    }

    /// **The grands at any host call.** The modal grand ran its control
    /// once a call: the phantoms the hammer's exit adds came in at the end of
    /// whichever call it left in, the damper's fall stepped once a call, a
    /// contact a call cut was solved afresh at the cut, and the bank was
    /// culled and the key's release rung once a call, the frame's key
    /// release (`grand::KeyNoise`) too. These performances were up to -29.9
    /// dB re peak off the 128-frame take on Piano 1 and -30.2 on Piano 2 at
    /// calls of 1 to 64 frames (0d20e3d1). It runs on a grid of its own now
    /// (`grand::GrandVoice`'s `Ctl`) and the voice renders the same to the bit
    /// at any call (`grand::tests::a_voice_renders_the_same_at_any_host_call`).
    /// At calls of 1, 16, 37, 64 and 1024 frames the takes are now at most
    /// -74.1 dB re peak off the 128-frame take on Piano 1 and -74.8 on Piano
    /// 2 (-80.6 and -85.2 at 64), the stop identical on both. What differs
    /// is the frame's and the harp's, each measured by taking it out:
    /// - STRETCH moved under ringing strings, and the harp's strings glide
    ///   to it a line step a run (`symp`'s `Glide::step_line`), a run ending
    ///   where a call does: the repeats -74.1 at 1-frame calls on Piano 1,
    ///   -105.1 on Piano 2 (the harp's following switched off: identical).
    /// - the frame falls silent at the end of a call, and the harp's frame
    ///   grid and the board resume where that call left them, so the next
    ///   phrase's harp gate falls elsewhere: the gap's second phrase -74.8
    ///   on Piano 2 at 1-frame calls (the shared side made new when the frame
    ///   falls silent: -81.1, the glide's).
    /// - the harp takes a string back at the end of the call its voice
    ///   ended in: the gap -85.4 at 1024-frame calls on Piano 1, the chord
    ///   -95.7 (handed back after one-frame blocks instead: identical).
    /// With all three taken out the takes are the same to the bit but for
    /// the tail a frame falling silent at the end of a longer call renders,
    /// -148 dB. The bound is -70.
    /// These performances do not reach every residual. Every knob taken to
    /// 0, to 1 and back to its default under a pedalled chord struck at each
    /// differs by -61.1 dB re peak at 1-frame calls on Piano 2 at 44.1 kHz
    /// (-65.1 at 48 kHz; the harp's glide, identical with its following
    /// switched off), and silent presses under a pedalled C2 by -44.3 at
    /// 1024-frame calls (the string handed back at the end of the call;
    /// -66.6 handed back after each frame block instead). Measured by the
    /// verifier, not held here; the voice itself renders the same to the bit
    /// in both.
    #[test]
    fn the_grands_render_the_same_at_any_host_call() {
        for patch in [Patch::Piano1, Patch::Piano2] {
            let worst = call_size_difference(patch, &[1, 16, 37, 64, 1024]);
            println!("{patch:?}: at calls of 1, 16, 37, 64 and 1024 frames, {}", worst.map_or("identical to the 128-frame takes".to_string(), |(w, name, call)| format!("at most {w:+.1} dB re peak off the 128-frame takes ({name}, {call}-frame calls)")));
            if let Some((w, name, call)) = worst {
                assert!(w <= -70.0, "{patch:?}: {name} at {call}-frame calls moved {w:+.1} dB re peak off the 128-frame take (bound -70.0)");
            }
        }
    }

    /// **A cleared key noise is a new one** (`grand::KeyNoise`, the frame's
    /// key release for keys whose voices have finished; tested here, beside
    /// the frame that owns it). After key-ups at random keys and times on
    /// either grand, `clear` leaves nothing waiting and nothing ringing: the
    /// same key-ups from there render what a new one renders, to the bit.
    #[test]
    fn a_cleared_key_noise_is_a_new_one() {
        let mut seed = 0x2d4c_6e8f_u32;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32
        };
        let knobs = PianoKnobs::default();
        for trial in 0..10 {
            let base = if trial % 2 == 0 { &grand::GrandBase::INTIMATE } else { &grand::GrandBase::CONCERT };
            let mut used = grand::KeyNoise::new(SR);
            let (mut l, mut r) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK]);
            for _ in 0..(5 + (rnd() * 60.0) as usize) {
                if rnd() < 0.3 {
                    used.key_up(21 + (rnd() * 87.0) as u8, &knobs, base);
                }
                used.render(base, &mut l, &mut r);
            }
            used.clear();
            let mut fresh = grand::KeyNoise::new(SR);
            for b in 0..120 {
                if b == 3 || b == 40 {
                    used.key_up(60, &knobs, base);
                    fresh.key_up(60, &knobs, base);
                }
                let (mut ul, mut ur, mut fl, mut fr) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK], [0.0_f32; BLOCK], [0.0_f32; BLOCK]);
                used.render(base, &mut ul, &mut ur);
                fresh.render(base, &mut fl, &mut fr);
                for i in 0..BLOCK {
                    assert!(ul[i].to_bits() == fl[i].to_bits() && ur[i].to_bits() == fr[i].to_bits(), "trial {trial}: the cleared key noise differs from a new one at block {b}, sample {i}");
                }
                assert_eq!(used.active(), fresh.active(), "trial {trial}: block {b}");
            }
        }
    }

    /// Drive two instruments' shared sides, component by component, with the
    /// same signals and keys, and hold them to the bit: the soundboard, the
    /// harp (on a patch that has one: the harp's pressure is synced to the
    /// pedal only on those, and the others never render it), the fretboard,
    /// the trapwork and the key release, and with `amp` the reed amp and the
    /// wah. A component left with something of its history rings it out
    /// here where the new one is silent.
    fn assert_shared_side_new(k: &mut Keys, fresh: &mut Keys, what: &str, amp: bool) {
        let x = |i: usize| if (2_400..2_600).contains(&i) { 0.3 * ((i as f32 * 0.37).sin() + (i as f32 * 1.13).cos()) } else { 0.0 };
        for i in 0..4_800 {
            let (a, b) = (k.body.tick(x(i)), fresh.body.tick(x(i)));
            assert!(a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits(), "{what}: the soundboard is not new, {i} samples on");
        }
        let (mut ha, mut hb) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK]);
        for blk in 0..if k.has_harp() { 80 } else { 0 } {
            if blk == 2 {
                k.harp.key_down(48);
                fresh.harp.key_down(48);
            }
            let d: Vec<f32> = (0..BLOCK).map(|i| 0.2 * (std::f32::consts::TAU * 130.8 * (i + blk * BLOCK) as f32 / SR).sin()).collect();
            ha.fill(0.0);
            hb.fill(0.0);
            k.harp.render(&d, &mut ha);
            fresh.harp.render(&d, &mut hb);
            assert!(ha.iter().zip(&hb).all(|(a, b)| a.to_bits() == b.to_bits()), "{what}: the harp is not new at block {blk}");
        }
        let gk = guitar::GuitarKnobs::from_params(&k.params);
        let (mut fa, mut fb) = ([0.0_f32; 2 * BLOCK], [0.0_f32; 2 * BLOCK]);
        for blk in 0..80 {
            for (f, out) in [(&mut k.fretboard, &mut fa), (&mut fresh.fretboard, &mut fb)] {
                // A key goes down, as on the harp: the open strings listen
                // from a key on (`guitar::Fretboard::hush`).
                if blk == 2 {
                    let pl = f.place(52);
                    f.take(pl, 52);
                }
                f.set_knobs(&gk);
                f.begin(BLOCK);
                for (i, v) in f.bus(1, BLOCK).iter_mut().enumerate() {
                    *v = if blk < 30 { 0.05 * (std::f32::consts::TAU * 220.0 * (i + blk * BLOCK) as f32 / SR).sin() } else { 0.0 };
                }
                out.fill(0.0);
                let (l, r) = out.split_at_mut(BLOCK);
                f.render(l, r, BLOCK);
            }
            assert!(fa.iter().zip(&fb).all(|(a, b)| a.to_bits() == b.to_bits()), "{what}: the fretboard is not new at block {blk}");
        }
        for i in 0..6_000 {
            if i == 1_000 {
                k.pedal_noise.ring(0.01);
                fresh.pedal_noise.ring(0.01);
            }
            let (a, b) = (k.pedal_noise.tick(), fresh.pedal_noise.tick());
            assert!(a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits(), "{what}: the trapwork is not new, {i} samples on");
        }
        for blk in 0..40 {
            if blk == 1 {
                k.key_noise.key_up(60, &PianoKnobs::default(), &grand::GrandBase::INTIMATE);
                fresh.key_noise.key_up(60, &PianoKnobs::default(), &grand::GrandBase::INTIMATE);
            }
            let (mut al, mut ar, mut bl, mut br) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK], [0.0_f32; BLOCK], [0.0_f32; BLOCK]);
            k.key_noise.render(&grand::GrandBase::INTIMATE, &mut al, &mut ar);
            fresh.key_noise.render(&grand::GrandBase::INTIMATE, &mut bl, &mut br);
            assert!(al.iter().chain(&ar).zip(bl.iter().chain(&br)).all(|(a, b)| a.to_bits() == b.to_bits()), "{what}: the key release is not new at block {blk}");
        }
        for i in 0..if amp { (3.0 * SR) as usize } else { 0 } {
            let v = x(i);
            let (a, b) = if i < 3_000 { (k.reed_amp.dry(v, v, 0.6), fresh.reed_amp.dry(v, v, 0.6)) } else { (k.reed_amp.wet(v, -v, 0.6), fresh.reed_amp.wet(v, -v, 0.6)) };
            assert!(a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits(), "{what}: the reed amp is not new, {i} samples on");
            let (a, b) = (k.wah.process(v, v, 0.5), fresh.wah.process(v, v, 0.5));
            assert!(a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits(), "{what}: the wah is not new, {i} samples on");
            if i % BLOCK == BLOCK - 1 {
                k.reed_amp.end_block();
                fresh.reed_amp.end_block();
            }
        }
    }

    /// **A patch change and the hush's clear leave the shared side new.**
    /// A performance on each patch with a shared side (the pedal thrown and
    /// lifted, a chord, the sostenuto, all notes off in the middle, notes
    /// after it, the amp knobs up), then either a change to every other
    /// patch, or all notes off and the hush's clear: every component of the
    /// shared side then renders what a new instrument's renders (given the
    /// same pedals and patch, which are the foot's and the player's, and are
    /// kept on purpose; after the hush, the knobs, the strings' owners and
    /// the amp side are kept too, see below), and after a change the frame's
    /// own state is a new one's (no voice, corpse, key, catch or hush left;
    /// the tremolo's phase and depth smoother and the knobs at the patch's).
    /// Of the frame's resets the soundboard's was the one that left
    /// something (`a_reset_body_is_a_new_one`).
    #[test]
    fn a_patch_change_and_the_hush_leave_the_shared_side_new() {
        let history = |a: Patch| {
            let mut k = Keys::new(SR);
            k.set_patch(a);
            k.set_param(4, 0.9);
            k.set_param(5, 1.0);
            let chord: [u8; 6] = [40, 47, 52, 56, 59, 64];
            let mut out = [0.0_f32; BLOCK * 2];
            for blk in 0..140 {
                match blk {
                    0 => {
                        k.set_pedals([127, 0, 0]);
                        for p in chord {
                            k.note_on(p, 100.0 / 127.0);
                        }
                    }
                    20 => k.note_off(47),
                    30 => k.set_pedals([127, 127, 0]),
                    40 => k.note_on(72, 0.9),
                    50 => k.all_notes_off(),
                    55 => k.note_on(60, 0.8),
                    70 => k.set_pedals([0, 0, 0]),
                    80 => {
                        k.note_on(84, 0.1);
                        k.note_off(84);
                    }
                    90 => k.set_pedals([127, 0, 0]),
                    100 => k.note_off(60),
                    // The frame's key release rung on the grands, as a key
                    // let up after its voice has finished rings it: the hush
                    // below must fade and clear it with the rest. The history
                    // ends in that hush's rise, so the patch changes under a
                    // rise.
                    132 => {
                        if matches!(a, Patch::Piano1 | Patch::Piano2) {
                            let base = if a == Patch::Piano2 { &grand::GrandBase::CONCERT } else { &grand::GrandBase::INTIMATE };
                            k.key_noise.key_up(61, &PianoKnobs::from_params(&k.params), base);
                            k.sounding = k.sounding.max(1);
                        }
                    }
                    133 => k.all_notes_off(),
                    _ => {}
                }
                out.fill(0.0);
                k.render(&mut out, BLOCK, 2);
            }
            // A patch with no shared side has no hush to run.
            if k.has_shared() {
                assert!(k.hush_rise > 0, "{a:?}: the history did not end in the hush's rise");
            } else {
                assert!(k.hush_left == 0 && !k.hush_due && k.hush_rise == 0 && !k.hush_counted, "{a:?}: a hush ran with no shared side to fade");
            }
            k
        };
        let other = |p: Patch| if p == Patch::Clav { Patch::EpMk1 } else { Patch::Clav };
        for a in [Patch::Piano1, Patch::Piano2, Patch::ElectricGrand, Patch::Guitar, Patch::ClavWah, Patch::Wurli140, Patch::Wurli200, Patch::EpSuitcase] {
            for &b in &Patch::ALL {
                let mut k = history(a);
                // A new instrument with the same foot: the pedal put down on
                // Piano 1, where every part that hears it does.
                let mut fresh = Keys::new(SR);
                fresh.set_pedals([127, 0, 0]);
                fresh.set_patch(other(b));
                fresh.set_patch(b);
                let what = if b == a {
                    // The hush's end: all notes off, then what the hush does
                    // once its fade has run. It keeps the knobs, which are
                    // the player's, so the new instrument is given them too
                    // (STRETCH at 1 strings the grands' harps and the
                    // electric grand's for it), and both have had the second
                    // of audio it takes every string to follow them.
                    k.all_notes_off();
                    k.clear_shared();
                    for (i, v) in k.params.iter().enumerate() {
                        fresh.set_param(i, *v);
                    }
                    for h in [&mut k, &mut fresh] {
                        h.idle(SR as usize);
                    }
                    format!("{a:?} after the hush")
                } else {
                    k.set_patch(b);
                    assert!(k.notes.iter().all(|n| n.pitch.is_none() && !n.held) && k.corpses.iter().all(|c| c.left == 0), "{a:?} -> {b:?}: a voice survived");
                    assert!(k.sounding == 0 && k.hush_left == 0 && !k.hush_due && k.hush_rise == 0, "{a:?} -> {b:?}: still sounding or hushing");
                    assert!(k.key_down == [false; 128] && k.sost_keys == [false; 128], "{a:?} -> {b:?}: a key or a catch survived");
                    assert!(k.trem_phase == 0.0 && !k.trem_knob_set && k.params == b.defaults(), "{a:?} -> {b:?}: the tremolo or the knobs survived");
                    format!("{a:?} -> {b:?}")
                };
                // The hush fades and clears the shared side only: the amp
                // side is the sum's own processing and rings its tail out
                // (the reed amp's room, the wah's follower), as it always has.
                assert_shared_side_new(&mut k, &mut fresh, &what, b != a);
            }
        }
    }

    /// **A knob moved while nothing sounds is heard at its new value from
    /// the next note's first sample.** On the suitcase (the tremolo's depth,
    /// the frame's), the Reed 200 (the dry throb) and the Reed Amp (the wet
    /// throb, through the slap and the room): C4 at knob 5 = 0, let go and
    /// rung out until the instrument is asleep, knob 5 to 1, C4 again at 4
    /// s; against the same take with knob 5 at 1 all along. The voices do
    /// not read knob 5, so from the second note's first sample the two are
    /// the same to the bit. Both smoothers kept their state through the
    /// silence and glided the new depth in: the takes differed there by up
    /// to -20.7 / -22.9 / -20.1 dBFS at 4122d312 (7.0 / 7.9 / 6.4 dB under
    /// the note's peak).
    #[test]
    fn a_knob_moved_in_silence_is_heard_from_the_next_note() {
        for patch in [Patch::EpSuitcase, Patch::Wurli200, Patch::Wurli140] {
            let take = |moved: bool| {
                let mut k = Keys::new(SR);
                k.set_patch(patch);
                k.set_param(5, if moved { 0.0 } else { 1.0 });
                k.note_on(60, 0.8);
                let mut out = vec![0.0_f32; 2 * at(4.5)];
                let mut asleep = false;
                for (b, chunk) in out.chunks_mut(2 * BLOCK).enumerate() {
                    if b == at(0.3) / BLOCK {
                        k.note_off(60);
                    }
                    if b == at(3.9) / BLOCK {
                        asleep = !k.active();
                        k.set_param(5, 1.0);
                    }
                    if b == at(4.0) / BLOCK {
                        k.note_on(60, 0.8);
                    }
                    k.render(chunk, BLOCK, 2);
                }
                (out, asleep)
            };
            let ((a, sa), (b, sb)) = (take(true), take(false));
            assert!(sa && sb, "{patch:?}: still sounding before the second note");
            let s = 2 * at(4.0);
            let worst = a[s..].iter().zip(&b[s..]).fold(0.0_f32, |m, (x, y)| m.max((x - y).abs()));
            assert!(a[s..] == b[s..], "{patch:?}: the knob moved in the silence glided in under the next note (at worst {:+.1} dBFS)", 20.0 * worst.max(1e-30).log10());
        }
    }

    /// **A silent Reed Amp holds nothing.** Across the keyboard (C2, C4, C6)
    /// at velocities from 1 to 127, held from 20 ms to 2 s: once the
    /// instrument has fallen asleep, every line and filter of the amp is
    /// zero. The frame stopped rendering the amp with its lines still
    /// holding -87 to -101 dBFS of the room (4122d312), which came back
    /// under the next note.
    #[test]
    fn a_silent_reed_amp_holds_nothing() {
        for (vel, hold) in [(1u8, 0.05f32), (10, 0.05), (40, 0.3), (80, 0.5), (127, 1.0), (127, 0.02), (5, 2.0)] {
            for key in [36u8, 60, 84] {
                let mut k = Keys::new(SR);
                k.set_patch(Patch::Wurli140);
                k.note_on(key, f32::from(vel) / 127.0);
                let mut out = [0.0_f32; BLOCK * 2];
                let mut blocks = 0;
                while k.active() && blocks < at(8.0) / BLOCK {
                    if blocks == at(hold) / BLOCK {
                        k.note_off(key);
                    }
                    k.render(&mut out, BLOCK, 2);
                    blocks += 1;
                }
                assert!(!k.active(), "key {key} v{vel}: still awake after 8 s");
                let left = k.reed_amp.residual();
                assert!(left == 0.0, "key {key} v{vel} held {hold} s: the silent amp holds {:+.1} dBFS", 20.0 * left.log10());
            }
        }
    }

    /// **The electric grand's harp is damped by its strings' rule.** C3 at
    /// MIDI 100 let go at 1 s with the sustain held at CC 0, 30, 50 and 63:
    /// below the switch at 64 the struck string is damped, and the tail over
    /// 1.3-2.3 s is the CC 0 tail within half a decibel; at 64, 80 and 127
    /// it is one pedalled tail. The harp followed the level while the strings
    /// followed the switch: -63.4/-54.1/-50.8 dBFS at CC 30/50/63 against
    /// -79.8 at 0, the string dead and its sympathetic copy ringing up to 29
    /// dB over its tail (review, F3). Measured: printed.
    #[test]
    fn the_electric_grands_harp_follows_its_strings_dampers() {
        let tail = |cc: u8| {
            let (l, r) = pedal_script(Patch::ElectricGrand, &[(0.0, PedalEv::Cc(0, cc)), (0.05, PedalEv::On(48, 100)), (1.0, PedalEv::Off(48))], 2.4);
            span_db(&l, &r, 1.3, 1.0)
        };
        let (off, on) = (tail(0), tail(64));
        for cc in [30u8, 50, 63, 80, 127] {
            let t = tail(cc);
            let want = if cc >= 64 { on } else { off };
            println!("the electric grand's C3 tail at CC 64 = {cc}: {t:+.1} dBFS (CC 0 {off:+.1}, CC 64 {on:+.1})");
            assert!((t - want).abs() <= 0.5, "CC 64 = {cc}: the tail at {t:+.1} dBFS against {want:+.1}");
        }
    }

    /// **The grands have eighty-eight keys.** On the three harp patches, the
    /// pedal down, a key below A0 or above C8 (MIDI 15, 20, 109, 120) plays
    /// nothing: the take is the pedal's alone, to the bit. And MIDI 15
    /// struck with A0 is A0 alone. Each voice clamped such a key onto its
    /// edge key while the frame and the harp took it as it came: MIDI 15
    /// rang A0 with A0's string asleep (-240 against -63.6 dB of harp), and
    /// struck with MIDI 21 it was a second voice on that one string, 6 dB
    /// over A0 (review, alloc 6).
    #[test]
    fn the_harp_patches_have_eighty_eight_keys() {
        for patch in [Patch::Piano1, Patch::Piano2, Patch::ElectricGrand] {
            let take = |keys: &[u8]| {
                let mut ev = vec![(0.0, PedalEv::Cc(0, 127))];
                ev.extend(keys.iter().map(|&p| (0.0, PedalEv::On(p, 90))));
                pedal_script(patch, &ev, 1.0)
            };
            let pedal = take(&[]);
            for p in [15u8, 20, 109, 120] {
                assert!(take(&[p]) == pedal, "{patch:?}: MIDI {p} sounded");
            }
            assert!(take(&[15, 21]) == take(&[21]), "{patch:?}: MIDI 15 beside A0 sounded");
        }
    }

    /// **A velocity that is not a number is the softest touch there is.** On
    /// every patch, a key (C4; E4 on the guitar) struck at a NaN velocity and
    /// let go at 0.5 s renders exactly what a strike at velocity 0 renders,
    /// and the instrument is asleep a second later. `clamp` passed the NaN
    /// on, and the tine pianos made a voice whose level was NaN: it never
    /// finished, held its slot and kept the instrument awake for good
    /// (review, alloc 10).
    #[test]
    fn a_nan_velocity_is_a_silent_touch() {
        for &patch in &Patch::ALL {
            let key = if patch == Patch::Guitar { 64 } else { 60 };
            let take = |velocity: f32| {
                let mut k = Keys::new(SR);
                k.set_patch(patch);
                k.note_on(key, velocity);
                let mut out = vec![0.0f32; 2 * at(1.5)];
                for (b, chunk) in out.chunks_mut(2 * BLOCK).enumerate() {
                    if b == at(0.5) / BLOCK {
                        k.note_off(key);
                    }
                    k.render(chunk, BLOCK, 2);
                }
                (out, k.active())
            };
            let ((nan, awake), (zero, _)) = (take(f32::NAN), take(0.0));
            assert!(nan == zero, "{patch:?}: a NaN velocity is not velocity 0");
            assert!(!awake, "{patch:?}: a NaN strike held the instrument awake");
        }
    }

    /// **The sostenuto's catch belongs to the instrument it caught.** C4
    /// caught on Piano 1 and let go, the patch changed, then C4 played and
    /// let go on the new patch: it is damped as the new patch damps a key the
    /// sostenuto never caught, the half second after its key-up within a
    /// decibel of the same note played with the sostenuto pressed over
    /// nothing. The catch survived the change and held the tine piano's C4
    /// ringing at -24.6 dBFS (review, F2 and alloc 5). Measured: printed.
    #[test]
    fn the_sostenutos_catch_does_not_survive_a_patch_change() {
        for to in [Patch::EpMk1, Patch::Piano2, Patch::Clav, Patch::Wurli200, Patch::Guitar] {
            let changed = [
                (0.0, PedalEv::On(60, 90)),
                (0.1, PedalEv::Cc(1, 127)),
                (0.2, PedalEv::Off(60)),
                (0.3, PedalEv::Patch(to)),
                (0.5, PedalEv::On(60, 90)),
                (0.6, PedalEv::Off(60)),
            ];
            let (cl, cr) = pedal_script(Patch::Piano1, &changed, 1.6);
            let (rl, rr) = pedal_script(to, &[(0.1, PedalEv::Cc(1, 127)), (0.5, PedalEv::On(60, 90)), (0.6, PedalEv::Off(60))], 1.6);
            let (c, r) = (span_db(&cl, &cr, 1.0, 0.5), span_db(&rl, &rr, 1.0, 0.5));
            println!("Piano 1 -> {to:?}: C4 let go after the change {c:+.1} dBFS, never caught {r:+.1} dBFS");
            assert!((c - r).abs() <= 1.0, "{to:?}: the old patch's catch held the new C4 ({c:+.1} dBFS against {r:+.1})");
        }
    }

    /// **Half pedal reaches the grand's dampers through the frame.** C4 at
    /// MIDI 80 let go at 1 s with CC 64 at 0, 64 and 127: the fundamental
    /// over the next half second falls fastest with no pedal, slowest with
    /// it down, and between with it half down (the reference's: about 75 /
    /// 34 / 10 dB/s, critic uncovered 1; `grand::tests::half_pedalling_
    /// lets_the_dampers_bear_lightly` pins the levels at the voice).
    #[test]
    fn half_pedalling_reaches_the_grand_through_the_frame() {
        let mut rates = Vec::new();
        for cc in [0u8, 64, 127] {
            let (l, r) = pedal_script(Patch::Piano1, &[(0.0, PedalEv::Cc(0, cc)), (0.0, PedalEv::On(60, 80)), (1.0, PedalEv::Off(60))], 1.8);
            let rate = (tone_db(&l, &r, 261.6, 1.05, 0.1) - tone_db(&l, &r, 261.6, 1.55, 0.1)) / 0.5;
            println!("CC 64 = {cc}: C4 falls {rate:.0} dB/s after its key comes up");
            rates.push(rate);
        }
        assert!(rates[0] > rates[1] + 10.0 && rates[1] > rates[2] + 10.0, "half pedal is not between: {rates:?}");
    }

    /// **The soft pedal reaches the grand's hammers through the frame.**
    /// C4 at MIDI 80 with CC 67 up and down: the attack (5-205 ms) a little
    /// quieter under the una corda (the reference's -0.7 dB) and its
    /// upper partials darker against the fundamental (n4 re n1 lower).
    #[test]
    fn the_soft_pedal_reaches_the_grand_through_the_frame() {
        let run = |soft: u8| pedal_script(Patch::Piano1, &[(0.0, PedalEv::Cc(2, soft)), (0.01, PedalEv::On(60, 80))], 0.3);
        let ((l0, r0), (l1, r1)) = (run(0), run(127));
        let rms = |l: &[f32], r: &[f32]| {
            let (a, b) = ((0.015 * SR) as usize, (0.215 * SR) as usize);
            let p: f64 = l[a..b].iter().chain(&r[a..b]).map(|s| f64::from(*s).powi(2)).sum::<f64>() / (2 * (b - a)) as f64;
            10.0 * p.log10()
        };
        let d_rms = rms(&l1, &r1) - rms(&l0, &r0);
        let bright = |l: &[f32], r: &[f32]| tone_db(l, r, 1049.5, 0.015, 0.2) - tone_db(l, r, 261.6, 0.015, 0.2);
        let d_n4 = bright(&l1, &r1) - bright(&l0, &r0);
        println!("una corda: attack {d_rms:+.2} dB, n4 re n1 {d_n4:+.2} dB");
        assert!((-3.0..-0.1).contains(&d_rms), "una corda moved the attack {d_rms:+.2} dB");
        assert!(d_n4 < -0.5, "una corda did not darken the attack ({d_n4:+.2} dB)");
    }

    /// The power of both channels from `from` for `len` seconds, dBFS.
    fn span_db(l: &[f32], r: &[f32], from: f32, len: f32) -> f32 {
        let (a, b) = ((from * SR) as usize, ((from + len) * SR) as usize);
        let p: f64 = l[a..b].iter().chain(&r[a..b]).map(|s| f64::from(*s).powi(2)).sum::<f64>() / (2 * (b - a)) as f64;
        (10.0 * p.max(1e-30).log10()) as f32
    }

    /// **A key comes up with its noise even when its note has died.** The
    /// grand's key release is one event per key-up (PX-10); a soft treble
    /// note is over long before its key is let go, its voice finished and
    /// its slot handed back, and the frame rings the event the voice can no
    /// longer (`grand::KeyNoise`). C8 at MIDI 20 held three seconds: the 300
    /// ms after key-up carry the release (the verifier's: digital silence;
    /// the voice's own key-ups measure -80 dBFS on Piano 1, -88 on Piano 2
    /// there, `relhold`); a note-off for a key that was never down rings
    /// nothing. Measured: printed.
    #[test]
    fn a_key_comes_up_with_its_noise_after_its_note_has_died() {
        for patch in [Patch::Piano1, Patch::Piano2] {
            let (l, r) = pedal_script(patch, &[(0.0, PedalEv::On(108, 20)), (3.0, PedalEv::Off(108))], 3.6);
            let died = span_db(&l, &r, 2.6, 0.3);
            let release = span_db(&l, &r, 3.0, 0.3);
            println!("{patch:?}: C8 MIDI 20 before its key-up {died:+.1} dBFS, the key-up's 300 ms {release:+.1} dBFS");
            assert!(died < -150.0, "{patch:?}: the C8 had not died ({died:+.1} dBFS)");
            assert!((-95.0..-70.0).contains(&release), "{patch:?}: the key-up's noise {release:+.1} dBFS");
            let (ls, rs) = pedal_script(patch, &[(0.0, PedalEv::Off(64))], 0.5);
            let stray = span_db(&ls, &rs, 0.0, 0.45);
            assert!(stray < -200.0, "{patch:?}: a key never pressed came up with a noise ({stray:+.1} dBFS)");
        }
    }

    /// **A key comes up with its noise wherever its voice ends.** A key-up
    /// ends the grand's control block on its own frame (`grand::GrandVoice`'s
    /// `Ctl`), and the block's end can find the voice finished there. Two
    /// such keys, each event on its own frame at 64-frame calls: a soft E7
    /// (MIDI 8) held to its voice's end under the finger and let up 1 to 127
    /// frames before it, and a silent press (MIDI 0) let up 1 to 127 frames
    /// after it. Every key-up's 400 ms carry the key's release, within 1 dB
    /// of the same key let up a block before that end (the voice rings it)
    /// or two blocks after the press (the frame does, `grand::KeyNoise`).
    /// Asked whether its voice was sounding before the key came up, the
    /// frame left the release to a voice the key-up then finished, and
    /// neither rang it: over those 400 ms the E7 read -122.3 dBFS on Piano 1
    /// and -142.3 on Piano 2, where its release reads -84.3 and -89.4, and
    /// the silent press digital silence (verifier; 0d20e3d1, which judged a
    /// voice at the end of a call, rang every one). Measured: printed.
    #[test]
    fn a_key_comes_up_with_its_noise_wherever_its_voice_ends() {
        let after = (0.4 * SR) as usize;
        let span = |t: &[f32], from: usize| {
            let p: f64 = t[2 * from..2 * (from + after)].iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / (2 * after) as f64;
            (10.0 * p.max(1e-30).log10()) as f32
        };
        let quietest = |db: &dyn Fn(usize) -> f32, at: &[usize]| at.iter().map(|&a| (db(a), a)).fold((f32::MAX, 0), |w, x| if x.0 < w.0 { x } else { w });
        for patch in [Patch::Piano1, Patch::Piano2] {
            // Held: the frame the E7's voice finishes on, the end of a block
            // from its strike, where 128-frame calls from the strike free
            // its slot (within ten seconds).
            let (key, vel) = (100u8, 8u8);
            let mut k = Keys::new(SR);
            k.set_patch(patch);
            k.note_on(key, f32::from(vel) / 127.0);
            let mut out = [0.0_f32; 2 * BLOCK];
            let mut end = 0;
            while k.notes.iter().any(|n| n.pitch == Some(key)) && end < (10.0 * SR) as usize {
                out.fill(0.0);
                k.render(&mut out, BLOCK, 2);
                end += BLOCK;
            }
            assert!(k.notes.iter().all(|n| n.pitch != Some(key)), "{patch:?}: the E7 MIDI {vel} held ten seconds had not finished");
            let held = |up: usize| span(&at_calls(patch, &[(0, PedalEv::On(key, vel)), (up, PedalEv::Off(key))], up + after, 64), up);
            let held_ref = held(end - BLOCK);
            let (held_db, early) = quietest(&|e| held(end - e), &[1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 127]);
            let silent = |up: usize| span(&at_calls(patch, &[(0, PedalEv::On(60, 0)), (up, PedalEv::Off(60))], up + after, 64), up);
            let silent_ref = silent(2 * BLOCK);
            let (silent_db, late) = quietest(&silent, &[1, 2, 5, 13, 34, 89, 127]);
            println!("{patch:?}: the 400 ms after a key-up: the E7 MIDI 8 (its voice ends at frame {end}) let up a block before its end {held_ref:+.1} dBFS, 1 to 127 frames before at least {held_db:+.1} ({early} before); a silent press let up two blocks after {silent_ref:+.1}, 1 to 127 frames after at least {silent_db:+.1} ({late} after)");
            assert!(held_db >= held_ref - 1.0, "{patch:?}: the E7 let up {early} frames before its voice's end came up at {held_db:+.1} dBFS against {held_ref:+.1}");
            assert!(silent_db >= silent_ref - 1.0, "{patch:?}: a silent press let up {late} frames after came up at {silent_db:+.1} dBFS against {silent_ref:+.1}");
        }
    }

    /// **Half pedal lands the same however the pedal got there.** C4 at
    /// MIDI 80 let go under the full pedal, then the pedal brought to CC 40
    /// in one message: over the half second from 0.3 s on its fundamental
    /// falls within a quarter of the rate it falls at when its key is let go
    /// with the pedal already at 40 (the reference's half pedal), and well
    /// under the rate a full damper takes it at (the pedal straight to 0).
    /// The frame's pedal-up damping used to drop every released grand damper
    /// with its full weight as CC 64 crossed 64, after the level had set the
    /// half-pedalled pressure: one message to 40 fell at -586 dB/s where the
    /// key let go at 40 fell at -178 (verifier). A pedal stepped down to 40
    /// is printed beside them: it reaches the same dampers, and crosses 64,
    /// where the harp's and the board's dampers drop together, 128 ms later.
    /// Measured: printed.
    #[test]
    fn half_pedal_lands_the_same_however_the_pedal_got_there() {
        // The fundamental's fall from 80 ms after the event, a line through
        // its level every 20 ms over `span` seconds: early, while every mode
        // of the course stands well over the bank's cull (a fall read into
        // the cull reads which modes it took; the first draft of this test
        // read 2.3-2.8 s and found the one-message path's slow modes culled
        // first because it had been damped 0.1 s longer).
        let run = |ev: &[(f32, PedalEv)], event: f32, span: f32| {
            let (l, r) = pedal_script(Patch::Piano1, ev, event + 0.1 + span + 0.1);
            let pts: Vec<(f32, f32)> = (0..=(span / 0.02) as usize).map(|i| {
                let t = event + 0.08 + 0.02 * i as f32;
                (t, tone_db(&l, &r, 261.6, t, 0.05))
            }).collect();
            let n = pts.len() as f32;
            let (mt, me) = (pts.iter().map(|p| p.0).sum::<f32>() / n, pts.iter().map(|p| p.1).sum::<f32>() / n);
            let k = pts.iter().map(|p| (p.0 - mt) * (p.1 - me)).sum::<f32>() / pts.iter().map(|p| (p.0 - mt).powi(2)).sum::<f32>();
            -k
        };
        let pedalled = |cc: &[(f32, u8)], span: f32| {
            let mut ev = vec![(0.0, PedalEv::Cc(0, 127)), (0.1, PedalEv::On(60, 80)), (1.0, PedalEv::Off(60))];
            ev.extend(cc.iter().map(|&(t, v)| (t, PedalEv::Cc(0, v))));
            run(&ev, 2.0, span)
        };
        let one = pedalled(&[(2.0, 40)], 0.24);
        let steps: Vec<(f32, u8)> = (0..=87).map(|i| (2.0 + 0.002 * i as f32, 127 - i as u8)).collect();
        let stepped = run(&[(0.0, PedalEv::Cc(0, 127)), (0.1, PedalEv::On(60, 80)), (1.0, PedalEv::Off(60))].iter().cloned().chain(steps.iter().map(|&(t, v)| (t, PedalEv::Cc(0, v)))).collect::<Vec<_>>(), 2.18, 0.24);
        let full = pedalled(&[(2.0, 0)], 0.12);
        let at40 = run(&[(0.0, PedalEv::Cc(0, 40)), (0.1, PedalEv::On(60, 80)), (1.0, PedalEv::Off(60))], 1.0, 0.24);
        println!("C4 n1: let go at CC 40 {at40:.0} dB/s; under the pedal, then to CC 40 in one message {one:.0} dB/s, stepped (read from its last step) {stepped:.0}; to 0 {full:.0} dB/s");
        assert!((one - at40).abs() <= 0.25 * at40.abs().max(one.abs()), "the pedal brought to 40 damps at {one:.0} dB/s, a key let go at 40 at {at40:.0}");
        assert!((stepped - one).abs() <= 0.25 * stepped.abs().max(one.abs()), "the stepped pedal damps at {stepped:.0} dB/s, one message at {one:.0}");
        assert!(one < 0.6 * full, "CC 40 damped as hard as no pedal: {one:.0} against {full:.0} dB/s");
    }

    /// **The trapwork thumps once a throw.** A sustain pedal fluttering
    /// about its throw point (CC 90 to 100, `INTIMATE.pedal_lift[1]` is 96.5
    /// of 127) ten times a second throws the dampers once: the thump's
    /// energy over the second is one press's, not ten (the verifier's: every
    /// upward crossing rang it). Piano 1, nothing struck. Measured: printed.
    #[test]
    fn the_trapwork_thumps_once_a_throw() {
        let once = {
            let (l, r) = pedal_script(Patch::Piano1, &[(0.05, PedalEv::Cc(0, 100))], 1.2);
            span_db(&l, &r, 0.0, 1.2)
        };
        let mut ev = Vec::new();
        for i in 0..10 {
            ev.push((0.05 + 0.1 * i as f32, PedalEv::Cc(0, 100)));
            ev.push((0.1 + 0.1 * i as f32, PedalEv::Cc(0, 90)));
        }
        let (l, r) = pedal_script(Patch::Piano1, &ev, 1.2);
        let flutter = span_db(&l, &r, 0.0, 1.2);
        println!("one press {once:+.1} dBFS, ten flutters about the throw {flutter:+.1} dBFS");
        assert!((flutter - once).abs() <= 1.0, "the flutter thumped {:+.1} dB over one throw", flutter - once);
    }

    /// **The trapwork's throw is the pedal's, whatever patch it moved under.**
    /// Piano 1's pedal down, the patch changed to the Clav, the pedal up
    /// there, back to Piano 1 and the pedal down again: the throw thumps as
    /// on a fresh Piano 1, the 0.6 s from it within a decibel. And the pedal
    /// put down on the Clav, the patch changed to Piano 1, the pedal lifted:
    /// the landing thumps as on a fresh one. The throw was followed only
    /// while Piano 1 was loaded, and both read -300 dBFS against -73.5 and
    /// -74.8 (review, F7). Measured: printed.
    #[test]
    fn the_trapworks_throw_follows_the_pedal_on_every_patch() {
        let thump = |patch: Patch, ev: &[(f32, PedalEv)], from: f32| {
            let (l, r) = pedal_script(patch, ev, from + 0.7);
            span_db(&l, &r, from, 0.6)
        };
        let throw = thump(Patch::Piano1, &[(0.05, PedalEv::Cc(0, 127))], 0.05);
        let tripped = thump(
            Patch::Piano1,
            &[(0.05, PedalEv::Cc(0, 127)), (0.6, PedalEv::Patch(Patch::Clav)), (0.7, PedalEv::Cc(0, 0)), (0.8, PedalEv::Patch(Patch::Piano1)), (1.0, PedalEv::Cc(0, 127))],
            1.0,
        );
        let land = thump(Patch::Piano1, &[(0.05, PedalEv::Cc(0, 127)), (1.0, PedalEv::Cc(0, 0))], 1.0);
        let landed = thump(Patch::Clav, &[(0.05, PedalEv::Cc(0, 127)), (0.6, PedalEv::Patch(Patch::Piano1)), (1.0, PedalEv::Cc(0, 0))], 1.0);
        println!("the throw {throw:+.1} dBFS fresh, {tripped:+.1} after a trip through the Clav; the landing {land:+.1} fresh, {landed:+.1} with the pedal put down on the Clav");
        assert!((tripped - throw).abs() <= 1.0, "the throw after a trip through the Clav: {tripped:+.1} dBFS against {throw:+.1}");
        assert!((landed - land).abs() <= 1.0, "the landing of a pedal put down on the Clav: {landed:+.1} dBFS against {land:+.1}");
    }

    /// **A knob that is not a number is its default.** `set_param` with a
    /// NaN puts the patch's default there (a NaN BODY knob reached the
    /// grand's output through `body_amt`; verifier), and the instrument
    /// stays finite.
    #[test]
    fn a_knob_that_is_not_a_number_is_its_default() {
        let mut k = Keys::new(SR);
        k.set_patch(Patch::Piano1);
        for i in 0..MAX_PARAMS {
            k.set_param(i, f32::NAN);
        }
        assert_eq!(k.params(), &Patch::Piano1.defaults(), "a NaN knob kept its NaN");
        k.note_on(60, 0.8);
        let mut out = vec![0.0_f32; BLOCK * 2 * 40];
        k.render(&mut out, BLOCK * 40, 2);
        assert!(out.iter().all(|s| s.is_finite()) && out.iter().any(|s| *s != 0.0), "the instrument went non-finite or silent");
    }

    /// **A fortissimo C4 sits 7 dB over where the waveguide piano's did.**
    /// The grand's own level (`GrandBase::gain`) is still the waveguide
    /// grand's, whose C4 at MIDI 127 put -24.6 dBFS into its first 300 ms
    /// through the frame (both channels' power; the base cc701c73 and the
    /// concert measure the same); Piano 1's refit had it at -21.6, 3 dB
    /// over, and its pedalled fortissimo chords went over full scale (+2.9
    /// dBFS, 199 samples; verifier). The frame then lifts both pianos 7 dB,
    /// level with the other sounds at forte (`Patch::lift`): -17.6. Both
    /// pianos within half a decibel of it, and of -24.6 with the lift taken
    /// out. Measured: printed.
    #[test]
    fn a_fortissimo_c4_sits_seven_db_over_the_waveguide_piano() {
        for patch in [Patch::Piano1, Patch::Piano2] {
            let (l, r) = pedal_script(patch, &[(0.0, PedalEv::On(60, 127))], 0.4);
            let rms = span_db(&l, &r, 0.0, 0.3);
            let own = rms - 20.0 * patch.lift().log10();
            println!("{patch:?}: C4 MIDI 127, the first 300 ms {rms:+.2} dBFS ({own:+.2} before the lift)");
            assert!((rms + 17.6).abs() <= 0.5, "{patch:?}: a fortissimo C4 at {rms:+.2} dBFS");
            assert!((own + 24.6).abs() <= 0.5, "{patch:?}: the grand's own fortissimo C4 at {own:+.2} dBFS");
        }
    }

    /// **The ten sounds sit together at forte.** The same six-note chord (C3
    /// G3 C4 E4 G4 C5 at MIDI 100, 9 ms between fingers, the pedal up) on
    /// each sound through the frame: the first 500 ms RMS, both channels,
    /// within 6 dB from the loudest sound to the quietest (measured 2.7:
    /// -14.1 dBFS, EP Mk.1, to -16.8, the guitar). Before the lift
    /// (`Patch::lift`) the two grands, the electric grand and the guitar sat
    /// 5.6 to 7.6 dB under the middle of the other six on this chord and the
    /// ten spanned 9.7, so a sound voiced away from the others, or a lift
    /// lost, shows here. And it is one, exactly, for the other six.
    /// Measured: printed.
    #[test]
    fn the_ten_sounds_sit_together_at_forte() {
        let chord = [48u8, 55, 60, 64, 67, 72];
        let mut got = Vec::new();
        for patch in Patch::ALL {
            let ev: Vec<(f32, PedalEv)> = chord.iter().enumerate().map(|(i, &p)| (0.009 * i as f32, PedalEv::On(p, 100))).collect();
            let (l, r) = pedal_script(patch, &ev, 0.6);
            let rms = span_db(&l, &r, 0.0, 0.5);
            println!("{patch:?}: the chord's first 500 ms {rms:+.2} dBFS (lift {:+.1} dB)", 20.0 * patch.lift().log10());
            got.push(rms);
        }
        let (lo, hi) = got.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        assert!(hi - lo <= 6.0, "the ten sounds span {:.1} dB at forte ({lo:+.1} to {hi:+.1} dBFS)", hi - lo);
        for patch in Patch::ALL {
            let lifted = matches!(patch, Patch::Piano1 | Patch::Piano2 | Patch::ElectricGrand | Patch::Guitar);
            assert_eq!(patch.lift() == 1.0, !lifted, "{patch:?}: lifted {}", patch.lift());
        }
    }
}
