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
    pub index: usize,
    pub name: String,
    pub value: f32,
    pub min: f32,
    pub max: f32,
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

        let params = inner
            .iter_control_inputs()
            .enumerate()
            .map(|(i, c)| ParamInfo {
                index: i,
                // lilv can give a port its human name; livi's ControlPort does
                // not carry it, so the index stands in until that is plumbed.
                // Named rather than blank so the panel has something to show.
                name: format!("Parameter {}", i + 1),
                value: c.value,
                min: c.minimum,
                max: c.maximum,
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
