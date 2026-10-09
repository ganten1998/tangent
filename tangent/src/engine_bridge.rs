//! **The one seam between the app and the engine** (C9 in
//! `docs/design/unlimited-tracks.md`).
//!
//! This first cut is the measuring half only: the harness and the export
//! goldens, recorded against the furnishing `DesktopApp` does TODAY, so the
//! move that follows is held to the bytes rather than to a reading of it.
//! The pushes and the offline furnishing below are frozen transcriptions of
//! `push_monitor_settings`, `publish_routes`, `push_auto_suspend` and
//! `start_offline_render` at the base commit (`self.app` read as `app`, the
//! live engine and the DX7 cartridge cache handed in, the notifications
//! dropped); they exist to write the goldens and leave with the move.

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use tangent_record::audio::Timebase;
    use tangent_ui::app::TangentApp;
    use tangent_ui::arrangement::{ArrClip, ArrNote, Arrangement, ClipKind, FrozenLane};
    use tangent_ui::automation::{AutoParam, Automation, Curve, Point, Shape};
    use tangent_ui::host::Caps;
    use tangent_ui::recorder::{ChannelKind, CHANNELS, INSERTS, STRIPS};
    use tangent_ui::routing::{Route, Target};
    use tangent_ui::settings::Settings;

    use crate::builtin_fx::{CHORUS_PATH, COMP_PATH, EQ_PATH, PLATE_PATH, SAMPLER_PATH, TRIM_PATH};
    use crate::desktop::{
        arm_stem_tap, effect_params_from, export_clip_placement, frozen_lane_mask,
        furnish_mirror_bay, is_instrument_sentinel, keys_patch_of, mirror_sampler_samples,
        sanitize_file_stem,
    };
    use crate::instrument::{Engine, MASTER_AUTO_LANE};

    const RATE: u32 = 48_000;

    /// Where the goldens live, beside the render goldens.
    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/exports")
            .join(name)
    }

    /// `TANGENT_GOLDEN_WRITE=1` rewrites a golden instead of comparing, as
    /// `render_golden` does. Only for a change MEANT to change what the
    /// engine is told or what an export writes, named in the commit.
    fn writing() -> bool {
        std::env::var_os("TANGENT_GOLDEN_WRITE").is_some()
    }

    /// FNV-1a, 64-bit, over a file's bytes.
    fn fnv(bytes: &[u8]) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{h:016x}")
    }

    /// A scratch folder of this test's own, under the temp dir and never
    /// under HOME: nothing here may reach a real config or media root.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tangent-bridge-{}-{:?}-{tag}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("media")).expect("a scratch project");
        dir
    }

    /// The app under test: a free install, and `Caps::PLUGIN` so nothing it
    /// does writes a settings file (`persist_global_settings` is off).
    fn app_with(settings: Settings) -> TangentApp {
        let ctx = egui::Context::default();
        TangentApp::with_license(&ctx, settings, Caps::PLUGIN, crate::composite::tiers::free("bridge"))
    }

    /// The shipped desk with every channel emptied, so each desk below says
    /// in full what is on it rather than inheriting the fresh install's
    /// drumpad and DX7.
    fn blank() -> Settings {
        let mut s = Settings::default();
        s.channel_kinds = [0; CHANNELS];
        s.strip_inserts.iter_mut().for_each(String::clear);
        s.strip_sends = [0.0; STRIPS];
        s.channel_armed = Some(0);
        s.strip_muted = 0;
        s.strip_soloed = 0;
        s.strip_phase = 0;
        s
    }

    fn kind(s: &mut Settings, lanes: &[usize], k: ChannelKind) {
        for &l in lanes {
            s.channel_kinds[l] = ChannelKind::as_u8(Some(k));
        }
    }

    fn bay(s: &mut Settings, strip: usize, slot: usize, unit: &str) {
        s.strip_inserts[strip * INSERTS + slot] = unit.to_owned();
    }

    // ── the desk-structure pushes ───────────────────────────────────────────

    /// What `PushedDesk` held at the base commit.
    #[derive(Clone, PartialEq)]
    struct BasePushed {
        epoch: u64,
        outs: usize,
        routes: tangent_ui::routing::Routes,
        keys: tangent_ui::routing::KeyEdges,
        bypass: Vec<(usize, usize)>,
        desk: tangent_ui::recorder::Desk,
    }

    /// `publish_routes` at the base commit.
    fn base_publish_routes(app: &TangentApp, e: &Engine) {
        let outs = e.output().channels as usize;
        let mut routes = app.routes().clone();
        routes.prune(outs, CHANNELS);
        e.set_routes(&routes);
        let keys: Vec<(usize, usize, usize)> = app
            .insert_keys()
            .0
            .iter()
            .filter(|k| k.from < CHANNELS)
            .map(|k| {
                let (seat, bay) = tangent_ui::recorder::seat_bay(k.strip, k.slot);
                (seat, bay, k.from)
            })
            .collect();
        e.set_insert_keys(&keys);
        e.set_insert_bypass(app.insert_bypass());
        e.set_frozen(frozen_lane_mask(app.frozen_lanes()));
        e.set_grain_ms(app.render_ahead_ms());
    }

    /// The desk-structure half of `push_monitor_settings` at the base
    /// commit, plus `push_auto_suspend`: every set_* the bridge takes over,
    /// in the order the frame made them.
    fn base_push(
        app: &TangentApp,
        e: &Engine,
        epoch: u64,
        pushed: &mut Option<BasePushed>,
        punches: [Option<u64>; CHANNELS],
    ) {
        let desk = app.desk();
        let outs = e.output().channels as usize;
        let now = BasePushed {
            epoch,
            outs,
            routes: app.routes().clone(),
            keys: app.insert_keys().clone(),
            bypass: app.insert_bypass().to_vec(),
            desk,
        };
        if pushed.as_ref() != Some(&now) {
            base_publish_routes(app, e);
            e.set_desk(&desk);
            *pushed = Some(now);
        }
        let mut builtin_mask = 0u32;
        let mut keys_mask = 0u32;
        let armed = app.armed_mask();
        for ch in 0..CHANNELS {
            if app.channel_kind(ch) == Some(ChannelKind::Midi) {
                if app.insert(ch, 0) == Some(tangent_ui::dialogs::BUILTIN_PATH) {
                    builtin_mask |= 1 << ch.min(31);
                }
                if app.insert(ch, 0) == Some(tangent_ui::dialogs::KEYS_PATH) {
                    keys_mask |= 1 << ch.min(31);
                }
            }
        }
        let kinds: Vec<_> = (0..CHANNELS)
            .map(|ch| app.channel_kind(ch))
            .collect();
        let midi_off = tangent_ui::recorder::live_midi_off(&kinds, armed, app.midi_editor_lane());
        e.set_midi_off(u64::from(midi_off) | u64::from(frozen_lane_mask(app.frozen_lanes())));
        e.set_midi_routes(&app.midi_routes());
        e.set_armed(armed);
        e.set_punches(punches);
        let thawed = !frozen_lane_mask(app.frozen_lanes());
        e.set_builtin_strips(builtin_mask & thawed);
        e.set_keys_strips(keys_mask & thawed);
        let drum_lanes: Vec<usize> = (0..CHANNELS)
            .filter(|ch| app.insert(*ch, 0) == Some(tangent_ui::dialogs::DRUMPAD_PATH))
            .collect();
        e.set_drum_strips(drum_lanes.iter().fold(0u32, |m, ch| m | 1 << ch.min(&31)));
        e.set_folders(app.folder_parents());
        e.set_strip_phase(app.strip_phase());
        e.set_auto_suspend(app.auto_suspend_mask());
    }

    /// The pushes' state between frames, as the desktop keeps it.
    struct Pusher {
        epoch: u64,
        pushed: Option<BasePushed>,
    }

    impl Pusher {
        fn new() -> Self {
            Self { epoch: 1, pushed: None }
        }

        fn engine_replaced(&mut self) {
            self.epoch += 1;
        }

        fn push(&mut self, app: &TangentApp, e: &mut Engine, punches: [Option<u64>; CHANNELS]) {
            base_push(app, e, self.epoch, &mut self.pushed, punches);
        }
    }

    /// One scripted edit: a name, the desk after it, and the punches the
    /// arrangement record would hand the frame.
    struct Step {
        name: &'static str,
        settings: Settings,
        punches: [Option<u64>; CHANNELS],
    }

    /// Arm, mute, solo, phase, folder, route, key, bypass and freeze, one
    /// edit at a time on top of the last, over a desk that holds every
    /// instrument kind the strip masks name.
    fn steps() -> Vec<Step> {
        let mut s = blank();
        kind(&mut s, &[0, 1, 2], ChannelKind::Midi);
        kind(&mut s, &[3, 4, 5, 6, 7], ChannelKind::Audio);
        bay(&mut s, 0, 0, tangent_ui::dialogs::BUILTIN_PATH);
        bay(&mut s, 1, 0, tangent_ui::dialogs::KEYS_PATH);
        bay(&mut s, 2, 0, tangent_ui::dialogs::DRUMPAD_PATH);
        bay(&mut s, 4, 0, COMP_PATH);
        bay(&mut s, 4, 1, EQ_PATH);
        s.strip_sends[3] = 0.5;
        let none = [None; CHANNELS];
        let mut out = vec![Step { name: "desk", settings: s.clone(), punches: none }];
        let mut edit = |name: &'static str, f: &dyn Fn(&mut Settings), punches| {
            f(&mut s);
            out.push(Step { name, settings: s.clone(), punches });
        };
        edit("arm", &|s| s.channel_armed = Some(1 << 0 | 1 << 4), none);
        edit("mute", &|s| s.strip_muted |= 1 << 2, none);
        edit("solo", &|s| s.strip_soloed |= 1 << 4, none);
        edit("phase", &|s| s.strip_phase |= 1 << 1 | 1 << 34, none);
        edit(
            "folder",
            &|s| {
                s.strip_is_folder[5] = true;
                s.strip_in_folder[6] = true;
                s.strip_in_folder[7] = true;
                s.strip_muted |= 1 << 5;
            },
            none,
        );
        edit(
            "route",
            &|s| {
                s.routes.add(Route::new(3, Target::Channel(4)));
                s.routes.add(Route::new(6, Target::Hardware { first: 0, stereo: true }));
                // Past the offline engine's two outputs: pruned at the door.
                s.routes.add(Route::new(7, Target::Hardware { first: 4, stereo: true }));
            },
            none,
        );
        edit("route-gain", &|s| s.routes.set_gain(0, 0.5), none);
        edit("key", &|s| s.insert_keys.set(4, 0, Some(3)), none);
        edit("bypass", &|s| s.insert_bypass.0.push((4, 1)), none);
        edit(
            "freeze",
            &|s| s.frozen_lanes.push(FrozenLane { lane: 0, path: "media/f.wav".into(), len_s: 1.0 }),
            none,
        );
        let mut punched = none;
        punched[3] = Some(48_000);
        edit("punch", &|_| {}, punched);
        edit("unpunch", &|_| {}, none);
        edit("render-ahead", &|s| s.render_ahead_ms = 5.0, none);
        out
    }

    /// **The bridge's harness**: a free app, an offline engine, and the
    /// scripted edits above synced one frame each; what the engine holds
    /// after every frame is read back whole and compared against the golden
    /// of what `DesktopApp` pushed at the base commit. Then the same desk
    /// again on a FRESH engine, which must be primed whole, and a frame that
    /// changed nothing, which must not republish the tables (a value
    /// planted behind the gate survives it).
    #[test]
    fn the_bridge_pushes_what_the_desktop_pushed() {
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Pusher::new();
        let mut seen: Vec<(String, serde_json::Value)> = Vec::new();
        let mut last = None;
        for step in steps() {
            let app = app_with(step.settings.clone());
            p.push(&app, &mut e, step.punches);
            seen.push((step.name.to_owned(), e.desk_readback()));
            last = Some((app, step.punches));
        }
        let (app, punches) = last.expect("steps");
        // An unchanged frame: a route planted behind the gate stays.
        let mut planted = tangent_ui::routing::Routes::default();
        planted.add(Route::new(9, Target::Channel(10)));
        e.set_routes(&planted);
        p.push(&app, &mut e, punches);
        seen.push(("unchanged".to_owned(), e.desk_readback()));
        // A fresh engine is primed whole.
        let mut fresh = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        p.engine_replaced();
        p.push(&app, &mut fresh, punches);
        seen.push(("fresh-engine".to_owned(), fresh.desk_readback()));

        let got = serde_json::Value::Array(
            seen.into_iter()
                .map(|(name, v)| serde_json::json!({ "step": name, "engine": v }))
                .collect(),
        );
        let path = fixture("bridge.json");
        if writing() {
            std::fs::write(&path, serde_json::to_string_pretty(&got).expect("json") + "\n")
                .expect("the bridge golden");
            return;
        }
        let want: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).expect("tests/fixtures/exports/bridge.json"),
        )
        .expect("the bridge golden parses");
        let (got, want) = (got.as_array().expect("steps"), want.as_array().expect("steps"));
        assert_eq!(got.len(), want.len(), "a different number of frames");
        for (g, w) in got.iter().zip(want) {
            assert_eq!(g, w, "the engine holds something else after `{}`", w["step"]);
        }
    }

    // ── the export goldens ─────────────────────────────────────────────────

    /// Seeded stereo noise, a second of it, written as a float WAV the
    /// export decodes for itself.
    fn noise_wav(path: &Path, seed: u32, level: f32) {
        let spec = tangent_record::wav::WavSpec {
            sample_rate: RATE,
            channels: 2,
            format: tangent_record::wav::SampleFormat::Float32,
        };
        let bext = tangent_record::wav::Bext::new(tangent_record::wav::Wallclock::from_unix(0, 0), spec);
        let mut w = tangent_record::wav::WavWriter::create(path, spec, &bext).expect("a noise file");
        let mut x: u32 = seed | 1;
        let mut samples = Vec::with_capacity(RATE as usize * 2);
        for i in 0..RATE as usize {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let n = (x as f32 / u32::MAX as f32).mul_add(2.0, -1.0);
            // A decaying burst every quarter second, so a pad, a key and a
            // compressor all have edges to act on.
            let env = 1.0 - (i % (RATE as usize / 4)) as f32 / (RATE as f32 / 4.0);
            samples.push(n * level * env);
            samples.push(n * level * 0.8 * env);
        }
        w.write_interleaved(&samples).expect("its audio");
        w.finish().expect("closed");
    }

    fn audio(lane: usize, file: &str, start_s: f64) -> ArrClip {
        ArrClip {
            kind: ClipKind::Audio,
            lane,
            path: format!("media/{file}"),
            start_s,
            len_s: 1.0,
            gain: 1.0,
            ..Default::default()
        }
    }

    fn midi(lane: usize, keys: &[(f64, u8)]) -> ArrClip {
        ArrClip {
            kind: ClipKind::Midi,
            lane,
            start_s: 0.0,
            len_s: 1.5,
            notes: keys
                .iter()
                .map(|&(at_s, key)| ArrNote { at_s, dur_s: 0.3, key, vel: 100, muted: false })
                .collect(),
            ..Default::default()
        }
    }

    fn curve(lane: usize, param: AutoParam, points: &[(f64, f64, Shape)]) -> Curve {
        Curve::sanitised(
            lane,
            param,
            points.iter().map(|&(at_s, value, shape)| Point { at_s, value, shape }).collect(),
        )
    }

    /// A built-in's state blob with some of its parameters dialled away
    /// from the defaults, so an export that seats it at defaults is a
    /// different file.
    fn dialled(unit: &str, values: &[(usize, f32)]) -> Vec<u8> {
        let spec = crate::builtin_fx::spec_for(unit).expect("a built-in");
        let mut v = spec.defaults();
        for &(i, x) in values {
            v[i] = x;
        }
        crate::builtin_fx::state_text(spec, &v, &[]).into_bytes()
    }

    /// One desk of the export corpus: its settings, its document, the bays
    /// the LIVE engine is wearing (the export takes its knobs from there),
    /// and the lanes to freeze.
    struct Corpus {
        name: &'static str,
        settings: Settings,
        doc: Arrangement,
        live: Vec<(usize, usize, &'static str, Vec<u8>)>,
        kits: Vec<(usize, Vec<(usize, &'static str)>)>,
        freeze: Vec<usize>,
    }

    fn corpus() -> Vec<Corpus> {
        let mut out = Vec::new();

        // Faders, pans, a mute, the master, the send to Return 0's built-ins.
        let mut s = blank();
        kind(&mut s, &[0, 1, 2, 3], ChannelKind::Audio);
        s.channel_gains[..4].copy_from_slice(&[0.8, 0.6, 0.9, 0.7]);
        s.channel_pans[..4].copy_from_slice(&[-0.5, 0.3, 0.0, 0.8]);
        s.strip_muted = 1 << 3;
        s.strip_sends[0] = 0.5;
        s.strip_sends[1] = 0.25;
        s.reverb_mix = 0.6;
        s.delay_mix = 0.3;
        s.master_gain = 0.9;
        let mut doc = Arrangement::default();
        for (lane, at) in [(0, 0.0), (1, 0.25), (2, 0.5), (3, 0.0)] {
            doc.add_clip(audio(lane, &format!("n{lane}.wav"), at));
        }
        out.push(Corpus { name: "faders-sends", settings: s, doc, live: vec![], kits: vec![], freeze: vec![1] });

        // Dialled racks, a bypassed bay, polarity.
        let mut s = blank();
        kind(&mut s, &[0, 1, 2], ChannelKind::Audio);
        bay(&mut s, 0, 0, EQ_PATH);
        bay(&mut s, 1, 0, COMP_PATH);
        bay(&mut s, 1, 1, CHORUS_PATH);
        bay(&mut s, 2, 0, PLATE_PATH);
        s.insert_bypass.0.push((1, 1));
        s.strip_phase = 1 << 0;
        let mut doc = Arrangement::default();
        for lane in 0..3 {
            doc.add_clip(audio(lane, &format!("n{lane}.wav"), 0.0));
        }
        let live = vec![
            (0, 0, EQ_PATH, dialled(EQ_PATH, &[(0, 0.8), (1, 0.3)])),
            (1, 0, COMP_PATH, dialled(COMP_PATH, &[(0, 0.2), (1, 0.9)])),
            (1, 1, CHORUS_PATH, dialled(CHORUS_PATH, &[(0, 0.7)])),
            (2, 0, PLATE_PATH, dialled(PLATE_PATH, &[(0, 0.6)])),
        ];
        out.push(Corpus { name: "racks", settings: s, doc, live, kits: vec![], freeze: vec![1] });

        // A folder with its own clip, a channel route into a racked lane, a
        // hardware route, a sidechain key, a solo inside the folder.
        let mut s = blank();
        kind(&mut s, &[0, 1, 2, 3, 4, 5], ChannelKind::Audio);
        s.strip_is_folder[1] = true;
        s.strip_in_folder[2] = true;
        s.strip_in_folder[3] = true;
        s.routes.add(Route::new(4, Target::Channel(5)));
        s.routes.add(Route::new(0, Target::Hardware { first: 0, stereo: true }));
        bay(&mut s, 5, 0, EQ_PATH);
        bay(&mut s, 3, 0, COMP_PATH);
        s.insert_keys.set(3, 0, Some(2));
        s.strip_soloed = 1 << 2 | 1 << 4 | 1 << 5;
        let mut doc = Arrangement::default();
        for lane in 0..5 {
            doc.add_clip(audio(lane, &format!("n{lane}.wav"), 0.1 * lane as f64));
        }
        let live = vec![
            (5, 0, EQ_PATH, dialled(EQ_PATH, &[(0, 0.2)])),
            (3, 0, COMP_PATH, dialled(COMP_PATH, &[(0, 0.1), (1, 1.0)])),
        ];
        out.push(Corpus { name: "folders-routes-key", settings: s, doc, live, kits: vec![], freeze: vec![4] });

        // Every instrument the desk has: DX7, Keys, drums with a kit, the
        // sampler.
        let mut s = blank();
        kind(&mut s, &[0, 1, 2, 3], ChannelKind::Midi);
        bay(&mut s, 0, 0, tangent_ui::dialogs::BUILTIN_PATH);
        bay(&mut s, 1, 0, tangent_ui::dialogs::KEYS_PATH);
        bay(&mut s, 2, 0, tangent_ui::dialogs::DRUMPAD_PATH);
        bay(&mut s, 3, 0, SAMPLER_PATH);
        s.strip_dx7_patch[0] = 3;
        s.strip_keys_patch[1] = 2;
        let mut doc = Arrangement::default();
        doc.add_clip(midi(0, &[(0.0, 60), (0.4, 64), (0.8, 67)]));
        doc.add_clip(midi(1, &[(0.1, 57), (0.5, 60)]));
        doc.add_clip(midi(2, &[(0.0, 36), (0.25, 38), (0.5, 36), (0.75, 38)]));
        doc.add_clip(midi(3, &[(0.2, 60), (0.6, 67)]));
        let sampler = {
            let spec = crate::builtin_fx::spec_for(SAMPLER_PATH).expect("the sampler");
            let extra = vec![("sample_path".to_owned(), "media/tone.wav".to_owned())];
            crate::builtin_fx::state_text(spec, &spec.defaults(), &extra).into_bytes()
        };
        let live = vec![(3, 0, SAMPLER_PATH, sampler)];
        let kits = vec![(2, vec![(36, "kick.wav"), (38, "snare.wav")])];
        out.push(Corpus { name: "instruments", settings: s, doc, live, kits, freeze: vec![0] });

        // Automation on gain, pan, send, mute, the master and a built-in.
        let mut s = blank();
        kind(&mut s, &[0, 1], ChannelKind::Audio);
        bay(&mut s, 0, 0, TRIM_PATH);
        s.reverb_mix = 0.5;
        let mut doc = Arrangement::default();
        doc.add_clip(audio(0, "n0.wav", 0.0));
        doc.add_clip(audio(1, "n1.wav", 0.0));
        let trim = crate::builtin_fx::spec_for(TRIM_PATH).expect("trim").params[0].id;
        doc.automation = Automation::from_curves(vec![
            curve(0, AutoParam::Gain, &[(0.0, 0.2, Shape::Linear), (0.6, 0.9, Shape::Linear)]),
            curve(1, AutoParam::Pan, &[(0.0, 0.0, Shape::Linear), (0.6, 1.0, Shape::Linear)]),
            curve(0, AutoParam::Send, &[(0.0, 0.0, Shape::Linear), (0.5, 1.0, Shape::Linear)]),
            curve(1, AutoParam::Mute, &[(0.0, 0.0, Shape::Hold), (0.4, 1.0, Shape::Hold)]),
            curve(MASTER_AUTO_LANE, AutoParam::Gain, &[(0.0, 0.9, Shape::Linear), (0.8, 0.4, Shape::Linear)]),
            curve(0, AutoParam::Plugin { bay: 0, id: trim }, &[(0.0, 0.2, Shape::Linear), (0.7, 0.9, Shape::Linear)]),
        ]);
        out.push(Corpus { name: "automation", settings: s, doc, live: vec![], kits: vec![], freeze: vec![0] });

        // A frozen lane: its file plays, its clip and its rack stand down.
        let mut s = blank();
        kind(&mut s, &[0, 1], ChannelKind::Audio);
        bay(&mut s, 1, 0, COMP_PATH);
        s.frozen_lanes.push(FrozenLane { lane: 1, path: "media/frozen-02.wav".into(), len_s: 1.0 });
        let mut doc = Arrangement::default();
        doc.add_clip(audio(0, "n0.wav", 0.0));
        doc.add_clip(audio(1, "n1.wav", 0.0));
        let live = vec![(1, 0, COMP_PATH, dialled(COMP_PATH, &[(0, 0.1)]))];
        out.push(Corpus { name: "frozen", settings: s, doc, live, kits: vec![], freeze: vec![] });

        out
    }

    /// Write `c`'s project into `dir` (its document, every file a clip, pad
    /// or sampler names, its kits) and open it in a free app.
    fn open(c: &Corpus, dir: &Path) -> TangentApp {
        let media = dir.join("media");
        for (i, f) in ["n0.wav", "n1.wav", "n2.wav", "n3.wav", "n4.wav", "tone.wav", "kick.wav", "snare.wav", "frozen-02.wav"]
            .iter()
            .enumerate()
        {
            noise_wav(&media.join(f), 0x9e37 + i as u32 * 7919, 0.3);
        }
        let doc_path = tangent_ui::arrangement::doc_in(dir);
        c.doc.save_to(&doc_path);
        if !c.kits.is_empty() {
            let mut kits: Vec<tangent_ui::drumpad::DrumpadDoc> =
                (0..CHANNELS).map(|_| tangent_ui::drumpad::DrumpadDoc::default()).collect();
            for (lane, pads) in &c.kits {
                for &(pad, file) in pads {
                    kits[*lane].pads[pad].path = format!("media/{file}");
                }
            }
            tangent_ui::drumpad::DrumpadDoc::save_all_beside(&kits, c.kits[0].0, &doc_path);
        }
        let mut app = app_with(c.settings.clone());
        app.open_project(&doc_path);
        app
    }

    /// The live engine the export reads its knobs from.
    fn live_engine(c: &Corpus) -> Engine {
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        for (seat, bay, unit, state) in &c.live {
            e.load_insert(*seat, *bay, Some(Path::new(unit)), Some(state))
                .unwrap_or_else(|err| panic!("{unit} did not load into seat {seat}: {err}"));
        }
        e
    }

    /// The DX7's voice for a strip, from the factory bank: the export's
    /// cartridge cache handed in, and a disk with no staged bank on it.
    fn factory_voice(app: &TangentApp, strip: usize) -> crate::dx7::Voice {
        let c = crate::dx7::sysex::factory();
        let (_, patch) = app.dx7_choice_of(strip);
        c.voices.get(patch).or_else(|| c.voices.first()).copied().unwrap_or_default()
    }

    /// The export's furnishing, as `start_offline_render` did it at the
    /// base commit.
    fn base_furnish(
        app: &TangentApp,
        rate: u32,
        live: Option<&Engine>,
        voice: &mut dyn FnMut(usize) -> crate::dx7::Voice,
        freeze: Option<usize>,
        spec: &tangent_ui::ports::ProjectExportSpec,
    ) -> Engine {
        let mut e = Engine::offline(rate, Timebase::new()).expect("an offline engine");
        e.set_offline_fanout(true);
        {
            let mut desk = app.desk();
            if freeze.is_some() {
                desk.muted.iter_mut().for_each(|m| *m = false);
                desk.soloed.iter_mut().for_each(|m| *m = false);
            }
            e.set_desk(&desk);
        }
        let gains = app.gains();
        for ch in 0..CHANNELS {
            e.set_channel_gain(ch, gains.channels[ch]);
            e.set_channel_pan(ch, app.channel_pan(ch));
        }
        if let Some(lane) = freeze {
            e.set_channel_gain(lane, 1.0);
            e.set_channel_pan(lane, 0.0);
        }
        e.set_midi_off(u64::MAX);
        e.set_armed(0);
        e.set_monitor_on(false);
        e.set_drums(app.drums_gain(), app.drums_pan());
        base_publish_routes(app, &e);
        e.set_master_gain(gains.master);
        e.set_fx_return(gains.fx_return);
        let [reverb, delay, chorus, phaser, hpf, lpf, gain, limiter] = app.effect_sends();
        e.set_effects(crate::effects::Sends { reverb, delay, chorus, phaser, hpf, lpf, gain, limiter });
        e.set_effect_params(effect_params_from(app.effect_params()));
        e.set_varispeed(app.varispeed_ratio());
        e.set_metronome_enabled(false);
        e.set_tempo(app.tempo_bpm());
        {
            let sig = app.time_signature();
            e.set_meter(u32::from(sig.beats), u32::from(sig.unit));
            e.set_metronome_div(app.metronome_div());
            e.set_strip_phase(app.strip_phase());
        }
        e.set_maps(crate::instrument::Maps { tempo: app.live_tempo_map(), meter: app.live_meter_map() });
        e.set_automation(match freeze {
            Some(lane) => {
                let mut out = Automation::default();
                for c in app.engine_automation().curves() {
                    let fader = c.lane == lane
                        && matches!(c.param, AutoParam::Gain | AutoParam::Pan | AutoParam::Mute);
                    if !fader {
                        out.set(c.clone());
                    }
                }
                out
            }
            None => app.engine_automation(),
        });
        let thawed = !frozen_lane_mask(app.frozen_lanes());
        let builtin_lanes: Vec<usize> = (0..CHANNELS)
            .filter(|ch| app.insert(*ch, 0) == Some(tangent_ui::dialogs::BUILTIN_PATH))
            .collect();
        e.set_builtin_strips(builtin_lanes.iter().fold(0u32, |m, ch| m | 1 << ch.min(&31)) & thawed);
        let drum_lanes: Vec<usize> = (0..CHANNELS)
            .filter(|ch| app.insert(*ch, 0) == Some(tangent_ui::dialogs::DRUMPAD_PATH))
            .collect();
        e.set_drum_strips(drum_lanes.iter().fold(0u32, |m, ch| m | 1 << ch.min(&31)));
        let keys_lanes: Vec<usize> = (0..CHANNELS)
            .filter(|ch| app.insert(*ch, 0) == Some(tangent_ui::dialogs::KEYS_PATH))
            .collect();
        e.set_keys_strips(keys_lanes.iter().fold(0u32, |m, ch| m | 1 << ch.min(&31)) & thawed);
        for &ch in &keys_lanes {
            e.set_keys_patch(ch, keys_patch_of(app.keys_patch_of(ch)));
            e.set_keys_params(ch, &app.keys_params_of(ch));
        }
        e.set_folders(app.folder_parents());
        let (made_dx7, _) = e.sync_instrument_lanes(
            builtin_lanes.iter().fold(0u32, |m, ch| m | 1 << ch.min(&31)),
            keys_lanes.iter().fold(0u32, |m, ch| m | 1 << ch.min(&31)),
        );
        for ch in 0..CHANNELS.min(32) {
            if made_dx7 & (1 << ch) != 0 {
                let v = voice(ch);
                e.set_builtin_voice(ch, v);
            }
        }
        let inserts = INSERTS;
        for i in 0..(crate::instrument::RACK_SEATS * inserts) {
            let seat = i / inserts;
            let bay = i % inserts;
            let want = app.insert(seat, bay).map(str::to_owned);
            let engine_want = want.as_deref().filter(|p| !is_instrument_sentinel(p));
            let Some(path) = engine_want else { continue };
            furnish_mirror_bay(&mut e, live, seat, bay, path).expect("a built-in seats");
        }
        if let Some(doc_path) = app.clip_doc() {
            let (placed, midi_placed, unread) =
                export_clip_placement(app.arrangement(), app.frozen_lanes(), rate, |rel| {
                    Arrangement::resolve(&doc_path, rel)
                });
            assert!(unread.is_empty(), "the corpus names a file it did not write: {unread:?}");
            e.set_clips(placed);
            e.set_midi_clips(midi_placed);
        }
        {
            for lane in (0..CHANNELS)
                .filter(|ch| app.insert(*ch, 0) == Some(tangent_ui::dialogs::DRUMPAD_PATH))
            {
                let mut kit = crate::instrument::PadKit::default();
                for (i, path) in app.drumpad_paths_of(lane).iter().enumerate() {
                    let Some(path) = path else { continue };
                    if let (Ok(c), Some(slot)) = (tangent_record::decode::decode(path, rate), kit.pads.get_mut(i)) {
                        *slot = Some(Arc::new(c));
                    }
                }
                e.set_pad_kit(lane, kit);
            }
            for lane in (0..CHANNELS)
                .filter(|ch| app.insert(*ch, 0) == Some(tangent_ui::dialogs::DRUMPAD_PATH))
            {
                e.set_pad_params(lane, &app.drum_kit_params_of(lane));
            }
        }
        mirror_sampler_samples(&mut e, rate, app.kit_doc().as_deref());
        arm_stem_tap(&e, spec.stems, spec.stems_dry);
        e
    }

    /// One export's files, rendered as `start_offline_render` names them and
    /// `tick_offline_bounce` writes them, into `dir`: `(name, bytes,
    /// whether any sample was non-zero)`.
    ///
    /// **The `bext` is pinned**: the writer stamps the wall clock and the
    /// build's version into every file, and neither is a fact about the
    /// export, so the golden would change with the date and with every
    /// release. Everything else in the file is the writer's own.
    fn render_files(
        mut e: Engine,
        app: &TangentApp,
        spec: &tangent_ui::ports::ProjectExportSpec,
        freeze: Option<usize>,
        dir: &Path,
    ) -> Vec<(String, Vec<u8>, bool)> {
        use tangent_record::wav::{Bext, SampleFormat, WavSpec, WavWriter, Wallclock};
        let rate = spec.rate;
        let span = app.arr_span_s();
        e.set_transport(1, 0);
        e.set_rolling(true);
        let wav_spec = WavSpec {
            sample_rate: rate,
            channels: if spec.mono { 1 } else { 2 },
            format: match spec.depth {
                tangent_ui::ports::ExportDepth::Pcm16 => SampleFormat::Int16,
                tangent_ui::ports::ExportDepth::Pcm24 => SampleFormat::Int24,
                tangent_ui::ports::ExportDepth::Float32 => SampleFormat::Float32,
            },
        };
        let mut bext = Bext::new(Wallclock::from_unix(0, 0), wav_spec);
        bext.originator = "Tangent golden".to_owned();
        bext.coding_history = String::new();
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).expect("the export dir");
        let mut files: Vec<(String, PathBuf, WavWriter, usize, bool)> = Vec::new();
        if spec.master {
            let name = format!("{}.wav", sanitize_file_stem(&spec.name));
            let path = dir.join(&name);
            files.push((name, path.clone(), WavWriter::create(&path, wav_spec, &bext).expect("master"), usize::MAX, false));
        }
        if let Some(lane) = freeze {
            let name = format!("frozen-{:02}.wav", lane + 1);
            let path = dir.join(&name);
            files.push((name, path.clone(), WavWriter::create(&path, wav_spec, &bext).expect("freeze"), lane, false));
        } else if spec.stems {
            std::fs::create_dir_all(dir.join(spec.stems_folder())).expect("stems dir");
            for lane in 0..CHANNELS {
                let sounds = app.arrangement().clips.iter().any(|c| c.lane == lane)
                    || (0..INSERTS).any(|b| app.insert(lane, b).is_some());
                if !sounds {
                    continue;
                }
                let name: String = app
                    .channel_display_name(lane)
                    .chars()
                    .map(|c| if std::path::is_separator(c) { '-' } else { c })
                    .collect();
                let name = format!("{}/{:02}-{}.wav", spec.stems_folder(), lane + 1, name.trim());
                let path = dir.join(&name);
                files.push((name, path.clone(), WavWriter::create(&path, wav_spec, &bext).expect("stem"), lane, false));
            }
        }
        let ratio = f64::from(app.varispeed_ratio()).clamp(0.5, 2.0);
        let until = (((span + 2.0) / ratio) * f64::from(rate)).ceil() as u64;
        let wide = CHANNELS * 2;
        let mut block = vec![0.0f32; 1024 * 2];
        let mut scratch = Vec::new();
        let mut fold = Vec::new();
        let mut skip: Option<u64> = None;
        let mut done = 0u64;
        while done < until {
            let n = 1024usize.min((until - done) as usize);
            let out = &mut block[..n * 2];
            out.fill(0.0);
            e.render_offline(out);
            if let Some(lane) = freeze {
                if skip.is_none() {
                    skip = Some(u64::from(e.offline_stem_lead(lane)));
                }
            }
            scratch.clear();
            e.drain_stems(&mut scratch);
            let frames = scratch.len() / wide;
            for (_, _, w, lane, loud) in &mut files {
                fold.clear();
                if *lane == usize::MAX {
                    *loud |= out.iter().any(|v| *v != 0.0);
                    if spec.mono {
                        for f in 0..n {
                            fold.push((out[f * 2] + out[f * 2 + 1]) * 0.5);
                        }
                        w.write_interleaved(&fold).expect("master");
                    } else {
                        w.write_interleaved(out).expect("master");
                    }
                    continue;
                }
                for f in 0..frames {
                    let l = scratch[f * wide + *lane * 2];
                    let r = scratch[f * wide + *lane * 2 + 1];
                    if spec.mono {
                        fold.push((l + r) * 0.5);
                    } else {
                        fold.push(l);
                        fold.push(r);
                    }
                }
                *loud |= fold.iter().any(|v| *v != 0.0);
                let mut drop = 0usize;
                if freeze == Some(*lane) {
                    let left = skip.unwrap_or(0);
                    drop = usize::try_from(left).unwrap_or(usize::MAX).min(frames);
                    skip = Some(left - drop as u64);
                }
                let per = if spec.mono { 1 } else { 2 };
                w.write_interleaved(&fold[drop * per..]).expect("stem");
            }
            done += n as u64;
        }
        files
            .into_iter()
            .map(|(name, path, w, _, loud)| {
                w.finish().expect("closed");
                (name, std::fs::read(&path).expect("written"), loud)
            })
            .collect()
    }

    /// Every pass of every corpus desk: the master, the wet stems, the dry
    /// stems, and each named freeze, keyed `desk/pass/file`.
    fn export_corpus() -> BTreeMap<String, (String, bool)> {
        let mut all = BTreeMap::new();
        for c in corpus() {
            let dir = scratch(c.name);
            let app = open(&c, &dir);
            let live = live_engine(&c);
            let base = tangent_ui::ports::ProjectExportSpec { rate: RATE, ..Default::default() };
            let mut passes: Vec<(String, tangent_ui::ports::ProjectExportSpec, Option<usize>)> = vec![
                ("master".to_owned(), base.clone(), None),
                ("stems".to_owned(), tangent_ui::ports::ProjectExportSpec { master: false, stems: true, ..base.clone() }, None),
                (
                    "stems-dry".to_owned(),
                    tangent_ui::ports::ProjectExportSpec { master: false, stems: true, stems_dry: true, ..base.clone() },
                    None,
                ),
            ];
            for &lane in &c.freeze {
                passes.push((
                    format!("freeze-{lane}"),
                    tangent_ui::ports::ProjectExportSpec {
                        master: false,
                        stems: true,
                        stems_dry: false,
                        depth: tangent_ui::ports::ExportDepth::Float32,
                        name: "freeze".to_owned(),
                        ..base.clone()
                    },
                    Some(lane),
                ));
            }
            for (pass, spec, freeze) in passes {
                let mut voice = |strip: usize| factory_voice(&app, strip);
                let e = base_furnish(&app, RATE, Some(&live), &mut voice, freeze, &spec);
                for (name, bytes, loud) in render_files(e, &app, &spec, freeze, &dir.join("out").join(&pass)) {
                    all.insert(format!("{}/{pass}/{name}", c.name), (fnv(&bytes), loud));
                }
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
        all
    }

    /// **The export goldens**: every corpus desk exported through the
    /// furnishing, rendered offline and written by the WAV writer, each
    /// file's bytes hashed against `tests/fixtures/exports/golden.json`,
    /// file names included. aarch64 macOS only, like the render goldens:
    /// floats are deterministic per platform, not across them.
    #[test]
    #[cfg_attr(
        not(all(target_arch = "aarch64", target_os = "macos")),
        ignore = "the hashes are aarch64 macOS floats"
    )]
    fn every_corpus_export_is_the_file_it_was() {
        let got: BTreeMap<String, String> =
            export_corpus().into_iter().map(|(k, (h, _))| (k, h)).collect();
        let path = fixture("golden.json");
        if writing() {
            std::fs::write(&path, serde_json::to_string_pretty(&got).expect("json") + "\n")
                .expect("the export golden");
            return;
        }
        let want: BTreeMap<String, String> = serde_json::from_str(
            &std::fs::read_to_string(&path).expect("tests/fixtures/exports/golden.json"),
        )
        .expect("the export golden parses");
        let names = |m: &BTreeMap<String, String>| m.keys().cloned().collect::<Vec<_>>();
        assert_eq!(names(&got), names(&want), "the export writes other files");
        let changed: Vec<&String> = want.iter().filter(|(k, v)| got.get(*k) != Some(*v)).map(|(k, _)| k).collect();
        assert!(changed.is_empty(), "these exports are not the files they were: {changed:?}");
    }

    /// **The corpus is not vacuous**, on every platform: every master and
    /// every freeze has sound in it, and so does every stem of a lane that
    /// is heard. A golden of silence would pin nothing.
    #[test]
    fn every_corpus_export_has_sound_in_it() {
        let all = export_corpus();
        assert!(all.len() > 30, "the corpus shrank to {} files", all.len());
        let silent: Vec<&String> = all
            .iter()
            .filter(|(k, (_, loud))| !loud && (k.contains("/master/") || k.contains("/freeze-")))
            .map(|(k, _)| k)
            .collect();
        assert!(silent.is_empty(), "silent masters or freezes: {silent:?}");
        let stems_heard = all.iter().filter(|(k, (_, loud))| k.contains("/stems") && *loud).count();
        assert!(stems_heard >= 20, "only {stems_heard} stems carry sound");
    }
}
