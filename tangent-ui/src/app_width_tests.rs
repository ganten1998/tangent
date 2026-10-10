//! **The desk at every width** (design 11.6, W6-rows-unlimited's
//! done-when): reorder, delete above, insert above, undo and redo, folder
//! runs, the switches, theory, inputs and kits at one row, at the legacy
//! thirty-two and one past it, either side of where a 64-bit mask ends, and
//! at two widths well past both; then the trailing add and its lock, the
//! hidden rows, a drop below the last lane, the record's and the export's
//! holds, and undo across every structural gesture. A child of `app`, in a
//! file of its own as `save_tests` is, so it reads the app's private roads
//! the way its `tests` module does.
//!
//! **What every edit here is held to.** Each track's whole world (its row,
//! every field of it, its clips, its curves, its kit, the drum panel aimed
//! at it, its automation lanes' mode, latch and open rows) is keyed by the
//! track, so after an edit every track still on the desk has exactly the
//! world it had, on whatever row it now stands. Whatever still counted rows
//! by an index (a mask that stopped at bit 31 or 63, a walk bounded by the
//! old width, a session field that named a row) shows up as a track whose
//! world changed under it. `DeskModel::check` is asserted after every
//! structural edit and every commit (`carry_lanes`, `desk_sentinel`), and a
//! free app's commits are watched by the cap sentinel, so both are silent
//! across this suite by construction.

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::automation::{AutoMode, AutoParam, Point, Shape};
use crate::desk_id::{Node, TrackId};
use crate::desk_model::{RowKind, TrackRow};
use crate::mixer_panel::{Hit, Plus};
use crate::recorder::Column;
use tangent_core::license::LicenseStore;

/// The widths design 11.6 names: one row, the legacy desk and one past it,
/// either side of a 64-bit mask's end, and two well past both.
const WIDTHS: [usize; 7] = [1, 32, 33, 64, 65, 200, 512];

/// **The ids start here**, so no track's id is the row it stands on: a
/// road that confused the two would pass by accident wherever id == row.
const FIRST_ID: u32 = 1000;

/// The desktop's capabilities without the settings file: nothing here is
/// a setting anybody keeps.
const QUIET: Caps = Caps {
    persist_global_settings: false,
    ..Caps::DESKTOP
};

/// **Row `n` of a test desk**: a track that differs from its neighbours in
/// every fact a row carries (kind, name, colour, fader, pan, input, the
/// four switches, theory), so a fact an edit left behind is a row that no
/// longer matches its own track.
fn row_for(n: usize) -> TrackRow {
    let mut r = TrackRow::vacant(TrackId(FIRST_ID + n as u32));
    let audio = n % 2 == 0;
    r.kind = Some(if audio { RowKind::Audio } else { RowKind::Midi });
    r.added = true;
    r.name = format!("Track {n}");
    r.color = (n % 32) as i64;
    r.gain = 0.5 + (n % 10) as f64 / 100.0;
    r.pan = ((n % 7) as f64 - 3.0) / 10.0;
    r.input = if audio { format!("{}", n % 8) } else { "midi:all".to_owned() };
    r.arm = n % 5 == 0;
    r.mute = n % 7 == 3;
    r.solo = n % 11 == 4;
    r.phase = n % 3 == 1;
    r.theory = n % 4 == 2;
    r
}

/// **A desk of `width` tracks**, as a document brings one in.
fn desk(width: usize) -> Settings {
    let mut s = Settings {
        show_welcome: false,
        ..Settings::default()
    };
    s.plugins.clear();
    s.rows = (0..width).map(row_for).collect();
    s.next_track_id = u64::from(FIRST_ID) + width as u64;
    s.arm_unsaid = false;
    s
}

/// An app on `settings` with the history starting there.
fn app_on(settings: Settings, store: LicenseStore) -> (egui::Context, TangentApp) {
    let ctx = egui::Context::default();
    let mut app = TangentApp::with_license(&ctx, settings, QUIET, store);
    app.undo_reset();
    (ctx, app)
}

/// **Where the history stands**: one more for each gesture committed, one
/// fewer for each undo. Not the history's length, which a commit after an
/// undo truncates and pushes, so it can stay the same across a gesture.
fn moments(app: &TangentApp) -> usize {
    app.hist_at
}

/// A supporter on a desk of `width` tracks: nothing here is about the cap.
fn wide(width: usize, tag: &str) -> (egui::Context, TangentApp) {
    app_on(desk(width), tiers::supporter(tag))
}

fn clip_on(lane: usize) -> crate::arrangement::ArrClip {
    crate::arrangement::ArrClip {
        kind: crate::arrangement::ClipKind::Audio,
        lane,
        path: "media/x.wav".to_owned(),
        start_s: 1.0,
        len_s: 1.0,
        gain: 1.0,
        ..Default::default()
    }
}

/// **Session state on row `row`'s track**: a clip, a gain curve, a kit,
/// its automation lane in Latch with the latch set and the Gain row open,
/// and with `panel` the drum panel aimed at it too.
fn dress(app: &mut TangentApp, row: usize, panel: bool) {
    let id = app.track_id_at(row);
    app.arrangement.add_clip(clip_on(row));
    app.arrangement.automation.entry(row, AutoParam::Gain).put(Point {
        at_s: 1.0,
        value: 0.4,
        shape: Shape::Linear,
    });
    app.kit_at_mut(row).pads[36].path = format!("media/kick-{}.wav", id.0);
    let owner = app.owner_at(row);
    app.arr_auto_modes.insert(owner, AutoMode::Latch);
    app.arr_auto_latched.insert(owner);
    app.arr_auto_open.insert((owner, AutoParam::Gain));
    if panel {
        app.set_drumpad_lane(row);
    }
}

/// **Everything the session keeps of each track, by the track.**
#[derive(Debug, Clone, PartialEq)]
struct World {
    rows: BTreeMap<TrackId, TrackRow>,
    clips: BTreeMap<u64, TrackId>,
    curves: BTreeSet<(TrackId, String)>,
    kits: BTreeMap<TrackId, String>,
    panel: TrackId,
    modes: BTreeMap<TrackId, String>,
    latched: BTreeSet<TrackId>,
    open: BTreeSet<(TrackId, String)>,
}

impl World {
    /// This world for the tracks `keep` alone (a removed track's inert
    /// entries, an inserted track's new ones, set aside).
    fn only(mut self, keep: &BTreeSet<TrackId>) -> Self {
        self.rows.retain(|id, _| keep.contains(id));
        self.clips.retain(|_, id| keep.contains(id));
        self.curves.retain(|(id, _)| keep.contains(id));
        self.kits.retain(|id, _| keep.contains(id));
        self.modes.retain(|id, _| keep.contains(id));
        self.latched.retain(|id| keep.contains(id));
        self.open.retain(|(id, _)| keep.contains(id));
        self
    }
}

/// The row's every field, by name and exhaustively (no `..`), so a field
/// added to `TrackRow` fails this to compile until somebody has decided
/// whether the width suites must see it travel. Everything it names is in
/// the comparison `World` makes, because `World` keeps the row whole.
fn every_field(r: &TrackRow) -> usize {
    let TrackRow {
        id: _,
        kind: _,
        added: _,
        icon: _,
        name: _,
        color: _,
        gain: _,
        pan: _,
        input: _,
        midi: _,
        mute: _,
        solo: _,
        phase: _,
        arm: _,
        theory: _,
        folder: _,
        in_folder: _,
        shut: _,
        sends: _,
        dx7: _,
        keys: _,
        frozen: _,
    } = r;
    22
}

fn world(app: &TangentApp) -> World {
    let id_of = |row: usize| app.track_at(row).unwrap_or_else(|| panic!("row {row} is past the desk"));
    let track = |n: &Node| match n {
        Node::Track(id) => Some(*id),
        _ => None,
    };
    for r in &app.settings.rows {
        let _ = every_field(r);
    }
    World {
        rows: app.settings.rows.iter().map(|r| (r.id, r.clone())).collect(),
        clips: app.arrangement.clips.iter().map(|c| (c.id, id_of(c.lane))).collect(),
        curves: app
            .arrangement
            .automation
            .curves()
            .iter()
            .filter_map(|c| match c.lane {
                AutoLane::Row(r) => Some((id_of(r), format!("{:?}", c.param))),
                AutoLane::Master => None,
            })
            .collect(),
        kits: app.drumpad_kits.iter().map(|(id, k)| (*id, k.pads[36].path.clone())).collect(),
        panel: app.drumpad_track(),
        modes: app.arr_auto_modes.iter().filter_map(|(n, m)| Some((track(n)?, format!("{m:?}")))).collect(),
        latched: app.arr_auto_latched.iter().filter_map(track).collect(),
        open: app.arr_auto_open.iter().filter_map(|(n, p)| Some((track(n)?, format!("{p:?}")))).collect(),
    }
}

/// **The drum panel and the latched automation row stand on `row`**: the
/// panel shows that row's kit, and the lane there is in Latch, latched and
/// open, read by the row as the views and the engine read them.
fn panel_and_latch_on(app: &TangentApp, row: usize, what: &str) {
    assert_eq!(app.drumpad_lane(), row, "{what}: the drum panel is not on its track's row");
    assert_eq!(
        app.drumpad().pads[36].path,
        format!("media/kick-{}.wav", app.track_id_at(row).0),
        "{what}: the drum panel shows another track's kit"
    );
    assert_eq!(app.arr_auto_mode(AutoLane::Row(row)), AutoMode::Latch, "{what}: the Latch is not on its track's row");
    assert!(app.arr_auto_latched.contains(&app.owner_at(row)), "{what}: the latch is not on its track's row");
    assert!(
        app.arr_auto_is_open(AutoLane::Row(row), AutoParam::Gain),
        "{what}: the open automation row is not under its track"
    );
}

/// **A reorder at every width moves each track whole, and undoes.** The
/// last track to the top, so every row on the desk moves.
#[test]
fn a_reorder_at_every_width_moves_every_track_whole_through_undo_and_redo() {
    const TAG: &str = "width-reorder";
    for width in WIDTHS {
        let (_ctx, mut app) = wide(width, TAG);
        let last = width - 1;
        for row in [0, width / 2, last] {
            dress(&mut app, row, row == last);
        }
        app.undo_reset();
        let before = world(&app);
        let ids = app.track_ids();
        let was_at = moments(&app);
        app.move_channel(last, 0);
        let moved: Vec<TrackId> = std::iter::once(ids[last]).chain(ids[..last].iter().copied()).collect();
        assert_eq!(app.track_ids(), moved, "width {width}: the rows are not in the order the drag asked");
        assert_eq!(world(&app), before, "width {width}: a track's world changed in the reorder");
        assert_eq!(moments(&app), was_at + usize::from(width > 1), "width {width}: the drag is not one moment");
        panel_and_latch_on(&app, 0, &format!("width {width}, reordered"));
        app.undo();
        assert_eq!(app.track_ids(), ids, "width {width}: the undo did not put the rows back");
        assert_eq!(world(&app), before, "width {width}: a track's world changed in the undo");
        panel_and_latch_on(&app, last, &format!("width {width}, undone"));
        app.redo();
        assert_eq!(app.track_ids(), moved, "width {width}: the redo did not move the rows again");
        assert_eq!(world(&app), before, "width {width}: a track's world changed in the redo");
        panel_and_latch_on(&app, 0, &format!("width {width}, redone"));
    }
    let _ = std::fs::remove_dir_all(tiers::dir(TAG));
}

/// **A track removed above the rest, at every width** (a desk one wider,
/// so it ends at the width): every track below moves up whole, the drum
/// panel and the latched automation row with their own tracks; the undo
/// brings the removed track back on its row with its id and everything it
/// had, and the redo takes it again.
#[test]
fn a_track_removed_above_at_every_width_leaves_every_other_whole_through_undo_and_redo() {
    const TAG: &str = "width-remove";
    for width in WIDTHS {
        let (_ctx, mut app) = wide(width + 1, TAG);
        let last = width;
        dress(&mut app, 0, false);
        for row in BTreeSet::from([1 + width / 2, last]) {
            dress(&mut app, row, row == last);
        }
        app.undo_reset();
        let before = world(&app);
        let ids = app.track_ids();
        let stay: BTreeSet<TrackId> = ids[1..].iter().copied().collect();
        let was_at = moments(&app);
        app.apply_dialog_action(DialogAction::TrackRemove(0));
        assert_eq!(moments(&app), was_at + 1, "width {width}: the removal is not one moment");
        assert_eq!(app.track_ids(), ids[1..].to_vec(), "width {width}: the rows below did not move up in order");
        let after = world(&app);
        assert_eq!(after.clone().only(&stay), before.clone().only(&stay), "width {width}: a track below the removal changed");
        assert!(!after.clips.values().any(|id| *id == ids[0]), "width {width}: the removed track's clip stayed");
        panel_and_latch_on(&app, last - 1, &format!("width {width}, removed above"));
        app.undo();
        assert_eq!(app.track_ids(), ids, "width {width}: the undo did not bring the track back on its row");
        assert_eq!(world(&app), before, "width {width}: the undo did not bring back everything");
        panel_and_latch_on(&app, last, &format!("width {width}, undone"));
        app.redo();
        assert_eq!(app.track_ids(), ids[1..].to_vec(), "width {width}: the redo did not take the track again");
        assert_eq!(world(&app).only(&stay), before.clone().only(&stay), "width {width}: the redo changed a track below");
        panel_and_latch_on(&app, last - 1, &format!("width {width}, redone"));
    }
    let _ = std::fs::remove_dir_all(tiers::dir(TAG));
}

/// **Tracks inserted above the rest, at every width**: Insert Track and
/// Paste Track under the first, each a new identity and one moment, every
/// track below moving down whole with its panel and its latch; undo and
/// redo walk both.
#[test]
fn tracks_inserted_above_at_every_width_move_every_other_whole_through_undo_and_redo() {
    const TAG: &str = "width-insert";
    for width in WIDTHS {
        let (_ctx, mut app) = wide(width, TAG);
        let last = width - 1;
        for row in BTreeSet::from([0, width / 2, last]) {
            dress(&mut app, row, row == last);
        }
        app.undo_reset();
        let before = world(&app);
        let ids = app.track_ids();
        let all: BTreeSet<TrackId> = ids.iter().copied().collect();
        // A row below the first moves down; the first itself stays.
        let shifted = |row: usize, by: usize| if row == 0 { 0 } else { row + by };

        let was_at = moments(&app);
        assert!(app.insert_track_below(0), "width {width}: Insert Track was refused");
        assert_eq!(moments(&app), was_at + 1, "width {width}: Insert Track is not one moment");
        let inserted = app.track_id_at(1);
        assert!(!all.contains(&inserted), "width {width}: the inserted track wears an id the desk had");
        let mut want = ids.clone();
        want.insert(1, inserted);
        assert_eq!(app.track_ids(), want, "width {width}: the insert did not open a row under the first");
        assert_eq!(world(&app).only(&all), before, "width {width}: a track changed under the insert");
        panel_and_latch_on(&app, shifted(last, 1), &format!("width {width}, inserted"));
        let one = world(&app);

        app.copy_track(0);
        assert!(app.paste_track_below(0), "width {width}: Paste Track was refused");
        assert_eq!(moments(&app), was_at + 2, "width {width}: Paste Track is not one moment");
        let pasted = app.track_id_at(1);
        assert!(pasted != inserted && !all.contains(&pasted), "width {width}: the paste is not a new track");
        want.insert(1, pasted);
        assert_eq!(app.track_ids(), want, "width {width}: the paste did not land under the first");
        let (copy, source) = (&app.settings.rows[1], &app.settings.rows[0]);
        assert!(
            copy.name.starts_with(&source.name) && (copy.kind, copy.input.as_str()) == (source.kind, source.input.as_str()),
            "width {width}: the paste is not the copy ({} from {})",
            copy.name,
            source.name
        );
        assert_eq!(world(&app).only(&all), before, "width {width}: a track changed under the paste");
        panel_and_latch_on(&app, shifted(last, 2), &format!("width {width}, pasted"));
        let two = world(&app);
        let pasted_ids = app.track_ids();

        app.undo();
        assert_eq!(world(&app), one, "width {width}: the paste's undo is not the desk before it");
        app.undo();
        assert_eq!(app.track_ids(), ids, "width {width}: the undos did not put the rows back");
        assert_eq!(world(&app), before, "width {width}: the undos changed a track");
        panel_and_latch_on(&app, last, &format!("width {width}, undone"));
        app.redo();
        app.redo();
        assert_eq!(app.track_ids(), pasted_ids, "width {width}: the redos did not bring both back with their ids");
        assert_eq!(world(&app), two, "width {width}: the redos changed a track");
    }
    let _ = std::fs::remove_dir_all(tiers::dir(TAG));
}

/// **A folder run at every width stays whole** through a removal above it
/// and a reorder carrying it to the top, and through their undo and redo:
/// the header a folder and each member directly under it, filed in it.
#[test]
fn a_folder_run_at_every_width_stays_whole_through_the_edits_around_it() {
    const TAG: &str = "width-folder";
    for width in WIDTHS {
        // A track to remove above everything, one that stays above the run
        // (so carrying the run to the top moves it at every width), then
        // `width` more, the last two of them filed.
        let (_ctx, mut app) = wide(width + 3, TAG);
        let (a, b) = (width + 1, width + 2);
        app.set_primary_row(Some(a));
        app.set_selected_rows(&[a, b]);
        assert!(app.make_folder(), "width {width}: no folder was made");
        let header = app.track_id_at(a);
        let members = [app.track_id_at(a + 1), app.track_id_at(a + 2)];
        let run = |app: &TangentApp| -> Option<usize> {
            let h = app.row_of(header)?;
            let whole = app.is_folder(h)
                && members.iter().enumerate().all(|(k, m)| {
                    let r = h + 1 + k;
                    app.row_of(*m) == Some(r) && app.in_folder(r) && app.folder_of(r) == Some(h)
                });
            whole.then_some(h)
        };
        assert_eq!(run(&app), Some(a), "width {width}: the folder is not over its run");
        app.apply_dialog_action(DialogAction::TrackRemove(0));
        assert_eq!(run(&app), Some(a - 1), "width {width}: the run broke under a removal above it");
        app.set_selected_rows(&[]);
        app.set_primary_row(None);
        app.move_selection(a - 1, 0);
        assert_eq!(run(&app), Some(0), "width {width}: the run broke carried to the top");
        app.undo();
        assert_eq!(run(&app), Some(a - 1), "width {width}: the run broke in the reorder's undo");
        app.undo();
        assert_eq!(run(&app), Some(a), "width {width}: the run broke in the removal's undo");
        app.redo();
        app.redo();
        assert_eq!(run(&app), Some(0), "width {width}: the run broke in the redo");
    }
    let _ = std::fs::remove_dir_all(tiers::dir(TAG));
}

/// **The switches at every width reach the last track and only it**: arm,
/// mute, solo, polarity and the theory switch pressed on the last strip
/// change that row alone, each in one moment that undoes; alt-arm stands
/// every other track down, wherever it is on the desk.
#[test]
fn the_switches_at_every_width_reach_the_last_track_and_only_it() {
    const TAG: &str = "width-switch";
    type Read = fn(&TrackRow) -> bool;
    for width in WIDTHS {
        let (_ctx, mut app) = wide(width, TAG);
        let last = width - 1;
        let col = Column::Track(last);
        for (name, hit, read) in [
            ("arm", Hit::Arm(col), (|r: &TrackRow| r.arm) as Read),
            ("mute", Hit::Mute(col), |r: &TrackRow| r.mute),
            ("solo", Hit::Solo(col), |r: &TrackRow| r.solo),
            ("polarity", Hit::Phase(col), |r: &TrackRow| r.phase),
            ("theory", Hit::ToTheory(col), |r: &TrackRow| r.theory),
        ] {
            let before = app.settings.rows.clone();
            let was_at = moments(&app);
            app.apply_mixer_hit(hit, 0.0);
            assert_eq!(moments(&app), was_at + 1, "width {width}: {name} is not one moment");
            for (n, (now, was)) in app.settings.rows.iter().zip(&before).enumerate() {
                if n == last {
                    assert_ne!(read(now), read(was), "width {width}: {name} did not reach the last track");
                    let mut put_back = now.clone();
                    put_back.arm = was.arm;
                    put_back.mute = was.mute;
                    put_back.solo = was.solo;
                    put_back.phase = was.phase;
                    put_back.theory = was.theory;
                    assert_eq!(&put_back, was, "width {width}: {name} changed more of the last track than itself");
                } else {
                    assert_eq!(now, was, "width {width}: {name} on the last track changed track {n}");
                }
            }
            app.undo();
            assert_eq!(app.settings.rows, before, "width {width}: {name}'s undo did not put it back");
        }
        // Alt: the last track alone, and again, nobody (or the other way
        // round when it already stands alone, as the one row of a desk of
        // one does).
        let alone = app.armed_rows() == vec![last];
        for want in [!alone, alone] {
            app.alt_down = true;
            app.apply_mixer_hit(Hit::Arm(col), 0.0);
            app.alt_down = false;
            let armed = if want { vec![last] } else { Vec::new() };
            assert_eq!(app.armed_rows(), armed, "width {width}: alt-arm is not the last track alone or nobody");
        }
    }
    let _ = std::fs::remove_dir_all(tiers::dir(TAG));
}

/// **Inputs at every width bind the last track and only it**: an audio
/// pair on an audio last track (and its pick is counted among every audio
/// track above it, however many), a device and channel on a MIDI one; the
/// live routing table covers every row; the undo puts the binding back.
#[test]
fn an_input_at_every_width_binds_the_last_track_and_only_it() {
    const TAG: &str = "width-input";
    for width in WIDTHS {
        let (_ctx, mut app) = wide(width, TAG);
        let last = width - 1;
        let before = app.settings.rows.clone();
        let audio = app.channel_kind(last) == Some(recorder::ChannelKind::Audio);
        if audio {
            assert!(app.set_channel_input_spec(last, "5+6"), "width {width}: the audio input was refused");
            assert_eq!(app.settings.rows[last].input, "5+6");
            // Every even row is audio: the last one's pick is past all of them.
            assert_eq!(app.audio_ordinal(last), Some(last / 2), "width {width}: the pick is not counted past the audio above");
        } else {
            let sel = recorder::MidiInputSel {
                devices: vec!["Keys".to_owned()],
                channel: Some(3),
            };
            app.set_channel_midi_sel(last, sel.clone());
            assert_eq!(app.settings.rows[last].midi, sel, "width {width}: the device set did not land");
        }
        for (n, (now, was)) in app.settings.rows.iter().zip(&before).enumerate() {
            if n != last {
                assert_eq!(now, was, "width {width}: binding the last track changed track {n}");
            }
        }
        assert_eq!(app.midi_routes().len(), width, "width {width}: the routing table does not cover the desk");
        app.undo();
        assert_eq!(app.settings.rows, before, "width {width}: the binding's undo did not put it back");
    }
    let _ = std::fs::remove_dir_all(tiers::dir(TAG));
}

/// One frame of the whole app.
fn frame(ctx: &egui::Context, app: &mut TangentApp) {
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1300.0, 900.0))),
            ..Default::default()
        },
        |ctx| app.frame(ctx),
    );
}

/// **The last track at every width draws once either view is carried to
/// the end of its rail** (design 11.2 item 1's drawing half): the desk's
/// scrollbar at its far end places the last track's strip and the trailing
/// add after it, and the timeline's places its lane and the add under it.
#[test]
fn the_last_track_at_every_width_draws_with_either_view_at_the_end_of_its_rail() {
    const TAG: &str = "width-draw";
    for width in WIDTHS {
        let (ctx, mut app) = wide(width, TAG);
        app.dialog = None;
        let last = width - 1;
        app.settings.mixer_visible = 8;
        app.settings.arr_visible = 8;

        app.mixer_open = true;
        frame(&ctx, &mut app);
        let rect = app.mixer_rect.expect("the mixer drew nowhere");
        app.mixer_scroll_px = crate::mixer_panel::Layout::new(rect, &app.mixer_view_in(rect)).bar.max_px;
        frame(&ctx, &mut app);
        let v = app.mixer_view_in(rect);
        let l = crate::mixer_panel::Layout::new(rect, &v);
        assert!(
            l.strips.get(Column::Track(last)).is_some_and(|s| s.panel.is_positive()),
            "width {width}: the last track's strip is not drawn at the end of the desk"
        );
        let add = Column::Track(app.desk_extent());
        assert!(
            l.strips.get(add).is_some_and(|s| s.panel.is_positive()),
            "width {width}: no trailing add after the last strip"
        );
        assert_eq!(v.strips.get(add).map(|s| s.plus), Some(Plus::Trailing), "width {width}: the strip after the last is not the add");

        app.mixer_open = false;
        app.arrangement_open = true;
        frame(&ctx, &mut app);
        let rect = app.arrangement_rect.expect("the timeline drew nowhere");
        let max_rows = {
            let clips = app.arr_clip_views();
            let v = app.arrange_view(&clips);
            crate::arrangement_panel::Layout::new(rect, &v).lane_bar.max_rows
        };
        app.arr_lane_scroll = max_rows;
        frame(&ctx, &mut app);
        let clips = app.arr_clip_views();
        let v = app.arrange_view(&clips);
        let l = crate::arrangement_panel::Layout::new(rect, &v);
        assert!(l.lane_rows[last].is_positive(), "width {width}: the last lane is not drawn at the end of the timeline");
        let extent = app.desk_extent();
        assert!(l.lane_rows[extent].is_positive(), "width {width}: no trailing add under the last lane");
        assert_eq!(v.lanes.get(extent).map(|l| l.plus), Some(Plus::Trailing), "width {width}: the row under the last is not the add");
    }
    let _ = std::fs::remove_dir_all(tiers::dir(TAG));
}

/// The rows each view carries, in order, with what the last one is.
fn mixer_rows(app: &TangentApp) -> (Vec<usize>, Option<Plus>) {
    let v = app.mixer_view();
    let rows = v
        .strips
        .tracks
        .iter()
        .filter_map(|s| match s.strip {
            Some(recorder::Strip::Channel(r)) => Some(r),
            _ => None,
        })
        .collect();
    (rows, v.strips.tracks.last().map(|s| s.plus))
}

/// The timeline's window, carried to the end of its rail so the add under
/// the last lane is in it.
fn timeline_rows(app: &mut TangentApp) -> (Vec<usize>, Option<Plus>) {
    app.arr_lane_scroll = usize::MAX;
    let clips = app.arr_clip_views();
    let v = app.arrange_view(&clips);
    let rows: Vec<usize> = v.lanes.rows().map(|(r, _)| r).collect();
    let plus = rows.last().and_then(|r| v.lanes.get(*r)).map(|l| l.plus);
    (rows, plus)
}

/// **One trailing add, after the last track in both views, and locked at
/// the free cap** (design section 10): the shipped desk draws its two
/// tracks and the add, not its thirty hidden rows; the add makes the next
/// track on the first hidden row with a fresh id; at thirty-two tracks it
/// is a padlock in both views whose press opens the Supporter Key dialog
/// and makes nothing; a supporter at thirty-two grows the desk with it.
#[test]
fn the_trailing_add_follows_the_last_track_and_locks_at_the_free_cap() {
    let (_ctx, mut app) = app_on(
        Settings {
            show_welcome: false,
            ..Settings::default()
        },
        tiers::free("trailing-free"),
    );
    app.dialog = None;
    let extent = app.desk_extent();
    assert_eq!(app.desk_index().shown(), [0u32, 1], "the shipped desk draws its hidden rows");
    assert_eq!(mixer_rows(&app), (vec![0, 1, extent], Some(Plus::Trailing)), "the desk is not two tracks and the add");
    assert_eq!(timeline_rows(&mut app), (vec![0, 1, extent], Some(Plus::Trailing)), "the timeline is not two lanes and the add");

    // The desk's add: the next track, on the first hidden row, a new id.
    let hidden = app.track_id_at(2);
    app.apply_mixer_hit(Hit::Add(Column::Track(extent)), 0.0);
    assert_eq!(app.counted_tracks(), 3, "the trailing add made no track");
    assert!(app.row_counted(2), "the add did not land after the last track");
    assert_ne!(app.track_id_at(2), hidden, "the hidden row's identity became the new track's");
    assert_eq!(app.desk_extent(), extent, "the add grew the desk past a hidden row it could take");
    // The timeline's add: the same Add.
    app.apply_arrange_key(crate::arrangement_panel::Hit::HdrAdd(app.desk_extent()), None);
    assert_eq!(app.counted_tracks(), 4, "the timeline's trailing add made no track");

    // To the cap, through the add: locked in both views.
    while app.counted_tracks() < recorder::FREE_TRACKS {
        app.press_trailing_add();
    }
    assert_eq!(app.trailing_plus(), Plus::Locked, "the add is open at the free cap");
    assert_eq!(mixer_rows(&app).1, Some(Plus::Locked), "the desk's add is not a padlock at the cap");
    assert_eq!(timeline_rows(&mut app).1, Some(Plus::Locked), "the timeline's add is not a padlock at the cap");
    let (rows, was_at) = (app.settings.rows.clone(), moments(&app));
    app.apply_mixer_hit(Hit::Add(Column::Track(app.desk_extent())), 0.0);
    assert!(matches!(app.dialog, Some(Dialog::SupporterKey { .. })), "the padlock did not open the Supporter Key dialog");
    assert_eq!((&app.settings.rows, moments(&app)), (&rows, was_at), "the padlock made something");

    // A supporter at the same count: the add, open, grows the desk.
    let (_ctx, mut paid) = app_on(desk(32), tiers::supporter("trailing-paid"));
    assert_eq!(paid.trailing_plus(), Plus::Trailing, "a supporter's add is locked");
    paid.press_trailing_add();
    assert_eq!((paid.counted_tracks(), paid.desk_extent()), (33, 33), "a supporter's add did not grow the desk");
    for tag in ["trailing-free", "trailing-paid"] {
        let _ = std::fs::remove_dir_all(tiers::dir(tag));
    }
}

/// **A hidden row holding residue is never taken over** (design section
/// 4): a pedalboard left on the first hidden row keeps it hidden and
/// uncounted, and the next track is made on a row of its own past it.
#[test]
fn a_hidden_row_with_residue_on_it_is_never_reused() {
    let (_ctx, mut app) = app_on(
        Settings {
            show_welcome: false,
            ..Settings::default()
        },
        tiers::free("residue"),
    );
    let residue = app.track_id_at(2);
    let pedal = crate::desk_id::SeatBay::new(Node::Track(residue), crate::desk_id::Rack::Pedal(0), 0);
    app.set_plugin(pedal, Some("/x/Fuzz.vst3"));
    app.undo_reset();
    assert_eq!(app.desk_index().class(2), crate::desk_index::RowClass::Vacant, "fixture: the residue row is drawn");
    app.add_track();
    assert_eq!(app.counted_tracks(), 3, "the add made no track");
    assert_eq!(app.track_id_at(2), residue, "the add took over the row with a pedalboard on it");
    assert!(!app.row_counted(2), "the residue row became a track");
    assert_eq!(app.plugin(pedal), Some("/x/Fuzz.vst3"), "the pedalboard left its row");
    let _ = std::fs::remove_dir_all(tiers::dir("residue"));
}

/// **A file dropped below the last lane is a new track with a new id**
/// (design sections 3 and 10): on the first hidden row past the last
/// track, minted rather than wearing the hidden row's identity, one
/// moment that undoes to the hidden row as it was; on a desk with no
/// hidden row left, a row of its own at the end.
#[test]
fn a_file_dropped_below_the_last_lane_is_a_new_track_with_a_new_id() {
    let root = super::tests::project_scratch("w6-drop-below");
    let wav = root.join("take.wav");
    std::fs::write(&wav, b"RIFF").expect("a file to drop");
    let (_ctx, mut app) = app_on(
        Settings {
            show_welcome: false,
            ..Settings::default()
        },
        tiers::free("drop-below"),
    );
    app.bootstrap_project(&root.join("Projects"));
    let extent = app.desk_extent();
    let hidden = app.track_id_at(2);
    let was_at = moments(&app);
    app.import_audio_at(&wav, extent, 0.0).expect("the drop below the last lane was refused");
    assert_eq!(moments(&app), was_at + 1, "the drop is not one moment");
    assert_eq!(app.desk_extent(), extent, "the drop grew the desk past a hidden row it could take");
    assert!(app.arrangement.clips.iter().any(|c| c.lane == 2), "the clip did not land after the last track");
    let made = app.track_id_at(2);
    assert_ne!(made, hidden, "the new track wears the hidden row's identity");
    assert!(!app.settings.rows.iter().any(|r| r.id == hidden), "the hidden row's identity is still on the desk");
    app.undo();
    assert_eq!(app.track_id_at(2), hidden, "the undo did not put the hidden row back as it was");
    assert!(!app.row_counted(2));
    app.redo();
    assert_eq!(app.track_id_at(2), made, "the redo did not bring the new track back with its id");

    // A supporter's desk with every row a track: the drop is a row of its own.
    let (_c, mut full) = app_on(desk(32), tiers::supporter("drop-below-full"));
    full.bootstrap_project(&root.join("Full"));
    full.import_audio_at(&wav, 32, 0.0).expect("the drop below a full desk was refused");
    assert_eq!(full.desk_extent(), 33, "the drop below a full desk made no row");
    assert!(full.arrangement.clips.iter().any(|c| c.lane == 32));
    assert!(full.track_id_at(32).0 >= FIRST_ID + 32, "the appended row was not minted");
    let _ = std::fs::remove_dir_all(&root);
    for tag in ["drop-below", "drop-below-full"] {
        let _ = std::fs::remove_dir_all(tiers::dir(tag));
    }
}

/// **The holds** (design 7.13, W6's done-when): while the arrangement
/// records, and while a realtime export runs, inserting, removing,
/// reordering, a kind and an input binding each wait with their line and
/// change nothing, leaving no moment; Add appends at the end without
/// moving any row, the selection notwithstanding.
#[test]
fn a_record_and_an_export_hold_the_desk_shape_and_let_add_append() {
    for (exporting, recording, during) in [(true, false, "while an export runs"), (false, true, "while the arrangement records")] {
        let (_ctx, mut app) = wide(6, "holds");
        app.recorder.exporting = exporting;
        app.recorder.arr_recording = recording;
        let audio_row = 0;
        let midi_row = 1;
        let sel = recorder::MidiInputSel {
            devices: vec!["Keys".to_owned()],
            channel: Some(2),
        };
        let gestures: [(&str, &str, fn(&mut TangentApp, &recorder::MidiInputSel)); 7] = [
            ("Insert Track", "No track is inserted", |a, _| {
                let _ = a.insert_track_below(0);
            }),
            ("Paste Track", "No track is inserted", |a, _| {
                a.copy_track(2);
                let _ = a.paste_track_below(0);
            }),
            ("Remove Track", "No track is removed", |a, _| a.apply_dialog_action(DialogAction::TrackRemove(1))),
            ("a drag", "Tracks keep their order", |a, _| a.move_channel(4, 0)),
            ("a kind", "Tracks keep their kind", |a, _| a.set_channel_kind(1, Some(recorder::ChannelKind::Audio))),
            ("an audio input", "Inputs stay as they are", |a, _| {
                let _ = a.set_channel_input_spec(0, "7");
            }),
            ("a MIDI device set", "Inputs stay as they are", |a, s| a.set_channel_midi_sel(1, s.clone())),
        ];
        let _ = (audio_row, midi_row);
        for (name, line, go) in gestures {
            let (rows, was_at) = (app.settings.rows.clone(), moments(&app));
            app.messages.clear();
            go(&mut app, &sel);
            assert_eq!(app.settings.rows, rows, "{during}: {name} changed the desk");
            assert_eq!(moments(&app), was_at, "{during}: the held {name} left a moment");
            let said = app.messages.notes().last().map(|n| n.text.clone()).unwrap_or_default();
            assert!(said.starts_with(line) && said.contains(during), "{during}: {name} said {said:?}");
        }
        // Add, with a track in the middle selected: at the end, nobody moved.
        let ids = app.track_ids();
        app.select_alone(2);
        app.add_track();
        let now = app.track_ids();
        assert_eq!(now.len(), ids.len() + 1, "{during}: Add was refused");
        assert_eq!(now[..ids.len()], ids[..], "{during}: Add moved a row");
        let _ = std::fs::remove_dir_all(tiers::dir("holds"));
    }
}

/// **Undo and redo across Add, Insert, Paste and Remove** restore the rows
/// with their ids and every fact, one gesture per moment, on a free desk
/// under its cap (the cap sentinel watching every commit) and on a
/// supporter's past every old width.
#[test]
fn undo_and_redo_across_every_structural_gesture_restore_rows_with_their_ids() {
    for (width, store, tag) in [(6, tiers::free("undo-free"), "undo-free"), (70, tiers::supporter("undo-wide"), "undo-wide")] {
        let (_ctx, mut app) = app_on(desk(width), store);
        let mut seen = vec![(app.track_ids(), world(&app))];
        let gestures: [(&str, fn(&mut TangentApp)); 4] = [
            ("Add", |a| a.add_track()),
            ("Insert Track", |a| {
                let _ = a.insert_track_below(1);
            }),
            ("Paste Track", |a| {
                a.copy_track(0);
                let _ = a.paste_track_below(2);
            }),
            ("Remove Track", |a| a.apply_dialog_action(DialogAction::TrackRemove(3))),
        ];
        for (name, go) in gestures {
            let was_at = moments(&app);
            go(&mut app);
            assert_eq!(moments(&app), was_at + 1, "width {width}: {name} is not one moment");
            seen.push((app.track_ids(), world(&app)));
            assert_ne!(seen[seen.len() - 1].0, seen[seen.len() - 2].0, "width {width}: {name} changed no row");
        }
        for k in (0..seen.len() - 1).rev() {
            app.undo();
            assert_eq!((app.track_ids(), world(&app)), seen[k], "width {width}: undo to step {k} is not the desk it was");
        }
        for (k, want) in seen.iter().enumerate().skip(1) {
            app.redo();
            assert_eq!(&(app.track_ids(), world(&app)), want, "width {width}: redo to step {k} is not the desk it was");
        }
        let _ = std::fs::remove_dir_all(tiers::dir(tag));
    }
}

/// **Undo and redo cross the free cap** (design 11.2 item 10) at the cap
/// itself: a free desk of thirty-one adds its thirty-second, is refused the
/// thirty-third on Insert and on Paste (no moment), and walks back and
/// forth across thirty-two with the cap sentinel watching.
#[test]
fn undo_and_redo_cross_the_free_cap_at_thirty_two() {
    let (_ctx, mut app) = app_on(desk(31), tiers::free("undo-cap"));
    app.add_track();
    assert_eq!(app.counted_tracks(), 32);
    let was_at = moments(&app);
    assert!(!app.insert_track_below(0), "a free desk at its cap took an insert");
    app.copy_track(0);
    assert!(!app.paste_track_below(0), "a free desk at its cap took a paste");
    assert_eq!((app.counted_tracks(), moments(&app)), (32, was_at), "a refusal left a track or a moment");
    app.undo();
    assert_eq!(app.counted_tracks(), 31);
    app.redo();
    assert_eq!(app.counted_tracks(), 32);
    app.undo();
    assert!(app.insert_track_below(0), "a free desk back under its cap was refused");
    assert_eq!(app.counted_tracks(), 32);
    let _ = std::fs::remove_dir_all(tiers::dir("undo-cap"));
}

/// **A strip's width and a lane's height follow the count asked for, never
/// the track count** (design section 10): at eight across and eight down,
/// a desk of one track, the shipped desk of two among thirty hidden rows,
/// and desks of thirty-two and sixty-five draw their strips one width and
/// their lanes one height. Once vacant rows were hidden, a short desk
/// divided the run and the timeline among the few rows it drew.
#[test]
fn strips_and_lanes_are_sized_by_the_count_asked_for_not_by_the_desk() {
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1300.0, 700.0));
    let shipped = Settings {
        show_welcome: false,
        ..Settings::default()
    };
    let mut sizes = Vec::new();
    for (what, settings) in [("one track", desk(1)), ("the shipped desk", shipped), ("32", desk(32)), ("65", desk(65))] {
        let (_ctx, mut app) = app_on(settings, tiers::supporter("sized"));
        app.settings.mixer_visible = 8;
        app.settings.arr_visible = 8;
        let v = app.mixer_view_in(rect);
        let strip = crate::mixer_panel::Layout::new(rect, &v).strips[Column::Track(0)].panel.width();
        let clips = app.arr_clip_views();
        let v = app.arrange_view(&clips);
        let lane = crate::arrangement_panel::Layout::new(rect, &v).lane_rows[0].height();
        assert!(strip > 0.0 && lane > 0.0, "{what}: the first track was not drawn");
        sizes.push((what, strip, lane));
    }
    let (_, strip, lane) = sizes[sizes.len() - 1];
    for (what, w, h) in &sizes {
        assert!((w - strip).abs() < 0.01, "{what}: a strip is {w} wide, not the {strip} eight across gives");
        assert!((h - lane).abs() < 0.01, "{what}: a lane is {h} tall, not the {lane} eight down gives");
    }
    let _ = std::fs::remove_dir_all(tiers::dir("sized"));
}

/// **A MIDI item drawn on an empty lane, and a MIDI file dropped below the
/// last one, are each one moment**: the binding that makes the lane a MIDI
/// track commits a moment of its own, so each was two, and the first Ctrl+Z
/// took the item back and left a track nobody had asked for. One undo is
/// the desk before the gesture, and one redo the desk after it.
#[test]
fn a_midi_item_drawn_or_dropped_on_an_empty_lane_is_one_moment() {
    let root = super::tests::project_scratch("w6-midi-moment");
    let mid = root.join("c.mid");
    let track: &[u8] = &[0x00, 0x90, 60, 100, 0x60, 0x80, 60, 0, 0x00, 0xFF, 0x2F, 0x00];
    let mut smf = Vec::new();
    smf.extend_from_slice(b"MThd");
    smf.extend_from_slice(&6u32.to_be_bytes());
    smf.extend_from_slice(&[0, 0, 0, 1, 0, 96]);
    smf.extend_from_slice(b"MTrk");
    smf.extend_from_slice(&(track.len() as u32).to_be_bytes());
    smf.extend_from_slice(track);
    std::fs::write(&mid, smf).expect("the midi file");
    let (_ctx, mut app) = app_on(
        Settings {
            show_welcome: false,
            ..Settings::default()
        },
        tiers::free("midi-moment"),
    );
    app.bootstrap_project(&root.join("Projects"));
    app.undo_reset();
    type Gesture = fn(&mut TangentApp, &std::path::Path);
    let gestures: [(&str, Gesture); 2] = [
        ("a MIDI item drawn on an empty lane", |a, _| a.land_drawn_midi_item(2, 0.0, 8.0)),
        ("a MIDI file dropped below the last lane", |a, mid| {
            let below = a.desk_extent();
            a.import_midi_at(mid, below, 0.0).expect("the drop was refused");
        }),
    ];
    for (name, go) in gestures {
        let (rows, clips) = (app.settings.rows.clone(), app.arrangement.clips.len());
        go(&mut app, &mid);
        assert_eq!(app.counted_tracks(), 3, "{name}: no track was made");
        let (made, made_clips) = (app.settings.rows.clone(), app.arrangement.clips.len());
        app.undo();
        assert_eq!(
            (&app.settings.rows, app.arrangement.clips.len()),
            (&rows, clips),
            "{name}: one undo is not the desk before the gesture"
        );
        app.redo();
        assert_eq!((&app.settings.rows, app.arrangement.clips.len()), (&made, made_clips), "{name}: one redo is not the desk after it");
        app.undo();
    }
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(tiers::dir("midi-moment"));
}

/// **`src` with every `#[cfg(test)]` item and every `//` comment blanked**:
/// the production code a scan reads. An item ends at its first line's `;`,
/// or at the line that closes it at its own indent, which is how rustfmt
/// lays out every item here.
fn production_code(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let (mut skipping, mut armed): (Option<String>, Option<String>) = (None, None);
    for line in src.lines() {
        if let Some(close) = skipping.as_deref() {
            if line == close {
                skipping = None;
            }
            out.push('\n');
            continue;
        }
        let t = line.trim_start();
        let indent = &line[..line.len() - t.len()];
        if let Some(at) = armed.take() {
            if t.starts_with("#[") || t.starts_with("//") {
                armed = Some(at);
            } else if !t.ends_with(';') && !t.ends_with(',') {
                skipping = Some(format!("{at}}}"));
            }
            out.push('\n');
            continue;
        }
        if t == "#[cfg(test)]" || t.starts_with("#[cfg(all(test") {
            armed = Some(indent.to_owned());
            out.push('\n');
            continue;
        }
        out.push_str(line.split("//").next().unwrap_or(""));
        out.push('\n');
    }
    out
}

/// **Persistence and drawing are tier-blind** (design 11.2 item 11, 5.6): a
/// desk past the cap opens, saves and draws whole on every tier because
/// nothing that reads, writes or lays it out asks about the licence. The
/// one licence-aware fact a view draws is the trailing add's padlock, read
/// in `app` through `track_cap` (`trailing_plus`), never in these files.
/// The engine half of the same scan is the tangent crate's `konami_e2e`.
#[test]
fn persistence_and_drawing_never_read_the_licence() {
    let files: [(&str, &str); 14] = [
        ("arrangement.rs", include_str!("arrangement.rs")),
        ("settings.rs", include_str!("settings.rs")),
        ("desk_format.rs", include_str!("desk_format.rs")),
        ("desk_model.rs", include_str!("desk_model.rs")),
        ("desk_lift.rs", include_str!("desk_lift.rs")),
        ("desk_legacy.rs", include_str!("desk_legacy.rs")),
        ("desk_id.rs", include_str!("desk_id.rs")),
        ("drumpad.rs", include_str!("drumpad.rs")),
        ("automation.rs", include_str!("automation.rs")),
        ("routing.rs", include_str!("routing.rs")),
        ("mixer_panel.rs", include_str!("mixer_panel.rs")),
        ("arrangement_panel.rs", include_str!("arrangement_panel.rs")),
        ("recorder_panel.rs", include_str!("recorder_panel.rs")),
        ("desk_index.rs", include_str!("desk_index.rs")),
    ];
    let mut bad = Vec::new();
    for (name, src) in files {
        let code = production_code(src);
        for word in code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            let lower = word.to_ascii_lowercase();
            let licence = lower.contains("license")
                || lower.contains("licence")
                || ["is_supporter", "track_cap", "cheated", "licensed", "FREE_TRACKS", "may_add_tracks", "counted_tracks"]
                    .contains(&word);
            if licence {
                bad.push(format!("{name} reads `{word}`"));
            }
        }
    }
    assert!(bad.is_empty(), "persistence or drawing reads the licence:\n  {}", bad.join("\n  "));
    // The scan sees what it is scanning for: a planted read is found.
    let planted = "fn draw() {\n    let cap = app.track_cap();\n}\n#[cfg(test)]\nmod tests {\n    fn x() { app.license.cheated(); }\n}\n";
    let code = production_code(planted);
    assert!(code.contains("track_cap") && !code.contains("cheated"), "the scan's own reading is wrong:\n{code}");
}
