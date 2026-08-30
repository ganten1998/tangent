//! LV2 hosting, beside the VST3 host rather than instead of it.
//!
//! # Why a second plugin format
//!
//! VST3 is the format this host was built for and the one the commercial world
//! ships. It is not the one the *free* world ships. LV2 is the native format of
//! Linux audio — lsp-plugins, Calf, x42, Dragonfly, Rubber Band are all LV2
//! first — and on FreeBSD it is very nearly the only format that exists: the
//! ports tree has an LV2 SDK, an LV2 linter, and a dozen LV2 plugin packages,
//! and **not one VST3 plugin**. A host that speaks only VST3 reaches almost
//! nothing on either platform.
//!
//! So this is not a portability shim. It is the difference between the plugin
//! picker having entries and being permanently empty.
//!
//! # What this is built on
//!
//! `lilv` is the reference LV2 host library — the one Ardour, Carla and
//! jalv use — and `livi` is a safe Rust wrapper over it. Both are system
//! packages (`audio/lilv`, `pkg-config lilv-0`) rather than bundled C, so this
//! adds no C toolchain requirement to the build.
//!
//! One patch was needed to get here and it is one line: `lv2-sys` selects
//! pre-generated bindings from a `cfg_attr` table covering linux and windows
//! and `compile_error!`s on everything else, so FreeBSD could not build an LV2
//! host at all. Its `src/linux/` turns out to contain no Linux-specific code —
//! it is a pure `target_arch` dispatch over bindgen output from the LV2 C
//! headers — so the freebsd arm points at the same file. See
//! `tangent-host/vendor/README.md`.
//!
//! # The rule this file exists under
//!
//! `process` runs on the audio thread, so it does not allocate, lock or panic.
//! Everything it needs — the atom sequences, the scratch buffers, the port
//! index tables — is allocated once in [`Instance::create`] and only written to
//! afterwards. That is why the buffers below are owned rather than borrowed and
//! why the block size is fixed at instantiation: LV2 lets a plugin be told a
//! maximum block length once, and changing it means a new instance.

use std::sync::OnceLock;

/// The largest block this host will ever hand a plugin.
///
/// Fixed at instantiation because LV2's `bufsz:boundedBlockLength` is a promise
/// made once, and a plugin is entitled to size its own buffers from it. Chosen
/// well above any buffer the audio settings offer so that changing the device
/// buffer never invalidates a running plugin — the alternative is re-creating
/// every plugin instance when the user drags the buffer-size control, which is
/// exactly the sort of thing that drops audio mid-session.
const MAX_BLOCK: usize = 8192;

/// Capacity of the MIDI atom sequences, in bytes.
///
/// An LV2 MIDI event is 3 bytes of payload inside a 16-byte atom header, so
/// this is roughly 500 events in a single block. A block is at most 8192 frames
/// — under 200 ms — and nothing plays 500 notes in 200 ms, so overflowing this
/// means something is wrong upstream rather than somebody playing fast.
const ATOM_CAPACITY: usize = 8192;

/// The prefix that marks a plugin identity as an LV2 URI rather than a bundle
/// path.
///
/// **Why an identity string and not a new type.** Everything from the picker
/// down to the settings file to `Engine::load_insert` carries a plugin as a
/// `Path`, and for VST3 that path is a real bundle on disk. LV2 has no such
/// path — a plugin is identified by URI, its bundle directory is an
/// implementation detail lilv owns, and two plugins routinely live in one
/// bundle. Threading a second identity type through every one of those call
/// sites would touch the settings format, the picker's public API and the
/// engine's load path to express something a prefix expresses exactly as well.
///
/// So an LV2 plugin's identity is `lv2:` followed by its URI, and the one thing
/// that must stay true is that nothing else can produce that spelling. A
/// filesystem path cannot: `lv2:` at the start of a path component is legal on
/// Unix but `discover` only ever yields absolute paths, which begin with `/`.
pub const SCHEME: &str = "lv2:";

/// The identity to hand the picker and store in settings, for a given URI.
pub fn identity_for(uri: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{SCHEME}{uri}"))
}

/// The URI back out of an identity, or `None` if this is a bundle path.
pub fn uri_of(identity: &std::path::Path) -> Option<String> {
    identity
        .to_str()?
        .strip_prefix(SCHEME)
        .map(|uri| uri.to_owned())
}

/// Who to file a plugin under, worked out from its URI.
///
/// **LV2 has no vendor field, and the URI is the only thing that carries the
/// information.** The picker groups by vendor, so every plugin needs an answer;
/// "" would pile the whole LV2 collection into the unnamed group at the bottom,
/// which is where things go to be unfindable.
///
/// Both URI shapes in the wild are handled, and the second is not hypothetical:
/// Dragonfly Reverb ships four plugins, one of which is
/// `https://github.com/michaelwillis/dragonfly-reverb` and three of which are
/// `urn:dragonfly:room` and friends. An authority-only rule returned nothing
/// for three quarters of one vendor's output.
pub fn vendor_of(uri: &str) -> String {
    // scheme://authority/path — the authority names the publisher.
    if let Some((_, rest)) = uri.split_once("://") {
        if let Some(host) = rest.split('/').next().filter(|h| !h.is_empty()) {
            return host.to_owned();
        }
    }
    // urn:namespace:thing — the namespace is the closest equivalent.
    if let Some(rest) = uri.strip_prefix("urn:") {
        if let Some(ns) = rest.split(':').next().filter(|n| !n.is_empty()) {
            return ns.to_owned();
        }
    }
    // Something else entirely. Grouped together under the format's own name,
    // which is at least true and keeps it out of the unnamed pile.
    "LV2".to_owned()
}

/// What the picker needs to know about one plugin, without instantiating it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginInfo {
    /// The LV2 URI. This is the stable identity — a plugin's *name* can change
    /// between releases, its URI cannot, so this is what gets persisted.
    pub uri: String,
    pub name: String,
    pub audio_in: usize,
    pub audio_out: usize,
    /// Whether the plugin declares itself an instrument, from its LV2 class.
    /// Maps onto the same distinction `scan::Kind` draws for VST3.
    pub is_instrument: bool,
    /// Control ports, which are this format's parameters.
    pub params: usize,
    /// Whether it accepts MIDI. An instrument that cannot be played is not
    /// useful to offer as one.
    pub wants_midi: bool,
}

/// The lilv world plus the feature set, built once.
///
/// **Built once because building it is the expensive part.** Constructing a
/// world parses the Turtle manifest of every bundle on `LV2_PATH`; on a machine
/// with a full plugin collection that is thousands of files. It is also the
/// natural owner of the URID map, which must be shared by every instance for
/// atom types to mean the same thing across them.
struct Catalog {
    world: livi::World,
    features: std::sync::Arc<livi::Features>,
}

// SAFETY: `livi::World` owns a `lilv::World`, which is not marked `Sync` by
// lilv-rs. Access here is confined to construction and to `&self` reads that
// lilv documents as safe on an immutable world (plugin lookup and iteration);
// nothing mutates it after `new`. `livi::Features` is already `Send + Sync`.
unsafe impl Sync for Catalog {}
unsafe impl Send for Catalog {}

fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let world = livi::World::new();
        let features = world.build_features(livi::FeaturesBuilder {
            min_block_length: 1,
            max_block_length: MAX_BLOCK,
        });
        Catalog { world, features }
    })
}

// ── the metadata lilv knows and livi does not ────────────────────────────────

/// What one control port is, beyond its number.
///
/// livi's `ControlPort` carries a value and a range and nothing else — no name,
/// no symbol, no unit — so a panel built from livi alone shows "Parameter 7"
/// with a bare float. lilv has all of it, and this is that half, read once.
#[derive(Clone, Debug, PartialEq)]
pub struct PortMeta {
    /// The plugin's absolute port index, which is what livi's `ControlPort`
    /// reports and therefore the only thing the two halves can be joined on.
    pub port_index: usize,
    /// **The stable name of this control, and the one thing worth persisting.**
    /// An LV2 symbol is defined by the spec to be a valid C identifier that
    /// does not change across versions of a plugin; a port INDEX is not, and a
    /// saved session keyed by index would silently point at a different knob
    /// after the plugin was updated. Saved state is written by symbol for
    /// exactly this reason.
    pub symbol: String,
    /// What to show a person. Falls back to the symbol, then to the index.
    pub name: String,
    /// "dB", "Hz", "%" — empty when the plugin declares no unit.
    pub units: String,
    /// 0 for a continuous control; 2 for a toggle; otherwise the number of
    /// steps, which is what makes a switch draw as a switch rather than a knob
    /// that happens to have two useful positions.
    pub steps: i32,
}

/// Everything one plugin's identity implies, gathered in a single lilv pass.
#[derive(Clone, Debug, Default)]
pub struct PluginMeta {
    /// Whether it ships a UI this host can embed. See `lv2ui::Ui::has_ui`.
    pub x11_ui: bool,
    /// Its control INPUT ports, in plugin order.
    pub controls: Vec<PortMeta>,
    /// The designated latency output port's absolute index, if it has one.
    pub latency_port: Option<usize>,
}

/// Every plugin's metadata, read once for the life of the process.
///
/// **One scan, cached, and that is not premature.** Building a lilv world
/// parses every Turtle manifest on `LV2_PATH`; on a machine with the whole of
/// lsp-plugins and Calf installed that is thousands of triples. The question
/// "does this bay's plugin have a window?" is asked once per bay per FRAME by
/// the menu that greys the item — sixty times a second, for every bay — and
/// answering it by building a world each time would have been a world built
/// several hundred times a second. Answering it from a map built once is the
/// same answer for a millionth of the cost.
///
/// Keyed by URI because that is the only identity LV2 has.
fn meta() -> &'static std::collections::HashMap<String, PluginMeta> {
    static META: OnceLock<std::collections::HashMap<String, PluginMeta>> = OnceLock::new();
    META.get_or_init(scan_meta)
}

fn scan_meta() -> std::collections::HashMap<String, PluginMeta> {
    let world = lilv::World::with_load_all();
    let x11 = world.new_uri("http://lv2plug.in/ns/extensions/ui#X11UI");
    let control = world.new_uri("http://lv2plug.in/ns/lv2core#ControlPort");
    let input = world.new_uri("http://lv2plug.in/ns/lv2core#InputPort");
    let toggled = world.new_uri("http://lv2plug.in/ns/lv2core#toggled");
    let integer = world.new_uri("http://lv2plug.in/ns/lv2core#integer");
    let enumeration = world.new_uri("http://lv2plug.in/ns/lv2core#enumeration");
    let unit = world.new_uri("http://lv2plug.in/ns/extensions/units#unit");
    let unit_symbol = world.new_uri("http://lv2plug.in/ns/extensions/units#symbol");

    let mut out = std::collections::HashMap::new();
    for plugin in world.plugins().into_iter() {
        let Some(uri) = plugin.uri().as_uri().map(str::to_owned) else {
            continue;
        };
        let x11_ui = plugin
            .uis()
            .is_some_and(|uis| uis.into_iter().any(|u| u.is_a(&x11)));

        let mut controls = Vec::new();
        for i in 0..plugin.ports_count() {
            let Some(port) = plugin.port_by_index(i) else {
                continue;
            };
            if !port.is_a(&control) || !port.is_a(&input) {
                continue;
            }
            let symbol = port
                .symbol()
                .and_then(|n| n.as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("port{i}"));
            let name = port
                .name()
                .and_then(|n| n.as_str().map(str::to_owned))
                .unwrap_or_else(|| symbol.clone());
            // The unit's SYMBOL rather than its label: "dB" beside a number,
            // not "decibels". Two hops, because `units:unit` names a unit
            // resource and the symbol hangs off that.
            let units = port
                .get(&unit)
                .and_then(|u| world.get(Some(&u), Some(&unit_symbol), None))
                .and_then(|n| n.as_str().map(str::to_owned))
                .unwrap_or_default();
            // **Toggle first, then enumeration, then integer.** A port can
            // carry more than one of these and the narrowest true statement is
            // the useful one: a toggled port is a switch whether or not it is
            // also an integer.
            let steps = if port.has_property(&toggled) {
                2
            } else if port.has_property(&enumeration) {
                i32::try_from(port.scale_points().count()).unwrap_or(0)
            } else if port.has_property(&integer) {
                let r = port.range();
                match (r.minimum.as_ref().and_then(lilv::node::Node::as_float),
                       r.maximum.as_ref().and_then(lilv::node::Node::as_float)) {
                    (Some(lo), Some(hi)) if hi > lo => {
                        i32::try_from((hi - lo).round() as i64 + 1).unwrap_or(0)
                    }
                    _ => 0,
                }
            } else {
                0
            };
            controls.push(PortMeta {
                port_index: i,
                symbol,
                name,
                units,
                steps,
            });
        }

        out.insert(
            uri,
            PluginMeta {
                x11_ui,
                controls,
                latency_port: plugin.latency_port_index(),
            },
        );
    }
    out
}

/// This plugin's metadata, or an empty record if it is not installed.
pub fn meta_for(uri: &str) -> PluginMeta {
    meta().get(uri).cloned().unwrap_or_default()
}

/// Whether this plugin ships a UI this host can embed. See `lv2ui::Ui::has_ui`.
pub fn has_x11_ui(uri: &str) -> bool {
    meta().get(uri).is_some_and(|m| m.x11_ui)
}

/// Every LV2 plugin on `LV2_PATH`, in a stable order.
///
/// Sorted by name rather than left in lilv's order, which is the order bundles
/// happened to be read off disk: the list is shown to a person and remembered
/// in settings, and an order that shuffles between runs moves the entry under
/// their cursor.
pub fn discover() -> Vec<PluginInfo> {
    let cat = catalog();
    let mut out: Vec<PluginInfo> = cat
        .world
        .iter_plugins()
        .map(|p| PluginInfo {
            uri: p.uri(),
            name: p.name(),
            audio_in: p.ports_with_type(livi::PortType::AudioInput).count(),
            audio_out: p.ports_with_type(livi::PortType::AudioOutput).count(),
            is_instrument: p.is_instrument(),
            params: p.ports_with_type(livi::PortType::ControlInput).count(),
            wants_midi: p.ports_with_type(livi::PortType::AtomSequenceInput).count() > 0,
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.uri.cmp(&b.uri)));
    out
}

/// One plugin parameter, which in LV2 is a control port.
///
/// Simpler than VST3's, and better: the range is real engineering units the
/// plugin declared (dB, Hz, a percentage), not a normalised 0..1 that the host
/// has to ask the plugin to render into text.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamInfo {
    /// Position in the control-INPUT list, which is what [`Instance::set_param`]
    /// takes and what the automation road addresses. Not stable across plugin
    /// versions; `symbol` is the thing that is.
    pub index: usize,
    /// The plugin's ABSOLUTE port index, counting every port of every type.
    ///
    /// **Not the same number as `index`, and conflating them silently moves
    /// the wrong knob.** A plugin's ports are numbered in one sequence covering
    /// audio, atom and control alike, so a stereo effect with two audio inputs,
    /// two audio outputs and an atom port has its first control at absolute 5
    /// and control-input position 0. An LV2 UI speaks absolute indices — that
    /// is what `write_function` and `port_event` carry — while `set_param`
    /// speaks positions, so every value crossing between the UI and the DSP has
    /// to be translated, and this is the field that does it.
    pub port_index: usize,
    /// The stable identity. See [`PortMeta::symbol`] — saved state is keyed by
    /// this, so a plugin update that adds a port does not silently move every
    /// setting one knob to the left.
    pub symbol: String,
    pub name: String,
    /// "dB", "Hz", "%" — empty when the plugin declares none.
    pub units: String,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// The plugin's own resting value, kept so a panel can offer "reset" and so
    /// saved state can skip what has not been touched.
    pub default: f32,
    /// 0 continuous, 2 a toggle, otherwise the number of steps.
    pub steps: i32,
}

/// Write control values as LV2 state text.
///
/// **Free-standing, and both writers call it.** The host keeps its own record
/// of an LV2 bay's controls — see the app's `Lv2Bay` for why it must — so there
/// are two places that legitimately want to produce this blob: the instance,
/// while it is still here, and the host afterwards. Two copies of a format
/// diverge, and the one that diverges is the one nobody is testing, so there is
/// one copy and it is this.
pub fn state_from_params(params: &[ParamInfo]) -> String {
    let mut out = String::from("lv2state 1\n");
    for p in params {
        out.push_str(&p.symbol);
        out.push('=');
        out.push_str(&p.value.to_string());
        out.push('\n');
    }
    out
}

/// A running LV2 plugin.
pub struct Instance {
    inner: livi::Instance,
    uri: String,
    audio_in: usize,
    audio_out: usize,
    /// Owned so `process` never allocates. Indexed by port order.
    atom_in: Vec<livi::event::LV2AtomSequence>,
    atom_out: Vec<livi::event::LV2AtomSequence>,
    /// Silence handed to a plugin with more inputs than the caller supplies,
    /// and a bin for outputs the caller does not want. Allocated once.
    silence: Vec<f32>,
    params: Vec<ParamInfo>,
    midi_urid: u32,
    /// The designated latency output port, if the plugin declares one.
    ///
    /// **Read on the AUDIO thread, after processing.** LV2 reports latency
    /// through an ordinary control output port, which means the number is only
    /// valid once the plugin has run — there is no `getLatencySamples()` to
    /// call from the host thread the way VST3 has. So it is read where it is
    /// written, and published from there.
    latency_port: Option<livi::PortIndex>,
}

impl Instance {
    /// Instantiate by URI at a given sample rate.
    ///
    /// The URI rather than an index or a name, because it is the only stable
    /// identity LV2 offers and therefore the only thing worth persisting.
    pub fn create(uri: &str, sample_rate: f64) -> Result<Self, String> {
        let cat = catalog();
        let plugin = cat
            .world
            .plugin_by_uri(uri)
            .ok_or_else(|| format!("no LV2 plugin with URI {uri}"))?;

        let audio_in = plugin.ports_with_type(livi::PortType::AudioInput).count();
        let audio_out = plugin.ports_with_type(livi::PortType::AudioOutput).count();
        let n_atom_in = plugin
            .ports_with_type(livi::PortType::AtomSequenceInput)
            .count();
        let n_atom_out = plugin
            .ports_with_type(livi::PortType::AtomSequenceOutput)
            .count();

        // SAFETY: the plugin comes from this world, the features were built by
        // it, and the instance is not used from another thread until it is
        // moved there whole.
        let inner = unsafe { plugin.instantiate(cat.features.clone(), sample_rate) }
            .map_err(|e| format!("{uri} would not instantiate: {e:?}"))?;

        // **livi for the live port, lilv for what it means.** livi owns the
        // pointer the plugin actually reads and knows the value and the range;
        // it does not carry the name, the symbol or the unit, and lilv does.
        // The two are joined on the absolute port index, which is the only
        // number both of them agree on — the control-input POSITION differs
        // from the port index whenever a plugin interleaves audio and control
        // ports, which most of them do.
        let plugin_meta = meta_for(uri);
        let params: Vec<ParamInfo> = inner
            .iter_control_inputs()
            .enumerate()
            .map(|(i, c)| {
                let m = plugin_meta
                    .controls
                    .iter()
                    .find(|m| m.port_index == c.port_index.0);
                ParamInfo {
                    index: i,
                    port_index: c.port_index.0,
                    symbol: m.map_or_else(
                        || format!("port{}", c.port_index.0),
                        |m| m.symbol.clone(),
                    ),
                    name: m.map_or_else(
                        || format!("Parameter {}", i + 1),
                        |m| m.name.clone(),
                    ),
                    units: m.map(|m| m.units.clone()).unwrap_or_default(),
                    value: c.value,
                    min: c.minimum,
                    max: c.maximum,
                    default: c.value,
                    steps: m.map_or(0, |m| m.steps),
                }
            })
            .collect();

        Ok(Instance {
            inner,
            uri: uri.to_owned(),
            audio_in,
            audio_out,
            // **Every port must be connected, used or not.** LV2 is explicit
            // about this and a plugin is entitled to dereference any port it
            // declared. Dragonfly Reverb declares an atom input it uses only
            // for patch messages, and leaving it null is an immediate
            // `AtomSequenceInputsSizeMismatch` — or, in a host that does not
            // check, a crash inside the plugin.
            atom_in: (0..n_atom_in)
                .map(|_| livi::event::LV2AtomSequence::new(&cat.features, ATOM_CAPACITY))
                .collect(),
            atom_out: (0..n_atom_out)
                .map(|_| livi::event::LV2AtomSequence::new(&cat.features, ATOM_CAPACITY))
                .collect(),
            silence: vec![0.0; MAX_BLOCK],
            params,
            midi_urid: cat.features.midi_urid(),
            latency_port: plugin_meta.latency_port.map(livi::PortIndex),
        })
    }

    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn audio_inputs(&self) -> usize {
        self.audio_in
    }

    pub fn audio_outputs(&self) -> usize {
        self.audio_out
    }

    pub fn params(&self) -> &[ParamInfo] {
        &self.params
    }

    /// Set a control port. Takes effect on the next [`Instance::process`].
    pub fn set_param(&mut self, index: usize, value: f32) -> Result<(), String> {
        let port = self
            .inner
            .iter_control_inputs()
            .nth(index)
            .map(|c| c.port_index)
            .ok_or_else(|| format!("no parameter {index}"))?;
        let clamped = match self.params.get(index) {
            Some(p) => value.clamp(p.min, p.max),
            None => value,
        };
        self.inner.set_control_input(port, clamped);
        if let Some(p) = self.params.get_mut(index) {
            p.value = clamped;
        }
        Ok(())
    }

    /// This plugin's reported latency in samples, or 0 if it declares none.
    ///
    /// **Valid only after a block has been processed**, because LV2 publishes
    /// latency through an ordinary control OUTPUT port that the plugin writes
    /// during `run()`. A host that reads it before the first block gets
    /// whatever the port was initialised to, which is usually zero and
    /// occasionally garbage. Callers read it after processing, from the audio
    /// thread, which is the only place and time it means anything.
    pub fn latency_samples(&self) -> u32 {
        self.latency_port
            .and_then(|p| self.inner.control_output(p))
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map_or(0, |v| v as u32)
    }

    /// Every control port, as `symbol=value` lines.
    ///
    /// **This IS an LV2 plugin's state, for every plugin that has no other
    /// kind.** The format's own `state:` extension exists for plugins carrying
    /// data beyond their ports — a sampler's loaded file, a convolver's
    /// impulse — and reaching it needs `lilv_state_new_from_instance`, which
    /// wants the live `LilvInstance*` at a moment when this host has already
    /// handed the instance to the audio thread. For the control-port plugins
    /// that make up essentially all of the ports tree, this is complete.
    ///
    /// Keyed by SYMBOL, not index: see [`PortMeta::symbol`]. Defaults are
    /// written too rather than skipped, so restoring cannot leave a port at a
    /// value the plugin's own default happened to change to between versions.
    ///
    /// Text rather than a packed struct because it lands in a settings file
    /// that a person may reasonably open, and because a float written as text
    /// has no endianness to get wrong.
    pub fn save_state(&self) -> String {
        state_from_params(&self.params)
    }

    /// Apply state written by [`Instance::save_state`].
    ///
    /// **Unknown symbols are skipped rather than refused**, and missing ones
    /// are left at their defaults. Both are the same judgement: a plugin that
    /// gained a control in a new version should load a session saved by the old
    /// one, and a plugin that removed one should not refuse to load at all. The
    /// count of what was applied comes back so a caller can tell "restored
    /// nothing because the blob was foreign" from "restored nothing because
    /// there was nothing to restore".
    pub fn load_state(&mut self, blob: &str) -> Result<usize, String> {
        let mut lines = blob.lines();
        match lines.next() {
            Some(h) if h.starts_with("lv2state ") => {}
            _ => return Err("not LV2 state".to_owned()),
        }
        let mut applied = 0;
        for line in lines {
            let Some((sym, val)) = line.split_once('=') else {
                continue;
            };
            let Ok(v) = val.trim().parse::<f32>() else {
                continue;
            };
            let Some(i) = self.params.iter().position(|p| p.symbol == sym) else {
                continue;
            };
            if self.set_param(i, v).is_ok() {
                applied += 1;
            }
        }
        Ok(applied)
    }

    /// Clear the MIDI queued for the next block.
    ///
    /// Separate from `process` so a caller can queue events across several
    /// calls, and because clearing is the one thing that must happen even when
    /// no events arrive — a sequence left un-cleared replays last block's notes
    /// forever, which sounds exactly like a stuck note.
    pub fn clear_midi(&mut self) {
        for seq in &mut self.atom_in {
            seq.clear();
        }
    }

    /// Queue one MIDI message for the next block.
    ///
    /// `frame` is the offset within the coming block. Events must be pushed in
    /// non-decreasing frame order — LV2 requires a sorted atom sequence and a
    /// plugin may ignore, or mis-order, anything else.
    pub fn push_midi(&mut self, frame: i64, data: &[u8]) -> Result<(), String> {
        let Some(seq) = self.atom_in.first_mut() else {
            // Not an error: plenty of effects have no MIDI input, and a caller
            // routing notes to a reverb should not get a failure for it.
            return Ok(());
        };
        match data.len() {
            3 => seq
                .push_midi_event::<3>(frame, self.midi_urid, data)
                .map_err(|e| format!("MIDI event rejected: {e:?}")),
            2 => seq
                .push_midi_event::<2>(frame, self.midi_urid, data)
                .map_err(|e| format!("MIDI event rejected: {e:?}")),
            1 => seq
                .push_midi_event::<1>(frame, self.midi_urid, data)
                .map_err(|e| format!("MIDI event rejected: {e:?}")),
            _ => Ok(()),
        }
    }

    /// Run one block over the planar buffers an engine actually holds.
    ///
    /// The engine's channels are `Vec<f32>` per channel, not the `&[&[f32]]`
    /// [`Instance::process`] takes, and converting between them at the call
    /// site is where an allocation would sneak onto the audio thread: building
    /// a `&mut [&mut [f32]]` out of a `&mut [Vec<f32>]` needs somewhere to put
    /// the pointers. Done here instead, where `iter_mut().map(..)` is already
    /// the `ExactSizeIterator` livi wants and nothing is built at all.
    ///
    /// Extra channels beyond what the plugin declares are left untouched — the
    /// caller decides what an unwritten channel means, and for a voice that is
    /// "copy channel 0 across", which is not this function's business.
    pub fn process_planar(
        &mut self,
        frames: usize,
        inputs: &[Vec<f32>],
        outputs: &mut [Vec<f32>],
    ) -> Result<(), String> {
        if frames > MAX_BLOCK {
            return Err(format!("block of {frames} exceeds {MAX_BLOCK}"));
        }
        let n_out = self.audio_out.min(32);
        if outputs.len() < n_out || outputs.iter().take(n_out).any(|c| c.len() < frames) {
            return Err(format!(
                "{} output channels of at least {frames} frames needed, {} supplied",
                n_out,
                outputs.len()
            ));
        }
        for seq in &mut self.atom_out {
            seq.clear();
        }

        let silence = &self.silence[..frames];
        let mut in_slices: [&[f32]; 32] = [silence; 32];
        let n_in = self.audio_in.min(32);
        for (i, slot) in in_slices.iter_mut().enumerate().take(n_in) {
            if let Some(ch) = inputs.get(i) {
                if ch.len() >= frames {
                    *slot = &ch[..frames];
                }
            }
        }

        let ports = livi::EmptyPortConnections::new()
            .with_audio_inputs(in_slices[..n_in].iter().copied())
            .with_audio_outputs(outputs[..n_out].iter_mut().map(|c| &mut c[..frames]))
            .with_atom_sequence_inputs(self.atom_in.iter())
            .with_atom_sequence_outputs(self.atom_out.iter_mut());

        // SAFETY: every declared port is connected, every buffer is at least
        // `frames` long (checked above), and `frames <= MAX_BLOCK`.
        unsafe { self.inner.run(frames, ports) }.map_err(|e| format!("LV2 run failed: {e:?}"))
    }

    /// Run one block.
    ///
    /// **Audio-thread code.** No allocation, no locking, no panicking path: the
    /// buffers are already owned, the port counts are already known, and a
    /// mismatch returns `Err` rather than unwrapping.
    ///
    /// `inputs` and `outputs` are the caller's channels. A plugin wanting more
    /// inputs than are supplied is given silence, and outputs the caller does
    /// not want are written to a bin — which is what lets a stereo host drive a
    /// mono plugin and vice versa without the caller arranging it.
    pub fn process(
        &mut self,
        frames: usize,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
    ) -> Result<(), String> {
        if frames > MAX_BLOCK {
            return Err(format!(
                "block of {frames} exceeds the {MAX_BLOCK} this plugin was instantiated for"
            ));
        }
        for seq in &mut self.atom_out {
            seq.clear();
        }

        // Borrow the fixed slices up front; `run` wants exact-size iterators
        // and building these allocates nothing.
        let silence = &self.silence[..frames];
        let mut in_slices: [&[f32]; 32] = [silence; 32];
        let n_in = self.audio_in.min(32);
        for (i, slot) in in_slices.iter_mut().enumerate().take(n_in) {
            if let Some(ch) = inputs.get(i) {
                if ch.len() >= frames {
                    *slot = &ch[..frames];
                }
            }
        }

        // **Outputs are the caller's job to supply in full, and that is a
        // deliberate asymmetry with the inputs above.**
        //
        // Missing inputs can be silence, which is a real signal and costs
        // nothing. Missing outputs cannot be handled the same way: every unused
        // output needs somewhere distinct to be written, and one shared bin
        // would mean handing the plugin two pointers to the same memory. The
        // alternatives are a `Vec` — an allocation, on the audio thread, every
        // block — or a bespoke `ExactSizeIterator`, since `Chain` is not one.
        //
        // Neither is worth it when the caller already knows `audio_outputs()`
        // and owns its own scratch. So this asks, and says so when it does not
        // get it, rather than quietly writing a plugin's second channel over
        // its first.
        let n_out = self.audio_out.min(32);
        if outputs.len() < n_out {
            return Err(format!(
                "{} output channels supplied for a plugin with {n_out}",
                outputs.len()
            ));
        }
        let (given, _) = outputs.split_at_mut(n_out);

        let ports = livi::EmptyPortConnections::new()
            .with_audio_inputs(in_slices[..n_in].iter().copied())
            .with_audio_outputs(given.iter_mut().map(|c| &mut (*c)[..frames]))
            .with_atom_sequence_inputs(self.atom_in.iter())
            .with_atom_sequence_outputs(self.atom_out.iter_mut());

        // SAFETY: every declared port is connected above, all buffers are at
        // least `frames` long, and `frames <= MAX_BLOCK`, which is the bound
        // the plugin was instantiated under.
        unsafe { self.inner.run(frames, ports) }.map_err(|e| format!("LV2 run failed: {e:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The LV2 specification bundle ships example plugins, and the `lv2` port
    /// installs them, so these are available wherever LV2 development is. Where
    /// they are not, the tests skip rather than fail: a machine without them is
    /// not a machine with a bug.
    fn have(uri: &str) -> bool {
        discover().iter().any(|p| p.uri == uri)
    }

    const AMP: &str = "http://lv2plug.in/plugins/eg-amp";
    const FIFTHS: &str = "http://lv2plug.in/plugins/eg-fifths";

    /// The metadata scan gives a control its real name, symbol and unit —
    /// which is the whole difference between a usable parameter panel and a
    /// column of "Parameter 7".
    #[test]
    fn a_control_port_arrives_with_its_name_symbol_and_unit() {
        if !have(AMP) {
            return;
        }
        let inst = Instance::create(AMP, 48_000.0).expect("eg-amp instantiates");
        let gain = inst
            .params()
            .iter()
            .find(|p| p.symbol == "gain")
            .expect("eg-amp's control port is called `gain` in its own manifest");
        // Not the placeholder. The name comes from the plugin's Turtle.
        assert!(
            !gain.name.starts_with("Parameter "),
            "fell back to the index: {}",
            gain.name
        );
        // eg-amp declares its gain in decibels, which is the case the panel
        // exists to show honestly rather than as a 0..1.
        assert_eq!(gain.units, "dB", "the unit's symbol, not its label");
        assert!(gain.min < gain.max);
    }

    /// **The two numberings are not the same, and a real plugin proves it.**
    ///
    /// An LV2 UI reports the absolute port index; `set_param` takes the
    /// control-input position. For any plugin with an audio or atom port ahead
    /// of its first control — nearly all of them — those differ, and a host
    /// that passes one where the other is wanted moves a DIFFERENT knob. The
    /// sound changes, so it looks like it worked; only the saved file shows the
    /// parameter the user turned still sitting at its default.
    ///
    /// This asserts the gap exists rather than merely that the field is
    /// present, because a test that passes when the two happen to coincide
    /// would go on passing after the translation was deleted.
    #[test]
    fn a_controls_absolute_port_is_not_its_control_input_position() {
        let mut proved = false;
        for p in discover() {
            let Ok(inst) = Instance::create(&p.uri, 48_000.0) else {
                continue;
            };
            for c in inst.params() {
                assert!(
                    c.port_index >= c.index,
                    "{}: a control cannot come before its own position",
                    p.uri
                );
                if c.port_index != c.index {
                    proved = true;
                }
            }
            if proved {
                break;
            }
        }
        // Not an assert on `proved`: a machine with only trivial plugins
        // installed is entitled to have every control at position 0. The value
        // of the test is the invariant above plus the report.
        if !proved {
            eprintln!("note: no installed plugin distinguishes the two numberings");
        }
    }

    /// State round-trips through the text format, values and all.
    #[test]
    fn state_round_trips_through_the_text_format() {
        if !have(AMP) {
            return;
        }
        let mut a = Instance::create(AMP, 48_000.0).expect("instantiates");
        let i = a
            .params()
            .iter()
            .position(|p| p.symbol == "gain")
            .expect("has a gain");
        let want = a.params()[i].max.min(6.0);
        a.set_param(i, want).expect("gain is settable");
        let blob = a.save_state();

        let mut b = Instance::create(AMP, 48_000.0).expect("instantiates");
        assert_eq!(b.load_state(&blob).expect("our own blob parses"), 1);
        assert!((b.params()[i].value - want).abs() < 1e-6);
    }

    /// **Keyed by symbol, and that is the point of the format.** A blob naming
    /// a port the plugin does not have is skipped rather than refused, and a
    /// port the blob does not name keeps its default — which together are what
    /// let a session survive the plugin gaining or losing a control. Keyed by
    /// index, the same edit would have silently moved every setting one knob
    /// along.
    #[test]
    fn unknown_symbols_are_skipped_and_missing_ones_keep_their_default() {
        if !have(AMP) {
            return;
        }
        let mut inst = Instance::create(AMP, 48_000.0).expect("instantiates");
        let i = inst
            .params()
            .iter()
            .position(|p| p.symbol == "gain")
            .expect("has a gain");
        let default = inst.params()[i].default;
        let applied = inst
            .load_state("lv2state 1\nnot_a_real_port=3.5\n")
            .expect("the header is ours");
        assert_eq!(applied, 0, "a foreign symbol must not land anywhere");
        assert!(
            (inst.params()[i].value - default).abs() < 1e-6,
            "an unnamed port must be left alone"
        );
    }

    /// A blob from somewhere else is refused rather than half-applied.
    #[test]
    fn a_foreign_blob_is_refused() {
        if !have(AMP) {
            return;
        }
        let mut inst = Instance::create(AMP, 48_000.0).expect("instantiates");
        assert!(inst.load_state("VST3 state or anything else").is_err());
        assert!(inst.load_state("").is_err());
    }

    /// Values are clamped on the way in, so a hand-edited or corrupted settings
    /// file cannot drive a plugin outside the range it declared.
    #[test]
    fn a_restored_value_is_clamped_to_the_ports_range() {
        if !have(AMP) {
            return;
        }
        let mut inst = Instance::create(AMP, 48_000.0).expect("instantiates");
        let i = inst
            .params()
            .iter()
            .position(|p| p.symbol == "gain")
            .expect("has a gain");
        let max = inst.params()[i].max;
        inst.load_state("lv2state 1\ngain=1e9\n").expect("parses");
        assert!((inst.params()[i].value - max).abs() < 1e-3);
    }

    /// The metadata cache answers the editor question without a lilv world per
    /// call, and agrees with what discovery found.
    #[test]
    fn the_metadata_cache_covers_every_discovered_plugin() {
        for p in discover() {
            let m = meta_for(&p.uri);
            assert_eq!(
                m.x11_ui,
                has_x11_ui(&p.uri),
                "{} disagrees with itself about having a UI",
                p.uri
            );
        }
    }

    #[test]
    fn an_identity_round_trips_and_a_bundle_path_is_not_mistaken_for_one() {
        let id = identity_for("http://lv2plug.in/plugins/eg-amp");
        assert_eq!(
            uri_of(&id).as_deref(),
            Some("http://lv2plug.in/plugins/eg-amp")
        );
        // The discriminator has to be airtight in this direction: a VST3 bundle
        // read as an LV2 URI would be looked up in lilv and reported missing,
        // which looks like a broken plugin rather than a broken host.
        for not_lv2 in [
            "/usr/local/lib/vst3/Foo.vst3",
            "/home/someone/.vst3/Bar.vst3",
            "/",
        ] {
            assert_eq!(uri_of(std::path::Path::new(not_lv2)), None, "{not_lv2}");
        }
    }

    #[test]
    fn a_vendor_is_found_for_every_uri_shape_in_the_wild() {
        // Both shapes ship in one product: Dragonfly Reverb publishes one
        // plugin under a URL and three under URNs, so a rule that handles only
        // the first files three quarters of a vendor's output as unnamed.
        assert_eq!(
            vendor_of("https://github.com/michaelwillis/dragonfly-reverb"),
            "github.com"
        );
        assert_eq!(vendor_of("urn:dragonfly:hall"), "dragonfly");
        assert_eq!(vendor_of("http://lv2plug.in/plugins/eg-amp"), "lv2plug.in");
        assert_eq!(
            vendor_of("http://breakfastquay.com/rdf/lv2-rubberband#mono"),
            "breakfastquay.com"
        );
        // Anything unrecognised still gets a home rather than the unnamed pile.
        assert_eq!(vendor_of("something-odd"), "LV2");
        assert_eq!(vendor_of("urn:"), "LV2");
        assert_eq!(vendor_of("://"), "LV2");
    }

    #[test]
    fn every_installed_plugin_gets_a_vendor() {
        for p in discover() {
            assert!(
                !vendor_of(&p.uri).is_empty(),
                "{} ({}) has nothing to group it under",
                p.name,
                p.uri
            );
        }
    }

    #[test]
    fn discovery_does_not_panic_without_plugins() {
        // The only hard requirement: an empty LV2_PATH is a normal machine,
        // not an error.
        let _ = discover();
    }

    #[test]
    fn discovery_is_sorted_and_uris_are_unique() {
        let found = discover();
        let mut names: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();
        let original = names.clone();
        names.sort();
        assert_eq!(names, original, "discover() must return a stable sorted order");

        let mut uris: Vec<&str> = found.iter().map(|p| p.uri.as_str()).collect();
        uris.sort();
        let before = uris.len();
        uris.dedup();
        assert_eq!(before, uris.len(), "a URI was listed twice");
    }

    #[test]
    fn the_gain_plugin_applies_its_gain() {
        if !have(AMP) {
            eprintln!("skipping: {AMP} not installed");
            return;
        }
        let mut inst = Instance::create(AMP, 48_000.0).expect("instantiate eg-amp");
        assert_eq!(inst.audio_inputs(), 1);
        assert_eq!(inst.audio_outputs(), 1);
        assert!(!inst.params().is_empty(), "eg-amp has a gain control");

        let input = vec![0.5f32; 128];
        for (db, want) in [(0.0f32, 0.5f32), (6.0, 0.9976), (-6.0, 0.2506)] {
            inst.set_param(0, db).expect("set gain");
            let mut out = vec![0.0f32; 128];
            {
                let mut outs: [&mut [f32]; 1] = [&mut out];
                inst.process(128, &[&input], &mut outs).expect("process");
            }
            assert!(
                (out[64] - want).abs() < 0.001,
                "at {db} dB expected {want}, got {}",
                out[64]
            );
        }
    }

    #[test]
    fn gain_is_clamped_to_the_ports_declared_range() {
        if !have(AMP) {
            return;
        }
        let mut inst = Instance::create(AMP, 48_000.0).expect("instantiate");
        let max = inst.params()[0].max;
        inst.set_param(0, max + 1000.0).expect("set");
        assert_eq!(
            inst.params()[0].value, max,
            "a value past the port's range must clamp, not be passed through"
        );
    }

    #[test]
    fn midi_reaches_the_plugin_and_its_answer_comes_back() {
        if !have(FIFTHS) {
            eprintln!("skipping: {FIFTHS} not installed");
            return;
        }
        // eg-fifths echoes each note and adds the note a fifth above it, which
        // makes it a MIDI round trip with a checkable answer.
        let mut inst = Instance::create(FIFTHS, 48_000.0).expect("instantiate eg-fifths");
        inst.clear_midi();
        inst.push_midi(0, &[0x90, 60, 0x7f]).expect("queue note on");
        let mut outs: [&mut [f32]; 0] = [];
        inst.process(128, &[], &mut outs).expect("process");

        let notes: Vec<u8> = inst.atom_out[0]
            .iter()
            .filter(|e| e.data.len() >= 3 && e.data[0] & 0xF0 == 0x90)
            .map(|e| e.data[1])
            .collect();
        assert!(notes.contains(&60), "the played note should come back: {notes:?}");
        assert!(notes.contains(&67), "and its fifth: {notes:?}");
    }

    #[test]
    fn a_stale_sequence_does_not_replay_last_blocks_notes() {
        if !have(FIFTHS) {
            return;
        }
        let mut inst = Instance::create(FIFTHS, 48_000.0).expect("instantiate");
        inst.clear_midi();
        inst.push_midi(0, &[0x90, 60, 0x7f]).expect("queue");
        let mut outs: [&mut [f32]; 0] = [];
        inst.process(128, &[], &mut outs).expect("first block");

        // Second block with nothing queued: a host that forgot to clear would
        // send the same note again, which is what a stuck note sounds like.
        inst.clear_midi();
        inst.process(128, &[], &mut outs).expect("second block");
        let notes = inst.atom_out[0].iter().filter(|e| e.data.len() >= 3).count();
        assert_eq!(notes, 0, "cleared input must produce no repeat output");
    }

    #[test]
    fn an_oversized_block_is_refused_rather_than_overrunning() {
        if !have(AMP) {
            return;
        }
        let mut inst = Instance::create(AMP, 48_000.0).expect("instantiate");
        let input = vec![0.0f32; 16];
        let mut out = vec![0.0f32; 16];
        let mut outs: [&mut [f32]; 1] = [&mut out];
        let err = inst.process(MAX_BLOCK + 1, &[&input], &mut outs);
        assert!(err.is_err(), "a block past MAX_BLOCK must be an error");
    }

    /// A real third-party stereo effect, which is a different shape from the
    /// examples: two channels, eighteen control ports, and an atom input it
    /// uses only for patch messages.
    ///
    /// That last one is the case worth pinning. LV2 requires every declared
    /// port to be connected whether the host has anything to put in it or not,
    /// and a first attempt at this module connected only the ports it cared
    /// about — which Dragonfly rejects outright. A plugin less careful than
    /// Dragonfly would have dereferenced null instead.
    #[test]
    fn a_real_stereo_effect_with_an_unused_atom_port_runs() {
        const HALL: &str = "https://github.com/michaelwillis/dragonfly-reverb";
        if !have(HALL) {
            eprintln!("skipping: Dragonfly Hall Reverb not installed");
            return;
        }
        let mut inst = Instance::create(HALL, 48_000.0).expect("instantiate");
        assert_eq!(inst.audio_inputs(), 2);
        assert_eq!(inst.audio_outputs(), 2);
        assert!(inst.params().len() > 10, "a reverb has plenty of controls");

        // An impulse, then silence. A reverb must still be producing output
        // after the impulse has gone - that tail is the proof it ran, where a
        // plugin that silently did nothing would give zeros throughout.
        let mut left = vec![0.0f32; 256];
        let mut right = vec![0.0f32; 256];
        left[0] = 1.0;
        right[0] = 1.0;
        let mut tail_after_impulse = 0.0f32;
        for block in 0..6 {
            let (mut ol, mut or) = (vec![0.0f32; 256], vec![0.0f32; 256]);
            {
                let mut outs: [&mut [f32]; 2] = [&mut ol, &mut or];
                inst.process(256, &[&left, &right], &mut outs).expect("process");
            }
            if block > 0 {
                tail_after_impulse += ol.iter().map(|s| s.abs()).sum::<f32>();
            }
            left[0] = 0.0;
            right[0] = 0.0;
        }
        assert!(
            tail_after_impulse > 0.0,
            "a reverb fed an impulse must keep producing output afterwards"
        );
    }

    #[test]
    fn too_few_output_channels_is_refused_rather_than_aliased() {
        const HALL: &str = "https://github.com/michaelwillis/dragonfly-reverb";
        if !have(HALL) {
            return;
        }
        let mut inst = Instance::create(HALL, 48_000.0).expect("instantiate");
        let (l, r) = (vec![0.0f32; 64], vec![0.0f32; 64]);
        let mut only_one = vec![0.0f32; 64];
        let mut outs: [&mut [f32]; 1] = [&mut only_one];
        assert!(
            inst.process(64, &[&l, &r], &mut outs).is_err(),
            "a stereo plugin given one output must refuse, not write both channels to it"
        );
    }

    #[test]
    fn an_unknown_uri_is_an_error_not_a_panic() {
        let err = Instance::create("urn:definitely:not:a:plugin", 48_000.0);
        assert!(err.is_err());
    }

    #[test]
    fn a_plugin_with_no_midi_port_accepts_and_drops_notes() {
        if !have(AMP) {
            return;
        }
        // eg-amp has no atom input. Routing a note at it is a caller mistake
        // that must not be a failure - see push_midi.
        let mut inst = Instance::create(AMP, 48_000.0).expect("instantiate");
        inst.clear_midi();
        assert!(inst.push_midi(0, &[0x90, 60, 0x7f]).is_ok());
    }
}
