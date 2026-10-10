//! **The Konami and paywall suite past thirty-two rows** (design 11.2,
//! W6-rows-unlimited): the claims that only mean something once a desk is
//! wider than the one this app used to have, made in this crate so every
//! claim about SOUND is made through the [`Bridge`] the live engine and every
//! export are furnished by (`full_build`), never through a model of it.
//!
//! The gate's own walk (every road, every tier, a forced cap of four, the
//! refusal line, the sentinel) is `tangent-ui`'s `paywall_tests`, and the
//! roads past the old width with each track drawn after scrolling are its
//! `width_tests`. What this file adds is the wide desk being REAL end to
//! end: made by a cheater, saved, reset, reopened by a free install,
//! rendered row by row, edited, saved again and undone across the cap, with
//! the cap meeting it at exactly the count the design says.
//!
//! **Nothing here touches the real home.** Every licence store is a temp
//! directory of the test's own (`composite::tiers`), every project is under
//! the temp dir, and the apps run with the global settings file off
//! (`persist_global_settings`): in a binary crate's test that file is the
//! real `~/.config/tangent`, which `Settings::path` only redirects for
//! `tangent-ui`'s own tests.
//!
//! [`Bridge`]: crate::engine_bridge::Bridge

use std::path::{Path, PathBuf};

use tangent_core::license::LicenseStore;
use tangent_record::audio::Timebase;
use tangent_ui::app::{TangentApp, TrackRefusal};
use tangent_ui::automation::{AutoParam, Shape};
use tangent_ui::desk_id::{Node, Rack, SeatBay, TrackId};
use tangent_ui::dialogs::DialogAction;
use tangent_ui::host::Caps;
use tangent_ui::ports::ProjectRequest;
use tangent_ui::settings::{ResetScope, Settings};

use crate::composite::tiers;
use crate::desk_plan::Slot;
use crate::engine_bridge::{full_build, full_build_with, BuildFor, Bridge, Built, Furnish};
use crate::instrument::Engine;

const RATE: u32 = 48_000;

/// The desktop's capabilities with the one write a binary crate's test may
/// not make taken out: the settings file here is the real one.
const QUIET: Caps = Caps {
    persist_global_settings: false,
    ..Caps::DESKTOP
};

/// **A desk with nothing on it**: every row vacant and no bay seated, so
/// each track below is made by a road and is exactly what that road made,
/// rather than inheriting the fresh install's kit and DX7 (which a render
/// could only make audible with a kit and a cartridge of their own). No
/// welcome card: it is a dialog, and a dialog takes the keys.
fn bare_desk() -> Settings {
    let mut s = Settings {
        show_welcome: false,
        ..Settings::default()
    };
    s.plugins.clear();
    for row in &mut s.rows {
        *row = tangent_ui::desk_model::TrackRow::vacant(row.id);
    }
    s.arm_unsaid = false;
    s
}

fn app_on(store: LicenseStore) -> (egui::Context, TangentApp) {
    let ctx = egui::Context::default();
    let app = TangentApp::with_license(&ctx, bare_desk(), QUIET, store);
    (ctx, app)
}

/// A folder of this test's own under the temp dir, emptied first.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tangent-konami-{}-{:?}-{tag}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir
}

/// A second of seeded stereo noise as a float WAV, loud from its first
/// sample: the clip every audible track below plays.
fn noise_wav(path: &Path, seed: u32) {
    use tangent_record::wav::{Bext, SampleFormat, WavSpec, WavWriter, Wallclock};
    let spec = WavSpec {
        sample_rate: RATE,
        channels: 2,
        format: SampleFormat::Float32,
    };
    let bext = Bext::new(Wallclock::from_unix(0, 0), spec);
    let mut w = WavWriter::create(path, spec, &bext).expect("a noise file");
    let mut x: u32 = seed | 1;
    let mut samples = Vec::with_capacity(RATE as usize * 2);
    for _ in 0..RATE {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let n = (x as f32 / u32::MAX as f32).mul_add(2.0, -1.0) * 0.3;
        samples.push(n);
        samples.push(n * 0.8);
    }
    w.write_interleaved(&samples).expect("its audio");
    w.finish().expect("closed");
}

/// One frame of the whole app at a desktop's size, carrying `events` under
/// `modifiers`: what the window hands it.
fn frame(ctx: &egui::Context, app: &mut TangentApp, events: Vec<egui::Event>, modifiers: egui::Modifiers) {
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1300.0, 900.0))),
            modifiers,
            events,
            ..Default::default()
        },
        |ctx| app.frame(ctx),
    );
}

/// Ctrl+Z, or with `shift` Ctrl+Shift+Z, through the key feed: the chord
/// a hand uses, read where the app reads it.
fn chord_z(ctx: &egui::Context, app: &mut TangentApp, shift: bool) {
    let modifiers = egui::Modifiers {
        ctrl: true,
        shift,
        command: true,
        ..Default::default()
    };
    let press = egui::Event::Key {
        key: egui::Key::Z,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    frame(ctx, app, vec![press], modifiers);
}

/// **What each row's lane made**, through the offline engine every export
/// is built by: `full_build` furnishes it from the app through the one
/// bridge, the transport rolls from zero for a quarter of a second, and each
/// row's lane meter is read by the slot the bridge gave that row's track.
/// `None` for a row the bridge gave no lane. The master's loudest sample
/// comes back too, so "every lane metered" cannot pass on a desk whose
/// lanes never reached the speakers.
fn heard(app: &TangentApp) -> (Vec<Option<f32>>, f32) {
    let spec = tangent_ui::ports::ProjectExportSpec {
        rate: RATE,
        ..Default::default()
    };
    let Built { engine: mut e, bridge } =
        full_build(app, RATE, Timebase::new(), BuildFor::Export(spec)).expect("an offline engine");
    e.set_transport(1, 0);
    e.set_rolling(true);
    let mut out = vec![0.0f32; 1024 * 2];
    let mut master = 0.0f32;
    for _ in 0..(RATE as usize / 4).div_ceil(1024) {
        out.fill(0.0);
        e.render_offline(&mut out);
        master = out.iter().fold(master, |m, v| m.max(v.abs()));
    }
    let rows = (0..app.desk_extent())
        .map(|row| {
            bridge.lane(row).map(|l| {
                let ([a, b], _) = e.lane_reading(l.slot);
                a.max(b)
            })
        })
        .collect();
    (rows, master)
}

/// **A cheater's desk of `n` tracks, each one audible**: a bare desk in a
/// fresh project under `root`, and every track a clip of noise dropped below
/// the last lane, the road that makes a track and gives it a sound in one
/// gesture (design section 10). The first thirty-two land on the bare
/// desk's own hidden rows, in order; the rest grow the desk.
fn cheaters_desk(root: &Path, tag: &str, n: usize) -> (egui::Context, TangentApp) {
    let wav = root.join("noise.wav");
    noise_wav(&wav, 0x5eed);
    let (ctx, mut app) = app_on(tiers::cheater(tag));
    app.bootstrap_project(&root.join("Projects"));
    assert_eq!(app.track_cap(), None, "fixture: the cheater is capped");
    for made in 1..=n {
        let below = app.desk_extent();
        app.import_audio_at(&wav, below, 0.0)
            .unwrap_or_else(|e| panic!("track {made}: the drop below the last lane was refused: {e}"));
        assert_eq!(app.counted_tracks(), made, "the drop below the last lane made no track");
    }
    (ctx, app)
}

/// **The relock, restated** (design 11.2 item 3, W6's done-when): a keyless
/// cheater saves a forty-track project and runs Reset Everything; the
/// session is free again and New is raised; the saved document reopened
/// through the host's apply path is forty rows, each audible offline,
/// counting forty; the forty-first track is refused for the free cap, and
/// one more is allowed once the desk is deleted to thirty-one.
#[test]
fn a_relocked_cheaters_forty_tracks_reopen_whole_and_meet_the_free_cap() {
    const TAG: &str = "konami-relock";
    let root = scratch("relock");
    let (ctx, mut app) = cheaters_desk(&root, TAG, 40);
    let ids = app.track_ids();
    assert_eq!(ids.len(), 40, "fixture: the desk is not forty rows");
    assert!(app.save_project_first(&root.join("Forty.tan")), "the first save did not land");
    let doc = app.project_doc().expect("the saved project").to_path_buf();
    let _ = app.take_project_request();

    // Reset Everything, as its confirmation's button sends it; the frame
    // carries it out (the reset needs the context, for the font).
    app.apply_dialog_action(DialogAction::ResetTo(ResetScope::Everything));
    frame(&ctx, &mut app, Vec::new(), egui::Modifiers::NONE);
    assert!(!tiers::dir(TAG).join("cheat.unlock").exists(), "the reset left the marker on disk");
    assert!(
        !LicenseStore::load_from(tiers::dir(TAG).join("license.key")).cheated(),
        "the store reloads as a cheater after the reset"
    );
    assert_eq!(app.track_cap(), Some(32), "the relocked session is still uncapped");
    assert_eq!(app.take_project_request(), Some(ProjectRequest::NewKeeping), "the reset raised no New");

    // The saved song, opened on the session the reset left: the app applies
    // the document's desk and says so, which is the host's cue to follow
    // with the states (`switch_project`); the engine is furnished through
    // the bridge.
    app.open_project(&doc);
    assert!(app.take_desk_reload(), "the open applied no desk for the host to follow");
    assert_eq!(app.desk_extent(), 40, "the reopened desk is not forty rows");
    assert_eq!(app.track_ids(), ids, "the reopened rows are not the tracks that were saved");
    assert_eq!(app.counted_tracks(), 40);
    let (rows, master) = heard(&app);
    let silent: Vec<usize> = rows.iter().enumerate().filter(|(_, h)| !h.is_some_and(|p| p > 0.0)).map(|(r, _)| r).collect();
    assert!(silent.is_empty(), "rows {silent:?} of the reopened forty are silent offline");
    assert!(master > 0.0, "the forty lanes metered and the master was silent");

    // The forty-first: refused for the free cap, on the gate and on the road.
    assert_eq!(app.may_add_tracks(1), Err(TrackRefusal::FreeCap { have: 40, cap: 32 }));
    app.add_track();
    assert_eq!((app.counted_tracks(), app.desk_extent()), (40, 40), "the forty-first track was made");

    // Deleted to thirty-one, one Remove Track from the Track menu at a time.
    for _ in 0..9 {
        let last = app.desk_extent() - 1;
        app.apply_dialog_action(DialogAction::TrackRemove(last));
    }
    assert_eq!(app.counted_tracks(), 31, "fixture: the removals did not take the desk to thirty-one");
    app.add_track();
    assert_eq!(app.counted_tracks(), 32, "a free desk back under its cap was refused a track");
    app.add_track();
    assert_eq!(app.counted_tracks(), 32, "a free desk at its cap grew");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(tiers::dir(TAG));
}

/// The tracks design 11.2 item 1 names: past the old width, either side of
/// the 64-bit masks' end, and well past both.
const MILESTONES: [usize; 5] = [33, 64, 65, 100, 200];

/// **A cheater's two hundred tracks**, made by Ctrl/Cmd+T except at the
/// [`MILESTONES`], which are clips dropped below the last lane: so the
/// milestones are the only tracks with anything to play, and a lane heard
/// anywhere else is a track heard on the wrong row. Answers the milestone
/// tracks' ids too, in the order they were made: Ctrl/Cmd+T makes each new
/// track under the one it made last, so the rows a milestone stands on
/// move down as tracks are added above it, and only its id says which
/// track it is.
fn two_hundred(root: &Path, tag: &str) -> (egui::Context, TangentApp, Vec<TrackId>) {
    let wav = root.join("noise.wav");
    noise_wav(&wav, 0x200);
    let (ctx, mut app) = app_on(tiers::cheater(tag));
    app.bootstrap_project(&root.join("Projects"));
    let mut loud = Vec::new();
    for made in 1..=200 {
        if MILESTONES.contains(&made) {
            let had = clip_tracks(&app);
            let below = app.desk_extent();
            app.import_audio_at(&wav, below, 0.0).unwrap_or_else(|e| panic!("track {made}: {e}"));
            let new: Vec<TrackId> = clip_tracks(&app).into_iter().filter(|id| !had.contains(id)).collect();
            assert_eq!(new.len(), 1, "track {made}: the drop did not make exactly one track with a clip");
            loud.push(new[0]);
        } else {
            app.add_track();
        }
        assert_eq!(app.counted_tracks(), made, "track {made} was not made");
    }
    (ctx, app, loud)
}

/// The tracks that have a clip, by id.
fn clip_tracks(app: &TangentApp) -> Vec<TrackId> {
    let mut ids: Vec<TrackId> = app.arrangement().clips.iter().filter_map(|c| app.track_at(c.lane)).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// **Every row of a render is the track standing on it**: each of the
/// `loud` tracks metered on whatever row it stands on now, every other row
/// silent, the master loud.
fn only_these_play(what: &str, app: &TangentApp, loud: &[TrackId]) {
    let rows: Vec<usize> = loud
        .iter()
        .map(|id| app.row_of(*id).unwrap_or_else(|| panic!("{what}: track {id:?} is not on the desk")))
        .collect();
    let (heard, master) = heard(app);
    for (row, h) in heard.iter().enumerate() {
        let playing = h.is_some_and(|p| p > 0.0);
        if rows.contains(&row) {
            assert!(playing, "{what}: row {row}, a track with a clip, is silent offline ({h:?})");
        } else {
            assert!(!playing, "{what}: row {row}, a track with nothing on it, is heard ({h:?})");
        }
    }
    assert!(master > 0.0, "{what}: the tracks metered and the master was silent");
}

/// **A cheater's tracks 33, 64, 65, 100 and 200 each play offline** (design
/// 11.2 item 1, its audible half; the roads and the drawing are
/// `tangent-ui`'s `width_tests`): two hundred tracks made, each milestone
/// the track it was made as, on its own lane, and nothing heard elsewhere.
#[test]
fn a_cheaters_tracks_past_the_old_width_each_play_offline() {
    const TAG: &str = "konami-milestones";
    let root = scratch("milestones");
    let (_ctx, app, loud) = two_hundred(&root, TAG);
    assert_eq!(app.counted_tracks(), 200);
    only_these_play("the cheater's desk", &app, &loud);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(tiers::dir(TAG));
}

/// **The downgrade** (design 11.2 item 9, 5.6): a free install opens a
/// cheater's two-hundred-track project whole, plays it, saves it untouched
/// byte for byte, edits it (a track that exists is edited freely, however
/// far past the cap the desk stands), saves the edit whole, and refuses the
/// next track.
#[test]
fn a_free_install_opens_plays_edits_and_saves_two_hundred_tracks_whole() {
    const TAG: &str = "konami-downgrade";
    let root = scratch("downgrade");
    let (_c, mut cheat, loud) = two_hundred(&root, TAG);
    assert!(cheat.save_project_first(&root.join("Wide.tan")), "the cheater's save did not land");
    let doc = cheat.project_doc().expect("the saved project").to_path_buf();
    let ids = cheat.track_ids();
    let saved = std::fs::read(&doc).expect("the saved document");
    drop(cheat);

    let (_ctx, mut free) = app_on(tiers::free("konami-downgrade-free"));
    assert_eq!(free.track_cap(), Some(32), "fixture: the free install is uncapped");
    free.open_project(&doc);
    assert!(free.take_desk_reload(), "the open applied no desk");
    assert_eq!(free.track_ids(), ids, "the free install opened other rows than were saved");
    assert_eq!(free.counted_tracks(), 200);
    only_these_play("the free install's open", &free, &loud);

    // Saved untouched, the document is the same bytes.
    free.save_project_doc();
    assert!(std::fs::read(&doc).expect("the document") == saved, "a free install's save of an untouched wide project changed its bytes");

    // An edit to a track that exists: a clip onto the 150th, which counts
    // no new track, so the cap has nothing to say.
    let wav = root.join("noise.wav");
    assert_eq!(free.may_add_tracks(0), Ok(()));
    free.import_audio_at(&wav, 149, 0.0).expect("an edit to an existing track past the cap was refused");
    assert_eq!(free.counted_tracks(), 200, "the edit counted a track");
    free.save_project_doc();

    // The next track is refused, on the gate and on the road.
    assert_eq!(free.may_add_tracks(1), Err(TrackRefusal::FreeCap { have: 200, cap: 32 }));
    let extent = free.desk_extent();
    free.add_track();
    assert_eq!((free.counted_tracks(), free.desk_extent()), (200, extent), "a free install grew a desk past its cap");

    // Reopened on another free install, the edit and every track are there.
    let (_c2, mut again) = app_on(tiers::free("konami-downgrade-again"));
    again.open_project(&doc);
    assert_eq!(again.track_ids(), ids, "the edited save lost or moved a track");
    assert!(
        again.arrangement().clips.iter().any(|c| c.lane == 149),
        "the edit on the 150th track did not survive its save"
    );
    let _ = std::fs::remove_dir_all(&root);
    for tag in [TAG, "konami-downgrade-free", "konami-downgrade-again"] {
        let _ = std::fs::remove_dir_all(tiers::dir(tag));
    }
}

/// **Undo and redo cross the cap** (design 11.2 item 10, 5.6): a free
/// install holding a cheater's forty tracks deletes to thirty-one and adds
/// the one it may, then Ctrl+Z walks it all back to forty, above the cap,
/// with every track on its own row and audible, and Ctrl+Shift+Z walks
/// forward to thirty-two again. Undo and redo apply moments; neither ever
/// asks the gate.
#[test]
fn undo_and_redo_cross_the_free_cap_through_the_key_feed() {
    const TAG: &str = "konami-undo";
    let root = scratch("undo");
    let (_c, mut cheat) = cheaters_desk(&root, TAG, 40);
    assert!(cheat.save_project_first(&root.join("Forty.tan")), "the cheater's save did not land");
    let doc = cheat.project_doc().expect("the saved project").to_path_buf();
    let ids = cheat.track_ids();
    drop(cheat);

    let (ctx, mut app) = app_on(tiers::free("konami-undo-free"));
    app.open_project(&doc);
    assert_eq!(app.counted_tracks(), 40, "fixture: the free install did not open forty tracks");
    for _ in 0..9 {
        let last = app.desk_extent() - 1;
        app.apply_dialog_action(DialogAction::TrackRemove(last));
    }
    app.add_track();
    assert_eq!(app.counted_tracks(), 32, "fixture: the desk is not at the cap");
    let at_cap: Vec<TrackId> = app.track_ids();

    // Back across the cap: the add, then the nine removals, one chord each.
    let mut counts = Vec::new();
    for _ in 0..10 {
        chord_z(&ctx, &mut app, false);
        counts.push(app.counted_tracks());
    }
    assert_eq!(counts, vec![31, 32, 33, 34, 35, 36, 37, 38, 39, 40], "each undo is not one gesture");
    assert_eq!(app.track_ids(), ids, "the undo did not bring every track back on its own row");
    let (rows, master) = heard(&app);
    assert!(rows.iter().all(|h| h.is_some_and(|p| p > 0.0)), "a track brought back above the cap is silent: {rows:?}");
    assert!(master > 0.0);
    assert_eq!(app.may_add_tracks(1), Err(TrackRefusal::FreeCap { have: 40, cap: 32 }));

    // And forward again.
    let mut counts = Vec::new();
    for _ in 0..10 {
        chord_z(&ctx, &mut app, true);
        counts.push(app.counted_tracks());
    }
    assert_eq!(counts, vec![39, 38, 37, 36, 35, 34, 33, 32, 31, 32], "each redo is not one gesture");
    assert_eq!(app.track_ids(), at_cap, "the redo did not land the desk where the gestures left it");
    let _ = std::fs::remove_dir_all(&root);
    for tag in [TAG, "konami-undo-free"] {
        let _ = std::fs::remove_dir_all(tiers::dir(tag));
    }
}

/// The bay a track's Trim stands in: its own rack's first.
fn trim_bay(id: TrackId) -> SeatBay {
    SeatBay::new(Node::Track(id), Rack::Chain(0), 0)
}

/// **A Trim in each of `ids`' first bays, and a noise clip on each**, through
/// the app's own roads (a clip dropped on a track that exists is an edit; a
/// plugin picked into a bay is `set_plugin`).
fn dress_tracks(app: &mut TangentApp, wav: &Path, ids: &[TrackId]) {
    for &id in ids {
        let row = app.row_of(id).expect("the track's row");
        app.import_audio_at(wav, row, 0.0).unwrap_or_else(|e| panic!("the clip on row {row}: {e}"));
        app.set_plugin(trim_bay(id), Some(crate::builtin_fx::TRIM_PATH));
    }
}

/// **Furnish a live engine as the host does each frame**: the bridge's
/// shape, a Trim at its defaults seated in each of `ids`' first bays (the
/// host's reconcile), the clips at their lanes and the curves at theirs
/// (`push_automation`).
fn furnish_live(app: &TangentApp, e: &mut Engine, p: &mut Bridge, ids: &[TrackId]) {
    for _ in 0..4 {
        p.sync(app, e);
    }
    let spec = crate::builtin_fx::spec_for(crate::builtin_fx::TRIM_PATH).expect("the trim");
    let state = crate::builtin_fx::state_text(spec, &spec.defaults(), &[]).into_bytes();
    for &id in ids {
        let (rack, bay) = p.engine_bay(trim_bay(id)).expect("the track has a lane");
        if e.insert_at(rack, bay).is_none() {
            e.load_insert_at(rack, bay, Some(Path::new(crate::builtin_fx::TRIM_PATH)), Some(&state))
                .expect("the trim loads")
                .expect("and seats");
        }
    }
    let doc = app.clip_doc().expect("a project");
    let (placed, midi, unread) = crate::desktop::export_clip_placement(app.arrangement(), &app.frozen_lanes(), RATE, |rel| {
        tangent_ui::arrangement::Arrangement::resolve(&doc, rel)
    });
    assert!(unread.is_empty(), "the live engine could not read {unread:?}");
    e.set_clips_slotted(p.slot_clips(placed));
    e.set_midi_slotted(p.slot_midi(midi));
    e.set_automation_slotted(p.slot_automation(&app.engine_automation()));
}

/// **The loudest each of `slots` makes once the trims and faders have
/// slewed**: a tenth of a second rendered from zero and its meters cleared,
/// then a sixth more read.
fn settled_peaks(e: &mut Engine, slots: &[Slot]) -> Vec<f32> {
    e.set_transport(1, 0);
    e.set_rolling(true);
    let mut out = vec![0.0f32; 1024 * 2];
    let mut run = |e: &mut Engine, blocks: usize| {
        for _ in 0..blocks {
            out.fill(0.0);
            e.render_offline(&mut out);
        }
    };
    run(e, (RATE as usize / 10).div_ceil(1024));
    for s in slots {
        let _ = e.lane_reading(*s);
    }
    run(e, (RATE as usize / 6).div_ceil(1024));
    slots
        .iter()
        .map(|s| {
            let ([a, b], _) = e.lane_reading(*s);
            a.max(b)
        })
        .collect()
}

/// **What the export hears of `ids`**: an engine furnished by `full_build`
/// with the live engine's knobs (the export's own road, which reads the
/// Trims off the live racks rather than the shelf), each track read at the
/// slot that build gave it. Answers the slots too.
fn offline_peaks(app: &TangentApp, live: (&Engine, &Bridge), ids: &[TrackId]) -> (Vec<Slot>, Vec<f32>) {
    let spec = tangent_ui::ports::ProjectExportSpec {
        rate: RATE,
        ..Default::default()
    };
    let mut ctx = Furnish {
        live: Some(live),
        ..Default::default()
    };
    let Built { engine: mut e, bridge } =
        full_build_with(app, RATE, Timebase::new(), BuildFor::Export(spec), &mut ctx).expect("an offline engine");
    assert!(ctx.refused.is_empty(), "the export refused {:?}", ctx.refused);
    let slots: Vec<Slot> = ids.iter().map(|id| bridge.lane_of(*id).expect("the export gave the track a lane").slot).collect();
    let peaks = settled_peaks(&mut e, &slots);
    (slots, peaks)
}

/// **The saved document with these curves drawn, opened again**: per
/// `(track, control, value)` a curve held at `value` from the top of the
/// song, written where a save writes curves (the document's `automation`,
/// by row), since this crate has no gesture that draws one. The tracks keep
/// their ids across the open, so a live bridge keeps their lanes.
///
/// **Two points, never one**: a lone point is its control's base, and an
/// open sets the control to it (`reconcile_auto_bases`), so a fader curve
/// of one point would arrive as the fader itself and be heard on a slot no
/// curve plays on.
fn reopen_with_curves(app: &mut TangentApp, curves: &[(TrackId, AutoParam, f64)]) {
    let doc = app.project_doc().expect("the saved project").to_path_buf();
    let text = std::fs::read_to_string(&doc).expect("the document");
    let mut json: serde_json::Value = serde_json::from_str(&text).expect("the document parses");
    let list: Vec<serde_json::Value> = curves
        .iter()
        .map(|(id, param, v)| {
            serde_json::json!({
                "lane": app.row_of(*id).expect("the track's row"),
                "param": param.key(),
                "points": [
                    { "at": 0.0, "v": v, "shape": Shape::Hold.key() },
                    { "at": 30.0, "v": v, "shape": Shape::Hold.key() },
                ],
            })
        })
        .collect();
    json.as_object_mut().expect("a document").insert("automation".to_owned(), serde_json::Value::Array(list));
    std::fs::write(&doc, serde_json::to_string_pretty(&json).expect("json")).expect("the document written");
    app.open_project(&doc);
    assert!(app.take_desk_reload(), "the reopen applied no desk");
    let ids: Vec<TrackId> = curves.iter().map(|(id, _, _)| *id).collect();
    assert!(ids.iter().all(|id| app.row_of(*id).is_some()), "a track did not come back from the reopen");
    assert_eq!(app.engine_automation().curves().len(), curves.len(), "the reopened song lost a curve");
}

/// **Each of `ids` follows a fader curve and a curve on its Trim's level,
/// live and in the export**: against the same song with no curve, held at
/// a twentieth of the fader's travel and at the Trim's floor, each track's
/// lane makes less than half of what it made, on the live engine and on the
/// export's. `what` names the desk for the failure.
fn curves_are_heard(what: &str, app: &mut TangentApp, e: &mut Engine, p: &mut Bridge, ids: &[TrackId]) {
    let trim_id = crate::builtin_fx::spec_for(crate::builtin_fx::TRIM_PATH).expect("the trim").params[0].id;
    let faders = |app: &TangentApp| -> Vec<f32> { ids.iter().map(|id| app.channel_gain(app.row_of(*id).expect("its row"))).collect() };
    let resting = faders(app);
    furnish_live(app, e, p, ids);
    let live_slots: Vec<Slot> = ids.iter().map(|id| p.lane_of(*id).expect("a live lane").slot).collect();
    let plain_live = settled_peaks(e, &live_slots);
    let (_, plain_offline) = offline_peaks(app, (&*e, &*p), ids);
    for (i, id) in ids.iter().enumerate() {
        assert!(plain_live[i] > 0.0 && plain_offline[i] > 0.0, "{what}: track {id:?} is silent with no curve");
    }
    for (control, param, value) in [
        ("a fader curve", AutoParam::Gain, 0.05),
        ("a curve on its Trim", AutoParam::Plugin { bay: 0, id: trim_id }, 0.0),
    ] {
        let curves: Vec<(TrackId, AutoParam, f64)> = ids.iter().map(|id| (*id, param, value)).collect();
        reopen_with_curves(app, &curves);
        // What moves the lane is the curve, never a fader the open moved.
        assert_eq!(faders(app), resting, "{what}: the reopen moved a fader");
        furnish_live(app, e, p, ids);
        let slots: Vec<Slot> = ids.iter().map(|id| p.lane_of(*id).expect("a live lane").slot).collect();
        assert_eq!(slots, live_slots, "{what}: the reopen moved a live lane");
        let live = settled_peaks(e, &slots);
        let (offline_slots, offline) = offline_peaks(app, (&*e, &*p), ids);
        for (i, id) in ids.iter().enumerate() {
            assert!(
                live[i] < plain_live[i] * 0.5,
                "{what}: {control} on track {id:?} (live slot {:?}) did not move it live: {} against {}",
                slots[i],
                live[i],
                plain_live[i]
            );
            assert!(
                offline[i] < plain_offline[i] * 0.5,
                "{what}: {control} on track {id:?} (export slot {:?}) did not move it in the export: {} against {}",
                offline_slots[i],
                offline[i],
                plain_offline[i]
            );
        }
    }
}

/// **A track's curves play wherever its lane sits, live and in the export**
/// (the W6 fixer's blocker). The engine read fader, pan, send and mute
/// curves for the legacy thirty-two slots only, and dropped a plugin curve
/// past them, while this lane's bridge seats lanes at any slot; a fresh
/// install's first Ctrl/Cmd+T took slot 32 live (every hidden row of the
/// shipped desk holds a lane, and the one it reused was still on its way
/// home), where the export, built fresh, gives it its row's. So:
///
/// - on a free install's stock desk, the first Ctrl/Cmd+T waits for the lane
///   of the hidden row it reuses and takes that slot, the one the export
///   gives it; a track Insert Track puts between two others lands past the
///   legacy slots live (the hidden rows below hold theirs) and on its row's
///   in the export; and each follows a fader curve and a Trim curve in both;
/// - a cheater's fortieth track, on slot 39 live and offline, does the same.
#[test]
fn a_tracks_curves_play_live_and_in_the_export_wherever_its_lane_sits() {
    const TAG: &str = "konami-curves";
    let root = scratch("curves");
    let wav = root.join("noise.wav");
    noise_wav(&wav, 0xc0de);

    // A free install on the stock desk of hidden rows, its live engine
    // seated as at launch: every hidden row's lane at its own slot.
    let (_ctx, mut app) = app_on(tiers::free(TAG));
    app.bootstrap_project(&root.join("Projects"));
    let mut e = Engine::offline(RATE, Timebase::new()).expect("a live engine");
    let mut p = Bridge::new();
    p.sync(&app, &mut e);
    app.add_track();
    let first = app.selected_track().expect("Add selects its track");
    let first_row = app.row_of(first).expect("its row");
    p.sync(&app, &mut e);
    assert_eq!(p.lane_of(first), None, "the new track took a slot while the hidden row's lane was still leaving");
    for _ in 0..3 {
        p.sync(&app, &mut e);
    }
    let lane = p.lane_of(first).expect("the new track never got a lane");
    assert_eq!(lane.slot, Slot(first_row as u32), "the first Ctrl/Cmd+T is not on its row's slot, the export's");
    // A second under it, its lane seated, so Insert Track below the first
    // lands between two tracks.
    app.add_track();
    let second = app.selected_track().expect("Add selects its track");
    for _ in 0..4 {
        p.sync(&app, &mut e);
    }
    assert!(
        p.lane_of(second).is_some_and(|l| (l.slot.0 as usize) < crate::instrument::LEGACY_LANES),
        "the second Ctrl/Cmd+T did not take a legacy slot"
    );
    assert!(app.insert_track_below(app.row_of(first).expect("its row")), "Insert Track was refused");
    let inserted = app.selected_track().expect("Insert selects its track");
    dress_tracks(&mut app, &wav, &[first, inserted]);
    assert!(app.save_project_first(&root.join("Stock.tan")), "the save did not land");
    furnish_live(&app, &mut e, &mut p, &[first, inserted]);
    assert!(
        p.lane_of(inserted).expect("a lane").slot.0 as usize >= crate::instrument::LEGACY_LANES,
        "fixture: the inserted track did not land past the legacy slots live"
    );
    let rows: Vec<Slot> = [first, inserted].iter().map(|id| Slot(app.row_of(*id).expect("its row") as u32)).collect();
    assert_eq!(offline_peaks(&app, (&e, &p), &[first, inserted]).0, rows, "fixture: the export did not seat each on its row's slot");
    curves_are_heard("the stock desk", &mut app, &mut e, &mut p, &[first, inserted]);

    // A cheater's forty-one tracks, the fortieth on slot 39.
    let cheat_root = scratch("curves-cheat");
    let (_c, mut cheat) = cheaters_desk(&cheat_root, "konami-curves-cheat", 41);
    let fortieth = cheat.track_ids()[39];
    cheat.set_plugin(trim_bay(fortieth), Some(crate::builtin_fx::TRIM_PATH));
    assert!(cheat.save_project_first(&cheat_root.join("Wide.tan")), "the cheater's save did not land");
    let mut e = Engine::offline(RATE, Timebase::new()).expect("a live engine");
    let mut p = Bridge::new();
    furnish_live(&cheat, &mut e, &mut p, &[fortieth]);
    assert_eq!(p.lane_of(fortieth).expect("a lane").slot, Slot(39), "fixture: the fortieth track is not on slot 39");
    curves_are_heard("the cheater's desk", &mut cheat, &mut e, &mut p, &[fortieth]);

    for dir in [&root, &cheat_root] {
        let _ = std::fs::remove_dir_all(dir);
    }
    for tag in [TAG, "konami-curves-cheat"] {
        let _ = std::fs::remove_dir_all(tiers::dir(tag));
    }
}

/// **The engine and the shelf are tier-blind** (design 11.2 item 11, 5.6):
/// a desk past the cap plays, exports, freezes and keeps its plugin states
/// whole on every tier because nothing that builds, drives or furnishes the
/// engine, or files a state, asks about the licence. Production code only:
/// each file's tests build apps on every tier, by name. The persistence and
/// drawing half is `tangent-ui`'s `width_tests`.
#[test]
fn the_engine_and_the_shelf_never_read_the_licence() {
    let files: [(&str, &str); 7] = [
        ("instrument.rs", include_str!("instrument.rs")),
        ("engine_bridge.rs", include_str!("engine_bridge.rs")),
        ("desk_plan.rs", include_str!("desk_plan.rs")),
        ("lane_bank.rs", include_str!("lane_bank.rs")),
        ("laneset.rs", include_str!("laneset.rs")),
        ("rtwork.rs", include_str!("rtwork.rs")),
        ("shelf.rs", include_str!("shelf.rs")),
    ];
    let mut bad = Vec::new();
    for (name, src) in files {
        // Everything from the first test module down is the file's tests.
        let code = src.find("\n#[cfg(test)]\nmod tests").map_or(src, |at| &src[..at]);
        for (n, line) in code.lines().enumerate() {
            let line = line.split("//").next().unwrap_or("");
            for word in line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                let lower = word.to_ascii_lowercase();
                let licence = lower.contains("license")
                    || lower.contains("licence")
                    || ["is_supporter", "track_cap", "cheated", "licensed", "FREE_TRACKS", "may_add_tracks", "counted_tracks"]
                        .contains(&word);
                if licence {
                    bad.push(format!("{name}:{} reads `{word}`", n + 1));
                }
            }
        }
    }
    assert!(bad.is_empty(), "the engine or the shelf reads the licence:\n  {}", bad.join("\n  "));
}
