//! **Track 33 is every bit a track** (design 13, W7-slot-gaps), end to end:
//! a desk two hundred and one rows wide, its subjects on rows 1, 31, 32, 33,
//! 40 and 200, each given the same part, and each part made to behave on
//! every subject as it does on row 1, LIVE (an engine driven by the
//! [`Bridge`], its lanes on their rows' slots, furnished by the host's own
//! functions: [`Host::frame`]) and in an EXPORT (`full_build_with`, the one
//! builder an export, a freeze and the probe come through, handed the live
//! engine as the host hands it). The engine's own doors are held at the
//! same widths by `instrument::slot_engine::gaps`; what this file adds is
//! every road from the desk to them.
//!
//! **The live roads are `DesktopApp`'s, not copies of them.** `DesktopApp`
//! cannot be built in a test, so each road the frame takes to these parts is
//! a function of what it touches, which the frame calls with its own fields
//! and [`Host`] with its own: the bridge's sync and what the host forgets
//! and takes back with it, the reconcile's walk and its load of one bay
//! (`next_owed_bay`, `load_bay`), the kits and their knobs (`deliver_kits`,
//! `push_pad_knobs`), the samplers' files (`deliver_samples`), and a face's
//! knob (`set_insert_param`). A copy of those here stayed green while the
//! road it copied could stop at the thirty-second row.
//!
//! The parts: a drum lane's pad effect (and one seated before its track's
//! kit is), a sampler bay's file, a built-in face (its push, its knobs and
//! the automation's overlay on it), a sidechain key, and a strip's send knob
//! as its Return 0 entry. And the scans that keep the host off the frozen
//! map's seats, which is what made the faces' roads stop at the
//! thirty-second row. The parts that need a BUS on the desk (a key to a
//! bus, a curve on a send to one, and the mixing leftovers of item 6, which
//! each cross one) had no road from the desk until W7-buses-ui gave the
//! bridge buses; the engine held them at these widths
//! (`slot_engine::gaps`, `slot_engine::buses`), and since W7-join the
//! desk's half of each runs here too. So does a kit owed the frame its
//! track takes the drumpad, which the frame's own rule decides
//! (`Synced::owes_kits`).
//!
//! **Nothing here touches the real home**: every licence store is a temp
//! directory of the test's own (`composite::tiers`), every project and every
//! live shelf is under the temp dir, and the apps run with the global
//! settings file off (`persist_global_settings`), as `konami_e2e`'s do.
//!
//! [`Bridge`]: crate::engine_bridge::Bridge

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use tangent_record::audio::Timebase;
use tangent_ui::app::TangentApp;
use tangent_ui::arrangement::{ArrClip, ArrNote, Arrangement, ClipKind};
use tangent_ui::desk_id::{Node, Rack, SeatBay, TrackId};
use tangent_ui::desk_model::{RowKind, TrackRow};
use tangent_ui::dialogs::{DialogAction, DRUMPAD_PATH, KEYS_PATH};
use tangent_ui::drumpad::DrumpadDoc;
use tangent_ui::host::Caps;
use tangent_ui::recorder::{DrumKitParams, RecorderRequest, StripRef, DRUM_PAD_BASE};
use tangent_ui::settings::Settings;

use crate::builtin_fx::{COMP_PATH, EQ_PATH, SAMPLER_PATH, TRIM_PATH};
use crate::composite::tiers;
use crate::desk_plan::{SendFrom, SendTo, Slot};
use crate::desktop::{BayRoad, Decoded, DesktopApp, InsertLedger};
use crate::engine_bridge::{full_build_with, lane_bay, BuildFor, Bridge, Built, Furnish};
use crate::instrument::Engine;
use crate::shelf::DisplacedShelf;

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

// ── the host, by its own roads ──────────────────────────────────────────────

/// **What the host keeps between frames**, for its own roads to keep here:
/// the reconcile's ledger and displaced shelf, the live shelf it seats bays
/// from (this test's own, `<scratch>/state`), the engine's sentence, the
/// samplers' record and cache, the kits' cache, the knobs each kit was last
/// told and whether a kit delivery is owed a retry, and the curves the
/// engine was last handed. [`Host::frame`] runs a frame over them through
/// `DesktopApp`'s own functions, so a road of the host's that stops at the
/// thirty-second row stops here too.
#[derive(Default)]
struct Host {
    shelf: PathBuf,
    ledger: InsertLedger,
    displaced: DisplacedShelf,
    error: Option<String>,
    sampler_have: BTreeMap<usize, Option<(PathBuf, u8)>>,
    sampler_cache: HashMap<PathBuf, Decoded>,
    pad_cache: HashMap<PathBuf, Decoded>,
    pushed_pads: BTreeMap<usize, DrumKitParams>,
    kits_retry: bool,
    pushed_auto: Option<tangent_ui::automation::Automation>,
}

impl Host {
    /// A host whose live shelf is `dir/state`.
    fn at(dir: &Path) -> Self {
        Self {
            shelf: dir.join("state"),
            ..Self::default()
        }
    }

    /// **A bay's saved state on this host's shelf**, as a save leaves it:
    /// what the reconcile seats the bay with.
    fn shelve(&self, sb: SeatBay, path: &str, blob: &[u8]) {
        assert!(crate::shelf::write_state_in(&self.shelf, sb, path, blob), "fixture: {sb}'s state did not land");
    }

    /// **One host frame**, each step `DesktopApp`'s own function, in the
    /// frame's order:
    ///
    /// - `push_monitor_settings`'s: the bridge's sync, a lane leaving
    ///   written down and forgotten (`forget_lane`) and one taken back
    ///   reclaimed (`reclaim_lanes`), what the host keeps per slot forgotten
    ///   for a lane that came or went (`forget_moved_slots`, and the curves
    ///   it last handed over), the kits the sync owes, and the kits' knobs
    ///   (`push_pad_knobs`);
    /// - `push_automation`'s: the curves, the sends' own among them
    ///   (`push_curves`), when they changed;
    /// - `after_frame`'s reconcile: the bays the desk wants
    ///   (`bays_wanted`), the next one owed a load (`next_owed_bay`) and its
    ///   load (`load_bay`), one a frame, the frames run back to back until
    ///   none is owed;
    /// - `after_frame`'s kits owed by a retry or an edit
    ///   (`take_drumpad_dirty`), and `run_warm`'s: the kits (`deliver_kits`)
    ///   when owed, a delivery that did not all land owed again, and the
    ///   samplers' files (`deliver_samples`). The host runs the samplers'
    ///   when they are owed; every frame here is one whose desk moved, so
    ///   every frame owes them. The kits keep the host's own rule, which is
    ///   what decides whether a track that takes the drumpad hears its kit.
    fn frame(&mut self, app: &mut TangentApp, e: &mut Engine, p: &mut Bridge) {
        let synced = {
            let (ledger, shelf) = (&mut self.ledger, &self.shelf);
            p.sync_with(app, e, &mut |e, id, lane| DesktopApp::forget_lane(ledger, shelf, e, id, lane))
        };
        DesktopApp::reclaim_lanes(&mut self.ledger, &synced);
        DesktopApp::forget_moved_slots(&synced.moved, &mut self.sampler_have, &mut self.pushed_pads);
        if !synced.moved.is_empty() {
            self.pushed_auto = None;
        }
        let mut kits_owed = synced.owes_kits();
        if synced.primed {
            self.pushed_pads.clear();
        }
        DesktopApp::push_pad_knobs(e, p, app, &mut self.pushed_pads);
        DesktopApp::push_curves(Some(&mut *e), p, app, &mut self.pushed_auto);
        let mut settled = false;
        for _ in 0..10_000 {
            let wanted = DesktopApp::bays_wanted(app, p);
            let (Some(at), _) = DesktopApp::next_owed_bay(&wanted, &self.ledger) else {
                settled = true;
                break;
            };
            let road = BayRoad {
                app: &mut *app,
                bridge: &*p,
                engine: Some(&mut *e),
                ledger: &mut self.ledger,
                displaced: &mut self.displaced,
                shelf: &self.shelf,
                restoring: false,
                error: &mut self.error,
            };
            DesktopApp::load_bay(road, at, wanted.get(&at).cloned());
            e.settle_offline();
        }
        assert!(settled, "the reconcile never ran out of bays to load");
        assert!(self.error.is_none(), "a bay would not load: {:?}", self.error);
        kits_owed |= std::mem::take(&mut self.kits_retry) || app.take_drumpad_dirty();
        if kits_owed {
            self.kits_retry = !DesktopApp::deliver_kits(Some(&mut *e), p, app, &mut self.pad_cache);
        }
        DesktopApp::deliver_samples(Some(&mut *e), p, app, &mut self.sampler_have, &mut self.sampler_cache);
        e.settle_offline();
    }
}

/// **The live engine, as the host furnishes it**: a frame of [`Host`]'s (the
/// bridge's shape, each row's lane on its own slot, which the subjects'
/// slots depend on; every bay the desk seats, from the host's shelf; each
/// drum lane's kit and knobs; each sampler's file), and the document's
/// clips, placed as the export places them and stamped with their lanes by
/// the bridge as `reload_clips` stamps them.
fn live(app: &mut TangentApp, host: &mut Host) -> (Engine, Bridge) {
    let mut e = Engine::offline(RATE, Timebase::new()).expect("a live engine");
    let mut p = Bridge::new();
    host.frame(app, &mut e, &mut p);
    for (row, id) in app.track_ids().iter().enumerate() {
        let at = p.lane_of(*id).map(|l| l.slot);
        assert_eq!(at, Some(Slot(row as u32)), "fixture: row {row}'s lane is not its own slot");
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

/// **The kits of `rows`, in the sidecar beside the document at `path`**:
/// pad 36 a steady tone at half its gain, a knob of the lane's own
/// (`set_pad_params` keeps a kit's pads by its lane) that the frame's push
/// (`push_pad_knobs`) has to tell every lane: a kit it missed plays the pad
/// at unity, twice row 1's level. Not the kit's fader: that is one number
/// for every kit (the drumpad's master), which any lane's push sets for all.
fn save_kits(s: &Settings, rows: &[usize], path: &Path) {
    let mut kits: Vec<DrumpadDoc> = (0..ROWS).map(|_| Default::default()).collect();
    for &row in rows {
        kits[row].pads[36].path = "media/pad.wav".to_owned();
        kits[row].pads[36].params.gain = 0.5;
    }
    let ids: Vec<TrackId> = s.rows.iter().map(|r| r.id).collect();
    DrumpadDoc::save_all_beside_ids(&kits, &ids, SUBJECTS[0], path);
}

/// **The kit desk**: every subject and [`CONTROL`] a drum track (the
/// drumpad in its first bay, its own kit in the project's sidecar), the
/// kit's pad 36 a steady tone struck by a clip; and with `trim`, a Trim at
/// its floor (-24 dB) on pad 36's first bay of every subject, its state on
/// the host's shelf, seated by the reconcile. Answers the app and the live
/// engine.
fn kit_desk(tag: &str, dir: &Path, trim: bool) -> (TangentApp, Engine, Bridge) {
    wav(&dir.join("media").join("pad.wav"), 0.25, false);
    let mut s = wide_desk();
    let mut doc = Arrangement::default();
    let rows: Vec<usize> = SUBJECTS.into_iter().chain([CONTROL]).collect();
    for &row in &rows {
        s.rows[row].kind = Some(RowKind::Midi);
        s.plugins.insert(bay_of(&s, row, Rack::Chain(0), 0), DRUMPAD_PATH.to_owned());
        if trim && row != CONTROL {
            s.plugins.insert(bay_of(&s, row, Rack::Pad(36), 0), TRIM_PATH.to_owned());
        }
        doc.add_clip(notes(row, DRUM_PAD_BASE + 36));
    }
    let path = tangent_ui::arrangement::doc_in(dir);
    doc.save_to(&path);
    save_kits(&s, &rows, &path);
    let mut host = Host::at(dir);
    if trim {
        let floor = state(TRIM_PATH, &[(0, 0.0)], &[]);
        for row in SUBJECTS {
            host.shelve(bay_of(&s, row, Rack::Pad(36), 0), TRIM_PATH, &floor);
        }
    }
    let mut app = open_saved(tag, s, &path);
    let (e, p) = live(&mut app, &mut host);
    (app, e, p)
}

/// **A pad effect on a drum track past slot 31 is heard as on row 1, live
/// and in the export** (W7-slot-gaps item 1). A Trim at -24 dB on each
/// subject's pad 36 cuts that subject's kit by the 24 dB it cuts row 1's
/// by: on the live engine, the bay seated by the host's reconcile, and on
/// the export's, the bay furnished from the live one's knobs. Every kit
/// plays at row 1's level, its pad's gain told by the frame's push; the control
/// drum track, with no pad effect, is not moved. Past slot 31 a pad's rack
/// had no seat, so the bay was seated nowhere live (`load_insert_at`
/// answered `Ok(None)`), refused in the export, and the kit played dry both
/// ways.
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
            assert!(dry[i] > 0.02, "{road}: the kit on row {row} is silent ({})", dry[i]);
            assert!(
                (dry[i] - dry[0]).abs() <= 0.02 * dry[0],
                "{road}: the kit on row {row} plays at {}, row 1's at {}: its knobs did not reach it",
                dry[i],
                dry[0]
            );
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

/// **A pad effect seated before its track is a drum track waits for the
/// track's kit, and is then heard past slot 31 as on row 1, live and in the
/// export** (W7-slot-gaps, after review). Every subject a MIDI track whose
/// first bay holds the keyboard, with a Trim at -24 dB still on its pad 36
/// (nothing clears a pad's bays when the instrument changes) and its kit in
/// the sidecar: a project reopened, or an engine rebuilt, after the drumpad
/// was swapped out. Then the drumpad goes back in every subject's first bay.
///
/// A lane past the frozen map is installed with no pad bank for such a
/// track, and a pad's rack lives in that bank. The reconcile loaded the Trim
/// all the same: its rack went home with the plugin in it, the ledger still
/// said it was seated, and once the drumpad was back the bank came with
/// nothing on pad 36, so the kit played that pad dry, live, for good, while
/// the export built the lane with its bank and played it trimmed. Now no
/// pad's bay is wanted before its lane has a bank (`Bridge::engine_bay`):
/// past the frozen map nothing is seated or written down before the
/// drumpad, a lane of the frozen map seats the Trim in its frozen rack at
/// once, and after the drumpad every subject's kit is cut by the 24 dB row
/// 1's is, against kits that were drum tracks from the start.
#[test]
fn a_pad_effect_waits_for_its_tracks_kit_and_is_heard_past_slot_31_as_on_row_1() {
    let tags = ["gaps-late-dry", "gaps-late"];
    let (dry_dir, dir) = (scratch("late-dry"), scratch("late"));
    let (dry_app, mut dry_e, dry_p) = kit_desk(tags[0], &dry_dir, false);
    wav(&dir.join("media").join("pad.wav"), 0.25, false);
    let mut s = wide_desk();
    let mut doc = Arrangement::default();
    for row in SUBJECTS {
        s.rows[row].kind = Some(RowKind::Midi);
        s.plugins.insert(bay_of(&s, row, Rack::Chain(0), 0), KEYS_PATH.to_owned());
        s.plugins.insert(bay_of(&s, row, Rack::Pad(36), 0), TRIM_PATH.to_owned());
        doc.add_clip(notes(row, DRUM_PAD_BASE + 36));
    }
    let path = tangent_ui::arrangement::doc_in(&dir);
    doc.save_to(&path);
    save_kits(&s, &SUBJECTS, &path);
    let mut host = Host::at(&dir);
    let floor = state(TRIM_PATH, &[(0, 0.0)], &[]);
    let firsts: Vec<SeatBay> = SUBJECTS.iter().map(|r| bay_of(&s, *r, Rack::Chain(0), 0)).collect();
    for row in SUBJECTS {
        host.shelve(bay_of(&s, row, Rack::Pad(36), 0), TRIM_PATH, &floor);
    }
    let mut app = open_saved(tags[1], s, &path);
    let (mut e, mut p) = live(&mut app, &mut host);

    // Before the drumpad: a lane of the frozen map seats the Trim in its
    // frozen rack; past it the lane has no bank, and the reconcile neither
    // loads the Trim nor writes it down as loaded.
    for row in SUBJECTS {
        let lane = p.lane(row).expect("a lane");
        let at = lane_bay(lane.slot, Rack::Pad(36), 0).expect("a pad's bay");
        let seated = e.insert_at(at.0, at.1).is_some();
        let noted = host.ledger.contains_key(&at);
        let frozen = row < 32;
        assert_eq!(
            (seated, noted),
            (frozen, frozen),
            "row {row}'s pad 36 before its track's kit: seated {seated}, in the ledger {noted}"
        );
    }

    // The drumpad back in every subject's first bay, and a frame: each lane
    // past the frozen map is sent its bank, and the reconcile seats the pad.
    for sb in firsts {
        app.set_plugin(sb, Some(DRUMPAD_PATH));
    }
    host.frame(&mut app, &mut e, &mut p);
    let Built { engine: mut x, bridge: xb } = export(&app, (&e, &p));
    let Built { engine: mut dx, bridge: dxb } = export(&dry_app, (&dry_e, &dry_p));
    let roads = [
        ("live", levels(&mut dry_e, &dry_p, &SUBJECTS), levels(&mut e, &p, &SUBJECTS)),
        ("export", levels(&mut dx, &dxb, &SUBJECTS), levels(&mut x, &xb, &SUBJECTS)),
    ];
    for (road, dry, cut) in &roads {
        let ratio = |i: usize| cut[i] / dry[i];
        let reference = ratio(0);
        assert!((reference - 0.063).abs() < 0.01, "{road}: row 1's pad effect left its kit at {reference} of itself, not -24 dB");
        for (i, row) in SUBJECTS.iter().enumerate() {
            let r = ratio(i);
            assert!(
                (r - reference).abs() <= 0.02 * reference,
                "{road}: row {row}'s pad effect, seated before its kit, left the kit at {r} of itself, row 1's at {reference}"
            );
        }
    }
    cleanup(&dry_dir, &tags[..1]);
    cleanup(&dir, &tags[1..]);
}

/// **A track that takes the drumpad hears its kit from the next frame, past
/// slot 31 as on row 1, live and in the export** (W7-join item 4). Row 2 is
/// a drum track from the start, and its kit has landed, so no delivery is
/// owed a retry; every subject is a MIDI track with the keyboard in its
/// first bay and a kit of its own in the sidecar (a project reopened, or an
/// engine rebuilt, after the drumpad was swapped out). The drumpad is picked
/// into each subject's first bay and the host runs one frame by its own rule
/// for when kits are owed: every subject's kit plays its clip at row 2's
/// level, live as in the export. The pick marked no kit owed: no lane came
/// or went, no kit was edited, and the last delivery had landed whole, so
/// `reload_pads` never ran and each subject's pads stayed silent live until
/// another kit edit or a reopen, while the export, which attaches every
/// drum lane's kit itself, played them.
#[test]
fn a_track_that_takes_the_drumpad_hears_its_kit_from_the_next_frame_past_slot_31_as_on_row_1() {
    const TAG: &str = "gaps-drum-pick";
    let dir = scratch("drum-pick");
    wav(&dir.join("media").join("pad.wav"), 0.25, false);
    let mut s = wide_desk();
    let mut doc = Arrangement::default();
    let rows: Vec<usize> = SUBJECTS.into_iter().chain([CONTROL]).collect();
    for &row in &rows {
        s.rows[row].kind = Some(RowKind::Midi);
        let first = if row == CONTROL { DRUMPAD_PATH } else { KEYS_PATH };
        s.plugins.insert(bay_of(&s, row, Rack::Chain(0), 0), first.to_owned());
        doc.add_clip(notes(row, DRUM_PAD_BASE + 36));
    }
    let path = tangent_ui::arrangement::doc_in(&dir);
    doc.save_to(&path);
    save_kits(&s, &rows, &path);
    let firsts: Vec<SeatBay> = SUBJECTS.iter().map(|r| bay_of(&s, *r, Rack::Chain(0), 0)).collect();
    let mut host = Host::at(&dir);
    let mut app = open_saved(TAG, s, &path);
    let (mut e, mut p) = live(&mut app, &mut host);
    assert!(!host.kits_retry, "fixture: row 2's kit did not land, so a retry would deliver every kit anyway");
    // The drumpad picked into every subject's first bay, and one frame.
    for sb in firsts {
        app.set_plugin(sb, Some(DRUMPAD_PATH));
    }
    host.frame(&mut app, &mut e, &mut p);
    let Built { engine: mut x, bridge: xb } = export(&app, (&e, &p));
    for (road, got) in [("live", levels(&mut e, &p, &rows)), ("export", levels(&mut x, &xb, &rows))] {
        let control = got[rows.len() - 1];
        assert!(control > 0.02, "{road}: row 2's kit is silent ({control})");
        for (row, level) in SUBJECTS.iter().zip(&got) {
            assert!(
                (level - control).abs() <= 0.02 * control,
                "{road}: row {row}'s kit plays at {level} the frame after its track took the drumpad, row 2's at {control}"
            );
        }
    }
    cleanup(&dir, &[TAG]);
}

// ── a sampler bay's file ────────────────────────────────────────────────────

/// **A sampler bay plays its file on a track past slot 31 as on row 1, live
/// and in the export, and lets it go as row 1's does** (W7-slot-gaps item
/// 2). Every subject a MIDI track with the sampler in its first bay, its
/// state on the host's shelf naming `media/tone.wav`, and a note at the root
/// key. The host's reload (`deliver_samples`, over its walk of the engine's
/// sampler bays, `sampler_bays_of`, which the export's
/// `mirror_sampler_samples` reads too) reaches every subject's lane, the
/// lane keeps the file handed to it, and the face is told, so each plays at
/// row 1's level on both engines. Every face is told in the frame the lanes
/// arrive: the host marked every lane it installed owed a file
/// (`forget_moved_slots`), and handed each an empty one first, a round trip
/// apiece, so on this desk row 200's file waited behind a hundred and
/// sixty-nine of them (the offline ring holds thirty-four a frame). Then
/// each sampler is taken out of its
/// bay: the reconcile empties the bay, and the reload lets the lane's file
/// go (its record by slot, `sampler_have`, is what wants nothing there now)
/// and tells the face so. The walk read the legacy thirty-two, and the
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
    let root = crate::instrument::SAMPLER_ROOT_KEY.to_string();
    let blob = state(SAMPLER_PATH, &[], &[("sample_path", "media/tone.wav"), ("root_key", &root)]);
    let mut host = Host::at(&dir);
    let samplers: Vec<SeatBay> = SUBJECTS.iter().map(|r| bay_of(&s, *r, Rack::Chain(0), 0)).collect();
    for sb in &samplers {
        host.shelve(*sb, SAMPLER_PATH, &blob);
    }
    let mut app = open(TAG, s, &doc, &dir);
    let (mut e, mut p) = live(&mut app, &mut host);
    let face = |app: &TangentApp, sb: SeatBay| app.sampler_info(sb.seat.node).name.clone();
    for (row, sb) in SUBJECTS.iter().zip(&samplers) {
        assert_eq!(face(&app, *sb), "tone.wav", "row {row}'s sampler face was not told its file");
    }
    {
        let Built { engine: mut x, bridge: xb } = export(&app, (&e, &p));
        for (road, got) in [("live", levels(&mut e, &p, &SUBJECTS)), ("export", levels(&mut x, &xb, &SUBJECTS))] {
            let want = got[0];
            assert!(want > 0.05, "{road}: row 1's sampler is silent ({want})");
            for (row, level) in SUBJECTS.iter().zip(&got) {
                assert!((level - want).abs() <= 0.02 * want, "{road}: row {row}'s sampler played {level}, row 1's {want}");
            }
        }
    }
    // Every sampler taken out of its bay, and a frame.
    for sb in &samplers {
        app.set_plugin(*sb, None);
    }
    host.frame(&mut app, &mut e, &mut p);
    for (row, sb) in SUBJECTS.iter().zip(&samplers) {
        let lane = p.lane(*row).expect("a lane");
        let gone = host.sampler_have.get(&(lane.slot.0 as usize)).cloned().flatten();
        assert_eq!(gone, None, "row {row}'s lane is still recorded holding {gone:?} with its sampler gone");
        assert_eq!(face(&app, *sb), "", "row {row}'s sampler face still shows the file of a sampler taken out");
    }
    cleanup(&dir, &[TAG]);
}

// ── a built-in face, its knobs and the automation's overlay ─────────────────

/// The EQ's fifth node: its frequency (the curve's) and its gain (a hand's).
const FREQ: u32 = 22;
const GAIN: u32 = 23;

/// **What the host does with what the app asked of it**, for the face's
/// requests: a knob turned goes down the host's own `SetInsertParam` road
/// (`DesktopApp::set_insert_param`: set on the bay's unit through the
/// bridge, and the face pushed back); everything else is dropped. Answers
/// the `(bay, id, value)` of each knob turned.
fn serve(app: &mut TangentApp, e: &mut Engine, p: &Bridge) -> Vec<(SeatBay, u32, f32)> {
    let mut turned = Vec::new();
    while let Some(r) = app.take_recorder_request() {
        if let RecorderRequest::SetInsertParam { bay, id, value } = r {
            DesktopApp::set_insert_param(app, p, Some(&mut *e), bay, id, value);
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
/// - the reconcile's load (`load_bay`) seats each EQ and pushes its face by
///   the rack's identity (`push_builtin_faces`), so the app has every
///   subject's face, row 1's face;
/// - a knob turned on it asks the host by the bay
///   (`RecorderRequest::SetInsertParam`), and the host's own arm sets it;
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
    // Every EQ seated by the host's reconcile, at its defaults (no state on
    // the shelf), its face pushed with the load.
    let mut host = Host::at(&dir);
    let (mut e, p) = live(&mut app, &mut host);
    let ids = app.track_ids();
    let eq = |row: usize| SeatBay::new(Node::Track(ids[row]), Rack::Chain(0), 1);
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

/// **Hold the comps of `comps` to a key from row 150, one subject at a
/// time** (the last of `comps` the control): the keyed comp squeezes its
/// row as row 1's does, on both engines, and every other comp holds still
/// and leaves its row at the control's level. `desk` builds each keyed desk
/// (its key drawn to the subject at `k`) with the comps' states on the
/// host's shelf; each desk's licence store is named `<prefix>-<row>`.
/// Answers those names.
fn hold_keys(prefix: &str, comps: &[usize], path: &Path, dir: &Path, desk: &dyn Fn(usize, &Host) -> Settings) -> Vec<String> {
    let control = comps.len() - 1;
    let mut squeezed = Vec::new();
    let mut tags = Vec::new();
    for (k, keyed) in SUBJECTS.into_iter().enumerate() {
        let tag = format!("{prefix}-{keyed}");
        let mut host = Host::at(dir);
        let s = desk(k, &host);
        let mut app = open_saved(&tag, s, path);
        let (mut e, mut p) = live(&mut app, &mut host);
        host.frame(&mut app, &mut e, &mut p);
        let Built { engine: mut x, bridge: xb } = export(&app, (&e, &p));
        for (road, (gr, lv)) in [("live", squeeze(&app, &mut e, &p, comps)), ("export", squeeze(&app, &mut x, &xb, comps))] {
            assert!(lv[control] > 0.005, "{road}: the control's quiet tone is silent ({})", lv[control]);
            for (j, row) in comps.iter().enumerate() {
                if j == k {
                    assert!(gr[j] > 6.0, "{road}: a key from row 150 squeezed row {row}'s comp only {:.2} dB", gr[j]);
                    assert!(lv[j] < 0.7 * lv[control], "{road}: row {row}'s keyed comp left its row at {}, the control at {}", lv[j], lv[control]);
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
    tags
}

/// **A key from row 150 reaches a comp past slot 31 as it reaches row 1's,
/// live and in the export, and no other comp** (W7-slot-gaps item 4). Every
/// subject and row 2 hold a hard comp over a quiet tone of their own (a
/// lane with nothing to play runs no rack, so a comp on one squeezes
/// nothing to read), seated by the host's reconcile from its shelf; the
/// desk's key (`insert_keys`, by the comp's bay and the source's track) is
/// drawn from row 150, past the old desk, to one subject at a time, and a
/// loud clip plays on row 150. The keyed comp squeezes its track as row 1's
/// does when it is the keyed one, on both engines, and every other comp
/// holds still and leaves its track at row 2's level. Keys past slot 31
/// needed no fix: this proves them through the bridge, both ways. The key
/// to a BUS is held at the engine
/// (`gaps::a_key_from_lane_40_reaches_a_comp_on_33_and_on_a_bus`) and from
/// the desk by the bus test below.
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
    let desk = |k: usize, host: &Host| {
        let mut s = wide_desk();
        for &row in &comps {
            s.rows[row].kind = Some(RowKind::Audio);
            s.plugins.insert(bay_of(&s, row, Rack::Chain(0), 0), COMP_PATH.to_owned());
            host.shelve(bay_of(&s, row, Rack::Chain(0), 0), COMP_PATH, &hard_comp());
        }
        s.rows[150].kind = Some(RowKind::Audio);
        let from = Node::Track(s.rows[150].id);
        s.insert_keys.set(bay_of(&s, SUBJECTS[k], Rack::Chain(0), 0), Some(from));
        s
    };
    let tags = hold_keys("gaps-keys", &comps, &path, &dir, &desk);
    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
    cleanup(&dir, &tags);
}

/// The quiet track row `i`'s bus is fed from, in the bus tests below.
fn feeder(i: usize) -> usize {
    101 + i
}

/// **A key reaches a comp on a BUS past slot 31 as it reaches row 1's, live
/// and in the export, and no other comp** (W7-slot-gaps item 4, its bus
/// half, run since W7-join). Every subject and row 2 a bus with a hard comp
/// in its first bay, each fed at unity by a quiet track of its own (rows 101
/// to 107), and the desk's key drawn from row 150 to one subject's comp at a
/// time while a loud clip plays there. The bridge makes each bus row a BUS
/// lane and carries each feeder's send to it (`Bridge::read_sends`), so
/// every comp has its feeder's tone to squeeze and the control's to be
/// measured against; the engine's own door is
/// `gaps::a_key_from_lane_40_reaches_a_comp_on_33_and_on_a_bus`, and this is
/// the road from the desk to it.
#[test]
fn a_key_reaches_a_comp_on_a_bus_past_slot_31_as_on_row_1_live_and_in_the_export() {
    use tangent_ui::desk_model::Send as DeskSend;
    let dir = scratch("bus-keys");
    wav(&dir.join("media").join("loud.wav"), 0.9, true);
    wav(&dir.join("media").join("soft.wav"), 0.01, false);
    let buses: Vec<usize> = SUBJECTS.into_iter().chain([CONTROL]).collect();
    let mut doc = Arrangement::default();
    doc.add_clip(audio(150, "loud.wav"));
    for i in 0..buses.len() {
        doc.add_clip(audio(feeder(i), "soft.wav"));
    }
    let path = tangent_ui::arrangement::doc_in(&dir);
    doc.save_to(&path);
    let desk = |k: usize, host: &Host| {
        let mut s = wide_desk();
        for (i, &bus) in buses.iter().enumerate() {
            s.rows[bus].kind = Some(RowKind::Bus);
            s.plugins.insert(bay_of(&s, bus, Rack::Chain(0), 0), COMP_PATH.to_owned());
            host.shelve(bay_of(&s, bus, Rack::Chain(0), 0), COMP_PATH, &hard_comp());
            let to = Node::Track(s.rows[bus].id);
            s.rows[feeder(i)].kind = Some(RowKind::Audio);
            s.rows[feeder(i)].sends.push(DeskSend { to, level: 1.0, pre: false });
        }
        s.rows[150].kind = Some(RowKind::Audio);
        let from = Node::Track(s.rows[150].id);
        s.insert_keys.set(bay_of(&s, SUBJECTS[k], Rack::Chain(0), 0), Some(from));
        s
    };
    let tags = hold_keys("gaps-bus-keys", &buses, &path, &dir, &desk);
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
/// knobs elsewhere, a host frame over it, which is all a frame after a drag
/// is) publishes no plan, and the playing plan's gains are bit for bit
/// those of a plan built at the new levels; the export carries the new
/// levels. And a knob taken to nothing republishes without its entry.
/// Before, a plan carried no send of a strip's at all, and the knob reached
/// the return only beside the plan.
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

    let mut host = Host::at(&dir);
    let mut app = open_saved(tags[0], send_desk(&first), &path);
    let (mut e, mut p) = live(&mut app, &mut host);
    assert_eq!(fx_entries(&p), entries(&first), "the live plan does not carry each knob as its strip's Return 0 entry");
    {
        let Built { bridge: xb, .. } = export(&app, (&e, &p));
        assert_eq!(fx_entries(&xb), entries(&first), "the export's plan does not carry each knob as its strip's Return 0 entry");
    }
    let mut out = vec![0.0f32; 512 * 2];
    e.render_offline(&mut out);

    // Every knob turned: no plan, and the playing plan holds the new levels.
    let mut moved = open_saved(tags[1], send_desk(&turned), &path);
    let plans = p.stats().plans;
    host.frame(&mut moved, &mut e, &mut p);
    e.render_offline(&mut out);
    assert_eq!(p.stats().plans, plans, "a knob turned published a plan of its own");
    let (mut fresh, _fresh_bridge) = live(&mut open_saved(tags[2], send_desk(&turned), &path), &mut Host::at(&dir));
    fresh.render_offline(&mut out);
    assert_eq!(gains(&e), gains(&fresh), "the playing plan's gains are not a plan's built at the turned knobs");
    {
        let Built { bridge: xb, .. } = export(&moved, (&e, &p));
        assert_eq!(fx_entries(&xb), entries(&turned), "the export does not carry the turned knobs");
    }

    // Row 200's knob to nothing: its entry goes, by a plan.
    let mut shut = turned.clone();
    shut[SUBJECTS.len() - 1] = 0.0;
    let mut shut_app = open_saved(tags[3], send_desk(&shut), &path);
    host.frame(&mut shut_app, &mut e, &mut p);
    assert_eq!(p.stats().plans, plans + 1, "a knob taken to nothing published no plan");
    let mut want = entries(&turned);
    want.pop();
    assert_eq!(fx_entries(&p), want, "a knob at nothing still carries an entry");
    cleanup(&dir, &tags);
}

/// **The control a curve on a strip's send to `bus` is drawn on**:
/// `AutoParam::SendTo { to: bus }`, the post-fader send's.
fn send_to(bus: TrackId) -> tangent_ui::automation::AutoParam {
    tangent_ui::automation::AutoParam::SendTo { to: bus }
}

/// **A curve on a strip's send to a bus plays on a track past slot 31 as on
/// row 1, live and in the export** (W7-slot-gaps item 5, its curves, run
/// since W7-join). Every subject an audio track playing a steady tone into a
/// bus of its own (rows 101 to 106) by a send at unity, and a curve on that
/// send holding it at a quarter: each bus hears a quarter of its track, as
/// row 1's does, on both engines (`Bridge::send_curves`, which the live
/// push and `full_build_with` both hand the engine). Without the curve a
/// bus hears its track whole. The control the curve is drawn on is
/// [`send_to`]'s, which `engine_bridge::send_bus_of` reads.
#[test]
fn a_send_curve_to_a_bus_past_slot_31_plays_as_on_row_1_live_and_in_the_export() {
    use tangent_ui::automation::{Curve, Point, Shape};
    use tangent_ui::desk_model::Send as DeskSend;
    const TAGS: [&str; 2] = ["gaps-send-curves-save", "gaps-send-curves"];
    let dir = scratch("send-curves");
    wav(&dir.join("media").join("tone.wav"), 0.5, false);
    let mut s = wide_desk();
    let mut doc = Arrangement::default();
    let mut buses = Vec::new();
    for (i, row) in SUBJECTS.into_iter().enumerate() {
        let bus = feeder(i);
        s.rows[bus].kind = Some(RowKind::Bus);
        s.rows[row].kind = Some(RowKind::Audio);
        let to = Node::Track(s.rows[bus].id);
        s.rows[row].sends.push(DeskSend { to, level: 1.0, pre: false });
        doc.add_clip(audio(row, "tone.wav"));
        buses.push((bus, s.rows[bus].id));
    }
    // The curves go into a document the app wrote, which carries its desk:
    // a legacy document's reader drops every lane past 32 (see the faces).
    let path = {
        let mut first = open(TAGS[0], s, &doc, &dir);
        first.save_project_doc();
        first.project_doc().expect("the saved project").to_path_buf()
    };
    let mut saved = Arrangement::load_from(&path);
    for (row, (_, bus)) in SUBJECTS.into_iter().zip(&buses) {
        let param = send_to(*bus);
        let hold = |at_s| Point { at_s, value: 0.25, shape: Shape::Hold };
        saved.automation.set(Curve::sanitised(row, param, vec![hold(0.0), hold(30.0)]));
    }
    saved.save_to(&path);
    let mut app = open_saved(TAGS[1], wide_desk(), &path);
    let (mut e, p) = live(&mut app, &mut Host::at(&dir));
    let Built { engine: mut x, bridge: xb } = export(&app, (&e, &p));
    let rows: Vec<usize> = SUBJECTS.into_iter().chain(buses.iter().map(|(b, _)| *b)).collect();
    for (road, got) in [("live", levels(&mut e, &p, &rows)), ("export", levels(&mut x, &xb, &rows))] {
        let n = SUBJECTS.len();
        let ratio = |i: usize| got[n + i] / got[i];
        let reference = ratio(0);
        assert!(reference > 0.05 && reference < 0.5, "{road}: row 1's bus heard {reference} of its track, not a quarter");
        for (i, row) in SUBJECTS.iter().enumerate() {
            let r = ratio(i);
            assert!((r - reference).abs() <= 0.02 * reference, "{road}: row {row}'s bus heard {r} of its track, row 1's {reference}");
        }
    }
    cleanup(&dir, &TAGS);
}

// ── the mixing leftovers, through the bridge (W7-slot-gaps item 6) ──────────
//
// The engine holds each of these at the width slots
// (`slot_engine::buses::*_at_every_width`); what this adds is the road from
// the desk to them, live and in an export, now that the bridge makes a bus
// row a BUS lane and carries every send to one (W7-join item 2).

/// The bus that looks ahead, which every leftover below crosses.
const AHEAD: usize = 120;

/// The source row a bus node is fed from, by a route.
const SOURCE: usize = 150;

/// **The denoise with its bank and its lookahead, nothing learned**: an
/// honest 3 ms delay and nothing else, the engine leftovers' own unit.
fn lookahead() -> Vec<u8> {
    state(
        crate::builtin_fx::DENOISE_PATH,
        &[(0, 0.0), (5, 1.0), (6, crate::units::denoise::DenoiseKernel::Bank.dial())],
        &[],
    )
}

/// A second of silence with one frame at `level` at frame `at`, as a float
/// WAV: an impulse whose frame says where a path delivered it.
fn impulse_wav(path: &Path, at: usize, level: f32) {
    use tangent_record::wav::{Bext, SampleFormat, WavSpec, WavWriter, Wallclock};
    let spec = WavSpec {
        sample_rate: RATE,
        channels: 2,
        format: SampleFormat::Float32,
    };
    let bext = Bext::new(Wallclock::from_unix(0, 0), spec);
    let mut w = WavWriter::create(path, spec, &bext).expect("a file");
    let mut samples = vec![0.0f32; RATE as usize * 2];
    samples[at * 2] = level;
    samples[at * 2 + 1] = level;
    w.write_interleaved(&samples).expect("its audio");
    w.finish().expect("closed");
}

/// **[`wide_desk`] a row wider** (row 201, for row 200's member), with row
/// `ahead`, when given, a bus whose first bay looks ahead, its state on
/// `host`'s shelf for the reconcile to seat it from.
fn desk_ahead_at(host: &Host, ahead: Option<usize>) -> Settings {
    let mut s = wide_desk();
    let next = s.next_track_id;
    s.rows.push(TrackRow::vacant(TrackId(next as u32)));
    s.next_track_id = next + 1;
    if let Some(at) = ahead {
        s.rows[at].kind = Some(RowKind::Bus);
        let sb = bay_of(&s, at, Rack::Chain(0), 0);
        s.plugins.insert(sb, crate::builtin_fx::DENOISE_PATH.to_owned());
        host.shelve(sb, crate::builtin_fx::DENOISE_PATH, &lookahead());
    }
    s
}

/// Row `from`'s post-fader send at unity to the bus on row `to`.
fn send_unity(s: &mut Settings, from: usize, to: usize) {
    let to = Node::Track(s.rows[to].id);
    s.rows[from].sends.push(tangent_ui::desk_model::Send { to, level: 1.0, pre: false });
}

/// **What a roll from zero makes**: the master, and each of `rows`' stem
/// columns (wet, or dry), over `secs` seconds. The tap is armed for those
/// rows' lanes and no others through the host's own door (`arm_stem_tap`,
/// as a pass arms it), and each column is read as a pass writes it.
fn roll_stems(e: &mut Engine, p: &Bridge, rows: &[usize], dry: bool, secs: f64) -> (Vec<f32>, Vec<Vec<f32>>) {
    let slots: Vec<Slot> = rows.iter().map(|r| p.lane(*r).expect("a row's lane").slot).collect();
    crate::desktop::arm_stem_tap(e, true, &slots, dry);
    e.set_transport(1, 0);
    e.set_rolling(true);
    let mut out = vec![0.0f32; 512 * 2];
    let mut master = Vec::new();
    let mut cols = vec![Vec::new(); rows.len()];
    let mut scratch = Vec::new();
    for _ in 0..((secs * f64::from(RATE)) as usize).div_ceil(512) {
        out.fill(0.0);
        e.render_offline(&mut out);
        master.extend_from_slice(&out);
        scratch.clear();
        e.drain_stems(&mut scratch);
        let wide = e.stem_frame_width().max(2);
        for (col, slot) in cols.iter_mut().zip(&slots) {
            let c = e.stem_column(*slot).expect("the tap was armed for the row's lane");
            for f in 0..scratch.len() / wide {
                col.extend_from_slice(&scratch[f * wide + c * 2..f * wide + c * 2 + 2]);
            }
        }
    }
    (master, cols)
}

/// The frame of a column's loudest sample.
fn peak_at(col: &[f32]) -> usize {
    let mut best = (0usize, 0.0f32);
    for (i, v) in col.chunks_exact(2).enumerate() {
        if v[0].abs() > best.1 {
            best = (i, v[0].abs());
        }
    }
    best.0
}

/// The frame a column first carries anything.
fn onset(col: &[f32]) -> Option<usize> {
    col.chunks_exact(2).position(|f| f[0].abs() > 1e-4 || f[1].abs() > 1e-4)
}

fn rms(xs: &[f32]) -> f64 {
    (xs.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / xs.len().max(1) as f64).sqrt()
}

/// **A lined node's dry stem lands with its wet stem past slot 31 as on row
/// 1, live and in the export** (W7-slot-gaps item 6, through the bridge). A
/// node at each subject in turn sends at unity into bus 120, whose first bay
/// looks ahead: a folder over the row after it, which plays an impulse, and
/// a bus fed by a route from row 150, which plays it. Its way out waits on
/// its line so it lands with the path through bus 120; its racks are empty,
/// so its dry stem is its wet stem, and each must peak on the frame the
/// other does and the frame row 1's node's do, on the live engine the bridge
/// drives and on the export's. The dry stem was left early by that wait.
#[test]
fn a_lined_nodes_dry_stem_lands_with_its_wet_stem_past_slot_31_as_on_row_1_live_and_in_the_export() {
    let dir = scratch("dry-stems");
    impulse_wav(&dir.join("media").join("impulse.wav"), 4_000, 0.5);
    let mut tags = Vec::new();
    for folder in [true, false] {
        let mut reference = None;
        for node in SUBJECTS {
            let tag = format!("gaps-dry-{}-{node}", if folder { "folder" } else { "bus" });
            let host = Host::at(&dir);
            let mut s = desk_ahead_at(&host, Some(AHEAD));
            let mut doc = Arrangement::default();
            let source = if folder {
                s.rows[node].kind = Some(RowKind::Audio);
                s.rows[node].folder = true;
                s.rows[node + 1].kind = Some(RowKind::Audio);
                s.rows[node + 1].in_folder = true;
                node + 1
            } else {
                s.rows[node].kind = Some(RowKind::Bus);
                s.rows[SOURCE].kind = Some(RowKind::Audio);
                let (from, to) = (Node::Track(s.rows[SOURCE].id), Node::Track(s.rows[node].id));
                s.routes.add(tangent_ui::routing::Route::new(from, tangent_ui::routing::Target::Node(to)));
                SOURCE
            };
            send_unity(&mut s, node, AHEAD);
            doc.add_clip(audio(source, "impulse.wav"));
            let path = tangent_ui::arrangement::doc_in(&dir);
            doc.save_to(&path);
            let mut app = open_saved(&tag, s, &path);
            let (mut wet_e, wet_p) = live(&mut app, &mut Host::at(&dir));
            let (mut dry_e, dry_p) = live(&mut app, &mut Host::at(&dir));
            let Built { engine: mut wet_x, bridge: wet_xb } = export(&app, (&wet_e, &wet_p));
            let Built { engine: mut dry_x, bridge: dry_xb } = export(&app, (&wet_e, &wet_p));
            let roads = [
                ("live", (&mut wet_e, &wet_p), (&mut dry_e, &dry_p)),
                ("export", (&mut wet_x, &wet_xb), (&mut dry_x, &dry_xb)),
            ];
            for (road, (we, wp), (de, dp)) in roads {
                let (_, wet) = roll_stems(we, wp, &[node], false, 0.3);
                let (_, dry) = roll_stems(de, dp, &[node], true, 0.3);
                assert!(
                    we.published_max_latency_for_test() > 100,
                    "{road}: bus 120's lookahead is not in the alignment (node {node}, folder {folder})"
                );
                let (wet, dry) = (&wet[0], &dry[0]);
                assert!(wet.iter().any(|v| *v != 0.0) && dry.iter().any(|v| *v != 0.0), "{road}: node {node}'s stem is silent (folder {folder})");
                assert_eq!(peak_at(dry), peak_at(wet), "{road}: node {node}'s dry stem is off its wet stem's frame (folder {folder})");
                let row_1 = *reference.get_or_insert(peak_at(wet));
                assert_eq!(peak_at(wet), row_1, "{road}: node {node} lands on another frame than row 1's (folder {folder})");
            }
            tags.push(tag);
        }
    }
    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
    cleanup(&dir, &tags);
}

/// **A bare kit sent into a bus that looks ahead lands with its own way out
/// past slot 31 as on row 1, live; and the export carries the send as the
/// live plan does** (W7-slot-gaps item 6, through the bridge). The bus at
/// each subject in turn looks ahead, and the drum bus sends to it at unity,
/// or at nothing, from the desk's file (no gesture makes a drum bus send).
/// No track holds the drumpad, so a strike on channel 10 plays the bare kit
/// (given here by the engine's test door, as the drumpad window played bare
/// gives it). Live, the desk's alignment covers the kit's path through the
/// bus, and the bus's share of the master peaks on the frame the kit's own
/// way out does, which is the frame it does through row 1's bus. The drum
/// bus had no rank in the compensation graph, so the share came out the
/// bus's lookahead late. An export hears no live MIDI, so no bare kit is
/// struck in one; its plan carries the drum bus's send to the bus's lane
/// all the same, which is what keeps the two engines one desk.
#[test]
fn a_bare_kit_sent_into_a_bus_that_looks_ahead_lands_with_its_own_way_out_past_slot_31_as_on_row_1() {
    use crate::desk_plan::InSend;
    let dir = scratch("bare-kit");
    let kit = || {
        let mut kit = crate::instrument::PadKit::default();
        let mut hit = vec![0.0f32; 4_800 * 2];
        hit[0] = 0.25;
        hit[1] = 0.25;
        kit.pads[0] = Some(std::sync::Arc::new(tangent_record::decode::Clip { samples: hit, rate: RATE, source: PathBuf::new() }));
        kit
    };
    let mut tags = Vec::new();
    let mut reference = None;
    for bus in SUBJECTS {
        let mut heard = Vec::new();
        for level in [0.0, 1.0] {
            let tag = format!("gaps-bare-kit-{bus}-{level}");
            let host = Host::at(&dir);
            let mut s = desk_ahead_at(&host, Some(bus));
            let to = Node::Track(s.rows[bus].id);
            s.specials.drums.sends.push(tangent_ui::desk_model::Send { to, level, pre: false });
            let doc = Arrangement::default();
            let mut app = open(&tag, s, &doc, &dir);
            let (mut e, p) = live(&mut app, &mut Host::at(&dir));
            if level > 0.0 {
                let Built { bridge: xb, .. } = export(&app, (&e, &p));
                let sent = InSend { from: SendFrom::Drums, to: SendTo::Lane(xb.lane(bus).expect("the bus's lane").slot), level: 1.0, pre: false };
                let plan = xb.published_for_test().expect("the export's plan");
                assert!(plan.sends.contains(&sent), "the export's plan lost the drum bus's send to bus {bus}");
                let live_sent = InSend { to: SendTo::Lane(p.lane(bus).expect("the bus's lane").slot), ..sent };
                assert!(p.published_for_test().expect("a plan").sends.contains(&live_sent), "fixture: the live plan's send");
            }
            assert!(e.set_pad_kit(0, kit()), "the bare kit went nowhere");
            let mut out = vec![0.0f32; 512 * 2];
            for _ in 0..4 {
                e.render_offline(&mut out);
            }
            e.send_midi(0, &[0x99, DRUM_PAD_BASE, 127]);
            let (master, _) = roll_stems(&mut e, &p, &[], false, 0.1);
            assert!(
                e.published_max_latency_for_test() > 100,
                "the bare kit's path through bus {bus} is not in the desk's alignment"
            );
            heard.push(master);
            tags.push(tag);
        }
        let (alone, both) = (&heard[0], &heard[1]);
        assert!(alone.iter().any(|v| *v != 0.0), "the bare kit is silent (bus {bus})");
        let share: Vec<f32> = both.iter().zip(alone).map(|(b, a)| b - a).collect();
        assert!(share.iter().any(|v| v.abs() > 1e-4), "nothing of the kit came out of bus {bus}");
        assert_eq!(peak_at(&share), peak_at(alone), "the bare kit's send into bus {bus} landed off its own way out's frame");
        let row_1 = *reference.get_or_insert(peak_at(alone));
        assert_eq!(peak_at(alone), row_1, "the kit through bus {bus} lands on another frame than through row 1's");
    }
    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
    cleanup(&dir, &tags);
}

/// **The click pays a bus that looks ahead uncompensated past slot 31 as on
/// row 1, live; and stays out of the export, holding it back by nothing**
/// (W7-slot-gaps item 6, through the bridge, the pin kept). The click sends
/// at unity, from the desk's file, into the bus at each subject in turn,
/// which looks ahead or does not. Live, the click is a cue, summed at once
/// so the beat is heard when it falls: what it sends reaches the bus at
/// once, the bus's lookahead delays it by exactly what the bus looks ahead,
/// and the desk's alignment stays at nothing, on row 1's frames at every
/// width. The export leaves the click out of the file: the bus hears
/// nothing, and the click's send holds the file back by nothing.
#[test]
fn the_click_pays_a_bus_that_looks_ahead_uncompensated_past_slot_31_and_stays_out_of_the_export() {
    let dir = scratch("click");
    let mut tags = Vec::new();
    let mut reference = None;
    for bus in SUBJECTS {
        let mut onsets = Vec::new();
        for ahead in [false, true] {
            let tag = format!("gaps-click-{bus}-{ahead}");
            let host = Host::at(&dir);
            let mut s = desk_ahead_at(&host, ahead.then_some(bus));
            s.rows[bus].kind = Some(RowKind::Bus);
            let to = Node::Track(s.rows[bus].id);
            s.specials.click.sends.push(tangent_ui::desk_model::Send { to, level: 1.0, pre: false });
            let doc = Arrangement::default();
            let mut app = open(&tag, s, &doc, &dir);
            let (mut e, p) = live(&mut app, &mut Host::at(&dir));
            if ahead {
                let Built { engine: mut x, bridge: xb } = export(&app, (&e, &p));
                let (_, cols) = roll_stems(&mut x, &xb, &[bus], false, 0.25);
                assert!(cols[0].iter().all(|v| *v == 0.0), "the click reached bus {bus} in the export");
                assert_eq!(x.published_max_latency_for_test(), 0, "the click's send into bus {bus} held the export back");
            }
            e.set_metronome_enabled(true);
            e.set_metronome_gain(1.0);
            let (_, cols) = roll_stems(&mut e, &p, &[bus], false, 0.25);
            assert_eq!(e.published_max_latency_for_test(), 0, "the click's send into bus {bus} held the desk back");
            onsets.push(onset(&cols[0]).unwrap_or_else(|| panic!("the click never reached bus {bus} (ahead {ahead})")));
            tags.push(tag);
        }
        let (at, late) = (onsets[0], onsets[1]);
        assert!(late > at + 100, "the click through bus {bus} is not late by the bus's lookahead ({at} then {late})");
        let row_1 = *reference.get_or_insert((at, late));
        assert_eq!((at, late), row_1, "the click through bus {bus} lands on other frames than through row 1's");
    }
    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
    cleanup(&dir, &tags);
}

/// **A track in a folder that is also routed into a bus is summed by both,
/// past slot 31 as on row 1, live and in the export** (W7-slot-gaps item 6,
/// through the bridge: the old fold rule, kept on purpose and pinned so a
/// change is deliberate). The folder at each subject in turn holds the row
/// after it, which plays a tone and is routed into bus 120. The folder's
/// stem and the bus's each carry the track whole, and the master hears it
/// twice over, in step, as loud as with the folder on row 1. Changing either
/// rule changes saved projects' sound.
#[test]
fn a_folded_track_routed_into_a_bus_is_summed_by_both_past_slot_31_as_on_row_1_live_and_in_the_export() {
    let dir = scratch("fold-pin");
    wav(&dir.join("media").join("tone.wav"), 0.3, false);
    let near = |a: f64, b: f64| (a - b).abs() <= 1e-3 * b;
    let tail = |v: &[f32]| v[v.len() - 512 * 2 * 4..].to_vec();
    let mut tags = Vec::new();
    let mut reference = None;
    for folder in SUBJECTS {
        let track = folder + 1;
        let mut heard: Vec<Vec<(Vec<f32>, Vec<Vec<f32>>)>> = Vec::new();
        for routed in [false, true] {
            let tag = format!("gaps-fold-{folder}-{routed}");
            let mut s = desk_ahead_at(&Host::at(&dir), None);
            s.rows[AHEAD].kind = Some(RowKind::Bus);
            s.rows[folder].kind = Some(RowKind::Audio);
            s.rows[folder].folder = true;
            s.rows[track].kind = Some(RowKind::Audio);
            s.rows[track].in_folder = true;
            if routed {
                let (from, to) = (Node::Track(s.rows[track].id), Node::Track(s.rows[AHEAD].id));
                s.routes.add(tangent_ui::routing::Route::new(from, tangent_ui::routing::Target::Node(to)));
            }
            let mut doc = Arrangement::default();
            doc.add_clip(audio(track, "tone.wav"));
            let mut app = open(&tag, s, &doc, &dir);
            let (mut e, p) = live(&mut app, &mut Host::at(&dir));
            let Built { engine: mut x, bridge: xb } = export(&app, (&e, &p));
            let rows = [folder, AHEAD, track];
            heard.push(vec![roll_stems(&mut e, &p, &rows, false, 0.25), roll_stems(&mut x, &xb, &rows, false, 0.25)]);
            tags.push(tag);
        }
        for (i, road) in ["live", "export"].into_iter().enumerate() {
            let (alone, _) = &heard[0][i];
            let (twice, cols) = &heard[1][i];
            let (held, bus, own) = (rms(&tail(&cols[0])), rms(&tail(&cols[1])), rms(&tail(&cols[2])));
            assert!(rms(&tail(alone)) > 1e-2, "{road}: the track in folder {folder} is silent");
            assert!(near(held, own), "{road}: folder {folder} no longer carries its child whole ({held} against {own})");
            assert!(near(bus, own), "{road}: the bus no longer carries the track in folder {folder} whole ({bus} against {own})");
            let (both, one) = (rms(&tail(twice)), rms(&tail(alone)));
            assert!(near(both, 2.0 * one), "{road}: the master does not hear the track in folder {folder} twice, in step ({both} against {one})");
            let row_1 = *reference.get_or_insert(both);
            assert!(near(both, row_1), "{road}: the master hears folder {folder}'s track at {both}, row 1's at {row_1}");
        }
    }
    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
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

/// **Every frame the host runs goes down the roads [`Host::frame`] drives**:
/// the frame's reconcile loads its bay through `load_bay` and walks through
/// `next_owed_bay`, the sampler reload is `deliver_samples`, the kits'
/// reload `deliver_kits`, their knobs `push_pad_knobs`, a lane that came or
/// went is forgotten by `forget_moved_slots` (and the curves last handed
/// over with it), the curves go by `push_curves`, a face's knob is set by
/// `set_insert_param`, and the kits are owed by the sync's own answer
/// (`Synced::owes_kits`), a retry or an edit. A road the frame took by other
/// lines would leave this suite proving a function nothing calls, and a
/// rule for when the kits are owed that the frame did not keep would leave
/// it proving a rule nothing keeps.
#[test]
fn the_frame_runs_the_roads_this_suite_drives() {
    let whole = include_str!("desktop.rs");
    let src = &whole[..whole.find("\nmod tests {").expect("the tests module")];
    // Each method to its own closing brace, the first at the impl's indent.
    let body_of = |name: &str| {
        let at = src.find(name).unwrap_or_else(|| panic!("`{name}` is gone"));
        &src[at..at + src[at..].find("\n    }\n").expect("its end")]
    };
    let frame = body_of("fn after_frame(&mut self");
    for road in [
        "Self::bays_wanted(&self.app, &self.bridge)",
        "Self::next_owed_bay(&wanted,",
        "Self::load_bay(",
        "Self::set_insert_param(",
        "self.push_automation();",
        "if std::mem::take(&mut self.recorder.pads_retry) || self.app.take_drumpad_dirty() {",
    ] {
        assert!(frame.contains(road), "the frame no longer takes `{road}`");
    }
    let push = body_of("fn push_monitor_settings(&mut self)");
    for road in ["Self::forget_moved_slots(", "Self::push_pad_knobs(", "self.pushed_auto = None;", "if synced.owes_kits() {"] {
        assert!(push.contains(road), "the frame's push no longer takes `{road}`");
    }
    assert!(
        body_of("fn push_automation(&mut self)").contains("Self::push_curves(self.recorder.engine.as_mut(), &self.bridge, &self.app, &mut self.pushed_auto)"),
        "the curves go another road"
    );
    assert!(body_of("fn reload_sampler(&mut self)").contains("Self::deliver_samples("), "the sampler reload goes another road");
    assert!(body_of("fn reload_pads(&mut self)").contains("Self::deliver_kits("), "the kits' reload goes another road");
}

// ── several instruments on one track ────────────────────────────────────────

/// **Each of `rows`' stems** on an engine whose bridge is `p`: a third of a
/// second rolled from zero with the stem tap armed for their lanes, in
/// `rows`' order.
fn stems_of(e: &mut Engine, p: &Bridge, rows: &[usize]) -> Vec<Vec<f32>> {
    let slots: Vec<Slot> = rows.iter().map(|r| p.lane(*r).expect("a row's lane").slot).collect();
    e.arm_stems(&slots);
    e.set_transport(1, 0);
    e.set_rolling(true);
    let mut out = vec![0.0f32; 1024 * 2];
    let mut stems = Vec::new();
    for _ in 0..(RATE as usize / 3).div_ceil(1024) {
        e.render_offline(&mut out);
        e.drain_stems(&mut stems);
    }
    let width = e.stem_frame_width();
    (0..rows.len())
        .map(|at| stems.chunks_exact(width).flat_map(|f| [f[at * 2], f[at * 2 + 1]]).collect())
        .collect()
}

/// **How far `got` is from the sum of `parts`**, each later by its own frame
/// count, as a share of `got`'s peak.
fn misfit(got: &[f32], parts: &[(&[f32], usize)]) -> f32 {
    let peak = got.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
    (0..got.len())
        .map(|i| {
            let want: f32 = parts.iter().map(|(p, k)| i.checked_sub(2 * k).and_then(|j| p.get(j)).copied().unwrap_or(0.0)).sum();
            (got[i] - want).abs()
        })
        .fold(0.0f32, f32::max)
        / peak
}

/// **Three instruments on one track all play its clip, past slot 31 as on
/// row 1, live and in the export, wherever their bays are** (the owner's
/// "two or more instruments on channel inserts receiving midi simultaneously
/// and playing back on that channel"). Rows 1 and 40 hold the analog in
/// their first bay, the modelled keyboard in their third and the DX7 in
/// their fourth (the extra rack's first); rows 3, 4 and 5 hold each of the
/// three alone, in the same bay. Every row has one note. Before the layering
/// only a first bay's DX7 or keyboard was ever played (the bridge asked the
/// first bay alone), so rows 4 and 5 were silent and rows 1 and 40 were the
/// analog alone; now each subject's stem is the three solo rows summed, the
/// two that enter after the analog held by its delay (or, where the desk's
/// compensation has already held the solo rows by as much, not), and row
/// 40's is row 1's.
///
/// And a kit layered with a voice: row 6 holds the drumpad in its first bay
/// and the analog in its second, row 7 the drumpad alone and row 8 the
/// analog alone in its second bay; the kit's pad 36 and the analog both
/// play row 6's note. A kit alone stopped the lane's walk, rack and all.
#[test]
fn every_instrument_on_a_track_plays_its_notes_past_slot_31_as_on_row_1_live_and_in_the_export() {
    use crate::builtin_fx::ANALOG_PATH;
    use tangent_ui::dialogs::BUILTIN_PATH;
    const TAG: &str = "gaps-multi-inst";
    let dir = scratch("multi-inst");
    wav(&dir.join("media").join("pad.wav"), 0.25, false);
    let mut s = wide_desk();
    let mut doc = Arrangement::default();
    let layered = [1usize, 40];
    let solo = [(3usize, Rack::Chain(0), 0u8, ANALOG_PATH), (4, Rack::Chain(0), 2, KEYS_PATH), (5, Rack::Chain(1), 0, BUILTIN_PATH)];
    let (kit_and_voice, kit, voice) = (6usize, 7usize, 8usize);
    for row in layered.into_iter().chain(solo.iter().map(|x| x.0)).chain([kit_and_voice, kit, voice]) {
        s.rows[row].kind = Some(RowKind::Midi);
        doc.add_clip(notes(row, DRUM_PAD_BASE + 36));
    }
    for row in layered {
        for &(_, rack, bay, path) in &solo {
            s.plugins.insert(bay_of(&s, row, rack, bay), path.to_owned());
        }
    }
    for &(row, rack, bay, path) in &solo {
        s.plugins.insert(bay_of(&s, row, rack, bay), path.to_owned());
    }
    s.plugins.insert(bay_of(&s, kit_and_voice, Rack::Chain(0), 0), DRUMPAD_PATH.to_owned());
    s.plugins.insert(bay_of(&s, kit_and_voice, Rack::Chain(0), 1), ANALOG_PATH.to_owned());
    s.plugins.insert(bay_of(&s, kit, Rack::Chain(0), 0), DRUMPAD_PATH.to_owned());
    s.plugins.insert(bay_of(&s, voice, Rack::Chain(0), 1), ANALOG_PATH.to_owned());
    let path = tangent_ui::arrangement::doc_in(&dir);
    doc.save_to(&path);
    save_kits(&s, &[kit_and_voice, kit], &path);
    let mut host = Host::at(&dir);
    let mut app = open_saved(TAG, s, &path);
    let (mut e, p) = live(&mut app, &mut host);
    let Built { engine: mut x, bridge: xb } = export(&app, (&e, &p));
    let lat = {
        let spec = crate::builtin_fx::spec_for(ANALOG_PATH).expect("the analog");
        spec.create(RATE as f32, &spec.defaults()).latency() as usize
    };
    let rows = [1usize, 40, 3, 4, 5, kit_and_voice, kit, voice];
    for (road, got) in [("live", stems_of(&mut e, &p, &rows)), ("export", stems_of(&mut x, &xb, &rows))] {
        let peak = |xs: &[f32]| xs.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        for (row, stem) in rows.iter().zip(&got) {
            assert!(peak(stem) > 1e-3, "{road}: row {row} is silent");
        }
        let (analog, keys, dx7) = (&got[2][..], &got[3][..], &got[4][..]);
        for (i, row) in layered.iter().enumerate() {
            // The solo keyboard and DX7 rows are held by the desk's
            // compensation exactly as much as the layered row holds them,
            // or not at all; either way the sum fits.
            let err = [0, lat]
                .into_iter()
                .map(|k| misfit(&got[i], &[(analog, 0), (keys, k), (dx7, k)]))
                .fold(f32::MAX, f32::min);
            assert!(err < 1e-4, "{road}: row {row} is not its three instruments summed (off by {err} of its peak)");
        }
        let err = [0, lat]
            .into_iter()
            .map(|k| misfit(&got[5], &[(&got[6], k), (&got[7], 0)]))
            .fold(f32::MAX, f32::min);
        assert!(err < 1e-4, "{road}: the kit and the voice on row {kit_and_voice} are not the two summed (off by {err})");
    }
    // **And a freeze of row 40 is its export**: the freeze renders through
    // the export's builder, so the three instruments land in its file as
    // they play in the export.
    let mut ctx = Furnish {
        live: Some((&e, &p)),
        ..Furnish::default()
    };
    let id = app.track_ids()[40];
    let Built { engine: mut f, bridge: fb } =
        full_build_with(&app, RATE, Timebase::new(), BuildFor::Freeze(id), &mut ctx).expect("a freeze engine");
    assert!(ctx.refused.is_empty(), "the freeze refused {:?}", ctx.refused);
    let frozen = stems_of(&mut f, &fb, &[40]).remove(0);
    let Built { engine: mut x2, bridge: xb2 } = export(&app, (&e, &p));
    let exported = stems_of(&mut x2, &xb2, &[40]).remove(0);
    let err = misfit(&frozen, &[(&exported, 0)]);
    assert!(err < 1e-4, "the freeze of row 40 is not its export (off by {err})");
    cleanup(&dir, &[TAG]);
}
