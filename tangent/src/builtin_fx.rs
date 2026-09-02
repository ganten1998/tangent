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

/// One parameter a unit publishes: what the automation lane, the state blob
/// and (one day) a knob panel all address it by.
///
/// `id` and `symbol` are both persisted — the id into automation curves as
/// `fx:<bay>:<id>`, the symbol into the state sidecar — so both are
/// append-only forever, exactly as `AutoParam` documents for its own space.
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
}

const TRIM_PARAMS: &[ParamSpec] = &[ParamSpec {
    id: 0,
    symbol: "gain",
    name: "Gain",
    units: "dB",
    default: 0.5,
    steps: 0,
}];

/// The EQ face's slice of the kernel: three bands of the eight
/// `units::eq::EqKernel` carries, five knobs each — on, shape, frequency,
/// gain, width. Ids run dense from 0 in band-major order, which is what lets
/// `EqUnit::set_param` decode them by division instead of a lookup; both
/// spaces are persisted, so bands four to eight, when a bigger face wants
/// them, take ids 15 upward and never renumber these.
///
/// Defaults are the kernel's shipped sound — every band OFF, gain centred,
/// width at Butterworth — except the frequency parking spots: the kernel's
/// own default spreads eight handles log-evenly as a ruler, and the first
/// three rungs of an eight-rung ruler are all in the bass. Three handles get
/// the three-handle version of the same idea: low, mid, high, at the quarter
/// points of the sweep (about 112 Hz, 632 Hz and 3.6 kHz).
const EQ_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "b1_on", name: "Band 1 On", units: "", default: 0.0, steps: 2 },
    ParamSpec { id: 1, symbol: "b1_kind", name: "Band 1 Type", units: "", default: 0.0, steps: EQ_KIND_STEPS },
    ParamSpec { id: 2, symbol: "b1_freq", name: "Band 1 Freq", units: "Hz", default: 0.25, steps: 0 },
    ParamSpec { id: 3, symbol: "b1_gain", name: "Band 1 Gain", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 4, symbol: "b1_q", name: "Band 1 Q", units: "", default: crate::units::eq::EQ_Q_BUTTERWORTH, steps: 0 },
    ParamSpec { id: 5, symbol: "b2_on", name: "Band 2 On", units: "", default: 0.0, steps: 2 },
    ParamSpec { id: 6, symbol: "b2_kind", name: "Band 2 Type", units: "", default: 0.0, steps: EQ_KIND_STEPS },
    ParamSpec { id: 7, symbol: "b2_freq", name: "Band 2 Freq", units: "Hz", default: 0.5, steps: 0 },
    ParamSpec { id: 8, symbol: "b2_gain", name: "Band 2 Gain", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 9, symbol: "b2_q", name: "Band 2 Q", units: "", default: crate::units::eq::EQ_Q_BUTTERWORTH, steps: 0 },
    ParamSpec { id: 10, symbol: "b3_on", name: "Band 3 On", units: "", default: 0.0, steps: 2 },
    ParamSpec { id: 11, symbol: "b3_kind", name: "Band 3 Type", units: "", default: 0.0, steps: EQ_KIND_STEPS },
    ParamSpec { id: 12, symbol: "b3_freq", name: "Band 3 Freq", units: "Hz", default: 0.75, steps: 0 },
    ParamSpec { id: 13, symbol: "b3_gain", name: "Band 3 Gain", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 14, symbol: "b3_q", name: "Band 3 Q", units: "", default: crate::units::eq::EQ_Q_BUTTERWORTH, steps: 0 },
];

/// How many positions the band-shape menu has. Spelled as a literal so the
/// table above stays `const`; a test pins it to `NodeKind::ALL.len()`, the
/// enum it steps through.
const EQ_KIND_STEPS: i32 = 6;

/// How many of the kernel's bands the face exposes, and how many knobs each
/// band takes in the id space.
const EQ_FACE_NODES: usize = 3;
const EQ_KNOBS_PER_NODE: u32 = 5;

/// One knob per `units::tape::TapeParams` field, ids in declaration order.
/// Defaults are `TapeParams::default()` verbatim — the shipped 388 at a
/// healthy record level — and a test holds the two tables together.
const TAPE_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "drive", name: "Drive", units: "%", default: 0.6, steps: 0 },
    ParamSpec { id: 1, symbol: "bump", name: "Bump", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 2, symbol: "roll", name: "Roll", units: "Hz", default: 0.12, steps: 0 },
    ParamSpec { id: 3, symbol: "wobble", name: "Wobble", units: "%", default: 0.3, steps: 0 },
    ParamSpec { id: 4, symbol: "hiss", name: "Hiss", units: "%", default: 0.25, steps: 0 },
    ParamSpec { id: 5, symbol: "trim", name: "Trim", units: "dB", default: 0.5, steps: 0 },
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
const DENOISE_FACE_PARAMS: &[ParamSpec] = &[
    ParamSpec { id: 0, symbol: "learn", name: "Learn", units: "", default: 0.0, steps: 2 },
    ParamSpec { id: 1, symbol: "reduce", name: "Reduce", units: "dB", default: 0.5, steps: 0 },
    ParamSpec { id: 2, symbol: "smooth", name: "Smooth", units: ":1", default: 0.5, steps: 0 },
    ParamSpec { id: 3, symbol: "release", name: "Release", units: "ms", default: 0.5, steps: 0 },
    ParamSpec { id: 4, symbol: "sc_hpf", name: "Detector HPF", units: "Hz", default: 0.0, steps: 0 },
    ParamSpec { id: 5, symbol: "lookahead", name: "Lookahead", units: "", default: 1.0, steps: 2 },
];

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

/// Every effect this build ships. Order is nothing; the path is the key.
pub const UNITS: &[UnitSpec] = &[
    UnitSpec {
        path: TRIM_PATH,
        class: "Tangent Trim",
        params: TRIM_PARAMS,
        build: build_trim,
    },
    UnitSpec {
        path: EQ_PATH,
        class: "Tangent EQ",
        params: EQ_FACE_PARAMS,
        build: build_eq,
    },
    UnitSpec {
        path: IMAGER_PATH,
        class: "Tangent Imager",
        params: IMAGER_FACE_PARAMS,
        build: build_imager,
    },
    UnitSpec {
        path: TAPE_PATH,
        class: "Tangent Tape",
        params: TAPE_FACE_PARAMS,
        build: build_tape,
    },
    UnitSpec {
        path: COMP_PATH,
        class: "Tangent Comp",
        params: COMP_FACE_PARAMS,
        build: build_comp,
    },
    UnitSpec {
        path: DENOISE_PATH,
        class: "Tangent Denoise",
        params: DENOISE_FACE_PARAMS,
        build: build_denoise,
    },
    UnitSpec {
        path: AMP_PATH,
        class: "Tangent Amp",
        params: AMP_FACE_PARAMS,
        build: build_amp,
    },
    UnitSpec {
        path: DRIVE_PATH,
        class: "Tangent Drive",
        params: DRIVE_FACE_PARAMS,
        build: build_drive,
    },
    UnitSpec {
        path: TREM_PATH,
        class: "Tangent Trem",
        params: TREM_FACE_PARAMS,
        build: build_trem,
    },
    UnitSpec {
        path: CHORUS_PATH,
        class: "Tangent Chorus",
        params: CHORUS_FACE_PARAMS,
        build: build_chorus,
    },
    UnitSpec {
        path: ECHO_PATH,
        class: "Tangent Echo",
        params: ECHO_FACE_PARAMS,
        build: build_echo,
    },
    UnitSpec {
        path: PLATE_PATH,
        class: "Tangent Plate",
        params: PLATE_FACE_PARAMS,
        build: build_plate,
    },
    UnitSpec {
        path: WAH_PATH,
        class: "Tangent Wah",
        params: WAH_FACE_PARAMS,
        build: build_wah,
    },
    UnitSpec {
        path: IR_PATH,
        class: "Tangent IR",
        params: IR_FACE_PARAMS,
        build: build_ir,
    },
];

/// The unit a sentinel names, or `None` for a sentinel this build does not
/// know — which the loader reports rather than swallows, because a session
/// from a NEWER build naming a unit this one lacks deserves a sentence, not a
/// silently empty bay.
/// The value as the face should PRINT it — the kernel's own mapping
/// constants turned into the band's reading formats, in one place, so the
/// panel and the sound can never quote two different numbers for one knob.
/// The formatters are the face kit's; this crate links the UI crate, so
/// reuse is free and the firewall is untouched (data flows outward only).
pub fn describe(path: &str, id: u32, value: f32) -> String {
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
                2 => format!("{:.1}:1", sweep(DENOISE_RATIO)),
                3 => fmt_ms(sweep(DENOISE_RELEASE_MS)),
                4 => {
                    if v <= 0.0 {
                        "off".to_owned()
                    } else {
                        fmt_hz(sweep(DENOISE_SC_HPF_HZ))
                    }
                }
                _ => {
                    if v >= 0.5 {
                        fmt_ms(DENOISE_LOOKAHEAD_MS)
                    } else {
                        "off".to_owned()
                    }
                }
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
        // **The three tape dials that have an honest number.** Their
        // `ParamSpec`s promise dB, Hz and dB, and the automation lane prints
        // that promise beside the curve; the face used to fall through to
        // percent, so the lane said "dB" and the panel said "50 %" about the
        // same knob. Which of the two was wrong is settled by the machine:
        // the head bump is +2.5 dB at 70 Hz on the service data, the repro
        // corner is a frequency, and a trim is decibels. The other three —
        // drive, wobble, hiss — stay percent below, and their specs say so.
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
        // The rest of the tape speaks in percent on purpose: those knobs are
        // character, not calibration, and a fake decibel is worse than an
        // honest one. Every dial not named above — drive amount, depths,
        // widths, mixes, the wah's damping — is percent for the same reason.
        _ => fmt_pct(v),
    }
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
        (DENOISE_PATH, 5) | (AMP_PATH, 0) => {
            vec!["off".to_owned(), "on".to_owned()]
        }
        // One diode against one, or against two — what the toggle does to
        // the harmonics, in the clipper's own terms.
        (DRIVE_PATH, 3) => vec!["matched".to_owned(), "uneven".to_owned()],
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
        DENOISE_PATH => symbol
            .strip_prefix('t')
            .and_then(|n| n.parse::<usize>().ok())
            // "t07" must NOT alias t7 — one value per symbol, one spelling.
            .is_some_and(|n| n < crate::units::denoise::DENOISE_BANDS && symbol == format!("t{n}")),
        IR_PATH => symbol == "ir_path",
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

/// Read a state blob back into a full value table.
///
/// Forgiving about lines and strict about the container, on the LV2 model: a
/// symbol this build does not know is skipped (a newer build's parameter), a
/// symbol the blob lacks reads as its default (an older build's file), a value
/// that does not parse or is not finite reads as its default — but a blob that
/// is not this format at all is an `Err`, because it is another plugin's state
/// or a damaged file, and the user should hear that their settings did not
/// come back rather than get defaults dressed as a restore.
pub fn values_from_state(spec: &UnitSpec, bytes: &[u8]) -> Result<Vec<f32>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "the stored block is not text".to_owned())?;
    let mut lines = text.lines();
    if lines.next() != Some("tangentfx 1") {
        return Err("the stored block is not a built-in effect's state".to_owned());
    }
    let mut values = spec.defaults();
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
            // The play head sits a couple of milliseconds behind the record
            // head (`units::tape::BASE_MS`), but the figure is deliberately
            // unreported this pass: the head also WANDERS around that point,
            // so the number is nominal, and the suite ships every unit at
            // zero. The road for the honest figure already exists —
            // `Backend::reported_latency` reads this every block — so
            // reporting it later is one arm here and no new plumbing.
            BuiltinUnit::Tape(_) => 0,
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
                let mut thresholds = [0.0f32; DENOISE_BANDS];
                let mut any = false;
                for (symbol, value) in extra {
                    let Some(k) = symbol
                        .strip_prefix('t')
                        .and_then(|n| n.parse::<usize>().ok())
                        .filter(|k| *k < DENOISE_BANDS)
                    else {
                        continue;
                    };
                    if let Ok(v) = value.parse::<f32>() {
                        thresholds[k] = v;
                        any = true;
                    }
                }
                // Nothing learned in the blob leaves the fresh all-off state
                // alone; `set_thresholds` itself washes non-finite to off.
                if any {
                    u.kernel.set_thresholds(&thresholds);
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
                    Err(e) => Some(format!("its impulse response did not load — {e}")),
                }
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
        }
    }

    /// The denoise's learned thresholds, when this block may have moved them
    /// — `Some` while the learn dial is up and for ONE block after it falls,
    /// which is the block the kernel froze them on. The engine forwards the
    /// snapshot into a shared cell the host adopts into its ledger; every
    /// other unit answers `None`, the `gain_reduction_db` shape exactly.
    pub fn learn_snapshot(&mut self) -> Option<[f32; crate::units::denoise::DENOISE_BANDS]> {
        match self {
            BuiltinUnit::Denoise(u) => u.publish.then(|| u.kernel.thresholds()),
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

    /// Drop whatever the unit remembers of the signal, the way every unit in
    /// `effects.rs` can. The trim remembers only where its slew is, and
    /// clearing that snaps the gain to its target.
    pub fn clear(&mut self) {
        match self {
            BuiltinUnit::Trim(t) => t.clear(),
            BuiltinUnit::Eq(u) => u.kernel.clear(),
            BuiltinUnit::Imager(u) => u.kernel.clear(),
            BuiltinUnit::Tape(u) => u.kernel.clear(),
            BuiltinUnit::Comp(u) => u.kernel.clear(),
            // The denoise's clear keeps what was LEARNED — its kernel
            // documents "forget what was playing, keep the thresholds",
            // which is exactly what a restored preset needs from the
            // `create`-then-`clear` load path.
            BuiltinUnit::Denoise(u) => u.kernel.clear(),
            BuiltinUnit::Amp(u) => u.kernel.clear(),
            BuiltinUnit::Drive(u) => u.kernel.clear(),
            BuiltinUnit::Trem(u) => u.kernel.clear(),
            BuiltinUnit::Chorus(u) => u.kernel.clear(),
            BuiltinUnit::Echo(u) => u.kernel.clear(),
            BuiltinUnit::Plate(u) => u.kernel.clear(),
            BuiltinUnit::Wah(u) => u.kernel.clear(),
            // And the convolver's clear keeps the IR itself: tails go, the
            // loaded filter stays, the same knowledge-vs-signal split.
            BuiltinUnit::Ir(u) => u.kernel.clear(),
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

pub struct TapeUnit {
    kernel: crate::units::tape::Tape,
    params: crate::units::tape::TapeParams,
    scratch: Vec<f32>,
}

impl TapeUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            kernel: crate::units::tape::Tape::new(sample_rate),
            params: crate::units::tape::TapeParams::default(),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(spec) = TAPE_FACE_PARAMS.get(id as usize) else {
            return;
        };
        let v = washed(spec, value);
        match id {
            0 => self.params.drive = v,
            1 => self.params.bump = v,
            2 => self.params.roll = v,
            3 => self.params.wobble = v,
            4 => self.params.hiss = v,
            _ => self.params.trim = v,
        }
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let (kernel, params) = (&mut self.kernel, &self.params);
        run_planar(&mut self.scratch, input, frames, out, |buf, n| {
            kernel.process(buf, n, 2, params);
        });
    }
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
pub struct DenoiseUnit {
    kernel: crate::units::denoise::Denoise,
    params: crate::units::denoise::DenoiseParams,
    scratch: Vec<f32>,
    /// Whether the LAST processed block ran with the learn dial up.
    last_learn: bool,
    /// Whether this block's thresholds are worth publishing — learn up, or
    /// the falling-edge block that froze them.
    publish: bool,
}

impl DenoiseUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            kernel: crate::units::denoise::Denoise::new(sample_rate),
            params: crate::units::denoise::DenoiseParams::default(),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
            last_learn: false,
            publish: false,
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
            _ => self.params.lookahead = v,
        }
    }

    fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let learn_now = self.params.learn >= 0.5;
        let (kernel, params) = (&mut self.kernel, &self.params);
        run_planar(&mut self.scratch, input, frames, out, |buf, n| {
            kernel.process(buf, n, 2, params);
        });
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
}

impl IrUnit {
    fn new(sample_rate: f32) -> Self {
        Self {
            kernel: crate::units::convolver::Convolver::new(sample_rate),
            params: crate::units::convolver::ConvolverParams::default(),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
            sample_rate,
            has_ir: false,
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
                 {engine} Hz — loaded as-is, so it plays shifted",
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
        for (spec, want) in TAPE_FACE_PARAMS
            .iter()
            .zip([t.drive, t.bump, t.roll, t.wobble, t.hiss, t.trim])
        {
            assert_eq!(spec.default, want, "{} drifted from TapeParams::default", spec.symbol);
        }
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
        // which are the documented three-handle spread (see EQ_FACE_PARAMS).
        let n = crate::units::eq::NodeParams::default();
        assert!(!n.on, "the kernel ships FLAT; the face must too");
        for (band, chunk) in EQ_FACE_PARAMS.chunks(EQ_KNOBS_PER_NODE as usize).enumerate() {
            assert_eq!(chunk[0].default, 0.0, "band {band} does not open off");
            assert_eq!(chunk[1].default, 0.0, "band {band} does not open a bell");
            assert_eq!(chunk[2].default, [0.25, 0.5, 0.75][band]);
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
            d.learn, d.reduce, d.smooth, d.release, d.sc_hpf, d.lookahead,
        ]) {
            assert_eq!(spec.default, want, "{} drifted from DenoiseParams::default", spec.symbol);
        }
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
        // Drive at the bottom, trim 0.5 unity, every colour off — the
        // kernel's own neutral recipe. The drive used to read 0.5 here and
        // was only neutral by accident; it is a record level, and a record
        // level's off is the end of its travel (`units::tape::DRIVE_TANH`).
        let mut unit = spec.create(sr, &[0.0, 0.0, 0.0, 0.0, 0.0, 0.5]);
        let mut out = vec![vec![0.0f32; frames]; 2];
        unit.process(&input, frames, &mut out);
        let rms = |v: &[f32]| {
            (v.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / v.len() as f64).sqrt()
        };
        // Steady state only: past the envelope settling and the line fill.
        let (lo, hi) = ((sr * 0.25) as usize, (sr * 1.15) as usize);
        let delta = 20.0 * (rms(&out[0][lo..hi]) / rms(&input[0][lo..hi])).log10();
        assert!(
            delta.abs() < 0.1,
            "neutral tape moved the broadband level by {delta:.3} dB through the adapter"
        );
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

    /// **Learn, freeze, persist, restore: the denoise's whole threshold
    /// story at the unit level.** The dial up is listening, the falling edge
    /// freezes, the snapshot is offered for exactly the freeze block, and a
    /// fresh instance fed the persisted lines opens with the same bank.
    #[test]
    fn the_denoise_learns_on_the_falling_edge_and_the_extra_state_restores_it() {
        let spec = spec_for(DENOISE_PATH).unwrap();
        let mut unit = spec.create(48_000.0, &spec.defaults());
        assert!(
            unit.learn_snapshot().is_none(),
            "an idle denoise offered a snapshot"
        );
        let frames = 512;
        let mut out = vec![vec![0.0f32; frames]; 2];
        let mut seed = 7u32;
        // Listening: ~160 ms of noise, far past the kernel's minimum pass.
        unit.set_param(0, 1.0);
        for _ in 0..15 {
            let input = noise_planar(frames, &mut seed);
            unit.process(&input, frames, &mut out);
        }
        // The falling edge: THIS block freezes, and the snapshot says so.
        unit.set_param(0, 0.0);
        let input = noise_planar(frames, &mut seed);
        unit.process(&input, frames, &mut out);
        let thr = unit
            .learn_snapshot()
            .expect("the freeze block must offer the thresholds");
        assert!(
            thr.iter().any(|t| *t > 0.05),
            "a broadband learn froze an all-off bank: {thr:?}"
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
            .collect();
        let mut restored = spec.create(48_000.0, &spec.defaults());
        assert!(restored.apply_extra_state(&extra).is_none());
        let BuiltinUnit::Denoise(r) = &restored else {
            panic!("the denoise path built something else");
        };
        for (k, (a, b)) in thr.iter().zip(r.kernel.thresholds()).enumerate() {
            assert!((a - b).abs() < 1.0e-6, "band {k}: froze {a}, restored {b}");
        }
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
        assert_eq!(describe(ECHO_PATH, 0, ECHO_TIME_DEFAULT), "375 ms");
        assert_eq!(describe(PLATE_PATH, 0, 0.34), "2.20 s");
        assert_eq!(describe(PLATE_PATH, 1, 0.0), "0.0 ms");
        assert_eq!(describe(WAH_PATH, 1, 0.5), "240 Hz");
        assert_eq!(describe(DENOISE_PATH, 0, 0.0), "Learn");
        assert_eq!(describe(DENOISE_PATH, 0, 1.0), "Listening");
        assert_eq!(describe(DENOISE_PATH, 1, 0.5), "-12.0 dB");
        assert_eq!(describe(DENOISE_PATH, 1, 0.0), "off");
        assert_eq!(describe(DENOISE_PATH, 5, 1.0), "3.0 ms");
        // The shipped level is unity to a tenth of a dB: 0.71² · 2 ≈ 1.008.
        assert_eq!(describe(DRIVE_PATH, 2, 0.71), "+0.1 dB");
        assert_eq!(describe(DRIVE_PATH, 3, 0.0), "matched");
        assert_eq!(describe(IR_PATH, 1, 0.5), "+0.0 dB");
        // The tape's three calibrated dials, at the positions it ships at:
        // the bump's own +2.5 dB, the repro corner just under 16 kHz, unity
        // trim. A face that printed "50 %" for any of these disagreed with
        // the automation lane's declared unit about the same knob.
        assert_eq!(describe(TAPE_PATH, 1, 0.5), "+2.5 dB");
        assert_eq!(describe(TAPE_PATH, 2, 0.12), "15.6 kHz");
        assert_eq!(describe(TAPE_PATH, 2, 0.0), "off");
        assert_eq!(describe(TAPE_PATH, 5, 0.5), "+0.0 dB");
        // And the three that are character rather than calibration keep
        // saying so.
        assert_eq!(describe(TAPE_PATH, 0, 0.6), fmt_pct(0.6));
        assert_eq!(describe(TAPE_PATH, 3, 0.3), fmt_pct(0.3));
        assert_eq!(describe(TAPE_PATH, 4, 0.25), fmt_pct(0.25));
        assert_eq!(choices(DENOISE_PATH, 0), vec!["Learn", "Listening"]);
        assert_eq!(choices(DRIVE_PATH, 3), vec!["matched", "uneven"]);
        assert_eq!(choices(AMP_PATH, 0), vec!["off", "on"]);
        assert!(choices(TREM_PATH, 0).is_empty(), "a continuous dial grew steps");
    }

    /// Two channels of independent noise: a record with a real image.
    fn stereo_program(frames: usize) -> Vec<Vec<f32>> {
        let mut a = 0x51DE_0001_u32;
        let mut b = 0x51DE_0002_u32;
        let mut step = |s: &mut u32| {
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
    /// 70 Hz and the right one for a drive is harmonic content.
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
        // And what it made is genuine side, not a level change: the mono
        // fold is untouched to the bit the kernel promises.
        for f in 0..FRAMES {
            let before = 0.5 * (program[0][f] + program[1][f]);
            let after = 0.5 * (widened[0][f] + widened[1][f]);
            assert!(
                (before - after).abs() < 1.0e-6,
                "the ensemble moved the mono fold at frame {f}: {before} -> {after}"
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
}
