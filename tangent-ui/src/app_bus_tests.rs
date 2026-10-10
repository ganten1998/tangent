//! **Buses, returns and sends, through the app's own roads** (design
//! section 9, W7-buses-ui's done-when): a return made and sent to by three
//! tracks, a level moved and a send taken away, each step one moment that
//! undoes and redoes; what a bus refuses (clips, inputs, an arm, a freeze);
//! a free desk at its thirty-two tracks making a hundred buses and a
//! thousand sends; the pickers refusing a new loop while a saved one opens
//! whole; every route, send and key end surviving a reorder, an insert and
//! a delete; and a kind change from a bus to a track asking the cap. A child
//! of `app`, in a file of its own as `width_tests` is, so it reads the app's
//! private roads the way its `tests` module does.
//!
//! **Nothing here reaches a real home.** Every app is built on a settings
//! value with `persist_global_settings` off and a per-test licence store
//! under the temp dir (`tiers`), and a test that needs a project makes it in
//! a scratch folder of its own.

use super::tests::project_scratch;
use super::*;
use crate::desk_id::{Node, TrackId};
use crate::desk_model::{RowKind, TrackRow};
use crate::dialogs::RoutingPage;
use crate::mixer_panel::Hit;
use crate::recorder::{BusRole, ChannelKind, Column, StripRef};
use crate::routing::LaneTarget;
use tangent_core::license::LicenseStore;

/// The desktop's capabilities without the settings file.
const QUIET: Caps = Caps {
    persist_global_settings: false,
    ..Caps::DESKTOP
};

/// **A desk of `n` MIDI tracks**, ids from 500 so no track's id is the row
/// it stands on, named `T0` and on.
fn desk(n: usize) -> Settings {
    let mut s = Settings {
        show_welcome: false,
        ..Settings::default()
    };
    s.plugins.clear();
    s.routes = crate::routing::Routes::default();
    s.insert_keys = crate::routing::KeyEdges::default();
    s.rows = (0..n)
        .map(|k| {
            let mut r = TrackRow::vacant(TrackId(500 + k as u32));
            r.kind = Some(RowKind::Midi);
            r.added = true;
            r.name = format!("T{k}");
            r
        })
        .collect();
    s.next_track_id = 500 + n as u64;
    s.arm_unsaid = false;
    s
}

/// An app on `settings`, its history starting there.
fn app_on(settings: Settings, store: LicenseStore) -> (egui::Context, TangentApp) {
    let ctx = egui::Context::default();
    let mut app = TangentApp::with_license(&ctx, settings, QUIET, store);
    app.undo_reset();
    (ctx, app)
}

/// **What the workflow compares**: every row by its id (its kind and its
/// name), and every send on the desk as (sender, bus, level bits, pre).
type Picture = (Vec<(TrackId, Option<RowKind>, String)>, Vec<(TrackId, TrackId, u64, bool)>);

fn picture(app: &TangentApp) -> Picture {
    let rows = app.settings.rows.iter().map(|r| (r.id, r.kind, r.name.clone())).collect();
    let mut sends = Vec::new();
    for r in &app.settings.rows {
        for x in &r.sends {
            if let Some(to) = x.to.track() {
                sends.push((r.id, to, x.level.to_bits(), x.pre));
            }
        }
    }
    (rows, sends)
}

/// Every send on the desk to a bus, counted.
fn bus_sends(app: &TangentApp) -> usize {
    app.settings.rows.iter().map(|r| r.sends.iter().filter(|x| x.to.track().is_some()).count()).sum()
}

/// Whether the app said something since `n` notes stood.
fn said_since(app: &TangentApp, n: usize) -> bool {
    app.said().notes().len() > n
}

/// **The workflow, one moment a step, every step back and forth**: a new
/// return, three tracks sent to it, a level moved (thirty moves of a drag
/// and its release: one moment), a send taken away. Undo walks back
/// through every picture in turn and redo walks forward through them all.
#[test]
fn a_return_takes_three_sends_and_every_step_undoes_and_redoes() {
    let (_ctx, mut app) = app_on(desk(3), tiers::free("bus-workflow"));
    let mut pictures = vec![picture(&app)];
    let step = |app: &mut TangentApp, what: &str, pictures: &mut Vec<Picture>| {
        assert_eq!(app.hist_at, pictures.len(), "{what} was not exactly one moment");
        pictures.push(picture(app));
    };
    let ret = app.new_bus(BusRole::Return).expect("a return");
    assert!(app.is_bus(ret), "the return is no bus");
    assert_eq!(app.settings.rows[ret].name, "Return 1");
    assert_eq!(app.counted_tracks(), 3, "a return was counted as a track");
    assert_eq!(app.selected_row(), Some(ret), "the new return is not selected");
    step(&mut app, "the new return", &mut pictures);
    let bus = app.row_node(ret);
    for k in 0..3 {
        assert!(app.add_send(StripRef::Row(k), ret), "track {k} could not send to the return");
        assert_eq!(app.strip_send_to(StripRef::Row(k), bus, false), Some(1.0), "a send arrived silent");
        step(&mut app, "a send", &mut pictures);
    }
    // A level, as the routing window moves it: thirty moves of the hand,
    // then the release.
    let before = app.hist_at;
    for i in 0..30 {
        app.apply_dialog_action(DialogAction::SetSendLevel {
            from: 1,
            to: ret,
            pre: false,
            level: 1.0 - i as f32 / 60.0,
            done: false,
        });
    }
    assert_eq!(app.hist_at, before, "a drag's moves were moments of their own");
    app.apply_dialog_action(DialogAction::SetSendLevel { from: 1, to: ret, pre: false, level: 0.5, done: true });
    assert_eq!(app.strip_send_to(StripRef::Row(1), bus, false), Some(0.5));
    step(&mut app, "a level moved", &mut pictures);
    assert!(app.remove_send(StripRef::Row(2), bus, false), "the send was not there to take away");
    assert_eq!(app.strip_send_to(StripRef::Row(2), bus, false), None);
    step(&mut app, "a send taken away", &mut pictures);
    // Back through every step, then forward through every one.
    for k in (0..pictures.len() - 1).rev() {
        app.undo();
        assert_eq!(picture(&app), pictures[k], "undo did not land on step {k}");
    }
    for (k, want) in pictures.iter().enumerate().skip(1) {
        app.redo();
        assert_eq!(&picture(&app), want, "redo did not land on step {k}");
    }
}

/// **A send moved to the other side of its fader, and back, is a moment
/// each**, keeping its level and its place in the strip's list; and a
/// second send to one bus is refused, from either side.
#[test]
fn a_sends_side_of_the_fader_is_one_gesture_and_a_bus_is_sent_to_once() {
    let (_ctx, mut app) = app_on(desk(2), tiers::free("bus-pre"));
    let a = app.new_bus(BusRole::Return).expect("a return");
    let b = app.new_bus(BusRole::Return).expect("a second return");
    assert_eq!(app.settings.rows[b].name, "Return 2");
    assert!(app.add_send(StripRef::Row(0), a));
    assert!(app.add_send(StripRef::Row(0), b));
    let (na, nb) = (app.row_node(a), app.row_node(b));
    app.set_send_level(StripRef::Row(0), na, false, 0.3, true);
    let at = app.hist_at;
    assert!(app.set_send_pre(StripRef::Row(0), na, false));
    assert_eq!(app.hist_at, at + 1, "a flip was not one moment");
    assert_eq!(app.strip_send_to(StripRef::Row(0), na, true), Some(0.3), "the flip lost the level");
    assert_eq!(app.strip_send_to(StripRef::Row(0), na, false), None);
    let order: Vec<Node> = app.strip_bus_sends(StripRef::Row(0)).iter().map(|x| x.to).collect();
    assert_eq!(order, vec![na, nb], "the flip moved the send in the strip's list");
    // Sent already: refused, from either side, with the line, no moment.
    let notes = app.said().notes().len();
    assert!(!app.add_send(StripRef::Row(0), a));
    assert!(said_since(&app, notes), "the refusal said nothing");
    assert_eq!(app.hist_at, at + 1);
    app.undo();
    assert_eq!(app.strip_send_to(StripRef::Row(0), na, false), Some(0.3), "the flip's undo");
}

/// **A bus refuses clips and inputs** (design 9: no clips, no input chip,
/// never hears MIDI, never frozen), on every road that could give it one,
/// and a file dropped on it makes a new track instead.
#[test]
fn a_bus_refuses_clips_and_inputs() {
    let root = project_scratch("bus-limits");
    let (_ctx, mut app) = app_on(desk(2), tiers::free("bus-limits"));
    app.bootstrap_project(&root.join("Projects"));
    let bus = app.new_bus(BusRole::Bus).expect("a bus");
    let on_bus = |app: &TangentApp| app.arrangement.clips.iter().filter(|c| c.lane == bus).count();
    // An input of either kind, and a MIDI device set: nothing is written.
    let notes = app.said().notes().len();
    assert!(!app.set_channel_input_spec(bus, "3"), "a bus took an audio input");
    assert!(!app.set_channel_input_spec(bus, "midi:all"), "a bus took MIDI");
    assert!(said_since(&app, notes), "the refusal said nothing");
    app.set_channel_midi_sel(bus, recorder::MidiInputSel { devices: vec!["Keys".into()], channel: Some(2) });
    assert_eq!(app.channel_kind(bus), Some(ChannelKind::Bus), "an input turned the bus into a track");
    assert!(app.channel_input_spec(bus).is_empty());
    assert_eq!(app.channel_midi_sel(bus), recorder::MidiInputSel::EVERYTHING);
    // The arm: pressed on its own, and as part of a selection.
    app.apply_mixer_hit(Hit::Arm(Column::Track(bus)), 0.0);
    assert!(!app.is_armed(bus), "a bus was armed");
    app.set_armed_rows(&[0, bus]);
    assert!(app.is_armed(0) && !app.is_armed(bus), "a selection's arm reached the bus");
    // A drawn item, a clip dragged onto it, a paste aimed at it.
    app.land_drawn_midi_item(bus, 1.0, 3.0);
    assert_eq!(on_bus(&app), 0, "a MIDI item was drawn on a bus");
    let id = app.arrangement.add_clip(crate::arrangement::ArrClip {
        kind: crate::arrangement::ClipKind::Midi,
        lane: 0,
        start_s: 1.0,
        len_s: 1.0,
        ..Default::default()
    });
    app.arr_move(id, 2.0, 0.0, Some(bus));
    assert_eq!(app.arrangement.clips.iter().find(|c| c.id == id).map(|c| c.lane), Some(0), "a clip was dragged onto a bus");
    app.clip_clipboard = vec![crate::arrangement::ArrClip { lane: 0, len_s: 1.0, ..Default::default() }];
    app.select_alone(bus);
    app.arr_paste();
    assert_eq!(on_bus(&app), 0, "a paste landed on a bus");
    // A track with clips is refused the bus kind; a bus is never frozen.
    app.set_channel_kind(0, Some(ChannelKind::Bus));
    assert_eq!(app.channel_kind(0), Some(ChannelKind::Midi), "a track with clips became a bus");
    assert!(!app.can_freeze(bus), "a bus offered a freeze");
    // A file dropped on the bus: a new track at the end takes it.
    let wav = root.join("dropped.wav");
    std::fs::write(&wav, b"RIFF").expect("a file to drop");
    let rows = app.desk_extent();
    app.import_audio_at(&wav, bus, 0.0).expect("the drop was refused outright");
    assert_eq!(on_bus(&app), 0, "the drop landed on the bus");
    assert_eq!(app.desk_extent(), rows + 1, "the drop made no track");
    let landed = app.arrangement.clips.iter().find(|c| c.path.ends_with("dropped.wav")).map(|c| c.lane);
    assert_eq!(landed, Some(rows), "the dropped file is not on the new track");
    assert!(app.row_counted(rows), "the new track is not a track");
    assert!(app.is_bus(bus) && app.channel_input_spec(bus).is_empty() && !app.is_armed(bus));
    // Add with the bus selected makes a track beside it, never one on it.
    let id = app.track_id_at(bus);
    assert!(!app.add_bare_track(bus), "a bare track was made on a bus");
    app.select_alone(bus);
    let counted = app.counted_tracks();
    app.add_track();
    assert_eq!(app.counted_tracks(), counted + 1, "Add with a bus selected made no track");
    assert_eq!((app.track_id_at(bus), app.is_bus(bus)), (id, true), "Add with a bus selected changed the bus");
    let _ = std::fs::remove_dir_all(&root);
}

/// **A free desk at its thirty-two tracks makes a hundred buses and a
/// thousand sends** (design 9: unlimited on every tier), and its
/// thirty-third TRACK is still refused after all of them.
#[test]
fn a_free_desk_at_thirty_two_makes_a_hundred_buses_and_a_thousand_sends() {
    let (_ctx, mut app) = app_on(desk(32), tiers::free("bus-free-tier"));
    assert_eq!(app.counted_tracks(), 32, "fixture");
    assert!(app.may_add_tracks(1).is_err(), "fixture: the free desk is full");
    let mut buses = Vec::new();
    for k in 0..100 {
        let role = if k % 2 == 0 { BusRole::Bus } else { BusRole::Return };
        buses.push(app.new_bus(role).unwrap_or_else(|| panic!("bus {k} was refused")));
    }
    assert_eq!(app.counted_tracks(), 32, "a bus was counted");
    assert_eq!(app.settings.rows[buses[0]].name, "Bus 1");
    assert_eq!(app.settings.rows[buses[1]].name, "Return 1");
    assert_eq!(app.settings.rows[buses[99]].name, "Return 50");
    // Each track to thirty-one buses, then eight buses each to the next.
    let mut made = 0;
    for t in 0..32 {
        for j in 0..31 {
            assert!(app.add_send(StripRef::Row(t), buses[(t + j) % 100]), "send {made} was refused");
            made += 1;
        }
    }
    for k in 0..8 {
        assert!(app.add_send(StripRef::Row(buses[k]), buses[k + 1]), "a bus's send was refused");
        made += 1;
    }
    assert_eq!(made, 1000);
    assert_eq!(bus_sends(&app), 1000, "the desk does not hold every send");
    // And a track is still the one thing the free desk counts.
    app.add_track();
    assert_eq!(app.counted_tracks(), 32, "the free desk made a thirty-third track");
    assert!(app.may_add_tracks(1).is_err());
}

/// **The pickers grey a new loop, and the gestures refuse one** (design 9,
/// `desk_topology::would_cycle`): a strip whose audio already reaches this
/// one is offered greyed with the reason, a route or a send that would
/// close the loop is refused with the line and changes nothing, and a
/// route or send that closes none goes through. Buses lead the route list.
#[test]
fn the_pickers_grey_a_new_loop_and_the_gestures_refuse_one() {
    let (_ctx, mut app) = app_on(desk(3), tiers::free("bus-loops"));
    let x = app.new_bus(BusRole::Bus).expect("a bus");
    let y = app.new_bus(BusRole::Return).expect("a return");
    // Track 0 sends to X, and X is routed into Y.
    assert!(app.add_send(StripRef::Row(0), x));
    app.apply_dialog_action(DialogAction::AddRoute { from: x, target: LaneTarget::Channel(y) });
    assert_eq!(app.settings.routes.0.len(), 1, "fixture: the route");
    let Dialog::Routing { looped, channels, buses, .. } = app.routing_dialog(y, RoutingPage::Main) else {
        panic!("not the routing window");
    };
    assert!(looped.contains(&x) && looped.contains(&0), "a strip feeding Y was not greyed: {looped:?}");
    assert!(!looped.contains(&1) && !looped.contains(&2), "a strip that feeds nothing was greyed");
    assert_eq!(channels.first().map(|c| c.0), Some(x), "the buses do not lead the route list: {channels:?}");
    assert_eq!(buses.iter().map(|b| b.0).collect::<Vec<_>>(), vec![x], "the send list is not the other buses");
    // The gestures, asked anyway: refused, said, and nothing changed.
    let (routes, sends, at, notes) = (app.settings.routes.clone(), bus_sends(&app), app.hist_at, app.said().notes().len());
    app.apply_dialog_action(DialogAction::AddRoute { from: y, target: LaneTarget::Channel(0) });
    assert_eq!(app.settings.routes, routes, "a route closing a loop was made");
    assert!(!app.add_send(StripRef::Row(y), x), "a send closing a loop was made");
    assert!(!app.add_send(StripRef::Row(x), x), "a bus sent to itself");
    assert!(!app.add_send(StripRef::Row(0), 1), "a send went to a track that is no bus");
    assert_eq!(bus_sends(&app), sends);
    assert_eq!(app.hist_at, at, "a refusal left a moment");
    assert!(said_since(&app, notes), "the refusals said nothing");
    // What closes no loop goes through: Y into track 1, track 2 to X.
    app.apply_dialog_action(DialogAction::AddRoute { from: y, target: LaneTarget::Channel(1) });
    assert_eq!(app.settings.routes.0.len(), 2, "a route closing no loop was refused");
    assert_eq!(app.hist_at, at + 1, "a route made was not one moment");
    assert!(app.add_send(StripRef::Row(2), x));
}

/// **A loop a legacy file saved opens whole and is drawn as feedback**
/// (design 9): read through the legacy routing table as every legacy reader
/// reads it, nothing is cut at load, and the routing window names the
/// routes in the loop as feedback and the way out of it as a plain route.
/// What it sounds like is the tangent crate's to hear
/// (`a_saved_route_loop_opens_and_plays_as_the_legacy_shim_played_it`).
#[test]
fn a_saved_loop_opens_whole_and_is_marked_feedback() {
    let legacy = serde_json::json!({
        "routes": [
            {"from": 1, "kind": "channel", "channel": 2, "gain": 0.6},
            {"from": 2, "kind": "channel", "channel": 1, "gain": 0.4},
            {"from": 2, "kind": "channel", "channel": 3, "gain": 0.5},
        ]
    });
    let mut s = Settings::from_json(&legacy.to_string());
    s.show_welcome = false;
    let (_ctx, app) = app_on(s, tiers::free("bus-saved-loop"));
    assert_eq!(app.settings.routes.0.len(), 3, "a saved loop was cut at load");
    let Dialog::Routing { existing, looped, .. } = app.routing_dialog(2, RoutingPage::Main) else {
        panic!("not the routing window");
    };
    let labels: Vec<&str> = existing.iter().map(|(_, _, l, _)| l.as_str()).collect();
    assert_eq!(labels.len(), 2, "{labels:?}");
    assert!(labels[0].ends_with("(feedback)"), "the route back into the loop is not marked: {labels:?}");
    assert!(!labels[1].ends_with("(feedback)"), "the way out of the loop is marked: {labels:?}");
    assert!(looped.contains(&1), "a new route back round the loop is offered");
}

/// **Every route, send and key end survives a reorder, an insert and a
/// bystander's delete exactly**, by identity, and an end that leaves takes
/// what named it with it until its undo brings all of it back.
#[test]
fn route_send_and_key_ends_survive_a_reorder_an_insert_and_a_delete() {
    use crate::desk_id::slot_key;
    let (_ctx, mut app) = app_on(desk(4), tiers::supporter("bus-ends"));
    let bus = app.new_bus(BusRole::Bus).expect("a bus");
    let ret = app.new_bus(BusRole::Return).expect("a return");
    let (b, c, d) = (app.row_node(1), app.row_node(2), app.row_node(3));
    let (bus_n, ret_n) = (app.row_node(bus), app.row_node(ret));
    // Track 0 routed into the bus, track 1 sent to the return, track 2's
    // second bay keyed from the bus.
    app.apply_dialog_action(DialogAction::AddRoute { from: 0, target: LaneTarget::Channel(bus) });
    assert!(app.add_send(StripRef::Row(1), ret));
    let keyed = slot_key(c, 1).expect("a bay");
    app.settings.insert_keys.set(keyed, Some(bus_n));
    app.commit_view_change();
    let (routes, keys, sends) = (app.settings.routes.clone(), app.settings.insert_keys.clone(), picture(&app).1);
    let row = |app: &TangentApp, n: Node| n.track().and_then(|id| app.row_of(id)).expect("on the desk");
    let check = |app: &TangentApp, edit: &str| {
        assert_eq!(app.settings.routes, routes, "{edit} rewrote the routing table");
        assert_eq!(app.settings.insert_keys, keys, "{edit} rewrote the key table");
        assert_eq!(picture(app).1, sends, "{edit} moved a send");
        assert_eq!(
            app.strip_send_to(StripRef::Row(row(app, b)), ret_n, false),
            Some(1.0),
            "after {edit} the send is not on its track's row"
        );
        let Dialog::Routing { existing, .. } = app.routing_dialog(row(app, routes.0[0].from), RoutingPage::Main) else {
            panic!("not the routing window");
        };
        assert_eq!(
            existing.iter().map(|(_, dest, _, _)| *dest).collect::<Vec<_>>(),
            vec![LaneTarget::Channel(row(app, bus_n))],
            "after {edit} the route does not land on the bus's row"
        );
    };
    check(&app, "nothing");
    // The bus dragged to the top: every track below it moves down a row.
    app.move_channel(bus, 0);
    assert_eq!(row(&app, bus_n), 0, "fixture: the drag");
    check(&app, "a drag reorder");
    app.undo();
    check(&app, "the drag's undo");
    // A track inserted under row 0.
    assert!(app.insert_track_below(0), "fixture: the insert");
    check(&app, "an insert-below");
    // A bystander leaves.
    app.remove_channel(row(&app, d));
    check(&app, "a bystander's delete");
    // The return leaves: the send into it goes with it, and comes back.
    app.remove_channel(row(&app, ret_n));
    assert_eq!(bus_sends(&app), 0, "a send outlived its return");
    app.undo();
    check(&app, "the return's delete undone");
    // The bus leaves: the route into it and the key from it go, and come back.
    app.remove_channel(row(&app, bus_n));
    assert!(app.settings.routes.0.is_empty(), "a route outlived its bus");
    assert!(app.settings.insert_keys.0.is_empty(), "a key outlived its bus");
    app.undo();
    check(&app, "the bus's delete undone");
}

/// **A bus made a track asks the cap, keeps its identity and lets go of
/// what was sent to it** (design 5.3: `set_channel_kind` from a bus to MIDI
/// or AUDIO is a road; design 9: only a bus takes a send). A track made a
/// bus asks nothing and stops counting; a copied bus pastes at the cap.
#[test]
fn a_bus_made_a_track_asks_the_cap_and_keeps_its_identity() {
    let (_ctx, mut app) = app_on(desk(2), tiers::free("bus-kind"));
    app.force_free_cap(2);
    let bus = app.new_bus(BusRole::Bus).expect("a bus at the cap");
    assert!(app.add_send(StripRef::Row(0), bus));
    let id = app.track_id_at(bus);
    let notes = app.said().notes().len();
    app.set_channel_kind(bus, Some(ChannelKind::Midi));
    assert!(app.is_bus(bus), "a bus became a third track on a desk capped at two");
    assert!(said_since(&app, notes), "the refusal said nothing");
    assert_eq!(bus_sends(&app), 1, "the refused change took the send");
    // A track becomes a bus: no question asked, one fewer counted.
    app.set_channel_kind(1, Some(ChannelKind::Bus));
    assert!(app.is_bus(1));
    assert_eq!(app.counted_tracks(), 1);
    // Now the first bus may become a track: the same strip, its sends gone.
    app.set_channel_kind(bus, Some(ChannelKind::Audio));
    assert_eq!(app.channel_kind(bus), Some(ChannelKind::Audio));
    assert_eq!(app.track_id_at(bus), id, "the bus turned track took a new identity");
    assert_eq!(bus_sends(&app), 0, "a send into a track was kept");
    assert_eq!(app.counted_tracks(), 2);
    // A copied bus pastes at the cap: it is a new bus, not a track.
    app.copy_track(1);
    assert!(app.paste_track_below(1), "a copied bus was refused at the cap");
    assert!(app.is_bus(2), "the paste is not a bus");
    assert_eq!(app.counted_tracks(), 2);
}

/// **The track menu's New Bus and New Return, and the routing window's
/// own two**: each one moment, at the end of the desk, never the cap's; the
/// window's makes the bus and routes or sends the channel to it in that
/// one moment.
#[test]
fn every_road_to_a_new_bus_is_one_moment_at_the_end_of_the_desk() {
    let (_ctx, mut app) = app_on(desk(3), tiers::free("bus-roads"));
    app.force_free_cap(3);
    let at = app.hist_at;
    app.apply_dialog_action(DialogAction::NewBus(BusRole::Return));
    assert_eq!(app.hist_at, at + 1, "New Return was not one moment");
    assert!(app.is_bus(3) && app.settings.rows[3].name == "Return 1");
    app.apply_dialog_action(DialogAction::SendToNewReturn(0));
    assert_eq!(app.hist_at, at + 2, "Send to a New Return was not one moment");
    assert!(app.is_bus(4) && app.settings.rows[4].name == "Return 2");
    assert_eq!(app.strip_send_to(StripRef::Row(0), app.row_node(4), false), Some(1.0));
    app.apply_dialog_action(DialogAction::RouteToNewBus(1));
    assert_eq!(app.hist_at, at + 3, "A New Bus was not one moment");
    assert!(app.is_bus(5) && app.settings.rows[5].name == "Bus 1");
    assert_eq!(
        app.settings.routes.0,
        vec![crate::routing::Route::new(app.row_node(1), crate::routing::Target::Node(app.row_node(5)))]
    );
    assert_eq!(app.counted_tracks(), 3);
    // One undo takes the bus and its route together.
    app.undo();
    assert_eq!(app.desk_extent(), 5);
    assert!(app.settings.routes.0.is_empty(), "the undo left the route");
}

/// **A send's curve moves its send and names its bus wherever the bus
/// stands** (`AutoParam::SendTo`): its dial reads and writes the send's
/// level, moves nothing on a strip with no such send, and the curve is
/// saved by its key and follows its bus through a reorder.
#[test]
fn a_send_curve_moves_its_send_and_names_its_bus_wherever_it_stands() {
    use crate::automation::{AutoParam, Point, Shape};
    let (_ctx, mut app) = app_on(desk(2), tiers::free("bus-curve"));
    let bus = app.new_bus(BusRole::Return).expect("a return");
    assert!(app.add_send(StripRef::Row(0), bus));
    let to = app.track_id_at(bus);
    let p = AutoParam::SendTo { to };
    assert_eq!(app.auto_resting(AutoLane::Row(0), p), 1.0, "the curve's rest is not the send");
    app.auto_set_control(AutoLane::Row(0), p, 0.25);
    assert_eq!(app.strip_send_to(StripRef::Row(0), Node::Track(to), false), Some(0.25));
    app.auto_set_control(AutoLane::Row(1), p, 0.8);
    assert_eq!(app.strip_send_to(StripRef::Row(1), Node::Track(to), false), None, "a dial made a send");
    assert_eq!(app.auto_resting(AutoLane::Row(1), p), 0.0);
    // The mixer's row drags the same send, and its curve follows a lone base.
    app.arrangement.automation.entry(0, p).put(Point { at_s: 0.0, value: 0.25, shape: Shape::Linear });
    app.apply_mixer_hit(Hit::SendLevel(Column::Track(0), 0), 0.75);
    assert_eq!(app.strip_send_to(StripRef::Row(0), Node::Track(to), false), Some(0.75));
    let base = app.arrangement.automation.get(AutoLane::Row(0), p).and_then(|c| c.points().first()).map(|q| q.value);
    assert_eq!(base, Some(0.75), "a lone base did not follow its send");
    // Saved by its key, read back by it.
    let doc = app.arrangement.to_json();
    assert!(doc.contains(&format!("\"send:t{}\"", to.0)), "the send curve is not saved by its key");
    let back = crate::arrangement::Arrangement::from_json(&doc);
    assert!(back.automation.get(AutoLane::Row(0), p).is_some(), "the send curve was lost to the file");
    // The bus moves; the curve still names it and drives it.
    app.move_channel(bus, 0);
    assert_eq!(app.row_of(to), Some(0));
    app.auto_set_control(AutoLane::Row(1), p, 0.5);
    assert_eq!(app.strip_send_to(StripRef::Row(1), Node::Track(to), false), Some(0.5), "the curve lost its bus");
}

/// **A track's post-fader sends are offered for automation, by their
/// buses' names** (`AutoParam::SendTo`): one row each in a Sends group of
/// the A menu, keyed `send:t<id>`, after the desk's own four; a pre-fader
/// send has no curve, and a track that sends nowhere has no group.
#[test]
fn a_tracks_post_fader_sends_are_offered_for_automation_by_their_buses_names() {
    let (_ctx, mut app) = app_on(desk(2), tiers::free("bus-auto-menu"));
    let a = app.new_bus(BusRole::Return).expect("a return");
    let b = app.new_bus(BusRole::Return).expect("a second return");
    assert!(app.add_send(StripRef::Row(0), a));
    assert!(app.add_send(StripRef::Row(0), b));
    assert!(app.set_send_pre(StripRef::Row(0), app.row_node(b), false));
    let id = app.track_id_at(a);
    app.arr_open_automation_menu(AutoLane::Row(0));
    let Some(Dialog::Automation { groups, .. }) = &app.dialog else {
        panic!("not the automation menu");
    };
    let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names.first(), Some(&"Desk"));
    let sends = groups.iter().find(|g| g.name == "Sends").expect("no Sends group");
    assert_eq!(
        sends.rows.iter().map(|r| (r.key.clone(), r.label.clone())).collect::<Vec<_>>(),
        vec![(format!("send:t{}", id.0), "send to Return 1".to_owned())],
        "the menu does not offer exactly the post-fader send"
    );
    app.arr_open_automation_menu(AutoLane::Row(1));
    let Some(Dialog::Automation { groups, .. }) = &app.dialog else {
        panic!("not the automation menu");
    };
    assert!(groups.iter().all(|g| g.name != "Sends"), "a track that sends nowhere offered sends");
}
