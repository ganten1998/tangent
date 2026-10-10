//! **Buses, returns and sends, heard** (design section 9, W7-buses-ui):
//! the claims about the desk's new strips that only mean something in the
//! engine, made through the [`Bridge`] every live engine and every export
//! is furnished by, never through a model of it.
//!
//! The gestures themselves (a return made and sent to, a level moved, a
//! send taken away, each one moment; what a bus refuses; the free tier's
//! hundred buses; loops greyed in the pickers) are `tangent-ui`'s
//! `bus_tests`. What this file adds is that the plan the engine plays
//! carries a bus as a bus and every send to one, in an order no reorder
//! moves; that a send's level moves as a number in that plan, never as a
//! plan of its own; that a send is heard, through its bus and nowhere else;
//! that a send's curve plays in an export, on its own send and never the
//! cue beside it; that a free desk's hundred buses and thousand sends reach
//! one plan; that the app and the bridge read one rule for what a send to a
//! bus is; and that a route loop a legacy file saved plays exactly as the
//! legacy shim played it, which is the road the render goldens pin.
//!
//! **Nothing here touches the real home**: the licence stores are temp
//! directories of their own (`composite::tiers`), every project is under
//! the temp dir, and the apps run with the global settings file off.
//!
//! [`Bridge`]: crate::engine_bridge::Bridge

use std::path::{Path, PathBuf};

use tangent_core::license::LicenseStore;
use tangent_record::audio::Timebase;
use tangent_ui::app::TangentApp;
use tangent_ui::desk_id::Node;
use tangent_ui::desk_model::{RowKind, TrackRow};
use tangent_ui::host::Caps;
use tangent_ui::recorder::{BusRole, StripRef};
use tangent_ui::routing::{LaneRoute, LaneRoutes, LaneTarget};
use tangent_ui::settings::Settings;

use crate::composite::tiers;
use crate::desk_plan::{InSend, LaneFlags, PlanInputs, SendFrom, SendTo, Slot};
use crate::engine_bridge::{full_build, BuildFor, Bridge, Built};
use crate::instrument::{Engine, Placed};

const RATE: u32 = 48_000;

/// The desktop's capabilities without the settings file, which in a binary
/// crate's test is the real one.
const QUIET: Caps = Caps {
    persist_global_settings: false,
    ..Caps::DESKTOP
};

/// **A desk of `n` audio tracks and nothing else**: no bay seated, no send,
/// nothing armed, so what a test hears is what it put there.
fn desk(n: usize) -> Settings {
    let mut s = Settings {
        show_welcome: false,
        ..Settings::default()
    };
    s.plugins.clear();
    s.routes = tangent_ui::routing::Routes::default();
    s.insert_keys = tangent_ui::routing::KeyEdges::default();
    let ids: Vec<_> = s.rows.iter().map(|r| r.id).collect();
    s.rows = ids
        .into_iter()
        .enumerate()
        .map(|(k, id)| {
            let mut r = TrackRow::vacant(id);
            if k < n {
                r.kind = Some(RowKind::Audio);
                r.added = true;
            }
            r
        })
        .collect();
    s.arm_unsaid = false;
    s
}

fn app_on(settings: Settings, store: LicenseStore) -> (egui::Context, TangentApp) {
    let ctx = egui::Context::default();
    let app = TangentApp::with_license(&ctx, settings, QUIET, store);
    (ctx, app)
}

/// A folder of this test's own under the temp dir, emptied first.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tangent-buses-{}-{:?}-{tag}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir
}

/// A second of seeded stereo noise as a float WAV.
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

/// The plan an app's desk is published as, through a fresh bridge.
fn plan_of(app: &TangentApp) -> (Bridge, PlanInputs) {
    let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
    let mut b = Bridge::new();
    b.sync(app, &mut e);
    let p = b.published_for_test().expect("a plan").clone();
    (b, p)
}

/// **What an export of the app makes**: the master over a quarter of a
/// second and each row's lane meter, through `full_build`.
fn heard(app: &TangentApp) -> (Vec<f32>, Vec<f32>) {
    let spec = tangent_ui::ports::ProjectExportSpec {
        rate: RATE,
        ..Default::default()
    };
    let Built { engine: mut e, bridge } =
        full_build(app, RATE, Timebase::new(), BuildFor::Export(spec)).expect("an offline engine");
    e.set_transport(1, 0);
    e.set_rolling(true);
    let mut master = Vec::new();
    let mut out = vec![0.0f32; 1024 * 2];
    for _ in 0..(RATE as usize / 4).div_ceil(1024) {
        out.fill(0.0);
        e.render_offline(&mut out);
        master.extend_from_slice(&out);
    }
    let lanes = (0..app.desk_extent())
        .map(|row| {
            bridge.lane(row).map_or(0.0, |l| {
                let ([a, b], _) = e.lane_reading(l.slot);
                a.max(b)
            })
        })
        .collect();
    (master, lanes)
}

/// **A bus row is a bus lane, and every send to a bus reaches the plan**:
/// flagged BUS (the one lane a send may land on, solo-safe, its rack run
/// once over what arrives), each send at its own level and side of the
/// fader, in the order of the senders' identities so no reorder moves one.
/// The knob's own send into the effects return is not a send here (the
/// engine plays a strip's knob for a strip with no Fx entry). A level moved
/// is carried to the engine, and a bus made a track lets go of its flag and
/// of the sends into it.
#[test]
fn a_bus_row_is_a_bus_lane_and_its_sends_reach_the_plan() {
    // A desk read with a send aimed at a TRACK (a hand-edited file can say
    // one; the reader keeps it, and no gesture makes one): only a bus takes
    // a send, so the plan carries none for it.
    let mut s = desk(3);
    let stray = Node::Track(s.rows[2].id);
    s.rows[1].sends.push(tangent_ui::desk_model::Send { to: stray, level: 1.0, pre: false });
    let (_ctx, mut app) = app_on(s, tiers::free("buses-plan"));
    let bus = app.new_bus(BusRole::Return).expect("a return");
    let to = app.track_at(bus).map(Node::Track).expect("the return's track");
    let first = app.track_at(0).expect("row 0's track");
    // Row 2 sends first, before its fader; row 0 after it.
    assert!(app.add_send(StripRef::Row(2), bus));
    assert!(app.set_send_pre(StripRef::Row(2), to, false));
    app.set_send_level(StripRef::Row(2), to, true, 0.25, true);
    assert!(app.add_send(StripRef::Row(0), bus));
    app.set_send_level(StripRef::Row(0), to, false, 0.75, true);
    // One engine and one bridge across every edit, as the live one is.
    let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
    let mut b = Bridge::new();
    b.sync(&app, &mut e);
    let p = b.published_for_test().expect("a plan").clone();
    let lane_of = |b: &Bridge, row: usize| b.lane(row).expect("a lane").slot;
    let (s0, s2, sb) = (lane_of(&b, 0), lane_of(&b, 2), lane_of(&b, bus));
    for l in &p.lanes {
        assert_eq!(l.flags.contains(LaneFlags::BUS), l.slot == sb, "slot {:?} flagged wrong", l.slot);
    }
    let want = vec![
        InSend { from: SendFrom::Lane(s0), to: SendTo::Lane(sb), level: 0.75, pre: false },
        InSend { from: SendFrom::Lane(s2), to: SendTo::Lane(sb), level: 0.25, pre: true },
    ];
    assert_eq!(p.sends, want, "the plan's sends are not the desk's, by sender");
    // Row 2 dragged to the top: the same lanes, the same sends, in the same
    // order (by identity, not by row).
    app.move_channel(2, 0);
    b.sync(&app, &mut e);
    assert_eq!(b.lane(0).map(|l| l.slot), Some(s2), "fixture: the drag");
    let p2 = b.published_for_test().expect("a plan").clone();
    assert_eq!(p2.sends, want, "a reorder moved a send in the plan");
    // A level moved reaches the engine's plan.
    let row0 = app.row_of(first).expect("row 0's track");
    app.set_send_level(StripRef::Row(row0), to, false, 0.5, true);
    b.sync(&app, &mut e);
    let p3 = b.published_for_test().expect("a plan").clone();
    assert_eq!(p3.sends[0].level, 0.5, "a moved level did not reach the plan");
    // The return made a track: no BUS flag, no sends into it.
    let at = app.row_of(to.track().expect("a track")).expect("on the desk");
    app.set_channel_kind(at, Some(tangent_ui::recorder::ChannelKind::Audio));
    b.sync(&app, &mut e);
    let p4 = b.published_for_test().expect("a plan").clone();
    assert!(p4.lanes.iter().all(|l| !l.flags.contains(LaneFlags::BUS)), "a track kept the BUS flag");
    assert!(p4.sends.is_empty(), "a send into a track reached the plan");
}

/// **A send's level moved is carried into the plan that plays, and
/// publishes no plan** (design 7.6: what only scales stays a number). A
/// drag writes the level on every pointer move, and the bridge used to read
/// each move as a new shape: two `build_plan`s, an op on the ring and one
/// more shadow, every frame of the drag. Now each level lands in its own
/// cell of the plan that plays, found by the send's two ends, and the count
/// of plans stands still. A send moved to the other side of its fader does
/// decide where it is gathered, and publishes. And a strip sending to one
/// bus from both sides of its fader (a file can say so; no gesture makes
/// one), whose two sends the engine cannot tell apart by their ends,
/// publishes a plan rather than write one level into both.
#[test]
fn a_moved_send_level_is_carried_into_the_playing_plan_and_publishes_none() {
    use std::sync::atomic::Ordering::Relaxed;
    use tangent_ui::desk_model::Send;
    // The level row `from`'s send to the bus on row `bus`, on `pre`'s side
    // of its fader, holds in the plan that plays: its cell found by the
    // send's ends in a plan built from the inputs the bridge published,
    // which numbers its cells as the engine's own does.
    let cell = |b: &Bridge, e: &mut Engine, from: usize, bus: usize, pre: bool| -> f32 {
        e.settle_offline();
        let lane = |row: usize| b.lane(row).expect("a lane").slot;
        let shape = crate::desk_plan::build_plan(b.published_for_test().expect("a plan"), None);
        let ends = crate::desk_plan::send_key(SendFrom::Lane(lane(from)), SendTo::Lane(lane(bus)));
        let i = shape
            .sends
            .find(ends)
            .find(|&i| shape.sends.sends[i as usize].pre == pre)
            .expect("the send is in the plan");
        let gains = e.plan_gains().expect("a plan plays");
        f32::from_bits(gains[shape.sends.sends[i as usize].gain as usize].load(Relaxed))
    };
    // Synced until the desk has settled (a reused row's old lane goes
    // home), then the count of plans an unchanged frame must leave alone.
    let settle = |app: &TangentApp, b: &mut Bridge, e: &mut Engine| {
        for _ in 0..4 {
            b.sync(app, e);
        }
        let plans = b.stats().plans;
        b.sync(app, e);
        assert_eq!(b.stats().plans, plans, "fixture: an unchanged frame published a plan");
        plans
    };

    let (_ctx, mut app) = app_on(desk(3), tiers::free("buses-carry"));
    let bus = app.new_bus(BusRole::Return).expect("a return");
    let to = app.track_at(bus).map(Node::Track).expect("the return's track");
    assert!(app.add_send(StripRef::Row(0), bus));
    assert!(app.add_send(StripRef::Row(1), bus));
    let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
    let mut b = Bridge::new();
    let plans = settle(&app, &mut b, &mut e);
    // A drag on row 0's send: thirty pointer moves, the last one released.
    for k in 0..30 {
        let v = 1.0 - f64::from(k) / 40.0;
        app.set_send_level(StripRef::Row(0), to, false, v, k == 29);
        b.sync(&app, &mut e);
        assert_eq!(cell(&b, &mut e, 0, bus, false), v as f32, "move {k} is not in the plan that plays");
    }
    assert_eq!(b.stats().plans, plans, "a send's level rebuilt the plan");
    assert_eq!(cell(&b, &mut e, 1, bus, false), 1.0, "the drag moved another strip's send");
    // The side of the fader decides where a send is gathered.
    assert!(app.set_send_pre(StripRef::Row(1), to, false));
    b.sync(&app, &mut e);
    assert_eq!(b.stats().plans, plans + 1, "a send moved ahead of its fader is not one plan");
    assert_eq!(cell(&b, &mut e, 1, bus, true), 1.0, "the send lost its level moving ahead of its fader");

    // Twins: row 0 sends to the return on row 2 from both sides of its fader.
    let mut s = desk(2);
    s.rows[2].kind = Some(RowKind::Bus);
    s.rows[2].name = "Return 1".to_owned();
    let ret = Node::Track(s.rows[2].id);
    s.rows[0].sends.push(Send { to: ret, level: 0.5, pre: false });
    s.rows[0].sends.push(Send { to: ret, level: 0.25, pre: true });
    let (_ctx, mut app) = app_on(s, tiers::free("buses-carry-twins"));
    let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
    let mut b = Bridge::new();
    let plans = settle(&app, &mut b, &mut e);
    assert_eq!((cell(&b, &mut e, 0, 2, false), cell(&b, &mut e, 0, 2, true)), (0.5, 0.25), "fixture: the twins' levels");
    app.set_send_level(StripRef::Row(0), ret, false, 0.75, true);
    b.sync(&app, &mut e);
    assert_eq!(b.stats().plans, plans + 1, "a twin's level was carried by the ends it shares");
    assert_eq!(cell(&b, &mut e, 0, 2, false), 0.75, "the moved twin's level is not in the plan that plays");
    assert_eq!(cell(&b, &mut e, 0, 2, true), 0.25, "the other twin took its level");
}

/// **A send is heard, through its bus and nowhere else**: with the send at
/// unity the bus's lane meters and the master changes; with the send at
/// nothing the bus is silent and the master is the master with no send at
/// all, bit for bit (a send is a copy: the track's own way out is
/// untouched), because a bus with nothing sent to it adds nothing.
#[test]
fn a_send_is_heard_through_its_bus_and_nowhere_else() {
    let root = scratch("heard");
    let wav = root.join("noise.wav");
    noise_wav(&wav, 0x0b05);
    let render = |level: Option<f64>, tag: &str| {
        let (_ctx, mut app) = app_on(desk(0), tiers::free(tag));
        app.bootstrap_project(&root.join(tag));
        app.import_audio_at(&wav, 0, 0.0).expect("a track with a sound");
        let bus = app.new_bus(BusRole::Return).expect("a return");
        if let Some(level) = level {
            assert!(app.add_send(StripRef::Row(0), bus));
            let to = app.track_at(bus).map(Node::Track).expect("the return's track");
            app.set_send_level(StripRef::Row(0), to, false, level, true);
        }
        let (master, lanes) = heard(&app);
        (master, lanes[bus], lanes[0])
    };
    let (none, bus_none, track_none) = render(None, "buses-heard-none");
    let (sent, bus_sent, track_sent) = render(Some(1.0), "buses-heard-sent");
    let (zero, bus_zero, _) = render(Some(0.0), "buses-heard-zero");
    assert!(track_none > 0.0 && none.iter().any(|s| s.abs() > 0.0), "fixture: the track is silent");
    assert_eq!(bus_none, 0.0, "a bus nobody sends to metered");
    assert!(bus_sent > 0.0, "the bus did not hear the send");
    assert_eq!(track_sent, track_none, "a send changed its own strip");
    let energy = |b: &[f32]| b.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>();
    assert!(energy(&sent) > energy(&none) * 1.5, "the send did not reach the master through its bus");
    assert_eq!(bus_zero, 0.0, "a send at nothing reached the bus");
    assert!(zero.iter().zip(&none).all(|(a, b)| a.to_bits() == b.to_bits()), "a send at nothing changed the mix");
    let _ = std::fs::remove_dir_all(&root);
}

/// **The saved song at `doc`, opened again with `curves` for its automation
/// and, given `sends`, those for the sends of track `from`**: what its
/// export makes, and what the lane of track `to` meters. Written where a
/// save writes them (the document and its format-2 desk) and opened as a
/// song is, so the curves and sends reach the export by every road a real
/// song's do.
fn reopen_heard(
    app: &mut TangentApp,
    doc: &Path,
    curves: serde_json::Value,
    sends: Option<(tangent_ui::desk_id::TrackId, serde_json::Value)>,
    to: tangent_ui::desk_id::TrackId,
) -> (Vec<f32>, f32) {
    let text = std::fs::read_to_string(doc).expect("the document");
    let mut json: serde_json::Value = serde_json::from_str(&text).expect("the document parses");
    json.as_object_mut().expect("a document").insert("automation".to_owned(), curves);
    if let Some((from, sends)) = sends {
        let rows = json["desk"]["rows"].as_array_mut().expect("the document carries a format-2 desk");
        let row = rows.iter_mut().find(|r| r["id"] == serde_json::json!(from.0)).expect("the sender's row");
        row["sends"] = sends;
    }
    std::fs::write(doc, serde_json::to_string_pretty(&json).expect("json")).expect("the document written");
    app.open_project(doc);
    let (master, lanes) = heard(app);
    (master, lanes[app.row_of(to).expect("the bus's row")])
}

/// A curve holding a track's send to `to` at `level` for the whole song.
fn held_send_curve(to: tangent_ui::desk_id::TrackId, level: f64) -> serde_json::Value {
    use tangent_ui::automation::{AutoParam, Shape};
    serde_json::json!([{
        "lane": 0,
        "param": AutoParam::SendTo { to }.key(),
        "points": [
            { "at": 0.0, "v": level, "shape": Shape::Hold.key() },
            { "at": 30.0, "v": level, "shape": Shape::Hold.key() },
        ],
    }])
}

/// **A curve on a track's send to a return plays in the export**
/// (`AutoParam::SendTo`, W7-join): the curve holds the send at nothing for
/// the whole song, written where a save writes curves and opened as a song
/// is, and the return hears nothing while the export is no longer the
/// curveless song's. While the engine played no send curve this was the
/// test of a switch that shut every door to one (`SEND_CURVES_PLAY`,
/// `the_send_curve_switch_says_what_an_export_plays`); the switch went with
/// the engine's half arriving, and this is its played branch.
#[test]
fn a_send_curve_plays_in_the_export() {
    use tangent_ui::automation::AutoParam;
    let root = scratch("send-curve");
    let wav = root.join("noise.wav");
    noise_wav(&wav, 0x5e4d);
    let (_ctx, mut app) = app_on(desk(0), tiers::free("buses-send-curve"));
    app.bootstrap_project(&root.join("Projects"));
    app.import_audio_at(&wav, 0, 0.0).expect("a track with a sound");
    let bus = app.new_bus(BusRole::Return).expect("a return");
    assert!(app.add_send(StripRef::Row(0), bus));
    let to = app.track_at(bus).expect("the return's track");
    assert!(app.save_project_first(&root.join("Send.tan")), "the save did not land");
    let doc = app.project_doc().expect("the saved project").to_path_buf();
    let (plain, return_plain) = reopen_heard(&mut app, &doc, serde_json::json!([]), None, to);
    let (curved, return_curved) = reopen_heard(&mut app, &doc, held_send_curve(to, 0.0), None, to);
    assert!(return_plain > 0.0, "fixture: the return hears nothing with no curve");
    assert!(
        app.automation().curves().iter().any(|c| c.param == AutoParam::SendTo { to }),
        "fixture: the song lost its send's curve"
    );
    assert_eq!(app.strip_send_to(StripRef::Row(0), Node::Track(to), false), Some(1.0), "fixture: the open moved the send");
    assert_eq!(return_curved, 0.0, "the return heard a send its curve holds at nothing");
    assert!(
        curved.iter().zip(&plain).any(|(a, b)| a.to_bits() != b.to_bits()),
        "the send's curve did not change the export"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// **A send's curve rides its own send and leaves the cue beside it**
/// (W7-join: the engine names a send by its two ends, and the app's curve
/// is the post-fader send's alone). A track a file has sending to one
/// return from both sides of its fader, the pre-fader cue at a half and the
/// post-fader send at full, and a curve holding the post-fader send at
/// nothing: the export is bit for bit the song with no curve and that send
/// at nothing, and the return still hears the cue. One engine curve named
/// by its ends alone drove both sends, and the cue fell silent with it.
#[test]
fn a_send_curve_rides_its_own_send_and_leaves_the_cue_beside_it() {
    let root = scratch("send-curve-twin");
    let wav = root.join("noise.wav");
    noise_wav(&wav, 0x7a1e);
    let (_ctx, mut app) = app_on(desk(0), tiers::free("buses-send-curve-twin"));
    app.bootstrap_project(&root.join("Projects"));
    app.import_audio_at(&wav, 0, 0.0).expect("a track with a sound");
    let bus = app.new_bus(BusRole::Return).expect("a return");
    assert!(app.add_send(StripRef::Row(0), bus));
    let (from, to) = (app.track_at(0).expect("the track"), app.track_at(bus).expect("the return's track"));
    assert!(app.save_project_first(&root.join("Twin.tan")), "the save did not land");
    let doc = app.project_doc().expect("the saved project").to_path_buf();
    let twins = |post: f64| {
        let bus = Node::Track(to).to_string();
        serde_json::json!([
            { "to": bus, "level": post, "pre": false },
            { "to": bus, "level": 0.5, "pre": true },
        ])
    };
    let (curved, return_curved) = reopen_heard(&mut app, &doc, held_send_curve(to, 0.0), Some((from, twins(1.0))), to);
    assert_eq!(
        (app.strip_send_to(StripRef::Row(0), Node::Track(to), false), app.strip_send_to(StripRef::Row(0), Node::Track(to), true)),
        (Some(1.0), Some(0.5)),
        "fixture: the file's twins did not open"
    );
    let (shut, return_shut) = reopen_heard(&mut app, &doc, serde_json::json!([]), Some((from, twins(0.0))), to);
    assert!(return_shut > 0.0, "fixture: the cue alone is silent");
    assert_eq!(return_curved.to_bits(), return_shut.to_bits(), "the return heard the curve take its cue as well as its send");
    let first = curved.iter().zip(&shut).position(|(a, b)| a.to_bits() != b.to_bits());
    assert_eq!(first, None, "the curved export is not the song with the post-fader send at nothing");
    let _ = std::fs::remove_dir_all(&root);
}

/// **A free desk at its thirty-two tracks, with a hundred buses and a
/// thousand sends, is one plan** (design 9: no limit on either tier): every
/// bus a BUS lane, every send in it, and no send dropped on its way.
#[test]
fn a_free_desks_hundred_buses_and_thousand_sends_are_one_plan() {
    let (_ctx, mut app) = app_on(desk(32), tiers::free("buses-thousand"));
    assert_eq!(app.counted_tracks(), 32, "fixture");
    let buses: Vec<usize> = (0..100)
        .map(|k| app.new_bus(if k % 2 == 0 { BusRole::Bus } else { BusRole::Return }).expect("a bus"))
        .collect();
    for t in 0..32 {
        for j in 0..31 {
            assert!(app.add_send(StripRef::Row(t), buses[(t + j) % 100]));
        }
    }
    for k in 0..8 {
        assert!(app.add_send(StripRef::Row(buses[k]), buses[k + 1]));
    }
    let (b, p) = plan_of(&app);
    let flagged: Vec<Slot> = p.lanes.iter().filter(|l| l.flags.contains(LaneFlags::BUS)).map(|l| l.slot).collect();
    let mut want: Vec<Slot> = buses.iter().map(|r| b.lane(*r).expect("a bus lane").slot).collect();
    want.sort_unstable();
    let mut got = flagged.clone();
    got.sort_unstable();
    assert_eq!(got, want, "not every bus is a bus lane");
    assert_eq!(p.sends.len(), 1000, "the plan dropped sends");
    assert!(p.sends.iter().all(|x| matches!(x.to, SendTo::Lane(s) if flagged.contains(&s))));
    // And the engine takes it: one block renders.
    let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
    let mut b = Bridge::new();
    b.sync(&app, &mut e);
    let mut out = vec![0.0f32; 512 * 2];
    e.render_offline(&mut out);
    assert!(out.iter().all(|s| s.is_finite()));
}

/// **A route loop a legacy file saved opens and plays exactly as the legacy
/// shim played it** (design 9: a legacy saved cycle opens and sounds the
/// same). The file's routing table is read by the legacy reader and bound
/// to the rows by identity, nothing is cut at load or on the way to the
/// engine, and the desk rendered through the bridge is the desk rendered
/// through `Engine::set_routes`, the road the render goldens' `route-cycle`
/// desk is built on, bit for bit.
#[test]
fn a_saved_route_loop_opens_and_plays_as_the_legacy_shim_played_it() {
    let lanes = LaneRoutes(vec![
        LaneRoute { from: 1, target: LaneTarget::Channel(2), gain: 0.6 },
        LaneRoute { from: 2, target: LaneTarget::Channel(1), gain: 0.4 },
        LaneRoute { from: 2, target: LaneTarget::Channel(3), gain: 0.5 },
    ]);
    let read = Settings::from_json(&serde_json::json!({ "routes": lanes.to_json() }).to_string());
    let mut s = desk(4);
    s.routes = read.routes;
    let (_ctx, app) = app_on(s, tiers::free("buses-saved-loop"));
    assert_eq!(app.routes().0.len(), 3, "the loop was cut at load");
    let placed: Vec<Placed> = [0usize, 1, 2]
        .iter()
        .map(|&lane| {
            let clip = crate::render_golden::noise(1.0, 0x9e37 + lane as u32 * 7919);
            let len = clip.frames() as u64;
            Placed { clip, strip: lane, start: 0, offset: 0, len, fade_in: 0, fade_out: 0, gain: 0.5 }
        })
        .collect();
    // Through the bridge, from the app's desk.
    let mut bridged = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
    bridged.set_master_gain(1.0);
    let mut b = Bridge::new();
    b.sync(&app, &mut bridged);
    let p = b.published_for_test().expect("a plan");
    assert_eq!(p.routes.len(), 3, "the bridge cut the loop");
    bridged.set_clips_slotted(b.slot_clips(placed.clone()));
    // Through the legacy shim, as the golden desks are built.
    let mut shim = crate::render_golden::offline();
    shim.set_clips(placed);
    shim.set_routes(&lanes);
    let render = |e: &mut Engine| {
        let mut all = Vec::new();
        let mut out = vec![0.0f32; 512 * 2];
        e.set_rolling(false);
        for _ in 0..8 {
            out.fill(0.0);
            e.render_offline(&mut out);
        }
        e.set_transport(1, 0);
        e.set_rolling(true);
        for _ in 0..48 {
            out.fill(0.0);
            e.render_offline(&mut out);
            all.extend_from_slice(&out);
        }
        all
    };
    let (a, z) = (render(&mut bridged), render(&mut shim));
    assert!(z.iter().any(|s| s.abs() > 0.0), "fixture: the shim's loop is silent");
    let first = a.iter().zip(&z).position(|(x, y)| x.to_bits() != y.to_bits());
    assert_eq!(first, None, "the opened loop does not play as the shim played it");
}
