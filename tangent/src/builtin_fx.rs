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

fn build_trim(sample_rate: f32) -> BuiltinUnit {
    BuiltinUnit::Trim(Trim::new(sample_rate))
}

/// Every effect this build ships. Order is nothing; the path is the key.
pub const UNITS: &[UnitSpec] = &[UnitSpec {
    path: TRIM_PATH,
    class: "Tangent Trim",
    params: TRIM_PARAMS,
    build: build_trim,
}];

/// The unit a sentinel names, or `None` for a sentinel this build does not
/// know — which the loader reports rather than swallows, because a session
/// from a NEWER build naming a unit this one lacks deserves a sentence, not a
/// silently empty bay.
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
}

impl BuiltinUnit {
    /// Frames this unit delays the signal by, reported once at load into the
    /// same per-bay cell every plugin's figure lands in.
    pub fn latency(&self) -> u32 {
        match self {
            // A trim looks at nothing ahead of the sample it is scaling.
            BuiltinUnit::Trim(_) => 0,
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
        }
    }

    /// Drop whatever the unit remembers of the signal, the way every unit in
    /// `effects.rs` can. The trim remembers only where its slew is, and
    /// clearing that snaps the gain to its target.
    pub fn clear(&mut self) {
        match self {
            BuiltinUnit::Trim(t) => t.clear(),
        }
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
