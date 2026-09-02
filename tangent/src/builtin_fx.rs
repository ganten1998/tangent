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

/// The amp's sentinel. Same law as the four above — and this one the engine
/// also LOOKS FOR: a channel bay holding the amp is what arms that
/// channel's seven pedal racks (see `recorder::AMP_PEDAL_SEATS` and the
/// lane pass in `instrument.rs`).
pub const AMP_PATH: &str = "@tangent-amp";

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

fn build_trim(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Trim(Trim::new(sample_rate))
}

fn build_amp(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Amp(AmpUnit::new(sample_rate))
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
        path: AMP_PATH,
        class: "Tangent Amp",
        params: AMP_FACE_PARAMS,
        build: build_amp,
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
        (AMP_PATH, id) => match id {
            // The switches say what they are.
            0 | 5 => (if v >= 0.5 { "on" } else { "off" }).to_owned(),
            // Speed is a real frequency, from the opto tremolo's measured
            // range; the community's 2.5–10 Hz, swept like every Hz knob.
            6 => {
                use crate::units::trem::TREM_SPEED_HZ;
                fmt_hz(sweep(TREM_SPEED_HZ))
            }
            7 | 8 => fmt_pct(v),
            // Volume and the tone stack read as panel numbers, 0–10: the
            // hardware's skirted dials are numbered, not calibrated, and a
            // fake decibel would claim a precision the pot never had.
            _ => format!("{:.1}", v * 10.0),
        },
        // The tape speaks in percent on purpose: its knobs are character,
        // not calibration, and a fake decibel is worse than an honest one.
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
        (AMP_PATH, 0 | 5) => vec!["off".to_owned(), "on".to_owned()],
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
}

/// Write a unit's values as state text — the blob the insert-state sidecar
/// carries between the owner string and the end of the file.
///
/// Keyed by SYMBOL, not position, for the reason `lv2::state_from_params`
/// gives: a unit that grows a parameter must still read its old sessions, and
/// a positional format moves every value one knob to the left the day that
/// happens.
pub fn state_text(spec: &UnitSpec, values: &[f32]) -> String {
    let mut out = String::from("tangentfx 1\n");
    for (p, v) in spec.params.iter().zip(values) {
        out.push_str(p.symbol);
        out.push('=');
        out.push_str(&v.to_string());
        out.push('\n');
    }
    out
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
    /// The amp is also what ARMS a channel's pedal racks — the engine's
    /// lane pass matches on this variant, which is the one honest "is the
    /// amp in this bay" question the audio thread can ask without a string
    /// compare or a second ledger.
    Amp(AmpUnit),
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
            // Filters and a static table: nothing reads ahead.
            BuiltinUnit::Amp(_) => 0,
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
            BuiltinUnit::Amp(u) => u.set_param(id, value),
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
            BuiltinUnit::Amp(u) => u.process(input, frames, out),
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
            BuiltinUnit::Amp(u) => u.kernel.clear(),
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
        let text = state_text(spec, &[0.75]);
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
        // The amp: kernel defaults for every dial; the VIB switch is the
        // wrapper's own and ships off, which IS the kernel's depth-0 rest.
        let a = crate::units::amp::AmpParams::default();
        for (spec, want) in AMP_FACE_PARAMS.iter().zip([
            a.bright, a.volume, a.treble, a.middle, a.bass, 0.0, a.vib_speed, a.vib_depth,
            a.vib_stereo,
        ]) {
            assert_eq!(spec.default, want, "{} drifted from AmpParams::default", spec.symbol);
        }
        assert_eq!(a.vib_depth, 0.0, "the kernel no longer ships with the vibrato at rest");
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
        // drive 0.5 is the calibrated machine, trim 0.5 unity, every colour
        // off — the kernel's own neutral recipe.
        let mut unit = spec.create(sr, &[0.5, 0.0, 0.0, 0.0, 0.0, 0.5]);
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
            let text = state_text(spec, &spec.defaults());
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
            }
        }
    }
}
