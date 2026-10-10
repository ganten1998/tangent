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
//! moves; that a send is heard, through its bus and nowhere else; that a
//! free desk's hundred buses and thousand sends reach one plan; and that a
//! route loop a legacy file saved plays exactly as the legacy shim played
//! it, which is the road the render goldens pin.
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
    let (_ctx, mut app) = app_on(desk(3), tiers::free("buses-plan"));
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
