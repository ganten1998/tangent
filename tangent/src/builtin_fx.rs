//! Tangent's own effects, seated in ordinary insert bays.
//!
//! The instruments proved the sentinel idea — a path beginning with `@`
//! travels through the picker, the settings, the saved session and the
//! reconcile exactly as a plugin's bundle path does, and only the loader ever
//! looks inside it. The instruments then step OUT of the rack (masks and
//! `mix_channels` render them), which was right for bay-0 voices and wrong for
//! effects: an effect belongs in the chain, wherever the chain is — a channel,
//! a folder, the master, a drum pad. So an effect sentinel goes the other way:
//! [`crate::instrument`]'s `load_insert` builds a [`BuiltinUnit`] and seats it
//! in the bay as an ordinary `Hosted { voice: false }`, and the hand-off
//! rings, `run_rack`, delay compensation, reorders and export all treat it as
//! one more plugin that happens to be ours.
//!
//! An enum rather than a trait object, deliberately: the audio thread already
//! dispatches `Backend` by match, the set of units is closed and compiled in,
//! and a `Box<dyn>` would buy nothing but an allocation and an indirect call
//! per block. Adding a unit is its DSP struct, one [`UnitSpec`] row, and one
//! constructor — the compiler names every other place by exhaustiveness.
//!
//! Parameters are normalised 0..=1 end to end, the same contract the effects
//! chain and the automation lanes already speak; the mapping to what the DSP
//! actually wants lives in a const beside the DSP, per the house rule in
//! `effects.rs`. Design arithmetic is `f64`, the audio path `f32`.
//!
//! The first unit is a stereo trim — deliberately the smallest possible DSP,
//! because its job is to prove the road: load, process, automate, save,
//! restore. It is not in `dialogs::FX_UNITS` and no picker offers it yet.

// ── the units on offer ──────────────────────────────────────────────────────

/// The trim's sentinel path. Like the instruments' sentinels it begins with a
/// character no filesystem path does, so it can never collide with a bundle
/// somebody has installed — `dialogs::is_builtin` recognises it unedited.
pub const TRIM_PATH: &str = "@tangent-trim";

/// The suite the trim proved the road for: the four kernels under
/// `crate::units`, each seated by its own sentinel. Same naming law as
/// [`TRIM_PATH`]; the strings are persisted into every document that loads
/// one, so they are as append-only as the param ids below.
pub const EQ_PATH: &str = "@tangent-eq";
pub const IMAGER_PATH: &str = "@tangent-imager";
pub const TAPE_PATH: &str = "@tangent-tape";
pub const COMP_PATH: &str = "@tangent-comp";

/// The rest of the shelf: the nine kernels registered after the first four
/// proved the road. Three of these sentinels — the chorus, the echo and the
/// plate — were declared in `dialogs::FX_UNITS` long before the units
/// existed, waiting for exactly this registration, so those strings are
/// reused rather than re-invented: they are the same effects the fixed
/// chain ships, now seatable in any bay.
pub const DENOISE_PATH: &str = "@tangent-denoise";
pub const AMP_PATH: &str = "@tangent-amp";
pub const DRIVE_PATH: &str = "@tangent-drive";
pub const TREM_PATH: &str = "@tangent-trem";
pub const CHORUS_PATH: &str = "@tangent-chorus";
pub const ECHO_PATH: &str = "@tangent-echo";
pub const PLATE_PATH: &str = "@tangent-plate";
pub const WAH_PATH: &str = "@tangent-wah";
pub const IR_PATH: &str = "@tangent-ir";
/// **The first built-in VOICE.** Seated by `load_insert` exactly as the
/// effects above are — the hand-off rings, `run_rack`, delay compensation,
/// state and automation all treat it as one more tenant — and the one row
/// in [`UNITS`] with `voice: true`, which is what routes the block's notes
/// to it and its output over the channel. Deliberately NOT in
/// `desktop::is_instrument_sentinel`: that list names the three mask-rendered
/// instruments, and adding this one there would filter it out of the engine
/// — a synth that loads, names itself, draws its panel and is silent. See
/// `docs/ANALOG-BRIEF.md` §7.
pub const ANALOG_PATH: &str = "@tangent-analog";

/// **The second built-in VOICE**, and the first that plays somebody else's
/// audio. Seated exactly as the analog is — `voice: true` in [`UNITS`] is
/// what routes the block's notes to it and takes its output as the channel's
/// signal — and deliberately NOT in `desktop::is_instrument_sentinel`, for
/// the reason spelled out above [`ANALOG_PATH`]: that list names the three
/// mask-rendered instruments, and a voice added to it is filtered out of the
/// engine and plays nothing.
///
/// Its audio does NOT travel in its state blob. The file is named there (the
/// extra symbols `sample_path` and `root_key`, the convolver's `ir_path`
/// road) and the samples themselves reach the audio thread the way a drum kit
/// does — see `instrument::SamplerSample`.
pub const SAMPLER_PATH: &str = "@tangent-sampler";

/// **The bitcrusher**, asked for by name ("I'd also like a bitcrusher plugin
/// Tangent Bitcrush") and registered after the shelf was full: an effect,
/// seated like the drive and the trem, with its kernel in
/// [`crate::units::bitcrush`]. Same naming law as every sentinel above.
pub const BITCRUSH_PATH: &str = "@tangent-bitcrush";

/// **Tangent Insert**: the bay whose effect is outboard gear, reached
/// through the interface Tangent is playing on. Its knobs are the two trims,
/// the mix and the polarity; its jacks and its measured loop are extra state
/// (`units::hwinsert::EXTRA_SYMBOLS`), because a device channel is not a
/// number anybody should automate. See `units::hwinsert` for the road.
pub const HWINSERT_PATH: &str = "@tangent-insert";

/// One parameter a unit publishes: what the automation lane, the state blob
/// and (one day) a knob panel all address it by.
///
/// `id` and `symbol` are both persisted — the id into automation curves as
/// `fx:<bay>:<id>`, the symbol into the state sidecar — so both are
/// append-only forever, exactly as `AutoParam` documents for its own space.
#[derive(Clone, Copy)]
pub struct ParamSpec {
    /// The lane's address. Stable for the life of the product; new
    /// parameters take new ids and dead ones retire their number.
    pub id: u32,
    /// The state blob's key. Same append-only law as the id, and the same
    /// reason LV2 keys state by port symbol: a value must never move one
    /// knob to the left because a parameter was added.
    pub symbol: &'static str,
    /// What the automation lane calls it.
    pub name: &'static str,
    /// "dB", "Hz", "%" — what the value READS in, not what it is stored in.
    pub units: &'static str,
    /// Where the knob rests, normalised. A curve with no points reads as this.
    pub default: f32,
    /// 0 continuous, 2 a toggle, otherwise the number of steps.
    pub steps: i32,
}

/// One built-in unit's identity: everything the loader needs to seat it and
/// everything the desk needs to name it. **Adding a unit is one row here** —
/// the loader, the state pipeline and the automation lanes read the table.
pub struct UnitSpec {
    pub path: &'static str,
    /// What the bay reports as the plugin's class — the chip label and the
    /// error messages both come from this.
    pub class: &'static str,
    pub params: &'static [ParamSpec],
    /// A plain fn pointer rather than a trait method, so the table stays a
    /// `const` and the dispatch stays visible.
    build: fn(f32) -> BuiltinUnit,
    /// **Whether this unit GENERATES rather than transforms** — the same
    /// flag `instrument::Hosted` carries for a VST3 instrument, decided
    /// here for a built-in. `true` means `run_rack` hands the bay the
    /// block's notes, controls and automation and takes its output as the
    /// channel's signal; `false` means the channel's signal goes in and
    /// comes back transformed. Appended, this struct's law; `false` on
    /// every effect and `true` on the analog alone.
    pub voice: bool,
}

const TRIM_PARAMS: &[ParamSpec] = &[ParamSpec {
    id: 0,
    symbol: "gain",
    name: "Gain",
    units: "dB",
    default: 0.5,
    steps: 0,
}];

/// The EQ face's slice of the kernel: FIVE bands of the eight
/// `units::eq::EqKernel` carries, five knobs each — on, shape, frequency,
/// gain, width. Ids run dense from 0 in band-major order, which is what lets
/// `EqUnit::set_param` decode them by division instead of a lookup; both
/// spaces are persisted, so bands six to eight, when a bigger face wants
/// them, take ids 25 upward and never renumber these.
///
/// **Bands four and five were APPENDED at ids 15 upward.** The note above,
/// written when there were three, promised exactly that, and the promise is
/// what makes it a non-event: every project and preset ever saved names its
/// values by SYMBOL in a full dump (`state_text`), so ids 0..15 still mean
/// what they meant, and the two new bands arrive OFF. An old EQ loaded into
/// the bigger face sounds identical and has two more parked handles.
///
/// **The parking spots are the k/(n+1) ruler, and all five moved to keep
/// it.** Three handles sat at the quarter points — 1/4, 2/4, 3/4 of the log
/// sweep — and the rule was already "n handles, evenly spaced INSIDE the
/// range". Five handles is the same rule with n = 5: sixths. The first cut
/// at this left the original three where they were and hung the new pair off
/// the ends, which put four of the five in the bass half and read as an
/// afterthought; the owner asked for the obvious thing — "5 eq nodes need to
/// be evenly distributed" — and the obvious thing is the rule this table was
/// already following.
///
/// Moving a DEFAULT is safe here and would not be everywhere: `state_text`
/// writes every parameter by name, so a saved bay restores its own numbers
/// and never inherits a new default. What changes is only where a freshly
/// inserted EQ opens — about 63, 200, 632, 2000 and 6320 Hz, left to right,
/// so the band walker now walks up the spectrum in order.
///
/// The rest are the kernel's shipped sound: every band OFF, gain centred,
/// width at Butterworth. The ruler is spelled as the arithmetic that makes
/// it rather than as five decimals, because the next person to add a band
/// has to move all of them again and the rule is the thing to read.

const EQ_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "b1_on", name: "Band 1 On", units: "", default: 0.0, steps: 2 },
    ParamSpec { id: 1, symbol: "b1_kind", name: "Band 1 Type", units: "", default: 0.0, steps: EQ_KIND_STEPS },
    ParamSpec { id: 2, symbol: "b1_freq", name: "Band 1 Freq", units: "Hz", default: 1.0 / 6.0, steps: 0 },
    ParamSpec { id: 3, symbol: "b1_gain", name: "Band 1 Gain", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 4, symbol: "b1_q", name: "Band 1 Q", units: "", default: crate::units::eq::EQ_Q_BUTTERWORTH, steps: 0 },
    ParamSpec { id: 5, symbol: "b2_on", name: "Band 2 On", units: "", default: 0.0, steps: 2 },
    ParamSpec { id: 6, symbol: "b2_kind", name: "Band 2 Type", units: "", default: 0.0, steps: EQ_KIND_STEPS },
    ParamSpec { id: 7, symbol: "b2_freq", name: "Band 2 Freq", units: "Hz", default: 2.0 / 6.0, steps: 0 },
    ParamSpec { id: 8, symbol: "b2_gain", name: "Band 2 Gain", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 9, symbol: "b2_q", name: "Band 2 Q", units: "", default: crate::units::eq::EQ_Q_BUTTERWORTH, steps: 0 },
    ParamSpec { id: 10, symbol: "b3_on", name: "Band 3 On", units: "", default: 0.0, steps: 2 },
    ParamSpec { id: 11, symbol: "b3_kind", name: "Band 3 Type", units: "", default: 0.0, steps: EQ_KIND_STEPS },
    ParamSpec { id: 12, symbol: "b3_freq", name: "Band 3 Freq", units: "Hz", default: 3.0 / 6.0, steps: 0 },
    ParamSpec { id: 13, symbol: "b3_gain", name: "Band 3 Gain", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 14, symbol: "b3_q", name: "Band 3 Q", units: "", default: crate::units::eq::EQ_Q_BUTTERWORTH, steps: 0 },
    ParamSpec { id: 15, symbol: "b4_on", name: "Band 4 On", units: "", default: 0.0, steps: 2 },
    ParamSpec { id: 16, symbol: "b4_kind", name: "Band 4 Type", units: "", default: 0.0, steps: EQ_KIND_STEPS },
    ParamSpec { id: 17, symbol: "b4_freq", name: "Band 4 Freq", units: "Hz", default: 4.0 / 6.0, steps: 0 },
    ParamSpec { id: 18, symbol: "b4_gain", name: "Band 4 Gain", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 19, symbol: "b4_q", name: "Band 4 Q", units: "", default: crate::units::eq::EQ_Q_BUTTERWORTH, steps: 0 },
    ParamSpec { id: 20, symbol: "b5_on", name: "Band 5 On", units: "", default: 0.0, steps: 2 },
    ParamSpec { id: 21, symbol: "b5_kind", name: "Band 5 Type", units: "", default: 0.0, steps: EQ_KIND_STEPS },
    ParamSpec { id: 22, symbol: "b5_freq", name: "Band 5 Freq", units: "Hz", default: 5.0 / 6.0, steps: 0 },
    ParamSpec { id: 23, symbol: "b5_gain", name: "Band 5 Gain", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 24, symbol: "b5_q", name: "Band 5 Q", units: "", default: crate::units::eq::EQ_Q_BUTTERWORTH, steps: 0 },
];

/// How many positions the band-shape menu has. Spelled as a literal so the
/// table above stays `const`; a test pins it to `NodeKind::ALL.len()`, the
/// enum it steps through.
const EQ_KIND_STEPS: i32 = 6;

/// How many of the kernel's bands the face exposes, and how many knobs each
/// band takes in the id space.
const EQ_FACE_NODES: usize = 5;
const EQ_KNOBS_PER_NODE: u32 = 5;

/// One knob per `units::tape::TapeParams` field, ids in declaration order:
/// the six 5.0 shipped, then the ones appended after them, which never move.
/// Defaults are `TapeParams::default()` verbatim, a fresh machine at the
/// studio speed lined up at its tape's datasheet level, and a test holds the
/// two tables together. Drive opens at 0.5 and Roll at 0 (out of circuit)
/// for a fresh bay only: a saved one keeps every value it was saved with,
/// because its state writes every symbol, so neither needs an
/// [`absent_reads_as`] entry.
///
/// Id 3 keeps 5.0's symbol, `wobble`, and is named Wow: it drove wow and
/// flutter together and drives the slow half alone now, Flutter (id 7) the
/// fast half, and Rate (id 8) how fast every rotating part turns. Wow Range
/// (id 9) moves the wow onto a second play head 25 ms back, a cassette's,
/// and is the one control that moves the delay the seat reports. Bias
/// (id 10) is the record bias either side of the aligned point, which its
/// middle is, and reads signed about it. Age (id 11) is an old tape on a
/// worn head, and Damage (id 12) what the tape has been through, laid along
/// it per metre; both open new. A session saved before these
/// existed reads them at their defaults, which is the owner's ruling for
/// every appended control: nothing is pinned to how 5.0 sounded.
const TAPE_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "drive", name: "Drive", units: "%", default: 0.5, steps: 0 },
    ParamSpec { id: 1, symbol: "bump", name: "Bump", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 2, symbol: "roll", name: "Roll", units: "Hz", default: 0.0, steps: 0 },
    ParamSpec { id: 3, symbol: "wobble", name: "Wow", units: "%", default: 0.3, steps: 0 },
    ParamSpec { id: 4, symbol: "hiss", name: "Hiss", units: "%", default: 0.25, steps: 0 },
    ParamSpec { id: 5, symbol: "trim", name: "Trim", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 6, symbol: "speed", name: "Speed", units: "ips", default: 2.0 / 3.0, steps: crate::units::tape::TAPE_SPEED_STEPS },
    ParamSpec { id: 7, symbol: "flutter", name: "Flutter", units: "%", default: 0.3, steps: 0 },
    ParamSpec { id: 8, symbol: "rate", name: "Rate", units: "x", default: 0.5, steps: 0 },
    ParamSpec { id: 9, symbol: "wow_range", name: "Wow Range", units: "", default: 0.0, steps: crate::units::tape::WOW_RANGE_STEPS },
    ParamSpec { id: 10, symbol: "bias", name: "Bias", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 11, symbol: "age", name: "Age", units: "%", default: 0.0, steps: 0 },
    ParamSpec { id: 12, symbol: "damage", name: "Damage", units: "%", default: 0.0, steps: 0 },
];

/// One knob per `units::imager::ImagerParams` field. Defaults are the
/// kernel's — a wire, because the first thing anybody does with an image
/// tool is listen for what it ruins.
const IMAGER_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "width", name: "Width", units: "%", default: 0.5, steps: 0 },
    ParamSpec { id: 1, symbol: "ensemble", name: "Ensemble", units: "%", default: 0.0, steps: 0 },
    ParamSpec { id: 2, symbol: "mono_below", name: "Mono Below", units: "Hz", default: 0.0, steps: 0 },
];

/// One knob per `units::comp::CompParams` field. Defaults are the kernel's
/// shipped "held" sound: −18 dB, 4 : 1, 6 ms on, 150 ms off, fully wet.
const COMP_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "threshold", name: "Threshold", units: "dB", default: 0.7, steps: 0 },
    ParamSpec { id: 1, symbol: "ratio", name: "Ratio", units: ":1", default: 0.463, steps: 0 },
    ParamSpec { id: 2, symbol: "attack", name: "Attack", units: "ms", default: 0.6, steps: 0 },
    ParamSpec { id: 3, symbol: "release", name: "Release", units: "ms", default: 0.5, steps: 0 },
    ParamSpec { id: 4, symbol: "makeup", name: "Makeup", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 5, symbol: "mix", name: "Mix", units: "%", default: 1.0, steps: 0 },
    ParamSpec { id: 6, symbol: "key_hpf", name: "Key HPF", units: "Hz", default: 0.0, steps: 0 },
];

/// One knob per `units::denoise::DenoiseParams` field. Defaults are the
/// kernel's — nothing learned, a wire — and the defaults-pin test holds the
/// two together. The 32 learned thresholds are NOT knobs: they persist as
/// the extra-state symbols `t0..t31` (see [`is_extra_symbol`]), because a
/// threshold is a measurement, not a dial an automation lane should sweep.
///
/// `kernel` was appended at id 6, and `attack` and `treble` after it at 7
/// and 8, and all three stay where they are. Ids and symbols are persisted
/// forever and a value must never move one knob to the left because a
/// parameter was added, so a new one takes a new number at the end of the
/// road — which is also why a session written before a parameter existed
/// reads it as the spec's default (10 ms; 2 dB) and not as position zero.
/// The KERNEL is the one exception: a fresh bay opens on the Bank (the
/// owner, 2026-10-11), while a blob with no `kernel=` line keeps reading
/// as Spectral Studio, what it has read as since the selector landed. See
/// [`absent_reads_as`].
///
/// The defaults for `smooth` and `release` moved on 2026-09-16 (the
/// softest knee, 100 ms) with the owner's new numbers; a bay saved before
/// that keeps the values it was saved with, as it should.
const DENOISE_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "learn", name: "Learn", units: "", default: 0.0, steps: 2 },
    ParamSpec { id: 1, symbol: "reduce", name: "Reduce", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 2, symbol: "smooth", name: "Smooth", units: "", default: 0.0, steps: 0 },
    ParamSpec { id: 3, symbol: "release", name: "Release", units: "ms", default: 0.197, steps: 0 },
    ParamSpec { id: 4, symbol: "sc_hpf", name: "Detector HPF", units: "Hz", default: 0.0, steps: 0 },
    ParamSpec { id: 5, symbol: "lookahead", name: "Lookahead", units: "", default: 1.0, steps: 2 },
    ParamSpec { id: 6, symbol: "kernel", name: "Kernel", units: "", default: 0.0, steps: DENOISE_KERNEL_STEPS },
    ParamSpec { id: 7, symbol: "attack", name: "Attack", units: "ms", default: 0.5, steps: 0 },
    ParamSpec { id: 8, symbol: "treble", name: "Treble Boost", units: "dB", default: 1.0 / 3.0, steps: 0 },
];

/// How many positions the denoise's KERNEL menu has. A literal so the table
/// above stays `const`; a test pins it to `DenoiseKernel::ALL`, the enum it
/// steps through — the same arrangement the EQ's shape knob uses.
const DENOISE_KERNEL_STEPS: i32 = crate::units::denoise::DENOISE_KERNEL_STEPS;

/// One knob per panel control of `units::amp`. Ids 0..4 are the top strip
/// (bright, volume, tone stack), 5..8 the vibrato group. Defaults are
/// `AmpParams::default()` verbatim — warm and clean — except the VIB
/// switch, which is the wrapper's own (the kernel spells "off" as depth 0;
/// a panel wants a switch that remembers the intensity it will come back
/// at, so the switch is a face parameter and the wrapper computes the
/// depth). A test holds the two tables together.
const AMP_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "bright", name: "Bright", units: "", default: 0.0, steps: 2 },
    ParamSpec { id: 1, symbol: "volume", name: "Volume", units: "", default: 0.7, steps: 0 },
    ParamSpec { id: 2, symbol: "treble", name: "Treble", units: "", default: 0.5, steps: 0 },
    ParamSpec { id: 3, symbol: "middle", name: "Middle", units: "", default: 0.5, steps: 0 },
    ParamSpec { id: 4, symbol: "bass", name: "Bass", units: "", default: 0.5, steps: 0 },
    ParamSpec { id: 5, symbol: "vib_on", name: "Vibrato", units: "", default: 0.0, steps: 2 },
    ParamSpec { id: 6, symbol: "vib_speed", name: "Speed", units: "Hz", default: 0.35, steps: 0 },
    ParamSpec { id: 7, symbol: "vib_depth", name: "Intensity", units: "%", default: 0.0, steps: 0 },
    ParamSpec { id: 8, symbol: "vib_stereo", name: "Stereo", units: "%", default: 0.0, steps: 0 },
];

/// One knob per `units::drive::DriveParams` field: the pedal as it leaves
/// the factory.
const DRIVE_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "drive", name: "Drive", units: "%", default: 0.35, steps: 0 },
    ParamSpec { id: 1, symbol: "tone", name: "Tone", units: "Hz", default: 0.5, steps: 0 },
    ParamSpec { id: 2, symbol: "level", name: "Level", units: "dB", default: 0.71, steps: 0 },
    ParamSpec { id: 3, symbol: "symmetry", name: "Symmetry", units: "", default: 0.0, steps: 2 },
];

/// One knob per `units::trem::TremParams` field: the amp circuit in a box,
/// at the setting the reference amp spends its life on.
const TREM_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "speed", name: "Speed", units: "Hz", default: 0.35, steps: 0 },
    ParamSpec { id: 1, symbol: "depth", name: "Depth", units: "%", default: 0.6, steps: 0 },
    ParamSpec { id: 2, symbol: "stereo", name: "Stereo", units: "%", default: 0.0, steps: 0 },
];

/// One knob per `units::chorus::ChorusParams` field — the fixed chain's
/// voice with its send full up, per the extraction lane's mapping table.
const CHORUS_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "rate", name: "Rate", units: "Hz", default: 0.28, steps: 0 },
    ParamSpec { id: 1, symbol: "depth", name: "Depth", units: "%", default: 0.55, steps: 0 },
    ParamSpec { id: 2, symbol: "width", name: "Width", units: "%", default: 0.85, steps: 0 },
    ParamSpec { id: 3, symbol: "tone", name: "Tone", units: "Hz", default: 0.45, steps: 0 },
    ParamSpec { id: 4, symbol: "mix", name: "Mix", units: "%", default: 1.0, steps: 0 },
];

/// One knob per `units::echo::EchoParams` field. `mix` is last ON PURPOSE
/// and stays last: the kernel documents that a `sync` field will be
/// APPENDED after it when the bay protocol carries a tempo, and this id
/// space is as append-only as every other.
const ECHO_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: 0,
        symbol: "time",
        name: "Time",
        units: "ms",
        default: crate::units::echo::ECHO_TIME_DEFAULT,
        steps: 0,
    },
    ParamSpec { id: 1, symbol: "feedback", name: "Feedback", units: "%", default: 0.42, steps: 0 },
    ParamSpec { id: 2, symbol: "tone", name: "Tone", units: "%", default: 0.55, steps: 0 },
    ParamSpec { id: 3, symbol: "mix", name: "Mix", units: "%", default: 1.0, steps: 0 },
];

/// One knob per `units::plate::PlateParams` field: the fixed chain's
/// 2.2-second plate with the send full up.
const PLATE_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "decay", name: "Decay", units: "s", default: 0.34, steps: 0 },
    ParamSpec { id: 1, symbol: "predelay", name: "Predelay", units: "ms", default: 0.0, steps: 0 },
    ParamSpec { id: 2, symbol: "width", name: "Width", units: "%", default: 0.80, steps: 0 },
    ParamSpec { id: 3, symbol: "mix", name: "Mix", units: "%", default: 1.0, steps: 0 },
];

/// One knob per `units::wah::WahParams` field: the clav patch's own pedal.
const WAH_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "sens", name: "Sens", units: "%", default: 0.55, steps: 0 },
    ParamSpec { id: 1, symbol: "base", name: "Base", units: "Hz", default: 0.5, steps: 0 },
    ParamSpec { id: 2, symbol: "q", name: "Q", units: "%", default: 0.5, steps: 0 },
    ParamSpec { id: 3, symbol: "mix", name: "Mix", units: "%", default: 1.0, steps: 0 },
];

/// One knob per `units::convolver::ConvolverParams` field. The IR itself is
/// not a knob: it persists as the extra-state symbol `ir_path` and is read
/// off disk at load time, host-side, like a plugin loading its own data.
const IR_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "mix", name: "Mix", units: "%", default: 1.0, steps: 0 },
    ParamSpec { id: 1, symbol: "trim", name: "Trim", units: "dB", default: 0.5, steps: 0 },
];

/// One knob per `units::bitcrush::BitcrushParams` field, ids in declaration
/// order. Defaults are `BitcrushParams::default()` verbatim (eight bits at
/// about 12 kHz, fully wet, unity levels, no dither) and a test holds the
/// two tables together. BITS and RATE both reach the wire at the top of
/// their travel; DRIVE and OUTPUT are unity at noon.
const BITCRUSH_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: 0,
        symbol: "bits",
        name: "Bits",
        units: "bit",
        default: crate::units::bitcrush::BITS_DEFAULT,
        steps: 0,
    },
    ParamSpec {
        id: 1,
        symbol: "rate",
        name: "Rate",
        units: "Hz",
        default: crate::units::bitcrush::RATE_DEFAULT,
        steps: 0,
    },
    ParamSpec { id: 2, symbol: "drive", name: "Drive", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 3, symbol: "mix", name: "Mix", units: "%", default: 1.0, steps: 0 },
    ParamSpec { id: 4, symbol: "output", name: "Output", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 5, symbol: "dither", name: "Dither", units: "", default: 0.0, steps: 2 },
    // Appended on the owner's ruling (2026-10-11). Off, so a blob saved
    // before it existed (no `aa=` line) reads as the classic folding it
    // was saved with, and a fresh unit opens on it too.
    ParamSpec { id: 6, symbol: "aa", name: "Anti-alias", units: "", default: 0.0, steps: 2 },
];

/// The insert's four knobs. Both trims centre on unity over the kernel's
/// `GAIN_DB`; the mix opens fully wet, because an insert you patched a box
/// into is one you want to hear the box through.
const HWINSERT_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "send", name: "Send", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 1, symbol: "return", name: "Return", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 2, symbol: "mix", name: "Mix", units: "%", default: 1.0, steps: 0 },
    ParamSpec { id: 3, symbol: "invert", name: "Return Polarity", units: "", default: 0.0, steps: 2 },
];

fn build_trim(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Trim(Trim::new(sample_rate))
}

fn build_eq(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Eq(EqUnit::new(sample_rate))
}

fn build_imager(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Imager(ImagerUnit::new(sample_rate))
}

fn build_tape(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Tape(TapeUnit::new(sample_rate))
}

fn build_comp(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Comp(CompUnit::new(sample_rate))
}

fn build_denoise(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Denoise(DenoiseUnit::new(sample_rate))
}

fn build_amp(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Amp(AmpUnit::new(sample_rate))
}

fn build_drive(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Drive(DriveUnit::new(sample_rate))
}

fn build_trem(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Trem(TremUnit::new(sample_rate))
}

fn build_chorus(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Chorus(ChorusUnit::new(sample_rate))
}

fn build_echo(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Echo(EchoUnit::new(sample_rate))
}

fn build_plate(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Plate(PlateUnit::new(sample_rate))
}

fn build_wah(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Wah(WahUnit::new(sample_rate))
}

fn build_ir(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Ir(IrUnit::new(sample_rate))
}

fn build_bitcrush(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Bitcrush(BitcrushUnit::new(sample_rate))
}

fn build_hwinsert(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::HwInsert(crate::units::hwinsert::HwInsert::new(sample_rate))
}

/// How many parameters the analog publishes: the kernel's own table.
const ANALOG_N: usize = crate::analog::PARAMS.len();

/// **The analog's parameter table, row for row the kernel's own.** The
/// kernel keeps its table in its own `ParamSpec` type so the module and its
/// bench stand alone without this crate's; this is that table transcribed
/// at compile time — id, symbol, name, units, default, steps, in order —
/// so there is one list and the two cannot drift. A test below holds them
/// equal anyway, because "cannot" is what every second copy said.
pub const ANALOG_FACE_PARAMS: [ParamSpec; ANALOG_N] = {
    let mut out = [ParamSpec {
        id: 0,
        symbol: "",
        name: "",
        units: "",
        default: 0.0,
        steps: 0,
    }; ANALOG_N];
    let mut i = 0;
    while i < ANALOG_N {
        let p = &crate::analog::PARAMS[i];
        out[i] = ParamSpec {
            id: p.id,
            symbol: p.symbol,
            name: p.name,
            units: p.units,
            default: p.default,
            steps: p.steps,
        };
        i += 1;
    }
    out
};

fn build_analog(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Analog(AnalogUnit::new(sample_rate))
}

/// **The sampler's sixteen dials.** Ids 0..15, in the order the brief
/// declares them, and both halves of the feature index by exactly these —
/// the face reads the symbols back through [`spec_for`].
///
/// The DEFAULTS are arithmetic on the kernel's own range constants rather
/// than sixteen decimals, so a range that moves takes its default with it:
/// `sampler::default_norm` is the one place a rest is decided, and a test
/// below holds this table to it. Everything here is normalised 0..=1, the
/// house contract; the maps to seconds, cents and semitones live beside the
/// DSP in `units::sampler` WITH their inverses, so the face can print a
/// number and read one back without keeping a second copy of the taper.
///
/// **Two dials that look continuous and are not, and one that looks stepped
/// and is not.** Mode, Obey Note Off, Loop, Max Voices and Bend Range are
/// stepped, because each position is a different thing rather than more of
/// the same one. Transpose is CONTINUOUS with a rounded readout: the brief
/// asks for whole semitones and the kernel rounds, but a forty-nine-position
/// cycling row is not a dial anybody wants to drag two octaves on.
pub const SAMPLER_FACE_PARAMS: [ParamSpec; crate::units::sampler::PARAM_COUNT] = {
    use crate::units::sampler as k;
    [
        ParamSpec { id: k::P_VOL, symbol: "vol", name: "Volume", units: "x", default: 0.5, steps: 0 },
        ParamSpec { id: k::P_MODE, symbol: "mode", name: "Sample Mode", units: "", default: 0.0, steps: 3 },
        ParamSpec { id: k::P_TRANSPOSE, symbol: "transpose", name: "Transpose", units: "semi", default: 0.5, steps: 0 },
        ParamSpec { id: k::P_FINE, symbol: "fine", name: "Fine", units: "c", default: 0.5, steps: 0 },
        ParamSpec { id: k::P_ATTACK, symbol: "attack", name: "Attack", units: "ms", default: SAMPLER_ATTACK_REST, steps: 0 },
        ParamSpec { id: k::P_DECAY, symbol: "decay", name: "Decay", units: "ms", default: SAMPLER_DECAY_REST, steps: 0 },
        ParamSpec { id: k::P_SUSTAIN, symbol: "sustain", name: "Sustain", units: "%", default: 1.0, steps: 0 },
        ParamSpec { id: k::P_RELEASE, symbol: "release", name: "Release", units: "ms", default: SAMPLER_RELEASE_REST, steps: 0 },
        ParamSpec { id: k::P_START, symbol: "start", name: "Start", units: "%", default: 0.0, steps: 0 },
        ParamSpec { id: k::P_END, symbol: "end", name: "End", units: "%", default: 1.0, steps: 0 },
        ParamSpec { id: k::P_NOTEOFF, symbol: "noteoff", name: "Obey Note Off", units: "", default: 1.0, steps: 2 },
        ParamSpec { id: k::P_LOOP, symbol: "loop", name: "Loop", units: "", default: 0.0, steps: 2 },
        ParamSpec { id: k::P_XFADE, symbol: "xfade", name: "Loop Crossfade", units: "ms", default: SAMPLER_XFADE_REST, steps: 0 },
        ParamSpec { id: k::P_VOICES, symbol: "voices", name: "Max Voices", units: "", default: SAMPLER_VOICES_REST, steps: k::MAX_VOICES as i32 },
        ParamSpec { id: k::P_PORTA, symbol: "porta", name: "Portamento", units: "ms", default: 0.0, steps: 0 },
        ParamSpec { id: k::P_BEND, symbol: "bend", name: "Bend Range", units: "semi", default: SAMPLER_BEND_REST, steps: k::BEND_MAX_SEMIS + 1 },
    ]
};

/// The four tapered rests and the two stepped ones, spelled where a `const`
/// can hold them: `ParamSpec::default` is a `const` field and the kernel's
/// maps are `fn`s, which const evaluation cannot call. The arithmetic is the
/// map's, inlined once each, and `the_samplers_defaults_are_the_kernels`
/// holds every one of them to `sampler::default_norm` at test time — so the
/// duplication cannot drift silently, which is the only thing wrong with it.
///
/// The cube root is what `sampler::secs_norm` does: 5 ms of a five-second
/// sweep sits a tenth of the way round, not a thousandth.
const SAMPLER_ATTACK_REST: f32 = 0.1; // (0.005 / 5.0).cbrt()
const SAMPLER_DECAY_REST: f32 = 0.341_995_2; // (0.20 / 5.0).cbrt()
const SAMPLER_RELEASE_REST: f32 = 0.246_621_2; // (0.15 / 10.0).cbrt()
const SAMPLER_XFADE_REST: f32 = 0.271_441_76; // (0.010 / 0.5).cbrt()
/// Sixteen voices of thirty-two: position 15 of 0..31.
const SAMPLER_VOICES_REST: f32 = 15.0 / 31.0;
/// Two semitones of the twenty-five positions 0..=24.
const SAMPLER_BEND_REST: f32 = 2.0 / 24.0;

fn build_sampler(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Sampler(SamplerUnit::new(sample_rate))
}

/// **The most parameters any one built-in publishes.** Computed over
/// [`UNITS`] rather than written down, so a unit with a bigger panel carries
/// its own answer here the day it is added.
///
/// This is a SIZE somebody else has to respect, not a fact about this file:
/// a built-in's knobs reach its instance over the audio thread's per-bay
/// parameter queue, and a queue shorter than this silently drops the tail of
/// every bulk set — a preset, an Init. See `AUTO_PARAMS_PER_BAY`.
pub const MAX_UNIT_PARAMS: usize = {
    let mut most = 0;
    let mut i = 0;
    while i < UNITS.len() {
        if UNITS[i].params.len() > most {
            most = UNITS[i].params.len();
        }
        i += 1;
    }
    most
};

/// Every effect this build ships. Order is nothing; the path is the key.
pub const UNITS: &[UnitSpec] = &[
    UnitSpec {
        path: TRIM_PATH,
        class: "Tangent Trim",
        params: TRIM_PARAMS,
        build: build_trim,
        voice: false,
    },
    UnitSpec {
        path: EQ_PATH,
        class: "Tangent EQ",
        params: EQ_FACE_PARAMS,
        build: build_eq,
        voice: false,
    },
    UnitSpec {
        path: IMAGER_PATH,
        class: "Tangent Imager",
        params: IMAGER_FACE_PARAMS,
        build: build_imager,
        voice: false,
    },
    UnitSpec {
        path: TAPE_PATH,
        class: "Tangent Tape",
        params: TAPE_FACE_PARAMS,
        build: build_tape,
        voice: false,
    },
    UnitSpec {
        path: COMP_PATH,
        class: "Tangent Comp",
        params: COMP_FACE_PARAMS,
        build: build_comp,
        voice: false,
    },
    UnitSpec {
        path: DENOISE_PATH,
        class: "Tangent Denoise",
        params: DENOISE_FACE_PARAMS,
        build: build_denoise,
        voice: false,
    },
    UnitSpec {
        path: AMP_PATH,
        class: "Tangent Amp",
        params: AMP_FACE_PARAMS,
        build: build_amp,
        voice: false,
    },
    UnitSpec {
        path: DRIVE_PATH,
        class: "Tangent Drive",
        params: DRIVE_FACE_PARAMS,
        build: build_drive,
        voice: false,
    },
    UnitSpec {
        path: TREM_PATH,
        class: "Tangent Trem",
        params: TREM_FACE_PARAMS,
        build: build_trem,
        voice: false,
    },
    UnitSpec {
        path: CHORUS_PATH,
        class: "Tangent Chorus",
        params: CHORUS_FACE_PARAMS,
        build: build_chorus,
        voice: false,
    },
    UnitSpec {
        path: ECHO_PATH,
        class: "Tangent Echo",
        params: ECHO_FACE_PARAMS,
        build: build_echo,
        voice: false,
    },
    UnitSpec {
        path: PLATE_PATH,
        class: "Tangent Plate",
        params: PLATE_FACE_PARAMS,
        build: build_plate,
        voice: false,
    },
    UnitSpec {
        path: WAH_PATH,
        class: "Tangent Wah",
        params: WAH_FACE_PARAMS,
        build: build_wah,
        voice: false,
    },
    UnitSpec {
        path: IR_PATH,
        class: "Tangent IR",
        params: IR_FACE_PARAMS,
        build: build_ir,
        voice: false,
    },
    UnitSpec {
        path: BITCRUSH_PATH,
        class: "Tangent Bitcrush",
        params: BITCRUSH_FACE_PARAMS,
        build: build_bitcrush,
        voice: false,
    },
    // The hardware loop. An effect like the rest of the shelf: the bay's
    // signal goes in and comes back transformed, by a box this time.
    UnitSpec {
        path: HWINSERT_PATH,
        class: "Tangent Insert",
        params: HWINSERT_FACE_PARAMS,
        build: build_hwinsert,
        voice: false,
    },
    // The one voice. Its class is the preset folder's name, so it stays
    // the plain words even if the picker one day wears a suffix.
    UnitSpec {
        path: ANALOG_PATH,
        class: "Tangent Analog",
        params: &ANALOG_FACE_PARAMS,
        build: build_analog,
        voice: true,
    },
    // The second voice, and the first that plays a file. Same class-is-the-
    // preset-folder rule as the analog's.
    UnitSpec {
        path: SAMPLER_PATH,
        class: "Tangent Sampler",
        params: &SAMPLER_FACE_PARAMS,
        build: build_sampler,
        voice: true,
    },
];

/// The unit a sentinel names, or `None` for a sentinel this build does not
/// know — which the loader reports rather than swallows, because a session
/// from a NEWER build naming a unit this one lacks deserves a sentence, not a
/// silently empty bay.
/// Whether the denoiser's bay is on the BANK engine, whose SMOOTH is an
/// expansion ratio and which has no attack or treble dial to read.
///
/// The kernel was appended, and appended things stay appended, so this
/// finds it by SYMBOL through the spec rather than by position, and
/// answers "Bank" for a bay whose values have not arrived yet: a face that
/// briefly printed a ratio over a knee is a smaller wrong than one that
/// printed "Spectral only" over a knob that works.
fn denoise_reads_the_gate(siblings: &[f32]) -> bool {
    let Some(spec) = spec_for(DENOISE_PATH) else {
        return true;
    };
    let Some(at) = spec.params.iter().position(|p| p.symbol == "kernel") else {
        return true;
    };
    siblings.get(at).is_none_or(|v| {
        crate::units::denoise::DenoiseKernel::from_dial(*v)
            == crate::units::denoise::DenoiseKernel::Bank
    })
}

/// The value as the face should PRINT it — the kernel's own mapping
/// constants turned into the band's reading formats, in one place, so the
/// panel and the sound can never quote two different numbers for one knob.
/// The formatters are the face kit's; this crate links the UI crate, so
/// reuse is free and the firewall is untouched (data flows outward only).
///
/// **`siblings` is the whole of that bay's values, and it is here because a
/// knob can mean two things.**
///
/// The denoiser has three engines behind one face. SMOOTH is an expansion
/// RATIO on the bank and the opening KNEE on the spectral kernels, so its
/// reading depends on which engine is up; and the bank has no attack or
/// treble dial at all — `process` sends everything but Bank to
/// `process_spectral`, and those two are read on that side only.
///
/// This used to say "Bank only" over SMOOTH and RELEASE, because on the
/// spectral path they did nothing and a knob that does nothing is worse
/// than a knob that is not there. They work there now (2026-09-16): the
/// release is the fall of a band's gain, the attack its rise, SMOOTH the
/// width of the knee a band opens over. An earlier release envelope had
/// made the residual worse; the difference is that the presence is now
/// gated on the band's excess over its print, so the envelope releases
/// from a gain that was allowed to open by a note and not by a chair.
///
/// The whole slice rather than the one sibling it needs: the caller has it,
/// every other arm ignores it, and the day a second unit has a mode that
/// parks a control this asks no new question.
pub fn describe(path: &str, id: u32, value: f32, siblings: &[f32]) -> String {
    use tangent_ui::face::{fmt_db, fmt_hz, fmt_ms, fmt_pct};
    let v = value.clamp(0.0, 1.0);
    let lerp = |r: (f32, f32)| r.0 + (r.1 - r.0) * v;
    let sweep = |r: (f32, f32)| r.0 * (r.1 / r.0).powf(v);
    match (path, id) {
        (TRIM_PATH, 0) => fmt_db(lerp((TRIM_DB.0 as f32, TRIM_DB.1 as f32))),
        (EQ_PATH, id) => {
            use crate::units::eq::{NodeKind, EQ_FREQ_HZ, EQ_GAIN_DB, EQ_Q};
            match id % EQ_KNOBS_PER_NODE {
                0 => (if v >= 0.5 { "in" } else { "out" }).to_owned(),
                1 => {
                    let last = (NodeKind::ALL.len() - 1) as f32;
                    let at = ((v * last).round() as usize).min(NodeKind::ALL.len() - 1);
                    NodeKind::ALL[at].label().to_owned()
                }
                2 => fmt_hz(sweep(EQ_FREQ_HZ)),
                3 => fmt_db((v * 2.0 - 1.0) * EQ_GAIN_DB),
                _ => format!("Q {:.2}", sweep(EQ_Q)),
            }
        }
        (IMAGER_PATH, 0) => {
            // The side gain doubles at the top of the travel, so the knob
            // reads 0..200% with unity at noon.
            use crate::units::imager::WIDTH_GAIN;
            fmt_pct(lerp(WIDTH_GAIN))
        }
        (IMAGER_PATH, 1) => fmt_pct(v),
        (IMAGER_PATH, 2) => {
            use crate::units::imager::MONO_BELOW_HZ;
            if v <= 0.0 {
                "off".to_owned()
            } else {
                fmt_hz(sweep(MONO_BELOW_HZ))
            }
        }
        (COMP_PATH, id) => {
            use crate::units::comp::*;
            match id {
                0 => fmt_db(lerp(COMP_THRESHOLD_DB)),
                1 => format!("{:.1}:1", COMP_RATIO_MAX.powf(v)),
                2 => fmt_ms(sweep(COMP_ATTACK_MS)),
                3 => fmt_ms(sweep(COMP_RELEASE_MS)),
                4 => fmt_db(lerp(COMP_MAKEUP_DB)),
                5 => fmt_pct(v),
                _ => {
                    if v <= 0.0 {
                        "off".to_owned()
                    } else {
                        fmt_hz(sweep(COMP_KEY_HPF_HZ))
                    }
                }
            }
        }
        (DENOISE_PATH, id) => {
            use crate::units::denoise::*;
            match id {
                // The same words `choices` offers, so the stepped row and
                // the reading can never disagree about what the button says.
                0 => (if v >= 0.5 { "Listening" } else { "Learn" }).to_owned(),
                // The cap on the CUT, so the sign says which way it goes.
                1 => {
                    if v <= 0.0 {
                        "off".to_owned()
                    } else {
                        fmt_db(-(v * DENOISE_REDUCE_DB))
                    }
                }
                // SMOOTH is a ratio on the bank and a knee on the spectral
                // kernels — see this function's own doc.
                2 if denoise_reads_the_gate(siblings) => format!("{:.1}:1", sweep(DENOISE_RATIO)),
                2 => {
                    let width = lerp((OPEN_SOFT_DB.1 - OPEN_SOFT_DB.0, OPEN_HARD_DB.1 - OPEN_HARD_DB.0));
                    format!("{width:.0} dB knee")
                }
                3 => fmt_ms(sweep(DENOISE_RELEASE_MS)),
                // The bank has neither of these dials.
                7 | 8 if denoise_reads_the_gate(siblings) => "Spectral only".to_owned(),
                7 => fmt_ms(sweep(DENOISE_ATTACK_MS)),
                8 => {
                    if v <= 0.0 {
                        "off".to_owned()
                    } else {
                        fmt_db(v * DENOISE_TREBLE_DB)
                    }
                }
                4 => {
                    if v <= 0.0 {
                        "off".to_owned()
                    } else {
                        fmt_hz(sweep(DENOISE_SC_HPF_HZ))
                    }
                }
                5 => {
                    if v >= 0.5 {
                        fmt_ms(DENOISE_LOOKAHEAD_MS)
                    } else {
                        "off".to_owned()
                    }
                }
                // The engine, in the selector's own words — the same
                // strings `choices` offers, so the row and the reading can
                // never disagree about which kernel is in the circuit.
                _ => DenoiseKernel::from_dial(v).label().to_owned(),
            }
        }
        (AMP_PATH, 0) => (if v >= 0.5 { "on" } else { "off" }).to_owned(),
        // The vibrato speed rides the opto circuit's own geometric sweep;
        // the pots read as pot travel — a tone stack has no honest Hz.
        (AMP_PATH, 5) => fmt_hz(sweep(crate::units::trem::TREM_SPEED_HZ)),
        (AMP_PATH, _) => fmt_pct(v),
        (DRIVE_PATH, 1) => fmt_hz(sweep(crate::units::drive::DRIVE_TONE_HZ)),
        (DRIVE_PATH, 2) => {
            // Square-law across 0..DRIVE_LEVEL_MAX, printed as the dB it
            // lands on — the shipped 0.71 reads as unity's +0.0.
            use crate::units::drive::DRIVE_LEVEL_MAX;
            if v <= 0.0 {
                "off".to_owned()
            } else {
                fmt_db(20.0 * (v * v * DRIVE_LEVEL_MAX).log10())
            }
        }
        (DRIVE_PATH, 3) => (if v >= 0.5 { "uneven" } else { "matched" }).to_owned(),
        (TREM_PATH, 0) => fmt_hz(sweep(crate::units::trem::TREM_SPEED_HZ)),
        (CHORUS_PATH, 0) => fmt_hz(lerp(crate::effects::CHORUS_RATE_HZ)),
        (CHORUS_PATH, 3) => fmt_hz(lerp(crate::effects::CHORUS_LP_HZ)),
        (ECHO_PATH, 0) => fmt_ms(sweep(crate::units::echo::ECHO_TIME_S) * 1_000.0),
        (PLATE_PATH, 0) => fmt_ms(lerp(crate::effects::PLATE_DECAY_S) * 1_000.0),
        (PLATE_PATH, 1) => fmt_ms(lerp(crate::effects::PLATE_PREDELAY_MS)),
        (WAH_PATH, 1) => fmt_hz(sweep(crate::units::wah::WAH_BASE_HZ)),
        (IR_PATH, 1) => fmt_db(lerp(crate::units::convolver::CONV_TRIM_DB)),
        // The bitcrusher reads through the kernel's own maps. BITS prints a
        // tenth of a bit because the depth is continuous and 7.5 bits is a
        // real step size; RATE's top is the host's own rate, which has no
        // number this function could know, so it says so in a word.
        (BITCRUSH_PATH, id) => {
            use crate::units::bitcrush::*;
            match id {
                0 => format!("{:.1} bit", bits_at(v)),
                1 => rate_at(v).map_or_else(|| "full".to_owned(), fmt_hz),
                2 => fmt_db(lerp(DRIVE_DB)),
                3 => fmt_pct(v),
                4 => fmt_db(lerp(OUTPUT_DB)),
                _ => (if v >= 0.5 { "on" } else { "off" }).to_owned(),
            }
        }
        (HWINSERT_PATH, 0 | 1) => fmt_db(lerp(crate::units::hwinsert::GAIN_DB)),
        (HWINSERT_PATH, 3) => (if v >= 0.5 { "inverted" } else { "normal" }).to_owned(),
        // **The tape dials that have an honest number.** Their `ParamSpec`s
        // promise dB, Hz, dB and ips, and the automation lane prints that
        // promise beside the curve; the face used to fall through to
        // percent, so the lane said "dB" and the panel said "50 %" about the
        // same knob. Which of the two was wrong is settled by the machine:
        // the bump is the decibels its bell adds at the head's own bump
        // frequency, the repro corner is a frequency, a trim is decibels,
        // and the speed is the selector's own words, the same strings
        // `choices` offers. Drive, Wow, Hiss and Flutter stay percent below,
        // and their specs say so.
        (TAPE_PATH, 1) => fmt_db(lerp(crate::units::tape::BUMP_DB)),
        (TAPE_PATH, 2) => {
            // Zero is the stage out of circuit, not a 20 kHz corner: naming
            // a frequency for a filter that is not running is the lie the
            // comp's key filter and the drive's level both refuse to tell.
            if v <= 0.0 {
                "off".to_owned()
            } else {
                fmt_hz(sweep(crate::units::tape::ROLL_HZ))
            }
        }
        (TAPE_PATH, 5) => fmt_db(lerp(crate::units::tape::TRIM_DB)),
        (TAPE_PATH, 6) => crate::units::tape::TapeSpeed::from_dial(v).label().to_owned(),
        // Rate is a multiple of the parts' own speed, so it reads as one:
        // half at the bottom, the machine as built at the middle, twice at
        // the top.
        (TAPE_PATH, 8) => format!("{:.2}x", crate::units::tape::rate_multiplier(v)),
        (TAPE_PATH, 9) => crate::units::tape::WowRange::from_dial(v).label().to_owned(),
        // Bias is decibels either side of the point the machine is aligned
        // at, and the middle of the dial IS that point, so it says so in
        // the machine's word rather than as a number: "aligned" there, and
        // a signed reading either side of it, symmetric about it. Every
        // figure that would print as zero says the word too
        // ([`bias_reads_aligned`]), so no reading is ever a signed nothing.
        (TAPE_PATH, 10) => {
            let db = crate::units::tape::BIAS_RANGE_DB * (2.0 * v - 1.0);
            if bias_reads_aligned(db) {
                "aligned".to_owned()
            } else {
                fmt_db(db)
            }
        }
        // The rest of the tape speaks in percent on purpose: those knobs are
        // character, not calibration, and a fake decibel is worse than an
        // honest one. Every dial not named above — drive amount, depths,
        // widths, mixes, the wah's damping — is percent for the same reason.
        (ANALOG_PATH, id) => analog_reading(id, v),
        (SAMPLER_PATH, id) => sampler_reading(id, v),
        _ => fmt_pct(v),
    }
}

/// **The number a reading carries, in the reading's base unit** - or the
/// number somebody typed into one, read the same way so the two can be
/// compared.
///
/// The first numeric run in the text, with a sign and a fraction, and the
/// unit word after it folded in: "1.5 kHz" and "1.5k" are 1500, "2.00 s"
/// is 2000 (`fmt_ms` prints seconds past a thousand milliseconds), "35%"
/// is 35, "4.0:1" is 4, "Q 0.71" is 0.71, "+3.0 dB" is 3. A reading with
/// no number in it - "off", "in", "Spectral only" - answers `None`.
pub fn reading_number(text: &str) -> Option<f64> {
    let s = text.trim();
    let starts_number = |i: usize, c: char| {
        c.is_ascii_digit()
            || ((c == '-' || c == '+' || c == '.')
                && s[i + c.len_utf8()..]
                    .chars()
                    .next()
                    .is_some_and(|n| n.is_ascii_digit() || n == '.'))
    };
    let start = s.char_indices().find(|&(i, c)| starts_number(i, c))?.0;
    let mut end = start;
    for (i, c) in s[start..].char_indices() {
        let sign = i == 0 && (c == '-' || c == '+');
        if c.is_ascii_digit() || c == '.' || sign {
            end = start + i + c.len_utf8();
        } else {
            break;
        }
    }
    let n: f64 = s[start..end].parse().ok()?;
    if !n.is_finite() {
        return None;
    }
    // "k" is a thousand of the unit; a bare "s" is a thousand milliseconds.
    // "ms" starts with an m and is left alone.
    let unit = s[end..].trim_start();
    let scale = if unit.starts_with(['k', 'K', 's']) { 1_000.0 } else { 1.0 };
    Some(n * scale)
}

/// **Whether a bias of `db` decibels reads as the aligned point**: every
/// figure the reading would print as zero, of either sign.
///
/// The dial's middle is exactly 0.5, but a reading has a tenth of a
/// decibel, so the knob a hair either side of it printed "+0.0 dB" or
/// "-0.0 dB": the aligned point in the panel's own precision, signed as if
/// it were not. Decided by printing the figure the way the reading does and
/// reading the number back, so the word's span is exactly the span the
/// format rounds to zero, whatever its rounding rule. [`describe`] prints
/// the word over that span, and [`value_for_reading`] lands a typed number
/// it covers on the middle itself.
fn bias_reads_aligned(db: f32) -> bool {
    reading_number(&tangent_ui::face::fmt_db(db)) == Some(0.0)
}

/// **A knob position for words typed into its reading**, on the unit's own
/// scale - `None` when the unit cannot read them.
///
/// The inverse of [`describe`], found rather than spelled: the readings are
/// one match arm per knob and an inverse written beside each would be a
/// second copy of every range in the suite, drifting from the first. So the
/// knob's travel is sampled, every sample is read back through `describe`
/// itself, and the typed number goes to the sample that reads nearest it,
/// sharpened by bisection between that sample's neighbours. A word that IS
/// a reading - "off" on a knob that has one - goes to the first sample that
/// says it. A number past either end lands on that end, which is what a
/// field means by a number past its end. `siblings` for the reason
/// `describe` takes them: a reading can depend on what another knob says.
pub fn value_for_reading(path: &str, id: u32, typed: &str, siblings: &[f32]) -> Option<f32> {
    let want = typed.trim();
    if want.is_empty() {
        return None;
    }
    const N: usize = 256;
    let at = |i: usize| i as f32 / N as f32;
    let read = |v: f32| describe(path, id, v, siblings);
    let lower = want.to_lowercase();
    // **Bias's word, and any bias typed as nothing, is the aligned point
    // itself**: the middle of the dial, where a drag's catch and a
    // double-click land. Asked before the search, because the word reads
    // over a span either side of the middle ([`bias_reads_aligned`]), whose
    // first sample is not the middle, and no sample reads a zero for a
    // typed 0 to find: it used to land on the first reading that parsed to
    // one, 127/256, which printed "-0.0 dB" and ran the tape 0.047 dB
    // under-biased.
    if (path, id) == (TAPE_PATH, 10)
        && (lower == "aligned" || reading_number(want).is_some_and(|n| bias_reads_aligned(n as f32)))
    {
        return Some(0.5);
    }
    if let Some(i) = (0..=N).find(|&i| read(at(i)).trim().to_lowercase() == lower) {
        return Some(at(i));
    }
    let n = reading_number(want)?;
    let num_at = |v: f32| reading_number(&read(v));
    let mut best: Option<(usize, f64)> = None;
    for i in 0..=N {
        if let Some(x) = num_at(at(i)) {
            let d = (x - n).abs();
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
    }
    let (i, _) = best?;
    let (mut lo, mut hi) = (at(i.saturating_sub(1)), at((i + 1).min(N)));
    let (Some(n_lo), Some(n_hi)) = (num_at(lo), num_at(hi)) else {
        return Some(at(i));
    };
    if n_lo == n_hi {
        return Some(at(i));
    }
    let rising = n_hi > n_lo;
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        let Some(n_mid) = num_at(mid) else { break };
        if (n_mid < n) == rising {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    // The readings quantise, so the bracket can straddle a step: of the
    // three positions in hand, the one that reads nearest the number.
    let mid = 0.5 * (lo + hi);
    [lo, mid, hi, at(i)]
        .into_iter()
        .filter_map(|v| num_at(v).map(|x| (v, (x - n).abs())))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(v, _)| v)
}

/// A stepped parameter's positions, in step order, for the face's cycling
/// row. Continuous parameters answer empty.
pub fn choices(path: &str, id: u32) -> Vec<String> {
    match (path, id) {
        (EQ_PATH, id) if id % EQ_KNOBS_PER_NODE == 0 => {
            vec!["out".to_owned(), "in".to_owned()]
        }
        (EQ_PATH, id) if id % EQ_KNOBS_PER_NODE == 1 => crate::units::eq::NodeKind::ALL
            .iter()
            .map(|k| k.label().to_owned())
            .collect(),
        // The learn toggle's positions are the BUTTON's words, not on/off:
        // at rest it offers the verb, engaged it reports the state.
        (DENOISE_PATH, 0) => vec!["Learn".to_owned(), "Listening".to_owned()],
        (DENOISE_PATH, 6) => crate::units::denoise::DenoiseKernel::ALL
            .iter()
            .map(|k| k.label().to_owned())
            .collect(),
        (DENOISE_PATH, 5) | (AMP_PATH, 0) | (BITCRUSH_PATH, 5 | 6) => {
            vec!["off".to_owned(), "on".to_owned()]
        }
        // One diode against one, or against two — what the toggle does to
        // the harmonics, in the clipper's own terms.
        (DRIVE_PATH, 3) => vec!["matched".to_owned(), "uneven".to_owned()],
        (HWINSERT_PATH, 3) => vec!["normal".to_owned(), "inverted".to_owned()],
        (TAPE_PATH, 6) => crate::units::tape::TapeSpeed::ALL
            .iter()
            .map(|v| v.label().to_owned())
            .collect(),
        (TAPE_PATH, 9) => crate::units::tape::WowRange::ALL
            .iter()
            .map(|r| r.label().to_owned())
            .collect(),
        (ANALOG_PATH, id) => analog_choices(id),
        (SAMPLER_PATH, id) => sampler_choices(id),
        _ => Vec::new(),
    }
}

/// **What each of the analog's dials reads**, printed by the kernel's own
/// mappings — the consts beside the DSP — so the face and the sound cannot
/// disagree. Ids are the kernel's `P_*` names; a stepped dial prints its
/// position's word.
fn analog_reading(id: u32, v: f32) -> String {
    use crate::analog::arp::{ArpMode, ARP_RATES};
    use crate::analog::ensemble::EnsembleMode;
    use crate::analog::env::{secs, ATTACK_SECS, DECAY_SECS, RELEASE_SECS};
    use crate::analog::filter::cutoff_hz;
    use crate::analog::osc::{LfoWave, Wave, RANGE_LABELS};
    use crate::analog::*;
    use tangent_ui::face::{fmt_hz, fmt_ms, fmt_pct};
    let id = id as usize;
    let onoff = |v: f32| (if v >= 0.5 { "on" } else { "off" }).to_owned();
    let semis = |s: f32| format!("{s:+.2} semi");
    let cents = |c: f32| format!("{c:+.0} c");
    match id {
        P_TUNE => cents((v - 0.5) * 2.0 * TUNE_CENTS),
        P_GLIDE => {
            if v <= 0.0 {
                "off".to_owned()
            } else {
                fmt_ms(GLIDE_SECS.0 * (GLIDE_SECS.1 / GLIDE_SECS.0).powf(v) * 1000.0)
            }
        }
        P_MOD_MIX => {
            if v <= 0.0 {
                "osc 3".to_owned()
            } else if v >= 1.0 {
                "noise".to_owned()
            } else {
                format!("{} noise", fmt_pct(v))
            }
        }
        P_MOD_DEPTH | P_PRESSURE | P_PWM | P_OSC_DRIFT | P_MIX_O1 | P_MIX_O2 | P_MIX_O3
        | P_MIX_NOISE | P_DRIVE | P_EMPHASIS | P_CONTOUR | P_KEYTRACK | P_F_SUSTAIN
        | P_A_SUSTAIN | P_VEL_AMP | P_VEL_FILTER | P_UNISON_SPREAD | P_DRIFT | P_ENS_WIDTH
        | P_SPREAD | P_VOLUME | P_ARP_GATE => fmt_pct(v),
        P_BEND_RANGE => format!("{} semi", step_index(v, 13)),
        P_O1_RANGE | P_O2_RANGE | P_O3_RANGE => RANGE_LABELS[step_index(v, 6)].to_owned(),
        P_O1_WAVE | P_O2_WAVE | P_O3_WAVE => Wave::from_norm(v).label().to_owned(),
        P_O2_TUNE | P_O3_TUNE => semis((v - 0.5) * 2.0 * OSC_TUNE_SEMIS),
        P_O3_TRACK | P_SYNC | P_OSC_MOD | P_FILTER_MOD | P_ARP_HOLD => onoff(v),
        P_NOISE_COLOUR => (if v >= 0.5 { "pink" } else { "white" }).to_owned(),
        P_FILTER_MODEL => (if v >= 0.5 { "SEM 12" } else { "Ladder 24" }).to_owned(),
        P_CUTOFF => fmt_hz(cutoff_hz(v)),
        P_SEM_MODE => {
            if v < 0.03 {
                "lowpass".to_owned()
            } else if (v - 0.5).abs() < 0.03 {
                "notch".to_owned()
            } else if v > 0.97 {
                "highpass".to_owned()
            } else if v < 0.5 {
                format!("LP {} notch", fmt_pct(v * 2.0))
            } else {
                format!("notch {} HP", fmt_pct((v - 0.5) * 2.0))
            }
        }
        P_F_ATTACK | P_A_ATTACK => fmt_ms(secs(v, ATTACK_SECS) * 1000.0),
        P_F_DECAY | P_A_DECAY => fmt_ms(secs(v, DECAY_SECS) * 1000.0),
        P_F_RELEASE | P_A_RELEASE => fmt_ms(secs(v, RELEASE_SECS) * 1000.0),
        P_VOICE_MODE => ["mono", "unison", "poly"][step_index(v, 3)].to_owned(),
        P_VOICES => format!("{}", step_index(v, 16) + 1),
        P_DETUNE => format!("{:.0} c", v * DETUNE_CENTS),
        P_PRIORITY => ["low", "last", "high"][step_index(v, 3)].to_owned(),
        P_TRIGGER => (if v >= 0.5 { "retrigger" } else { "legato" }).to_owned(),
        P_LFO_RATE => fmt_hz(LFO_HZ.0 * (LFO_HZ.1 / LFO_HZ.0).powf(v)),
        P_LFO_WAVE => LfoWave::from_norm(v).label().to_owned(),
        P_LFO_DELAY => fmt_ms(v * LFO_DELAY_SECS * 1000.0),
        P_ENSEMBLE => EnsembleMode::from_norm(v).label().to_owned(),
        P_ARP_MODE => ArpMode::from_norm(v).label().to_owned(),
        P_ARP_RATE => ARP_RATES[arp::rate_index(v)].0.to_owned(),
        P_ARP_OCT => format!("{} oct", step_index(v, 4) + 1),
        _ => fmt_pct(v),
    }
}

/// The analog's stepped dials, each position's word in order.
fn analog_choices(id: u32) -> Vec<String> {
    use crate::analog::arp::{ArpMode, ARP_RATES};
    use crate::analog::ensemble::EnsembleMode;
    use crate::analog::osc::{LfoWave, Wave, RANGE_LABELS};
    use crate::analog::*;
    let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    match id as usize {
        P_BEND_RANGE => (0..=12).map(|n| format!("{n} semi")).collect(),
        P_O1_RANGE | P_O2_RANGE | P_O3_RANGE => words(&RANGE_LABELS),
        P_O1_WAVE | P_O2_WAVE | P_O3_WAVE => Wave::ALL.iter().map(|w| w.label().to_owned()).collect(),
        P_O3_TRACK | P_SYNC | P_OSC_MOD | P_FILTER_MOD | P_ARP_HOLD => words(&["off", "on"]),
        P_NOISE_COLOUR => words(&["white", "pink"]),
        P_FILTER_MODEL => words(&["Ladder 24", "SEM 12"]),
        P_VOICE_MODE => words(&["mono", "unison", "poly"]),
        P_VOICES => (1..=16).map(|n| format!("{n}")).collect(),
        P_PRIORITY => words(&["low", "last", "high"]),
        P_TRIGGER => words(&["legato", "retrigger"]),
        P_LFO_WAVE => LfoWave::ALL.iter().map(|w| w.label().to_owned()).collect(),
        P_ENSEMBLE => EnsembleMode::ALL.iter().map(|m| m.label().to_owned()).collect(),
        P_ARP_MODE => ArpMode::ALL.iter().map(|m| m.label().to_owned()).collect(),
        P_ARP_RATE => ARP_RATES.iter().map(|(l, _)| (*l).to_owned()).collect(),
        P_ARP_OCT => (1..=4).map(|n| format!("{n} oct")).collect(),
        _ => Vec::new(),
    }
}

/// **What each of the sampler's dials reads**, printed through the kernel's
/// own maps — the functions beside the DSP — so the face and the sound
/// cannot disagree. A missing arm here is a knob that reads wrong, which is
/// why every one of the sixteen is named and none falls through.
fn sampler_reading(id: u32, v: f32) -> String {
    use crate::units::sampler as k;
    use tangent_ui::face::{fmt_ms, fmt_pct};
    let onoff = |v: f32| (if v >= 0.5 { "on" } else { "off" }).to_owned();
    let ms = |secs: f32| {
        if secs <= 0.0 {
            "off".to_owned()
        } else {
            fmt_ms(secs * 1000.0)
        }
    };
    match id {
        // A linear gain, printed as the multiplier it is. Not decibels: the
        // dial's own range is 0..2 and 0 has no decibel, so a dB readout
        // would need a special case at the one end people actually use.
        k::P_VOL => format!("{:.2}x", v.clamp(0.0, 1.0) * k::VOL_MAX),
        k::P_MODE => k::MODE_LABELS[k::step_index(v, 3)].to_owned(),
        // Whole semitones, because the kernel rounds to them — printing
        // "+6.9" over a knob that plays +7 is the readout lying about the
        // sound, which is the fault this whole function exists to avoid.
        k::P_TRANSPOSE => format!("{:+.0} semi", k::bipolar(v, k::TRANSPOSE_SEMIS).round()),
        // CENTS, and cents alone here. The owner asked for "pitch change
        // (hz)" and the face prints the Hz beside this — through
        // `sampler::fine_hz_at`, which needs the ROOT KEY and therefore
        // cannot be answered by a function that is handed only a dial.
        k::P_FINE => format!("{:+.0} c", k::bipolar(v, k::FINE_CENTS)),
        k::P_ATTACK => ms(k::secs(v, k::ATTACK_SECS)),
        k::P_DECAY => ms(k::secs(v, k::DECAY_SECS)),
        k::P_RELEASE => ms(k::secs(v, k::RELEASE_SECS)),
        k::P_XFADE => ms(k::secs(v, k::XFADE_MS / 1000.0)),
        k::P_PORTA => ms(k::secs(v, k::PORTA_SECS)),
        k::P_SUSTAIN | k::P_START | k::P_END => fmt_pct(v),
        k::P_NOTEOFF | k::P_LOOP => onoff(v),
        k::P_VOICES => format!("{}", k::step_index(v, k::MAX_VOICES as i32) + 1),
        k::P_BEND => format!("{} semi", k::step_index(v, k::BEND_MAX_SEMIS + 1)),
        _ => fmt_pct(v),
    }
}

/// The sampler's stepped dials, each position's word in step order.
fn sampler_choices(id: u32) -> Vec<String> {
    use crate::units::sampler as k;
    match id {
        k::P_MODE => k::MODE_LABELS.iter().map(|w| (*w).to_owned()).collect(),
        k::P_NOTEOFF | k::P_LOOP => vec!["off".to_owned(), "on".to_owned()],
        k::P_VOICES => (1..=k::MAX_VOICES).map(|n| format!("{n}")).collect(),
        k::P_BEND => (0..=k::BEND_MAX_SEMIS).map(|n| format!("{n} semi")).collect(),
        _ => Vec::new(),
    }
}

pub fn spec_for(path: &str) -> Option<&'static UnitSpec> {
    UNITS.iter().find(|u| u.path == path)
}

impl UnitSpec {
    /// A fresh instance with every parameter at `values` — settled there, not
    /// slewing there, because this runs at LOAD time and a restored setting
    /// that faded in over a second would be audible as somebody's preset
    /// arriving late.
    pub fn create(&self, sample_rate: f32, values: &[f32]) -> BuiltinUnit {
        let mut unit = (self.build)(sample_rate);
        for (spec, v) in self.params.iter().zip(values) {
            unit.set_param(spec.id, *v);
        }
        unit.clear();
        unit
    }

    /// Every parameter at its published rest.
    pub fn defaults(&self) -> Vec<f32> {
        self.params.iter().map(|p| p.default).collect()
    }
}

// ── the host-side ledger and its state blob ─────────────────────────────────

/// One built-in bay: which unit, and where its controls stand.
///
/// The same inversion [`crate::instrument`]'s `Lv2Bay` documents, inherited
/// for the same reason: the instance goes to the audio thread and cannot be
/// asked anything afterwards, but the host is the only durable writer of these
/// values — a restore and (later) a panel both come through here — so the
/// host's record IS the state and `save_insert_state` reads it without a round
/// trip. Automation is transient by the same rule LV2's is: a curve moves the
/// instance for the blocks it covers and the saved value is the knob's.
pub struct BuiltinBay {
    pub spec: &'static UnitSpec,
    /// Current value per parameter, normalised, indexed as `spec.params`.
    pub values: Vec<f32>,
    /// State lines BEYOND the parameters — `(symbol, value)` pairs a unit
    /// claims through [`is_extra_symbol`]: the denoise's 32 learned
    /// thresholds, the IR's file path. Same durable-truth rule as `values`:
    /// what the host holds here IS what `save_insert_state` writes, refilled
    /// from the blob at load and refreshed by the host when the engine
    /// publishes new measurements (the learn snapshot road).
    pub extra: Vec<(String, String)>,
}

/// Whether `symbol` is one of the EXTRA state lines `path`'s unit owns —
/// state that is not a knob. These symbol spaces are append-only exactly as
/// the param symbols are: `t0..t31` are the denoise's learned thresholds,
/// `ir_path` is the convolver's impulse file.
pub fn is_extra_symbol(path: &str, symbol: &str) -> bool {
    match path {
        // `t<n>` is the print and `k<n>` the bank's knee beside it - see
        // `Denoise::knees`. "t07" must NOT alias t7: one value per symbol,
        // one spelling.
        DENOISE_PATH => ['t', 'k'].into_iter().any(|p| {
            symbol
                .strip_prefix(p)
                .and_then(|n| n.parse::<usize>().ok())
                .is_some_and(|n| n < crate::units::denoise::DENOISE_BANDS && symbol == format!("{p}{n}"))
        }),
        IR_PATH => symbol == "ir_path",
        // **The sampler's file, named rather than carried.** The audio
        // travels the drum kit's road (`instrument::SamplerSample`) and never
        // through a state blob; what persists is where it came from, what to
        // call it, and the key it plays at unity — three lines, so a reopened
        // project can find the same sound again. `sample_name` is written
        // down as well as derived, because a library may rename a file and
        // the words the face printed are what somebody chose the preset by.
        SAMPLER_PATH => matches!(symbol, "sample_path" | "sample_name" | "root_key"),
        // **The insert's jacks and its measured loop**: which device
        // channels it sends and returns on, and what a ping found (with the
        // rate and buffers it found it at). See `units::hwinsert::state_lines`.
        HWINSERT_PATH => crate::units::hwinsert::EXTRA_SYMBOLS.contains(&symbol),
        _ => false,
    }
}

/// Write a unit's values as state text — the blob the insert-state sidecar
/// carries between the owner string and the end of the file.
///
/// Keyed by SYMBOL, not position, for the reason `lv2::state_from_params`
/// gives: a unit that grows a parameter must still read its old sessions, and
/// a positional format moves every value one knob to the left the day that
/// happens. `extra` lines ride after the params, in the same `symbol=value`
/// grammar — a value may hold any text without newlines (a path, say).
pub fn state_text(spec: &UnitSpec, values: &[f32], extra: &[(String, String)]) -> String {
    let mut out = String::from("tangentfx 1\n");
    for (p, v) in spec.params.iter().zip(values) {
        out.push_str(p.symbol);
        out.push('=');
        out.push_str(&v.to_string());
        out.push('\n');
    }
    for (symbol, value) in extra {
        out.push_str(symbol);
        out.push('=');
        out.push_str(value);
        out.push('\n');
    }
    out
}

/// Read a blob's EXTRA lines back — the ones [`is_extra_symbol`] claims for
/// this unit. Forgiving like [`values_from_state`] and quieter still: a blob
/// that is not this format at all answers empty, because the values reader
/// already turned that case into the sentence the user sees.
pub fn extra_from_state(spec: &UnitSpec, bytes: &[u8]) -> Vec<(String, String)> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Vec::new();
    };
    let mut lines = text.lines();
    if lines.next() != Some("tangentfx 1") {
        return Vec::new();
    }
    lines
        .filter_map(|line| line.split_once('='))
        .filter(|(symbol, _)| is_extra_symbol(spec.path, symbol))
        .map(|(s, v)| (s.to_owned(), v.to_owned()))
        .collect()
}

/// **What a symbol a saved blob LACKS reads as, when that is no longer the
/// spec's default.**
///
/// A missing line normally reads as the spec's default, which is right
/// while the default has never moved: the blob was written before the
/// knob existed, and the default is what that knob gave the day it
/// arrived. Once a default moves, the two part company. A fresh bay should
/// open on the new one; a saved one must go on playing what it played.
/// Each entry here is a default that moved after blobs without its line
/// were already in the world, pinned to what those blobs have always read.
///
/// The denoise's kernel: a fresh bay opens on the Bank since 2026-10-11,
/// and a blob from before the selector stays on
/// [`crate::units::denoise::DenoiseKernel::UNWRITTEN`] (Spectral Studio).
fn absent_reads_as(path: &str, symbol: &str) -> Option<f32> {
    match (path, symbol) {
        (DENOISE_PATH, "kernel") => Some(crate::units::denoise::DenoiseKernel::UNWRITTEN.dial()),
        _ => None,
    }
}

/// Read a state blob back into a full value table.
///
/// Forgiving about lines and strict about the container, on the LV2 model: a
/// symbol this build does not know is skipped (a newer build's parameter), a
/// symbol the blob lacks reads as its default (an older build's file; see
/// [`absent_reads_as`] for a default that moved since), a value that does
/// not parse or is not finite reads as that same value; but a blob that
/// is not this format at all is an `Err`, because it is another plugin's state
/// or a damaged file, and the user should hear that their settings did not
/// come back rather than get defaults dressed as a restore.
pub fn values_from_state(spec: &UnitSpec, bytes: &[u8]) -> Result<Vec<f32>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "the stored block is not text".to_owned())?;
    let mut lines = text.lines();
    if lines.next() != Some("tangentfx 1") {
        return Err("the stored block is not a built-in effect's state".to_owned());
    }
    let mut values: Vec<f32> = spec
        .params
        .iter()
        .map(|p| absent_reads_as(spec.path, p.symbol).unwrap_or(p.default))
        .collect();
    for line in lines {
        let Some((symbol, value)) = line.split_once('=') else {
            continue;
        };
        let Some(at) = spec.params.iter().position(|p| p.symbol == symbol) else {
            continue;
        };
        if let Ok(v) = value.parse::<f32>() {
            if v.is_finite() {
                values[at] = v.clamp(0.0, 1.0);
            }
        }
    }
    Ok(values)
}

// ── the units themselves ────────────────────────────────────────────────────

/// A built-in unit, ready to sit in a `Backend`. One variant per unit; the
/// audio thread dispatches by match, exactly as it does formats.
pub enum BuiltinUnit {
    Trim(Trim),
    Eq(EqUnit),
    Imager(ImagerUnit),
    Tape(TapeUnit),
    Comp(CompUnit),
    Denoise(DenoiseUnit),
    Amp(AmpUnit),
    Drive(DriveUnit),
    Trem(TremUnit),
    Chorus(ChorusUnit),
    Echo(EchoUnit),
    Plate(PlateUnit),
    Wah(WahUnit),
    Ir(IrUnit),
    /// The voice. Appended, the enum's law.
    Analog(AnalogUnit),
    /// The second voice. Appended for the same reason.
    Sampler(SamplerUnit),
    /// The bitcrusher, an effect. Appended, the enum's law, which is why it
    /// sits after the voices rather than beside the pedals.
    Bitcrush(BitcrushUnit),
    /// The hardware loop. Appended, the enum's law.
    HwInsert(crate::units::hwinsert::HwInsert),
}

impl BuiltinUnit {
    /// Frames this unit delays the signal by, reported once at load into the
    /// same per-bay cell every plugin's figure lands in.
    pub fn latency(&self) -> u32 {
        match self {
            // A trim looks at nothing ahead of the sample it is scaling.
            BuiltinUnit::Trim(_) => 0,
            // Minimum-phase by charter: no lookahead, no pre-ring. See the
            // module header of `units::eq`.
            BuiltinUnit::Eq(_) => 0,
            // Width, floor and velvet all read the sample they write.
            BuiltinUnit::Imager(_) => 0,
            // **The play head's delay, reported.** The studio head sits
            // `units::tape::BASE_MS` behind the record head and the deep head
            // `units::tape::DEEP_MS`, the record stage's oversampler inside
            // either figure, and an impulse leaves on exactly the frame the
            // kernel names, so the number is the real delay rather than a
            // nominal one: the transport sways the head around it, never off
            // it on average. Only the wow range moves it. The kernel answers
            // from the params alone, so the figure is right straight after
            // `UnitSpec::create` and for a bay restored bypassed, whose
            // kernel never runs (the denoiser's lesson, in
            // `DenoiseUnit::set_param`). `Backend::reported_latency`
            // re-reads it every block in circuit, and the rack's bypass arm
            // every block out of it, because a bypassed Tape keeps this
            // delay ([`TapeUnit::bypassed`]).
            BuiltinUnit::Tape(u) => u.kernel.latency_frames(&u.params) as u32,
            // Feed-forward with no lookahead window.
            BuiltinUnit::Comp(_) => 0,
            // The one honest mover: 3 ms with the lookahead up, nothing with
            // it down. The kernel answers for its current setting, and
            // `Backend::reported_latency` re-reads this every block, so the
            // toggle's change reaches the compensation graph on its own.
            BuiltinUnit::Denoise(u) => u.kernel.latency(),
            // Filters, saturators and modulators that read the sample they
            // write, like the four above. The tape-echo/chorus/plate DELAYS
            // are the effect, not a reporting debt — the dry half of each
            // additive mix leaves on time.
            BuiltinUnit::Amp(_)
            | BuiltinUnit::Drive(_)
            | BuiltinUnit::Trem(_)
            | BuiltinUnit::Chorus(_)
            | BuiltinUnit::Echo(_)
            | BuiltinUnit::Plate(_)
            | BuiltinUnit::Wah(_) => 0,
            // One partition of FIFO priming — but only once an IR is in:
            // with nothing loaded the kernel is a bit-exact wire, and
            // claiming 512 frames for a wire would hold the mix late for
            // nothing.
            BuiltinUnit::Ir(u) => {
                if u.has_ir {
                    u.kernel.latency()
                } else {
                    0
                }
            }
            // The oscillators' correction window plus the 4× pair: the
            // honest figure, reported so delay compensation can do its job
            // (brief §7.9). `run_rack` reads it for a built-in voice where
            // it exempts a VST3 one, which has no route to say.
            BuiltinUnit::Analog(u) => u.synth.latency(),
            // Nothing: the read head is at the sample it writes, and every
            // interpolation tap is one already in the file. A sampler that
            // billed frames it does not use would push the whole channel
            // late for arithmetic that happens between two of them.
            BuiltinUnit::Sampler(_) => 0,
            // A hold reads the sample it is holding or one before it, and
            // the quantizer is memoryless: nothing ahead is ever looked at.
            BuiltinUnit::Bitcrush(_) => 0,
            // **The loop's whole round trip**, measured by a ping or, until
            // one, the round-trip law's estimate (`units::hwinsert`'s module
            // notes). Re-read every block like the denoise's, so a ping, a
            // buffer change or a rate change reaches the compensation graph
            // on its own.
            BuiltinUnit::HwInsert(u) => u.latency(),
        }
    }

    /// One normalised value onto one parameter. Out-of-range ids are ignored
    /// — an automation curve can outlive the unit it was drawn for, and a
    /// stale id must move nothing rather than the wrong thing.
    pub fn set_param(&mut self, id: u32, value: f32) {
        match self {
            BuiltinUnit::Trim(t) => {
                if id == 0 {
                    t.set_gain(value);
                }
            }
            BuiltinUnit::Eq(u) => u.set_param(id, value),
            BuiltinUnit::Imager(u) => u.set_param(id, value),
            BuiltinUnit::Tape(u) => u.set_param(id, value),
            BuiltinUnit::Comp(u) => u.set_param(id, value),
            BuiltinUnit::Denoise(u) => u.set_param(id, value),
            BuiltinUnit::Amp(u) => u.set_param(id, value),
            BuiltinUnit::Drive(u) => u.set_param(id, value),
            BuiltinUnit::Trem(u) => u.set_param(id, value),
            BuiltinUnit::Chorus(u) => u.set_param(id, value),
            BuiltinUnit::Echo(u) => u.set_param(id, value),
            BuiltinUnit::Plate(u) => u.set_param(id, value),
            BuiltinUnit::Wah(u) => u.set_param(id, value),
            BuiltinUnit::Ir(u) => u.set_param(id, value),
            BuiltinUnit::Analog(u) => u.set_param(id, value),
            BuiltinUnit::Sampler(u) => u.set_param(id, value),
            BuiltinUnit::Bitcrush(u) => u.set_param(id, value),
            BuiltinUnit::HwInsert(u) => u.set_param(id, value),
        }
    }

    /// Apply a blob's EXTRA state — the lines beyond the knobs — to a fresh
    /// instance, HOST-SIDE, before it crosses to the audio thread. This is
    /// where the denoise gets its learned thresholds back and where the IR
    /// unit reads its impulse file off disk (allocation and file IO, which
    /// is exactly why it may never run on the audio thread; `Convolver::set_ir`
    /// documents the same contract from below).
    ///
    /// `Some(sentence)` reports what did not restore, in the voice of a
    /// failed plugin state: the unit still loads and runs — the IR with
    /// nothing loaded is the kernel's own bit-exact wire — because a missing
    /// file must cost the session one insert's colour, not the session.
    pub fn apply_extra_state(&mut self, extra: &[(String, String)]) -> Option<String> {
        match self {
            BuiltinUnit::Denoise(u) => {
                use crate::units::denoise::DENOISE_BANDS;
                // `t<n>` lines are the print, `k<n>` the bank's knees; a blob
                // from before the knees existed has only the first, and
                // `set_thresholds` gives such a print knees equal to its
                // thresholds - the reading every bank print had anyway.
                let mut thresholds = [0.0f32; DENOISE_BANDS];
                let mut knees = [0.0f32; DENOISE_BANDS];
                let (mut any_t, mut any_k) = (false, false);
                for (symbol, value) in extra {
                    let Some(prefix) = symbol.chars().next().filter(|c| *c == 't' || *c == 'k') else {
                        continue;
                    };
                    let Some(k) = symbol[1..].parse::<usize>().ok().filter(|k| *k < DENOISE_BANDS)
                    else {
                        continue;
                    };
                    let Ok(v) = value.parse::<f32>() else {
                        continue;
                    };
                    if prefix == 't' {
                        thresholds[k] = v;
                        any_t = true;
                    } else {
                        knees[k] = v;
                        any_k = true;
                    }
                }
                if any_t {
                    u.kernel.set_thresholds(&thresholds);
                }
                if any_k {
                    u.kernel.set_knees(&knees);
                }
                None
            }
            BuiltinUnit::Ir(u) => {
                let path = extra
                    .iter()
                    .find(|(s, _)| s == "ir_path")
                    .map(|(_, v)| v.as_str())?;
                match crate::wavread::read(std::path::Path::new(path)) {
                    Ok(wav) => u.load_ir(&wav),
                    Err(e) => Some(format!("its impulse response did not load: {e}")),
                }
            }
            // The jacks and the measured loop. Nothing to fail: a side not
            // named is a side not chosen, which the face asks for.
            BuiltinUnit::HwInsert(u) => {
                let (send, ret, measured) = crate::units::hwinsert::parse_lines(extra);
                u.set_routes(send, ret);
                u.set_measured(measured);
                None
            }
            _ => None,
        }
    }

    /// How hard a unit's gain element has been working since this was last
    /// asked, in positive dB of reduction — `Some` only for a unit that HAS
    /// one. Read-and-reset, like the meter it feeds: the engine drains it
    /// after each block into a per-bay cell, and the face reads the cell.
    pub fn gain_reduction_db(&mut self) -> Option<f32> {
        match self {
            BuiltinUnit::Comp(u) => Some(u.kernel.gain_reduction_db()),
            _ => None,
        }
    }

    /// This block's automation, applied the way the LV2 arm applies it: the
    /// LAST point per parameter wins, because the points arrive in offset
    /// order and later writes overwrite earlier ones. Sample-accurate points
    /// are an honesty the first version does not claim; the per-sample slew
    /// below keeps the block boundary from clicking meanwhile.
    pub fn apply(&mut self, points: &[tangent_host::ParamPoint]) {
        for p in points {
            self.set_param(p.id, p.value as f32);
        }
    }

    /// The channel in, the channel out — the planar contract `run_rack`
    /// already holds every effect to. Whatever `out` held before is
    /// overwritten for `frames`; channels past what either side carries are
    /// left alone, as the VST3 path leaves them.
    pub fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        match self {
            BuiltinUnit::Trim(t) => t.process(input, frames, out),
            BuiltinUnit::Eq(u) => u.process(input, frames, out),
            BuiltinUnit::Imager(u) => u.process(input, frames, out),
            BuiltinUnit::Tape(u) => u.process(input, frames, out),
            BuiltinUnit::Comp(u) => u.process(input, frames, out),
            BuiltinUnit::Denoise(u) => u.process(input, frames, out),
            BuiltinUnit::Amp(u) => u.process(input, frames, out),
            BuiltinUnit::Drive(u) => u.process(input, frames, out),
            BuiltinUnit::Trem(u) => u.process(input, frames, out),
            BuiltinUnit::Chorus(u) => u.process(input, frames, out),
            BuiltinUnit::Echo(u) => u.process(input, frames, out),
            BuiltinUnit::Plate(u) => u.process(input, frames, out),
            BuiltinUnit::Wah(u) => u.process(input, frames, out),
            BuiltinUnit::Ir(u) => u.process(input, frames, out),
            BuiltinUnit::Bitcrush(u) => u.process(input, frames, out),
            BuiltinUnit::HwInsert(u) => u.process(input, frames, out),
            // A voice asked to work as an effect has no input to transform
            // and no notes to play: it answers silence rather than passing
            // the input through as if it had done something. `run_rack`
            // never asks — `voice: true` routes it to `process_voice` — so
            // this arm is the same kind of line the loader's used to be.
            BuiltinUnit::Analog(_) | BuiltinUnit::Sampler(_) => {
                for ch in out.iter_mut() {
                    for s in ch.iter_mut().take(frames) {
                        *s = 0.0;
                    }
                }
            }
        }
    }

    /// **Render a voice: notes in, the plugin's own buffers out** — the
    /// VST3 `process_voice` contract, for the built-ins that take notes.
    /// `params` is this block's automation and `transport` the song's
    /// position, both of which the effects' road carries past a voice; the
    /// analog reads both (the arpeggiator needs the second). Every unit that
    /// is an effect answers silence, the arm above's line.
    ///
    /// **`sample` is the LANE's loaded file, borrowed for this block.** It is
    /// an argument and not a field for the reason `units::sampler`'s header
    /// gives at length: the engine holds the decoded clip in one per-lane
    /// place so that a displaced sample is freed on the host thread, and a
    /// unit that kept its own `Arc` would put that free on the callback. It
    /// arrives here rather than at `set_param` time because a bay has no way
    /// to ask for anything between blocks — the road in is the block.
    pub fn process_voice(
        &mut self,
        notes: &[tangent_host::Note],
        controls: &[tangent_host::Control],
        params: &[tangent_host::ParamPoint],
        transport: crate::analog::arp::Transport,
        sample: Option<&crate::instrument::SamplerSample>,
        frames: usize,
        bufs: &mut [Vec<f32>],
    ) {
        match self {
            BuiltinUnit::Analog(u) => u.process_voice(notes, controls, params, transport, frames, bufs),
            BuiltinUnit::Sampler(u) => u.process_voice(notes, controls, params, sample, frames, bufs),
            _ => {
                for ch in bufs.iter_mut() {
                    for s in ch.iter_mut().take(frames) {
                        *s = 0.0;
                    }
                }
            }
        }
    }

    /// The denoise's learned thresholds, when this block may have moved them
    /// — `Some` while the learn dial is up and for ONE block after it falls,
    /// which is the block the kernel froze them on. The engine forwards the
    /// snapshot into a shared cell the host adopts into its ledger; every
    /// other unit answers `None`, the `gain_reduction_db` shape exactly.
    pub fn learn_snapshot(
        &mut self,
    ) -> Option<(
        [f32; crate::units::denoise::DENOISE_BANDS],
        [f32; crate::units::denoise::DENOISE_BANDS],
    )> {
        match self {
            BuiltinUnit::Denoise(u) => u.publish.then(|| (u.kernel.thresholds(), u.kernel.knees())),
            _ => None,
        }
    }

    /// [`BuiltinUnit::process`] with the block's sidechain, when the desk
    /// armed one. Only the comp has a detector to point at it; every other
    /// unit ignores the key rather than erroring, because "this knob does
    /// nothing here" is the desk's to grey, not the kernel's to refuse.
    pub fn process_keyed(
        &mut self,
        input: &[Vec<f32>],
        frames: usize,
        out: &mut [Vec<f32>],
        key: Option<&[Vec<f32>]>,
    ) {
        match (self, key) {
            (BuiltinUnit::Comp(u), Some(k)) => u.process_keyed(input, frames, out, k),
            (me, _) => me.process(input, frames, out),
        }
    }

    /// **Whether a bypassed bay of this unit keeps the delay it reports.**
    ///
    /// A bypassed bay's figure stays in the compensation graph, so a unit
    /// with a delay that let its input past untouched would play its lane
    /// that much early against the rest of the mix. The rack sends a bay of
    /// a unit that answers yes through [`BuiltinUnit::bypassed`] instead of
    /// past it: the hardware insert (its loop's round trip, the owner's
    /// ruling), the tape (its play head), the denoise (its lookahead or its
    /// spectral frame) and the convolver (its partition, once an IR is in),
    /// the last two through a [`DryLine`]. Every other built-in reports
    /// nothing a bypass could owe.
    pub fn keeps_its_delay_when_bypassed(&self) -> bool {
        matches!(
            self,
            BuiltinUnit::HwInsert(_) | BuiltinUnit::Tape(_) | BuiltinUnit::Denoise(_) | BuiltinUnit::Ir(_)
        )
    }

    /// The bay bypassed, for a unit that
    /// [keeps its delay](BuiltinUnit::keeps_its_delay_when_bypassed): its
    /// input, as late as its latency says. Any other unit answers the
    /// untouched signal, which is what its bypass is.
    pub fn bypassed(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        match self {
            BuiltinUnit::HwInsert(u) => u.bypassed(input, frames, out),
            BuiltinUnit::Tape(u) => u.bypassed(input, frames, out),
            BuiltinUnit::Denoise(u) => {
                let late = u.kernel.latency() as usize;
                u.dry.pass(input, frames, out, late);
            }
            BuiltinUnit::Ir(u) => {
                let late = if u.has_ir { u.kernel.latency() as usize } else { 0 };
                u.dry.pass(input, frames, out, late);
            }
            _ => {
                for (src, dst) in input.iter().zip(out.iter_mut()) {
                    let n = frames.min(src.len()).min(dst.len());
                    dst[..n].copy_from_slice(&src[..n]);
                }
            }
        }
    }

    /// Drop whatever the unit remembers of the signal, the way every unit in
    /// `effects.rs` can. The trim remembers only where its slew is, and
    /// clearing that snaps the gain to its target.
    pub fn clear(&mut self) {
        match self {
            BuiltinUnit::Trim(t) => t.clear(),
            BuiltinUnit::Eq(u) => u.kernel.clear(),
            BuiltinUnit::Imager(u) => u.kernel.clear(),
            // The tape, and the dry line its bypass keeps.
            BuiltinUnit::Tape(u) => u.clear(),
            BuiltinUnit::Comp(u) => u.kernel.clear(),
            // The denoise's clear keeps what was LEARNED — its kernel
            // documents "forget what was playing, keep the thresholds",
            // which is exactly what a restored preset needs from the
            // `create`-then-`clear` load path. Seating the ENGINE is
            // `DenoiseUnit::set_param`'s job and is deliberately not
            // duplicated here: `create` writes every parameter through
            // `set_param` before it calls this, `set_param` primes on every
            // write, and a second call would be a guard no revert could
            // turn red.
            BuiltinUnit::Denoise(u) => {
                u.kernel.clear();
                u.dry.clear();
            }
            BuiltinUnit::Amp(u) => u.kernel.clear(),
            BuiltinUnit::Drive(u) => u.kernel.clear(),
            BuiltinUnit::Trem(u) => u.kernel.clear(),
            BuiltinUnit::Chorus(u) => u.kernel.clear(),
            BuiltinUnit::Echo(u) => u.kernel.clear(),
            BuiltinUnit::Plate(u) => u.kernel.clear(),
            BuiltinUnit::Wah(u) => u.kernel.clear(),
            // And the convolver's clear keeps the IR itself: tails go, the
            // loaded filter stays, the same knowledge-vs-signal split.
            BuiltinUnit::Ir(u) => {
                u.kernel.clear();
                u.dry.clear();
            }
            // Every voice off, the dials settled where they stand.
            BuiltinUnit::Analog(u) => u.synth.clear(),
            // The same, and it keeps NOTHING about the file — because it
            // holds nothing about the file. The convolver's clear has to say
            // "tails go, the loaded filter stays"; a sampler borrows its
            // audio for the length of a block and has no such split.
            BuiltinUnit::Sampler(u) => u.kernel.clear(),
            // The holds empty and the knobs settle where they stand.
            BuiltinUnit::Bitcrush(u) => u.kernel.clear(),
            // The delay line and the alignment go; the jacks, the trims and
            // the measured loop are knowledge and stay.
            BuiltinUnit::HwInsert(u) => u.clear(),
        }
    }
}

// ── the planar adapter ──────────────────────────────────────────────────────

/// How many frames of interleaved stereo the per-unit scratch holds — the
/// engine's own block cap (`MAX_BLOCK` in `instrument.rs`, restated because
/// this module must stand without that one). Not a correctness bound: a
/// larger block is CHUNKED through the scratch below, and the kernels cannot
/// tell — their state carries across calls, which is all a block boundary is.
const SCRATCH_FRAMES: usize = 512;

/// Run an interleaved in-place stereo kernel over the planar in/out contract
/// `run_rack` speaks, through a preallocated scratch, allocation-free.
///
/// The four kernels under `crate::units` all process interleaved stereo in
/// place — the `Effects::process` shape they were proven in — while a bay
/// tenant is handed planar buffers. This is the one bridge: interleave a
/// chunk, run the kernel, deinterleave into `out`. Fewer than two channels
/// on either side is a copy-through, the kernels' own "a mono buffer is
/// returned untouched rather than half-processed" contract restated at the
/// seam where it must hold.
fn run_planar(
    scratch: &mut [f32],
    input: &[Vec<f32>],
    frames: usize,
    out: &mut [Vec<f32>],
    mut kernel: impl FnMut(&mut [f32], usize),
) {
    if input.len() < 2 || out.len() < 2 {
        for (src, dst) in input.iter().zip(out.iter_mut()) {
            let n = frames.min(src.len()).min(dst.len());
            dst[..n].copy_from_slice(&src[..n]);
        }
        return;
    }
    let step = (scratch.len() / 2).max(1);
    let mut done = 0usize;
    while done < frames {
        let n = step.min(frames - done);
        for f in 0..n {
            scratch[f * 2] = input[0].get(done + f).copied().unwrap_or(0.0);
            scratch[f * 2 + 1] = input[1].get(done + f).copied().unwrap_or(0.0);
        }
        kernel(&mut scratch[..n * 2], n);
        for f in 0..n {
            if let Some(d) = out[0].get_mut(done + f) {
                *d = scratch[f * 2];
            }
            if let Some(d) = out[1].get_mut(done + f) {
                *d = scratch[f * 2 + 1];
            }
        }
        done += n;
    }
}

/// A knob value with the wire's dirt washed off: clamped into the travel,
/// and a non-finite reading lands on the SPEC'S default rather than an
/// arbitrary midpoint — the `ImagerParams::sane` policy, applied once at the
/// seam so every kernel behind it sees a number.
fn washed(spec: &ParamSpec, value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        spec.default
    }
}

// ── Tangent EQ, seated ──────────────────────────────────────────────────────

/// The EQ kernel behind its face table: three of the kernel's eight bands,
/// five ids each. Bands the face does not name stay at the kernel's default
/// — off — forever.
pub struct EqUnit {
    kernel: crate::units::eq::EqKernel,
    params: crate::units::eq::EqParams,
    scratch: Vec<f32>,
}

impl EqUnit {
    fn new(sample_rate: f32) -> Self {
        let mut params = crate::units::eq::EqParams::default();
        // The face's parking spots, not the eight-handle ruler's: the specs
        // are the published defaults, so the kernel must open agreeing with
        // them or the first `state_text` save would move handles nobody
        // touched.
        for (node, spec) in params
            .nodes
            .iter_mut()
            .zip(EQ_FACE_PARAMS.chunks(EQ_KNOBS_PER_NODE as usize))
        {
            node.freq = spec[2].default;
        }
        Self {
            kernel: crate::units::eq::EqKernel::new(sample_rate),
            params,
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        use crate::units::eq::NodeKind;
        let node = (id / EQ_KNOBS_PER_NODE) as usize;
        if node >= EQ_FACE_NODES {
            return;
        }
        let Some(spec) = EQ_FACE_PARAMS.get(id as usize) else {
            return;
        };
        let v = washed(spec, value);
        let n = &mut self.params.nodes[node];
        match id % EQ_KNOBS_PER_NODE {
            // A toggle reads its upper half as on, the stepped convention.
            0 => n.on = v >= 0.5,
            // The menu convention: N positions across the travel, nearest
            // wins, so 0.0 is the first shape and 1.0 the last.
            1 => {
                let last = (NodeKind::ALL.len() - 1) as f32;
                let at = ((v * last).round() as usize).min(NodeKind::ALL.len() - 1);
                n.kind = NodeKind::ALL[at];
            }
            2 => n.freq = v,
            3 => n.gain = v,
            _ => n.q = v,
        }
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let (kernel, params) = (&mut self.kernel, &self.params);
        run_planar(&mut self.scratch, input, frames, out, |buf, n| {
            kernel.process(buf, n, 2, params);
        });
    }
}

// ── Tangent Imager, seated ──────────────────────────────────────────────────

pub struct ImagerUnit {
    kernel: crate::units::imager::Imager,
    params: crate::units::imager::ImagerParams,
    scratch: Vec<f32>,
}

impl ImagerUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            kernel: crate::units::imager::Imager::new(sample_rate),
            params: crate::units::imager::ImagerParams::default(),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(spec) = IMAGER_FACE_PARAMS.get(id as usize) else {
            return;
        };
        let v = washed(spec, value);
        match id {
            0 => self.params.width = v,
            1 => self.params.ensemble = v,
            _ => self.params.mono_below = v,
        }
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let (kernel, params) = (&mut self.kernel, &self.params);
        run_planar(&mut self.scratch, input, frames, out, |buf, n| {
            kernel.process(buf, n, 2, params);
        });
    }
}

// ── Tangent Tape, seated ────────────────────────────────────────────────────

/// How long the seat's fade between the tape and its own dry takes, either
/// way, in milliseconds.
///
/// **An equal-gain fade, because the two are one signal at one time.** The
/// dry line holds the input exactly as late as the tape reports, and the
/// tape's play head sways around that figure and never off it on average,
/// so a linear fade between them is a fade between two takes of the same
/// sound with nothing in it to cancel. Ten milliseconds is too short to
/// hear as a fade and long enough that neither end can click.
const TAPE_BYPASS_FADE_MS: f32 = 10.0;

/// How long the dry line takes to move from one head's delay to the other's
/// when the wow range is switched while the bay is bypassed, in
/// milliseconds: the kernel's own head crossfade, so a switch sounds the same
/// in circuit and out.
const TAPE_DRY_FADE_MS: f32 = 25.0;

/// How long a kernel started again after a bypass plays dry beyond the
/// tape's own delay, before the fade back to it, in milliseconds.
///
/// A stopped kernel is cleared before it runs again, so it never replays what
/// was on its tape before the bay went out. Its head then reads the silence
/// of a clean tape for exactly the tape's delay while the new signal is
/// recorded, and its filters settle from rest; the dry covers both, the
/// delay and this much more, and only then fades back to the tape.
const TAPE_REFILL_MS: f32 = 5.0;

/// **The one map from a tape control's id to the field it drives**: an arm
/// per row of [`TAPE_FACE_PARAMS`], and nothing for an id the table does not
/// declare.
///
/// Every id names its field, and an id with no arm writes nothing.
/// `TapeUnit::set_param` used to end in a catch-all into the trim, which was
/// harmless while the table stopped at the trim and would not stay so: a row
/// appended to the table without an arm would have driven the output level,
/// and the seat's sweep would still have passed, because the trim moves the
/// output. One map rather than a match inside `set_param`, so whatever else
/// has to turn a bay's values into the kernel's params reads the same arms
/// and cannot grow a second map that disagrees with this one.
/// `every_tape_id_reaches_its_own_field` holds every arm to its row.
fn tape_slot(p: &mut crate::units::tape::TapeParams, id: u32) -> Option<&mut f32> {
    match id {
        0 => Some(&mut p.drive),
        1 => Some(&mut p.bump),
        2 => Some(&mut p.roll),
        3 => Some(&mut p.wobble),
        4 => Some(&mut p.hiss),
        5 => Some(&mut p.trim),
        6 => Some(&mut p.speed),
        7 => Some(&mut p.flutter),
        8 => Some(&mut p.rate),
        9 => Some(&mut p.wow_range),
        10 => Some(&mut p.bias),
        11 => Some(&mut p.age),
        12 => Some(&mut p.damage),
        _ => None,
    }
}

pub struct TapeUnit {
    kernel: crate::units::tape::Tape,
    params: crate::units::tape::TapeParams,
    scratch: Vec<f32>,
    /// **The input, as late as the tape.** A line per channel, written every
    /// block whether the bay is in circuit or not and read at the latency
    /// the kernel reports, so a bypassed bay is heard where the tape would
    /// have been. Sized at build for the most a tape can ever report.
    dry: [Vec<f32>; 2],
    dry_at: usize,
    /// **The dry line's two taps**: the delay it is reading at, the one it
    /// is fading from, and how far through that equal-power fade it is (1 is
    /// settled). A wow range switch moves the report between the two heads'
    /// delays, and the dry follows it over [`TAPE_DRY_FADE_MS`], as the
    /// kernel's heads do; a switch back mid-fade turns it round from where it
    /// is. The first block after `new` or `clear` starts on the report.
    dry_late: usize,
    dry_was: usize,
    dry_fade: f32,
    dry_step: f32,
    dry_fresh: bool,
    /// How much of the output is the dry: 0 in circuit, 1 bypassed, and
    /// between while one fades into the other.
    mix: f32,
    /// Whether the kernel has stopped. A bypassed bay runs it only for the
    /// fade out; after that it costs nothing until the bay is back in
    /// circuit, and then it starts again from a clean tape.
    stale: bool,
    /// Whether the kernel has run since the last clear. A bay opened
    /// bypassed has nothing on its tape to fade away from, so it goes to the
    /// dry at once.
    live: bool,
    /// Frames of dry still owed while a kernel started again refills its
    /// tape, before the fade back to it begins.
    warm: usize,
    /// How far `mix` moves per frame: a whole fade in
    /// [`TAPE_BYPASS_FADE_MS`].
    fade: f32,
    /// [`TAPE_REFILL_MS`] in frames at this unit's rate.
    refill: usize,
    /// How many blocks the kernel has been run, so a test can hold a
    /// bypassed bay to costing nothing.
    #[cfg(test)]
    kernel_blocks: usize,
}

impl TapeUnit {
    fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let line = crate::units::tape::Tape::max_latency_frames(sample_rate) + 8;
        Self {
            kernel: crate::units::tape::Tape::new(sample_rate),
            params: crate::units::tape::TapeParams::default(),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
            dry: [vec![0.0; line], vec![0.0; line]],
            dry_at: 0,
            dry_late: 0,
            dry_was: 0,
            dry_fade: 1.0,
            dry_step: 1.0 / (TAPE_DRY_FADE_MS * 1.0e-3 * sr).max(1.0),
            dry_fresh: true,
            mix: 0.0,
            stale: false,
            live: false,
            warm: 0,
            fade: 1.0 / (TAPE_BYPASS_FADE_MS * 1.0e-3 * sr).max(1.0),
            refill: (TAPE_REFILL_MS * 1.0e-3 * sr).round() as usize,
            #[cfg(test)]
            kernel_blocks: 0,
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(spec) = TAPE_FACE_PARAMS.get(id as usize) else {
            return;
        };
        if let Some(slot) = tape_slot(&mut self.params, id) {
            *slot = washed(spec, value);
        }
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        self.run(input, frames, out, false);
    }

    /// **The bay bypassed: its input, exactly as late as the tape it
    /// stands in for.** The latency the tape reports stays in the
    /// compensation graph while the bay is out, as every bypass in the rack
    /// keeps it, so a bypass that passed the input untouched would play the
    /// lane that many frames early against the rest of the mix: two
    /// milliseconds, a flam on every A/B. So the dry leaves through a line
    /// as long as the report. Going out, the kernel keeps running for
    /// [`TAPE_BYPASS_FADE_MS`] while the output fades to the dry, then
    /// stops and costs nothing; a bay opened bypassed has no tape to fade
    /// from and goes to the dry at once. Coming back, see
    /// [`TAPE_REFILL_MS`]. A wow range switch while bypassed moves the dry
    /// between the two heads' delays over [`TAPE_DRY_FADE_MS`].
    pub fn bypassed(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        self.run(input, frames, out, true);
    }

    /// One block, in circuit or bypassed: the one road both take, so the dry
    /// line is written either way and the fade between the two is one
    /// state whichever way it is going, a fade reversed halfway included.
    fn run(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>], bypassed: bool) {
        // Fewer than two channels is the planar adapter's copy-through, in
        // circuit and out: a mono bay never runs the tape, so it has no
        // delay of the tape's to keep.
        if input.len() < 2 || out.len() < 2 {
            run_planar(&mut self.scratch, input, frames, out, |_, _| {});
            return;
        }
        let late = self.kernel.latency_frames(&self.params);
        if self.dry_fresh {
            // Nothing has left the line since it was cleared, so there is no
            // fade to have been in: start on the report.
            (self.dry_late, self.dry_was, self.dry_fade) = (late, late, 1.0);
            self.dry_fresh = false;
        } else if late != self.dry_late {
            if late == self.dry_was && self.dry_fade < 1.0 {
                std::mem::swap(&mut self.dry_late, &mut self.dry_was);
                self.dry_fade = 1.0 - self.dry_fade;
            } else {
                (self.dry_was, self.dry_late, self.dry_fade) = (self.dry_late, late, 0.0);
            }
        }
        if bypassed && !self.live {
            self.mix = 1.0;
            self.stale = true;
        }
        if !bypassed && self.stale {
            self.kernel.clear();
            self.stale = false;
            self.warm = late + self.refill;
        }
        let wet = !self.stale;
        if wet {
            let (kernel, params) = (&mut self.kernel, &self.params);
            run_planar(&mut self.scratch, input, frames, out, |buf, n| {
                kernel.process(buf, n, 2, params);
            });
            self.live = true;
            #[cfg(test)]
            {
                self.kernel_blocks += 1;
            }
        }
        let line = self.dry[0].len();
        for f in 0..frames {
            for (ch, d) in self.dry.iter_mut().enumerate() {
                d[self.dry_at] = input[ch].get(f).copied().unwrap_or(0.0);
            }
            let back = (self.dry_at + line - self.dry_late) % line;
            let mut dry = [self.dry[0][back], self.dry[1][back]];
            if self.dry_fade < 1.0 {
                let then = (self.dry_at + line - self.dry_was) % line;
                let (now, was) = (self.dry_fade * std::f32::consts::FRAC_PI_2).sin_cos();
                for (d, tape) in dry.iter_mut().zip(&self.dry) {
                    *d = was * tape[then] + now * *d;
                }
                self.dry_fade = (self.dry_fade + self.dry_step).min(1.0);
            }
            self.dry_at = (self.dry_at + 1) % line;
            if bypassed {
                self.mix = (self.mix + self.fade).min(1.0);
            } else if self.warm > 0 {
                self.warm -= 1;
            } else {
                self.mix = (self.mix - self.fade).max(0.0);
            }
            let m = self.mix;
            if m > 0.0 {
                for (o, d) in out.iter_mut().zip(dry) {
                    if let Some(o) = o.get_mut(f) {
                        *o = if m >= 1.0 { d } else { *o + (d - *o) * m };
                    }
                }
            }
        }
        if bypassed && self.mix >= 1.0 {
            self.stale = true;
        }
    }

    /// Forget the tape and the dry with it, and stand in circuit: a bay
    /// that loads bypassed is told so by its first block.
    fn clear(&mut self) {
        self.kernel.clear();
        for d in &mut self.dry {
            d.fill(0.0);
        }
        self.dry_at = 0;
        self.dry_fresh = true;
        self.mix = 0.0;
        self.stale = false;
        self.live = false;
        self.warm = 0;
    }
}

/// **A tape bay's values as the kernel's parameters**, for the arithmetic
/// the host does beside the audio (`desktop::tape_notes`): every id through
/// [`tape_slot`] and `washed`, the road `TapeUnit::set_param` takes, so the
/// two cannot disagree about which value is which. A value the slice lacks
/// reads as its row's default.
pub(crate) fn tape_params_of(values: &[f32]) -> crate::units::tape::TapeParams {
    let mut p = crate::units::tape::TapeParams::default();
    for spec in TAPE_FACE_PARAMS {
        let v = values.get(spec.id as usize).copied().unwrap_or(spec.default);
        if let Some(slot) = tape_slot(&mut p, spec.id) {
            *slot = washed(spec, v);
        }
    }
    p
}

// ── Tangent Comp, seated ────────────────────────────────────────────────────

pub struct CompUnit {
    kernel: crate::units::comp::Comp,
    params: crate::units::comp::CompParams,
    scratch: Vec<f32>,
    /// The key's own interleave scratch, beside the main one: the kernel
    /// wants both signals interleaved and aligned chunk for chunk, and
    /// sharing one buffer would overwrite the audio with its own key.
    /// Bought at build, on the UI thread, like every scratch here.
    key_scratch: Vec<f32>,
}

impl CompUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            kernel: crate::units::comp::Comp::new(sample_rate),
            params: crate::units::comp::CompParams::default(),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
            key_scratch: vec![0.0; SCRATCH_FRAMES * 2],
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(spec) = COMP_FACE_PARAMS.get(id as usize) else {
            return;
        };
        let v = washed(spec, value);
        match id {
            0 => self.params.threshold = v,
            1 => self.params.ratio = v,
            2 => self.params.attack = v,
            3 => self.params.release = v,
            4 => self.params.makeup = v,
            5 => self.params.mix = v,
            _ => self.params.key_hpf = v,
        }
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let (kernel, params) = (&mut self.kernel, &self.params);
        run_planar(&mut self.scratch, input, frames, out, |buf, n| {
            // No key on THIS call — the unkeyed passes (a send
            // destination's second run, for one) come through here, and the
            // detector honestly follows the main signal on them. The keyed
            // path is `process_keyed` below.
            kernel.process(buf, n, 2, params, None);
        });
    }

    /// The keyed path: the same chunked interleave bridge as `process`, with
    /// the key's planar pair interleaved beside the audio, chunk-aligned, so
    /// the kernel's `k[2f]`/`k[2f+1]` indexing lines up with the frames it is
    /// gaining. Mono keys repeat their one channel; a key channel shorter
    /// than the block reads as silence past its end, never as a neighbour's
    /// samples.
    fn process_keyed(
        &mut self,
        input: &[Vec<f32>],
        frames: usize,
        out: &mut [Vec<f32>],
        key: &[Vec<f32>],
    ) {
        // Fewer than two channels on either side is `run_planar`'s
        // copy-through contract, restated because this bridge is its twin.
        if input.len() < 2 || out.len() < 2 {
            for (src, dst) in input.iter().zip(out.iter_mut()) {
                let n = frames.min(src.len()).min(dst.len());
                dst[..n].copy_from_slice(&src[..n]);
            }
            return;
        }
        let step = (self.scratch.len() / 2).max(1).min((self.key_scratch.len() / 2).max(1));
        let mut done = 0usize;
        while done < frames {
            let n = step.min(frames - done);
            for f in 0..n {
                self.scratch[f * 2] = input[0].get(done + f).copied().unwrap_or(0.0);
                self.scratch[f * 2 + 1] = input[1].get(done + f).copied().unwrap_or(0.0);
                let kl = key.first().and_then(|k| k.get(done + f)).copied().unwrap_or(0.0);
                let kr = key
                    .get(1)
                    .or_else(|| key.first())
                    .and_then(|k| k.get(done + f))
                    .copied()
                    .unwrap_or(0.0);
                self.key_scratch[f * 2] = kl;
                self.key_scratch[f * 2 + 1] = kr;
            }
            self.kernel.process(
                &mut self.scratch[..n * 2],
                n,
                2,
                &self.params,
                Some(&self.key_scratch[..n * 2]),
            );
            for f in 0..n {
                if let Some(d) = out[0].get_mut(done + f) {
                    *d = self.scratch[f * 2];
                }
                if let Some(d) = out[1].get_mut(done + f) {
                    *d = self.scratch[f * 2 + 1];
                }
            }
            done += n;
        }
    }
}

// ── Tangent Denoise, seated ─────────────────────────────────────────────────

/// The learned expander bank behind its face. Beyond the shared shape it
/// carries the learn edge-detector that feeds [`BuiltinUnit::learn_snapshot`]:
/// the kernel freezes thresholds on the block AFTER the dial falls, so the
/// snapshot must stay offered for exactly one block past the fall.
/// **A bypassed bay's input, exactly as late as the unit says it is.**
///
/// A bypassed bay's latency stays in the compensation graph (the rack's
/// bypass arm keeps it, so a bypass never re-times the desk), which means a
/// unit with a delay that let its input past untouched would play its lane
/// that many frames early: the denoise's 3 ms lookahead or its spectral
/// frame, the convolver's 512-frame partition, a flam on every A/B. So the
/// unit writes its input here on every block, in circuit or out, and its
/// bypass reads it back at the latency it reports. Sized once, at build, for
/// the most that unit can ever report; nothing here allocates afterwards.
/// The switch itself is the instant one it always was: what changes is that
/// the dry side now lands where the processed side did.
struct DryLine {
    line: [Vec<f32>; 2],
    at: usize,
}

impl DryLine {
    fn new(max_late: usize) -> Self {
        Self { line: [vec![0.0; max_late + 1], vec![0.0; max_late + 1]], at: 0 }
    }

    /// Remember this block's input, the in-circuit half: a bay switched out
    /// mid-song has its history ready.
    fn write(&mut self, input: &[Vec<f32>], frames: usize) {
        let len = self.line[0].len();
        for f in 0..frames {
            for (ch, d) in self.line.iter_mut().enumerate() {
                d[self.at] = input.get(ch).and_then(|c| c.get(f)).copied().unwrap_or(0.0);
            }
            self.at = (self.at + 1) % len;
        }
    }

    /// Remember this block's input and play it `late` frames back, the
    /// bypassed half. `late` is held inside the line, so a figure the line
    /// was not sized for reads its far end rather than out of bounds; a
    /// third channel or more, which no unit here processes, goes past as it
    /// always did.
    fn pass(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>], late: usize) {
        let len = self.line[0].len();
        let late = late.min(len - 1);
        for f in 0..frames {
            let back = (self.at + len - late) % len;
            for (ch, d) in self.line.iter_mut().enumerate() {
                d[self.at] = input.get(ch).and_then(|c| c.get(f)).copied().unwrap_or(0.0);
                if let Some(o) = out.get_mut(ch).and_then(|c| c.get_mut(f)) {
                    *o = d[back];
                }
            }
            self.at = (self.at + 1) % len;
        }
        for (src, dst) in input.iter().zip(out.iter_mut()).skip(2) {
            let n = frames.min(src.len()).min(dst.len());
            dst[..n].copy_from_slice(&src[..n]);
        }
    }

    fn clear(&mut self) {
        for d in &mut self.line {
            d.fill(0.0);
        }
        self.at = 0;
    }
}

pub struct DenoiseUnit {
    kernel: crate::units::denoise::Denoise,
    params: crate::units::denoise::DenoiseParams,
    scratch: Vec<f32>,
    /// Whether the LAST processed block ran with the learn dial up.
    last_learn: bool,
    /// Whether this block's thresholds are worth publishing — learn up, or
    /// the falling-edge block that froze them.
    publish: bool,
    /// The input as late as the lookahead or the frame, for the bypass.
    dry: DryLine,
}

impl DenoiseUnit {
    fn new(sample_rate: f32) -> Self {
        let kernel = crate::units::denoise::Denoise::new(sample_rate);
        let dry = DryLine::new(kernel.max_latency() as usize);
        Self {
            kernel,
            params: crate::units::denoise::DenoiseParams::default(),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
            last_learn: false,
            publish: false,
            dry,
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(spec) = DENOISE_FACE_PARAMS.get(id as usize) else {
            return;
        };
        let v = washed(spec, value);
        match id {
            0 => self.params.learn = v,
            1 => self.params.reduce = v,
            2 => self.params.smooth = v,
            3 => self.params.release = v,
            4 => self.params.sc_hpf = v,
            5 => self.params.lookahead = v,
            6 => self.params.kernel = v,
            7 => self.params.attack = v,
            _ => self.params.treble = v,
        }
        // **The selector and the lookahead toggle move LATENCY, and the
        // kernel answers about them from its own fields, not from these.**
        // Those fields were only ever written inside `process`, so a unit
        // built from a blob answered with the CONSTRUCTOR's engine until it
        // had seen a block: `UnitSpec::create` sets the parameters and
        // `prepare_builtin` reads `latency()` the next line, so a bay
        // restored on Bank reported the spectral frame instead of zero, and
        // for a bay restored BYPASSED it kept reporting it — the block loop
        // is the only correction road and it is skipped for a bypassed bay
        // by design. `prime` runs the same two edges `process` runs, in the
        // same order, allocation-free, so it is safe from here.
        self.kernel.prime(&self.params);
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let learn_now = self.params.learn >= 0.5;
        let (kernel, params) = (&mut self.kernel, &self.params);
        run_planar(&mut self.scratch, input, frames, out, |buf, n| {
            kernel.process(buf, n, 2, params);
        });
        self.dry.write(input, frames);
        self.publish = learn_now || self.last_learn;
        self.last_learn = learn_now;
    }
}

// ── Tangent Amp, seated ─────────────────────────────────────────────────────

/// The amp kernel behind its panel table.
///
/// The one wrinkle is the VIB switch: the kernel spells "vibrato off" as
/// depth zero, but a panel switch that zeroed the intensity knob would
/// forget where the player had it. So the switch and the intensity are two
/// face parameters, both persisted, and the kernel's depth is their product
/// — computed here, in the only place both are known.
pub struct AmpUnit {
    kernel: crate::units::amp::Amp,
    params: crate::units::amp::AmpParams,
    /// The VIB switch's own state; see the struct doc.
    vib_on: bool,
    /// Where the Intensity knob stands, kept apart from the depth the
    /// kernel sees so the switch can come back to it.
    intensity: f32,
    scratch: Vec<f32>,
}

impl AmpUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            kernel: crate::units::amp::Amp::new(sample_rate),
            params: crate::units::amp::AmpParams::default(),
            vib_on: false,
            intensity: 0.0,
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(spec) = AMP_FACE_PARAMS.get(id as usize) else {
            return;
        };
        let v = washed(spec, value);
        match id {
            0 => self.params.bright = v,
            1 => self.params.volume = v,
            2 => self.params.treble = v,
            3 => self.params.middle = v,
            4 => self.params.bass = v,
            5 => self.vib_on = v >= 0.5,
            6 => self.params.vib_speed = v,
            7 => self.intensity = v,
            _ => self.params.vib_stereo = v,
        }
        self.params.vib_depth = if self.vib_on { self.intensity } else { 0.0 };
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let (kernel, params) = (&mut self.kernel, &self.params);
        run_planar(&mut self.scratch, input, frames, out, |buf, n| {
            kernel.process(buf, n, 2, params);
        });
    }
}

// ── Tangent Drive, seated ───────────────────────────────────────────────────

pub struct DriveUnit {
    kernel: crate::units::drive::Drive,
    params: crate::units::drive::DriveParams,
    scratch: Vec<f32>,
}

impl DriveUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            kernel: crate::units::drive::Drive::new(sample_rate),
            params: crate::units::drive::DriveParams::default(),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(spec) = DRIVE_FACE_PARAMS.get(id as usize) else {
            return;
        };
        let v = washed(spec, value);
        match id {
            0 => self.params.drive = v,
            1 => self.params.tone = v,
            2 => self.params.level = v,
            _ => self.params.symmetry = v,
        }
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let (kernel, params) = (&mut self.kernel, &self.params);
        run_planar(&mut self.scratch, input, frames, out, |buf, n| {
            kernel.process(buf, n, 2, params);
        });
    }
}

// ── Tangent Trem, seated ────────────────────────────────────────────────────

pub struct TremUnit {
    kernel: crate::units::trem::Trem,
    params: crate::units::trem::TremParams,
    scratch: Vec<f32>,
}

impl TremUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            kernel: crate::units::trem::Trem::new(sample_rate),
            params: crate::units::trem::TremParams::default(),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(spec) = TREM_FACE_PARAMS.get(id as usize) else {
            return;
        };
        let v = washed(spec, value);
        match id {
            0 => self.params.speed = v,
            1 => self.params.depth = v,
            _ => self.params.stereo = v,
        }
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let (kernel, params) = (&mut self.kernel, &self.params);
        run_planar(&mut self.scratch, input, frames, out, |buf, n| {
            kernel.process(buf, n, 2, params);
        });
    }
}

// ── Tangent Bitcrush, seated ────────────────────────────────────────────────

/// The bitcrusher behind its panel table. The one difference from the
/// pedals around it is the road in: it crushes PLANAR, in `out`, rather than
/// through `run_planar`'s interleaving scratch, because that adapter hands a
/// mono channel back untouched and a bitcrusher on a mono lane should crush
/// it. The kernel is per-channel arithmetic on one shared clock, so planar
/// costs it nothing and saves the two copies.
pub struct BitcrushUnit {
    kernel: crate::units::bitcrush::Bitcrush,
    params: crate::units::bitcrush::BitcrushParams,
}

impl BitcrushUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            kernel: crate::units::bitcrush::Bitcrush::new(sample_rate),
            params: crate::units::bitcrush::BitcrushParams::default(),
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(spec) = BITCRUSH_FACE_PARAMS.get(id as usize) else {
            return;
        };
        let v = washed(spec, value);
        match id {
            0 => self.params.bits = v,
            1 => self.params.rate = v,
            2 => self.params.drive = v,
            3 => self.params.mix = v,
            4 => self.params.output = v,
            5 => self.params.dither = v,
            _ => self.params.aa = v,
        }
    }

    /// The input copied into `out`, then crushed there in place: one channel
    /// as mono, two as stereo, any past the second passed through as the
    /// copy left them.
    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let n = input.len().min(out.len());
        for (src, dst) in input.iter().zip(out.iter_mut()) {
            let m = frames.min(src.len()).min(dst.len());
            dst[..m].copy_from_slice(&src[..m]);
        }
        let crushed = n.min(crate::units::bitcrush::CRUSH_CHANNELS);
        self.kernel.process_planar(&mut out[..crushed], frames, &self.params);
    }
}

// ── Tangent Chorus, seated ──────────────────────────────────────────────────

pub struct ChorusUnit {
    kernel: crate::units::chorus::Chorus,
    params: crate::units::chorus::ChorusParams,
    scratch: Vec<f32>,
}

impl ChorusUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            kernel: crate::units::chorus::Chorus::new(sample_rate),
            params: crate::units::chorus::ChorusParams::default(),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(spec) = CHORUS_FACE_PARAMS.get(id as usize) else {
            return;
        };
        let v = washed(spec, value);
        match id {
            0 => self.params.rate = v,
            1 => self.params.depth = v,
            2 => self.params.width = v,
            3 => self.params.tone = v,
            _ => self.params.mix = v,
        }
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let (kernel, params) = (&mut self.kernel, &self.params);
        run_planar(&mut self.scratch, input, frames, out, |buf, n| {
            kernel.process(buf, n, 2, params);
        });
    }
}

// ── Tangent Echo, seated ────────────────────────────────────────────────────

pub struct EchoUnit {
    kernel: crate::units::echo::Echo,
    params: crate::units::echo::EchoParams,
    scratch: Vec<f32>,
}

impl EchoUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            kernel: crate::units::echo::Echo::new(sample_rate),
            params: crate::units::echo::EchoParams::default(),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(spec) = ECHO_FACE_PARAMS.get(id as usize) else {
            return;
        };
        let v = washed(spec, value);
        match id {
            0 => self.params.time = v,
            1 => self.params.feedback = v,
            2 => self.params.tone = v,
            _ => self.params.mix = v,
        }
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let (kernel, params) = (&mut self.kernel, &self.params);
        run_planar(&mut self.scratch, input, frames, out, |buf, n| {
            kernel.process(buf, n, 2, params);
        });
    }
}

// ── Tangent Plate, seated ───────────────────────────────────────────────────

pub struct PlateUnit {
    kernel: crate::units::plate::Plate,
    params: crate::units::plate::PlateParams,
    scratch: Vec<f32>,
}

impl PlateUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            kernel: crate::units::plate::Plate::new(sample_rate),
            params: crate::units::plate::PlateParams::default(),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(spec) = PLATE_FACE_PARAMS.get(id as usize) else {
            return;
        };
        let v = washed(spec, value);
        match id {
            0 => self.params.decay = v,
            1 => self.params.predelay = v,
            2 => self.params.width = v,
            _ => self.params.mix = v,
        }
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let (kernel, params) = (&mut self.kernel, &self.params);
        run_planar(&mut self.scratch, input, frames, out, |buf, n| {
            kernel.process(buf, n, 2, params);
        });
    }
}

// ── Tangent Wah, seated ─────────────────────────────────────────────────────

pub struct WahUnit {
    kernel: crate::units::wah::Wah,
    params: crate::units::wah::WahParams,
    scratch: Vec<f32>,
}

impl WahUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            kernel: crate::units::wah::Wah::new(sample_rate),
            params: crate::units::wah::WahParams::default(),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(spec) = WAH_FACE_PARAMS.get(id as usize) else {
            return;
        };
        let v = washed(spec, value);
        match id {
            0 => self.params.sens = v,
            1 => self.params.base = v,
            2 => self.params.q = v,
            _ => self.params.mix = v,
        }
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let (kernel, params) = (&mut self.kernel, &self.params);
        run_planar(&mut self.scratch, input, frames, out, |buf, n| {
            kernel.process(buf, n, 2, params);
        });
    }
}

// ── Tangent IR, seated ──────────────────────────────────────────────────────

/// The convolver behind its two knobs. What makes it different from every
/// other seat here is that its real state is a FILE: `ir_path` rides the
/// extra-state road, and [`IrUnit::load_ir`] turns the decoded WAV into the
/// kernel's spectra at load time, host-side, before the hand-off.
pub struct IrUnit {
    kernel: crate::units::convolver::Convolver,
    params: crate::units::convolver::ConvolverParams,
    scratch: Vec<f32>,
    /// The engine rate this instance was built at, kept to notice a capture
    /// made at some other rate — see [`IrUnit::load_ir`].
    sample_rate: f32,
    /// Whether an IR ever landed, which is what decides the honest latency:
    /// the empty kernel is a wire and must not bill a partition for it.
    has_ir: bool,
    /// The input as late as the partition, for the bypass.
    dry: DryLine,
}

impl IrUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            kernel: crate::units::convolver::Convolver::new(sample_rate),
            params: crate::units::convolver::ConvolverParams::default(),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
            sample_rate,
            has_ir: false,
            dry: DryLine::new(crate::units::convolver::CONV_PARTITION),
        }
    }

    /// Hand a decoded WAV to the kernel. A mono capture plays as the same
    /// IR on both channels; a stereo capture plays as the pair; anything
    /// wider is refused with a sentence, because guessing which four of six
    /// channels form a true-stereo set is how somebody's Atmos bounce
    /// becomes a mystery reverb.
    ///
    /// A rate mismatch LOADS ANYWAY and says so: the convolution is still
    /// well-defined, just shifted in pitch and time by the ratio, and
    /// hearing something wrong-but-reported beats silence. Resampling is
    /// deliberately not attempted this pass.
    fn load_ir(&mut self, wav: &crate::wavread::WavData) -> Option<String> {
        use crate::units::convolver::IrLayout;
        let irs: Vec<Vec<f32>> = match wav.channels.len() {
            1 => vec![wav.channels[0].clone(), wav.channels[0].clone()],
            2 => vec![wav.channels[0].clone(), wav.channels[1].clone()],
            n => {
                return Some(format!(
                    "its impulse response has {n} channels; one or two are playable"
                ))
            }
        };
        if !self.kernel.set_ir(&irs, IrLayout::Stereo) {
            return Some("its impulse response holds no samples".to_owned());
        }
        self.has_ir = true;
        let engine = self.sample_rate.round() as u32;
        if wav.rate != engine {
            return Some(format!(
                "its impulse response was captured at {} Hz and this session runs at \
                 {engine} Hz; loaded as-is, so it plays shifted",
                wav.rate
            ));
        }
        None
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(spec) = IR_FACE_PARAMS.get(id as usize) else {
            return;
        };
        let v = washed(spec, value);
        match id {
            0 => self.params.mix = v,
            _ => self.params.trim = v,
        }
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let (kernel, params) = (&mut self.kernel, &self.params);
        run_planar(&mut self.scratch, input, frames, out, |buf, n| {
            kernel.process(buf, n, 2, params);
        });
        self.dry.write(input, frames);
    }
}

// ── Tangent Trim ────────────────────────────────────────────────────────────

/// Where the trim's one knob travels, in dB. 0.5 is exactly unity — the knob
/// maps linearly in decibels, so the middle of the travel is 0 dB and the
/// default bay changes nothing.
const TRIM_DB: (f64, f64) = (-24.0, 24.0);

/// Sample rate the slew below is written at, matching `effects.rs`'s
/// `DESIGN_RATE`: constants are designed once at 44.1k and scaled, so the
/// ramp lasts the same fraction of a second whatever the device negotiated.
const DESIGN_RATE: f64 = 44_100.0;

/// How far the gain moves toward its target per sample, at [`DESIGN_RATE`].
/// The same figure as `effects.rs`'s `KNOB_SLEW` and for the same reason:
/// stepping a gain at a block boundary is a click, and a knob drag or an
/// automation curve is a few hundred block boundaries.
const TRIM_SLEW: f32 = 0.0004;

/// A stereo gain: the smallest true effect, here to prove that a built-in can
/// ride an insert bay end to end. Every sample is multiplied by one number;
/// the number glides so automation cannot click.
pub struct Trim {
    /// Linear gain the unit is heading for, from the knob's dB mapping.
    target: f32,
    /// Linear gain now — the slew's state, and the unit's only memory.
    current: f32,
    /// Per-sample step, scaled from [`DESIGN_RATE`] at construction.
    slew: f32,
}

impl Trim {
    fn new(sample_rate: f32) -> Self {
        // The step scales INVERSELY with the rate: more samples per second
        // means less movement per sample, so the glide lasts the same time
        // at 96k as at 44.1k. Designed in f64, per the house rule.
        let slew = (f64::from(TRIM_SLEW) * DESIGN_RATE / f64::from(sample_rate.max(8_000.0)))
            as f32;
        let unity = Self::linear(TRIM_PARAMS[0].default);
        Self {
            target: unity,
            current: unity,
            slew,
        }
    }

    /// The knob's position as a linear factor. At exactly 0.5 the dB figure
    /// is exactly 0.0 and the factor exactly 1.0, which is what makes the
    /// at-rest unit a bit-for-bit null rather than a rounding of one.
    fn linear(gain: f32) -> f32 {
        let x = f64::from(gain.clamp(0.0, 1.0));
        let db = TRIM_DB.0 + (TRIM_DB.1 - TRIM_DB.0) * x;
        10f64.powf(db / 20.0) as f32
    }

    /// Move the knob. A value that is not a number reads as the default —
    /// the same refusal `Params::sane` makes, because a NaN multiplied into
    /// a channel is a channel of NaN forever.
    fn set_gain(&mut self, gain: f32) {
        let v = if gain.is_finite() {
            gain
        } else {
            TRIM_PARAMS[0].default
        };
        self.target = Self::linear(v);
    }

    fn clear(&mut self) {
        self.current = self.target;
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        // Settle when the glide has arrived, so the steady state is one
        // multiply by an EXACT factor — at unity, a bit-exact copy — rather
        // than an asymptote that never quite lands.
        if (self.target - self.current).abs()
            <= f32::EPSILON * self.target.abs().max(1.0)
        {
            self.current = self.target;
        }
        if self.current == self.target {
            let g = self.current;
            for (src, dst) in input.iter().zip(out.iter_mut()) {
                let n = frames.min(src.len()).min(dst.len());
                for (s, d) in src[..n].iter().zip(&mut dst[..n]) {
                    *d = s * g;
                }
            }
            return;
        }
        // Mid-glide: one gain track shared by both channels, advanced once
        // per FRAME — a per-channel slew would leave the right channel a
        // block's glide behind the left.
        for f in 0..frames {
            self.current += (self.target - self.current) * self.slew;
            let g = self.current;
            for (src, dst) in input.iter().zip(out.iter_mut()) {
                if let (Some(s), Some(d)) = (src.get(f), dst.get_mut(f)) {
                    *d = s * g;
                }
            }
        }
    }
}

// ── Tangent Analog, seated ──────────────────────────────────────────────────

/// The most events one block may carry to the synth. A block of 128 frames
/// at a keyboard's fastest carries a few dozen; the ceiling is for a stuck
/// controller sending a value per sample, and past it the extras are
/// dropped rather than allocated for, because this runs on the audio
/// thread.
const ANALOG_EVENTS: usize = 1024;

/// The voice, seated. Translates the host's notes, controls and automation
/// points — VST3's shapes — into the kernel's own events, in time order,
/// and de-interleaves the kernel's stereo into the bay's planar buffers.
pub struct AnalogUnit {
    synth: crate::analog::Analog,
    /// Events gathered with their offset and arrival order, then sorted
    /// unstably on both — the second key is what makes the sort stable
    /// without the allocation a stable sort makes.
    queue: Vec<(usize, u32, crate::analog::Event)>,
    events: Vec<crate::analog::Event>,
    scratch: Vec<f32>,
}

impl AnalogUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            synth: crate::analog::Analog::new(sample_rate),
            queue: Vec::with_capacity(ANALOG_EVENTS),
            events: Vec::with_capacity(ANALOG_EVENTS),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        self.synth.set_param(id, value);
    }

    /// Notes, controls and automation into events; events into the synth;
    /// the synth's stereo into `bufs`. Frames past [`SCRATCH_FRAMES`] are
    /// rendered in passes with the events re-based, so a long offline
    /// block is the same audio as the same block in callback-sized pieces.
    pub fn process_voice(
        &mut self,
        notes: &[tangent_host::Note],
        controls: &[tangent_host::Control],
        params: &[tangent_host::ParamPoint],
        transport: crate::analog::arp::Transport,
        frames: usize,
        bufs: &mut [Vec<f32>],
    ) {
        use crate::analog::Event;
        if frames == 0 || bufs.is_empty() {
            return;
        }
        self.queue.clear();
        let last = frames - 1;
        let mut seq = 0u32;
        let mut push = |queue: &mut Vec<(usize, u32, Event)>, at: i32, e: Event| {
            if queue.len() < ANALOG_EVENTS {
                queue.push(((at.max(0) as usize).min(last), seq, e));
                seq += 1;
            }
        };
        for n in notes {
            let key = n.pitch.clamp(0, 127) as u8;
            let e = if n.on {
                Event::NoteOn { at: 0, key, vel: n.velocity.clamp(0.0, 1.0) }
            } else {
                Event::NoteOff { at: 0, key }
            };
            push(&mut self.queue, n.offset, e);
        }
        for c in controls {
            let e = match c.controller {
                tangent_host::Control::PITCH_BEND => Event::Bend {
                    at: 0,
                    amount: (f32::from(c.value.min(16383)) - 8192.0) / 8192.0,
                },
                tangent_host::Control::AFTERTOUCH => Event::Pressure {
                    at: 0,
                    value: f32::from(c.value.min(127)) / 127.0,
                },
                // The modulation wheel: CC 1, the one controller the
                // 1970 machine's whole modulation section hangs off.
                1 => Event::Wheel { at: 0, value: f32::from(c.value.min(127)) / 127.0 },
                tangent_host::Control::SUSTAIN => Event::Sustain { at: 0, down: c.value >= 64 },
                // All sound off, all notes off: the wire's two panic buttons.
                120 | 123 => Event::AllNotesOff { at: 0 },
                _ => continue,
            };
            push(&mut self.queue, c.offset, e);
        }
        for p in params {
            push(
                &mut self.queue,
                p.offset,
                Event::Param { at: 0, id: p.id, value: p.value as f32 },
            );
        }
        self.queue.sort_unstable_by_key(|e| (e.0, e.1));

        // Passes of at most the scratch's length, events re-based into
        // each and the ones past it held back for the next.
        let mut done = 0usize;
        while done < frames {
            let n = (frames - done).min(SCRATCH_FRAMES);
            self.events.clear();
            for (at, _, e) in &self.queue {
                if *at < done || *at >= done + n {
                    continue;
                }
                let at = *at - done;
                self.events.push(match *e {
                    Event::NoteOn { key, vel, .. } => Event::NoteOn { at, key, vel },
                    Event::NoteOff { key, .. } => Event::NoteOff { at, key },
                    Event::Bend { amount, .. } => Event::Bend { at, amount },
                    Event::Wheel { value, .. } => Event::Wheel { at, value },
                    Event::Pressure { value, .. } => Event::Pressure { at, value },
                    Event::Sustain { down, .. } => Event::Sustain { at, down },
                    Event::Param { id, value, .. } => Event::Param { at, id, value },
                    Event::AllNotesOff { .. } => Event::AllNotesOff { at },
                });
            }
            let mut t = transport;
            if t.playing {
                t.beat += done as f64 * t.bpm / 60.0 / f64::from(self.synth.sample_rate());
            }
            self.synth.render(&mut self.scratch, n, 2, &self.events, t);
            for (ch, buf) in bufs.iter_mut().enumerate() {
                for (i, s) in buf.iter_mut().skip(done).take(n).enumerate() {
                    *s = if ch < 2 { self.scratch[2 * i + ch] } else { 0.0 };
                }
            }
            done += n;
        }
    }
}

/// The most events one block of a sampler's may carry, the analog's figure
/// and its reason: sized once so a wild MIDI file cannot grow a `Vec` on the
/// audio thread.
const SAMPLER_EVENTS: usize = 1024;

/// **The sampler, seated.** Translates the host's notes, controls and
/// automation — VST3's shapes — into the kernel's events in time order, hands
/// the kernel the lane's file for the length of the call, and de-interleaves
/// the result into the bay's planar buffers.
///
/// Deliberately the same shape as [`AnalogUnit`], down to the sort key and
/// the pass loop, because they are the same job: two voices whose kernels
/// speak events and interleaved stereo, sitting behind one planar contract.
/// The one difference is the borrowed sample, and that difference is the
/// whole design — see [`BuiltinUnit::process_voice`].
pub struct SamplerUnit {
    kernel: crate::units::sampler::Sampler,
    /// Events gathered with their offset and arrival order, then sorted
    /// unstably on both — the second key is what makes the sort stable
    /// without the allocation a stable sort makes.
    queue: Vec<(usize, u32, crate::units::sampler::Event)>,
    events: Vec<crate::units::sampler::Event>,
    scratch: Vec<f32>,
}

impl SamplerUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            kernel: crate::units::sampler::Sampler::new(sample_rate),
            queue: Vec::with_capacity(SAMPLER_EVENTS),
            events: Vec::with_capacity(SAMPLER_EVENTS),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(spec) = SAMPLER_FACE_PARAMS.get(id as usize) else {
            return;
        };
        self.kernel.set_param(id, washed(spec, value));
    }

    /// Notes, controls and automation into events; events and the lane's
    /// file into the kernel; the kernel's stereo into `bufs`. Frames past
    /// [`SCRATCH_FRAMES`] are rendered in passes with the events re-based,
    /// so a long offline block is the same audio as the same block in
    /// callback-sized pieces.
    pub fn process_voice(
        &mut self,
        notes: &[tangent_host::Note],
        controls: &[tangent_host::Control],
        params: &[tangent_host::ParamPoint],
        sample: Option<&crate::instrument::SamplerSample>,
        frames: usize,
        bufs: &mut [Vec<f32>],
    ) {
        use crate::units::sampler::Event;
        if frames == 0 || bufs.is_empty() {
            return;
        }
        self.queue.clear();
        let last = frames - 1;
        let mut seq = 0u32;
        let mut push = |queue: &mut Vec<(usize, u32, Event)>, at: i32, e: Event| {
            if queue.len() < SAMPLER_EVENTS {
                queue.push(((at.max(0) as usize).min(last), seq, e));
                seq += 1;
            }
        };
        for n in notes {
            let key = n.pitch.clamp(0, 127) as u8;
            let e = if n.on {
                Event::NoteOn { at: 0, key, vel: n.velocity.clamp(0.0, 1.0) }
            } else {
                Event::NoteOff { at: 0, key }
            };
            push(&mut self.queue, n.offset, e);
        }
        for c in controls {
            let e = match c.controller {
                tangent_host::Control::PITCH_BEND => Event::Bend {
                    at: 0,
                    amount: (f32::from(c.value.min(16383)) - 8192.0) / 8192.0,
                },
                tangent_host::Control::SUSTAIN => Event::Sustain { at: 0, down: c.value >= 64 },
                // All sound off, all notes off: the wire's two panic buttons.
                120 | 123 => Event::AllNotesOff { at: 0 },
                // A sampler has no modulation section to hang a wheel off,
                // and aftertouch with nothing to move is a control that
                // would arrive and do nothing. Dropped by name rather than
                // by a wildcard nobody has to think about.
                _ => continue,
            };
            push(&mut self.queue, c.offset, e);
        }
        for p in params {
            push(
                &mut self.queue,
                p.offset,
                Event::Param { at: 0, id: p.id, value: p.value as f32 },
            );
        }
        self.queue.sort_unstable_by_key(|e| (e.0, e.1));

        // **The lane's file, borrowed for exactly this call.** A clip whose
        // rate is zero — which no decoder produces and a corrupt cache
        // might — is treated as no file at all rather than as a division.
        let borrowed = sample.and_then(|s| {
            (s.clip.rate > 0 && !s.clip.samples.is_empty()).then(|| {
                crate::units::sampler::SampleRef {
                    data: &s.clip.samples,
                    // The decoder's contract: interleaved stereo, always.
                    channels: 2,
                    rate: s.clip.rate as f32,
                    root_key: s.root_key,
                }
            })
        });

        let mut done = 0usize;
        while done < frames {
            let n = (frames - done).min(SCRATCH_FRAMES);
            self.events.clear();
            for (at, _, e) in &self.queue {
                if *at < done || *at >= done + n {
                    continue;
                }
                let at = *at - done;
                self.events.push(match *e {
                    Event::NoteOn { key, vel, .. } => Event::NoteOn { at, key, vel },
                    Event::NoteOff { key, .. } => Event::NoteOff { at, key },
                    Event::Bend { amount, .. } => Event::Bend { at, amount },
                    Event::Sustain { down, .. } => Event::Sustain { at, down },
                    Event::Param { id, value, .. } => Event::Param { at, id, value },
                    Event::AllNotesOff { .. } => Event::AllNotesOff { at },
                });
            }
            self.kernel
                .render(&mut self.scratch, n, 2, &self.events, borrowed);
            for (ch, buf) in bufs.iter_mut().enumerate() {
                for (i, s) in buf.iter_mut().skip(done).take(n).enumerate() {
                    *s = if ch < 2 { self.scratch[2 * i + ch] } else { 0.0 };
                }
            }
            done += n;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_at(gain: f32, rate: f32) -> BuiltinUnit {
        spec_for(TRIM_PATH)
            .expect("the trim is in the table")
            .create(rate, &[gain])
    }

    fn run(unit: &mut BuiltinUnit, input: &[Vec<f32>], frames: usize) -> Vec<Vec<f32>> {
        let mut out = vec![vec![0.0f32; frames]; input.len()];
        unit.process(input, frames, &mut out);
        out
    }

    /// **The analog's registration is the kernel's table, row for row**, the
    /// voice flag is on it and on the other VOICE alone, and every one of its
    /// dials reads and, where stepped, offers its positions. The
    /// transcription is a compile-time loop, so "cannot drift" is true; this
    /// is here for the day somebody rewrites that loop.
    ///
    /// The flag's list was `path == ANALOG_PATH` while the analog was the
    /// only voice, and it is spelled as a LIST now rather than loosened,
    /// because the thing worth catching has not changed: a `voice: true`
    /// that lands on an effect makes the channel play silence, and a
    /// `voice: false` on an instrument makes it never hear a note.
    #[test]
    fn the_analog_registers_as_the_kernels_table_and_one_of_the_two_voices() {
        let spec = spec_for(ANALOG_PATH).expect("the analog is in the table");
        assert!(spec.voice, "the analog is not a voice");
        assert_eq!(spec.class, "Tangent Analog");
        let voices = [ANALOG_PATH, SAMPLER_PATH];
        for u in UNITS {
            assert_eq!(
                u.voice,
                voices.contains(&u.path),
                "{} has the wrong voice flag",
                u.path
            );
        }
        assert_eq!(spec.params.len(), crate::analog::PARAMS.len());
        for (a, b) in spec.params.iter().zip(crate::analog::PARAMS) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.symbol, b.symbol);
            assert_eq!(a.name, b.name);
            assert_eq!(a.units, b.units);
            assert_eq!(a.default, b.default);
            assert_eq!(a.steps, b.steps);
        }
        let values = spec.defaults();
        for p in spec.params {
            let reading = describe(ANALOG_PATH, p.id, p.default, &values);
            assert!(!reading.is_empty(), "{} reads nothing", p.symbol);
            let choices = choices(ANALOG_PATH, p.id);
            if p.steps >= 2 {
                assert_eq!(
                    choices.len(),
                    p.steps as usize,
                    "{} offers {} positions for {} steps",
                    p.symbol,
                    choices.len(),
                    p.steps
                );
                let at = crate::analog::step_index(p.default, p.steps);
                assert_eq!(choices[at], reading, "{}'s reading is not its position's word", p.symbol);
            } else {
                assert!(choices.is_empty(), "{} is continuous but offers positions", p.symbol);
            }
        }
        // A few readings by the kernel's own mappings.
        assert_eq!(describe(ANALOG_PATH, crate::analog::P_O1_RANGE as u32, 0.6, &values), "8'");
        assert_eq!(describe(ANALOG_PATH, crate::analog::P_O1_WAVE as u32, 0.4, &values), "saw");
        assert_eq!(describe(ANALOG_PATH, crate::analog::P_VOICES as u32, 7.0 / 15.0, &values), "8");
        assert_eq!(describe(ANALOG_PATH, crate::analog::P_GLIDE as u32, 0.0, &values), "off");
        assert!(describe(ANALOG_PATH, crate::analog::P_CUTOFF as u32, 0.5, &values).contains("632"));
    }

    /// **The analog's state round-trips by symbol** — sixty-one lines out,
    /// the same sixty-one values back, and a blob from a build that lacked
    /// a dial reads that dial at its default.
    #[test]
    fn the_analogs_state_round_trips_by_symbol() {
        let spec = spec_for(ANALOG_PATH).unwrap();
        let mut values = spec.defaults();
        for (i, v) in values.iter_mut().enumerate() {
            *v = (i as f32 * 0.37) % 1.0;
        }
        let blob = state_text(spec, &values, &[]);
        let back = values_from_state(spec, blob.as_bytes()).expect("parses");
        assert_eq!(back, values);
        let short = "tangentfx 1\ncutoff=0.25\n";
        let back = values_from_state(spec, short.as_bytes()).expect("parses");
        let cut = spec.params.iter().position(|p| p.symbol == "cutoff").unwrap();
        assert_eq!(back[cut], 0.25);
        let vol = spec.params.iter().position(|p| p.symbol == "volume").unwrap();
        assert_eq!(back[vol], spec.params[vol].default);
    }

    /// **A note through the seated voice makes sound, and a pitch-bend
    /// control and an automation point both reach the kernel**: the three
    /// translations `process_voice` does, each observable at the output.
    #[test]
    fn the_seated_analog_plays_notes_and_hears_controls_and_automation() {
        let spec = spec_for(ANALOG_PATH).unwrap();
        let render = |controls: &[tangent_host::Control], params: &[tangent_host::ParamPoint]| {
            let mut unit = spec.create(48_000.0, &spec.defaults());
            let notes = [tangent_host::Note { offset: 0, pitch: 60, velocity: 0.8, on: true }];
            let mut bufs = vec![vec![0.0f32; 512]; 2];
            let mut out = Vec::new();
            for block in 0..40 {
                unit.process_voice(
                    if block == 0 { &notes } else { &[] },
                    if block == 0 { controls } else { &[] },
                    if block == 0 { params } else { &[] },
                    crate::analog::arp::Transport::default(),
                    // The analog plays no file; the sampler's argument is
                    // the only one that ever carries anything here.
                    None,
                    512,
                    &mut bufs,
                );
                out.extend_from_slice(&bufs[0]);
            }
            out
        };
        let plain = render(&[], &[]);
        let peak = plain.iter().fold(0.0f32, |a, v| a.max(v.abs()));
        assert!(peak > 0.005, "no sound: {peak}");
        // A bend a whole step up moves the pitch.
        let bent = render(
            &[tangent_host::Control {
                offset: 0,
                controller: tangent_host::Control::PITCH_BEND,
                value: 16383,
                channel: 0,
            }],
            &[],
        );
        let hz = |x: &[f32], guess: f32| crate::analog::measure::measure_hz(&x[8192..], guess, 48_000.0);
        let (f_plain, f_bent) = (hz(&plain, 261.63), hz(&bent, 293.66));
        assert!((f_plain / 261.63 - 1.0).abs() < 0.01, "plain at {f_plain} Hz");
        assert!((f_bent / 293.66 - 1.0).abs() < 0.01, "bent to {f_bent} Hz, not a whole step");
        // A cutoff point at the floor darkens the note.
        let dark = render(
            &[],
            &[tangent_host::ParamPoint { offset: 0, id: crate::analog::P_CUTOFF as u32, value: 0.0 }],
        );
        let hf = |x: &[f32]| {
            let w = &x[8192..];
            let (mut high, mut total) = (0.0f64, 0.0f64);
            for p in w.windows(2) {
                let d = f64::from(p[1] - p[0]);
                high += d * d;
                total += f64::from(p[1]) * f64::from(p[1]);
            }
            high / total.max(1e-18)
        };
        assert!(hf(&dark) < hf(&plain) * 0.5, "the cutoff point did not darken the note");
    }

    #[test]
    fn no_setting_the_gain_can_reach_makes_the_trim_explode() {
        // Full-scale in, every knob position out to both ends, plus the
        // values a damaged file or a wild curve could deliver. The cap is
        // the mapping's own top: +24 dB of a full-scale signal.
        let input = vec![vec![1.0f32; 256], vec![-1.0f32; 256]];
        let cap = 10f32.powf(24.0 / 20.0) * 1.001;
        for step in 0..=100 {
            let g = step as f32 / 100.0;
            let mut unit = unit_at(g, 48_000.0);
            for _ in 0..8 {
                let out = run(&mut unit, &input, 256);
                for ch in &out {
                    for s in ch {
                        assert!(s.is_finite(), "gain {g} produced {s}");
                        assert!(s.abs() <= cap, "gain {g} produced {s}, over the cap");
                    }
                }
            }
        }
        for wild in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -7.0, 7.0] {
            let mut unit = unit_at(0.5, 48_000.0);
            unit.set_param(0, wild);
            for _ in 0..8 {
                let out = run(&mut unit, &input, 256);
                for ch in &out {
                    for s in ch {
                        assert!(s.is_finite(), "gain {wild} produced {s}");
                        assert!(s.abs() <= cap, "gain {wild} produced {s}, over the cap");
                    }
                }
            }
        }
    }

    #[test]
    fn a_trim_at_rest_passes_audio_bit_for_bit() {
        // The default IS unity, exactly, so an untouched bay must change
        // nothing — down to negative zero and a denormal, because "nearly
        // the same" out of a unit that claims to be off is how nulls die.
        let input = vec![
            vec![0.25f32, -0.5, 1.0, -1.0, 0.0, -0.0, 1.0e-38, 3.1e-4],
            vec![-0.125f32, 0.75, -1.0, 1.0, -0.0, 0.0, -1.0e-38, -3.1e-4],
        ];
        let mut unit = unit_at(TRIM_PARAMS[0].default, 48_000.0);
        let out = run(&mut unit, &input, 8);
        for (src, dst) in input.iter().zip(&out) {
            for (s, d) in src.iter().zip(dst) {
                assert_eq!(s.to_bits(), d.to_bits(), "{s} came out {d}");
            }
        }
    }

    #[test]
    fn a_full_travel_glide_lasts_the_same_time_at_every_sample_rate() {
        // The slew is designed at 44.1k and scaled, so a quarter-second of
        // glide from -24 dB toward +24 dB must land at the same gain
        // whatever the rate. Measured on DC so the last sample IS the gain.
        let landed: Vec<f32> = [44_100.0f32, 48_000.0, 96_000.0]
            .iter()
            .map(|&rate| {
                let mut unit = unit_at(0.0, rate);
                unit.set_param(0, 1.0);
                let frames = (rate * 0.25) as usize;
                let input = vec![vec![1.0f32; frames]; 2];
                let out = run(&mut unit, &input, frames);
                out[0][frames - 1]
            })
            .collect();
        for pair in landed.windows(2) {
            assert!(
                (pair[0] - pair[1]).abs() <= pair[0].abs() * 0.01,
                "the glide landed at {landed:?} across rates"
            );
        }
        // And it is genuinely mid-travel: not still at the bottom, not
        // already at the top — otherwise this test passes on a slew of
        // zero or infinity alike.
        let (lo, hi) = (Trim::linear(0.0), Trim::linear(1.0));
        assert!(landed[0] > lo * 2.0 && landed[0] < hi * 0.999, "{landed:?}");
    }

    #[test]
    fn the_last_automation_point_in_a_block_wins() {
        let mut unit = unit_at(0.5, 48_000.0);
        unit.apply(&[
            tangent_host::ParamPoint { offset: 0, id: 0, value: 0.0 },
            tangent_host::ParamPoint { offset: 64, id: 0, value: 1.0 },
            // A parameter the trim does not have, which must move nothing.
            tangent_host::ParamPoint { offset: 65, id: 9, value: 0.0 },
        ]);
        unit.clear();
        let input = vec![vec![1.0f32; 4]; 2];
        let out = run(&mut unit, &input, 4);
        let want = 10f32.powf(24.0 / 20.0);
        assert!(
            (out[0][0] - want).abs() < 1.0e-3,
            "the block settled at {} rather than the last point's {want}",
            out[0][0]
        );
    }

    #[test]
    fn state_text_round_trips_and_forgives_what_it_should() {
        let spec = spec_for(TRIM_PATH).unwrap();
        let text = state_text(spec, &[0.75], &[]);
        let back = values_from_state(spec, text.as_bytes()).expect("own text reads back");
        assert_eq!(back, vec![0.75]);

        // An older build's blob missing the key reads as the default; a
        // newer build's extra key is skipped; junk values fall back too.
        assert_eq!(
            values_from_state(spec, b"tangentfx 1\n").unwrap(),
            vec![TRIM_PARAMS[0].default]
        );
        assert_eq!(
            values_from_state(spec, b"tangentfx 1\nshine=0.9\ngain=0.25\n").unwrap(),
            vec![0.25]
        );
        assert_eq!(
            values_from_state(spec, b"tangentfx 1\ngain=NaN\n").unwrap(),
            vec![TRIM_PARAMS[0].default]
        );
        // But another plugin's state is refused out loud, not defaulted.
        assert!(values_from_state(spec, b"lv2state 1\ngain=0.25\n").is_err());
        assert!(values_from_state(spec, &[0xFF, 0xFE, 0x00]).is_err());
    }

    /// **The face tables open at the kernels' shipped defaults.** The specs
    /// are literals — a `const` table cannot call `Default::default()` — so
    /// this is the one place the two are held together; drift here is a knob
    /// panel that reads one number while the audio does another, the exact
    /// disease `ports::EffectDefaults` documents.
    #[test]
    fn the_face_tables_open_at_the_kernels_shipped_defaults() {
        let t = crate::units::tape::TapeParams::default();
        let fields = [
            t.drive, t.bump, t.roll, t.wobble, t.hiss, t.trim, t.speed, t.flutter, t.rate, t.wow_range, t.bias, t.age,
            t.damage,
        ];
        assert_eq!(TAPE_FACE_PARAMS.len(), fields.len(), "a tape row has no field here");
        for (spec, want) in TAPE_FACE_PARAMS.iter().zip(fields) {
            assert_eq!(spec.default, want, "{} drifted from TapeParams::default", spec.symbol);
        }
        // **And where a fresh tape opens is the owner's ruling**, which the
        // two tables agreeing cannot hold on its own: Drive 0.5, the
        // high-output tape's datasheet alignment (its 3 % point about 10 dB
        // over 0 VU at 15 ips), and Roll out of circuit, because each speed
        // has its own top. A saved bay keeps the values it was saved with.
        assert_eq!(t.drive, 0.5, "a fresh tape's Drive is not the datasheet alignment");
        assert_eq!(t.roll, 0.0, "a fresh tape's Roll is in circuit");
        let i = crate::units::imager::ImagerParams::default();
        for (spec, want) in IMAGER_FACE_PARAMS.iter().zip([i.width, i.ensemble, i.mono_below]) {
            assert_eq!(spec.default, want, "{} drifted from ImagerParams::default", spec.symbol);
        }
        let c = crate::units::comp::CompParams::default();
        for (spec, want) in COMP_FACE_PARAMS.iter().zip([
            c.threshold, c.ratio, c.attack, c.release, c.makeup, c.mix, c.key_hpf,
        ]) {
            assert_eq!(spec.default, want, "{} drifted from CompParams::default", spec.symbol);
        }
        // The EQ: kernel defaults for everything except the parking spots,
        // which are the documented ruler — n handles evenly spaced inside
        // the sweep at k/(n+1), so five of them sit on the sixths. Written
        // as the arithmetic and not as five decimals, so that adding a sixth
        // band makes this fail in the one place that says what to do about
        // it. See EQ_FACE_PARAMS.
        let n = crate::units::eq::NodeParams::default();
        assert!(!n.on, "the kernel ships FLAT; the face must too");
        for (band, chunk) in EQ_FACE_PARAMS.chunks(EQ_KNOBS_PER_NODE as usize).enumerate() {
            assert_eq!(chunk[0].default, 0.0, "band {band} does not open off");
            assert_eq!(chunk[1].default, 0.0, "band {band} does not open a bell");
            let rungs = EQ_FACE_PARAMS.len() / EQ_KNOBS_PER_NODE as usize;
            assert_eq!(
                chunk[2].default,
                (band + 1) as f32 / (rungs + 1) as f32,
                "band {band} is not on the {rungs}-handle ruler"
            );
            assert_eq!(chunk[3].default, n.gain);
            assert_eq!(chunk[4].default, n.q);
        }
        // The shape menu steps across exactly the kernel's enum, ends
        // inclusive — a literal in the table, pinned to its source here.
        assert_eq!(
            EQ_KIND_STEPS as usize,
            crate::units::eq::NodeKind::ALL.len(),
            "the kind knob's step count drifted from NodeKind::ALL"
        );
        // And the wave-4 nine, same disease, same cure.
        let d = crate::units::denoise::DenoiseParams::default();
        for (spec, want) in DENOISE_FACE_PARAMS.iter().zip([
            d.learn, d.reduce, d.smooth, d.release, d.sc_hpf, d.lookahead, d.kernel, d.attack,
            d.treble,
        ]) {
            assert_eq!(spec.default, want, "{} drifted from DenoiseParams::default", spec.symbol);
        }
        // The kernel menu steps across exactly the kernel's enum, ends
        // inclusive, and its default position is the one the enum calls
        // Bank (the owner, 2026-10-11): a literal in the table, pinned to
        // its source.
        use crate::units::denoise::DenoiseKernel;
        assert_eq!(
            DENOISE_KERNEL_STEPS as usize,
            DenoiseKernel::ALL.len(),
            "the kernel knob's step count drifted from DenoiseKernel::ALL"
        );
        assert_eq!(
            DenoiseKernel::from_dial(d.kernel),
            DenoiseKernel::Bank,
            "the shipped denoise no longer opens on the Bank"
        );
        // The amp's table is nine knobs to the kernel's eight: VIB is a
        // panel SWITCH the wrapper folds into vib_depth (off = depth 0),
        // so the switch and the intensity both rest at zero by design and
        // the remaining rows pin straight to the kernel's defaults.
        let a = crate::units::amp::AmpParams::default();
        for (spec, want) in AMP_FACE_PARAMS.iter().zip([
            a.bright, a.volume, a.treble, a.middle, a.bass, 0.0, a.vib_speed, 0.0,
            a.vib_stereo,
        ]) {
            assert_eq!(spec.default, want, "{} drifted from AmpParams::default", spec.symbol);
        }
        let dr = crate::units::drive::DriveParams::default();
        for (spec, want) in DRIVE_FACE_PARAMS.iter().zip([dr.drive, dr.tone, dr.level, dr.symmetry]) {
            assert_eq!(spec.default, want, "{} drifted from DriveParams::default", spec.symbol);
        }
        let tr = crate::units::trem::TremParams::default();
        for (spec, want) in TREM_FACE_PARAMS.iter().zip([tr.speed, tr.depth, tr.stereo]) {
            assert_eq!(spec.default, want, "{} drifted from TremParams::default", spec.symbol);
        }
        let ch = crate::units::chorus::ChorusParams::default();
        for (spec, want) in CHORUS_FACE_PARAMS.iter().zip([ch.rate, ch.depth, ch.width, ch.tone, ch.mix]) {
            assert_eq!(spec.default, want, "{} drifted from ChorusParams::default", spec.symbol);
        }
        let ec = crate::units::echo::EchoParams::default();
        for (spec, want) in ECHO_FACE_PARAMS.iter().zip([ec.time, ec.feedback, ec.tone, ec.mix]) {
            assert_eq!(spec.default, want, "{} drifted from EchoParams::default", spec.symbol);
        }
        let pl = crate::units::plate::PlateParams::default();
        for (spec, want) in PLATE_FACE_PARAMS.iter().zip([pl.decay, pl.predelay, pl.width, pl.mix]) {
            assert_eq!(spec.default, want, "{} drifted from PlateParams::default", spec.symbol);
        }
        let w = crate::units::wah::WahParams::default();
        for (spec, want) in WAH_FACE_PARAMS.iter().zip([w.sens, w.base, w.q, w.mix]) {
            assert_eq!(spec.default, want, "{} drifted from WahParams::default", spec.symbol);
        }
        let ir = crate::units::convolver::ConvolverParams::default();
        for (spec, want) in IR_FACE_PARAMS.iter().zip([ir.mix, ir.trim]) {
            assert_eq!(spec.default, want, "{} drifted from ConvolverParams::default", spec.symbol);
        }
        let bc = crate::units::bitcrush::BitcrushParams::default();
        for (spec, want) in BITCRUSH_FACE_PARAMS
            .iter()
            .zip([bc.bits, bc.rate, bc.drive, bc.mix, bc.output, bc.dither, bc.aa])
        {
            assert_eq!(spec.default, want, "{} drifted from BitcrushParams::default", spec.symbol);
        }
    }

    /// A planar in/out pair with awkward values in it — negative zero and a
    /// denormal included, because "nearly the same" out of a unit that
    /// claims to be off is how nulls die.
    fn tricky_planar(frames: usize) -> Vec<Vec<f32>> {
        let vals = [0.25f32, -0.5, 1.0, -1.0, 0.0, -0.0, 1.0e-38, 3.1e-4];
        vec![
            (0..frames).map(|i| vals[i % vals.len()]).collect(),
            (0..frames).map(|i| -vals[(i + 3) % vals.len()]).collect(),
        ]
    }

    /// **An untouched EQ and imager, and a comp with its threshold parked at
    /// the top, are nulls through the planar road** — interleave, kernel,
    /// deinterleave and all. Bit-for-bit for the two whose off-path never
    /// touches the buffer; value-exact for the comp, whose unity gain is a
    /// multiply (its own kernel tests make the same claim the same way).
    #[test]
    fn untouched_eq_imager_and_a_neutral_comp_are_nulls_through_the_planar_road() {
        let frames = 2_048;
        let input = tricky_planar(frames);

        for path in [EQ_PATH, IMAGER_PATH] {
            let spec = spec_for(path).unwrap();
            let mut unit = spec.create(48_000.0, &spec.defaults());
            let mut out = vec![vec![7.0f32; frames]; 2];
            unit.process(&input, frames, &mut out);
            for (src, dst) in input.iter().zip(&out) {
                for (s, d) in src.iter().zip(dst) {
                    assert_eq!(s.to_bits(), d.to_bits(), "{path}: {s} came out {d}");
                }
            }
        }

        // The comp's NEUTRAL is not its default — the shipped default holds
        // a signal, which is the point of shipping it — it is the threshold
        // at the top AND the ratio at 1 : 1. Both, because the threshold's
        // top is 0 dBFS and this signal's near-square shape reads +3 dB on
        // the detector's sine calibration: over the top threshold, caught,
        // unless the ratio is a wire too.
        let spec = spec_for(COMP_PATH).unwrap();
        let mut neutral = spec.defaults();
        neutral[0] = 1.0;
        neutral[1] = 0.0;
        let mut unit = spec.create(48_000.0, &neutral);
        let mut out = vec![vec![7.0f32; frames]; 2];
        unit.process(&input, frames, &mut out);
        for (src, dst) in input.iter().zip(&out) {
            assert_eq!(src, dst, "a comp with nothing to catch changed the signal");
        }
    }

    /// **Tape at its neutral-null settings is level-transparent through the
    /// planar road** — the kernel's own tenth-of-a-decibel claim, restated
    /// through the adapter. Not bit-exact and never will be: the play head
    /// is a real delay. The 1.2-second call is also more than two full
    /// scratches of frames, so this is the chunking path proving itself on
    /// real audio.
    #[test]
    fn tape_at_neutral_is_level_transparent_through_the_planar_road() {
        let sr = 44_100.0f32;
        let frames = (sr * 1.2) as usize;
        let mono: Vec<f32> = (0..frames)
            .map(|i| {
                let t = i as f32 / sr;
                0.07 * (std::f32::consts::TAU * 400.0 * t).sin()
                    + 0.07 * (std::f32::consts::TAU * 1_000.0 * t).sin()
                    + 0.07 * (std::f32::consts::TAU * 3_300.0 * t).sin()
            })
            .collect();
        let input = vec![mono.clone(), mono];
        let spec = spec_for(TAPE_PATH).unwrap();
        let rms = |v: &[f32]| {
            (v.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / v.len() as f64).sqrt()
        };
        // Steady state only: past the envelope settling and the line fill.
        let (lo, hi) = ((sr * 0.25) as usize, (sr * 1.15) as usize);
        // The six values are the 5.0 controls; `create` leaves every row it
        // is not handed at the spec's default, so the speed is the studio
        // 15 ips a fresh bay opens at.
        let at = |drive: f32| {
            let mut unit = spec.create(sr, &[drive, 0.0, 0.0, 0.0, 0.0, 0.5]);
            let mut out = vec![vec![0.0f32; frames]; 2];
            unit.process(&input, frames, &mut out);
            20.0 * (rms(&out[0][lo..hi]) / rms(&input[0][lo..hi])).log10()
        };

        // Drive at the bottom, trim 0.5 unity, every colour off — the
        // kernel's own neutral recipe. The drive used to read 0.5 here and
        // was only neutral by accident; it is a record level, and a record
        // level's off is the end of its travel (`units::tape::DRIVE_TANH`).
        let off = at(0.0);
        assert!(
            off.abs() < 0.1,
            "neutral tape moved the broadband level by {off:.3} dB through the adapter"
        );
        // **And at the drive every new bay opens at.** Moving this guard to
        // the bottom of the travel was right and it also left the SHIPPED
        // position unwatched, which is how a record level that gave away
        // 11.8 dB at the top of its travel reached the seat. See
        // `units::tape::sat_makeup`.
        let shipped = at(spec.defaults()[0]);
        assert!(
            shipped.abs() < 0.4,
            "the shipped tape moved the broadband level by {shipped:.3} dB \
             through the adapter"
        );
    }

    /// **The tape reports its head to delay compensation before its first
    /// block.**
    ///
    /// The play head holds the tape `units::tape::BASE_MS` behind the
    /// record head, and the seat used to tell the compensation graph zero,
    /// so every Tape lane played two milliseconds late against the rest of
    /// the mix. Now it reports the kernel's own figure, and the figure has to
    /// be there the moment the bay is seated: `UnitSpec::create` sets the
    /// parameters and `instrument`'s `prepare_builtin` reads `latency()` on
    /// the next line, and a bay restored bypassed never runs the block that
    /// could correct it: the denoiser's fault, pinned by
    /// `a_denoise_seated_from_a_blob_reports_its_latency_before_the_first_block`.
    ///
    /// So it is read through the real load path, `state_text`,
    /// `values_from_state`, `create`, with no block processed: two
    /// milliseconds at 44.1, 48 and 96 kHz, which is 88, 96 and 192 frames,
    /// and on the deep head 25, which is 1103, 1200 and 2400. And only Wow
    /// Range moves it, the owner's ruling: set on a live unit it moves the
    /// report at once, both ways, and every other id driven to both ends of
    /// its travel leaves the figure where it was, because a report that
    /// moved with a character dial would re-time the lane every time
    /// somebody turned it.
    #[test]
    fn the_tape_reports_its_head_to_delay_compensation_before_its_first_block() {
        let spec = spec_for(TAPE_PATH).unwrap();
        for (sr, studio, deep) in [(44_100.0_f32, 88_u32, 1_103_u32), (48_000.0, 96, 1_200), (96_000.0, 192, 2_400)] {
            for (range, want) in [(0.0_f32, studio), (1.0, deep)] {
                let mut values = spec.defaults();
                values[9] = range;
                let text = state_text(spec, &values, &[]);
                let back = values_from_state(spec, text.as_bytes()).expect("own text reads back");
                let unit = spec.create(sr, &back);
                assert_eq!(
                    unit.latency(),
                    want,
                    "a tape seated at {sr} Hz on Wow Range {range} reported {} frames before its \
                     first block",
                    unit.latency()
                );
            }
            let mut unit = spec.create(sr, &spec.defaults());
            for p in spec.params.iter().filter(|p| p.id != 9) {
                for end in [0.0_f32, 1.0] {
                    unit.set_param(p.id, end);
                    assert_eq!(
                        unit.latency(),
                        studio,
                        "{} at {end} moved the tape's report at {sr} Hz",
                        p.symbol
                    );
                }
            }
            unit.set_param(9, 1.0);
            assert_eq!(unit.latency(), deep, "the deep range did not move the report at {sr} Hz");
            unit.set_param(9, 0.0);
            assert_eq!(unit.latency(), studio, "the studio range did not move it back at {sr} Hz");
        }
    }

    /// **The tape changes heads without the allocator**, in circuit and out.
    /// A wow range switch starts the deep head's cassette from nothing,
    /// fades the heads, and moves the bypassed dry line between its two
    /// taps, all on the audio thread, so every one of those roads is walked
    /// inside the counting window (full Wow, so the cassette runs): into the
    /// deep head and out of it in circuit, each fade to its end so the
    /// cassette starts and stops; switched mid-block, bypassed mid-fade,
    /// switched back while bypassed, and back in circuit.
    #[test]
    fn the_tape_switches_heads_without_the_allocator() {
        use crate::rt_alloc::{least_of, Counted};
        let spec = spec_for(TAPE_PATH).unwrap();
        let mut values = spec.defaults();
        values[3] = 1.0;
        let mut unit = spec.create(48_000.0, &values);
        let frames = 1_500;
        let input: Vec<Vec<f32>> = (0..2)
            .map(|c| (0..frames).map(|i| ((i + c) as f32 * 0.01).sin() * 0.5).collect())
            .collect();
        let mut out = vec![vec![0.0f32; frames]; 2];
        unit.process(&input, frames, &mut out);
        let ((), counted) = least_of(3, || {
            // Into the deep head and out again in circuit, each fade run to
            // its end, so the cassette starts and stops.
            unit.set_param(9, 1.0);
            unit.process(&input, frames, &mut out);
            unit.set_param(9, 0.0);
            unit.process(&input, frames, &mut out);
            // Switched mid-block, then bypassed mid-fade, switched back while
            // bypassed, and back in circuit.
            unit.set_param(9, 1.0);
            unit.process(&input, 256, &mut out);
            unit.bypassed(&input, frames, &mut out);
            unit.set_param(9, 0.0);
            unit.bypassed(&input, 256, &mut out);
            unit.bypassed(&input, frames, &mut out);
            unit.process(&input, frames, &mut out);
        });
        assert_eq!(counted, Counted::default(), "the tape went to the allocator to change heads");
    }

    /// **A tape saved by 5.0 reads the new controls at their defaults.**
    ///
    /// The owner's ruling: a 5.0 or 5.1 session is not pinned to how it
    /// sounded. Its blob carries the six symbols 5.0 had, and every control
    /// appended since reads at its spec default, the studio speed, a
    /// specified transport's Flutter, the parts' own Rate and the studio
    /// head among them, with no `absent_reads_as` entry to say otherwise; so
    /// the bay opens on the studio head's two milliseconds. The literal blob
    /// is the one a fresh 5.0 Tape saved.
    #[test]
    fn a_5_0_tape_blob_reads_the_new_controls_at_their_defaults() {
        let spec = spec_for(TAPE_PATH).unwrap();
        let blob = b"tangentfx 1\ndrive=0.6\nbump=0.5\nroll=0.12\nwobble=0.3\nhiss=0.25\ntrim=0.5\n";
        let values = values_from_state(spec, blob).expect("a 5.0 blob reads");
        assert_eq!(&values[..6], &[0.6, 0.5, 0.12, 0.3, 0.25, 0.5], "the six 5.0 values moved");
        assert_eq!(&values[6..13], &[2.0 / 3.0, 0.3, 0.5, 0.0, 0.5, 0.0, 0.0], "an appended control read off its default");
        for p in &spec.params[6..] {
            assert_eq!(values[p.id as usize], p.default, "{} read off its default", p.symbol);
            assert!(absent_reads_as(TAPE_PATH, p.symbol).is_none(), "{} is pinned to 5.0", p.symbol);
        }
        assert_eq!(spec.create(48_000.0, &values).latency(), 96, "a 5.0 tape opened off the studio head");
    }

    /// **Every tape id reaches its own field, and an id the table does not
    /// name reaches none.**
    ///
    /// The seat's sweep (`every_imager_and_tape_control_moves_the_audio_through_the_seat`)
    /// asks whether each id moves the sound, and an id wired to the wrong
    /// field passes it whenever that field moves the sound too, which the
    /// trim always does. `TapeUnit::set_param` used to end in a catch-all
    /// into the trim, so the first row appended to the table without an arm
    /// of its own would have driven the output level and nothing would have
    /// said so. So each id is set to a value of its own and the kernel's
    /// params are read back field by field: its field takes the value and no
    /// other field moves. The field list must be exactly as long as the
    /// table or this fails first, so a row appended to one is a row added to
    /// the other. Ids past the table move nothing, and a NaN lands on the
    /// row's default rather than on what was there.
    #[test]
    fn every_tape_id_reaches_its_own_field() {
        let spec = spec_for(TAPE_PATH).unwrap();
        let BuiltinUnit::Tape(mut u) = spec.create(48_000.0, &spec.defaults()) else {
            panic!("the tape path built something that is not a tape");
        };
        let fields = |p: &crate::units::tape::TapeParams| {
            [
                p.drive, p.bump, p.roll, p.wobble, p.hiss, p.trim, p.speed, p.flutter, p.rate, p.wow_range, p.bias, p.age,
                p.damage,
            ]
        };
        assert_eq!(
            fields(&u.params).len(),
            TAPE_FACE_PARAMS.len(),
            "a tape row has no field in this list, or a field has no row"
        );
        for (k, row) in TAPE_FACE_PARAMS.iter().enumerate() {
            assert_eq!(row.id as usize, k, "the tape's ids are not its rows");
            let before = fields(&u.params);
            let v = (k as f32 + 1.0) / 16.0;
            u.set_param(row.id, v);
            for (j, (was, now)) in before.iter().zip(fields(&u.params)).enumerate() {
                if j == k {
                    assert_eq!(now, v, "id {k} ({}) did not reach its own field", row.symbol);
                } else {
                    assert_eq!(
                        now, *was,
                        "id {k} ({}) moved {}",
                        row.symbol, TAPE_FACE_PARAMS[j].symbol
                    );
                }
            }
        }
        let before = u.params;
        for id in [TAPE_FACE_PARAMS.len() as u32, 13, 99, u32::MAX] {
            u.set_param(id, 0.875);
        }
        assert_eq!(u.params, before, "an id past the tape's table moved a knob");
        for row in TAPE_FACE_PARAMS {
            u.set_param(row.id, f32::NAN);
            assert_eq!(
                fields(&u.params)[row.id as usize],
                row.default,
                "NaN on {} did not read as its default",
                row.symbol
            );
        }
    }

    /// **A tape bay's values read as the params its unit holds.** The host
    /// works out the Tape panel's notes from a bay's values beside the
    /// audio (`desktop::tape_notes`), and the unit on the audio thread hears
    /// the same values through `set_param`. One road from values to fields,
    /// `tape_slot` and `washed`, so the panel's arithmetic and the sound
    /// cannot be told different numbers: for distinct values on every id
    /// the two hold the same params; a slice too short for the table reads
    /// its absent ids at their defaults on both sides, as a bay saved before
    /// those rows existed opens; and a NaN lands on its row's default on
    /// both.
    #[test]
    fn a_tape_bays_values_read_as_the_params_its_unit_holds() {
        let spec = spec_for(TAPE_PATH).unwrap();
        let held = |values: &[f32]| match spec.create(48_000.0, values) {
            BuiltinUnit::Tape(u) => u.params,
            _ => panic!("the tape path built something that is not a tape"),
        };
        let distinct: Vec<f32> = (0..TAPE_FACE_PARAMS.len()).map(|k| (k as f32 + 1.0) / 16.0).collect();
        let mut nan = spec.defaults();
        nan[4] = f32::NAN;
        for (what, values) in [
            ("distinct values", distinct.clone()),
            ("three values", distinct[..3].to_vec()),
            ("a NaN on the hiss", nan),
        ] {
            assert_eq!(
                tape_params_of(&values),
                held(&values),
                "{what}: the panel's arithmetic reads other params than the unit holds"
            );
        }
        assert_ne!(
            tape_params_of(&distinct),
            crate::units::tape::TapeParams::default(),
            "the distinct values read as a fresh tape, so the comparison above proves nothing"
        );
    }

    /// Rising zero crossings of `v`, each placed between its two samples by
    /// linear interpolation, past the first `skip` frames.
    fn rising_crossings(v: &[f32], skip: usize) -> Vec<f64> {
        (skip.max(1)..v.len())
            .filter(|&i| v[i - 1] < 0.0 && v[i] >= 0.0)
            .map(|i| (i - 1) as f64 + f64::from(v[i - 1]) / f64::from(v[i - 1] - v[i]))
            .collect()
    }

    /// **A bypassed tape keeps its delay, and switching it out and back in
    /// does not click.**
    ///
    /// The tape's report stays in the compensation graph while its bay is
    /// bypassed, as every bypass in the rack keeps its bay's figure, so a
    /// bypass that let the input past untouched played the lane two
    /// milliseconds early: every A/B against the tape was a comparison of
    /// two timings as well as two sounds. The seat now holds the dry signal
    /// exactly as late as the tape reports and fades between the two.
    ///
    /// A 440 Hz sine at -12 dBFS through every colour off at 48 kHz, in
    /// 512-frame blocks through `BuiltinUnit::process` and
    /// `BuiltinUnit::bypassed`. Bypassed from creation it is the input
    /// delayed by the 96 frames reported, to a millionth, and the kernel
    /// never runs. Switched out at block 10 and back at block 40, against
    /// the same sine left in circuit: no sample-to-sample step is more than
    /// half as big again as the sine's own largest, so neither fade clicks;
    /// every zero crossing stays within a sample of the in-circuit run's, so
    /// the bypass never moved the lane; the kernel does not run once its
    /// fade out is done; and 40 ms after the switch back the output is the
    /// in-circuit run's within a thousandth, so the tape it went back to
    /// plays as if it had never left. Measured: the worst step 1.02 times
    /// the sine's own, the crossings 0.86 of a sample, the kernel idle from
    /// block 11 to 39, and 4.5e-5 at 40 ms (the input transformer and the
    /// head's roll-off settling from the clean start).
    ///
    /// The crossings' bar is the tape's own phase and not its timing: its
    /// low-end high-passes lead a pure delay by most of a sample at 440 Hz,
    /// while a bypass that dropped the tape's 96 frames would move a
    /// crossing by 13. 440 Hz and not 1 kHz, because a 1 kHz period at
    /// 48 kHz is 48 frames, which 96 is two of, and a lane that slipped by
    /// exactly the report would cross zero where it always had.
    ///
    /// **And on the deep head.** Opened bypassed on it, the bay is the input
    /// 1200 frames late, to a millionth, and the kernel never runs. Switched
    /// to it while bypassed, the report moves to 1200 on that very block,
    /// and the dry follows over the heads' own 25 ms equal-power fade with
    /// no step past the same bar, after which it is the input 1200 frames
    /// late again, to a millionth. Measured: the worst step through that
    /// fade 1.30 times the sine's own, the equal-power fade of two copies of
    /// one sine swelling it.
    #[test]
    fn a_bypassed_tape_keeps_its_delay_and_switches_without_a_click() {
        const BLOCK: usize = 512;
        const BLOCKS: usize = 64;
        let sr = 48_000.0_f32;
        let spec = spec_for(TAPE_PATH).unwrap();
        // The 5.0 six with every colour off, the studio speed, and Flutter
        // at 0 too, so the transport is still.
        let neutral = [0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 2.0 / 3.0, 0.0];
        let amp = 10.0_f64.powf(-12.0 / 20.0);
        let sine: Vec<f32> = (0..BLOCK * BLOCKS)
            .map(|i| (amp * (std::f64::consts::TAU * 440.0 * i as f64 / f64::from(sr)).sin()) as f32)
            .collect();
        let largest = sine.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0_f32, f32::max);
        // The sine through a fresh unit, bypassed on the blocks `out_of`
        // names: the left channel, how many blocks the kernel had run by the
        // end of each block, and the latency reported.
        let run = |out_of: &dyn Fn(usize) -> bool| {
            let mut unit = spec.create(sr, &neutral);
            let mut heard = Vec::with_capacity(sine.len());
            let mut ran = Vec::with_capacity(BLOCKS);
            let mut out = vec![vec![0.0_f32; BLOCK]; 2];
            for k in 0..BLOCKS {
                let block = sine[k * BLOCK..(k + 1) * BLOCK].to_vec();
                let input = vec![block.clone(), block];
                if out_of(k) {
                    unit.bypassed(&input, BLOCK, &mut out);
                } else {
                    unit.process(&input, BLOCK, &mut out);
                }
                heard.extend_from_slice(&out[0]);
                let BuiltinUnit::Tape(t) = &unit else {
                    panic!("the tape path built something that is not a tape");
                };
                ran.push(t.kernel_blocks);
            }
            (heard, ran, unit.latency() as usize)
        };

        let (dry, ran, late) = run(&|_| true);
        assert_eq!(late, 96, "the tape reports {late} frames at 48 kHz");
        for (i, v) in dry.iter().enumerate() {
            let want = if i < late { 0.0 } else { sine[i - late] };
            assert!(
                (v - want).abs() < 1.0e-6,
                "frame {i} of a bay bypassed since creation read {v}, and the input \
                 {late} frames back is {want}"
            );
        }
        assert_eq!(ran[BLOCKS - 1], 0, "a bay bypassed since creation ran its kernel");

        let (live, _, _) = run(&|_| false);
        let (flip, ran, _) = run(&|k| (10..40).contains(&k));
        for (i, w) in flip.windows(2).enumerate() {
            let step = (w[1] - w[0]).abs();
            assert!(
                step <= 1.5 * largest,
                "frame {i}: a step of {step:.4} against the sine's own {largest:.4}, so \
                 the switch clicked"
            );
        }
        let (was, now) = (rising_crossings(&live, 200), rising_crossings(&flip, 200));
        assert_eq!(was.len(), now.len(), "the switched run crossed zero a different number of times");
        let moved = was.iter().zip(&now).map(|(a, b)| (a - b).abs()).fold(0.0_f64, f64::max);
        assert!(
            moved < 1.0,
            "a zero crossing moved {moved:.3} of a sample, so the bypass moved the lane"
        );
        assert_eq!(
            ran[10], ran[39],
            "the kernel ran while the bay was fully bypassed ({} blocks, then {})",
            ran[10], ran[39]
        );
        assert!(ran[40] > ran[39], "the kernel did not start again when the bay came back");
        let back = 40 * BLOCK + (0.040 * sr) as usize;
        let off = live[back..]
            .iter()
            .zip(&flip[back..])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            off < 1.0e-3,
            "40 ms after the switch back the tape is {off:.2e} from the one that never left"
        );

        // The deep head, bypassed: opened on it, and switched to it at
        // block 20.
        let mut on_deep = neutral.to_vec();
        on_deep.extend([0.5, 1.0]);
        let bypassed_run = |values: &[f32], switch: Option<usize>| {
            let mut unit = spec.create(sr, values);
            let (mut heard, mut late) = (Vec::with_capacity(sine.len()), Vec::with_capacity(BLOCKS));
            let mut out = vec![vec![0.0_f32; BLOCK]; 2];
            for k in 0..BLOCKS {
                if Some(k) == switch {
                    unit.set_param(9, 1.0);
                }
                let block = sine[k * BLOCK..(k + 1) * BLOCK].to_vec();
                unit.bypassed(&[block.clone(), block], BLOCK, &mut out);
                heard.extend_from_slice(&out[0]);
                late.push(unit.latency() as usize);
            }
            let BuiltinUnit::Tape(t) = &unit else {
                panic!("the tape path built something that is not a tape");
            };
            (heard, late, t.kernel_blocks)
        };
        let delayed = |i: usize, by: usize| if i < by { 0.0 } else { sine[i - by] };
        let (deep, late, ran) = bypassed_run(&on_deep, None);
        assert_eq!(late[0], 1_200, "a deep tape reports {} frames at 48 kHz", late[0]);
        for (i, v) in deep.iter().enumerate() {
            assert!(
                (v - delayed(i, 1_200)).abs() < 1.0e-6,
                "frame {i} of a deep bay bypassed since creation read {v}, not the input 1200 back"
            );
        }
        assert_eq!(ran, 0, "a deep bay bypassed since creation ran its kernel");
        let (switched, late, _) = bypassed_run(&neutral, Some(20));
        assert_eq!((late[19], late[20]), (96, 1_200), "the report did not move with the range at once");
        for (i, w) in switched.windows(2).enumerate() {
            let step = (w[1] - w[0]).abs();
            assert!(
                step <= 1.5 * largest,
                "frame {i}: a step of {step:.4} against the sine's own {largest:.4}, so the range \
                 switch clicked"
            );
        }
        let settled = 20 * BLOCK + (0.025 * sr) as usize + 1;
        for (i, v) in switched.iter().enumerate() {
            let want = if i < 20 * BLOCK {
                delayed(i, 96)
            } else if i >= settled {
                delayed(i, 1_200)
            } else {
                continue;
            };
            assert!(
                (v - want).abs() < 1.0e-6,
                "frame {i} of the bay switched to the deep head while bypassed read {v}, not {want}"
            );
        }
    }

    /// **A bypassed denoise or convolver keeps the delay it reports.**
    ///
    /// Both bill the compensation graph for a delay (the denoise's
    /// lookahead, the convolver's partition once an IR is in), and a bypassed
    /// bay's figure stays in that graph, so a bypass that let the input past
    /// untouched played the lane that many frames early. Each is held here
    /// through the seat: it opts in to the rack's latency-keeping arm; an
    /// impulse sent through the bypass leaves exactly the reported number of
    /// frames late; the frames that open a bypass are the input the unit
    /// heard in circuit just before (so the history is written on the
    /// in-circuit road too); and a unit reporting nothing (the lookahead
    /// down, no IR loaded) passes its input bit for bit, as before.
    #[test]
    fn a_bypassed_denoise_or_convolver_keeps_the_delay_it_reports() {
        const FRAMES: usize = 2_048;
        let impulse_at = 10;
        let check = |unit: &mut BuiltinUnit, what: &str| {
            assert!(unit.keeps_its_delay_when_bypassed(), "{what} does not keep its delay when bypassed");
            let late = unit.latency() as usize;
            assert!(late > 0 && late + impulse_at < FRAMES, "{what} reports {late} frames");
            // In circuit first, on a ramp, so the bypass opens on history.
            let ramp: Vec<Vec<f32>> = (0..2).map(|_| (0..FRAMES).map(|i| i as f32 / FRAMES as f32).collect()).collect();
            let mut out = vec![vec![0.0f32; FRAMES]; 2];
            unit.process(&ramp, FRAMES, &mut out);
            let mut imp = vec![vec![0.0f32; FRAMES]; 2];
            for c in &mut imp {
                c[impulse_at] = 1.0;
            }
            unit.bypassed(&imp, FRAMES, &mut out);
            for (ch, o) in out.iter().enumerate() {
                for (i, v) in o.iter().enumerate().take(late) {
                    let want = ramp[ch][FRAMES - late + i];
                    assert!((v - want).abs() < 1.0e-7, "{what} ch {ch} frame {i}: {v}, wanted the ramp's {want}");
                }
                let peak = o.iter().enumerate().skip(late).fold((0, 0.0f32), |a, (i, v)| if v.abs() > a.1 { (i, v.abs()) } else { a });
                assert_eq!(peak, (impulse_at + late, 1.0), "{what} ch {ch}: the impulse left at {peak:?}, {late} frames late wanted");
            }
        };
        let untouched = |unit: &mut BuiltinUnit, what: &str| {
            assert_eq!(unit.latency(), 0, "{what} reports a delay");
            let input: Vec<Vec<f32>> = (0..2).map(|c| (0..FRAMES).map(|i| ((i + c) as f32 * 0.37).sin()).collect()).collect();
            let mut out = vec![vec![0.0f32; FRAMES]; 2];
            unit.bypassed(&input, FRAMES, &mut out);
            assert_eq!(out, input, "{what} reporting nothing did not pass its input untouched");
        };

        let spec = spec_for(DENOISE_PATH).unwrap();
        let mut values = spec.defaults();
        values[5] = 1.0;
        values[6] = crate::units::denoise::DenoiseKernel::Bank.dial();
        check(&mut spec.create(48_000.0, &values), "the denoise with its lookahead up");
        values[5] = 0.0;
        untouched(&mut spec.create(48_000.0, &values), "the denoise with its lookahead down");

        let spec = spec_for(IR_PATH).unwrap();
        let mut unit = spec.create(48_000.0, &spec.defaults());
        untouched(&mut unit, "the convolver with no IR");
        let BuiltinUnit::Ir(u) = &mut unit else { panic!("the IR path seats a convolver") };
        let mut ir = vec![0.0f32; 300];
        ir[0] = 1.0;
        assert_eq!(u.load_ir(&crate::wavread::WavData { rate: 48_000, channels: vec![ir] }), None);
        check(&mut unit, "the convolver with an IR in");
    }

    /// **A bypassed tape keeps its lane in time, through the real rack.**
    ///
    /// The seat's dry line is half of the fix; the other half is the rack's
    /// bypass arm, which used to send only the hardware insert through its
    /// bypass and let every other bay's input past untouched while the bay's
    /// figure stayed in the compensation graph. So a pad note on lane 5,
    /// through a tape with every colour off in that lane's own first bay, is
    /// played three ways in the offline engine: in circuit; bypassed from
    /// before the note; and bypassed, then put back in circuit at the
    /// twentieth block, mid-note. Bypassed it sounds on the very frame the
    /// tape in circuit does, at the same level, and flipped mid-note it
    /// never steps by more than the tone's own largest step and a
    /// thousandth, which a lane jumping by the tape's 96 frames would, by
    /// about the tone's whole height.
    #[test]
    fn a_bypassed_tape_keeps_its_lane_in_time() {
        use crate::instrument::{ClipNote, Engine, PadKit, PlacedMidi};
        use std::path::{Path, PathBuf};
        use std::sync::Arc;
        const BLOCK: usize = 256;
        const LANE: usize = 5;
        let spec = spec_for(TAPE_PATH).unwrap();
        let neutral = state_text(spec, &[0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 2.0 / 3.0, 0.0], &[]).into_bytes();
        let amp = 0.05_f64;
        let tone: Vec<f32> = (0..24_000)
            .flat_map(|i| {
                let v = (amp * (std::f64::consts::TAU * 440.0 * f64::from(i) / 48_000.0).sin()) as f32;
                [v, v]
            })
            .collect();
        let largest = tone
            .chunks_exact(2)
            .map(|f| f[0])
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max);
        let note = |bypassed: bool, flip_at: Option<usize>| -> Vec<f32> {
            let mut e = Engine::offline(48_000, tangent_record::audio::Timebase::new()).expect("offline");
            e.load_insert(LANE, 0, Some(Path::new(TAPE_PATH)), Some(&neutral))
                .expect("the tape loads")
                .expect("the bay reports it");
            if bypassed {
                e.set_insert_bypass(&[(LANE, 0)]);
            }
            e.set_pad_kit(LANE, {
                let mut kit = PadKit::default();
                kit.pads[0] = Some(Arc::new(tangent_record::decode::Clip {
                    samples: tone.clone(),
                    rate: 48_000,
                    source: PathBuf::new(),
                }));
                kit
            });
            e.set_channel_gain(LANE, 1.0);
            e.set_drum_strips(1 << LANE);
            let mut out = vec![0.0f32; BLOCK * 2];
            for _ in 0..8 {
                out.fill(0.0);
                e.render_offline(&mut out);
            }
            e.set_midi_clips(vec![PlacedMidi {
                pedal: Arc::new(Vec::new()),
                strip: LANE,
                start: 0,
                len: 48_000,
                notes: Arc::new(vec![ClipNote {
                    at: 0,
                    dur: 20_000,
                    key: tangent_ui::recorder::DRUM_PAD_BASE,
                    vel: 100,
                }]),
            }]);
            e.set_transport(1, 0);
            e.set_rolling(true);
            let mut heard = Vec::new();
            for k in 0..80 {
                if Some(k) == flip_at {
                    e.set_insert_bypass(&[]);
                }
                out.fill(0.0);
                e.render_offline(&mut out);
                heard.extend(out.iter().step_by(2));
            }
            heard
        };
        let onset = |t: &[f32]| t.iter().position(|v| v.abs() > 1.0e-3).expect("a sound");
        let level = |t: &[f32], at: usize| {
            let w = &t[at + 1_000..at + 5_800];
            (w.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / w.len() as f64).sqrt()
        };
        let through = note(false, None);
        let at = onset(&through);
        let bypassed = note(true, None);
        assert_eq!(
            onset(&bypassed),
            at,
            "the bypassed tape moved its lane by {} frames",
            onset(&bypassed) as i64 - at as i64
        );
        let gain = 20.0 * (level(&bypassed, at) / level(&through, at)).log10();
        assert!(gain.abs() < 0.1, "bypassed, the lane is {gain:+.3} dB from the tape in circuit");
        let flipped = note(true, Some(20));
        assert_eq!(onset(&flipped), at, "the lane bypassed until mid-note moved");
        for (k, w) in flipped[at..at + 13_000].windows(2).enumerate() {
            let step = (w[1] - w[0]).abs();
            assert!(
                step <= largest + 1.0e-3,
                "frame {k} after the onset stepped by {step:.4}, against the tone's own \
                 {largest:.4}: the lane jumped when the tape came back"
            );
        }
    }

    /// **The bias reads signed about aligned, and every reading reads back.**
    ///
    /// Bias is the record bias in decibels either side of the point the
    /// machine is aligned at, and the middle of its dial is that point. So
    /// the middle reads "aligned", the machine's own word, and a hand typing
    /// the word lands exactly there, where a drag's catch and a double-click
    /// home land too; either side reads signed decibels, the same figure
    /// with its sign turned at the same distance either way; and every
    /// reading the dial can show, typed back into it, lands where the
    /// reading says the same: the word, or the same number. The middle
    /// answers to zero as well as to its word now, and no reading is a
    /// signed zero: the test after this one holds that.
    #[test]
    fn the_bias_reads_signed_about_aligned_and_every_reading_reads_back() {
        assert_eq!(describe(TAPE_PATH, 10, 0.5, &[]), "aligned");
        assert_eq!(value_for_reading(TAPE_PATH, 10, "aligned", &[]), Some(0.5));
        assert_eq!(value_for_reading(TAPE_PATH, 10, "Aligned", &[]), Some(0.5));
        for k in 1..=64 {
            let x = k as f32 / 128.0;
            let (up, down) = (describe(TAPE_PATH, 10, 0.5 + x, &[]), describe(TAPE_PATH, 10, 0.5 - x, &[]));
            assert!(
                up.starts_with('+') && down.starts_with('-') && up[1..] == down[1..],
                "Bias reads {up} at {} and {down} at {}: not signed and symmetric about aligned",
                0.5 + x,
                0.5 - x
            );
        }
        for i in 0..=96 {
            let v = i as f32 / 96.0;
            let shown = describe(TAPE_PATH, 10, v, &[]);
            let typed = value_for_reading(TAPE_PATH, 10, &shown, &[])
                .unwrap_or_else(|| panic!("Bias's own reading {shown} does not read back"));
            let again = describe(TAPE_PATH, 10, typed, &[]);
            assert!(
                again == shown || (reading_number(&again).is_some() && reading_number(&again) == reading_number(&shown)),
                "Bias at {v} reads {shown}, which typed back lands at {typed}, reading {again}"
            );
        }
    }

    /// **A bias typed as zero lands aligned, and no reading is a signed
    /// zero.**
    ///
    /// A reading has a tenth of a decibel, so the dial a hair either side
    /// of its middle printed "+0.0 dB" or "-0.0 dB", and a hand typing 0
    /// into the row, meaning aligned, landed on the first of those the
    /// search met: 127/256, reading "-0.0 dB", the tape 0.047 dB
    /// under-biased and off the panel's centre mark. Every figure the
    /// reading would print as zero is the aligned point in its own
    /// precision, so it reads the word, and a typed number that rounds to
    /// zero, of either sign, with or without its unit, lands on the middle
    /// exactly, as the word does. The word covers that span and no more:
    /// a figure that prints a tenth of a decibel still prints it, signed.
    #[test]
    fn a_bias_typed_as_zero_lands_aligned_and_no_reading_is_a_signed_zero() {
        for typed in ["0", "0 dB", "+0.0", "-0.0", "-0.0 dB", "+0.0 dB", "0.04", "-0.04 dB"] {
            assert_eq!(
                value_for_reading(TAPE_PATH, 10, typed, &[]),
                Some(0.5),
                "Bias typed {typed:?} did not land on the aligned point"
            );
        }
        // The whole travel, finely, and closer still about the middle, where
        // the signed zeroes stood (within 1/240 of it, at 6 dB a side).
        let fine = (0..=65_536).map(|i| i as f32 / 65_536.0);
        let near = (1..=2_000).flat_map(|k| [0.5 + k as f32 * 2.5e-6, 0.5 - k as f32 * 2.5e-6]);
        for v in fine.chain(near) {
            let shown = describe(TAPE_PATH, 10, v, &[]);
            assert!(
                shown == "aligned" || reading_number(&shown).is_some_and(|n| n != 0.0),
                "Bias at {v} reads {shown}: a signed zero, not the aligned point"
            );
        }
        // The word's span is the one the format rounds to zero, no wider.
        let step = 0.1 / (2.0 * crate::units::tape::BIAS_RANGE_DB);
        assert_eq!(describe(TAPE_PATH, 10, 0.5 + 0.8 * step / 2.0, &[]), "aligned");
        assert_eq!(describe(TAPE_PATH, 10, 0.5 - 0.8 * step / 2.0, &[]), "aligned");
        assert_eq!(describe(TAPE_PATH, 10, 0.5 + 1.2 * step / 2.0, &[]), "+0.1 dB");
        assert_eq!(describe(TAPE_PATH, 10, 0.5 - 1.2 * step / 2.0, &[]), "-0.1 dB");
    }

    /// **The tape's selectors offer exactly their positions.** A stepped row
    /// cycles through `choices`, so the list must be as long as the steps
    /// the spec declares and in the kernel's own order, the reading of the
    /// default must be the choice the default lands on, and the default
    /// must survive the state road bit for bit, or a fresh bay saved and
    /// reopened would move off the studio speed by a rounding.
    #[test]
    fn the_tape_selectors_offer_exactly_their_positions() {
        use crate::units::tape::{TapeSpeed, WowRange, TAPE_SPEED_STEPS, WOW_RANGE_STEPS};
        let spec = spec_for(TAPE_PATH).unwrap();
        assert_eq!(TAPE_SPEED_STEPS as usize, TapeSpeed::ALL.len());
        assert_eq!(WOW_RANGE_STEPS as usize, WowRange::ALL.len());
        for (id, positions) in [(6_u32, TapeSpeed::ALL.len()), (9, WowRange::ALL.len())] {
            let row = &TAPE_FACE_PARAMS[id as usize];
            assert_eq!(row.steps as usize, positions, "{} steps", row.symbol);
            let offered = choices(TAPE_PATH, id);
            assert_eq!(offered.len(), positions, "{} offers {offered:?}", row.symbol);
            let at = (row.default * (positions - 1) as f32).round() as usize;
            assert_eq!(
                offered[at],
                describe(TAPE_PATH, id, row.default, &[]),
                "{}'s default reads as a position its choices do not name",
                row.symbol
            );
        }
        for (k, v) in TapeSpeed::ALL.iter().enumerate() {
            assert_eq!(TapeSpeed::from_dial(v.dial()), *v, "{} does not round-trip", v.label());
            assert_eq!(choices(TAPE_PATH, 6)[k], v.label());
        }
        assert_eq!(TAPE_FACE_PARAMS[6].default, TapeSpeed::Ips15.dial());
        for (k, r) in WowRange::ALL.iter().enumerate() {
            assert_eq!(WowRange::from_dial(r.dial()), *r, "{} does not round-trip", r.label());
            assert_eq!(choices(TAPE_PATH, 9)[k], r.label());
        }
        assert_eq!(TAPE_FACE_PARAMS[9].default, WowRange::Studio.dial());
        let text = state_text(spec, &spec.defaults(), &[]);
        let back = values_from_state(spec, text.as_bytes()).expect("own text reads back");
        for (want, got) in spec.defaults().iter().zip(&back) {
            assert_eq!(want.to_bits(), got.to_bits(), "a default changed on the state road");
        }
    }

    /// **A block bigger than the scratch is chunked, not truncated or
    /// reallocated** — the unit fed 1000 frames in one call must equal the
    /// bare kernel fed the same 1000 interleaved, because kernel state
    /// carries across chunk boundaries and that is all a boundary is.
    #[test]
    fn a_block_bigger_than_the_scratch_is_chunked_seamlessly() {
        use crate::units::eq::{EqKernel, EqParams, NodeKind, NodeParams};
        let frames = 1_000;
        assert!(frames > SCRATCH_FRAMES, "the case must overflow the scratch");
        let band = NodeParams { on: true, kind: NodeKind::Bell, freq: 0.5, gain: 0.8, q: 0.5 };

        let spec = spec_for(EQ_PATH).unwrap();
        let mut unit = spec.create(48_000.0, &spec.defaults());
        // Through the road, so the test drives the same decoding a face will.
        unit.set_param(0, 1.0); // on
        unit.set_param(1, 0.0); // bell
        unit.set_param(2, band.freq);
        unit.set_param(3, band.gain);
        unit.set_param(4, band.q);

        let input = tricky_planar(frames);
        let mut out = vec![vec![0.0f32; frames]; 2];
        unit.process(&input, frames, &mut out);

        let mut kernel = EqKernel::new(48_000.0);
        let mut p = EqParams::default();
        p.nodes[0] = band;
        let mut buf = vec![0.0f32; frames * 2];
        for f in 0..frames {
            buf[f * 2] = input[0][f];
            buf[f * 2 + 1] = input[1][f];
        }
        kernel.process(&mut buf, frames, 2, &p);
        for f in 0..frames {
            assert_eq!(out[0][f].to_bits(), buf[f * 2].to_bits(), "left, frame {f}");
            assert_eq!(out[1][f].to_bits(), buf[f * 2 + 1].to_bits(), "right, frame {f}");
        }
    }

    /// **The kind knob steps across every shape, ends inclusive**, the
    /// toggle splits at half travel, and an id no spec names moves nothing —
    /// the stale-curve contract, checked against the ledger a face reads.
    #[test]
    fn the_eq_road_decodes_toggles_menus_and_refuses_stale_ids() {
        use crate::units::eq::NodeKind;
        let spec = spec_for(EQ_PATH).unwrap();
        let BuiltinUnit::Eq(mut u) = spec.create(48_000.0, &spec.defaults()) else {
            panic!("the EQ path built something that is not an EQ");
        };
        let last = (NodeKind::ALL.len() - 1) as f32;
        for (i, want) in NodeKind::ALL.iter().enumerate() {
            u.set_param(1, i as f32 / last);
            assert_eq!(u.params.nodes[0].kind, *want, "position {i}");
        }
        u.set_param(0, 0.49);
        assert!(!u.params.nodes[0].on, "0.49 is the lower half of a toggle");
        u.set_param(0, 0.5);
        assert!(u.params.nodes[0].on, "0.5 is the upper half of a toggle");

        let before = u.params;
        u.set_param(99, 0.0);
        u.set_param(u32::MAX, 1.0);
        assert_eq!(u.params, before, "an unknown id moved a knob");

        // And a NaN lands on the spec's default, not wherever it was.
        u.set_param(3, f32::NAN);
        assert_eq!(u.params.nodes[0].gain, 0.5, "NaN did not read as the default");
    }

    /// **The comp unit's meter answers through the enum and resets on read**
    /// — the same read-and-reset the kernel promises, proven where the
    /// engine will actually call it.
    #[test]
    fn the_comp_units_meter_answers_and_resets_on_read() {
        let spec = spec_for(COMP_PATH).unwrap();
        let mut squeezed = spec.defaults();
        squeezed[0] = 0.0; // threshold to the floor: everything is over it
        let mut unit = spec.create(48_000.0, &squeezed);
        let frames = 4_800;
        let loud: Vec<f32> = (0..frames)
            .map(|i| 0.9 * (i as f32 * 1_000.0 * std::f32::consts::TAU / 48_000.0).sin())
            .collect();
        let input = vec![loud.clone(), loud];
        let mut out = vec![vec![0.0f32; frames]; 2];
        unit.process(&input, frames, &mut out);
        let gr = unit.gain_reduction_db().expect("a comp has a meter");
        assert!(gr > 6.0, "a floored threshold only metered {gr:.1} dB");
        assert_eq!(
            unit.gain_reduction_db(),
            Some(0.0),
            "the meter did not reset on read"
        );
        // Units without a gain element answer None, so the engine's per-block
        // drain costs the others one match arm and nothing else.
        let mut eq = spec_for(EQ_PATH).unwrap().create(48_000.0, &spec_for(EQ_PATH).unwrap().defaults());
        assert_eq!(eq.gain_reduction_db(), None);
    }

    #[test]
    fn every_unit_in_the_table_is_wired_and_honestly_described() {
        // The table is the registry; this walks it the way the loader will.
        for spec in UNITS {
            assert!(spec.path.starts_with('@'), "{} is not a sentinel", spec.path);
            assert!(spec.class.starts_with("Tangent "), "{}", spec.class);
            let unit = spec.create(48_000.0, &spec.defaults());
            // Latency answers something; the state round-trips at defaults.
            let _ = unit.latency();
            let text = state_text(spec, &spec.defaults(), &[]);
            assert_eq!(
                values_from_state(spec, text.as_bytes()).unwrap(),
                spec.defaults()
            );
            // Ids and symbols are unique within the unit — the append-only
            // spaces the doc comments promise.
            for (i, a) in spec.params.iter().enumerate() {
                for b in &spec.params[i + 1..] {
                    assert_ne!(a.id, b.id, "{} reuses id {}", spec.path, a.id);
                    assert_ne!(a.symbol, b.symbol, "{} reuses {}", spec.path, a.symbol);
                }
                // A knob symbol may never collide with the unit's EXTRA
                // state space — one blob, one meaning per line.
                assert!(
                    !is_extra_symbol(spec.path, a.symbol),
                    "{} names {} as both a knob and extra state",
                    spec.path,
                    a.symbol
                );
            }
        }
    }

    /// **Every registered unit processes finite audio through the planar
    /// road at its defaults** — the smallest liveness claim, walked over the
    /// whole table so a kernel that panics on its first block (a bad map, a
    /// zero-length ring) is caught at the registry rather than in a session.
    #[test]
    fn every_unit_in_the_table_processes_finite_audio() {
        let frames = 2_048;
        let input = tricky_planar(frames);
        for spec in UNITS {
            let mut unit = spec.create(48_000.0, &spec.defaults());
            let mut out = vec![vec![7.0f32; frames]; 2];
            for _ in 0..3 {
                unit.process(&input, frames, &mut out);
            }
            for (c, ch) in out.iter().enumerate() {
                for (f, s) in ch.iter().enumerate() {
                    assert!(s.is_finite(), "{} channel {c} frame {f}: {s}", spec.path);
                }
            }
        }
    }

    /// **Extra state lines ride the blob beside the knobs and stay out of
    /// their space.** The values reader skips them, the extra reader claims
    /// exactly its own, and the whole `symbol=value` grammar survives a
    /// round trip — including a path with spaces, because `ir_path` is one.
    #[test]
    fn extra_state_lines_ride_the_blob_and_stay_out_of_the_knob_space() {
        let spec = spec_for(DENOISE_PATH).unwrap();
        let extra = vec![
            ("t0".to_owned(), "0.25".to_owned()),
            ("t31".to_owned(), "0.75".to_owned()),
            ("k0".to_owned(), "0.5".to_owned()),
        ];
        let text = state_text(spec, &spec.defaults(), &extra);
        assert_eq!(
            values_from_state(spec, text.as_bytes()).unwrap(),
            spec.defaults(),
            "a threshold line moved a knob"
        );
        assert_eq!(extra_from_state(spec, text.as_bytes()), extra);

        // The claims are exact: the 32 bands and nothing that merely looks
        // like one — "t32" is out of range, "t07" would alias t7's value
        // under a second spelling, and another unit claims none of them.
        assert!(is_extra_symbol(DENOISE_PATH, "t0"));
        assert!(is_extra_symbol(DENOISE_PATH, "t31"));
        assert!(is_extra_symbol(DENOISE_PATH, "k0"), "the bank's knees ride beside the print");
        assert!(is_extra_symbol(DENOISE_PATH, "k31"));
        assert!(!is_extra_symbol(DENOISE_PATH, "k32"));
        assert!(!is_extra_symbol(DENOISE_PATH, "k07"));
        assert!(!is_extra_symbol(DENOISE_PATH, "t32"));
        assert!(!is_extra_symbol(DENOISE_PATH, "t07"));
        assert!(!is_extra_symbol(DENOISE_PATH, "ir_path"));
        assert!(is_extra_symbol(IR_PATH, "ir_path"));
        assert!(!is_extra_symbol(IR_PATH, "t0"));
        assert!(!is_extra_symbol(EQ_PATH, "t0"));

        // A path with spaces and an equals sign in its tail survives: the
        // split is on the FIRST `=`, so everything after it is the value.
        let spec = spec_for(IR_PATH).unwrap();
        let odd = vec![("ir_path".to_owned(), "/My IRs/room=big.wav".to_owned())];
        let text = state_text(spec, &spec.defaults(), &odd);
        assert_eq!(extra_from_state(spec, text.as_bytes()), odd);
        // And a blob that is not ours answers empty rather than inventing.
        assert!(extra_from_state(spec, b"vstpreset\x00junk").is_empty());
    }

    /// A deterministic broadband rattle for the learn pass — an LCG rather
    /// than anything seeded from time, so the learned thresholds are the
    /// same on every run of this test.
    fn noise_planar(frames: usize, seed: &mut u32) -> Vec<Vec<f32>> {
        let mut next = move || {
            *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (*seed >> 8) as f32 / 16_777_216.0 - 0.5
        };
        let l: Vec<f32> = (0..frames).map(|_| next() * 0.2).collect();
        let r: Vec<f32> = (0..frames).map(|_| next() * 0.2).collect();
        vec![l, r]
    }

    /// **A denoise seated from a saved blob reports its latency BEFORE it
    /// has seen a block.**
    ///
    /// `Denoise::latency` answers from the kernel's own `kernel` and
    /// `la_on` fields, and those were only ever written inside `process`.
    /// `UnitSpec::create` sets the parameters and `instrument`'s
    /// `prepare_builtin` reads `latency()` on the very next line, so a bay
    /// restored on Bank claimed the SPECTRAL frame — 1024 samples at 48 kHz
    /// — and a bay restored on Spectral Live claimed 1024 instead of 512.
    /// Built-ins carry no latency handle, so the only correction road is
    /// `Backend::reported_latency` in the block loop, and that call is
    /// skipped for a BYPASSED bay by design: a project reopened with this
    /// bay bypassed held a phantom 1024 frames in that cell for the whole
    /// session, and `publish_latency` aligned every other lane behind it.
    ///
    /// Read through the real load path — `state_text`, `values_from_state`,
    /// `create` — because the fault lives in the seam between them and a
    /// test that called `set_param` by hand would have walked past it.
    #[test]
    fn a_denoise_seated_from_a_blob_reports_its_latency_before_the_first_block() {
        use crate::units::denoise::{DenoiseKernel, DenoiseParams, DENOISE_LOOKAHEAD_MS};
        let spec = spec_for(DENOISE_PATH).unwrap();
        let la = (48_000.0 * DENOISE_LOOKAHEAD_MS / 1_000.0).round() as u32;
        for (kernel, lookahead, want) in [
            (DenoiseKernel::Bank, 1.0_f32, la),
            (DenoiseKernel::Bank, 0.0, 0),
            (DenoiseKernel::SpectralStudio, 1.0, 1_024),
            (DenoiseKernel::SpectralLive, 1.0, 512),
            (DenoiseKernel::SpectralLive, 0.0, 512),
        ] {
            let p = DenoiseParams {
                lookahead,
                kernel: kernel.dial(),
                ..DenoiseParams::default()
            };
            let values = vec![p.learn, p.reduce, p.smooth, p.release, p.sc_hpf, p.lookahead, p.kernel];
            let text = state_text(spec, &values, &[]);
            let back = values_from_state(spec, text.as_bytes()).expect("own text reads back");
            let unit = spec.create(48_000.0, &back);
            assert_eq!(
                unit.latency(),
                want,
                "{} with lookahead {lookahead} reported {} frames before its first block",
                kernel.label(),
                unit.latency()
            );
        }

        // And the disclosure this unit's Kernel selector owes every blob
        // written before it existed: a state block with no `kernel=` line
        // reads as the spec default, which is Spectral Studio, so it opens
        // on the spectral engine and reports the frame rather than the
        // bank's zero. That is deliberate — the enum's own doc argues for
        // it and the CHANGELOG says so out loud — and it is written down
        // here so that changing it is a decision rather than a drift. The
        // profile such a blob carries was learned by the BANK, and a
        // profile belongs to the kernel that learned it.
        let old_blob = b"tangentfx 1\nlearn=0\nreduce=0.5\nsmooth=0.5\nrelease=0.5\nsc_hpf=0\nlookahead=1\n";
        let back = values_from_state(spec, old_blob).expect("a pre-kernel blob still reads");
        assert_eq!(
            back[6],
            DenoiseKernel::SpectralStudio.dial(),
            "a blob written before the Kernel parameter no longer reads as Spectral Studio"
        );
        let unit = spec.create(48_000.0, &back);
        assert_eq!(
            unit.latency(),
            1_024,
            "a pre-kernel blob's bay does not report the spectral frame it will actually cost"
        );
    }

    /// **A fresh denoise opens on the Bank; a saved one plays the engine it
    /// was saved with.**
    ///
    /// The owner, 2026-10-11: "make the bank setting the default for the
    /// denoise plugin". A new bay is `spec.defaults()` through
    /// `UnitSpec::create` (the loader's `None` state), and a reset-to-default
    /// reads the face's `default`, which is the same spec row. A saved bay is
    /// `values_from_state` over its blob, and that must not follow: a blob
    /// that names its kernel keeps it, and a blob written before the selector
    /// existed keeps reading as Spectral Studio, every value of it exactly
    /// what it decoded to before the default moved.
    ///
    /// Revert-check: put `values_from_state` back on `spec.defaults()` and
    /// the pre-selector blob opens on the Bank, which this names.
    #[test]
    fn a_fresh_denoise_opens_on_the_bank_and_a_saved_one_keeps_its_engine() {
        use crate::units::denoise::{DenoiseKernel, DenoiseParams, DENOISE_LOOKAHEAD_MS};
        let spec = spec_for(DENOISE_PATH).unwrap();
        let at = spec.params.iter().position(|p| p.symbol == "kernel").expect("a kernel knob");
        let la = (48_000.0 * DENOISE_LOOKAHEAD_MS / 1_000.0).round() as u32;

        // Fresh: the spec row, the kernel's own default and the face agree,
        // and the latency is the bank's (its lookahead, which ships on).
        let fresh = spec.defaults();
        assert_eq!(DenoiseKernel::from_dial(fresh[at]), DenoiseKernel::Bank);
        assert_eq!(fresh[at], DenoiseParams::default().kernel);
        assert_eq!(describe(DENOISE_PATH, 6, fresh[at], &fresh), "Bank");
        assert_eq!(
            spec.create(48_000.0, &fresh).latency(),
            la,
            "a fresh denoise does not report the bank's lookahead"
        );
        // The owner's other dials did not move with the engine (the softest
        // knee, 100 ms): a fresh bay is what choosing Bank on a fresh bay
        // gave before, with nothing else touched.
        assert_eq!((fresh[2], fresh[3]), (0.0, 0.197), "a fresh bay's smooth/release moved with the engine");

        // Saved, naming its kernel: every engine comes back as itself.
        for k in DenoiseKernel::ALL {
            let mut v = spec.defaults();
            v[at] = k.dial();
            let back = values_from_state(spec, state_text(spec, &v, &[]).as_bytes()).expect("reads");
            assert_eq!(DenoiseKernel::from_dial(back[at]), k, "a saved {} reopened as another engine", k.label());
            assert_eq!(back, v, "a saved {} bay did not come back value for value", k.label());
        }

        // Saved before the selector: exactly the table it decoded to before
        // the fresh default moved, written out rather than derived, so a
        // second default moving underneath it is caught too.
        let old_blob = b"tangentfx 1\nlearn=0\nreduce=0.3\nsmooth=0.5\nrelease=0.5\nsc_hpf=0\nlookahead=0\n";
        let back = values_from_state(spec, old_blob).expect("a pre-kernel blob still reads");
        assert_eq!(
            back,
            vec![0.0, 0.3, 0.5, 0.5, 0.0, 0.0, 0.5, 0.5, 1.0 / 3.0],
            "a blob written before the Kernel parameter no longer decodes as it did"
        );
        assert_eq!(
            DenoiseKernel::from_dial(back[at]),
            DenoiseKernel::SpectralStudio,
            "a blob written before the Kernel parameter followed the fresh default onto the Bank"
        );
        // And a kernel line that does not parse reads as the same promise,
        // as it did before (it read as the spec default, then Studio).
        let garbled = b"tangentfx 1\nkernel=bank\n";
        let back = values_from_state(spec, garbled).expect("reads");
        assert_eq!(DenoiseKernel::from_dial(back[at]), DenoiseKernel::SpectralStudio);
        // No other unit's missing lines changed meaning.
        for other in UNITS.iter().filter(|u| u.path != DENOISE_PATH) {
            let back = values_from_state(other, b"tangentfx 1\n").expect("an empty blob reads");
            assert_eq!(back, other.defaults(), "{}'s empty blob no longer reads as its defaults", other.class);
        }
    }

    /// **Learn, freeze, persist, restore: the denoise's whole threshold
    /// story at the unit level.** The dial up is listening, the falling edge
    /// freezes, the snapshot is offered for exactly the freeze block, and a
    /// fresh instance fed the persisted lines opens with the same bank.
    #[test]
    fn the_denoise_learns_on_the_falling_edge_and_the_extra_state_restores_it() {
        let spec = spec_for(DENOISE_PATH).unwrap();
        // On Spectral Studio by name: this test is about a SPECTRAL print
        // and the bank's knees taken beside it, and the unit opens on the
        // Bank since 2026-10-11.
        let mut studio = spec.defaults();
        studio[6] = crate::units::denoise::DenoiseKernel::SpectralStudio.dial();
        let mut unit = spec.create(48_000.0, &studio);
        assert!(
            unit.learn_snapshot().is_none(),
            "an idle denoise offered a snapshot"
        );
        let frames = 512;
        let mut out = vec![vec![0.0f32; frames]; 2];
        let mut seed = 7u32;
        // Listening: ~213 ms of noise, past the kernel's 200 ms minimum
        // pass. This was 15 blocks and 160 ms, which cleared the 10.7 ms
        // guard the bank shipped with; the guard is 200 ms now, because a
        // tenth of a second is a mouse bounce and both kernels' learned
        // statistics are averages that mean nothing over one.
        unit.set_param(0, 1.0);
        for _ in 0..20 {
            let input = noise_planar(frames, &mut seed);
            unit.process(&input, frames, &mut out);
        }
        // The falling edge: THIS block freezes, and the snapshot says so.
        unit.set_param(0, 0.0);
        let input = noise_planar(frames, &mut seed);
        unit.process(&input, frames, &mut out);
        let (thr, knees) = unit
            .learn_snapshot()
            .expect("the freeze block must offer the thresholds");
        assert!(
            thr.iter().any(|t| *t > 0.05),
            "a broadband learn froze an all-off bank: {thr:?}"
        );
        // The unit runs Spectral Studio here, so this print is the spectral
        // kernel's own reading and the knees beside it are the BANK's
        // measurement of the same noise, taken by `learn_feed` during the
        // same pass - see `Band::knee_db`. Real knees, then: every band
        // heard, in range, and not simply the print copied over.
        assert!(
            knees.iter().all(|k| k.is_finite() && (0.0..=1.0).contains(k) && *k > 0.05),
            "a broadband spectral learn left the bank's knees unheard: {knees:?}"
        );
        assert!(
            knees.iter().zip(thr.iter()).any(|(k, t)| (k - t).abs() > 1.0e-3),
            "a spectral print's knees were the print itself, so the bank never listened"
        );
        // One block later the offer is withdrawn — the ledger has it now.
        let input = noise_planar(frames, &mut seed);
        unit.process(&input, frames, &mut out);
        assert!(
            unit.learn_snapshot().is_none(),
            "a settled denoise kept offering its snapshot"
        );

        // Persist exactly as the host does, restore into a fresh instance,
        // and the bank matches to the text format's precision.
        let extra: Vec<(String, String)> = thr
            .iter()
            .enumerate()
            .map(|(k, t)| (format!("t{k}"), t.to_string()))
            .chain(knees.iter().enumerate().map(|(k, t)| (format!("k{k}"), t.to_string())))
            .collect();
        let mut restored = spec.create(48_000.0, &studio);
        assert!(restored.apply_extra_state(&extra).is_none());
        let BuiltinUnit::Denoise(r) = &restored else {
            panic!("the denoise path built something else");
        };
        for (k, (a, b)) in thr.iter().zip(r.kernel.thresholds()).enumerate() {
            assert!((a - b).abs() < 1.0e-6, "band {k}: froze {a}, restored {b}");
        }
        // And the knees came back as their own lines, not as the print.
        for (k, (a, b)) in knees.iter().zip(r.kernel.knees()).enumerate() {
            assert!((a - b).abs() < 1.0e-5, "band {k}: knee froze {a}, restored {b}");
        }
    }

    /// **A reading's number, in its base unit, however the reading spells
    /// it** - and the same for what a hand types, so the two compare.
    #[test]
    fn a_readings_number_is_read_in_its_base_unit() {
        let cases: [(&str, Option<f64>); 14] = [
            ("+3.0 dB", Some(3.0)),
            ("-12.5 dB", Some(-12.5)),
            ("440 Hz", Some(440.0)),
            ("1.5 kHz", Some(1500.0)),
            ("1.5k", Some(1500.0)),
            ("35%", Some(35.0)),
            ("4.0:1", Some(4.0)),
            ("Q 0.71", Some(0.71)),
            ("2.00 s", Some(2000.0)),
            ("500 ms", Some(500.0)),
            ("0 dB knee", Some(0.0)),
            ("off", None),
            ("Spectral only", None),
            ("", None),
        ];
        for (text, want) in cases {
            assert_eq!(reading_number(text), want, "{text:?}");
        }
    }

    /// **Words typed into a reading land the knob where the reading says
    /// those words** - the inverse of `describe`, found by search rather
    /// than spelled per knob, on every kind of scale the suite prints:
    /// linear dB, a log sweep in Hz, a ratio, a percent, a knob that says
    /// "off" at its foot, and a number past the end.
    #[test]
    fn typed_words_land_a_knob_where_its_reading_says_them() {
        let none: &[f32] = &[];
        let reads = |path: &str, id: u32, v: f32| describe(path, id, v, none);
        // The trim: a linear dB range.
        let v = value_for_reading(TRIM_PATH, 0, "-6", none).expect("dB");
        assert_eq!(reads(TRIM_PATH, 0, v), "-6.0 dB");
        let v = value_for_reading(TRIM_PATH, 0, "+3 dB", none).expect("dB with its unit");
        assert_eq!(reads(TRIM_PATH, 0, v), "+3.0 dB");
        // An EQ frequency: a log sweep, typed three ways. Compared as a
        // number, because `fmt_hz` says "1000 Hz" on one side of a
        // thousand and "1.0 kHz" on the other and either is the answer.
        for typed in ["1000", "1k", "1.0 kHz"] {
            let v = value_for_reading(EQ_PATH, 2, typed, none).expect(typed);
            let hz = reading_number(&reads(EQ_PATH, 2, v)).expect("a frequency");
            assert!((hz - 1000.0).abs() <= 5.0, "{typed} landed on {hz} Hz");
        }
        let v = value_for_reading(EQ_PATH, 2, "440", none).expect("Hz");
        assert_eq!(reads(EQ_PATH, 2, v), "440 Hz");
        // The compressor's ratio and a percent.
        let v = value_for_reading(COMP_PATH, 1, "4", none).expect("ratio");
        assert_eq!(reads(COMP_PATH, 1, v), "4.0:1");
        let v = value_for_reading(COMP_PATH, 5, "35%", none).expect("percent");
        assert_eq!(reads(COMP_PATH, 5, v), "35%");
        // A word the knob has a position for.
        let v = value_for_reading(IMAGER_PATH, 2, "off", none).expect("off");
        assert_eq!(reads(IMAGER_PATH, 2, v), "off");
        // Past either end: the end.
        assert_eq!(value_for_reading(COMP_PATH, 5, "250", none), Some(1.0));
        assert_eq!(value_for_reading(COMP_PATH, 5, "-40", none), Some(0.0));
        // Not a number, and not a word the knob knows: nothing moves.
        assert_eq!(value_for_reading(COMP_PATH, 5, "loud", none), None);
        assert_eq!(value_for_reading(COMP_PATH, 5, "   ", none), None);
    }

    /// Write a 32-bit float WAV the test owns byte for byte, so the IR road
    /// is proven against a real file rather than a mocked reader.
    fn write_float_wav(name: &str, rate: u32, channels: u16, samples: &[f32]) -> std::path::PathBuf {
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&3u16.to_le_bytes());
        v.extend_from_slice(&channels.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(&32u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&((samples.len() * 4) as u32).to_le_bytes());
        for s in samples {
            v.extend_from_slice(&s.to_le_bytes());
        }
        let path = std::env::temp_dir().join(format!("tangent-ir-{}-{name}.wav", std::process::id()));
        std::fs::write(&path, v).expect("the temp WAV writes");
        path
    }

    /// **An impulse IR makes the convolver a 512-frame-late wire, and every
    /// way the file can be wrong is a sentence, not a refusal to seat.**
    #[test]
    fn an_impulse_ir_convolves_as_a_delayed_wire_and_bad_files_read_as_sentences() {
        use crate::units::convolver::CONV_PARTITION;
        let mut impulse = vec![0.0f32; 2_048];
        impulse[0] = 1.0; // interleaved stereo: frame 0 left only
        impulse[1] = 1.0; // and right, so both channels carry the wire
        let good = write_float_wav("good", 48_000, 2, &impulse);

        let spec = spec_for(IR_PATH).unwrap();
        let mut unit = spec.create(48_000.0, &spec.defaults());
        assert_eq!(unit.latency(), 0, "an empty convolver billed a partition");
        let extra = vec![("ir_path".to_owned(), good.display().to_string())];
        assert_eq!(unit.apply_extra_state(&extra), None);
        assert_eq!(unit.latency(), CONV_PARTITION as u32);

        // Through the planar road: input against output, 512 frames apart.
        // Long enough that the first-load fade (two partitions) is history.
        let frames = 4_096;
        let input = {
            let mut seed = 99u32;
            noise_planar(frames, &mut seed)
        };
        let mut out = vec![vec![0.0f32; frames]; 2];
        unit.process(&input, frames, &mut out);
        for c in 0..2 {
            for f in CONV_PARTITION * 3..frames {
                let (want, got) = (input[c][f - CONV_PARTITION], out[c][f]);
                assert!(
                    (want - got).abs() < 1.0e-3,
                    "channel {c} frame {f}: sent {want}, heard {got}"
                );
            }
        }

        // A wrong-rate capture loads AND says so — the sentence names both
        // rates, because "it sounds shifted" starts as "why".
        let slow = write_float_wav("slow", 44_100, 1, &[1.0, 0.0, 0.0, 0.0]);
        let mut unit = spec.create(48_000.0, &spec.defaults());
        let extra = vec![("ir_path".to_owned(), slow.display().to_string())];
        let note = unit.apply_extra_state(&extra).expect("a mismatch says so");
        assert!(note.contains("44100") && note.contains("48000"), "{note}");
        assert_eq!(unit.latency(), CONV_PARTITION as u32, "it still loaded");

        // A missing file is a sentence and a wire: nothing loaded, nothing
        // billed, audio untouched.
        let mut unit = spec.create(48_000.0, &spec.defaults());
        let extra = vec![("ir_path".to_owned(), "/nowhere/tangent-gone.wav".to_owned())];
        assert!(unit.apply_extra_state(&extra).is_some());
        assert_eq!(unit.latency(), 0);
        let input = tricky_planar(64);
        let mut out = vec![vec![7.0f32; 64]; 2];
        unit.process(&input, 64, &mut out);
        for (src, dst) in input.iter().zip(&out) {
            for (s, d) in src.iter().zip(dst) {
                assert_eq!(s.to_bits(), d.to_bits(), "an IR-less bay touched the audio");
            }
        }

        let _ = std::fs::remove_file(good);
        let _ = std::fs::remove_file(slow);
    }

    /// **The new dials read in their own units** — spot checks holding the
    /// describe arms to the kernels' published mappings, one per mapping
    /// family, so a describe arm quoting the wrong constant fails here and
    /// not on somebody's panel.
    #[test]
    fn the_new_dials_read_in_their_own_units() {
        use crate::units::echo::ECHO_TIME_DEFAULT;
        use tangent_ui::face::fmt_pct;
        assert_eq!(describe(ECHO_PATH, 0, ECHO_TIME_DEFAULT, &[]), "375 ms");
        assert_eq!(describe(PLATE_PATH, 0, 0.34, &[]), "2.20 s");
        assert_eq!(describe(PLATE_PATH, 1, 0.0, &[]), "0.0 ms");
        assert_eq!(describe(WAH_PATH, 1, 0.5, &[]), "240 Hz");
        assert_eq!(describe(DENOISE_PATH, 0, 0.0, &[]), "Learn");
        assert_eq!(describe(DENOISE_PATH, 0, 1.0, &[]), "Listening");
        assert_eq!(describe(DENOISE_PATH, 1, 0.5, &[]), "-12.0 dB");
        assert_eq!(describe(DENOISE_PATH, 1, 0.0, &[]), "off");
        assert_eq!(describe(DENOISE_PATH, 5, 1.0, &[]), "3.0 ms");
        // The kernel selector reads as the engine, in the words the
        // cycling row offers, at each of its three positions.
        assert_eq!(describe(DENOISE_PATH, 6, 0.0, &[]), "Bank");
        assert_eq!(describe(DENOISE_PATH, 6, 0.5, &[]), "Spectral Studio");
        assert_eq!(describe(DENOISE_PATH, 6, 1.0, &[]), "Spectral Live");
        // The shipped level is unity to a tenth of a dB: 0.71² · 2 ≈ 1.008.
        assert_eq!(describe(DRIVE_PATH, 2, 0.71, &[]), "+0.1 dB");
        assert_eq!(describe(DRIVE_PATH, 3, 0.0, &[]), "matched");
        assert_eq!(describe(IR_PATH, 1, 0.5, &[]), "+0.0 dB");
        // The tape's calibrated dials, at the positions it opens at and the
        // Roll 5.0 shipped: the bump's own +2.5 dB, the repro corner just
        // under 16 kHz and off, unity trim, and the studio speed in the
        // selector's words. A face that printed "50 %" for any of these
        // disagreed with the automation lane's declared unit about the same
        // knob.
        assert_eq!(describe(TAPE_PATH, 1, 0.5, &[]), "+2.5 dB");
        assert_eq!(describe(TAPE_PATH, 2, 0.12, &[]), "15.6 kHz");
        assert_eq!(describe(TAPE_PATH, 2, 0.0, &[]), "off");
        assert_eq!(describe(TAPE_PATH, 5, 0.5, &[]), "+0.0 dB");
        assert_eq!(describe(TAPE_PATH, 6, 2.0 / 3.0, &[]), "15 ips");
        assert_eq!(describe(TAPE_PATH, 6, 0.0, &[]), "3.75 ips");
        assert_eq!(describe(TAPE_PATH, 6, 1.0 / 3.0, &[]), "7.5 ips");
        assert_eq!(describe(TAPE_PATH, 6, 1.0, &[]), "30 ips");
        assert_eq!(choices(TAPE_PATH, 6), vec!["3.75 ips", "7.5 ips", "15 ips", "30 ips"]);
        // Rate as the multiple its parts turn at, and read back from it.
        assert_eq!(describe(TAPE_PATH, 8, 0.0, &[]), "0.50x");
        assert_eq!(describe(TAPE_PATH, 8, 0.5, &[]), "1.00x");
        assert_eq!(describe(TAPE_PATH, 8, 1.0, &[]), "2.00x");
        let v = value_for_reading(TAPE_PATH, 8, "1.5x", &[]).expect("a rate");
        assert_eq!(describe(TAPE_PATH, 8, v, &[]), "1.50x");
        assert!(choices(TAPE_PATH, 8).is_empty(), "Rate grew steps");
        // The range in the selector's words.
        assert_eq!(describe(TAPE_PATH, 9, 0.0, &[]), "studio");
        assert_eq!(describe(TAPE_PATH, 9, 1.0, &[]), "deep");
        assert_eq!(choices(TAPE_PATH, 9), vec!["studio", "deep"]);
        // The bias, in the machine's word at its middle and in signed
        // decibels either side of it.
        assert_eq!(describe(TAPE_PATH, 10, 0.5, &[]), "aligned");
        assert_eq!(describe(TAPE_PATH, 10, 0.0, &[]), "-6.0 dB");
        assert_eq!(describe(TAPE_PATH, 10, 1.0, &[]), "+6.0 dB");
        assert_eq!(describe(TAPE_PATH, 10, 0.75, &[]), "+3.0 dB");
        assert!(choices(TAPE_PATH, 10).is_empty(), "Bias grew steps");
        // And the six that are character rather than calibration keep
        // saying so.
        assert_eq!(describe(TAPE_PATH, 0, 0.6, &[]), fmt_pct(0.6));
        assert_eq!(describe(TAPE_PATH, 3, 0.3, &[]), fmt_pct(0.3));
        assert_eq!(describe(TAPE_PATH, 4, 0.25, &[]), fmt_pct(0.25));
        assert_eq!(describe(TAPE_PATH, 7, 0.3, &[]), fmt_pct(0.3));
        assert_eq!(describe(TAPE_PATH, 11, 0.5, &[]), fmt_pct(0.5));
        assert_eq!(describe(TAPE_PATH, 12, 0.25, &[]), fmt_pct(0.25));
        assert_eq!(choices(DENOISE_PATH, 0), vec!["Learn", "Listening"]);
        assert_eq!(
            choices(DENOISE_PATH, 6),
            vec!["Bank", "Spectral Studio", "Spectral Live"]
        );
        assert_eq!(choices(DRIVE_PATH, 3), vec!["matched", "uneven"]);
        assert_eq!(choices(AMP_PATH, 0), vec!["off", "on"]);
        assert!(choices(TREM_PATH, 0).is_empty(), "a continuous dial grew steps");
    }

    /// **A knob that does nothing must not print a number.**
    ///
    /// `Denoise::process` sends every kernel but the Bank to
    /// `process_spectral` and returns; `params.smooth` and `params.release`
    /// are read below that line. So on Spectral Studio (the unit's SHIPPED
    /// kernel until 2026-10-11) those two dials are inert, and the face used to print a
    /// confident "2.8:1" and "219 ms" under both of them. Measured, not
    /// inferred: the same take rendered at smooth 0.0 and 1.0 and at release
    /// 0.0 and 1.0 comes back byte for byte identical, all four hashes equal
    /// to the untouched render.
    ///
    /// Revert-check: delete the `2 | 3 if !denoise_reads_the_gate(..)` arm in
    /// `describe` and the two spectral halves of this go red with the name of
    /// the knob that lied.
    #[test]
    fn the_denoiser_says_which_of_its_knobs_the_running_kernel_ignores() {
        use crate::units::denoise::DenoiseKernel;
        let spec = spec_for(DENOISE_PATH).expect("the denoiser has a spec");
        let at = spec
            .params
            .iter()
            .position(|p| p.symbol == "kernel")
            .expect("the denoiser has a kernel knob");
        // A whole bay's worth of values, with the kernel where the spec puts
        // it rather than where this test guesses.
        let bay = |k: DenoiseKernel| {
            let mut v: Vec<f32> = spec.params.iter().map(|p| p.default).collect();
            v[at] = k.dial();
            v
        };

        // The Bank reads SMOOTH as a ratio and RELEASE as its own release,
        // and has neither an attack nor a treble dial — and says so.
        let bank = bay(DenoiseKernel::Bank);
        assert_eq!(describe(DENOISE_PATH, 2, 0.5, &bank), "2.8:1");
        assert_eq!(describe(DENOISE_PATH, 3, 0.5, &bank), "219 ms");
        assert_eq!(describe(DENOISE_PATH, 7, 0.5, &bank), "Spectral only");
        assert_eq!(describe(DENOISE_PATH, 8, 0.5, &bank), "Spectral only");

        // On the spectral kernels SMOOTH is the opening knee, not a ratio,
        // and RELEASE, ATTACK and TREBLE are live.
        for k in [DenoiseKernel::SpectralStudio, DenoiseKernel::SpectralLive] {
            let v = bay(k);
            assert_eq!(describe(DENOISE_PATH, 2, 0.5, &v), "12 dB knee", "SMOOTH on {}", k.label());
            assert_eq!(describe(DENOISE_PATH, 3, 0.5, &v), "219 ms", "RELEASE on {}", k.label());
            assert_eq!(describe(DENOISE_PATH, 7, 0.5, &v), "10 ms", "ATTACK on {}", k.label());
            assert_eq!(describe(DENOISE_PATH, 8, 0.5, &v), "+3.0 dB", "TREBLE on {}", k.label());
            assert_eq!(describe(DENOISE_PATH, 8, 0.0, &v), "off");
            // Everything else on that face keeps its own reading.
            assert_eq!(describe(DENOISE_PATH, 1, 0.5, &v), "-12.0 dB");
        }

        // An empty slice is a bay whose values have not arrived. It reads as
        // Bank, because printing "Bank only" over a knob that works is the
        // worse of the two wrongs.
        assert_eq!(describe(DENOISE_PATH, 2, 0.5, &[]), "2.8:1");
    }

    /// Two channels of independent noise: a record with a real image.
    fn stereo_program(frames: usize) -> Vec<Vec<f32>> {
        let mut a = 0x51DE_0001_u32;
        let mut b = 0x51DE_0002_u32;
        let step = |s: &mut u32| {
            *s ^= *s << 13;
            *s ^= *s >> 17;
            *s ^= *s << 5;
            0.5 * ((*s as f32 / u32::MAX as f32) * 2.0 - 1.0)
        };
        let l: Vec<f32> = (0..frames).map(|_| step(&mut a)).collect();
        let r: Vec<f32> = (0..frames).map(|_| step(&mut b)).collect();
        vec![l, r]
    }

    /// **One microphone.** `mix_channels` sends a mono input to both sides,
    /// so this is what most of the desk's tracks look like by the time they
    /// reach a bay — and a pair with `L == R` has a side of exactly zero,
    /// which is the shape that made every control on the imager a wire.
    fn microphone_program(frames: usize) -> Vec<Vec<f32>> {
        let mut s = 0x51DE_0003_u32;
        let mono: Vec<f32> = (0..frames)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                0.5 * ((s as f32 / u32::MAX as f32) * 2.0 - 1.0)
            })
            .collect();
        vec![mono.clone(), mono]
    }

    /// How far apart two runs of the same program came out, in dB relative
    /// to the program — the only question a dead-control test may ask.
    fn divergence_db(a: &[Vec<f32>], b: &[Vec<f32>], skip: usize) -> f64 {
        let mut diff = 0.0_f64;
        let mut program = 0.0_f64;
        for (x, y) in a.iter().zip(b) {
            for (p, q) in x.iter().skip(skip).zip(y.iter().skip(skip)) {
                diff += f64::from(*p - *q).powi(2);
                program += f64::from(*p).powi(2);
            }
        }
        10.0 * (diff / program.max(1.0e-30)).log10()
    }

    /// Run one unit at one value table over the planar road, from a fresh
    /// instance, exactly as a bay would after a load.
    fn through_seat(spec: &'static UnitSpec, values: &[f32], input: &[Vec<f32>]) -> Vec<Vec<f32>> {
        let frames = input[0].len();
        let mut unit = spec.create(48_000.0, values);
        let mut out = vec![vec![0.0f32; frames]; 2];
        unit.process(input, frames, &mut out);
        out
    }

    /// **A control that does not move the OUTPUT is a decoration, whatever
    /// the ledger says it is set to.**
    ///
    /// The existing tests here prove a unit is a NULL at rest, which is the
    /// other half of the same question and the half that was already asked.
    /// This asks the half that was not, at the seat rather than in the
    /// kernel: the value goes in through `set_param` — the id decoding a
    /// face actually drives — and what comes out the other end of the planar
    /// road has to be a different sound.
    ///
    /// The floor is deliberately low. This test's job is "did the value
    /// arrive"; whether the move is worth HEARING is pinned per knob in each
    /// kernel's own module, where the right measurement for a bump is dB at
    /// the head's own bump frequency and the right one for a drive is
    /// harmonic content.
    #[test]
    fn every_imager_and_tape_control_moves_the_audio_through_the_seat() {
        const FRAMES: usize = 24_000;
        // Past the tape's line fill and envelope settling; the imager has
        // no such warm-up but pays nothing for the skip.
        const SKIP: usize = 4_800;
        const FLOOR_DB: f64 = -50.0;
        let program = stereo_program(FRAMES);

        for path in [IMAGER_PATH, TAPE_PATH] {
            let spec = spec_for(path).unwrap();
            let rest = through_seat(spec, &spec.defaults(), &program);
            for p in spec.params {
                let mut best = f64::MIN;
                for end in [0.0_f32, 1.0] {
                    let mut values = spec.defaults();
                    values[p.id as usize] = end;
                    let moved = through_seat(spec, &values, &program);
                    best = best.max(divergence_db(&rest, &moved, SKIP));
                }
                assert!(
                    best > FLOOR_DB,
                    "{path}: {} moved the output by {best:.1} dB — the knob is \
                     wired to nothing",
                    p.symbol
                );
            }
        }
    }

    /// **The fault a demo session found, pinned at the seat.**
    ///
    /// On one microphone the imager did nothing at any setting of any
    /// control, and the vectorscope — which reads the same post-bay audio —
    /// drew the vertical line to match. Two of the three knobs are innocent:
    /// width scales a side and a mono pair has none, so width doing nothing
    /// there is arithmetic, and this test says so out loud rather than
    /// leaving the next reader to rediscover it. The ensemble is the control
    /// that owes this case an answer, and it was convolving the very signal
    /// it exists to manufacture.
    ///
    /// So: the ensemble must widen a mono pair on its own, and once it has,
    /// the other two must bite.
    #[test]
    fn one_microphone_through_the_imager_seat_answers_the_ensemble() {
        use crate::units::imager::{ensemble_law, ensemble_trim};
        const FRAMES: usize = 24_000;
        const SKIP: usize = 2_400;
        let program = microphone_program(FRAMES);
        let spec = spec_for(IMAGER_PATH).unwrap();
        let ids = |symbol: &str| {
            spec.params
                .iter()
                .position(|p| p.symbol == symbol)
                .expect("the imager lost a parameter")
        };
        let (width, ensemble, mono_below) = (ids("width"), ids("ensemble"), ids("mono_below"));

        let rest = through_seat(spec, &spec.defaults(), &program);
        // The honest part: with no side to scale, the side controls cannot
        // and must not invent one.
        for id in [width, mono_below] {
            for end in [0.0_f32, 1.0] {
                let mut values = spec.defaults();
                values[id] = end;
                let moved = through_seat(spec, &values, &program);
                assert!(
                    divergence_db(&rest, &moved, SKIP) < -100.0,
                    "{} moved a mono pair with the ensemble shut — a widener \
                     may not invent a side out of nothing",
                    spec.params[id].symbol
                );
            }
        }

        // The part that was broken: the ensemble makes the side.
        let mut open = spec.defaults();
        open[ensemble] = 1.0;
        let widened = through_seat(spec, &open, &program);
        assert!(
            divergence_db(&rest, &widened, SKIP) > -12.0,
            "the ensemble left one microphone exactly where it found it: \
             {:.1} dB",
            divergence_db(&rest, &widened, SKIP)
        );
        // And what it made is genuine side, not a level change: the mono fold
        // is the mid it was handed, times the one trim both channels took —
        // and nothing else, to the bit. See `units::imager::ensemble_trim`:
        // an ensemble adds decorrelated power and the trim takes that power
        // back out of the pair, so the fold arrives scaled and never altered.
        // A hole in the mono fold is the one failure this whole unit exists
        // to make impossible, and a scalar cannot hide one.
        let trim = ensemble_trim(ensemble_law(1.0));
        for f in 0..FRAMES {
            let before = 0.5 * (program[0][f] + program[1][f]) * trim;
            let after = 0.5 * (widened[0][f] + widened[1][f]);
            assert!(
                (before - after).abs() < 1.0e-6,
                "the ensemble put something other than the mid in the mono \
                 fold at frame {f}: {before} -> {after}"
            );
        }

        // And now the other two have something to act on.
        for id in [width, mono_below] {
            let mut values = open.clone();
            values[id] = 1.0;
            let moved = through_seat(spec, &values, &program);
            assert!(
                divergence_db(&widened, &moved, SKIP) > -30.0,
                "{} still did nothing to a microphone the ensemble had \
                 already widened",
                spec.params[id].symbol
            );
        }
    }

    // ── the sampler ─────────────────────────────────────────────────────────

    /// A tone that is not silence, decoded shape, for a bay to play.
    fn test_sample(frames: usize, rate: u32) -> crate::instrument::SamplerSample {
        let mut samples = vec![0.0f32; frames * 2];
        for i in 0..frames {
            let x = (i as f32 / 64.0).sin() * 0.5;
            samples[i * 2] = x;
            samples[i * 2 + 1] = x;
        }
        crate::instrument::SamplerSample {
            clip: std::sync::Arc::new(tangent_record::decode::Clip {
                samples,
                rate,
                source: std::path::PathBuf::from("/tmp/tangent-test-sample.wav"),
            }),
            name: "test sample".to_owned(),
            source: std::path::PathBuf::from("/tmp/tangent-test-sample.wav"),
            root_key: crate::instrument::SAMPLER_ROOT_KEY,
        }
    }

    /// **The sampler's registration is the brief's table, id for id, and it
    /// is a voice.** Both halves of the feature index by these ids and read
    /// these symbols; a row out of order is a face whose Attack knob moves
    /// the Decay, and the compiler cannot see it. Every dial reads, and every
    /// stepped one offers exactly its positions — a missing `describe` arm is
    /// a knob that prints a percentage over a mode name.
    #[test]
    fn the_sampler_registers_as_the_briefs_table_and_as_a_voice() {
        let spec = spec_for(SAMPLER_PATH).expect("the sampler is in the table");
        assert!(spec.voice, "the sampler is not a voice");
        assert_eq!(spec.class, "Tangent Sampler");
        let want = [
            "vol", "mode", "transpose", "fine", "attack", "decay", "sustain", "release",
            "start", "end", "noteoff", "loop", "xfade", "voices", "porta", "bend",
        ];
        assert_eq!(spec.params.len(), want.len());
        for (id, symbol) in want.iter().enumerate() {
            let p = &spec.params[id];
            assert_eq!(p.id as usize, id, "row {id} is addressed as {}", p.id);
            assert_eq!(&p.symbol, symbol, "id {id} is not {symbol}");
        }
        let values = spec.defaults();
        for p in spec.params {
            let reading = describe(SAMPLER_PATH, p.id, p.default, &values);
            assert!(!reading.is_empty(), "{} reads nothing", p.symbol);
            let choices = choices(SAMPLER_PATH, p.id);
            if p.steps >= 2 {
                assert_eq!(
                    choices.len(),
                    p.steps as usize,
                    "{} offers {} positions for {} steps",
                    p.symbol,
                    choices.len(),
                    p.steps
                );
                let at = crate::units::sampler::step_index(p.default, p.steps);
                assert_eq!(
                    choices[at], reading,
                    "{}'s reading is not its position's word",
                    p.symbol
                );
            } else {
                assert!(choices.is_empty(), "{} is continuous but offers positions", p.symbol);
            }
        }
        // The readings the brief names, by the kernel's own maps.
        assert_eq!(describe(SAMPLER_PATH, 1, 1.0, &values), "one-shot");
        assert_eq!(describe(SAMPLER_PATH, 0, 0.5, &values), "1.00x");
        assert_eq!(describe(SAMPLER_PATH, 13, 1.0, &values), "32");
        assert_eq!(describe(SAMPLER_PATH, 15, 0.0, &values), "0 semi");
        assert_eq!(describe(SAMPLER_PATH, 10, 1.0, &values), "on");
        assert!(describe(SAMPLER_PATH, 2, 1.0, &values).contains("+24"));
        assert!(describe(SAMPLER_PATH, 3, 0.0, &values).contains("-100"));
        // Portamento at rest is OFF, not "0 ms": a time of zero is a knob
        // that is out of circuit, and the analog's glide says so too.
        assert_eq!(describe(SAMPLER_PATH, 14, 0.0, &values), "off");
    }

    /// **The published defaults ARE the kernel's.** The table cannot call
    /// `sampler::default_norm` — `ParamSpec::default` is a `const` field and
    /// that is a `fn` — so the four tapered rests are spelled twice, once as
    /// arithmetic in a comment and once as the decimal a `const` can hold.
    /// This is the join: it is the only thing standing between the face's
    /// rest and the kernel's, and a drift between those two is a bay that
    /// opens sounding different from the one the tests measured.
    #[test]
    fn the_samplers_defaults_are_the_kernels_own() {
        for p in &SAMPLER_FACE_PARAMS {
            let want = crate::units::sampler::default_norm(p.id);
            assert!(
                (p.default - want).abs() < 1e-6,
                "{} rests at {} in the table and {want} in the kernel",
                p.symbol,
                p.default
            );
        }
        // And the brief's own numbers, read back through the maps.
        use crate::units::sampler as k;
        let at = |id: u32| SAMPLER_FACE_PARAMS[id as usize].default;
        assert!((k::secs(at(k::P_ATTACK), k::ATTACK_SECS) - 0.005).abs() < 1e-5);
        assert!((k::secs(at(k::P_DECAY), k::DECAY_SECS) - 0.20).abs() < 1e-4);
        assert!((k::secs(at(k::P_RELEASE), k::RELEASE_SECS) - 0.15).abs() < 1e-4);
        assert!((k::secs(at(k::P_XFADE), k::XFADE_MS / 1000.0) * 1000.0 - 10.0).abs() < 1e-2);
        assert_eq!(k::step_index(at(k::P_VOICES), k::MAX_VOICES as i32) + 1, 16);
        assert_eq!(k::step_index(at(k::P_BEND), k::BEND_MAX_SEMIS + 1), 2);
        assert!((at(k::P_VOL) * k::VOL_MAX - 1.0).abs() < 1e-6);
        assert!((k::bipolar(at(k::P_TRANSPOSE), k::TRANSPOSE_SEMIS)).abs() < 1e-4);
        assert!((k::bipolar(at(k::P_FINE), k::FINE_CENTS)).abs() < 1e-3);
    }

    /// **The sampler's file is NAMED in its state, never carried in it.**
    /// Three extra symbols, round-tripping through the same blob the knobs
    /// use — and a blob from a build that had never heard of them still
    /// loads, because that is what the symbol space is for. The failure this
    /// prevents is the one the IR unit met: a bay that reopens pointing at
    /// nothing, silently, with the project claiming it is fine.
    #[test]
    fn the_samplers_file_survives_its_own_state_blob() {
        let spec = spec_for(SAMPLER_PATH).unwrap();
        assert!(is_extra_symbol(SAMPLER_PATH, "sample_path"));
        assert!(is_extra_symbol(SAMPLER_PATH, "sample_name"));
        assert!(is_extra_symbol(SAMPLER_PATH, "root_key"));
        assert!(!is_extra_symbol(SAMPLER_PATH, "ir_path"));
        assert!(!is_extra_symbol(ANALOG_PATH, "sample_path"));
        let mut values = spec.defaults();
        values[4] = 0.42;
        // A path with spaces and an equals sign, because a sample library
        // folder is exactly where both live.
        let extra = vec![
            ("sample_path".to_owned(), "/Samples/My Kit/kick=hard.wav".to_owned()),
            ("sample_name".to_owned(), "kick=hard.wav".to_owned()),
            ("root_key".to_owned(), "36".to_owned()),
        ];
        let blob = state_text(spec, &values, &extra);
        assert_eq!(values_from_state(spec, blob.as_bytes()).expect("parses"), values);
        assert_eq!(extra_from_state(spec, blob.as_bytes()), extra);
    }

    /// **A seated sampler plays the file it is handed, and is silent
    /// without one.** The whole hand-off in one test: the bay is built from
    /// the table, fed a note through the VST3-shaped road every voice
    /// travels, and handed the lane's sample as a borrow. The empty half
    /// matters as much — a bay whose file has not landed yet must make no
    /// sound rather than whatever was in the buffers.
    #[test]
    fn a_seated_sampler_plays_what_it_is_handed_and_nothing_otherwise() {
        let spec = spec_for(SAMPLER_PATH).unwrap();
        let mut unit = spec.create(48_000.0, &spec.defaults());
        let notes = [tangent_host::Note { offset: 0, pitch: 60, velocity: 1.0, on: true }];
        let sample = test_sample(24_000, 48_000);
        let mut bufs = vec![vec![0.0f32; 512]; 2];

        // With nothing loaded: a note, and silence.
        unit.process_voice(
            &notes,
            &[],
            &[],
            crate::analog::arp::Transport::default(),
            None,
            512,
            &mut bufs,
        );
        let quiet = bufs[0].iter().fold(0.0f32, |a, v| a.max(v.abs()));
        assert_eq!(quiet, 0.0, "an empty sampler bay made {quiet} of sound");

        // And with the file: the same note, heard, on both channels.
        unit.process_voice(
            &[],
            &[],
            &[],
            crate::analog::arp::Transport::default(),
            Some(&sample),
            512,
            &mut bufs,
        );
        let peak = bufs[0].iter().fold(0.0f32, |a, v| a.max(v.abs()));
        assert!(peak > 0.05, "the loaded sampler only reached {peak}");
        assert_eq!(bufs[0], bufs[1], "the file is mono-summed but the bay is not");

        // An effect's road answers silence rather than passing audio
        // through — a voice that leaked its input would double the channel.
        let input = vec![vec![0.7f32; 512]; 2];
        let mut out = vec![vec![0.0f32; 512]; 2];
        unit.process(&input, 512, &mut out);
        assert!(out.iter().all(|c| c.iter().all(|s| *s == 0.0)));
    }

    /// **A block longer than the scratch is the same audio as the same
    /// block in pieces.** The pass loop re-bases its events, and an event
    /// left at its original offset would fire in every pass — a note
    /// retriggering four times inside one offline block, which is what an
    /// export sounds like when this is wrong.
    #[test]
    fn a_long_block_is_the_same_audio_as_the_short_blocks_that_make_it() {
        let spec = spec_for(SAMPLER_PATH).unwrap();
        let sample = test_sample(48_000, 48_000);
        let long = {
            let mut unit = spec.create(48_000.0, &spec.defaults());
            let mut bufs = vec![vec![0.0f32; 2_048]; 2];
            let notes = [tangent_host::Note { offset: 700, pitch: 67, velocity: 0.8, on: true }];
            unit.process_voice(
                &notes,
                &[],
                &[],
                crate::analog::arp::Transport::default(),
                Some(&sample),
                2_048,
                &mut bufs,
            );
            bufs[0].clone()
        };
        let short = {
            let mut unit = spec.create(48_000.0, &spec.defaults());
            let mut out = Vec::new();
            let mut bufs = vec![vec![0.0f32; 512]; 2];
            for block in 0..4 {
                let notes = [tangent_host::Note {
                    offset: 700 - block * 512,
                    pitch: 67,
                    velocity: 0.8,
                    on: true,
                }];
                let here: &[tangent_host::Note] = if block == 1 { &notes } else { &[] };
                unit.process_voice(
                    here,
                    &[],
                    &[],
                    crate::analog::arp::Transport::default(),
                    Some(&sample),
                    512,
                    &mut bufs,
                );
                out.extend_from_slice(&bufs[0]);
            }
            out
        };
        assert_eq!(long.len(), short.len());
        for (i, (a, b)) in long.iter().zip(&short).enumerate() {
            assert!(
                (a - b).abs() < 1e-6,
                "frame {i} differs: {a} in one pass, {b} in four"
            );
        }
    }

    /// **Tangent Bitcrush reads in its own units and offers its switch's
    /// words**: eight bits and about 12 kHz at rest, the wire's words at the
    /// tops of BITS and RATE, unity at noon on both levels, and DITHER a
    /// two-position row whose reading is one of its positions.
    #[test]
    fn the_bitcrusher_reads_in_bits_hertz_and_decibels() {
        let spec = spec_for(BITCRUSH_PATH).expect("the bitcrusher is registered");
        assert_eq!(spec.class, "Tangent Bitcrush");
        assert!(!spec.voice);
        let rest = spec.defaults();
        assert_eq!(describe(BITCRUSH_PATH, 0, rest[0], &rest), "8.0 bit");
        assert_eq!(describe(BITCRUSH_PATH, 0, 1.0, &rest), "24.0 bit");
        assert_eq!(describe(BITCRUSH_PATH, 0, 0.0, &rest), "1.0 bit");
        assert_eq!(describe(BITCRUSH_PATH, 1, rest[1], &rest), "12.2 kHz");
        assert_eq!(describe(BITCRUSH_PATH, 1, 0.0, &rest), "200 Hz");
        assert_eq!(describe(BITCRUSH_PATH, 1, 1.0, &rest), "full");
        assert_eq!(describe(BITCRUSH_PATH, 2, 0.5, &rest), "+0.0 dB");
        assert_eq!(describe(BITCRUSH_PATH, 2, 1.0, &rest), "+24.0 dB");
        assert_eq!(describe(BITCRUSH_PATH, 4, 0.0, &rest), "-24.0 dB");
        assert_eq!(describe(BITCRUSH_PATH, 3, 1.0, &rest), tangent_ui::face::fmt_pct(1.0));
        assert_eq!(choices(BITCRUSH_PATH, 5), vec!["off", "on"]);
        assert_eq!(describe(BITCRUSH_PATH, 5, 0.0, &rest), "off");
        assert_eq!(describe(BITCRUSH_PATH, 5, 1.0, &rest), "on");
        assert_eq!(choices(BITCRUSH_PATH, 6), vec!["off", "on"]);
        assert_eq!(describe(BITCRUSH_PATH, 6, rest[6], &rest), "off");
        for id in 0..5 {
            assert!(choices(BITCRUSH_PATH, id).is_empty(), "dial {id} grew steps");
        }
        // Typed words land where the reading says them: "full" is the top,
        // and a typed depth finds its own position.
        assert_eq!(value_for_reading(BITCRUSH_PATH, 1, "full", &rest), Some(1.0));
        let v = value_for_reading(BITCRUSH_PATH, 0, "12", &rest).expect("a depth");
        assert_eq!(describe(BITCRUSH_PATH, 0, v, &rest), "12.0 bit");
    }

    /// **The face's picture and the kernel are one map.** `faces_bitcrush`
    /// restates the sweeps because the UI crate cannot see this one; a stair
    /// drawn on a different map would be a picture of a sound the unit does
    /// not make. The sentinel is pinned the same way.
    #[test]
    fn the_bitcrushers_face_draws_the_kernels_own_maps() {
        use crate::units::bitcrush as k;
        use tangent_ui::faces_bitcrush as f;
        assert_eq!(f::BITCRUSH_PATH, BITCRUSH_PATH);
        assert_eq!(f::BITS_RANGE, k::BITS_RANGE);
        assert_eq!(f::RATE_HZ, k::RATE_HZ);
        assert_eq!(f::DRIVE_DB, k::DRIVE_DB);
        assert_eq!(f::AA_CUTOFF, k::AA_CUTOFF);
        // The stair at the shipped rest, held against the kernel crushing
        // the same tone at the same rate.
        let (clean, stair) = f::staircase(k::BITS_DEFAULT, k::RATE_DEFAULT, 0.5, false);
        let mut kernel = k::Bitcrush::new(48_000.0);
        let mut buf = clean.clone();
        let n = buf.len();
        kernel.process(&mut buf, n, 1, &k::BitcrushParams::default());
        for (i, (a, b)) in stair.iter().zip(&buf).enumerate() {
            assert!((a - b).abs() < 1.0e-6, "sample {i}: the face draws {a}, the kernel plays {b}");
        }
    }

    /// **The tape's panel has a place for every control this table
    /// declares, and the ids it restates are this table's.** `faces_tape`
    /// spells the ids because the UI crate cannot see this one, and its own
    /// tests draw synthetic names, so a swapped pair (Age and Damage,
    /// Flutter and Rate, Speed and Wow Range) passes every one of them and
    /// lays a section out in the wrong order. So each restated id is held
    /// to its symbol here, whenever its row is declared, which makes the pin
    /// hold at every prefix of the table as the kernel appends to it. Every
    /// declared row has exactly one place in the four sections, under the
    /// ceiling the shared sweeps number up to, and is stepped exactly when
    /// the panel draws it as a selector. A fourteenth control fails here by
    /// name, so its place on the panel is decided rather than left to the
    /// tail the panel draws for whatever it has no place for.
    #[test]
    fn the_tape_face_has_a_place_for_every_control_the_engine_declares() {
        use tangent_ui::faces_tape as f;
        assert_eq!(f::TAPE_PATH, TAPE_PATH);
        for p in TAPE_FACE_PARAMS {
            let places = f::SECTIONS.iter().filter(|(_, ids)| ids.contains(&p.id)).count();
            assert_eq!(places, 1, "{} (id {}) has {places} places on the panel", p.symbol, p.id);
            assert!(
                (p.id as usize) < f::ID_CEILING,
                "{} (id {}) is past the panel's id ceiling of {}",
                p.symbol,
                p.id,
                f::ID_CEILING
            );
            assert_eq!(
                p.steps >= 2,
                f::STEPPED.contains(&p.id),
                "{} (id {}) has {} steps and the panel {} it as a selector",
                p.symbol,
                p.id,
                p.steps,
                if f::STEPPED.contains(&p.id) { "draws" } else { "does not draw" }
            );
        }
        let restated = [
            (f::P_DRIVE, "drive"),
            (f::P_BUMP, "bump"),
            (f::P_ROLL, "roll"),
            (f::P_WOBBLE, "wobble"),
            (f::P_HISS, "hiss"),
            (f::P_TRIM, "trim"),
            (f::P_SPEED, "speed"),
            (f::P_FLUTTER, "flutter"),
            (f::P_RATE, "rate"),
            (f::P_WOW_RANGE, "wow_range"),
            (f::P_BIAS, "bias"),
            (f::P_AGE, "age"),
            (f::P_DAMAGE, "damage"),
        ];
        assert_eq!(restated.len(), f::ID_CEILING, "the ceiling is not one past the panel's last id");
        for (id, symbol) in restated {
            if let Some(p) = TAPE_FACE_PARAMS.get(id as usize) {
                assert_eq!(p.id, id, "row {id} of the table carries id {}", p.id);
                assert_eq!(
                    p.symbol, symbol,
                    "the panel places id {id} as {symbol}, and the engine's id {id} is {}",
                    p.symbol
                );
            }
        }
    }

    /// **Seated, the bitcrusher's wire is still a wire and a mono lane is
    /// still crushed.** Through the planar road at 24 bits, full rate and
    /// unity, the awkward values come back to the bit; at its shipped rest
    /// a ONE-channel block comes back changed, because a bitcrusher on a
    /// mono lane that passed it untouched would be a knob that does
    /// nothing; and a state blob round-trips every knob it was given.
    #[test]
    fn the_seated_bitcrusher_is_a_wire_at_the_top_and_crushes_mono() {
        let spec = spec_for(BITCRUSH_PATH).unwrap();
        let frames = 2_048;
        let input = tricky_planar(frames);
        // The anti-alias switch ON: at "full" it is out of circuit.
        let wire = [1.0, 1.0, 0.5, 1.0, 0.5, 0.0, 1.0];
        let mut unit = spec.create(48_000.0, &wire);
        let out = run(&mut unit, &input, frames);
        for (c, (a, b)) in input.iter().zip(&out).enumerate() {
            for (f, (x, y)) in a.iter().zip(b).enumerate() {
                assert_eq!(x.to_bits(), y.to_bits(), "channel {c} frame {f}: {x:e} came out {y:e}");
            }
        }
        let mono = vec![(0..frames).map(|i| (i as f32 * 0.03).sin() * 0.5).collect::<Vec<f32>>()];
        let mut unit = spec.create(48_000.0, &spec.defaults());
        let out = run(&mut unit, &mono, frames);
        assert_ne!(out[0], mono[0], "a mono block went through the crusher untouched");
        assert!(out[0].iter().all(|s| s.is_finite()));
        // Every knob off its rest, through the blob and back.
        let moved = [0.6, 0.2, 0.7, 0.4, 0.3, 1.0, 1.0];
        let text = state_text(spec, &moved, &[]);
        assert_eq!(values_from_state(spec, text.as_bytes()).unwrap(), moved.to_vec());
        // A blob saved before the anti-alias switch existed has no `aa=`
        // line, and reads with it OFF: the folding it was saved with.
        let old = "tangentfx 1\nbits=0.6\nrate=0.2\ndrive=0.7\nmix=0.4\noutput=0.3\ndither=1\n";
        assert_eq!(
            values_from_state(spec, old.as_bytes()).unwrap(),
            vec![0.6, 0.2, 0.7, 0.4, 0.3, 1.0, 0.0]
        );
        // An automation point lands on the knob it names, and a stale id
        // moves nothing.
        let mut unit = spec.create(48_000.0, &spec.defaults());
        unit.apply(&[tangent_host::ParamPoint { id: 3, offset: 0, value: 0.0 }]);
        unit.apply(&[tangent_host::ParamPoint { id: 99, offset: 0, value: 0.0 }]);
        // A second of it: the glide is 20 ms and lands exactly.
        let long = tricky_planar(48_000);
        let out = run(&mut unit, &long, 48_000);
        // MIX automated to zero is the dry signal, once the glide lands.
        let tail = 48_000 - 64;
        assert_eq!(out[0][tail..], long[0][tail..], "a MIX lane at zero did not reach the dry");
    }
}
