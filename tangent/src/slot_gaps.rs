//! **Track 33 is every bit a track** (design 13, W7-slot-gaps), end to end:
//! a desk two hundred and one rows wide, its subjects on rows 1, 31, 32, 33,
//! 40 and 200, each given the same part, and each part made to behave on
//! every subject as it does on row 1, LIVE (an engine driven by the
//! [`Bridge`], its lanes on their rows' slots, furnished down the host's
//! own roads) and in an EXPORT (`full_build_with`, the one builder an
//! export, a freeze and the probe come through, handed the live engine as
//! the host hands it). The engine's own doors are held at the same widths
//! by `instrument::slot_engine::gaps`; what this file adds is every road
//! from the desk to them.
//!
//! The parts: a drum lane's pad effect, a sampler bay's file, a built-in
//! face (its push, its knobs and the automation's overlay on it), a
//! sidechain key, and a strip's send knob as its Return 0 entry. And the
//! scans that keep the host off the frozen map's seats, which is what made
//! the faces' roads stop at the thirty-second row.
//!
//! **Nothing here touches the real home**: every licence store is a temp
//! directory of the test's own (`composite::tiers`), every project is under
//! the temp dir, and the apps run with the global settings file off
//! (`persist_global_settings`), as `konami_e2e`'s do.
//!
//! [`Bridge`]: crate::engine_bridge::Bridge

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tangent_record::audio::Timebase;
use tangent_ui::app::TangentApp;
use tangent_ui::arrangement::{ArrClip, ArrNote, Arrangement, ClipKind};
use tangent_ui::desk_id::{Node, Rack, SeatBay, TrackId};
use tangent_ui::desk_model::{RowKind, TrackRow};
use tangent_ui::dialogs::DialogAction;
use tangent_ui::host::Caps;
use tangent_ui::recorder::{RecorderRequest, StripRef};
use tangent_ui::settings::Settings;

use crate::builtin_fx::{COMP_PATH, EQ_PATH, SAMPLER_PATH, TRIM_PATH};
use crate::composite::tiers;
use crate::desk_plan::{SendFrom, SendTo, Slot};
use crate::engine_bridge::{full_build_with, BuildFor, Bridge, Built, Furnish};
use crate::instrument::{Engine, LanePart, PadKit};

const RATE: u32 = 48_000;

/// The desktop's capabilities with the one write a binary crate's test may
/// not make taken out: the settings file here is the real one.
const QUIET: Caps = Caps {
    persist_global_settings: false,
    ..Caps::DESKTOP
};

/// Row 1, the reference, then the rows the design names: either side of the
/// frozen map's edge, past it, and far past it. On a fresh desk each row's
/// lane is the row's own slot, live and in the export (`live` checks).
const SUBJECTS: [usize; 6] = [1, 31, 32, 33, 40, 200];

/// The desk's width: row 200 is its last.
const ROWS: usize = 201;

/// A folder of this test's own under the temp dir, emptied, with a `media/`.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tangent-slot-gaps-{}-{:?}-{tag}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("media")).expect("a scratch project");
    dir
}

/// A second of a 440 Hz stereo tone at `level` as a float WAV: steady, or
/// with `burst` a decaying burst every quarter second, so a compressor and
/// its key have edges to act on.
fn wav(path: &Path, level: f32, burst: bool) {
    use tangent_record::wav::{Bext, SampleFormat, WavSpec, WavWriter, Wallclock};
    let spec = WavSpec {
        sample_rate: RATE,
        channels: 2,
        format: SampleFormat::Float32,
    };
    let bext = Bext::new(Wallclock::from_unix(0, 0), spec);
    let mut w = WavWriter::create(path, spec, &bext).expect("a file");
    let quarter = RATE as usize / 4;
    let mut samples = Vec::with_capacity(RATE as usize * 2);
    for i in 0..RATE as usize {
        let env = if burst { 1.0 - (i % quarter) as f32 / quarter as f32 } else { 1.0 };
        let x = level * env * (std::f32::consts::TAU * 440.0 * i as f32 / RATE as f32).sin();
        samples.push(x);
        samples.push(x);
    }
    w.write_interleaved(&samples).expect("its audio");
    w.finish().expect("closed");
}

/// **A bare desk `ROWS` wide**: every row vacant and no bay seated, so each
/// subject below is exactly what its test made it; no welcome card.
fn wide_desk() -> Settings {
    let mut s = Settings {
        show_welcome: false,
        ..Settings::default()
    };
    s.plugins.clear();
    for row in &mut s.rows {
        *row = TrackRow::vacant(row.id);
    }
    let mut next = s.next_track_id;
    while s.rows.len() < ROWS {
        s.rows.push(TrackRow::vacant(TrackId(next as u32)));
        next += 1;
    }
    s.next_track_id = next;
    s.arm_unsaid = false;
    s
}

/// Bay `bay` of rack `rack` of the track on row `row` of `s`.
fn bay_of(s: &Settings, row: usize, rack: Rack, bay: u8) -> SeatBay {
    SeatBay::new(Node::Track(s.rows[row].id), rack, bay)
}

/// A built-in's state with some of its knobs set and some extra lines.
fn state(path: &str, values: &[(usize, f32)], extra: &[(&str, &str)]) -> Vec<u8> {
    let spec = crate::builtin_fx::spec_for(path).expect("a built-in");
    let mut v = spec.defaults();
    for &(i, x) in values {
        v[i] = x;
    }
    let extra: Vec<(String, String)> = extra.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    crate::builtin_fx::state_text(spec, &v, &extra).into_bytes()
}

/// A MIDI clip on `row` holding `key` from the top for most of a second.
fn notes(row: usize, key: u8) -> ArrClip {
    ArrClip {
        kind: ClipKind::Midi,
        lane: row,
        start_s: 0.0,
        len_s: 1.0,
        notes: vec![ArrNote { at_s: 0.0, dur_s: 0.9, key, vel: 110, muted: false }],
        ..Default::default()
    }
}

/// An audio clip on `row` playing `media/<file>` from the top.
fn audio(row: usize, file: &str) -> ArrClip {
    ArrClip {
        kind: ClipKind::Audio,
        lane: row,
        path: format!("media/{file}"),
        start_s: 0.0,
        len_s: 1.0,
        gain: 1.0,
        ..Default::default()
    }
}

/// **A paid install on `settings`, with `doc` saved into `dir` and opened**
/// (the document carries no desk of its own, so the desk is the settings').
fn open(tag: &str, settings: Settings, doc: &Arrangement, dir: &Path) -> TangentApp {
    let path = tangent_ui::arrangement::doc_in(dir);
    doc.save_to(&path);
    open_saved(tag, settings, &path)
}

/// [`open`] on a document already saved at `path`.
fn open_saved(tag: &str, settings: Settings, path: &Path) -> TangentApp {
    let ctx = egui::Context::default();
    let mut app = TangentApp::with_license(&ctx, settings, QUIET, tiers::supporter(tag));
    app.open_project(path);
    app
}

/// **The live engine, as the host furnishes it**: the bridge's shape (each
/// row's lane on its own slot, which the subjects' slots depend on), each
/// drum lane's own kit and knobs (`reload_pads`' road and the frame's pad
/// push), and the document's clips, placed as the export places them and
/// stamped with their lanes by the bridge as `reload_clips` stamps them.
/// Its bays are seated by the caller, as the reconcile seats them
/// ([`seat`]).
fn live(app: &TangentApp) -> (Engine, Bridge) {
    let mut e = Engine::offline(RATE, Timebase::new()).expect("a live engine");
    let mut p = Bridge::new();
    p.sync(app, &mut e);
    for (row, id) in app.track_ids().iter().enumerate() {
        let at = p.lane_of(*id).map(|l| l.slot);
        assert_eq!(at, Some(Slot(row as u32)), "fixture: row {row}'s lane is not its own slot");
    }
    for (row, lane) in p.lanes_holding(app, tangent_ui::dialogs::DRUMPAD_PATH) {
        let mut kit = PadKit::default();
        for (i, path) in app.drumpad_paths_of(row).iter().enumerate() {
            let Some(path) = path else { continue };
            let clip = tangent_record::decode::decode(path, RATE).unwrap_or_else(|err| panic!("row {row}'s pad {i}: {err}"));
            kit.pads[i] = Some(Arc::new(clip));
        }
        e.attach(lane.slot, lane.gen, LanePart::Kit(Box::new(kit))).expect("the kit goes");
        e.set_pad_params(lane.slot, &app.drum_kit_params_of(row));
    }
    let doc = app.clip_doc().expect("a project");
    let (placed, midi, unread) = crate::desktop::export_clip_placement(app.arrangement(), &app.frozen_lanes(), RATE, |rel| {
        Arrangement::resolve(&doc, rel)
    });
    assert!(unread.is_empty(), "the live engine could not read {unread:?}");
    e.set_clips_slotted(p.slot_clips(placed));
    e.set_midi_slotted(p.slot_midi(midi));
    e.settle_offline();
    (e, p)
}

/// **Seat `path` with `blob` in bay `sb` of the live engine**, by the road
/// the reconcile takes (`bays_wanted`: the bay's identity through the
/// bridge to its rack; then `load_insert_at`), and land it. A bay whose
/// rack the engine has no seat for loads as `Ok(None)` there, and nothing
/// is seated: that is a failure here.
fn seat(e: &mut Engine, p: &Bridge, sb: SeatBay, path: &str, blob: &[u8]) {
    let (rack, bay) = p.engine_bay(sb).unwrap_or_else(|| panic!("{sb}: no engine bay"));
    e.load_insert_at(rack, bay, Some(Path::new(path)), Some(blob))
        .unwrap_or_else(|err| panic!("{sb}: {path} did not load: {err}"))
        .unwrap_or_else(|| panic!("{sb}: {path} was seated nowhere"));
    e.settle_offline();
}

/// **The export's engine**: `full_build_with` handed the live engine and its
/// bridge, as the host hands them (`Furnish::live`), so each bay plays the
/// knobs the live one wears.
fn export(app: &TangentApp, live: (&Engine, &Bridge)) -> Built {
    let spec = tangent_ui::ports::ProjectExportSpec {
        rate: RATE,
        ..Default::default()
    };
    let mut ctx = Furnish {
        live: Some(live),
        ..Furnish::default()
    };
    let built = full_build_with(app, RATE, Timebase::new(), BuildFor::Export(spec), &mut ctx).expect("an export engine");
    assert!(ctx.refused.is_empty(), "the export refused {:?}", ctx.refused);
    assert!(ctx.unread.is_empty(), "the export could not read {:?}", ctx.unread);
    built
}

/// **What each of `rows` makes once everything has settled**: a fifth of a
/// second rolled from zero, the meters cleared, a fifth more read, each row
/// at its lane on that engine's bridge.
fn levels(e: &mut Engine, p: &Bridge, rows: &[usize]) -> Vec<f32> {
    let slots: Vec<Slot> = rows.iter().map(|r| p.lane(*r).expect("a row's lane").slot).collect();
    e.set_transport(1, 0);
    e.set_rolling(true);
    let mut out = vec![0.0f32; 1024 * 2];
    let mut roll = |e: &mut Engine| {
        for _ in 0..(RATE as usize / 5).div_ceil(1024) {
            out.fill(0.0);
            e.render_offline(&mut out);
        }
    };
    roll(e);
    for s in &slots {
        let _ = e.lane_reading(*s);
    }
    roll(e);
    slots
        .iter()
        .map(|s| {
            let ([a, b], _) = e.lane_reading(*s);
            a.max(b)
        })
        .collect()
}

fn cleanup(dir: &Path, tags: &[&str]) {
    let _ = std::fs::remove_dir_all(dir);
    for tag in tags {
        let _ = std::fs::remove_dir_all(tiers::dir(tag));
    }
}

// ── a drum lane's pad effects ───────────────────────────────────────────────

/// Row 2: a drum track like the subjects, with no pad effect on any desk.
const CONTROL: usize = 2;

/// **The kit desk**: every subject and [`CONTROL`] a drum track (the
/// drumpad in its first bay, its own kit in the project's sidecar), the
/// kit's pad 36 a steady tone struck by a clip; and with `trim`, a Trim at
/// its floor (-24 dB) on pad 36's first bay of every subject, seated live
/// as the reconcile seats it. Answers the app and the live engine.
fn kit_desk(tag: &str, dir: &Path, trim: bool) -> (TangentApp, Engine, Bridge) {
    wav(&dir.join("media").join("pad.wav"), 0.25, false);
    let mut s = wide_desk();
    let mut doc = Arrangement::default();
    let mut kits: Vec<tangent_ui::drumpad::DrumpadDoc> = (0..ROWS).map(|_| Default::default()).collect();
    for row in SUBJECTS.into_iter().chain([CONTROL]) {
        s.rows[row].kind = Some(RowKind::Midi);
        s.plugins.insert(bay_of(&s, row, Rack::Chain(0), 0), tangent_ui::dialogs::DRUMPAD_PATH.to_owned());
        if trim && row != CONTROL {
            s.plugins.insert(bay_of(&s, row, Rack::Pad(36), 0), TRIM_PATH.to_owned());
        }
        kits[row].pads[36].path = "media/pad.wav".to_owned();
        doc.add_clip(notes(row, tangent_ui::recorder::DRUM_PAD_BASE + 36));
    }
    let ids: Vec<TrackId> = s.rows.iter().map(|r| r.id).collect();
    let path = tangent_ui::arrangement::doc_in(dir);
    doc.save_to(&path);
    tangent_ui::drumpad::DrumpadDoc::save_all_beside_ids(&kits, &ids, SUBJECTS[0], &path);
    let app = open_saved(tag, s, &path);
    let (mut e, p) = live(&app);
    if trim {
        let floor = state(TRIM_PATH, &[(0, 0.0)], &[]);
        for row in SUBJECTS {
            let sb = SeatBay::new(Node::Track(app.track_ids()[row]), Rack::Pad(36), 0);
            seat(&mut e, &p, sb, TRIM_PATH, &floor);
        }
    }
    (app, e, p)
}

/// **A pad effect on a drum track past slot 31 is heard as on row 1, live
/// and in the export** (W7-slot-gaps item 1). A Trim at -24 dB on each
/// subject's pad 36 cuts that subject's kit by the 24 dB it cuts row 1's
/// by: on the live engine, the bay seated through the bridge as the
/// reconcile seats it, and on the export's, the bay furnished from the live
/// one's knobs. The control drum track, with no pad effect, is not moved.
/// Past slot 31 a pad's rack had no seat, so the bay was seated nowhere
/// live (`load_insert_at` answered `Ok(None)`), refused in the export, and
/// the kit played dry both ways.
#[test]
fn a_pad_effect_on_a_track_past_slot_31_is_heard_as_on_row_1_live_and_in_the_export() {
    let tags = ["gaps-pads-dry", "gaps-pads-cut"];
    let (dry_dir, cut_dir) = (scratch("pads-dry"), scratch("pads-cut"));
    let rows: Vec<usize> = SUBJECTS.into_iter().chain([CONTROL]).collect();
    let (app, mut e, p) = kit_desk(tags[0], &dry_dir, false);
    let (cut_app, mut cut_e, cut_p) = kit_desk(tags[1], &cut_dir, true);
    let Built { engine: mut x, bridge: xb } = export(&app, (&e, &p));
    let Built { engine: mut cx, bridge: cxb } = export(&cut_app, (&cut_e, &cut_p));
    let roads = [
        ("live", levels(&mut e, &p, &rows), levels(&mut cut_e, &cut_p, &rows)),
        ("export", levels(&mut x, &xb, &rows), levels(&mut cx, &cxb, &rows)),
    ];
    for (road, dry, cut) in &roads {
        let ratio = |i: usize| cut[i] / dry[i];
        for (i, row) in rows.iter().enumerate() {
            assert!(dry[i] > 0.05, "{road}: the kit on row {row} is silent ({})", dry[i]);
        }
        let reference = ratio(0);
        assert!((reference - 0.063).abs() < 0.01, "{road}: row 1's pad effect left its kit at {reference} of itself, not -24 dB");
        for (i, row) in SUBJECTS.iter().enumerate() {
            let r = ratio(i);
            assert!((r - reference).abs() <= 0.02 * reference, "{road}: row {row}'s pad effect left its kit at {r} of itself, row 1's at {reference}");
        }
        let control = ratio(rows.len() - 1);
        assert!((control - 1.0).abs() < 1e-3, "{road}: the control kit moved to {control} of itself");
    }
    cleanup(&dry_dir, &tags[..1]);
    cleanup(&cut_dir, &tags[1..]);
}

// ── a sampler bay's file ────────────────────────────────────────────────────

/// **A sampler bay plays its file on a track past slot 31 as on row 1, live
/// and in the export** (W7-slot-gaps item 2). Every subject a MIDI track
/// with the sampler in its first bay, its state naming `media/tone.wav`,
/// and a note at the root key. The host's walk over the engine's sampler
/// bays (`DesktopApp::sampler_bays_of`, which the live reload and the
/// export's `mirror_sampler_samples` both read) reaches every subject's
/// lane, and the lane keeps the file handed to it, so each plays at row 1's
/// level on both engines. The walk read the legacy thirty-two and the
/// renderer's file table was thirty-two long: past them the bay played
/// silence both ways.
#[test]
fn a_sampler_on_a_track_past_slot_31_plays_its_file_as_on_row_1_live_and_in_the_export() {
    const TAG: &str = "gaps-sampler";
    let dir = scratch("sampler");
    wav(&dir.join("media").join("tone.wav"), 0.5, false);
    let mut s = wide_desk();
    let mut doc = Arrangement::default();
    for row in SUBJECTS {
        s.rows[row].kind = Some(RowKind::Midi);
        s.plugins.insert(bay_of(&s, row, Rack::Chain(0), 0), SAMPLER_PATH.to_owned());
        doc.add_clip(notes(row, crate::instrument::SAMPLER_ROOT_KEY));
    }
    let app = open(TAG, s, &doc, &dir);
    let (mut e, p) = live(&app);
    let root = crate::instrument::SAMPLER_ROOT_KEY.to_string();
    let blob = state(SAMPLER_PATH, &[], &[("sample_path", "media/tone.wav"), ("root_key", &root)]);
    for row in SUBJECTS {
        let sb = SeatBay::new(Node::Track(app.track_ids()[row]), Rack::Chain(0), 0);
        seat(&mut e, &p, sb, SAMPLER_PATH, &blob);
    }
    // The live hand-off: the host's own walk over the engine's bays, each
    // file decoded and handed to its lane.
    crate::desktop::mirror_sampler_samples(&mut e, RATE, app.kit_doc().as_deref());
    e.settle_offline();
    let Built { engine: mut x, bridge: xb } = export(&app, (&e, &p));
    for (road, got) in [("live", levels(&mut e, &p, &SUBJECTS)), ("export", levels(&mut x, &xb, &SUBJECTS))] {
        let want = got[0];
        assert!(want > 0.05, "{road}: row 1's sampler is silent ({want})");
        for (row, level) in SUBJECTS.iter().zip(&got) {
            assert!((level - want).abs() <= 0.02 * want, "{road}: row {row}'s sampler played {level}, row 1's {want}");
        }
    }
    cleanup(&dir, &[TAG]);
}

// ── a built-in face, its knobs and the automation's overlay ─────────────────

/// The EQ's fifth node: its frequency (the curve's) and its gain (a hand's).
const FREQ: u32 = 22;
const GAIN: u32 = 23;

/// **What the host does with what the app asked of it**, for the face's
/// requests: a knob turned is set on the bay's unit through the bridge and
/// the face pushed back (the host's `SetInsertParam` arm); everything else
/// is dropped. Answers the `(bay, id, value)` of each knob turned.
fn serve(app: &mut TangentApp, e: &mut Engine, p: &Bridge) -> Vec<(SeatBay, u32, f32)> {
    let mut turned = Vec::new();
    while let Some(r) = app.take_recorder_request() {
        if let RecorderRequest::SetInsertParam { bay, id, value } = r {
            if let Some(at) = p.engine_bay(bay) {
                e.set_insert_param_at(at.0, at.1, id, value);
                crate::desktop::push_builtin_faces(app, e, (bay.seat, at.0));
            }
            turned.push((bay, id, value));
        }
    }
    turned
}

/// The value of knob `id` in `face`.
fn knob(face: &tangent_ui::ports::BuiltinFace, id: u32) -> Option<f32> {
    face.params.iter().find(|p| p.id == id).map(|p| p.value)
}

/// **A built-in face on a track past slot 31 draws, takes its knobs and
/// wears the automation's overlay as row 1's does, and what its knobs were
/// left at plays in the export** (W7-slot-gaps item 3). Every subject an EQ
/// in its second bay and a curve on that EQ's fifth node's frequency.
/// Through the host's roads, as the frame composes them:
///
/// - the reconcile's face push (`push_builtin_faces`, by the rack's
///   identity) gives the app every subject's face, row 1's face;
/// - a knob turned on it asks the host by the bay
///   (`RecorderRequest::SetInsertParam`), and the host sets it;
/// - with the face open, the frame's walk (`open_live_faces`: the open
///   faces by identity, the curve matched by identity) evaluates its
///   overlay, which wears the curve's value and is handed back for the bay;
///   and the overlay governs the face: a gesture over both of the node's
///   knobs writes only the one the curve does not own;
/// - the export plays each subject's EQ at the knob its face left it at,
///   the automated one at its setting, and files the curve under the
///   subject's own lane.
///
/// Past row 31 the host named the bay by the frozen map's seat, which no
/// track there has: the face never drew, its knobs asked nothing, and no
/// overlay was evaluated, so a hand on the automated knob wrote the
/// curve's momentary value into the setting.
#[test]
fn a_built_in_face_past_slot_31_draws_takes_its_knobs_and_wears_its_overlay_as_on_row_1() {
    use tangent_ui::automation::{AutoParam, Curve, Point, Shape};
    const TAGS: [&str; 2] = ["gaps-faces-save", "gaps-faces"];
    let dir = scratch("faces");
    wav(&dir.join("media").join("tone.wav"), 0.3, false);
    let spec = crate::builtin_fx::spec_for(EQ_PATH).expect("the EQ");
    assert!(
        [FREQ, GAIN].iter().all(|id| spec.params.iter().any(|p| p.id == *id)),
        "fixture: the EQ has no fifth node"
    );
    let mut s = wide_desk();
    let mut doc = Arrangement::default();
    for row in SUBJECTS {
        s.rows[row].kind = Some(RowKind::Audio);
        s.plugins.insert(bay_of(&s, row, Rack::Chain(0), 1), EQ_PATH.to_owned());
        doc.add_clip(audio(row, "tone.wav"));
    }
    // **The curves go into a document the app wrote**, which carries its
    // desk (format 2): one with no desk is a legacy document, whose reader
    // files lane 32 as the master and drops every lane past it.
    let path = {
        let mut first = open(TAGS[0], s, &doc, &dir);
        first.save_project_doc();
        first.project_doc().expect("the saved project").to_path_buf()
    };
    let mut saved = Arrangement::load_from(&path);
    assert!(saved.desk.is_some(), "fixture: the app saved no desk with its document");
    for row in SUBJECTS {
        let hold = |at_s| Point { at_s, value: 0.75, shape: Shape::Hold };
        saved.automation.set(Curve::sanitised(row, AutoParam::Plugin { bay: 1, id: FREQ }, vec![hold(0.0), hold(30.0)]));
    }
    saved.save_to(&path);
    let mut app = open_saved(TAGS[1], wide_desk(), &path);
    let curved: Vec<_> = app.engine_automation().curves().iter().map(|c| c.lane).collect();
    let want: Vec<_> = SUBJECTS.iter().map(|r| tangent_ui::automation::AutoLane::Row(*r)).collect();
    assert_eq!(curved, want, "fixture: the curves did not open on their rows");
    let (mut e, p) = live(&app);
    let ids = app.track_ids();
    let eq = |row: usize| SeatBay::new(Node::Track(ids[row]), Rack::Chain(0), 1);
    let blob = state(EQ_PATH, &[], &[]);
    for row in SUBJECTS {
        seat(&mut e, &p, eq(row), EQ_PATH, &blob);
        // The reconcile's push, by the rack's identity.
        let (rack, _) = p.engine_bay(eq(row)).expect("an engine bay");
        crate::desktop::push_builtin_faces(&mut app, &e, (eq(row).seat, rack));
    }
    let auto = app.engine_automation();
    let reference = app.bay_face(eq(SUBJECTS[0])).cloned().expect("row 1's EQ face was never pushed");
    assert_eq!(reference.path, EQ_PATH);
    let mut overlays = Vec::new();
    for row in SUBJECTS {
        let sb = eq(row);
        let face = app.bay_face(sb).cloned().unwrap_or_else(|| panic!("row {row}'s EQ face was never pushed"));
        assert_eq!(face.params, reference.params, "row {row}'s face is not row 1's");
        // A knob turned asks the host by the bay, and the face shows what the
        // host set.
        app.apply_dialog_action(DialogAction::SetInsertParam { bay: sb, id: GAIN, value: 0.9 });
        assert_eq!(serve(&mut app, &mut e, &p), vec![(sb, GAIN, 0.9)], "a knob turned on row {row}'s face asked otherwise");
        let shown = app.bay_face(sb).and_then(|f| knob(f, GAIN));
        assert_eq!(shown, Some(0.9), "row {row}'s face does not show the knob the host set");
        // Open, the frame's walk evaluates the overlay for it.
        app.open_bay_face(sb);
        let _ = serve(&mut app, &mut e, &p);
        assert_eq!(app.open_face_bays(), vec![sb], "row {row}'s face did not open on its bay");
        let walked = crate::desktop::open_live_faces(&app, &p, &e, &auto, 1.0);
        let [(at, live)] = walked.as_slice() else {
            panic!("the walk gave row {row}'s open face {} overlays", walked.len());
        };
        assert_eq!(*at, sb, "the walk dressed another bay than row {row}'s open face");
        overlays.push((row, knob(live, FREQ), live.curve_db.clone()));
        app.set_live_bay_face(sb, Some(live.clone()));
        // The overlay governs the face: a hand over both of the node's
        // knobs writes the gain and leaves the curve's frequency alone.
        app.apply_dialog_action(DialogAction::SetInsertParamPair { bay: sb, id_a: FREQ, value_a: 0.1, id_b: GAIN, value_b: 0.2 });
        assert_eq!(serve(&mut app, &mut e, &p), vec![(sb, GAIN, 0.2)], "a gesture on row {row}'s face wrote the knob its curve owns");
    }
    let (_, freq, line) = &overlays[0];
    assert!(freq.is_some_and(|v| (v - 0.75).abs() < 1e-6), "row 1's overlay does not wear its curve ({freq:?})");
    for (row, f, l) in &overlays {
        assert_eq!((f, l), (freq, line), "row {row}'s overlay is not row 1's");
    }
    // The export: each EQ at the knobs its face left it at, and its curve
    // filed under its own lane.
    let Built { engine: x, bridge: xb } = export(&app, (&e, &p));
    let rest = spec.params.iter().position(|q| q.id == FREQ).map(|k| spec.defaults()[k]);
    let filed = xb.slot_automation(&auto);
    for row in SUBJECTS {
        let (rack, bay) = xb.engine_bay(eq(row)).expect("an export bay");
        let (_, values) = x.builtin_insert_at(rack, bay).unwrap_or_else(|| panic!("the export seats no EQ on row {row}"));
        let at = |id: u32| spec.params.iter().position(|q| q.id == id).map(|k| values[k]);
        assert_eq!(at(GAIN), Some(0.2), "the export plays row {row}'s EQ at another gain than its face left");
        assert_eq!(at(FREQ), rest, "the curve's value was written into row {row}'s setting");
        let lane = xb.lane(row).expect("an export lane");
        assert!(filed.gens.contains(&(lane.slot, lane.gen)), "the export files no curve under row {row}'s lane");
    }
    cleanup(&dir, &TAGS);
}

// ── a sidechain key ─────────────────────────────────────────────────────────

/// A comp that squeezes hard and fast on its key: the engine gaps' own.
fn hard_comp() -> Vec<u8> {
    state(COMP_PATH, &[(0, 0.5), (1, 1.0), (2, 0.0), (3, 0.0)], &[])
}

/// **What the comp in the first bay of each of `rows` did**: the most it
/// squeezed, in dB, over two fifths of a second rolled from zero, and its
/// track's level over the second fifth.
fn squeeze(app: &TangentApp, e: &mut Engine, p: &Bridge, rows: &[usize]) -> (Vec<f32>, Vec<f32>) {
    let ids = app.track_ids();
    let bays: Vec<_> = rows
        .iter()
        .map(|r| p.engine_bay(SeatBay::new(Node::Track(ids[*r]), Rack::Chain(0), 0)).expect("a comp's bay"))
        .collect();
    let slots: Vec<Slot> = rows.iter().map(|r| p.lane(*r).expect("a row's lane").slot).collect();
    e.set_transport(1, 0);
    e.set_rolling(true);
    let mut out = vec![0.0f32; 512 * 2];
    let mut gr = vec![0.0f32; rows.len()];
    let blocks = (RATE as usize / 5).div_ceil(512);
    for n in 0..2 * blocks {
        if n == blocks {
            for s in &slots {
                let _ = e.lane_reading(*s);
            }
        }
        out.fill(0.0);
        e.render_offline(&mut out);
        for (g, (rack, bay)) in gr.iter_mut().zip(&bays) {
            *g = g.max(e.insert_gr_db_at(*rack, *bay));
        }
    }
    let levels = slots
        .iter()
        .map(|s| {
            let ([a, b], _) = e.lane_reading(*s);
            a.max(b)
        })
        .collect();
    (gr, levels)
}

/// **A key from row 150 reaches a comp past slot 31 as it reaches row 1's,
/// live and in the export, and no other comp** (W7-slot-gaps item 4). Every
/// subject and row 2 hold a hard comp over a quiet tone of their own (a
/// lane with nothing to play runs no rack, so a comp on one squeezes
/// nothing to read); the desk's key (`insert_keys`, by the comp's bay and
/// the source's track) is drawn from row 150, past the old desk, to one
/// subject at a time, and a loud clip plays on row 150. The keyed comp
/// squeezes its track as row 1's does when it is the keyed one, on both
/// engines, and every other comp holds still and leaves its track at row
/// 2's level. Keys past slot 31 needed no fix: this proves them through the
/// bridge, both ways. The key to a BUS is proved at the engine
/// (`gaps::a_key_from_lane_40_reaches_a_comp_on_33_and_on_a_bus`): the
/// bridge makes no lane a bus until W7-buses-ui.
#[test]
fn a_key_reaches_a_comp_past_slot_31_as_on_row_1_live_and_in_the_export() {
    let dir = scratch("keys");
    wav(&dir.join("media").join("loud.wav"), 0.9, true);
    wav(&dir.join("media").join("soft.wav"), 0.01, false);
    let comps: Vec<usize> = SUBJECTS.into_iter().chain([CONTROL]).collect();
    let mut doc = Arrangement::default();
    doc.add_clip(audio(150, "loud.wav"));
    for &row in &comps {
        doc.add_clip(audio(row, "soft.wav"));
    }
    let path = tangent_ui::arrangement::doc_in(&dir);
    doc.save_to(&path);
    let control = comps.len() - 1;
    let mut squeezed = Vec::new();
    let mut tags = Vec::new();
    for (k, keyed) in SUBJECTS.into_iter().enumerate() {
        let tag = format!("gaps-keys-{keyed}");
        let mut s = wide_desk();
        for &row in &comps {
            s.rows[row].kind = Some(RowKind::Audio);
            s.plugins.insert(bay_of(&s, row, Rack::Chain(0), 0), COMP_PATH.to_owned());
        }
        s.rows[150].kind = Some(RowKind::Audio);
        let from = Node::Track(s.rows[150].id);
        s.insert_keys.set(bay_of(&s, keyed, Rack::Chain(0), 0), Some(from));
        let app = open_saved(&tag, s, &path);
        let (mut e, mut p) = live(&app);
        for &row in &comps {
            seat(&mut e, &p, SeatBay::new(Node::Track(app.track_ids()[row]), Rack::Chain(0), 0), COMP_PATH, &hard_comp());
        }
        p.sync(&app, &mut e);
        let Built { engine: mut x, bridge: xb } = export(&app, (&e, &p));
        for (road, (gr, lv)) in [("live", squeeze(&app, &mut e, &p, &comps)), ("export", squeeze(&app, &mut x, &xb, &comps))] {
            assert!(lv[control] > 0.005, "{road}: row 2's quiet tone is silent ({})", lv[control]);
            for (j, row) in comps.iter().enumerate() {
                if j == k {
                    assert!(gr[j] > 6.0, "{road}: a key from row 150 squeezed row {row}'s comp only {:.2} dB", gr[j]);
                    assert!(lv[j] < 0.7 * lv[control], "{road}: row {row}'s keyed comp left its track at {}, row 2 at {}", lv[j], lv[control]);
                    squeezed.push((road, keyed, gr[j]));
                } else {
                    assert!(gr[j] < 0.5, "{road}: the key for row {keyed} squeezed row {row}'s comp {:.2} dB", gr[j]);
                    assert!((lv[j] - lv[control]).abs() <= 1e-3 * lv[control], "{road}: the key for row {keyed} moved row {row} to {}", lv[j]);
                }
            }
        }
        tags.push(tag);
    }
    // As row 1's: every subject's keyed comp squeezes as deep as row 1's.
    for road in ["live", "export"] {
        let on: Vec<(usize, f32)> = squeezed.iter().filter(|(r, _, _)| *r == road).map(|(_, k, g)| (*k, *g)).collect();
        let reference = on[0].1;
        for (keyed, g) in &on {
            assert!((g - reference).abs() <= 0.05 * reference, "{road}: row {keyed}'s keyed comp squeezed {g:.2} dB, row 1's {reference:.2}");
        }
    }
    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
    cleanup(&dir, &tags);
}

// ── the send knob, as the strip's Return 0 entry ─────────────────────────────

/// **The send desk**: every subject an audio track, its knob at `knobs[i]`,
/// and row 7 an audio track whose knob is at nothing.
fn send_desk(knobs: &[f64]) -> Settings {
    let mut s = wide_desk();
    for (row, level) in SUBJECTS.iter().zip(knobs) {
        s.rows[*row].kind = Some(RowKind::Audio);
        s.set_strip_send(StripRef::Row(*row), *level);
    }
    s.rows[7].kind = Some(RowKind::Audio);
    s
}

/// The post-fader Return 0 entries a plan was built with, `(slot, level)`
/// by slot, the strips' only.
fn fx_entries(p: &Bridge) -> Vec<(Slot, f32)> {
    let inputs = p.published_for_test().expect("a plan was published");
    let mut out: Vec<(Slot, f32)> = inputs
        .sends
        .iter()
        .filter_map(|s| match (s.from, s.to, s.pre) {
            (SendFrom::Lane(slot), SendTo::Fx, false) => Some((slot, s.level)),
            _ => None,
        })
        .collect();
    out.sort_by_key(|(s, _)| *s);
    out
}

/// The playing plan's gains, as bits.
fn gains(e: &Engine) -> Vec<u32> {
    let g = e.plan_gains().expect("a plan plays");
    g.iter().map(|c| c.load(std::sync::atomic::Ordering::Relaxed)).collect()
}

/// **A strip's send knob is its Return 0 entry on a track past slot 31 as
/// on row 1, live and in the export, and a knob turned rides into the plan
/// playing with no plan of its own** (W7-slot-gaps item 5). Every subject's
/// knob, each at its own level, is its lane's post-fader Fx entry at that
/// level in the plan the live bridge publishes and in the export's; row 7,
/// at nothing, carries none. Then every knob turned (the same desk with the
/// knobs elsewhere, synced through the same bridge, which is all a frame
/// after a drag is) publishes no plan, and the playing plan's gains are
/// bit for bit those of a plan built at the new levels; the export carries
/// the new levels. And a knob taken to nothing republishes without its
/// entry. Before, a plan carried no send of a strip's at all, and the knob
/// reached the return only beside the plan.
#[test]
fn a_send_knob_past_slot_31_is_its_return_entry_as_on_row_1_live_and_in_the_export() {
    let tags = ["gaps-sends", "gaps-sends-turned", "gaps-sends-fresh", "gaps-sends-shut"];
    let dir = scratch("sends");
    wav(&dir.join("media").join("tone.wav"), 0.5, true);
    let mut doc = Arrangement::default();
    for row in SUBJECTS.into_iter().chain([7]) {
        doc.add_clip(audio(row, "tone.wav"));
    }
    let path = tangent_ui::arrangement::doc_in(&dir);
    doc.save_to(&path);
    let first: Vec<f64> = (0..SUBJECTS.len()).map(|i| 0.2 + 0.1 * i as f64).collect();
    let turned: Vec<f64> = first.iter().map(|v| v + 0.25).collect();
    let entries = |knobs: &[f64]| -> Vec<(Slot, f32)> {
        SUBJECTS.iter().zip(knobs).map(|(r, v)| (Slot(*r as u32), *v as f32)).collect()
    };

    let app = open_saved(tags[0], send_desk(&first), &path);
    let (mut e, mut p) = live(&app);
    assert_eq!(fx_entries(&p), entries(&first), "the live plan does not carry each knob as its strip's Return 0 entry");
    {
        let Built { bridge: xb, .. } = export(&app, (&e, &p));
        assert_eq!(fx_entries(&xb), entries(&first), "the export's plan does not carry each knob as its strip's Return 0 entry");
    }
    let mut out = vec![0.0f32; 512 * 2];
    e.render_offline(&mut out);

    // Every knob turned: no plan, and the playing plan holds the new levels.
    let moved = open_saved(tags[1], send_desk(&turned), &path);
    let plans = p.stats().plans;
    p.sync(&moved, &mut e);
    e.render_offline(&mut out);
    assert_eq!(p.stats().plans, plans, "a knob turned published a plan of its own");
    let (mut fresh, _fresh_bridge) = live(&open_saved(tags[2], send_desk(&turned), &path));
    fresh.render_offline(&mut out);
    assert_eq!(gains(&e), gains(&fresh), "the playing plan's gains are not a plan's built at the turned knobs");
    {
        let Built { bridge: xb, .. } = export(&moved, (&e, &p));
        assert_eq!(fx_entries(&xb), entries(&turned), "the export does not carry the turned knobs");
    }

    // Row 200's knob to nothing: its entry goes, by a plan.
    let mut shut = turned.clone();
    shut[SUBJECTS.len() - 1] = 0.0;
    let shut_app = open_saved(tags[3], send_desk(&shut), &path);
    p.sync(&shut_app, &mut e);
    assert_eq!(p.stats().plans, plans + 1, "a knob taken to nothing published no plan");
    let mut want = entries(&turned);
    want.pop();
    assert_eq!(fx_entries(&p), want, "a knob at nothing still carries an entry");
    cleanup(&dir, &tags);
}

// ── the host is off the frozen map's seats ──────────────────────────────────

/// **`src` with everything `#[cfg(test)]` cut out**: a gated item to its
/// closing brace at the gate's own indent (a test module, a test-only impl,
/// a test-only fn in an impl), a gated one-liner alone; and the comment
/// tail of every line, so a sentence ABOUT a name is not read as a use.
fn production(src: &str) -> String {
    let mut out = String::new();
    let mut skip: Option<String> = None;
    let mut gate: Option<usize> = None;
    for line in src.lines() {
        if let Some(end) = &skip {
            if line == end {
                skip = None;
            }
            continue;
        }
        let t = line.trim_start();
        let indent = line.len() - t.len();
        if t.starts_with("#[cfg(") && t.contains("test") && !t.contains("not(test") && !t.contains("any(") {
            gate = Some(indent);
            continue;
        }
        if let Some(at) = gate {
            if t.starts_with("#[") || t.starts_with("//") {
                continue;
            }
            gate = None;
            let end = line.trim_end();
            let opens = line.matches('{').count();
            let one_line = end.ends_with(';') || end.ends_with(',') || (opens > 0 && opens == line.matches('}').count());
            if !one_line {
                skip = Some(format!("{}}}", " ".repeat(at)));
            }
            continue;
        }
        let code = line.split("//").next().unwrap_or("");
        if !code.trim().is_empty() {
            out.push_str(code);
            out.push('\n');
        }
    }
    out
}

/// **No road of the host's names a bay by the frozen map's seat, and the app
/// offers none** (W7-slot-gaps item 3: the C11 seat-form adapters retired
/// with the overlay that went through them). The map has no seat for a
/// track past its thirty-second row, so every road that held one stopped
/// there: the faces, their knobs and their overlay, the editors, the FX
/// drag, copy, paste and swap, the IR and sampler loads, the insert tap,
/// the compressor's needle and the plugin states written on save. The
/// crossings still exist for the tests' legacy fixtures, cfg(test) beside
/// them; production code reaches none.
#[test]
fn no_host_road_names_a_bay_by_the_frozen_maps_seat() {
    let host = [
        ("desktop.rs", production(include_str!("desktop.rs"))),
        ("engine_bridge.rs", production(include_str!("engine_bridge.rs"))),
    ];
    let crossings = ["seat_bay_at(", "app_seat_of(", "engine_seat(", "app_seat(", "strip_slot(", "strip_node("];
    let mut bad = Vec::new();
    for (name, src) in &host {
        for line in src.lines() {
            for c in crossings {
                if line.contains(c) {
                    bad.push(format!("{name}: `{c}` in {}", line.trim()));
                }
            }
        }
    }
    assert!(bad.is_empty(), "the host names a bay by the frozen map's seat:\n  {}", bad.join("\n  "));
    // The app's host-facing surface: none of the retired adapters.
    let app = production(include_str!("../../tangent-ui/src/app.rs"));
    let retired = [
        "fn insert(", "fn set_insert(", "fn insert_bypassed(", "fn insert_bypass(", "fn toggle_insert_bypass(",
        "fn set_insert_bypass(", "fn trade_inserts(", "fn open_insert_face(", "fn open_face_seats(",
        "fn set_insert_gr(", "fn set_builtin_face(", "fn set_builtin_face_at(", "fn set_live_face(",
        "fn set_fx_params(", "fn insert_short_name(", "fn take_open_insert_on_load(", "fn open_insert_when_loaded(",
        "fn follow_insert_face(", "fn legacy_bay(", "fn legacy_seat(",
    ];
    let offered: Vec<&str> = retired.iter().copied().filter(|r| app.contains(r)).collect();
    assert!(offered.is_empty(), "the app offers the host a seat form again: {offered:?}");
    // And the cut keeps what it must: the identity roads are production.
    for kept in ["fn set_bay_face(", "fn open_face_bays(", "fn set_live_bay_face(", "fn trade_bays("] {
        assert!(app.contains(kept), "`{kept}` is gone from the app's production code; this scan needs rethinking");
    }
    let cut = production(
        "impl A {\n    #[cfg(test)]\n    fn legacy_bay(\n        &self,\n    ) {\n        x\n    }\n    #[cfg(test)]\n    fn gated() {}\n    fn kept() {}\n}\n",
    );
    assert!(!cut.contains("legacy_bay") && !cut.contains("gated"), "the cut keeps a gated fn: {cut}");
    assert!(cut.contains("fn kept()"), "the cut takes what follows a gated one-liner: {cut}");
}
