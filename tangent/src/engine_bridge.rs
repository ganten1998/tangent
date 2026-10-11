//! **The one seam between the app and the engine** (C9 in
//! `docs/design/unlimited-tracks.md`): the broker.
//!
//! The app names tracks by identity ([`TrackId`]) and the engine names lanes
//! by where they sit in its spine ([`Slot`]). Nothing else in the host may
//! turn one into the other: this file owns `TrackId -> Slot`, and every road
//! from the desk to the engine passes through it.
//!
//! - [`Bridge::sync`], once a frame: adopt a new engine's legacy lanes (row r
//!   at `Slot(r)`), install a lane for every track that has none and send
//!   home the lanes whose track has gone (two phases, the states first),
//!   publish the desk's shape as ONE plan when an order-deciding input moved
//!   (`DeskPlan`, design 7.6), and store the continuous values (faders,
//!   pans, sends, the keyboard's knobs) into each lane's shared numbers by
//!   slot. A reorder is therefore one plan: no plugin moves, nothing
//!   reloads, no engine round trip.
//! - The translations the host's other roads need: a row's lane, a bay's
//!   engine seat by identity, the desk's clips, MIDI and curves restamped by
//!   `(slot, gen)`, and a slot's row for the meters coming back.
//! - [`full_build`]: a separate OFFLINE engine furnished from the app alone,
//!   through the same sync, the one builder an export, a freeze and the
//!   eligibility probe all come through ([`BuildFor`] says which).
//!
//! **Held to bytes.** The harness below scripts desk edits through a real app
//! into an offline engine and decodes what the engine's plan says back into
//! the words `DesktopApp` pushed before any of this existed
//! (`tests/fixtures/exports/bridge.json`), and the export goldens hash every
//! file a corpus of built-in desks exports (`golden.json`): both recorded
//! against the shim pushes this broker replaced, both unchanged by it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tangent_record::audio::Timebase;
use tangent_ui::app::TangentApp;
use tangent_ui::desk_id::{Node, Rack, SeatBay, TrackId};
use tangent_ui::automation::AutoLane;
use tangent_ui::recorder::{ChannelKind, MidiRoute, StripRef};
use tangent_ui::routing::Target;

use crate::desk_plan::{
    InKey, InLane, InRoute, InSend, LaneFlags, PickId, PlanInputs, RouteEnd, SendFrom, SendTo, Slot, SpecialFlags,
};
use crate::desktop::{
    arm_stem_tap, effect_params_from, export_clip_placement, furnish_mirror_bay, is_instrument_sentinel,
    keys_patch_of, leaf_of, mirror_sampler_samples,
};
use crate::instrument::{
    Engine, LaneKind, LanePart, LaneSpec, PadBank, PadKit, Placed, PlacedMidi, RackAddr, RackOwner, SendCurve,
    SlotAutomation, SlotPlaced, SlotPlacedMidi, LEGACY_LANES,
};

/// **A bay, by the engine's identity for its rack** (`RackAddr`: a lane's by
/// its slot, a strip's by the strip) and the bay in it: what the host asks
/// the engine about a plugin by, through that rack's `HostRack` (design 8,
/// item 8), and what its ledger of loaded bays is keyed by.
pub type EngineBay = (RackAddr, usize);

/// What an export is asked for: the Export dialog's own spec.
pub type ExportSpec = tangent_ui::ports::ProjectExportSpec;

/// **How long a leaving lane is heard fading before it is sent home**
/// (design 7.13, owner call 11): the RETIRING plan's fader slew carries it
/// to silence over about this, so the Remove that follows takes away a lane
/// that is already quiet rather than stepping the master at a block edge.
const RETIRE_RAMP: Duration = Duration::from_millis(10);

/// **The number the engine files the master's curves under**
/// ([`Bridge::slot_automation`]): one no slot reaches, however wide the desk.
pub const MASTER_CURVES: usize = usize::MAX;

/// **Which offline engine [`full_build`] is building.** The three differ in
/// exactly what the comments on each arm say and nowhere else, so a fix to
/// the furnishing reaches all three by construction.
#[derive(Clone, Debug)]
pub enum BuildFor {
    /// A full-speed export: no live input of any kind, the click out, the
    /// stem tap armed as the spec asks.
    Export(ExportSpec),
    /// The eligibility and click-hunt probe: the arm and live-MIDI state
    /// the session actually has (`set_armed(0)` and every lane deaf would
    /// make every lane eligible by construction, which is the one question
    /// the probe asks), plus the knobs in [`Furnish::probe`].
    #[cfg_attr(not(test), allow(dead_code, reason = "the probe is a test harness"))]
    Probe,
    /// A freeze of one track: the export's engine with that track's lane
    /// made pre-fader (unity, centre, every mute and solo off, its own
    /// fader rides left out) and only its wet stem tapped. **By identity**:
    /// the row is looked up through `row_of` when the engine is built, and
    /// the render lands on the track wherever it stands when it finishes.
    Freeze(TrackId),
}

/// The probe's own knobs - `TANGENT_SOLO`, `TANGENT_BYPASS`, `TANGENT_SKIP`,
/// `TANGENT_NO_INSERTS`, `TANGENT_GRAIN` and `TANGENT_PROBE_FANOUT`, read by
/// the harness that drives it. Inert for every other [`BuildFor`].
#[derive(Clone, Debug, Default)]
pub struct ProbeKnobs {
    /// Rows soloed on top of the desk, so one track can be heard through
    /// the same master chain the whole mix goes through.
    pub solo: Vec<usize>,
    /// Bays taken out of circuit on top of the desk's own table, by
    /// identity (`TANGENT_BYPASS=t3-c0-b1,...`).
    pub bypass: Vec<SeatBay>,
    /// Bays left EMPTY: a bypassed bay still holds its plugin, still
    /// reports latency; this is the other question.
    pub skip: Vec<SeatBay>,
    /// No bays at all: a song with analog-modelled units does not render
    /// the same twice, and stripped of them it is arithmetic two builds can
    /// be differenced on.
    pub no_inserts: bool,
    /// Render-ahead in milliseconds, over the app's own setting.
    pub grain_ms: Option<f64>,
    /// Spread the render over the cores, as an export always does.
    pub fanout: bool,
}

/// What [`full_build_with`] is handed beyond the app, and what it hands back
/// beside the engine.
#[derive(Default)]
pub struct Furnish<'a> {
    /// **The live engine and its bridge, whose racks are wearing the knobs
    /// as dialled this second** - see `desk_insert_state`. An export
    /// furnished from the shelf alone rendered every unit dialled in since
    /// the last save at its old settings. `None` reads the shelf. The
    /// bridge is what finds a bay's live seat: the two engines' lanes are
    /// at different slots once anything has been reordered.
    pub live: Option<(&'a Engine, &'a Bridge)>,
    /// Each DX7 row's voice, already read out of its strip's OWN cartridge
    /// by whoever keeps the cartridge cache (the desktop does). A row
    /// missing here reads its cartridge itself, through [`dx7_voice`].
    pub dx7: BTreeMap<usize, crate::dx7::Voice>,
    /// Read only by [`BuildFor::Probe`].
    pub probe: ProbeKnobs,
    /// The bays that would not load, one `"<leaf> (<why>)"` each: a batch
    /// reports ONCE, so the caller says how many and names the first.
    pub refused: Vec<String>,
    /// The clips and frozen files that could not be read, each a sentence.
    pub unread: Vec<String>,
}

/// **An offline engine and the bridge that furnished it.** The bridge is the
/// engine's map: an export reads its stems by slot, and a freeze finds its
/// track's lane through it.
pub struct Built {
    pub engine: Engine,
    pub bridge: Bridge,
}

/// [`full_build_with`] with nothing handed in: no live engine (states from
/// the shelf), every DX7 cartridge read from disk, no probe knobs.
#[cfg_attr(not(test), allow(dead_code, reason = "the host hands its live engine in"))]
pub fn full_build(app: &TangentApp, rate: u32, timebase: Timebase, build: BuildFor) -> Result<Built, String> {
    full_build_with(app, rate, timebase, build, &mut Furnish::default())
}

/// **An offline engine furnished exactly like the live one** (C9's
/// `Bridge::full_build`): desk, racks with the knobs the live racks are
/// wearing, clips, pads, samplers, the instruments' instances and voices,
/// built from the app alone through the same [`Bridge`] the live engine is
/// driven by, so an export, a freeze and the probe run without a
/// `DesktopApp` and can never disagree with the live desk about its shape.
///
/// The transport is left where a fresh engine has it: the caller places and
/// rolls it, because where the render starts is the caller's span.
/// **The Tangent Inserts in circuit**: every bay holding one that is not
/// bypassed (a bypassed bay is passed over, so it renders anywhere). With
/// `row`, only the ones a freeze of that row would render: the row's own
/// racks, and a folder's children's, whose audio the folder's stem sums.
pub fn hw_inserts_in_circuit(app: &TangentApp, row: Option<usize>) -> Vec<SeatBay> {
    let nodes: Option<Vec<Node>> = row.map(|r| {
        std::iter::once(r)
            .chain(app.folder_children(r))
            .filter_map(|ch| app.track_ids().get(ch).copied())
            .map(Node::Track)
            .collect()
    });
    app.plugins()
        .iter()
        .filter(|(_, p)| p.as_str() == crate::builtin_fx::HWINSERT_PATH)
        .map(|(sb, _)| *sb)
        .filter(|sb| !app.bay_bypassed(*sb))
        .filter(|sb| nodes.as_ref().is_none_or(|n| n.contains(&sb.seat.node)))
        .collect()
}

/// **Whether an export can run through the loops in circuit, and how.**
/// `links` is each in-circuit Tangent Insert's (connected, pinging, status
/// line). A loop that is pinging has its send silenced and its wet held for
/// over a second, so an export waits for it rather than writing that in.
///
/// None in circuit: `Ok(false)`, nothing to do. Every one connected:
/// `Ok(true)` when the export asked for full speed and must run in real time
/// instead, `Ok(false)` when it already does. Any one not connected (no
/// input open, a return missing, the varispeed engaged, no jacks chosen):
/// `Err`, because a realtime pass through a loop that is down writes the
/// track dry and says nothing.
pub fn hw_export_verdict(realtime: bool, links: &[(bool, bool, String)]) -> Result<bool, String> {
    if links.is_empty() {
        return Ok(false);
    }
    if let Some((_, _, status)) = links.iter().find(|(up, _, _)| !*up) {
        return Err(format!(
            "export: this project plays through a Tangent Insert whose loop is down: {status} \
             Fix the loop, or bypass the insert, and export again."
        ));
    }
    if links.iter().any(|(_, pinging, _)| *pinging) {
        return Err("export: a Tangent Insert is pinging its loop. Wait for the ping to finish, then export again."
            .to_owned());
    }
    Ok(!realtime)
}

/// **Why an offline render of this project cannot run**, or `None`. A
/// hardware loop plays through the interface in real time; an export can
/// follow it there (the realtime export), a freeze and the probe cannot,
/// and rendering one dry or silent would be a file that lies. `freeze` is
/// the row a freeze would render; `None` is the whole desk.
pub fn hw_refusal_for(app: &TangentApp, freeze: Option<usize>) -> Option<String> {
    let found = hw_inserts_in_circuit(app, freeze);
    if found.is_empty() {
        return None;
    }
    let n = found.len();
    let what = if n == 1 {
        "a Tangent Insert".to_owned()
    } else {
        format!("{n} Tangent Inserts")
    };
    Some(match freeze {
        Some(row) => format!(
            "{} plays through {what} (outboard gear), which cannot render faster than real \
             time, so it cannot be frozen. Bypass the insert to freeze without it, or leave the \
             track live.",
            app.channel_display_name(row)
        ),
        None => format!(
            "This project plays through {what} (outboard gear), which cannot render faster \
             than real time. Export it in real time, through the device."
        ),
    })
}

pub fn full_build_with(
    app: &TangentApp,
    rate: u32,
    timebase: Timebase,
    build: BuildFor,
    ctx: &mut Furnish<'_>,
) -> Result<Built, String> {
    // **The backstop**: every offline road comes through here, and none of
    // them may render a hardware loop dry or silent. The export turns
    // realtime and the freeze refuses before this (`desktop`); the probe and
    // anything added later refuse here.
    let scope = match &build {
        BuildFor::Freeze(id) => Some(app.row_of(*id).ok_or_else(|| "the track to freeze is gone".to_owned())?),
        BuildFor::Export(_) | BuildFor::Probe => None,
    };
    if let Some(why) = hw_refusal_for(app, scope) {
        return Err(why);
    }
    let probe = matches!(build, BuildFor::Probe);
    let freeze = match &build {
        BuildFor::Freeze(id) => Some(app.row_of(*id).ok_or_else(|| "the track to freeze is gone".to_owned())?),
        BuildFor::Export(_) | BuildFor::Probe => None,
    };
    let mut e = Engine::offline(rate, timebase)?;
    // **Spread the lanes over the other cores.**
    //
    // A bounce and a freeze both come through here, and until now both
    // ran on one core: measured on 96 hosted reverbs, 13.11 s of wall
    // for twelve seconds of audio against 13.32 s of CPU. With this it
    // is 3.79 s. The threads are ordinary ones at ordinary priority -
    // see `LanePool::new` for why a bounce must not be real-time - and
    // the fold sums lanes in rank order either way, so the file is the
    // same file. `an_exported_file_is_the_same_whether_or_not_it_forked`
    // holds that against a real render rather than against a bench. The
    // probe forks only when asked, so a real project can be rendered both
    // ways and the two files compared byte for byte.
    if !probe || ctx.probe.fanout {
        e.set_offline_fanout(true);
    }
    // ── the desk, through the one bridge ──
    //
    // Adopts the engine's lanes row for slot, publishes the shape as a plan
    // and stores every fader, pan, send and keyboard knob by slot: the live
    // engine's own road, with the offline differences as a `Shape`.
    let mut bridge = Bridge::new();
    // An export of selected tracks hears those alone, on this plan only.
    if let BuildFor::Export(spec) = &build {
        bridge.set_left_out(export_left_out(app, spec));
    }
    let shape = Shape::Offline {
        freeze,
        probe: probe.then_some(&ctx.probe),
    };
    let synced = bridge.sync_shape(app, &mut e, shape, &mut |_, _, _| Forgotten::new());
    e.set_monitor_on(false);
    e.set_drums(app.drums_gain(), app.drums_pan());
    if probe {
        if !ctx.probe.bypass.is_empty() {
            let mut bays = bridge.engine_bypass(app);
            bays.extend(ctx.probe.bypass.iter().filter_map(|sb| bridge.engine_bay(*sb)));
            e.set_insert_bypass_at(&bays);
        }
        if let Some(ms) = ctx.probe.grain_ms {
            e.set_grain_ms(ms);
        }
    }
    let gains = app.gains();
    e.set_master_gain(gains.master);
    e.set_fx_return(gains.fx_return);
    let [reverb, delay, chorus, phaser, hpf, lpf, gain, limiter] = app.effect_sends();
    e.set_effects(crate::effects::Sends {
        reverb,
        delay,
        chorus,
        phaser,
        hpf,
        lpf,
        gain,
        limiter,
    });
    e.set_effect_params(effect_params_from(app.effect_params()));
    // **And the varispeed, with the same knobs and for the same reason.**
    // In an EXPORT this reaches the offline renderer exactly as it
    // reaches the live one — the owner's words were that it "applies to
    // exports when engaged", and it does so by being one more thing the
    // desk is pushed rather than a special case anywhere.
    e.set_varispeed(app.varispeed_ratio());
    // **The click stays out of the file**, whatever the band says.
    e.set_metronome_enabled(false);
    e.set_tempo(app.tempo_bpm());
    {
        let sig = app.time_signature();
        e.set_meter(u32::from(sig.beats), u32::from(sig.unit));
        // Beside the meter, because it is read with it: how many clicks
        // a beat of that signature carries.
        e.set_metronome_div(app.metronome_div());
    }
    // **The maps, in the export mirror too.** An export whose bar lines
    // are somewhere else than the ones you were listening to is the least
    // visible way this copy of the desk could be wrong.
    e.set_maps(crate::instrument::Maps {
        tempo: app.live_tempo_map(),
        meter: app.live_meter_map(),
    });
    // **And the curves, in the export mirror too.** An export that
    // ignored the automation would be the same class of quiet wrongness
    // as one whose bar lines were somewhere else: it plays, it sounds
    // plausible, and it is not what you were listening to.
    let auto = match freeze {
        Some(row) => {
            // The track's fader stays live, so its rides are not rendered.
            let mut out = tangent_ui::automation::Automation::default();
            for c in app.engine_automation().curves() {
                let fader = c.lane == AutoLane::Row(row)
                    && matches!(
                        c.param,
                        tangent_ui::automation::AutoParam::Gain
                            | tangent_ui::automation::AutoParam::Pan
                            | tangent_ui::automation::AutoParam::Mute
                    );
                if !fader {
                    out.set(c.clone());
                }
            }
            out
        }
        None => app.engine_automation(),
    };
    e.set_automation_slotted(bridge.slot_automation(&auto));
    // **And the sends' own curves** (`Bridge::send_curves`), so a level
    // drawn on a send to a bus plays in the export as it does live. A fresh
    // engine's ring is empty, so they land.
    let _ = e.set_send_curves(bridge.send_curves(&auto, &send_bus_of));
    // Each DX7 lane's sound out of its strip's OWN cartridge: the sync
    // above shipped an instance to every DX7 row (a fresh offline engine
    // has none). An export whose second DX7 track rendered with the first
    // track's bank would be the same class of quiet wrongness as the bar
    // lines being elsewhere.
    for (row, slot) in synced.made_dx7 {
        let v = match ctx.dx7.get(&row) {
            Some(v) => *v,
            None => dx7_voice(app, row),
        };
        e.set_builtin_voice(slot, v);
    }
    // ── every configured bay, saved state included ──
    //
    // **From the DESK, not from the shelf** — see `desk_insert_state`. The
    // mirror used to be furnished from the sidecars alone, so a unit
    // dialled in since the last save rendered at whatever its bay had been
    // wearing then — and when that was one of the settings a unit sits at
    // before anybody dials it, the "processed" stem came out identical to
    // the dry one. Refusals are collected, not said: see `Furnish::refused`.
    if !(probe && ctx.probe.no_inserts) {
        for (sb, path) in desk_bays(app) {
            // Instrument sentinels only — effect sentinels load into the
            // offline engine's racks exactly as they do the live one's.
            if is_instrument_sentinel(&path) || (probe && ctx.probe.skip.contains(&sb)) {
                continue;
            }
            let Some(at) = bridge.engine_bay(sb) else {
                continue;
            };
            // The live engine's own seat for the bay, through ITS bridge: the
            // two engines' lanes sit at different slots once anything has
            // moved. The shelf names the bay by identity.
            let live = ctx.live.and_then(|(le, lb)| lb.engine_bay(sb).map(|seat| (le, seat)));
            if let Err(err) = furnish_mirror_bay(&mut e, live, at, Some(sb), &path) {
                ctx.refused.push(format!("{} ({err})", leaf_of(&path)));
            }
        }
    }
    // ── the clips, at the EXPORT rate - see `export_clip_placement` ──
    if let Some(doc_path) = app.clip_doc() {
        let (placed, midi_placed, unread) =
            export_clip_placement(app.arrangement(), &app.frozen_lanes(), rate, |rel| {
                tangent_ui::arrangement::Arrangement::resolve(&doc_path, rel)
            });
        ctx.unread.extend(unread);
        e.set_clips_slotted(bridge.slot_clips(placed));
        e.set_midi_slotted(bridge.slot_midi(midi_placed));
    }
    // ── the pads, decoded at the export rate ──
    //
    // **Each drum lane's own kit**, attached to its lane by identity. The
    // live path does the same; an export that gave every lane one kit would
    // render a song nobody was listening to.
    for (row, lane) in bridge.lanes_holding(app, tangent_ui::dialogs::DRUMPAD_PATH) {
        let mut kit = PadKit::default();
        for (i, path) in app.drumpad_paths_of(row).iter().enumerate() {
            let Some(path) = path else { continue };
            if let (Ok(c), Some(slot)) = (tangent_record::decode::decode(path, rate), kit.pads.get_mut(i)) {
                *slot = Some(Arc::new(c));
            }
        }
        // An offline engine's op ring is empty here, so the attach lands.
        let _ = e.attach(lane.slot, lane.gen, LanePart::Kit(Box::new(kit)));
        e.set_pad_params(lane.slot, &app.drum_kit_params_of(row));
    }
    // ── the samplers, decoded at the export rate ──
    // Their bays were seated above, file names and all; their audio
    // lives outside the bay and has to be handed over, or a lane the
    // sampler plays renders silent. See `mirror_sampler_samples`.
    mirror_sampler_samples(&mut e, rate, app.kit_doc().as_deref());
    // The chips' answer, through the one place either road turns a spec
    // into the engine's two atomics. See `arm_stem_tap`. A freeze taps
    // its lane wet; the probe taps nothing.
    // **Armed for the lanes the pass writes and no others** (design 7.12:
    // the stem ring is built at arm time, for the armed slots): each stem
    // row's lane, or the frozen track's.
    match &build {
        BuildFor::Export(spec) => {
            let files = stem_files(app, spec);
            let slots: Vec<Slot> = files.iter().filter_map(|(row, _)| bridge.lane(*row)).map(|l| l.slot).collect();
            let own: Vec<Slot> = files
                .iter()
                .filter(|(row, _)| app.is_folder(*row))
                .filter_map(|(row, _)| bridge.lane(*row))
                .map(|l| l.slot)
                .collect();
            crate::desktop::arm_stem_tap_own(&e, spec.stems, &slots, &own, spec.stems_dry);
        }
        BuildFor::Freeze(_) => {
            let slots: Vec<Slot> = freeze.and_then(|row| bridge.lane(row)).map(|l| l.slot).into_iter().collect();
            arm_stem_tap(&e, true, &slots, false);
        }
        BuildFor::Probe => {}
    }
    // Everything sent lands before the first block: an offline engine has
    // no callback to wait for.
    e.settle_offline();
    Ok(Built { engine: e, bridge })
}

/// **The stems an export writes, and what each file is called**, by ROW,
/// in desk order: only the rows with something to SAY - clips, an
/// instrument or any unit in a bay (the DX7, the drumpad, an SD3 following
/// the transport), or a bus, which is a mix of what reaches it. A kind with
/// none of those is an armed input that renders silence offline, and a
/// folder of silent WAVs reads as a bug (a used lane that never sounds is
/// still dropped at the finish, see `desktop::StemWriter`).
///
/// **Never a folder's group.** A folder's group stem is its children
/// summed, so a stems pass that wrote one shipped every child twice; the
/// children are written on their own instead. **A folder with content of
/// its OWN** (`TangentApp::folder_has_own_content`: clips standing on its
/// row, or an instrument kept in a bay from before it became a folder) gets
/// a stem of that alone, named for the folder and cut from its own row
/// before its children are summed into it (`Engine::arm_stems_own`; the
/// caller knows such a row by `is_folder`). A bus and a return are real
/// mixes of their own (what is SENT to them, not what sits under them) and
/// keep their stems.
///
/// **Only the checked tracks** when the export names some
/// (`ProjectExportSpec::tracks`), by identity - named as they would be in
/// an export of everything, so a track's file has one name.
///
/// Each file is named for its TRACK, `<track name>.wav`, through
/// `desktop::stem_file_names`: made safe for every file system, and two
/// tracks of one name are told apart by a number. Both export roads and
/// the export window's rows read their names from here. The caller binds
/// each row to its lane through the bridge at the press (design 7.13: stems
/// bind `(slot, gen)` at the press).
pub fn stem_files(app: &TangentApp, spec: &ExportSpec) -> Vec<(usize, String)> {
    // The one rule, the app's: the Export window's checklist reads it too.
    let rows: Vec<usize> = (0..app.desk_extent()).filter(|&row| app.exports_a_stem(row)).collect();
    let names: Vec<String> = rows
        .iter()
        .map(|&row| {
            let shown = app.channel_display_name(row).trim();
            if shown.is_empty() {
                format!("Track {}", row + 1)
            } else {
                shown.to_owned()
            }
        })
        .collect();
    // Named over every stem the desk has, THEN narrowed to the checked
    // ones, so a track's file is called the same whichever tracks an export
    // takes: the second "Gtr" is `Gtr 2.wav` alone or beside the first.
    let checked = |row: usize| {
        spec.tracks
            .as_ref()
            .is_none_or(|only| app.track_at(row).is_some_and(|id| only.contains(&id)))
    };
    rows.into_iter()
        .zip(crate::desktop::stem_file_names(&names))
        .filter(|(row, _)| checked(*row))
        .collect()
}

/// **Why a stems-only export would write no file at all**, or `None`: none
/// of the tracks it takes has anything to make a stem of. Refused with the
/// sentence rather than run to an empty folder; the Export window refuses
/// the same press before it is sent (`dialogs::resolve_project_export`).
pub fn no_stems_refusal(app: &TangentApp, spec: &ExportSpec) -> Option<&'static str> {
    (spec.stems && !spec.master && stem_files(app, spec).is_empty())
        .then_some("Nothing to export: none of the checked tracks has anything on it to make a stem of.")
}

/// **What an export of selected tracks leaves out of its MIX**
/// ([`export_left_out`]): whole tracks, and folders' own rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LeftOut {
    /// Tracks silent whole, what they make and what they send
    /// ([`crate::desk_plan::LaneFlags::LEFT_OUT`]).
    pub tracks: BTreeSet<TrackId>,
    /// Folders whose OWN ROW is silent while they still carry their
    /// children ([`crate::desk_plan::LaneFlags::ROW_LEFT_OUT`]).
    pub rows: BTreeSet<TrackId>,
}

/// **What an export of selected tracks leaves out of its MIX**, or `None`
/// when it leaves out nothing: no selection (everything), or no master in
/// the pass (a stems-only pass chooses which files it writes and renders
/// the desk whole, so a bus's stem still carries what every track sends
/// it).
///
/// Every unchecked track; for a folder, only its own row and only when it
/// is unchecked (a folder is the road its checked children take to the
/// master, so its group is never left out); never a bus, because a checked
/// track routed into it reaches the master only through it - with the
/// unchecked sources gone, a bus passes exactly the checked tracks' share
/// (which is why the Export window offers no bus row for a mixdown). What
/// is left in still answers the desk's own mutes and solos, so checking
/// every track is exactly "Everything". The desk is never written: the
/// lanes are flagged on the pass's own plan (`Bridge::set_left_out`).
pub fn export_left_out(app: &TangentApp, spec: &ExportSpec) -> Option<LeftOut> {
    let only = spec.tracks.as_ref().filter(|_| spec.master)?;
    let mut out = LeftOut::default();
    for row in 0..app.desk_extent() {
        let Some(id) = app.track_at(row) else { continue };
        if only.contains(&id) || app.is_bus(row) {
            continue;
        }
        if app.is_folder(row) {
            out.rows.insert(id);
        } else {
            out.tracks.insert(id);
        }
    }
    Some(out)
}

/// The patch strip `strip`'s DX7 plays, out of its own cartridge, read from
/// disk: what `DesktopApp::voice_for_strip` answers, without its cache. A
/// bank that will not load, or an empty setting, is the DEFAULT bank -
/// `default.syx` on the SYSEX shelf before the binary's (`default_bank_at`)
/// - and a stale index takes the bank's first patch rather than silence.
pub fn dx7_voice(app: &TangentApp, strip: usize) -> crate::dx7::Voice {
    let (path, patch) = app.dx7_choice_of(strip);
    let shelf = app.sysex_root();
    let cart = if path.is_empty() {
        crate::dx7::staging::default_bank_at(&shelf).0
    } else {
        crate::dx7::Cartridge::load(std::path::Path::new(path))
            .unwrap_or_else(|_| crate::dx7::staging::default_bank_at(&shelf).0)
    };
    cart.voices.get(patch).or_else(|| cart.voices.first()).copied().unwrap_or_default()
}

/// **Which bus a curve's control is the send to** (`AutoParam::SendTo {
/// to }`, key `send:t<id>`), the decoder [`Bridge::send_curves`] is handed
/// by the live push and the export: the one control that names another row.
/// Every other control's curve rides its own lane's curves, the effects
/// return's send (`AutoParam::Send`) among them.
pub fn send_bus_of(param: tangent_ui::automation::AutoParam) -> Option<TrackId> {
    match param {
        tangent_ui::automation::AutoParam::SendTo { to } => Some(to),
        _ => None,
    }
}

/// **Every bay the desk seats, by identity**: the app's own map (C11's
/// `plugins`), read directly. A track's bays are wherever the track is, so
/// a reorder changes nothing here; a bay whose track no row holds is not in
/// the map at all (a removal takes its track's bays with it).
pub fn desk_bays(app: &TangentApp) -> BTreeMap<SeatBay, String> {
    app.plugins().iter().filter(|(_, p)| !p.is_empty()).map(|(sb, p)| (*sb, p.clone())).collect()
}

/// **The bay the frozen map names `(seat, bay)`, by identity**, under row
/// ids `ids` (`desk_legacy::bay_at`): the tests' way to name their legacy
/// fixtures' bays. No road of the host's holds a seat any more
/// (W7-slot-gaps): the frozen map has none for a track past its
/// thirty-second row.
#[cfg(test)]
pub fn seat_bay_at(ids: &[TrackId], seat: usize, bay: usize) -> Option<SeatBay> {
    tangent_ui::desk_legacy::bay_at(ids, seat, bay)
}

/// **Where the frozen map keeps a bay**, `(seat, bay)`, under row ids
/// `ids`: [`seat_bay_at`]'s inverse, for the tests. `None` for a track no
/// row holds.
#[cfg(test)]
pub fn app_seat_of(ids: &[TrackId], sb: SeatBay) -> Option<(usize, usize)> {
    tangent_ui::desk_legacy::seat_of(ids, sb)
}

/// **Carry a gesture's bay relabels on the host** (C11's
/// `take_bay_relabels`): each plugin that moved from one bay to another (a
/// drum pad swap, an FX drag or swap, their undo and redo) takes its
/// settings with it.
///
/// The app has already moved the NAMES: what stood at `from` is wanted at
/// `to`. The engine still holds the old instances, and the reconcile that
/// follows rebuilds each moved bay from its own state file, so what this
/// owes is that every destination's file is the moved plugin's own state
/// before that rebuild reads it. **Read every source first, then write
/// every destination**, because a swap's two files are each the other's
/// source.
///
/// **A source is the moved plugin's NEWEST word**, in the order
/// `shelf::incoming_state` ranks them, with the playing instance above
/// both: the instance at `from` when it is that plugin (a unit somebody has
/// been turning has not written its file since it loaded), else what the
/// displaced shelf kept of that plugin at `from` (what it was playing when
/// the reconcile last took it out of that bay, newer than the file by
/// construction), else the file on `shelf` (owner-checked). The shelf is
/// what a step back across more than one moment needs: a plugin swapped
/// into a bay, dialled there and then replaced has left the bay, so it is
/// not playing, and its file still holds the swap's settings, while the
/// dial is on the shelf. Read from the file, an undo across the swap and the
/// replace in one go brought it home without the dial.
///
/// Answers each destination written and the bundle written there: the
/// caller lets go of any displaced copy for it (`incoming_state` must not
/// prefer an older one) and marks the bay to rebuild even when its NAME did
/// not change (two bays holding the same unit trade settings, not names).
pub(crate) fn carry_relabels(
    app: &TangentApp,
    bridge: &Bridge,
    engine: Option<&Engine>,
    displaced: &crate::shelf::DisplacedShelf,
    relabels: &[tangent_ui::desk_id::BayRelabel],
    shelf: &std::path::Path,
) -> Vec<(SeatBay, String)> {
    let moving: Vec<(SeatBay, String, Vec<u8>)> = relabels
        .iter()
        .filter_map(|r| {
            let bundle = app.plugin(r.to)?.to_owned();
            let live = engine.zip(bridge.engine_bay(r.from)).and_then(|(e, at)| {
                let playing = e.insert_at(at.0, at.1).and_then(|l| l.bundle.to_str()) == Some(bundle.as_str());
                playing.then(|| e.save_insert_state_at(at.0, at.1)).flatten()
            });
            let state = live.or_else(|| {
                crate::shelf::incoming_state(displaced, &(r.from, bundle.clone()), || {
                    crate::shelf::read_state(shelf, r.from, &bundle)
                })
            })?;
            Some((r.to, bundle, state))
        })
        .collect();
    moving
        .into_iter()
        .filter_map(|(to, bundle, state)| crate::shelf::write_state_in(shelf, to, &bundle, &state).then_some((to, bundle)))
        .collect()
}

/// **The engine's name for a bay of the lane at `slot`**: the lane's rack
/// by identity (`RackAddr::of`). A track's bays are reached through its
/// lane's slot and never through its row, so a reorder renames nothing, and
/// no seat map is consulted: a lane at any slot names its racks the same way.
pub fn lane_bay(slot: Slot, rack: Rack, bay: u8) -> Option<EngineBay> {
    Some((RackAddr::of(RackOwner::Lane(slot), rack)?, usize::from(bay)))
}

/// The engine's name for a bay of a special's rack (the drum bus, the
/// click, the return, the master), the same in every engine.
fn special_bay(node: Node, rack: Rack, bay: u8) -> Option<EngineBay> {
    let owner = match node {
        Node::Drums => RackOwner::Drums,
        Node::Click => RackOwner::Click,
        Node::Fx => RackOwner::Fx,
        Node::Master => RackOwner::Master,
        Node::Track(_) => return None,
    };
    Some((RackAddr::of(owner, rack)?, usize::from(bay)))
}

/// **Whether row `row` is armed**: the row's own arm (`TangentApp::is_armed`),
/// as many rows as the desk has. The host asks a row and never builds or
/// reads a mask of its own; the arm word it once read stopped at bit 31.
pub fn row_armed(app: &TangentApp, row: usize) -> bool {
    app.is_armed(row)
}

/// **The plugin path in a bay of a track, by identity**, read off the app's
/// map: the host names a track's bay and never works out a seat.
pub fn track_bay(app: &TangentApp, id: TrackId, rack: Rack, bay: u8) -> Option<&str> {
    app.plugin(SeatBay::new(Node::Track(id), rack, bay))
}

/// [`track_bay`] for the track on row `row`'s first bay: what holds its
/// instrument.
pub fn row_instrument(app: &TangentApp, row: usize) -> Option<&str> {
    track_bay(app, app.track_at(row)?, Rack::Chain(0), 0)
}

/// **Where in its chain a track holds `sentinel`**, if it does: the first of
/// its six bays (its own rack's three, then its extra rack's) holding it, as
/// a chain position `0..6`. A track's instruments are wherever its bays put
/// them, not only in the first: two or more on one track all play (see
/// `instrument::Layer` for the rule), so every question of the form "does
/// this track carry the DX7, the keyboard, the drumpad" is this one.
pub fn track_holds(app: &TangentApp, id: TrackId, sentinel: &str) -> Option<u8> {
    tangent_ui::desk_id::chain_bays()
        .position(|(rack, bay)| track_bay(app, id, rack, bay) == Some(sentinel))
        .and_then(|at| u8::try_from(at).ok())
}

/// [`track_holds`] for the track on row `row`, as a yes or no.
pub fn row_holds(app: &TangentApp, row: usize, sentinel: &str) -> bool {
    app.track_at(row).is_some_and(|id| track_holds(app, id, sentinel).is_some())
}

/// **The plugin path in a bay, by identity**, off the app's map.
pub fn bay_path(app: &TangentApp, sb: SeatBay) -> Option<&str> {
    app.plugin(sb)
}

/// **What the host does with a lane whose track has gone**, called by the
/// bridge while the lane's plugins are alive: the engine, the track and its
/// lane. The track's bays are named by its identity, so no row is needed.
/// It answers what it let go of ([`Forgotten`]), which the bridge keeps on
/// the leaving lane.
pub type Retire<'a> = dyn FnMut(&Engine, TrackId, LaneAt) -> Forgotten + 'a;

/// **What the host let go of when a lane started home**: its record of the
/// lane's loaded bays (the ledger's entries, by the engine's names). Kept on
/// the leaving lane and handed back in [`Synced::reclaimed`] if the track
/// returns before the lane is sent home, which takes back the same lane with
/// every plugin still seated in it (design 7.13: "nothing was sent home, so
/// nothing reloads"). Opaque here: only the host reads it.
pub type Forgotten = Vec<(EngineBay, String)>;

/// One track's lane in the engine: where it sits, and which lane it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaneAt {
    pub slot: Slot,
    pub gen: u32,
}

/// A lane the bridge holds for a track: the lane, and whether its pad bank
/// is attached (every legacy lane is built with one; a lane installed for a
/// track that is not a drum track is not, until it becomes one).
#[derive(Clone, Copy, Debug)]
struct Held {
    at: LaneAt,
    pads: bool,
}

/// **A lane on its way home** (design 7.13): first heard fading under a
/// RETIRING plan, then dropped from the plan and sent home whole. Its track
/// waits for it if it comes back before it is home.
#[derive(Clone, Debug)]
struct Leaving {
    id: TrackId,
    at: LaneAt,
    pads: bool,
    /// When the track went, for the ramp.
    since: Instant,
    /// The plan that first marked it RETIRING, once one was published.
    retiring_in: Option<u64>,
    /// The Remove has been sent: the lane is in no plan, and the slot is
    /// the engine's until it comes home.
    sent: bool,
    /// What the host forgot of the lane's bays as it started home, handed
    /// back if its track takes it back before it is sent.
    forgotten: Forgotten,
}

/// **How a sync builds the plan.** The live engine hears the session's arm
/// and live MIDI and the record's punches; an offline engine hears nothing
/// live, plays its built-in instruments on any lane, and may neutralise one
/// row (a freeze) or solo some (the probe).
#[derive(Clone, Copy)]
enum Shape<'a> {
    Live,
    Offline {
        /// The row a freeze makes pre-fader: unity, centre, every mute and
        /// solo on the desk off.
        freeze: Option<usize>,
        /// The probe's knobs: the session's arm and live MIDI, and its
        /// solos on top of the desk's.
        probe: Option<&'a ProbeKnobs>,
    },
}

impl Shape<'_> {
    /// Whether this engine is told the session's live input (arm, live
    /// MIDI): the live engine and the probe, never an export or a freeze.
    fn hears_live(&self) -> bool {
        match self {
            Shape::Live => true,
            Shape::Offline { probe, .. } => probe.is_some(),
        }
    }

    fn freeze(&self) -> Option<usize> {
        match self {
            Shape::Live => None,
            Shape::Offline { freeze, .. } => *freeze,
        }
    }
}

/// How many plans, installs, removes and attaches a bridge has sent: the
/// counts the reorder test reads ("one plan, nothing else").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BridgeStats {
    pub plans: u64,
    pub installs: u64,
    pub removes: u64,
    pub attaches: u64,
}

/// **The host's half of the seam**: which lane is whose, what the engine was
/// last told, and the one place it is told the desk's shape.
#[derive(Default)]
pub struct Bridge {
    /// Which engine the map describes. Bumped by [`Self::engine_replaced`],
    /// so a fresh engine is adopted rather than mistaken for the old one.
    epoch: u64,
    /// The epoch `lanes` was adopted at.
    adopted: Option<u64>,
    /// **`TrackId -> Slot`**: every track's lane.
    lanes: BTreeMap<TrackId, Held>,
    leaving: Vec<Leaving>,
    /// This frame's rows, in order: each row's track and its lane.
    rows: Vec<(TrackId, Option<LaneAt>)>,
    /// **The tracks whose lane was a drum lane at the last sync**: the
    /// drumpad in the track's first bay and a lane to play it on. A track
    /// that joins the set has a lane that has just become a drum lane, and
    /// is owed its kit ([`Synced::drummed`]).
    drums: BTreeSet<TrackId>,
    /// The inputs of the last plan the engine took, for the change test.
    published: Option<PlanInputs>,
    /// The bypass table last stored, by the engine's names.
    bypass: Option<Vec<EngineBay>>,
    /// Where an arrangement record is replacing each row's schedule, in
    /// frames, handed in by the host each frame (it owns the record).
    punches: Vec<Option<u64>>,
    stats: BridgeStats,
    /// **Which input each audio track takes, by identity**: see
    /// [`PickTable`] and [`Self::sync_inputs`]. Kept across a new engine,
    /// because a pick is the track's and not the engine's.
    picks: PickTable,
    /// A test's ask to run an offline engine's removals through the live
    /// two-phase road (RETIRING, then home), which an offline engine skips.
    #[cfg(test)]
    staged: bool,
    /// **The tracks an export of selected tracks leaves out of its mix**
    /// ([`export_left_out`]), or `None` for every other plan. Set on the
    /// offline mirror when it is built and on the live bridge for the
    /// length of a realtime pass; each of their lanes is published
    /// [`LaneFlags::LEFT_OUT`] and the desk's solos stand down while it is
    /// set. The desk itself, its mutes and solos and its undo, is never
    /// written.
    left_out: Option<LeftOut>,
}

/// What a [`Bridge::sync`] did that the host has to follow.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Synced {
    /// The engine was adopted from nothing - a fresh engine forgets the
    /// pads' knobs with everything else, so whatever the host caches as
    /// "what this engine was told" has to forget too.
    pub primed: bool,
    /// **The slots whose lane was installed or sent home.** Everything the
    /// host sends stamped `(slot, gen)` (clips, MIDI, curves, kits) is owed
    /// again, so a new lane hears its own, and whatever the host keeps per
    /// slot outside the lane (a sampler's file) is owed for these.
    pub moved: Vec<Slot>,
    /// The DX7 rows whose instance was just made, with their lanes: each
    /// wants its strip's own voice.
    pub made_dx7: Vec<(usize, Slot)>,
    /// **What the host forgot of each lane its track took back this frame**
    /// ([`Forgotten`]): the lane never left, its plugins are still seated,
    /// and the host's record of them is owed back, or the reconcile reads
    /// every bay of it as wanted and not loaded and reloads them in place.
    pub reclaimed: Vec<Forgotten>,
    /// **The lanes that have just become drum lanes**: the drumpad arrived
    /// in their track's first bay (a pick, a paste, a swap, an undo: every
    /// road), or the lane arrived under a track that holds it. Each is owed
    /// its track's kit (design 7.13: "Attach(Pads) then Attach(Kit)"). A
    /// lane's kit goes to it only while it holds the drumpad, so one that
    /// took it now holds no kit, or one older than its track's.
    pub drummed: Vec<Slot>,
}

impl Synced {
    /// **Whether this frame owes the drum lanes their kits again**: a lane
    /// came or went (`moved`), or one became a drum lane (`drummed`). The
    /// host's other two reasons, a kit edited and a delivery that did not
    /// all land, are its own (`take_drumpad_dirty`, `pads_retry`). A track
    /// that took the drumpad was owed none: no lane moved, no kit was
    /// edited, the last delivery had landed whole, and its pads were silent
    /// until another kit edit or a reopen.
    pub fn owes_kits(&self) -> bool {
        !self.moved.is_empty() || !self.drummed.is_empty()
    }
}

impl Bridge {
    pub fn new() -> Self {
        Self::default()
    }

    /// **The tracks an export leaves out of its mix**, from here until it is
    /// set again ([`export_left_out`]): the next plan publishes each of
    /// their lanes `LEFT_OUT` and the desk's solos stand down, and `None`
    /// puts the desk back exactly as it plays. Nothing of the app's is
    /// touched.
    pub fn set_left_out(&mut self, out: Option<LeftOut>) {
        self.left_out = out;
    }

    /// A new engine is seated: everything it is told from here on is told
    /// from scratch, and its lanes are adopted afresh. Called wherever the
    /// host replaces its engine.
    pub fn engine_replaced(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    /// **The live punch, by row**: while the arrangement records over a
    /// lane, everything scheduled on it from the record start is being
    /// replaced and must not sound. Handed in every frame like the arms;
    /// cleared the same way when the take ends and the carve is real.
    pub fn punch_at(&mut self, punches: Vec<Option<u64>>) {
        self.punches = punches;
    }

    /// What this bridge has sent so far.
    #[cfg_attr(not(test), allow(dead_code, reason = "the reorder and removal tests read it"))]
    pub fn stats(&self) -> BridgeStats {
        self.stats
    }

    /// The inputs of the last plan this bridge published.
    #[cfg(test)]
    pub(crate) fn published_for_test(&self) -> Option<&PlanInputs> {
        self.published.as_ref()
    }

    /// Run removals on an offline engine through the live two-phase road.
    #[cfg(test)]
    pub(crate) fn stage_removals_for_test(&mut self) {
        self.staged = true;
    }

    /// **Row `row`'s lane**, as of the last sync.
    pub fn lane(&self, row: usize) -> Option<LaneAt> {
        self.rows.get(row).and_then(|(_, at)| *at)
    }

    /// **A track's lane**, wherever its row is.
    pub fn lane_of(&self, id: TrackId) -> Option<LaneAt> {
        self.lanes.get(&id).map(|h| h.at)
    }

    /// **The row whose lane is at `slot`**, as of the last sync: how a
    /// reading the engine makes by slot (a meter, a sampler's file) reaches
    /// the row the app draws it on.
    pub fn row_of_slot(&self, slot: Slot) -> Option<usize> {
        self.rows.iter().position(|(_, at)| at.is_some_and(|a| a.slot == slot))
    }

    /// **The engine's name for a bay, by identity**: a track's through its
    /// lane, a special's as it always was. `None` for a track with no lane
    /// (gone, or waiting for one).
    ///
    /// **And `None` for a pad's bay while its track's lane has no pad bank**
    /// (`Held::pads`: a lane installed for a track whose first bay was not
    /// the drumpad). The bay is not wanted anywhere until the bank is sent
    /// (`attach_pads`), and from that frame the reconcile finds it wanted
    /// and not loaded, and seats it. It used to be wanted at once: the load
    /// went to a lane with nowhere to put a pad's rack past slot 31, the
    /// plugin went home with the rack, and the ledger still said it was
    /// seated, so when the drumpad came back the kit played that pad dry,
    /// live, for good, while an export built the lane with its bank and
    /// played it through its effect (W7-slot-gaps). A pad of a lane of the
    /// frozen map has its rack in the frozen bank either way; it waits for
    /// the bank too, as it can be heard through nothing else.
    pub fn engine_bay(&self, sb: SeatBay) -> Option<EngineBay> {
        match sb.seat.node {
            Node::Track(id) => {
                let h = self.lanes.get(&id)?;
                if matches!(sb.seat.rack, Rack::Pad(_)) && !h.pads {
                    return None;
                }
                lane_bay(h.at.slot, sb.seat.rack, sb.bay)
            }
            node => special_bay(node, sb.seat.rack, sb.bay),
        }
    }

    /// **The engine's name for the frozen map's `(seat, bay)`** under the
    /// app's rows now, for the tests' legacy fixtures (see [`seat_bay_at`]).
    #[cfg(test)]
    pub fn engine_seat(&self, app: &TangentApp, seat: usize, bay: usize) -> Option<EngineBay> {
        self.engine_bay(seat_bay_at(&app.track_ids(), seat, bay)?)
    }

    /// **The bay the engine names `at`, by identity**: the inverse of
    /// [`Self::engine_bay`], for what the engine reports by its racks. A
    /// lane's rack is its track's wherever the row is; one whose slot no
    /// track holds (a lane on its way home) is nobody's. A pad's rack is
    /// named whether or not its lane has its bank yet: one with none holds
    /// nothing to report.
    pub fn bay_at_engine(&self, at: EngineBay) -> Option<SeatBay> {
        let (addr, bay) = at;
        let node = match addr.owner {
            RackOwner::Lane(slot) => Node::Track(self.lanes.iter().find(|(_, h)| h.at.slot == slot).map(|(id, _)| *id)?),
            RackOwner::Drums => Node::Drums,
            RackOwner::Click => Node::Click,
            RackOwner::Fx => Node::Fx,
            RackOwner::Master => Node::Master,
        };
        let sb = SeatBay::new(node, addr.desk_rack()?, u8::try_from(bay).ok()?);
        sb.is_valid().then_some(sb)
    }

    /// Every row whose track holds `sentinel` in any of its bays
    /// ([`track_holds`]), with its lane: the drum, DX7 and keyboard lanes.
    pub fn lanes_holding(&self, app: &TangentApp, sentinel: &str) -> Vec<(usize, LaneAt)> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, (id, _))| track_holds(app, *id, sentinel).is_some())
            .filter_map(|(row, (_, at))| at.map(|a| (row, a)))
            .collect()
    }

    /// **The desk's audio clips, stamped with their lanes**: each clip's row
    /// becomes its track's `(slot, gen)`, so a reorder after the push plays
    /// every clip on its own track and a clip for a lane that has gone plays
    /// on nobody (design 7.5).
    pub fn slot_clips(&self, placed: Vec<Placed>) -> Vec<SlotPlaced> {
        placed
            .into_iter()
            .filter_map(|p| {
                let at = self.lane(p.strip)?;
                Some(SlotPlaced { slot: at.slot, gen: at.gen, placed: p })
            })
            .collect()
    }

    /// The desk's MIDI clips, the same way.
    pub fn slot_midi(&self, placed: Vec<PlacedMidi>) -> Vec<SlotPlacedMidi> {
        placed
            .into_iter()
            .filter_map(|p| {
                let at = self.lane(p.strip)?;
                Some(SlotPlacedMidi { slot: at.slot, gen: at.gen, placed: p })
            })
            .collect()
    }

    /// **The curves, by lane**: a row's curve moves to its track's slot and
    /// plays only on that lane's generation; the master's stays the master's.
    ///
    /// **The engine files a curve under a number**: a lane's under its slot
    /// (`AutoLane::Row(slot)`), the master's under [`MASTER_CURVES`], a
    /// number no slot reaches, and it says which with the list
    /// (`SlotAutomation::master`). The master was the lane one past the
    /// thirty-two channels while slots stopped there; a slot is any number
    /// now, so the master's is one no slot can be.
    pub fn slot_automation(&self, auto: &tangent_ui::automation::Automation) -> SlotAutomation {
        let mut gens = Vec::new();
        let mut curves = Vec::new();
        for c in auto.curves() {
            let mut moved = c.clone();
            match c.lane {
                AutoLane::Master => moved.lane = AutoLane::Row(MASTER_CURVES),
                AutoLane::Row(row) => {
                    let Some(at) = self.lane(row) else { continue };
                    moved.lane = AutoLane::Row(at.slot.0 as usize);
                    gens.push((at.slot, at.gen));
                }
            }
            curves.push(moved);
        }
        gens.sort_unstable_by_key(|(s, _)| *s);
        gens.dedup();
        SlotAutomation { curves: tangent_ui::automation::Automation::from_curves(curves), gens, master: MASTER_CURVES }
    }

    /// **The curves that automate a strip's send to a bus, as the plan's
    /// send curves** (design 9: send automation works per return): each
    /// such curve on a row becomes a [`SendCurve`] from that row's lane to
    /// the bus's lane, stamped with the source lane's generation, so the
    /// engine finds its send by its two ends in whichever plan plays
    /// (`Engine::set_send_curves`) and a curve drawn for a track that has
    /// gone plays on nobody. `bus_of` reads which bus a curve's control
    /// names ([`send_bus_of`], `AutoParam::SendTo { to }`); a curve naming
    /// none, or a bus with no lane, is no send curve. A send to Return 0 is
    /// not here: `AutoParam::Send` stays the Fx send, on the lane's own
    /// curves.
    ///
    /// **Each names the POST-fader send** (`SendCurve::pre` false): the
    /// app's curve on a send to a bus is that send's alone, and a pre-fader
    /// send is a cue feed with no curve (`AutoParam::SendTo`). A strip a
    /// file has sending to one bus from both sides of its fader keeps its
    /// cue where it was set while the curve rides the other.
    pub fn send_curves(
        &self,
        auto: &tangent_ui::automation::Automation,
        bus_of: &dyn Fn(tangent_ui::automation::AutoParam) -> Option<TrackId>,
    ) -> Vec<SendCurve> {
        auto.curves()
            .iter()
            .filter_map(|c| {
                let AutoLane::Row(row) = c.lane else { return None };
                let from = self.lane(row)?;
                let to = self.lane_of(bus_of(c.param)?)?;
                Some(SendCurve {
                    from: SendFrom::Lane(from.slot),
                    to: SendTo::Lane(to.slot),
                    pre: false,
                    gen: from.gen,
                    curve: c.clone(),
                })
            })
            .collect()
    }

    /// **The bypassed bays, by the engine's names**: the desk keeps them by
    /// identity (C11's `bypass`), the engine by its lanes' racks.
    pub fn engine_bypass(&self, app: &TangentApp) -> Vec<EngineBay> {
        app.bypass().iter().filter_map(|sb| self.engine_bay(*sb)).collect()
    }

    /// **A node's lane**, for a route or a key's end: a track's through its
    /// lane; a special has none (no route or key reaches one before buses
    /// do).
    fn node_lane(&self, n: Node) -> Option<LaneAt> {
        self.lane_of(n.track()?)
    }

    /// **The desk's wiring, read by identity** (C9's `read_desk`, this
    /// lane's half of the seam): the routing table and the sidechain keys,
    /// each end turned into the engine's names through this bridge's lanes.
    /// No row is read and no seat worked out: a route names its tracks and
    /// a key its bay and its source by identity, so a reorder hands the
    /// engine the same wiring, and an end whose track has no lane (gone, or
    /// waiting for one) wires nothing.
    ///
    /// **The routing table is pruned against what the device actually has**:
    /// an interface swapped for a smaller one leaves routes pointing at
    /// jacks that are gone, and a fader that moves and does nothing is
    /// worse than a route that admitted it went away. The pruned copy is
    /// never written back to the desk, so a route survives an interface
    /// being unplugged and replugged. **In an EXPORT this drops the
    /// hardware routes and keeps the sends**, which is exactly right: a
    /// rendered file has no jacks, but a channel sent through another
    /// channel's reverb is part of the mix and has to be in the file.
    pub fn read_desk(&self, app: &TangentApp, outs: usize) -> (Vec<InRoute>, Vec<InKey>) {
        let mut table = app.routes().clone();
        table.prune(outs, |n| match n {
            Node::Track(id) => self.rows.iter().any(|(t, _)| *t == id),
            _ => true,
        });
        let routes = table
            .0
            .iter()
            .filter_map(|r| {
                let from = self.node_lane(r.from)?.slot;
                let to = match r.target {
                    Target::Node(n) => RouteEnd::Lane(self.node_lane(n)?.slot),
                    Target::Hardware { first, stereo } => RouteEnd::Hardware { first: u16::try_from(first).ok()?, stereo },
                };
                Some(InRoute { from, to, gain: r.gain })
            })
            .collect();
        let keys = app
            .insert_keys()
            .0
            .iter()
            .filter_map(|k| {
                let from = self.node_lane(k.from)?.slot;
                let (addr, bay) = self.engine_bay(k.seat)?;
                Some(InKey { seat: addr.plan_seat()?, bay: u8::try_from(bay).ok()?, from })
            })
            .collect();
        (routes, keys)
    }

    /// **Push the desk into the live engine**, once a frame. See
    /// [`Self::sync_with`]; this one has no states to save for a leaving
    /// lane, which only a host with a shelf has.
    #[cfg_attr(not(test), allow(dead_code, reason = "the host passes its shelf"))]
    pub fn sync(&mut self, app: &TangentApp, e: &mut Engine) -> Synced {
        self.sync_shape(app, e, Shape::Live, &mut |_, _, _| Forgotten::new())
    }

    /// **Push the desk into the live engine**, once a frame: adopt, install
    /// and send home by identity, publish the shape when it moved, store the
    /// continuous values. `on_retire` is called for each lane the frame its
    /// track goes, while every plugin in it is still alive and before
    /// anything is sent home, so the host can write every bay's state to the
    /// shelf first (design 7.13: "the lane comes home whole ... the host
    /// writes each bay's state to its pstate file on the global shelf, then
    /// drops it").
    pub fn sync_with(&mut self, app: &TangentApp, e: &mut Engine, on_retire: &mut Retire<'_>) -> Synced {
        self.sync_shape(app, e, Shape::Live, on_retire)
    }

    fn sync_shape(&mut self, app: &TangentApp, e: &mut Engine, shape: Shape<'_>, on_retire: &mut Retire<'_>) -> Synced {
        let mut synced = Synced { primed: self.adopt(app, e), ..Synced::default() };
        let ids: Vec<TrackId> = app.track_ids();
        // **A lane for every row**, as many as the desk has. A vacant row
        // keeps one too, for now: the words every engine is told were pinned
        // with a lane per row (`bridge.json`), and section 4's "a vacant row
        // has no engine lane" is a change to them of its own.
        let laned: Vec<(usize, TrackId)> = ids.iter().copied().enumerate().collect();
        let wanted: BTreeSet<TrackId> = ids.iter().copied().collect();
        // ── the lanes, by identity ──
        // **A track that has gone starts its lane home**, and the host writes
        // its bays down NOW, while every plugin in the lane is alive: the
        // lane comes home whole and its plugins drop with it (design 7.13).
        let gone: Vec<TrackId> = self.lanes.keys().filter(|id| !wanted.contains(id)).copied().collect();
        for id in gone {
            if let Some(h) = self.lanes.remove(&id) {
                let forgotten = on_retire(e, id, h.at);
                self.leaving.push(Leaving {
                    id,
                    at: h.at,
                    pads: h.pads,
                    since: Instant::now(),
                    retiring_in: None,
                    sent: false,
                    forgotten,
                });
            }
        }
        // **A track back before its lane left takes the lane back**: nothing
        // was sent home, so nothing reloads, and what the host forgot of its
        // bays goes back to the host with it. One whose Remove went waits for
        // its lane to be home, then takes a fresh one like any new track.
        let reclaimed = &mut synced.reclaimed;
        self.leaving.retain_mut(|l| {
            if !l.sent && wanted.contains(&l.id) {
                self.lanes.insert(l.id, Held { at: l.at, pads: l.pads });
                reclaimed.push(std::mem::take(&mut l.forgotten));
                return false;
            }
            !(l.sent && e.slot_free(l.at.slot))
        });
        synced.moved.extend(self.install(app, e, &laned));
        self.attach_pads(app, e, &laned);
        synced.drummed = self.note_drums(app, &laned);
        self.rows = ids.iter().map(|id| (*id, self.lanes.get(id).map(|h| h.at))).collect();
        // ── the lanes due home: faded out under a plan the callback took,
        // or at once on an engine nobody is listening to ──
        #[cfg(test)]
        let offline = e.is_offline() && !self.staged;
        #[cfg(not(test))]
        let offline = e.is_offline();
        let playing = e.plan_playing();
        let due: Vec<usize> = (0..self.leaving.len())
            .filter(|&i| {
                let l = &self.leaving[i];
                !l.sent
                    && (offline
                        || l.retiring_in.is_some_and(|g| playing >= g) && l.since.elapsed() >= RETIRE_RAMP)
            })
            .collect();
        // ── the shape ──
        let outs = e.output().channels as usize;
        let inputs = self.plan_inputs(app, outs, shape, &due);
        let published = self.publish(e, inputs);
        if let Published::New(gen) = published {
            for l in self.leaving.iter_mut().filter(|l| !l.sent && l.retiring_in.is_none()) {
                l.retiring_in = Some(gen);
            }
        }
        // ── home: only once the plan without them is in the ring ──
        //
        // A plan that could not be sent leaves them for next frame: the plan
        // playing may still name them, and the Remove waits behind the plan
        // that lets them go.
        if published != Published::Failed && !due.is_empty() {
            let slots: Vec<Slot> = due.iter().map(|&i| self.leaving[i].at.slot).collect();
            if e.remove_lanes(&slots).is_ok() {
                for &i in &due {
                    self.leaving[i].sent = true;
                }
                self.stats.removes += slots.len() as u64;
                synced.moved.extend(slots);
            }
        }
        // ── the continuous values, every frame, by slot ──
        self.store_levels(app, e, shape);
        // **What is bypassed, by the engine's names**, stored only when it
        // moved: `set_insert_bypass_at` reads before it writes, but the
        // translation is a walk too, and the table changes when somebody
        // clicks.
        let bypass = self.engine_bypass(app);
        if self.bypass.as_ref() != Some(&bypass) {
            e.set_insert_bypass_at(&bypass);
            self.bypass = Some(bypass);
        }
        // **How far ahead a track's effects are rendered.** One store of a
        // frame count, every frame and not behind any gate: it used to ride
        // inside the table publish, so a render-ahead change with nothing
        // else edited never reached a running engine. See
        // `Settings::render_ahead_ms` for what it trades.
        e.set_grain_ms(app.render_ahead_ms());
        // **The instruments' instances, by slot.** A DX7 or keyboard lane
        // gets its instance shipped whole the frame it becomes one; frozen
        // lanes keep theirs (the host keeps them, so thawing is instant), and
        // their strip flags in the plan stand them down instead.
        let live_rule = matches!(shape, Shape::Live);
        // **And where each enters its chain**, restated every frame: a
        // level, so a bay moved, emptied or refilled is followed without a
        // diff to keep. Zero (the head) for a track holding neither.
        for (id, at) in &self.rows {
            if let Some(at) = at {
                let pos = |sentinel: &str| track_holds(app, *id, sentinel).unwrap_or(0);
                e.set_inlets(
                    at.slot,
                    pos(tangent_ui::dialogs::BUILTIN_PATH),
                    pos(tangent_ui::dialogs::KEYS_PATH),
                );
            }
        }
        let dx7: Vec<Slot> = self.instrument_lanes(app, tangent_ui::dialogs::BUILTIN_PATH, live_rule);
        let keys: Vec<Slot> = self.instrument_lanes(app, tangent_ui::dialogs::KEYS_PATH, live_rule);
        for slot in e.sync_instruments(&dx7, &keys) {
            if let Some(row) = self.row_of_slot(slot) {
                synced.made_dx7.push((row, slot));
            }
        }
        if offline {
            e.settle_offline();
        }
        synced
    }

    /// **A new engine's lanes, adopted**: row r at `Slot(r)`, the legacy
    /// desk every engine is built with (design 3: "a desk loaded from disk
    /// installs row r at Slot(r)"), so slots equal today's lane indices and
    /// the goldens stay bit-identical. Answers whether it adopted.
    fn adopt(&mut self, app: &TangentApp, e: &Engine) -> bool {
        if self.adopted == Some(self.epoch) {
            return false;
        }
        self.lanes.clear();
        self.leaving.clear();
        self.published = None;
        self.bypass = None;
        // A fresh engine holds no kit: every drum lane is a new one to it.
        self.drums.clear();
        for (row, id) in app.track_ids().iter().enumerate() {
            let Ok(s) = u32::try_from(row) else { break };
            let slot = Slot(s);
            if let Some(gen) = e.lane_gen(slot) {
                // Every legacy lane is built with its pad bank.
                self.lanes.entry(*id).or_insert(Held { at: LaneAt { slot, gen }, pads: true });
            }
        }
        self.adopted = Some(self.epoch);
        true
    }

    /// **A lane for every track that has none**, in one Install: as many as
    /// the desk has rows (W6-rows-unlimited; the engine grows its spine for
    /// them). A track whose old lane is still on its way home waits for it.
    /// A lane past the legacy thirty-two is reached by identity like any
    /// other (its plugin racks, its pedal board, its instrument, its clips,
    /// its curves, its kit, its meter). Answers the slots it installed at.
    ///
    /// **The lowest free slot below [`LEGACY_LANES`] first, and one on its
    /// way home there is waited for** (a frame or two, design 7.13), before
    /// any slot past them is taken. An export builds its engine fresh with
    /// row r at slot r: a track Ctrl/Cmd+T made on a shipped desk's hidden
    /// row took slot 32 the frame that row's old lane started home, so live
    /// it stood on another slot than its export gave it, and while the
    /// engine kept parts for the legacy slots alone (routes, sends and keys
    /// until W6-engine-buses-sends; a drum lane's pad effects and a
    /// sampler's file until W7-slot-gaps) it lost what its export had.
    /// Nothing is kept that way now; the wait still gives a track the slot
    /// its export gives it, so the two engines agree slot for slot. A slot
    /// past them is taken only when none below is free or coming free.
    fn install(&mut self, app: &TangentApp, e: &mut Engine, rows: &[(usize, TrackId)]) -> Vec<Slot> {
        let legacy = u32::try_from(LEGACY_LANES).unwrap_or(u32::MAX);
        let mut taken: BTreeSet<Slot> = self.lanes.values().map(|h| h.at.slot).collect();
        taken.extend(self.leaving.iter().map(|l| l.at.slot));
        // The legacy slots on their way home that no track of this desk is
        // waiting to take back: one new track may wait for each.
        let mut owed = self
            .leaving
            .iter()
            .filter(|l| l.at.slot.0 < legacy && !rows.iter().any(|(_, id)| *id == l.id))
            .count();
        let mut batch: Vec<(TrackId, Slot, LaneSpec)> = Vec::new();
        for &(row, ref id) in rows {
            if self.lanes.contains_key(id)
                || self.leaving.iter().any(|l| l.id == *id)
                || batch.iter().any(|(b, _, _)| b == id)
            {
                continue;
            }
            let free = |s: &Slot| !taken.contains(s) && e.slot_free(*s);
            let slot = match (0..legacy).map(Slot).find(free) {
                Some(slot) => slot,
                None if owed > 0 => {
                    owed -= 1;
                    continue;
                }
                None => match (legacy..=u32::MAX).map(Slot).find(free) {
                    Some(slot) => slot,
                    None => break,
                },
            };
            taken.insert(slot);
            let kind = match app.channel_kind(row) {
                Some(ChannelKind::Midi) => LaneKind::Midi,
                Some(ChannelKind::Audio) => LaneKind::Audio,
                Some(ChannelKind::Bus) => LaneKind::Bus,
                None if app.desk_index().is_folder(row) => LaneKind::Folder,
                None => LaneKind::Empty,
            };
            let drum = track_holds(app, *id, tangent_ui::dialogs::DRUMPAD_PATH).is_some();
            batch.push((*id, slot, LaneSpec { kind, drum }));
        }
        if batch.is_empty() {
            return Vec::new();
        }
        let specs: Vec<(Slot, LaneSpec)> = batch.iter().map(|(_, s, spec)| (*s, *spec)).collect();
        match e.install_lanes(specs) {
            Ok(gens) => {
                let mut made = Vec::with_capacity(batch.len());
                for ((id, slot, spec), gen) in batch.into_iter().zip(gens) {
                    self.lanes.insert(id, Held { at: LaneAt { slot, gen }, pads: spec.drum });
                    made.push(slot);
                }
                self.stats.installs += 1;
                made
            }
            // Busy waits a frame; anything else is a slot this bridge
            // believed free and the engine did not, which the next frame's
            // `slot_free` will have caught up with.
            Err(_) => Vec::new(),
        }
    }

    /// **A lane that has just become a drum lane gets its pad bank**,
    /// attached behind its Install (design 7.13: "Attach(Pads) then
    /// Attach(Kit). Nothing reloads"). From the frame it is sent the track's
    /// pad bays have their engine names ([`Self::engine_bay`]), so the
    /// reconcile seats each pad's effects then, behind the bank on the ring.
    fn attach_pads(&mut self, app: &TangentApp, e: &mut Engine, rows: &[(usize, TrackId)]) {
        for (_, id) in rows {
            let Some(h) = self.lanes.get_mut(id) else { continue };
            if h.pads || track_holds(app, *id, tangent_ui::dialogs::DRUMPAD_PATH).is_none() {
                continue;
            }
            if e.attach(h.at.slot, h.at.gen, LanePart::Pads(Box::new(PadBank::new()))).is_ok() {
                h.pads = true;
                self.stats.attaches += 1;
            }
        }
    }

    /// **The lanes that became drum lanes since the last sync** (W7-join):
    /// each laned track whose first bay holds the drumpad now and did not
    /// then. Rebuilt whole every frame from the desk, so a track whose
    /// drumpad leaves and comes back (an undo, a swap there and back) is
    /// owed its kit again on its return, by then perhaps edited while its
    /// lane was not a drum lane and was handed none. A track waiting for its
    /// lane joins when the lane arrives, which `moved` already owes.
    fn note_drums(&mut self, app: &TangentApp, rows: &[(usize, TrackId)]) -> Vec<Slot> {
        let now: BTreeSet<TrackId> = rows
            .iter()
            .map(|(_, id)| *id)
            .filter(|id| self.lanes.contains_key(id))
            .filter(|id| track_holds(app, *id, tangent_ui::dialogs::DRUMPAD_PATH).is_some())
            .collect();
        let drummed = now
            .difference(&self.drums)
            .filter_map(|id| self.lanes.get(id).map(|h| h.at.slot))
            .collect();
        self.drums = now;
        drummed
    }

    /// The lanes whose row holds `sentinel` in any bay: on the live
    /// engine only MIDI rows (the live rule), on an offline one any row (the
    /// export's) - the two have always differed in this, and both are kept.
    fn instrument_lanes(&self, app: &TangentApp, sentinel: &str, live_rule: bool) -> Vec<Slot> {
        self.lanes_holding(app, sentinel)
            .into_iter()
            .filter(|(row, _)| !live_rule || app.channel_kind(*row) == Some(ChannelKind::Midi))
            .map(|(_, at)| at.slot)
            .collect()
    }

    /// **Publish `inputs` if they decide anything new**, else carry only the
    /// route gains and send levels that moved into every plan in flight
    /// (design 7.6: a gain that stays on its side of zero scales, it does not
    /// reorder, and a send's level never orders anything). Answers what
    /// became of them.
    fn publish(&mut self, e: &mut Engine, inputs: PlanInputs) -> Published {
        if let Some(p) = self.published.as_ref().filter(|p| structure(p) == structure(&inputs)) {
            // What only scales goes into the plans in flight; the one move
            // that cannot (`carry_scales`) publishes below like a new shape.
            if carry_scales(e, p, &inputs) {
                self.published = Some(inputs);
                return Published::Same;
            }
        }
        match e.publish_plan(&inputs) {
            Ok(gen) => {
                self.stats.plans += 1;
                self.published = Some(inputs);
                Published::New(gen)
            }
            // The ring is full: the previous plan keeps playing and this one
            // is built again next frame from whatever the desk says then.
            Err(_) => Published::Failed,
        }
    }

    /// **Everything the plan reads, from the app**: one lane per row that has
    /// one, in row order (rank is the shown order, design 7.9), then each
    /// lane on its way home, RETIRING, unless it is going this frame.
    fn plan_inputs(&self, app: &TangentApp, outs: usize, shape: Shape<'_>, due: &[usize]) -> PlanInputs {
        use LaneFlags as F;
        let n = self.rows.len();
        let freeze = shape.freeze();
        let probe = match shape {
            Shape::Offline { probe, .. } => probe,
            Shape::Live => None,
        };
        let hears = shape.hears_live();
        let live_rule = matches!(shape, Shape::Live);
        // **Per row, as many as the desk has**: the arm, the live-MIDI rule,
        // polarity and the curve suspend are each a row's own fact, read by
        // row. They were bits of words that stopped at row thirty-one.
        let armed: Vec<bool> = (0..n).map(|r| hears && app.is_armed(r)).collect();
        let frozen: BTreeSet<usize> = app.frozen_lanes().iter().map(|z| z.lane).collect();
        // **Armed, or alone in the editor** — the whole rule, in one place,
        // tested there. A frozen lane listens to nothing: its rack rests and
        // its file plays, so a note into it would sound through nothing
        // anyway. An export and a freeze hear no live input of any kind.
        let deaf: Vec<bool> = if hears {
            let kinds: Vec<_> = (0..n).map(|r| app.channel_kind(r)).collect();
            tangent_ui::recorder::live_midi_off(&kinds, &armed, app.midi_editor_lane())
        } else {
            vec![true; n]
        };
        let parents = app.folder_parents();
        let routes_midi = app.midi_routes();
        // **The desk by what each strip is** (W5): a row's mute and solo as
        // the engine hears them (a folder's reaching its children), the
        // specials' their own, never an index into a strip-space desk.
        // **An export of selected tracks**: each track left out is
        // published LEFT_OUT, each folder whose own row is left out
        // ROW_LEFT_OUT; what is left in answers the desk's mutes and solos
        // as it always does, so every box checked is "Everything".
        let subset = self.left_out.as_ref();
        let soloed = |row: usize| {
            freeze.is_none()
                && (app.strip_heard_soloed(StripRef::Row(row)) || probe.is_some_and(|p| p.solo.contains(&row)))
        };
        let muted = |row: usize| freeze.is_none() && app.strip_heard_muted(StripRef::Row(row));
        let mut lanes = Vec::with_capacity(n + self.leaving.len());
        for (row, (id, at)) in self.rows.iter().enumerate() {
            let Some(at) = at else { continue };
            // **Every instrument the track holds, wherever its bay**: a DX7,
            // a keyboard and a kit on one track all play from the same notes
            // (`instrument::Layer`). This read the FIRST bay alone, so a
            // second instrument after it was silence.
            let holds = |sentinel: &str| track_holds(app, *id, sentinel).is_some();
            let is_frozen = frozen.contains(&row);
            let midi_ok = !live_rule || app.channel_kind(row) == Some(ChannelKind::Midi);
            let flags = F::default()
                .with(F::ARMED, armed.get(row).copied().unwrap_or(false))
                .with(F::MIDI_OFF, deaf.get(row).copied().unwrap_or(true) || is_frozen)
                .with(F::FROZEN, is_frozen)
                .with(F::DRUM, holds(tangent_ui::dialogs::DRUMPAD_PATH))
                .with(F::BUILTIN, midi_ok && holds(tangent_ui::dialogs::BUILTIN_PATH) && !is_frozen)
                .with(F::KEYS, midi_ok && holds(tangent_ui::dialogs::KEYS_PATH) && !is_frozen)
                .with(F::MUTED, muted(row))
                .with(F::SOLOED, soloed(row))
                .with(F::LEFT_OUT, subset.is_some_and(|out| out.tracks.contains(id)))
                .with(F::ROW_LEFT_OUT, subset.is_some_and(|out| out.rows.contains(id)))
                .with(F::PHASE, app.strip_inverted(StripRef::Row(row)))
                .with(F::SUSPEND, app.auto_suspended(AutoLane::Row(row)))
                // **A bus row** (design 9): its rack runs once over what is
                // routed and sent to it, it is solo-safe, and it is the only
                // lane a send may land on (`desk_plan::SendTo`).
                .with(F::BUS, app.channel_kind(row) == Some(ChannelKind::Bus));
            let parent = parents
                .get(row)
                .and_then(|p| usize::try_from(*p).ok())
                .and_then(|p| self.lane(p))
                .map(|a| a.slot);
            lanes.push(InLane {
                slot: at.slot,
                gen: at.gen,
                flags,
                parent,
                punch: if live_rule { self.punches.get(row).copied().flatten() } else { None },
                midi: if live_rule {
                    routes_midi.get(row).copied().unwrap_or(MidiRoute::EVERYTHING)
                } else {
                    MidiRoute::EVERYTHING
                },
                pick: None,
            });
        }
        for (i, l) in self.leaving.iter().enumerate() {
            if l.sent || due.contains(&i) {
                continue;
            }
            lanes.push(InLane {
                slot: l.at.slot,
                gen: l.at.gen,
                flags: F::RETIRING.with(F::MIDI_OFF, true),
                parent: None,
                punch: None,
                midi: MidiRoute::EVERYTHING,
                pick: None,
            });
        }
        // The four strips that are not rows, by what each is. The master's
        // own mute is the host's (`set_master_mute`), never a plan flag.
        let special = |s: StripRef| {
            let desk_strip = s != StripRef::Master;
            F::default()
                .with(F::MUTED, desk_strip && freeze.is_none() && app.strip_heard_muted(s))
                .with(F::SOLOED, desk_strip && freeze.is_none() && app.strip_heard_soloed(s))
                .with(F::PHASE, app.strip_inverted(s))
                .with(F::MIDI_OFF, !hears)
        };
        let specials = SpecialFlags {
            drums: special(StripRef::Drums),
            click: special(StripRef::Click),
            fx: special(StripRef::Fx),
            master: special(StripRef::Master).with(F::SUSPEND, app.auto_suspended(AutoLane::Master)),
        };
        // **The wiring, by identity** (`read_desk`): the routing table pruned
        // against the device, and the sidechain keys, in the engine's names.
        let (routes, keys) = self.read_desk(app, outs);
        PlanInputs { lanes, specials, routes, sends: self.plan_sends(app), keys }
    }

    /// **Every send the plan carries**: each strip's knob as its Fx entry
    /// (`knob_sends`, W7-slot-gaps) and every send to a bus (`read_sends`,
    /// W7-buses-ui). The two never share a pair of ends, so `carry_scales`
    /// tells each from the other.
    fn plan_sends(&self, app: &TangentApp) -> Vec<InSend> {
        let mut out = self.knob_sends(app);
        out.extend(self.read_sends(app));
        out
    }

    /// **The desk's sends to buses, read by identity** (W7-buses-ui, design
    /// 9): every strip's sends to a bus by the one rule the app shows them by
    /// (`desk_model::bus_sends` over `TangentApp::bus_ids`), in the engine's
    /// names, the sender's lane (or the drum bus, or the click) and the
    /// bus's lane, at the send's own level and side of the fader. A bus
    /// waiting for its lane is sent nothing until it has one.
    ///
    /// **In the order of the senders' identities, not their rows**: a bus
    /// sums its sends in the order it is handed them (`desk_plan::InSend`),
    /// and f32 addition does not commute bit for bit, so an order by row
    /// would change a bus's last bits on every reorder. By track id, each
    /// strip's own sends in the order they were made, then the drum bus's
    /// and the click's: no reorder moves a send, in the plan as on the desk.
    ///
    /// **The knob's send into the effects return is not here**: it is
    /// `knob_sends`, and `plan_sends` hands the plan both.
    pub fn read_sends(&self, app: &TangentApp) -> Vec<InSend> {
        // Each laned track's row and lane this frame, by identity, so the
        // senders walk in id order and a bus is found in one look.
        let laned: BTreeMap<TrackId, (usize, Slot)> = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(row, (id, at))| Some((*id, (row, at.as_ref()?.slot))))
            .collect();
        let buses = app.bus_ids();
        let mut out = Vec::new();
        let mut push = |from: SendFrom, s: StripRef| {
            for x in tangent_ui::desk_model::bus_sends(app.strip_sends(s), &buses) {
                let Some(&(_, to)) = x.to.track().and_then(|id| laned.get(&id)) else { continue };
                out.push(InSend { from, to: SendTo::Lane(to), level: x.level as f32, pre: x.pre });
            }
        };
        for (row, slot) in laned.values() {
            push(SendFrom::Lane(*slot), StripRef::Row(*row));
        }
        push(SendFrom::Drums, StripRef::Drums);
        push(SendFrom::Click, StripRef::Click);
        out
    }

    /// **Each strip's send knob, as its post-fader Return 0 entry** (design
    /// 9: "today's knob becomes the Fx entry"; W7-slot-gaps): a row's at its
    /// lane, the drum bus's and the click's at their own, at the knob's
    /// level, and NO entry for a knob at nothing, which is how the desk keeps
    /// it (`desk_model::Send`: a send at nothing is not a send). The engine
    /// hears a strip with an entry through its entries alone, summed where
    /// the knob was (`InSend`), so this moves no bit; it makes the knob a
    /// plan send, whose level rides across plans by its ends
    /// (`Engine::set_send_level`, `publish`) and whose curves are the plan's
    /// own. A knob crossing zero makes or takes an entry, so it republishes,
    /// as a route's gain crossing zero does. Sends to buses are the desk's
    /// other entries (W7-buses-ui).
    fn knob_sends(&self, app: &TangentApp) -> Vec<InSend> {
        let entry = |from: SendFrom, s: StripRef| {
            let level = app.strip_send(s) as f32;
            (level.is_finite() && level > 0.0).then_some(InSend { from, to: SendTo::Fx, level, pre: false })
        };
        let rows = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(row, (_, at))| entry(SendFrom::Lane(at.as_ref()?.slot), StripRef::Row(row)));
        rows.chain(entry(SendFrom::Drums, StripRef::Drums))
            .chain(entry(SendFrom::Click, StripRef::Click))
            .collect()
    }

    /// **The continuous values, by slot**: each row's fader, pan and Fx send
    /// into its lane's shared numbers, the two sending specials' sends, and
    /// every keyboard lane's patch and knobs. Atomic stores, every frame:
    /// writing an unchanged value costs nothing, and a cache of "what the
    /// engine holds" would be one more thing to disagree with the desk.
    fn store_levels(&self, app: &TangentApp, e: &Engine, shape: Shape<'_>) {
        let freeze = shape.freeze();
        for (row, (_, at)) in self.rows.iter().enumerate() {
            let Some(at) = at else { continue };
            // A freeze renders post-rack and PRE-fader: unity gain, centre
            // pan (unity there).
            let (gain, pan) = if freeze == Some(row) {
                (1.0, 0.0)
            } else {
                (app.channel_gain(row), app.channel_pan(row))
            };
            e.set_lane_levels(at.slot, gain, pan, app.strip_send(StripRef::Row(row)) as f32);
        }
        let send = |s: StripRef| app.strip_send(s) as f32;
        e.set_special_sends(send(StripRef::Drums), send(StripRef::Click));
        // **Every keyboard lane's own state, every frame.** A patch is a flag
        // the engine compares per block, and three atomic stores per lane
        // per frame is what every fader already costs.
        let live_rule = matches!(shape, Shape::Live);
        for (row, at) in self.lanes_holding(app, tangent_ui::dialogs::KEYS_PATH) {
            if live_rule && app.channel_kind(row) != Some(ChannelKind::Midi) {
                continue;
            }
            e.set_keys_patch(at.slot, keys_patch_of(app.keys_patch_of(row)));
            e.set_keys_params(at.slot, &app.keys_params_of(row));
        }
    }
}

/// What became of a frame's plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Published {
    /// Sent, at this generation.
    New(u64),
    /// Nothing that decides an order moved; the route gains and send levels
    /// that moved, if any, were written.
    Same,
    /// The op ring was full.
    Failed,
}

/// **What decides an order**, of a plan's inputs: everything but the route
/// gains' size, which only scale (a gain crossing zero does decide, so its
/// side is kept), and the send levels, which only scale at any size:
/// `build_plan` keeps a send whatever its level. A knob at nothing is no
/// entry at all (`knob_sends`), so a knob crossing zero still republishes,
/// by the entry it makes or takes, not by its level.
fn structure(inputs: &PlanInputs) -> PlanInputs {
    let mut s = inputs.clone();
    for r in &mut s.routes {
        r.gain = if r.gain.is_finite() && r.gain > 0.0 { 1.0 } else { 0.0 };
    }
    for x in &mut s.sends {
        x.level = 0.0;
    }
    s
}

/// **Write what only scales into every plan in flight**: each route gain
/// and send level of `now` that moved since `was` (two inputs of one
/// structure), found in each plan by the identity the engine keys it by
/// (`Engine::set_route_gain`, `Engine::set_send_level`).
///
/// Answers false, writing nothing, when a moved send cannot be told from
/// its twin. The engine names a send by its two ends, so a strip sending to
/// one bus from both sides of its fader (a file can say so; no gesture
/// makes one) would have one level written into both. That frame publishes
/// a plan instead, which stores each level in its own cell.
fn carry_scales(e: &Engine, was: &PlanInputs, now: &PlanInputs) -> bool {
    let moved: Vec<&InSend> = was
        .sends
        .iter()
        .zip(&now.sends)
        .filter(|(a, b)| a.level.to_bits() != b.level.to_bits())
        .map(|(_, b)| b)
        .collect();
    if !moved.is_empty() {
        let ends_of = |s: &InSend| crate::desk_plan::send_key(s.from, s.to);
        let mut ends: Vec<(u64, u64)> = now.sends.iter().map(ends_of).collect();
        ends.sort_unstable();
        let twinned = |s: &InSend| {
            let k = ends_of(s);
            ends.get(ends.partition_point(|x| *x < k) + 1) == Some(&k)
        };
        if moved.iter().any(|s| twinned(s)) {
            return false;
        }
    }
    for (old, new) in was.routes.iter().zip(&now.routes) {
        if old.gain.to_bits() != new.gain.to_bits() {
            e.set_route_gain(new.from, new.to, new.gain);
        }
    }
    for s in moved {
        e.set_send_level(s.from, s.to, s.level);
    }
    true
}

// ── the inputs: which device input each audio track takes ─────────────────
//
// W5-engine-memory-inputs (design 7.12, 7.13 and A5). The app names a
// track's input by the track ("3", "1+2"); the stream takes them as a list
// of picks; the engine's monitor and its capture tee know a pick by its
// place in that list. This is the one place the three meet.

/// **The input stream as the host has it open this frame**: how wide the
/// device is (`None`: no device), and the picks it was opened with, as
/// asked. The device fits them (`Picks::fitted`), and so does
/// [`PickTable::bind`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputStream {
    pub device_channels: Option<usize>,
    pub picks: tangent_record::audio::Picks,
}

/// **One row of the desk as the pick table reads it**, in row order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputRow {
    pub id: TrackId,
    /// An AUDIO track: the only kind with an input.
    pub audio: bool,
    /// The input it names, when it names one (`TangentApp::parse_input_spec`).
    pub spec: Option<tangent_record::audio::ChannelPick>,
    /// R lit: its pick is owed a capture ring (design 7.12).
    pub armed: bool,
}

/// **The desk's rows, for [`PickTable::bind`]**: each row's track, whether
/// it is an audio track, the input it names and its arm.
pub fn input_rows(app: &TangentApp) -> Vec<InputRow> {
    app.track_ids()
        .iter()
        .enumerate()
        .map(|(row, id)| InputRow {
            id: *id,
            audio: app.channel_kind(row) == Some(ChannelKind::Audio),
            spec: TangentApp::parse_input_spec(app.channel_input_spec(row)).map(|(a, b)| match b {
                Some(b) => tangent_record::audio::ChannelPick::Stereo(a, b),
                None => tangent_record::audio::ChannelPick::Mono(a),
            }),
            armed: row_armed(app, row),
        })
        .collect()
}

/// **One pick of this frame**, by its place: the track it feeds (if any),
/// the input it reads, and that input's columns on the open device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickEntry {
    pub pick: PickId,
    pub track: Option<TrackId>,
    /// The device input it reads. `None` with a width is the whole device
    /// taken as one input (a stream that never named an input); `None`
    /// without one reads nothing.
    pub from: Option<tangent_record::audio::ChannelPick>,
    /// Device columns read: 1, 2, the device's width for the whole device,
    /// or 0 for nothing (no device, or an input the device does not have).
    pub width: u8,
    /// The device columns of its left and right (a mono input's twice).
    pub cols: [u16; 2],
}

/// **Which input each audio track takes, by identity** (contract C9's
/// `PickTable`; design 7.13 and A5).
///
/// **Explicit**, once any audio track names its own input: each track that
/// names one holds a `PickId` from the moment it first does, the lowest one
/// free, and keeps it for as long as it names one. So a reorder, an insert
/// above, a removal elsewhere or a second track bound moves no pick: the
/// capture ring a record is writing is never another track's, and the stream
/// is never reopened for a change of shape (its picks are listed by `PickId`,
/// which a reorder does not change). A track that names none takes nothing.
///
/// **Ordinal**, for a desk that never named an input: the k-th audio row
/// takes pick k of the stream as it was opened (the band's input and the
/// ones beside it), exactly as it always has.
///
/// **Held**: a pick a running record took at its press keeps the input it
/// was reading and is given to no other track until the record lets it go,
/// whatever happens to its track meanwhile (design A5: the record is bound by
/// identity). It feeds the track that recorded through it for as long as
/// that is an audio track with no other pick, so the track hears what it
/// takes. In EITHER mode, and across a change of mode: a desk that names its
/// first input mid-take, or stops naming its last, turns explicit or ordinal
/// around the held picks and renumbers none of them. The design holds
/// input-binding changes for a record's length (`desk_shape_held`, W6); the
/// table keeps its own promise without that hold.
#[derive(Clone, Debug, Default)]
pub struct PickTable {
    /// Explicit mode: every track that names an input, and its pick.
    named: BTreeMap<TrackId, PickId>,
    /// The picks a record holds, each with what it fed and read at the press.
    held: BTreeMap<PickId, HeldPick>,
    /// This frame's answer, by PickId: `picks[p].pick == PickId(p)`.
    picks: Vec<PickEntry>,
    explicit: bool,
}

/// **What a held pick keeps** ([`PickTable::hold`]): the track it fed at the
/// press, and what it read there, the whole device included.
///
/// The input, never its columns: the columns are worked out against the
/// device as it is open ([`pick_columns`]), and the monitor ring carries
/// whole device frames whatever the stream's picks say, so a held pick reads
/// the same columns for as long as its record's device is open. It kept only
/// `from`, which for the whole device is `None`: the pick a record took on a
/// stream opened whole was rebuilt as reading NOTHING the moment the desk
/// named an input anywhere, and the take stopped growing with no word said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HeldPick {
    track: Option<TrackId>,
    from: Option<tangent_record::audio::ChannelPick>,
    whole: bool,
}

impl PickTable {
    /// **This frame's picks**, from the rows in row order and the stream as
    /// it is open. Allocates per call (a frame's worth, on the host).
    pub fn bind(&mut self, rows: &[InputRow], stream: &InputStream) {
        use tangent_record::audio::ChannelPick;
        self.explicit = rows.iter().any(|r| r.audio && r.spec.is_some());
        let dev = stream.device_channels.unwrap_or(0);
        let mut entries: Vec<(Option<TrackId>, Option<ChannelPick>, bool)> = Vec::new();
        if self.explicit {
            let wanted: BTreeMap<TrackId, ChannelPick> =
                rows.iter().filter(|r| r.audio).filter_map(|r| Some((r.id, r.spec?))).collect();
            // A track that stopped naming an input lets its pick go, unless a
            // record holds it.
            self.named.retain(|id, _| wanted.contains_key(id));
            let mut used: BTreeSet<u32> = self.named.values().map(|p| p.0).collect();
            used.extend(self.held.keys().map(|p| p.0));
            // In row order, so a desk bound for the first time numbers its
            // picks the way it reads (and the way the stream used to list them).
            for r in rows.iter().filter(|r| r.audio && r.spec.is_some()) {
                if self.named.contains_key(&r.id) {
                    continue;
                }
                // **A track a record holds a pick for names its input on
                // that pick**: the record keeps reading what it read at the
                // press, and when it lets go the track's own binding is
                // there, on the pick it had. A fresh one would have been a
                // second pick for one track, both on its lane.
                if let Some(p) = self.held.iter().find(|(_, h)| h.track == Some(r.id)).map(|(p, _)| *p) {
                    self.named.insert(r.id, p);
                    continue;
                }
                let Some(free) = (0..u32::MAX).find(|p| !used.contains(p)) else { break };
                if free as usize >= tangent_ui::desk_id::MAX_ROWS {
                    break;
                }
                used.insert(free);
                self.named.insert(r.id, PickId(free));
            }
            let n = used.iter().next_back().map_or(0, |p| *p as usize + 1);
            entries.resize(n, (None, None, false));
            for (id, pick) in &self.named {
                entries[pick.0 as usize] = (Some(*id), wanted.get(id).copied(), false);
            }
        } else {
            // Ordinal: nothing named, nothing kept by name. **Around the
            // held picks**: each stays its own track's (below), and every
            // other audio row takes the next pick nobody holds, in row
            // order, each pick reading the stream at its own place. With
            // nothing held that is the walk it always was, the k-th audio
            // row on pick k. It ignored the held picks, so a desk that
            // stopped naming its last input mid-take had the record's pick
            // read whatever the stream held at its place, and a frame later
            // that was the band's own channel (`devices::audio_selection`):
            // another microphone in the take.
            self.named.clear();
            let honoured = stream.picks.clone().fitted(dev.max(1));
            let stream_n = if honoured.is_empty() { usize::from(dev > 0) } else { honoured.len() };
            let holders: BTreeSet<TrackId> = self.held.values().filter_map(|h| h.track).collect();
            let mut free = rows.iter().filter(|r| r.audio && !holders.contains(&r.id)).map(|r| r.id).peekable();
            let past_held = self.held.keys().next_back().map_or(0, |p| p.0 as usize + 1);
            let mut p = 0usize;
            while p < stream_n.max(past_held) || free.peek().is_some() {
                let whole = honoured.is_empty() && p == 0 && dev > 0;
                let track = if self.held.contains_key(&PickId(p as u32)) { None } else { free.next() };
                entries.push((track, honoured.get(p), whole));
                p += 1;
            }
        }
        // **A held pick reads what it read at the press, in either mode**,
        // the whole device included, and feeds the track it fed there while
        // that is still an audio track with no other pick: what a track
        // monitors through its record is what the record takes. A track
        // removed meanwhile is fed by nothing, and its pick still reads, so
        // the tee still writes the take (`Renderer::mix_monitor` tees by
        // PickId before it asks the binding).
        if !self.held.is_empty() {
            let audio: BTreeSet<TrackId> = rows.iter().filter(|r| r.audio).map(|r| r.id).collect();
            for (pick, h) in &self.held {
                if let Some(e) = entries.get_mut(pick.0 as usize) {
                    let track = h
                        .track
                        .filter(|id| audio.contains(id) && self.named.get(id).is_none_or(|p| p == pick));
                    *e = (track, h.from, h.whole);
                }
            }
        }
        self.picks = entries
            .into_iter()
            .enumerate()
            .map(|(p, (track, from, whole))| {
                let (width, cols) = pick_columns(from, whole, dev);
                PickEntry { pick: PickId(p as u32), track, from, width, cols }
            })
            .collect();
    }

    /// **A record takes these picks**: each keeps the track it feeds now and
    /// the input it reads now, the whole device included (a pick that names
    /// no input and reads columns: see [`PickEntry::from`]), and none is
    /// given to another track, until [`Self::release`].
    pub fn hold(&mut self, picks: &[PickId]) {
        for p in picks {
            let e = self.picks.get(p.0 as usize);
            let held = HeldPick {
                track: e.and_then(|e| e.track),
                from: e.and_then(|e| e.from),
                whole: e.is_some_and(|e| e.from.is_none() && e.width > 0),
            };
            self.held.insert(*p, held);
        }
    }

    /// The record is over: its picks are the desk's again.
    pub fn release(&mut self, picks: &[PickId]) {
        for p in picks {
            self.held.remove(p);
        }
    }

    /// **The held picks that read nothing now**, as of the last
    /// [`Self::bind`]: a record holds each, and the device as it is open has
    /// none of the input it read at the press (an interface reopened with
    /// fewer inputs). The tee writes a pick's ring only while the pick reads
    /// (`Renderer::mix_monitor` skips one of width 0), so each of these is a
    /// take that has stopped growing, and the host says so.
    pub fn held_reading_nothing(&self) -> Vec<PickId> {
        self.held
            .keys()
            .copied()
            .filter(|p| self.picks.get(p.0 as usize).is_none_or(|e| e.width == 0))
            .collect()
    }

    /// **A track's pick**, as of the last [`Self::bind`].
    pub fn pick_of(&self, id: TrackId) -> Option<PickId> {
        self.picks.iter().find(|e| e.track == Some(id)).map(|e| e.pick)
    }

    /// Every pick of the last [`Self::bind`], by PickId.
    #[cfg_attr(not(test), allow(dead_code, reason = "read by the pick table's tests"))]
    pub fn picks(&self) -> &[PickEntry] {
        &self.picks
    }

    /// Whether the desk names its inputs (explicit), or takes them by order.
    #[cfg_attr(not(test), allow(dead_code, reason = "asked by the pick table's tests"))]
    pub fn explicit(&self) -> bool {
        self.explicit
    }

    /// **The monitor's layout of these picks**: every pick by PickId, on the
    /// lane its track has (`lane_of`), with its width and device columns.
    /// A pick whose track has no lane, or that feeds no track, is bound to
    /// no lane and still drains.
    pub fn monitor(&self, lane_of: impl Fn(TrackId) -> Option<LaneAt>) -> Vec<crate::lane_bank::MonitorPick> {
        use crate::lane_bank::MonitorPick;
        self.picks
            .iter()
            .map(|p| {
                let (slot, gen) =
                    p.track.and_then(&lane_of).map_or((Slot(0), MonitorPick::NO_LANE), |l| (l.slot, l.gen));
                MonitorPick { pick: p.pick, slot, gen, width: p.width, cols: p.cols }
            })
            .collect()
    }

    /// **The picks owed a capture ring**: each armed audio row's pick that
    /// reads something (design 7.12: capture rings are per ARMED pick). A
    /// pick that reads nothing has nothing to capture and is owed none.
    pub fn armed(&self, rows: &[InputRow]) -> Vec<PickId> {
        rows.iter()
            .filter(|r| r.audio && r.armed)
            .filter_map(|r| self.pick_of(r.id))
            .filter(|p| self.picks.get(p.0 as usize).is_some_and(|e| e.width > 0))
            .collect()
    }

    /// **The inputs the stream opens with**, for an explicit desk: every
    /// named pick's input, in PickId order, so a change of shape never
    /// changes the list (and so never reopens the stream). Empty for an
    /// ordinal desk, whose stream is the band's own choice.
    pub fn stream_picks(&self) -> Vec<tangent_record::audio::ChannelPick> {
        if !self.explicit {
            return Vec::new();
        }
        self.picks.iter().filter(|e| e.track.is_some()).filter_map(|e| e.from).collect()
    }
}

/// **A pick's width and device columns on a `dev`-wide device**: its own
/// input's channels if the device has them, the first pair of the whole
/// device for a whole-device pick, nothing otherwise.
fn pick_columns(from: Option<tangent_record::audio::ChannelPick>, whole: bool, dev: usize) -> (u8, [u16; 2]) {
    use tangent_record::audio::ChannelPick;
    let col = |c: u16| if usize::from(c) < dev { Some(c) } else { None };
    match from {
        Some(ChannelPick::Mono(a)) => match col(a) {
            Some(a) => (1, [a, a]),
            None => (0, [0, 0]),
        },
        Some(ChannelPick::Stereo(a, b)) => match (col(a), col(b)) {
            (Some(a), Some(b)) => (2, [a, b]),
            _ => (0, [0, 0]),
        },
        None if whole && dev > 0 => {
            let right = u16::try_from(1.min(dev - 1)).unwrap_or(0);
            (u8::try_from(dev).unwrap_or(u8::MAX), [0, right])
        }
        None => (0, [0, 0]),
    }
}

impl Bridge {
    /// **Every audio track's input, once a frame** (C9's `sync_inputs`):
    /// the pick table bound against this frame's rows and the stream, the
    /// monitor told every pick by `PickId` with its lane (slot and
    /// generation) and its device columns, and a capture ring held for every
    /// armed pick and for no other (design 7.12). Answers the inputs the
    /// stream should open with (see [`PickTable::stream_picks`]), for the
    /// host to hand the device selection.
    pub fn sync_inputs(
        &mut self,
        app: &TangentApp,
        e: &mut Engine,
        stream: &InputStream,
    ) -> Vec<tangent_record::audio::ChannelPick> {
        self.sync_input_rows(&input_rows(app), e, stream)
    }

    /// [`Self::sync_inputs`] over rows already read: the one road, which the
    /// app's rows take every frame and a test can drive with rows of its own
    /// (an arm the app has no keyboard road to, or more rows than a desk of
    /// today holds).
    pub fn sync_input_rows(
        &mut self,
        rows: &[InputRow],
        e: &mut Engine,
        stream: &InputStream,
    ) -> Vec<tangent_record::audio::ChannelPick> {
        self.picks.bind(rows, stream);
        let monitor = self.picks.monitor(|id| self.lane_of(id));
        let dev = stream.device_channels.unwrap_or(0).max(1);
        e.set_monitor_inputs(&monitor, &stream.picks.clone().fitted(dev));
        // Busy waits a frame: the ring is asked for again next frame.
        let _ = e.sync_capture_rings(&self.picks.armed(rows));
        self.picks.stream_picks()
    }

    /// **A track's pick**, as of the last [`Self::sync_inputs`].
    pub fn pick_of(&self, id: TrackId) -> Option<PickId> {
        self.picks.pick_of(id)
    }

    /// The pick table, as of the last [`Self::sync_inputs`].
    #[cfg_attr(not(test), allow(dead_code, reason = "read by the input tests"))]
    pub fn pick_table(&self) -> &PickTable {
        &self.picks
    }

    /// **A record holds these picks** for as long as it runs: see
    /// [`PickTable::hold`].
    pub fn hold_picks(&mut self, picks: &[PickId]) {
        self.picks.hold(picks);
    }

    /// The record is over. See [`PickTable::release`].
    pub fn release_picks(&mut self, picks: &[PickId]) {
        self.picks.release(picks);
    }

    /// The held picks that read nothing now: see
    /// [`PickTable::held_reading_nothing`].
    pub fn held_reading_nothing(&self) -> Vec<PickId> {
        self.picks.held_reading_nothing()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    use tangent_ui::arrangement::{ArrClip, ArrNote, Arrangement, ClipKind};
    use tangent_ui::desk_model::Frozen;
    use tangent_ui::automation::{AutoParam, Automation, Curve, Point, Shape};
    use tangent_ui::host::Caps;
    // The settings fixtures are legacy desks, as wide as the frozen V32
    // map: its numbers, never the UI's live desk.
    use tangent_ui::desk_legacy::{V32_CHANNELS as CHANNELS, V32_STRIPS as STRIPS};
    use tangent_ui::recorder::INSERTS;
    use tangent_ui::routing::Route;
    use tangent_ui::settings::Settings;

    use crate::builtin_fx::{CHORUS_PATH, COMP_PATH, EQ_PATH, PLATE_PATH, SAMPLER_PATH, TRIM_PATH};
    use crate::desktop::sanitize_file_stem;

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

    /// The app under test: a free install, and `Caps::NONE` so nothing it
    /// does writes a settings file (`persist_global_settings` is off).
    fn app_with(settings: Settings) -> TangentApp {
        let ctx = egui::Context::default();
        TangentApp::with_license(&ctx, settings, Caps::NONE, crate::composite::tiers::free("bridge"))
    }

    /// **No offline road renders a hardware loop dry or silent.** A Tangent
    /// Insert in circuit refuses the probe and the full-speed builder (the
    /// export goes realtime before it reaches it), and refuses a freeze of
    /// its own track and of a folder whose stem sums it, but not a freeze of
    /// an unrelated track; a bypassed one is passed over everywhere.
    #[test]
    fn a_tangent_insert_in_circuit_refuses_every_offline_road() {
        use crate::builtin_fx::HWINSERT_PATH;
        let mut s = blank();
        kind(&mut s, &[0, 1, 2, 3], ChannelKind::Audio);
        // Row 2 is a folder over row 3, which holds the loop.
        s.rows[2].folder = true;
        s.rows[3].in_folder = true;
        let held = SeatBay::new(row(&s, 3), Rack::Chain(0), 0);
        s.plugins.insert(held, HWINSERT_PATH.to_owned());
        let mut app = app_with(s);
        assert_eq!(hw_inserts_in_circuit(&app, None), vec![held]);
        let why = hw_refusal_for(&app, None).expect("the export of a desk with a loop refuses");
        assert!(why.contains("Tangent Insert") && why.contains("real time"), "{why}");
        // The probe goes through the one builder and is refused there.
        let mut furnish = Furnish::default();
        match full_build_with(&app, RATE, Timebase::new(), BuildFor::Probe, &mut furnish) {
            Err(e) => assert!(e.contains("Tangent Insert"), "{e}"),
            Ok(_) => panic!("the probe rendered a hardware loop offline"),
        }
        // A freeze of the track, and of its folder, refuses; of another, not.
        assert!(hw_refusal_for(&app, Some(3)).is_some_and(|w| w.contains("frozen")));
        assert!(hw_refusal_for(&app, Some(2)).is_some(), "the folder's stem sums the loop");
        assert!(hw_refusal_for(&app, Some(0)).is_none(), "an unrelated track could not be frozen");
        let id = app.track_ids()[3];
        match full_build_with(&app, RATE, Timebase::new(), BuildFor::Freeze(id), &mut Furnish::default()) {
            Err(e) => assert!(e.contains("frozen"), "{e}"),
            Ok(_) => panic!("a freeze rendered a hardware loop offline"),
        }
        // Bypassed, it is passed over: every road is open again.
        app.toggle_bay_bypass(held);
        assert!(hw_inserts_in_circuit(&app, None).is_empty());
        assert!(hw_refusal_for(&app, None).is_none() && hw_refusal_for(&app, Some(3)).is_none());
        assert!(full_build_with(&app, RATE, Timebase::new(), BuildFor::Probe, &mut Furnish::default()).is_ok());
    }

    /// **An export runs through its loops only when every one is up**: a
    /// full-speed export turns realtime, a realtime one runs as asked, and
    /// any loop that is down refuses the export with its own status line
    /// rather than writing the track dry.
    #[test]
    fn an_export_runs_through_its_loops_only_when_every_one_is_up() {
        let up = (true, false, "Connected.".to_owned());
        let down = (false, false, crate::units::hwinsert::Link::NoInput.sentence().to_owned());
        let pinging = (true, true, "Connected.".to_owned());
        assert_eq!(hw_export_verdict(false, &[]), Ok(false), "a desk with no loops was touched");
        assert_eq!(hw_export_verdict(true, &[]), Ok(false));
        assert_eq!(hw_export_verdict(false, &[up.clone()]), Ok(true), "a full-speed export was not made realtime");
        assert_eq!(hw_export_verdict(true, &[up.clone(), up.clone()]), Ok(false));
        for realtime in [false, true] {
            let why = hw_export_verdict(realtime, &[up.clone(), down.clone()]).expect_err("a dry export ran");
            assert!(why.contains(&down.2), "{why}");
            assert!(why.contains(". Fix the loop"), "{why}");
            let why = hw_export_verdict(realtime, &[up.clone(), pinging.clone()]).expect_err("an export ran through a ping");
            assert!(why.contains("pinging"), "{why}");
        }
    }

    /// The shipped desk with every channel emptied, so each desk below says
    /// in full what is on it rather than inheriting the fresh install's
    /// drumpad and DX7.
    fn blank() -> Settings {
        let mut s = Settings::default();
        s.plugins.clear();
        for row in &mut s.rows {
            row.kind = None;
            row.sends.clear();
            (row.arm, row.mute, row.solo, row.phase) = (false, false, false, false);
        }
        s.arm_unsaid = false;
        s
    }

    fn kind(s: &mut Settings, lanes: &[usize], k: ChannelKind) {
        for &l in lanes {
            s.rows[l].kind = Some(k.row_kind());
        }
    }

    /// Row `row`'s send to the effects return (`Settings::set_strip_send`).
    fn send(s: &mut Settings, row: usize, level: f64) {
        s.set_strip_send(StripRef::Row(row), level);
    }

    /// Seat `unit` at the legacy desk's flat bay `strip * INSERTS + slot`,
    /// by the frozen map under the desk's rows: the fixtures' own numbers,
    /// which the desk now keeps by identity.
    fn bay(s: &mut Settings, strip: usize, slot: usize, unit: &str) {
        let sb = tangent_ui::desk_legacy::bay_of_flat(&s.desk_ids(), strip * INSERTS + slot).expect("a legacy bay");
        s.plugins.insert(sb, unit.to_owned());
    }

    /// The legacy desk's `(seat, bay)`, by identity.
    fn legacy_sb(s: &Settings, seat: usize, bay: usize) -> SeatBay {
        tangent_ui::desk_legacy::bay_at(&s.desk_ids(), seat, bay).expect("a legacy bay")
    }

    /// The track on row `n` of the fixture desk.
    fn row(s: &Settings, n: usize) -> Node {
        Node::Track(s.desk_ids()[n])
    }

    // ── the desk-structure pushes ───────────────────────────────────────────

    /// One scripted edit: a name, the desk after it, the punches the
    /// arrangement record would hand the frame, and the lanes' automation
    /// modes (app state, not settings: see `set_arr_auto_mode_for_test`).
    struct Step {
        name: &'static str,
        settings: Settings,
        punches: Vec<Option<u64>>,
        modes: Vec<(AutoLane, tangent_ui::automation::AutoMode)>,
    }

    /// The step's app: its settings, then its automation modes.
    fn step_app(step: &Step) -> TangentApp {
        let mut app = app_with(step.settings.clone());
        for &(lane, mode) in &step.modes {
            app.set_arr_auto_mode_for_test(lane, mode);
        }
        app
    }

    /// Arm, mute, solo, phase, folder, route, key, bypass, a MIDI lane's
    /// device and channel, the curves' modes and freeze, one edit at a time
    /// on top of the last, over a desk that holds every instrument kind the
    /// strip masks name.
    ///
    /// **Every word the bridge writes is moved off a fresh engine's value by
    /// some step.** A word that only ever holds its default reads back the
    /// same whether the push happened or not, so the MIDI routes (`u32::MAX`
    /// on a fresh engine) and the suspend mask (zero) each get a step of
    /// their own, the master's bit 32 included.
    fn steps() -> Vec<Step> {
        let mut s = blank();
        kind(&mut s, &[0, 1, 2], ChannelKind::Midi);
        kind(&mut s, &[3, 4, 5, 6, 7], ChannelKind::Audio);
        bay(&mut s, 0, 0, tangent_ui::dialogs::BUILTIN_PATH);
        bay(&mut s, 1, 0, tangent_ui::dialogs::KEYS_PATH);
        bay(&mut s, 2, 0, tangent_ui::dialogs::DRUMPAD_PATH);
        bay(&mut s, 4, 0, COMP_PATH);
        bay(&mut s, 4, 1, EQ_PATH);
        send(&mut s, 3, 0.5);
        let none: Vec<Option<u64>> = Vec::new();
        let mut modes: Vec<(AutoLane, tangent_ui::automation::AutoMode)> = Vec::new();
        let mut out = vec![Step { name: "desk", settings: s.clone(), punches: none.clone(), modes: modes.clone() }];
        let mut edit = |name: &'static str, f: &dyn Fn(&mut Settings, &mut Vec<_>), punches: Vec<Option<u64>>| {
            f(&mut s, &mut modes);
            out.push(Step { name, settings: s.clone(), punches, modes: modes.clone() });
        };
        edit(
            "arm",
            &|s, _| {
                s.rows[0].arm = true;
                s.rows[4].arm = true;
            },
            none.clone(),
        );
        edit("mute", &|s, _| s.rows[2].mute = true, none.clone());
        edit("solo", &|s, _| s.rows[4].solo = true, none.clone());
        edit(
            "phase",
            &|s, _| {
                s.rows[1].phase = true;
                // The effects return's polarity is its own switch now (the
                // specials left strip space, W5): bit 34 of the old mask,
                // and the same bit of the engine's.
                s.set_strip_switch(StripRef::Fx, tangent_ui::settings::StripSwitch::Phase, true);
            },
            none.clone(),
        );
        edit(
            "folder",
            &|s, _| {
                s.rows[5].folder = true;
                s.rows[6].in_folder = true;
                s.rows[7].in_folder = true;
                s.rows[5].mute = true;
            },
            none.clone(),
        );
        edit(
            "route",
            &|s, _| {
                s.routes.add(Route::new(row(&s, 3), Target::Node(row(&s, 4))));
                s.routes.add(Route::new(row(&s, 6), Target::Hardware { first: 0, stereo: true }));
                // Past the offline engine's two outputs: pruned at the door.
                s.routes.add(Route::new(row(&s, 7), Target::Hardware { first: 4, stereo: true }));
            },
            none.clone(),
        );
        edit("route-gain", &|s, _| s.routes.set_gain(0, 0.5), none.clone());
        edit("key", &|s, _| s.insert_keys.set(legacy_sb(&s, 4, 0), Some(row(&s, 3))), none.clone());
        edit(
            "bypass",
            &|s, _| {
                s.bypass.insert(legacy_sb(&s, 4, 1));
            },
            none.clone(),
        );
        edit(
            "midi-route",
            &|s, _| {
                // The Keys lane hears one named device on wire channel 10. No
                // ports are open in a test app, so the name resolves to no
                // source slot at all: `src` 0, which is still off the default.
                s.rows[1].midi =
                    tangent_ui::recorder::MidiInputSel { devices: vec!["Keystation".to_owned()], channel: Some(9) };
            },
            none.clone(),
        );
        edit(
            "auto-modes",
            &|_, m| {
                use tangent_ui::automation::AutoMode;
                m.push((AutoLane::Row(3), AutoMode::Off));
                m.push((AutoLane::Row(4), AutoMode::Write));
                // The master's lane is the master's by name (W6), where it
                // was bit 32 of a word past a u32.
                m.push((AutoLane::Master, AutoMode::Touch));
            },
            none.clone(),
        );
        edit(
            "freeze",
            &|s, _| s.rows[0].frozen = Some(Frozen { path: "media/f.wav".into(), len_s: 1.0 }),
            none.clone(),
        );
        let punched = vec![None, None, None, Some(48_000)];
        edit("punch", &|_, _| {}, punched);
        edit("unpunch", &|_, _| {}, none.clone());
        edit("render-ahead", &|s, _| s.render_ahead_ms = 5.0, none);
        out
    }

    /// **The bridge's harness**: a free app, an offline engine, and the
    /// scripted edits above synced one frame each; what the engine holds
    /// after every frame is read back whole and compared against the golden
    /// of what `DesktopApp` pushed at the base commit. Then the same desk
    /// again on a FRESH engine, which must be primed whole, and a frame that
    /// changed nothing, which must not republish (a plan planted behind the
    /// bridge's back survives it).
    ///
    /// **The golden is the shim's, and the engine now holds a plan.** The
    /// readback decodes the newest plan into the words the shim pushed
    /// (`Engine::desk_readback`), slot for bit, so every frame recorded
    /// against `DesktopApp`'s pushes before there was a bridge has to come
    /// out of the plan unchanged: the plan carries the same desk.
    #[test]
    fn the_bridge_pushes_what_the_desktop_pushed() {
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        let mut seen: Vec<(String, serde_json::Value)> = Vec::new();
        let mut last = None;
        for step in steps() {
            let app = step_app(&step);
            p.punch_at(step.punches.clone());
            p.sync(&app, &mut e);
            seen.push((step.name.to_owned(), e.desk_readback()));
            last = Some(app);
        }
        let app = last.expect("steps");
        // An unchanged frame: a plan planted behind the bridge's back - the
        // last one's lanes with one route of its own - is still the one the
        // engine holds after it, so the frame published nothing.
        let mut planted = p.published_for_test().expect("a plan went out").clone();
        planted.routes = vec![InRoute { from: Slot(9), to: RouteEnd::Lane(Slot(10)), gain: 1.0 }];
        e.publish_plan(&planted).expect("the planted plan");
        let plans = p.stats().plans;
        p.sync(&app, &mut e);
        assert_eq!(p.stats().plans, plans, "a frame that changed nothing published a plan");
        seen.push(("unchanged".to_owned(), e.desk_readback()));
        // A fresh engine is primed whole.
        let mut fresh = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        p.engine_replaced();
        assert!(p.sync(&app, &mut fresh).primed, "a fresh engine was not primed");
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

    /// **A freeze, a thaw and a render-ahead change each reach a RUNNING
    /// engine with nothing else on the desk edited.** They used to ride
    /// inside the table publish, behind a change gate whose key held
    /// neither, so the engine's frozen bit waited for some unrelated route,
    /// key, bypass, mute, solo or send edit: until then a frozen lane's rack
    /// processed its already post-rack file a second time. A freeze and a
    /// thaw are order-deciding (a frozen lane's rack stands down), so each
    /// is ONE plan; the render-ahead is a scale and is stored with no plan
    /// at all.
    #[test]
    fn a_freeze_reaches_a_running_engine_with_nothing_else_edited() {
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        let mut s = blank();
        kind(&mut s, &[0, 1], ChannelKind::Audio);
        bay(&mut s, 1, 0, COMP_PATH);
        p.sync(&app_with(s.clone()), &mut e);
        let read = |e: &Engine, k: &str| e.desk_readback()[k].clone();
        assert_eq!(read(&e, "frozen"), serde_json::json!(0));
        let plans = p.stats().plans;

        s.rows[1].frozen = Some(Frozen { path: "media/f.wav".into(), len_s: 1.0 });
        p.sync(&app_with(s.clone()), &mut e);
        assert_eq!(read(&e, "frozen"), serde_json::json!(1 << 1), "a freeze waited for some other edit");
        assert_eq!(p.stats().plans, plans + 1, "a freeze is not one plan");

        s.render_ahead_ms = 10.0;
        p.sync(&app_with(s.clone()), &mut e);
        assert_eq!(read(&e, "grain"), serde_json::json!(RATE / 100), "a render-ahead change waited for some other edit");
        assert_eq!(p.stats().plans, plans + 1, "a render-ahead change rebuilt the plan");

        s.rows[1].frozen = None;
        p.sync(&app_with(s.clone()), &mut e);
        assert_eq!(read(&e, "frozen"), serde_json::json!(0), "a thaw waited for some other edit");
        assert_eq!(p.stats().plans, plans + 2, "a thaw is not one plan");
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

    fn curve(lane: impl Into<AutoLane>, param: AutoParam, points: &[(f64, f64, Shape)]) -> Curve {
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
        for (row, (gain, pan)) in s.rows.iter_mut().zip([(0.8, -0.5), (0.6, 0.3), (0.9, 0.0), (0.7, 0.8)]) {
            (row.gain, row.pan) = (gain, pan);
        }
        s.rows[3].mute = true;
        send(&mut s, 0, 0.5);
        send(&mut s, 1, 0.25);
        s.reverb_mix = 0.6;
        s.delay_mix = 0.3;
        s.master_gain = 0.9;
        let mut doc = Arrangement::default();
        for (lane, at) in [(0, 0.0), (1, 0.25), (2, 0.5), (3, 0.0)] {
            doc.add_clip(audio(lane, &format!("n{lane}.wav"), at));
        }
        // Lane 3 is muted: its freeze is the one that proves a mute on the desk
        // cannot silence the lane being frozen.
        out.push(Corpus { name: "faders-sends", settings: s, doc, live: vec![], kits: vec![], freeze: vec![1, 3] });

        // Dialled racks, a bypassed bay, polarity.
        let mut s = blank();
        kind(&mut s, &[0, 1, 2], ChannelKind::Audio);
        bay(&mut s, 0, 0, EQ_PATH);
        bay(&mut s, 1, 0, COMP_PATH);
        bay(&mut s, 1, 1, CHORUS_PATH);
        bay(&mut s, 2, 0, PLATE_PATH);
        s.bypass.insert(legacy_sb(&s, 1, 1));
        s.rows[0].phase = true;
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
        s.rows[1].folder = true;
        s.rows[2].in_folder = true;
        s.rows[3].in_folder = true;
        s.routes.add(Route::new(row(&s, 4), Target::Node(row(&s, 5))));
        s.routes.add(Route::new(row(&s, 0), Target::Hardware { first: 0, stereo: true }));
        bay(&mut s, 5, 0, EQ_PATH);
        bay(&mut s, 3, 0, COMP_PATH);
        s.insert_keys.set(legacy_sb(&s, 3, 0), Some(row(&s, 2)));
        for r in [2, 4, 5] {
            s.rows[r].solo = true;
        }
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
        s.rows[0].dx7.patch = 3;
        s.rows[1].keys.patch = 2;
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
            // The master's, by name (W6).
            curve(AutoLane::Master, AutoParam::Gain, &[(0.0, 0.9, Shape::Linear), (0.8, 0.4, Shape::Linear)]),
            curve(0, AutoParam::Plugin { bay: 0, id: trim }, &[(0.0, 0.2, Shape::Linear), (0.7, 0.9, Shape::Linear)]),
        ]);
        // The Trim at its defaults, and LIVE: with no live state the export
        // falls back to the shelf, which is the real home's sidecar outside
        // the sandbox (see `every_corpus_bay_is_live`).
        let live = vec![(0, 0, TRIM_PATH, dialled(TRIM_PATH, &[]))];
        out.push(Corpus { name: "automation", settings: s, doc, live, kits: vec![], freeze: vec![0] });

        // A frozen lane: its file plays, its clip and its rack stand down.
        let mut s = blank();
        kind(&mut s, &[0, 1], ChannelKind::Audio);
        bay(&mut s, 1, 0, COMP_PATH);
        s.rows[1].frozen = Some(Frozen { path: "media/frozen-02.wav".into(), len_s: 1.0 });
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

    /// The live engine the export reads its knobs from, and the bridge that
    /// drives it: the export finds each bay's live seat through it.
    fn live_engine(c: &Corpus, app: &TangentApp) -> (Engine, Bridge) {
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut b = Bridge::new();
        b.sync(app, &mut e);
        for (seat, bay, unit, state) in &c.live {
            let at = b.engine_seat(app, *seat, *bay).expect("the bay's lane");
            e.load_insert_at(at.0, at.1, Some(Path::new(unit)), Some(state))
                .unwrap_or_else(|err| panic!("{unit} did not load into seat {seat}: {err}"));
        }
        (e, b)
    }

    /// The DX7's voice for a strip, from the factory bank: the export's
    /// cartridge cache handed in, and a disk with no staged bank on it.
    fn factory_voice(app: &TangentApp, strip: usize) -> crate::dx7::Voice {
        let c = crate::dx7::sysex::factory();
        let (_, patch) = app.dx7_choice_of(strip);
        c.voices.get(patch).or_else(|| c.voices.first()).copied().unwrap_or_default()
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
        built: Built,
        app: &TangentApp,
        spec: &tangent_ui::ports::ProjectExportSpec,
        freeze: Option<usize>,
        dir: &Path,
    ) -> Vec<(String, Vec<u8>, bool)> {
        use tangent_record::wav::{Bext, SampleFormat, WavSpec, WavWriter, Wallclock};
        // Each row's stem is cut from its track's lane in the mirror, bound
        // here at the press as `start_offline_render` binds it.
        let Built { engine: mut e, bridge } = built;
        let lane = |row: usize| bridge.lane(row).map(|l| l.slot.0 as usize).expect("the row's lane");
        let rate = spec.rate;
        // The frames `start_offline_render` pulls, from the one function it
        // asks: the TIME row's span (or the song) plus the tail, at the
        // varispeed.
        let ratio = f64::from(app.varispeed_ratio()).clamp(0.5, 2.0);
        let (first, until) = crate::desktop::export_frames(spec.span, app.arr_span_s(), rate, ratio);
        e.set_transport(1, first);
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
        if let Some(row) = freeze {
            let name = format!("frozen-{:02}.wav", row + 1);
            let path = dir.join(&name);
            files.push((name, path.clone(), WavWriter::create(&path, wav_spec, &bext).expect("freeze"), lane(row), false));
        } else if spec.stems {
            std::fs::create_dir_all(dir.join(spec.stems_folder())).expect("stems dir");
            for (row, file) in stem_files(app, spec) {
                let name = format!("{}/{file}", spec.stems_folder());
                let path = dir.join(&name);
                files.push((name, path.clone(), WavWriter::create(&path, wav_spec, &bext).expect("stem"), lane(row), false));
            }
        }
        let freeze = freeze.map(lane);
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
            let wide = e.stem_frame_width().max(2);
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
                // The lane's own column of a tap armed for the written lanes.
                let col = e.stem_column(Slot(*lane as u32)).expect("the tap was armed for every written lane");
                for f in 0..frames {
                    let l = scratch[f * wide + col * 2];
                    let r = scratch[f * wide + col * 2 + 1];
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

    /// **Every effect bay a corpus desk seats has its state in the live
    /// engine.** A bay the live engine is not wearing is furnished from the
    /// shelf (`desk_insert_state` -> `shelf::saved_state`), and the shelf is
    /// `Settings::path().parent()`: the REAL `~/.config/tangent` in a binary
    /// crate's test, unless the run is sandboxed. A golden recorded in the
    /// sandbox would then depend on whatever the person running the suite
    /// last dialled into that bay, and a sidecar that will not load would
    /// trip `every_corpus_export_has_sound_in_it`'s refusal check. Instrument
    /// sentinels are not furnished as bays, so they need no state.
    fn every_corpus_bay_is_live(c: &Corpus, app: &TangentApp) {
        for (sb, path) in desk_bays(app) {
            if is_instrument_sentinel(&path) {
                continue;
            }
            let (seat, bay) = app_seat_of(&app.track_ids(), sb).expect("the bay's seat");
            assert!(
                c.live.iter().any(|(s, b, unit, _)| (*s, *b, *unit) == (seat, bay, path.as_str())),
                "{}: bay {sb} seats {path} with no live state, so its knobs would come from the real shelf",
                c.name
            );
        }
    }

    /// Every pass of every corpus desk: the master, the wet stems, the dry
    /// stems, and each named freeze, keyed `desk/pass/file`. Rendered once
    /// per test binary and shared by the two tests that read it: a debug
    /// render of the corpus is most of a minute.
    fn export_corpus() -> &'static BTreeMap<String, (String, bool)> {
        static CORPUS: std::sync::OnceLock<BTreeMap<String, (String, bool)>> = std::sync::OnceLock::new();
        CORPUS.get_or_init(render_corpus)
    }

    fn render_corpus() -> BTreeMap<String, (String, bool)> {
        let mut all = BTreeMap::new();
        for c in corpus() {
            let dir = scratch(c.name);
            let app = open(&c, &dir);
            every_corpus_bay_is_live(&c, &app);
            let (live, live_bridge) = live_engine(&c, &app);
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
                let build = match freeze {
                    Some(row) => BuildFor::Freeze(app.track_ids()[row]),
                    None => BuildFor::Export(spec.clone()),
                };
                let mut ctx = Furnish {
                    live: Some((&live, &live_bridge)),
                    dx7: (0..CHANNELS)
                        .filter(|ch| row_instrument(&app, *ch) == Some(tangent_ui::dialogs::BUILTIN_PATH))
                        .map(|ch| (ch, factory_voice(&app, ch)))
                        .collect(),
                    ..Furnish::default()
                };
                let built = full_build_with(&app, RATE, Timebase::new(), build, &mut ctx)
                    .expect("an offline engine");
                assert!(ctx.refused.is_empty(), "{}: a bay refused: {:?}", c.name, ctx.refused);
                assert!(ctx.unread.is_empty(), "{}: the corpus names a file it did not write: {:?}", c.name, ctx.unread);
                for (name, bytes, loud) in render_files(built, &app, &spec, freeze, &dir.join("out").join(&pass)) {
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
            export_corpus().iter().map(|(k, (h, _))| (k.clone(), h.clone())).collect();
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

    /// **A plan is published when something that decides an ORDER moved,
    /// and a gain that only scales is written into the plans in flight.**
    ///
    /// The pure half: a route's gain on the same side of zero is the same
    /// structure, a gain crossing zero, a route added or a lane's flag moved
    /// is not. The live half: an unchanged frame publishes nothing, a route
    /// gain moved publishes nothing and lands in the playing plan's gains,
    /// and a mute publishes exactly one plan.
    #[test]
    fn the_plan_is_published_on_change_only() {
        let lane = |s: u32| InLane {
            slot: Slot(s),
            gen: 1,
            flags: LaneFlags::default(),
            parent: None,
            punch: None,
            midi: MidiRoute::EVERYTHING,
            pick: None,
        };
        let route = |gain: f32| InRoute { from: Slot(0), to: RouteEnd::Lane(Slot(1)), gain };
        let at = PlanInputs { lanes: vec![lane(0), lane(1)], routes: vec![route(1.0)], ..PlanInputs::default() };
        let scaled = PlanInputs { routes: vec![route(0.25)], ..at.clone() };
        assert_eq!(structure(&at), structure(&scaled), "a gain on the same side of zero read as a new order");
        let shut = PlanInputs { routes: vec![route(0.0)], ..at.clone() };
        assert_ne!(structure(&at), structure(&shut), "a gain crossing zero read as a scale");
        let more = PlanInputs { routes: vec![route(1.0), route(0.5)], ..at.clone() };
        assert_ne!(structure(&at), structure(&more), "a route added read as a scale");
        let mut muted = at.clone();
        muted.lanes[1].flags = LaneFlags::MUTED;
        assert_ne!(structure(&at), structure(&muted), "a mute read as a scale");

        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        let mut s = blank();
        kind(&mut s, &[0, 1], ChannelKind::Audio);
        s.routes.add(Route::new(row(&s, 0), Target::Node(row(&s, 1))));
        p.sync(&app_with(s.clone()), &mut e);
        let plans = p.stats().plans;
        p.sync(&app_with(s.clone()), &mut e);
        assert_eq!(p.stats().plans, plans, "an unchanged frame published a plan");
        s.routes.set_gain(0, 0.5);
        p.sync(&app_with(s.clone()), &mut e);
        assert_eq!(p.stats().plans, plans, "a route's gain rebuilt the plan");
        e.settle_offline();
        let gains = e.plan_gains().expect("a plan plays");
        assert_eq!(f32::from_bits(gains[0].load(std::sync::atomic::Ordering::Relaxed)), 0.5, "the gain never reached the plan");
        s.rows[1].mute = true;
        p.sync(&app_with(s.clone()), &mut e);
        assert_eq!(p.stats().plans, plans + 1, "a mute is not one plan");
    }

    /// And the sync's wiring, by scan: the desk is built once a frame, the
    /// shape goes out through the one change test (`publish`), and the
    /// bypass table is stored only when it moved.
    #[test]
    fn the_sync_publishes_the_shape_through_one_change_test() {
        let src = include_str!("engine_bridge.rs");
        let src = &src[..src.find("#[cfg(test)]\nmod tests {").expect("the tests module")];
        let at = src.find("fn sync_shape(&mut self").expect("Bridge::sync_shape");
        let body = &src[at..at + src[at..].find("\n    }\n").expect("its end")];
        // The strip-space desk the host's take facts were once relayed is
        // gone (W6): the bridge reads the desk by what each strip is.
        assert_eq!(body.matches("app.desk()").count(), 0, "the strip-space desk is built again");
        assert_eq!(body.matches("e.publish_plan(").count(), 0, "a plan goes out past the change test");
        assert_eq!(body.matches("self.publish(e, inputs)").count(), 1, "the shape is not published through the change test");
        let gate = body.find("if self.bypass.as_ref() != Some(&bypass) {").expect("the bypass compare");
        let store = body.find("e.set_insert_bypass_at(&bypass);").expect("the bypass store");
        assert!(gate < store, "the bypass is stored every frame");
    }

    /// **The probe hears the session's arm; an export and a freeze hear
    /// nothing live.** The one documented difference between the two
    /// builders that used to be two hand copies, now a `BuildFor` arm:
    /// through the contract's own entry point, an armed MIDI lane and an
    /// open lane are what the probe's engine is told, and every lane is
    /// deaf and unarmed in an export's and a freeze's.
    #[test]
    fn the_probe_hears_the_arm_and_an_export_hears_nothing_live() {
        let mut s = blank();
        kind(&mut s, &[0, 1], ChannelKind::Midi);
        kind(&mut s, &[2], ChannelKind::Audio);
        s.rows[1].arm = true;
        s.rows[2].arm = true;
        let app = app_with(s);
        let read = |build: BuildFor| {
            let built = full_build(&app, RATE, Timebase::new(), build).expect("an offline engine");
            let r = built.engine.desk_readback();
            (r["armed"].as_u64().expect("armed"), r["midi_off"].as_u64().expect("midi_off"))
        };
        let (armed, off) = read(BuildFor::Probe);
        assert_eq!(armed, 1 << 1 | 1 << 2, "the probe was not told the session's arm");
        assert_eq!(off & 0b11, 0b01, "the probe's armed MIDI lane is not the one listening");
        // Every lane deaf, and the four strips that are not rows too (their
        // bits past the lanes'): the plan has no word bits past the strips,
        // where the shim's `u64::MAX` had them all.
        let every = (1u64 << (STRIPS + 1)) - 1;
        for build in [BuildFor::Export(ExportSpec::default()), BuildFor::Freeze(app.track_ids()[1])] {
            let (armed, off) = read(build.clone());
            assert_eq!((armed, off & every), (0, every), "{build:?} hears something live");
        }
    }

    /// **A row past the old words carries its own flags** (W6): the arm,
    /// mute, solo, polarity and live-MIDI rule of row forty reach its own
    /// lane's plan entry, and no other row's. The words the bridge read them
    /// from stopped at bit 31 (the arm) and 63 (the masks), so a row past
    /// them had no bit to give; each is a row's own fact now, read by row.
    /// The replacement for `a_row_bit_is_its_own_or_none`, whose words are
    /// gone with the fixed desk.
    #[test]
    fn a_row_past_the_old_words_carries_its_own_flags() {
        let mut s = blank();
        let mut next = s.next_track_id;
        while s.rows.len() < 41 {
            s.rows.push(tangent_ui::desk_model::TrackRow::vacant(TrackId(next as u32)));
            next += 1;
        }
        s.next_track_id = next;
        kind(&mut s, &(0..41).collect::<Vec<_>>(), ChannelKind::Midi);
        let far = &mut s.rows[40];
        (far.arm, far.mute, far.solo, far.phase) = (true, true, true, true);
        let app = app_with(s);
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        p.sync(&app, &mut e);
        let lane = |row: usize| p.lane(row).unwrap_or_else(|| panic!("row {row} has no lane"));
        let plan = p.published_for_test().expect("a plan went out");
        let flags = |row: usize| {
            let at = lane(row).slot;
            plan.lanes.iter().find(|l| l.slot == at).map(|l| l.flags).expect("the row's lane is in the plan")
        };
        use LaneFlags as F;
        for f in [F::ARMED, F::MUTED, F::SOLOED, F::PHASE] {
            assert!(flags(40).contains(f), "row forty lost {f:?}");
            assert!(!flags(39).contains(f), "row thirty-nine took row forty's {f:?}");
            assert!(!flags(8).contains(f), "row eight took row forty's {f:?}");
        }
        // Armed MIDI hears the keys; every other lane is deaf.
        assert!(!flags(40).contains(F::MIDI_OFF), "the armed row forty took no notes");
        assert!(flags(39).contains(F::MIDI_OFF), "an unarmed row heard the keys");
    }

    /// **A curve on a strip's send to a bus is that send's curve, between
    /// its two lanes, at any row** (W7-slot-gaps item 5, the bridge's half
    /// of `AutoParam::SendTo`, decoded by [`send_bus_of`] as the live push
    /// and the export decode it). A curve on each of rows 1, 31, 32, 33, 40
    /// and 200 on its send to row 120's bus is a [`SendCurve`] from that
    /// row's lane to row 120's, stamped with the source lane's generation,
    /// on the post-fader side its curve belongs to. A curve on a send to a
    /// track the desk does not hold, the master's, and a control that names
    /// no bus (the effects return's `Send` rides the lane's own curves) are
    /// none.
    #[test]
    fn a_send_curve_runs_between_its_two_lanes_at_any_row() {
        const ROWS: [usize; 6] = [1, 31, 32, 33, 40, 200];
        let mut s = blank();
        let mut next = s.next_track_id;
        while s.rows.len() < 201 {
            s.rows.push(tangent_ui::desk_model::TrackRow::vacant(TrackId(next as u32)));
            next += 1;
        }
        s.next_track_id = next;
        kind(&mut s, &[1, 31, 32, 33, 40, 200], ChannelKind::Audio);
        kind(&mut s, &[120], ChannelKind::Bus);
        let app = app_with(s);
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        p.sync(&app, &mut e);
        let bus = app.track_ids()[120];
        let gone = TrackId(u32::MAX - 1);
        assert!(app.row_of(gone).is_none(), "fixture: the gone track is on the desk");
        let flat = |lane: AutoLane, param| Curve::sanitised(lane, param, vec![Point { at_s: 0.0, value: 0.5, shape: Shape::Hold }]);
        let mut curves: Vec<Curve> = ROWS.iter().map(|r| flat(AutoLane::Row(*r), AutoParam::SendTo { to: bus })).collect();
        curves.push(flat(AutoLane::Row(40), AutoParam::SendTo { to: gone }));
        curves.push(flat(AutoLane::Row(33), AutoParam::Send));
        curves.push(flat(AutoLane::Row(32), AutoParam::Gain));
        curves.push(flat(AutoLane::Master, AutoParam::SendTo { to: bus }));
        let got = p.send_curves(&Automation::from_curves(curves), &send_bus_of);
        let to = SendTo::Lane(p.lane_of(bus).expect("the bus's lane").slot);
        let have: Vec<(SendFrom, SendTo, bool, u32)> = got.iter().map(|c| (c.from, c.to, c.pre, c.gen)).collect();
        assert_eq!(have.len(), ROWS.len(), "not one send curve a row: {have:?}");
        for row in ROWS {
            let l = p.lane(row).expect("a row's lane");
            assert!(
                have.contains(&(SendFrom::Lane(l.slot), to, false, l.gen)),
                "row {row}'s curve is no post-fader send from its lane to the bus's: {have:?}"
            );
        }
        assert!(
            got.iter().all(|c| c.curve.param == AutoParam::SendTo { to: bus }),
            "a curve that names no bus on this desk became a send curve"
        );
    }

    /// **A key to a comp on a bus, and a key from a bus, are wired by
    /// identity at any row** (W7-join item 2, the bridge's half of a key to
    /// a bus; the sound is `slot_gaps`' and the engine's door
    /// `gaps::a_key_from_lane_40_reaches_a_comp_on_33_and_on_a_bus`). Rows
    /// 1, 31, 32, 33, 40 and 200 are buses, each with a comp in its first
    /// bay keyed from row 150; row 7's comp is keyed from bus 120. Every bus
    /// is a BUS lane, and the plan carries each key as an `InKey` on that
    /// lane's own first rack, from its source's lane, and no other key. A
    /// reorder that moves every subject hands the engine the same keys on
    /// the same lanes.
    #[test]
    fn a_key_to_or_from_a_bus_is_wired_by_identity_at_any_row() {
        const BUSES: [usize; 6] = [1, 31, 32, 33, 40, 200];
        let mut s = blank();
        let mut next = s.next_track_id;
        while s.rows.len() < 201 {
            s.rows.push(tangent_ui::desk_model::TrackRow::vacant(TrackId(next as u32)));
            next += 1;
        }
        s.next_track_id = next;
        kind(&mut s, &BUSES, ChannelKind::Bus);
        kind(&mut s, &[120], ChannelKind::Bus);
        kind(&mut s, &[7, 150], ChannelKind::Audio);
        let comp = |s: &Settings, n: usize| SeatBay::new(row(s, n), Rack::Chain(0), 0);
        let mut keyed: Vec<(SeatBay, Node)> = BUSES.iter().map(|&b| (comp(&s, b), row(&s, 150))).collect();
        keyed.push((comp(&s, 7), row(&s, 120)));
        for &(sb, from) in &keyed {
            s.plugins.insert(sb, COMP_PATH.to_owned());
            s.insert_keys.set(sb, Some(from));
        }
        let mut app = app_with(s);
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        p.sync(&app, &mut e);
        let wired = |p: &Bridge| -> Vec<InKey> {
            let mut want: Vec<InKey> = keyed
                .iter()
                .map(|&(sb, from)| {
                    let (addr, bay) = p.engine_bay(sb).expect("the comp's bay");
                    let RackOwner::Lane(at) = addr.owner else { panic!("{sb} is no lane's bay") };
                    assert_eq!(Some(at), sb.seat.node.track().and_then(|id| p.lane_of(id)).map(|l| l.slot), "{sb} is wired to another lane");
                    let from = p.lane_of(from.track().expect("a track")).expect("the source's lane").slot;
                    InKey { seat: addr.plan_seat().expect("a plan seat"), bay: bay as u8, from }
                })
                .collect();
            want.sort_by_key(|k| (k.seat, k.bay, k.from));
            want
        };
        let played = |p: &Bridge| -> Vec<InKey> {
            let mut got = p.published_for_test().expect("a plan").keys.clone();
            got.sort_by_key(|k| (k.seat, k.bay, k.from));
            got
        };
        let plan = p.published_for_test().expect("a plan");
        for b in BUSES.into_iter().chain([120]) {
            let at = p.lane(b).expect("a bus lane").slot;
            assert!(
                plan.lanes.iter().any(|l| l.slot == at && l.flags.contains(LaneFlags::BUS)),
                "row {b}'s bus is not a BUS lane"
            );
        }
        assert_eq!(played(&p), wired(&p), "the plan's keys are not the desk's");
        // Row 200 dragged to the top: every subject moves a row, and every key
        // stays on its own lanes.
        let before = played(&p);
        app.move_channel(200, 0);
        p.sync(&app, &mut e);
        assert_eq!(played(&p), wired(&p), "the plan's keys are not the desk's after a reorder");
        assert_eq!(played(&p), before, "a reorder moved a key in the plan");
    }

    /// **A lane that becomes a drum lane is owed its kit, the frame it does**
    /// (W7-join item 4, `Synced::drummed`). On a fresh engine every drum
    /// lane is new to it; a steady frame owes nothing; the drumpad picked
    /// into a track's first bay owes that track's lane its kit, once, and
    /// owes it again when it leaves and comes back, as an undo or a swap
    /// there and back brings it. Before, the pick moved no lane and edited
    /// no kit, so with another kit landed the host reloaded nothing.
    #[test]
    fn a_lane_that_takes_the_drumpad_is_owed_its_kit_the_frame_it_does() {
        let mut s = blank();
        kind(&mut s, &[0, 1, 2], ChannelKind::Midi);
        let first_bay = |s: &Settings, n: usize| SeatBay::new(row(s, n), Rack::Chain(0), 0);
        s.plugins.insert(first_bay(&s, 0), tangent_ui::dialogs::DRUMPAD_PATH.to_owned());
        s.plugins.insert(first_bay(&s, 1), tangent_ui::dialogs::KEYS_PATH.to_owned());
        let bay_1 = first_bay(&s, 1);
        let mut app = app_with(s);
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        let fresh = p.sync(&app, &mut e);
        let lane = |p: &Bridge, n: usize| p.lane(n).expect("a lane").slot;
        assert_eq!(fresh.drummed, vec![lane(&p, 0)], "a fresh engine's drum lane is not owed its kit");
        let steady = p.sync(&app, &mut e);
        assert!(steady.drummed.is_empty() && !steady.owes_kits(), "a steady frame owes kits: {steady:?}");
        app.set_plugin(bay_1, Some(tangent_ui::dialogs::DRUMPAD_PATH));
        let picked = p.sync(&app, &mut e);
        assert!(picked.moved.is_empty(), "fixture: the pick moved a lane, which owes the kits on its own");
        assert_eq!(picked.drummed, vec![lane(&p, 1)], "the lane that took the drumpad is not owed its kit");
        assert!(picked.owes_kits());
        assert!(p.sync(&app, &mut e).drummed.is_empty(), "the pick was owed twice");
        app.set_plugin(bay_1, Some(tangent_ui::dialogs::KEYS_PATH));
        assert!(p.sync(&app, &mut e).drummed.is_empty(), "a lane that let the drumpad go was owed a kit");
        app.set_plugin(bay_1, Some(tangent_ui::dialogs::DRUMPAD_PATH));
        assert_eq!(p.sync(&app, &mut e).drummed, vec![lane(&p, 1)], "the drumpad back is not owed its kit again");
    }

    /// **Both offline builders are one builder, and the host pushes no desk
    /// structure of its own.** `start_offline_render` and the probe each
    /// build through `furnish_offline_with`, the export as `Export` or
    /// `Freeze` and the probe as `Probe`; and outside its tests desktop.rs
    /// calls none of the setters `Bridge::sync` took over, so a second copy
    /// of the desk's push cannot grow back there. Whole-line comments are
    /// skipped, so a sentence ABOUT a setter is not read as a call.
    #[test]
    fn the_desktop_pushes_the_desk_only_through_the_bridge() {
        let whole = include_str!("desktop.rs");
        let at = whole.find("\nmod tests {").expect("desktop's tests");
        let (src, tests) = (&whole[..at], &whole[at..]);
        let body_of = |src: &'static str, name: &str| -> &'static str {
            let at = src.find(name).unwrap_or_else(|| panic!("`{name}` is gone"));
            let end = src[at + 1..].find("\n    fn ").map_or(src.len(), |n| at + 1 + n);
            &src[at..end]
        };
        let render = body_of(src, "fn start_offline_render(");
        assert!(render.contains("crate::engine_bridge::full_build_with("), "the export builds its own engine");
        assert!(render.contains("crate::engine_bridge::BuildFor::Freeze("), "a freeze is not a freeze arm");
        assert!(render.contains("crate::engine_bridge::BuildFor::Export("), "an export is not an export arm");
        let probe = body_of(tests, "fn furnish_probe_engine(");
        assert!(probe.contains("crate::engine_bridge::full_build_with("), "the probe builds its own engine");
        assert!(probe.contains("crate::engine_bridge::BuildFor::Probe"), "the probe is not the probe arm");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .map(|l| format!("{l}\n"))
            .collect();
        // The desk's shape, its lanes and its per-lane numbers are the
        // bridge's alone: the shim setters (test-only now) and the slot API's
        // structural calls alike.
        let moved = [
            "set_desk(", "set_armed(", "set_frozen(", "set_midi_off(", "set_drum_strips(",
            "set_builtin_strips(", "set_keys_strips(", "set_auto_suspend(", "set_strip_phase(",
            "set_folders(", "set_punches(", "set_midi_routes(", "set_routes(", "set_insert_keys(",
            "set_insert_bypass(", "set_grain_ms(", "install_lanes(", "remove_lanes(", "publish_plan(",
            "set_lane_levels(", "set_special_sends(", "sync_instruments(", "set_channel_gain(",
            "set_channel_pan(", "set_keys_patch(", "set_keys_params(",
        ];
        let mut direct = Vec::new();
        for name in moved {
            for (at, _) in code.match_indices(&format!(".{name}")) {
                // The app's own setters of the same name are the desk being
                // EDITED, not pushed.
                if !code[..at].ends_with("self.app") {
                    let line = code[..at].rfind('\n').map_or(0, |n| n + 1);
                    let end = at + code[at..].find('\n').unwrap_or(code.len() - at);
                    direct.push(code[line..end].trim().to_owned());
                }
            }
        }
        assert!(direct.is_empty(), "desktop.rs pushes desk structure itself: {direct:#?}");
    }

    /// **What a steady frame of the bridge costs**, release, on an offline
    /// engine: the plan's inputs built from the app and compared against the
    /// last plan's, the levels stored, no plan sent. `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn what_a_steady_sync_costs_per_frame_probe() {
        use std::time::Instant;
        let mut s = blank();
        kind(&mut s, &(0..CHANNELS).collect::<Vec<_>>(), ChannelKind::Audio);
        for ch in 0..8 {
            s.routes.add(Route::new(row(&s, ch), Target::Hardware { first: 0, stereo: true }));
        }
        let app = app_with(s);
        let mut e = Engine::offline(48_000, Timebase::new()).expect("offline engine");
        let mut p = Bridge::new();
        p.sync(&app, &mut e);
        let n = 2_000u32;
        let t = Instant::now();
        for _ in 0..n {
            p.sync(&app, &mut e);
        }
        let per = t.elapsed().as_secs_f64() * 1e6 / f64::from(n);
        eprintln!("a steady sync of {CHANNELS} lanes: {per:.1} us a frame ({} plans in all)", p.stats().plans);
    }

    // ── the broker ─────────────────────────────────────────────────────────

    /// One frame of the app with one key down, the way the app's own tests
    /// press Ctrl/Cmd+Z: through `frame`, so the undo is the app's own.
    fn key(app: &mut TangentApp, key: egui::Key, mods: egui::Modifiers) {
        let ctx = egui::Context::default();
        app.install_fonts(&ctx);
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1300.0, 900.0))),
                modifiers: mods,
                events: vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: mods }],
                ..Default::default()
            },
            |ctx| app.frame(ctx),
        );
    }

    /// **The bays the desk wants, at the engine's address**: the host's
    /// reconcile, without its overlay. Every bay the app holds, at its
    /// track's lane.
    fn wanted(p: &Bridge, app: &TangentApp) -> BTreeMap<EngineBay, String> {
        desk_bays(app).into_iter().filter_map(|(sb, path)| Some((p.engine_bay(sb)?, path))).collect()
    }

    /// **A pad-FX swap carries both instances and both state files**
    /// (W5-identity-maps). The app moves the NAMES and hands the host the
    /// swap as two relabels; `carry_relabels` reads BOTH sources before it
    /// writes either destination, each from the instance playing in the
    /// engine where it is that plugin (its knobs have moved since its file
    /// was written), and each pad's file ends up holding the other pad's
    /// plugin as it was playing. Two different units, so a carry that wrote
    /// a file twice, or read a destination it had just written, shows as the
    /// wrong bytes under the wrong owner. With no engine (the undo's road
    /// when nothing is playing) the files themselves cross back. Every file
    /// is on this test's own scratch shelf.
    #[test]
    fn a_pad_fx_swap_carries_both_instances_and_both_state_files() {
        use tangent_ui::desk_id::BayRelabel;
        let shelf = scratch("padswap").join("state");
        let mut s = blank();
        kind(&mut s, &[0, 1], ChannelKind::Audio);
        let drums = row(&s, 1);
        let (kick, snare) = (SeatBay::new(drums, Rack::Pad(36), 0), SeatBay::new(drums, Rack::Pad(38), 0));
        s.plugins.insert(kick, COMP_PATH.to_owned());
        s.plugins.insert(snare, EQ_PATH.to_owned());
        let before = app_with(s.clone());
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        p.sync(&before, &mut e);
        let comp_at = p.engine_bay(kick).expect("the kick pad's bay");
        let eq_at = p.engine_bay(snare).expect("the snare pad's bay");
        e.load_insert_at(comp_at.0, comp_at.1, Some(Path::new(COMP_PATH)), Some(dialled(COMP_PATH, &[(0, 0.2), (1, 0.9)]).as_slice()))
            .expect("the comp loads");
        e.load_insert_at(eq_at.0, eq_at.1, Some(Path::new(EQ_PATH)), Some(dialled(EQ_PATH, &[(0, 0.8)]).as_slice()))
            .expect("the EQ loads");
        e.settle_offline();
        let comp_live = e.save_insert_state_at(comp_at.0, comp_at.1).expect("the comp's live state");
        let eq_live = e.save_insert_state_at(eq_at.0, eq_at.1).expect("the EQ's live state");
        // Each pad's file is OLDER than what is playing: the unit's
        // defaults, as written when it was seated.
        let (comp_file, eq_file) = (dialled(COMP_PATH, &[]), dialled(EQ_PATH, &[]));
        assert!(crate::shelf::write_state_in(&shelf, kick, COMP_PATH, &comp_file), "fixture: the kick's file");
        assert!(crate::shelf::write_state_in(&shelf, snare, EQ_PATH, &eq_file), "fixture: the snare's file");
        assert_ne!(comp_live, comp_file, "fixture: the comp plays its file");
        assert_ne!(eq_live, eq_file, "fixture: the EQ plays its file");

        // The swap as the app makes it: the names trade, two relabels.
        let mut swapped = s.clone();
        swapped.plugins.insert(kick, EQ_PATH.to_owned());
        swapped.plugins.insert(snare, COMP_PATH.to_owned());
        let after = app_with(swapped);
        let swap = [BayRelabel { from: kick, to: snare }, BayRelabel { from: snare, to: kick }];
        let none = crate::shelf::DisplacedShelf::new();
        let mut written = carry_relabels(&after, &p, Some(&e), &none, &swap, &shelf);
        written.sort();
        assert_eq!(
            written,
            vec![(kick, EQ_PATH.to_owned()), (snare, COMP_PATH.to_owned())],
            "the swap did not write both destinations, each under its new tenant"
        );
        assert_eq!(
            crate::shelf::read_state(&shelf, snare, COMP_PATH),
            Some(comp_live.clone()),
            "the snare's file does not hold the comp as it was playing"
        );
        assert_eq!(
            crate::shelf::read_state(&shelf, kick, EQ_PATH),
            Some(eq_live.clone()),
            "the kick's file does not hold the EQ as it was playing"
        );

        // The undo, with nothing playing: the inverse relabels, read off the
        // files the swap wrote, and every byte goes home.
        let back = swap.map(BayRelabel::inverse);
        let mut written = carry_relabels(&before, &p, None, &none, &back, &shelf);
        written.sort();
        assert_eq!(written, vec![(kick, COMP_PATH.to_owned()), (snare, EQ_PATH.to_owned())]);
        assert_eq!(crate::shelf::read_state(&shelf, kick, COMP_PATH), Some(comp_live), "the undo lost the comp");
        assert_eq!(crate::shelf::read_state(&shelf, snare, EQ_PATH), Some(eq_live), "the undo lost the EQ");
        let _ = std::fs::remove_dir_all(shelf.parent().expect("the scratch"));
    }

    /// **The host's reconcile over `bays`, without its overlay**: what
    /// `DesktopApp`'s does to a bay whose wanted tenant the engine is not
    /// playing, or that a carry marked to rebuild (`rebuild`). The outgoing
    /// tenant's PLAYING settings are shelved under its bay and bundle when a
    /// different one is coming in (`shelve_displaced`), the incoming one is
    /// seated from the shelf first and its file second (`incoming_state`),
    /// and a shelved state is spent once it is seated.
    fn reconcile(
        e: &mut Engine,
        p: &Bridge,
        app: &TangentApp,
        displaced: &mut crate::shelf::DisplacedShelf,
        shelf: &Path,
        bays: &[SeatBay],
        rebuild: &[SeatBay],
    ) {
        for &sb in bays {
            let at = p.engine_bay(sb).expect("a bay with a lane");
            let want = app.plugin(sb).map(str::to_owned);
            let gone = e.insert_at(at.0, at.1).and_then(|l| l.bundle.to_str()).map(str::to_owned);
            if gone == want && !rebuild.contains(&sb) {
                continue;
            }
            if let Some(gone) = gone.filter(|g| want.as_deref() != Some(g.as_str())) {
                if let Some(blob) = e.save_insert_state_at(at.0, at.1) {
                    crate::shelf::shelve_displaced(displaced, (sb, gone), blob);
                }
            }
            let state = want.as_deref().and_then(|w| {
                crate::shelf::incoming_state(displaced, &(sb, w.to_owned()), || crate::shelf::read_state(shelf, sb, w))
            });
            e.load_insert_at(at.0, at.1, want.as_deref().map(Path::new), state.as_deref()).expect("a bay loads");
            if let Some(w) = want {
                displaced.remove(&(sb, w));
            }
        }
        take_handoffs(e);
    }

    /// **The host's half of a carry** (`DesktopApp`'s relabel block): what
    /// was written outranks anything shelved for it there, and every bay
    /// written is rebuilt.
    fn land(written: Vec<(SeatBay, String)>, displaced: &mut crate::shelf::DisplacedShelf) -> Vec<SeatBay> {
        for (sb, bundle) in &written {
            displaced.remove(&(*sb, bundle.clone()));
        }
        written.into_iter().map(|(sb, _)| sb).collect()
    }

    /// **One host frame over `bays`**: the relabels the app handed over,
    /// carried and landed, then the reconcile.
    fn host_frame(
        app: &mut TangentApp,
        p: &Bridge,
        e: &mut Engine,
        displaced: &mut crate::shelf::DisplacedShelf,
        shelf: &Path,
        bays: &[SeatBay],
    ) {
        let relabels = app.take_bay_relabels();
        let rebuild = land(carry_relabels(app, p, Some(e), displaced, &relabels, shelf), displaced);
        reconcile(e, p, app, displaced, shelf, bays, &rebuild);
    }

    /// One block rendered, so the callback takes every instance handed to a
    /// bay's ring and retires the one it replaced: a bay's ring is two deep,
    /// and a test that seats one bay three times without a callback between
    /// finds it full, as a stalled callback would.
    fn take_handoffs(e: &mut Engine) {
        e.settle_offline();
        let mut out = vec![0.0f32; 256];
        e.render_offline(&mut out);
        e.settle_offline();
    }

    /// **A carry takes a moved plugin's newest settings, the displaced
    /// shelf's before its file** (W5-identity-maps, after review). A comp
    /// and an EQ trade bays; the comp is dialled in its new bay and then
    /// replaced there by a plate; one step back crosses both moments. The
    /// swap's inverse relabels are all there is to carry (a replace has
    /// none), and they are carried while the plate still plays where the
    /// comp went: the comp is playing nowhere, its new bay's FILE still holds
    /// what the swap wrote, and the dial lives only on the displaced shelf,
    /// where the replace put it. Read from the file, the comp came home
    /// without the dial; read from the shelf (`incoming_state`'s own order),
    /// it comes home dialled, and the rebuild seats it so.
    #[test]
    fn a_carry_takes_the_displaced_shelfs_dial_before_the_file() {
        use tangent_ui::desk_id::BayRelabel;
        let shelf = scratch("relabel-shelf").join("state");
        let mut s = blank();
        kind(&mut s, &[0, 1], ChannelKind::Audio);
        let track = row(&s, 1);
        let (a, b) = (SeatBay::new(track, Rack::Chain(0), 0), SeatBay::new(track, Rack::Chain(0), 1));
        s.plugins.insert(a, COMP_PATH.to_owned());
        s.plugins.insert(b, EQ_PATH.to_owned());
        let before = app_with(s.clone());
        let mut swapped = s.clone();
        swapped.plugins.insert(a, EQ_PATH.to_owned());
        swapped.plugins.insert(b, COMP_PATH.to_owned());
        let after = app_with(swapped.clone());
        let mut replaced = swapped;
        replaced.plugins.insert(b, PLATE_PATH.to_owned());
        let last = app_with(replaced);

        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        p.sync(&before, &mut e);
        let (at_a, at_b) = (p.engine_bay(a).expect("bay a"), p.engine_bay(b).expect("bay b"));
        let mut displaced = crate::shelf::DisplacedShelf::new();
        e.load_insert_at(at_a.0, at_a.1, Some(Path::new(COMP_PATH)), Some(dialled(COMP_PATH, &[(0, 0.2)]).as_slice()))
            .expect("the comp loads");
        e.load_insert_at(at_b.0, at_b.1, Some(Path::new(EQ_PATH)), Some(dialled(EQ_PATH, &[(0, 0.8)]).as_slice()))
            .expect("the EQ loads");
        take_handoffs(&mut e);
        // The swap, carried and rebuilt.
        let swap = [BayRelabel { from: a, to: b }, BayRelabel { from: b, to: a }];
        let rebuild = land(carry_relabels(&after, &p, Some(&e), &displaced, &swap, &shelf), &mut displaced);
        reconcile(&mut e, &p, &after, &mut displaced, &shelf, &[a, b], &rebuild);
        let swap_file = crate::shelf::read_state(&shelf, b, COMP_PATH).expect("the swap wrote the comp's file");
        // The comp dialled where it landed; nothing writes its file.
        e.load_insert_at(at_b.0, at_b.1, Some(Path::new(COMP_PATH)), Some(dialled(COMP_PATH, &[(0, 0.9), (1, 0.1)]).as_slice()))
            .expect("the dial");
        take_handoffs(&mut e);
        let dial = e.save_insert_state_at(at_b.0, at_b.1).expect("the comp's dial");
        assert_ne!(dial, swap_file, "fixture: the dial is what the swap wrote");
        // Replaced there by a plate: the reconcile shelves the dial.
        reconcile(&mut e, &p, &last, &mut displaced, &shelf, &[a, b], &[]);

        // One step back across both moments, the plate still playing.
        let back = swap.map(BayRelabel::inverse);
        let written = carry_relabels(&before, &p, Some(&e), &displaced, &back, &shelf);
        assert!(written.contains(&(a, COMP_PATH.to_owned())), "the step back did not carry the comp home");
        assert_eq!(
            crate::shelf::read_state(&shelf, a, COMP_PATH),
            Some(dial.clone()),
            "the comp came home without its dial: the carry read its file and not the displaced shelf"
        );
        let rebuild = land(written, &mut displaced);
        reconcile(&mut e, &p, &before, &mut displaced, &shelf, &[a, b], &rebuild);
        assert_eq!(e.save_insert_state_at(at_a.0, at_a.1), Some(dial), "the comp plays in its own bay without its dial");
        let _ = std::fs::remove_dir_all(shelf.parent().expect("the scratch"));
    }

    /// **The specials reach the engine by what each is** (W5-identity-maps:
    /// the specials left strip space, and the Strip/Desk pair no longer
    /// feeds the bridge). Each special's own mute, solo, polarity and send,
    /// set on the desk by identity and nothing else, lands on that special's
    /// bits of the plan the engine holds (the engine's own words keep the
    /// specials at strips 32 to 35, `desk_legacy::v32_strip`) and on its send
    /// cell; a row's mute lands on its own lane's bit, and nothing lands on
    /// anybody else's.
    #[test]
    fn the_specials_reach_the_engine_by_what_each_is() {
        use tangent_ui::desk_legacy::v32_strip;
        use tangent_ui::settings::StripSwitch as W;
        let mut s = blank();
        kind(&mut s, &[0, 1], ChannelKind::Audio);
        s.set_strip_switch(StripRef::Click, W::Mute, true);
        s.set_strip_switch(StripRef::Fx, W::Solo, true);
        s.set_strip_switch(StripRef::Drums, W::Phase, true);
        s.set_strip_switch(StripRef::Master, W::Phase, true);
        s.set_strip_switch(StripRef::Row(1), W::Mute, true);
        s.set_strip_send(StripRef::Drums, 0.375);
        s.set_strip_send(StripRef::Click, 0.25);
        s.set_strip_send(StripRef::Row(0), 0.5);
        let on = |f: fn(&tangent_ui::desk_model::TrackRow) -> bool| s.rows.iter().enumerate().filter(|(_, r)| f(r)).map(|(n, _)| n).collect::<Vec<_>>();
        assert_eq!((on(|r| r.mute), on(|r| r.solo), on(|r| r.phase)), (vec![1], vec![], vec![]), "fixture: a special sat on a row");
        let app = app_with(s);
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        p.sync(&app, &mut e);
        let words = e.desk_readback();
        let at = |sr: StripRef| v32_strip(sr).expect("a strip the engine keeps");
        let bit = |sr: StripRef| 1u64 << at(sr);
        assert_eq!(words["muted"].as_u64(), Some(bit(StripRef::Click) | bit(StripRef::Row(1))), "the mutes");
        assert_eq!(words["soloed"].as_u64(), Some(bit(StripRef::Fx)), "the solos");
        assert_eq!(words["strip_phase"].as_u64(), Some(bit(StripRef::Drums) | bit(StripRef::Master)), "the polarity");
        let send = |sr: StripRef| {
            words["send"][at(sr)].as_u64().and_then(|b| u32::try_from(b).ok()).map(f32::from_bits)
        };
        assert_eq!(send(StripRef::Drums), Some(0.375), "the drum bus's send");
        assert_eq!(send(StripRef::Click), Some(0.25), "the click's send");
        assert_eq!(send(StripRef::Row(0)), Some(0.5), "row 0's send");
        assert_eq!(e.plan_refused_for_test(), 0);
    }

    /// **An FX swap's undo and redo carry each unit with what it was dialled
    /// to since** (W5-identity-maps, after review: an FX drag or swap is a
    /// bay move, so it is relabels, design 8). A comp and an EQ trade bays
    /// through the app's own trade, the comp is dialled in its new bay, and
    /// Ctrl+Z puts the names back. The swap's moment hands the host its
    /// inverse, so the comp's playing settings are carried home before its
    /// bay is rebuilt and it comes back dialled. With the trade moving the
    /// names alone, the undo carried nothing: the comp's bay was rebuilt
    /// from what the displaced shelf kept of it when it LEFT that bay, and
    /// the dial was lost. Then Ctrl+Shift+Z swaps them again, dial and all.
    #[test]
    fn a_bay_swaps_undo_and_redo_carry_each_units_dial() {
        let shelf = scratch("bayswap").join("state");
        let mut s = blank();
        kind(&mut s, &[0, 1], ChannelKind::Audio);
        let track = row(&s, 1);
        let (a, b) = (SeatBay::new(track, Rack::Chain(0), 0), SeatBay::new(track, Rack::Chain(0), 1));
        s.plugins.insert(a, COMP_PATH.to_owned());
        s.plugins.insert(b, EQ_PATH.to_owned());
        let mut app = app_with(s);
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        p.sync(&app, &mut e);
        let (at_a, at_b) = (p.engine_bay(a).expect("bay a"), p.engine_bay(b).expect("bay b"));
        let mut displaced = crate::shelf::DisplacedShelf::new();
        e.load_insert_at(at_a.0, at_a.1, Some(Path::new(COMP_PATH)), Some(dialled(COMP_PATH, &[(0, 0.2)]).as_slice()))
            .expect("the comp loads");
        e.load_insert_at(at_b.0, at_b.1, Some(Path::new(EQ_PATH)), Some(dialled(EQ_PATH, &[(0, 0.8)]).as_slice()))
            .expect("the EQ loads");
        take_handoffs(&mut e);
        let comp_live = e.save_insert_state_at(at_a.0, at_a.1).expect("the comp's settings");
        let eq_live = e.save_insert_state_at(at_b.0, at_b.1).expect("the EQ's settings");
        // The desk before the swap is a moment of its own: the undo returns
        // there. (Escape closes whatever the first frame opened over it.)
        key(&mut app, egui::Key::F24, egui::Modifiers::NONE);
        key(&mut app, egui::Key::Escape, egui::Modifiers::NONE);
        app.commit_undo_moment();
        let _ = app.take_bay_relabels();

        app.trade_bays(a, b);
        host_frame(&mut app, &p, &mut e, &mut displaced, &shelf, &[a, b]);
        assert_eq!(e.save_insert_state_at(at_b.0, at_b.1), Some(comp_live), "the swap did not carry the comp's settings");
        assert_eq!(e.save_insert_state_at(at_a.0, at_a.1), Some(eq_live.clone()), "the swap did not carry the EQ's settings");
        // Dialled where it landed; nothing writes its file.
        e.load_insert_at(at_b.0, at_b.1, Some(Path::new(COMP_PATH)), Some(dialled(COMP_PATH, &[(0, 0.9), (1, 0.1)]).as_slice()))
            .expect("the dial");
        take_handoffs(&mut e);
        let dial = e.save_insert_state_at(at_b.0, at_b.1).expect("the comp's dial");

        key(&mut app, egui::Key::Z, egui::Modifiers::COMMAND);
        assert_eq!(app.plugin(a), Some(COMP_PATH), "fixture: the undo did not put the comp's name back");
        host_frame(&mut app, &p, &mut e, &mut displaced, &shelf, &[a, b]);
        assert_eq!(
            e.save_insert_state_at(at_a.0, at_a.1),
            Some(dial.clone()),
            "the swap's undo brought the comp home without the dial it was given after the swap"
        );
        assert_eq!(e.save_insert_state_at(at_b.0, at_b.1), Some(eq_live.clone()), "the undo lost the EQ's settings");

        key(&mut app, egui::Key::Z, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT);
        assert_eq!(app.plugin(b), Some(COMP_PATH), "fixture: the redo did not swap the names again");
        host_frame(&mut app, &p, &mut e, &mut displaced, &shelf, &[a, b]);
        assert_eq!(e.save_insert_state_at(at_b.0, at_b.1), Some(dial), "the redo lost the comp's dial");
        assert_eq!(e.save_insert_state_at(at_a.0, at_a.1), Some(eq_live), "the redo lost the EQ's settings");
        let _ = std::fs::remove_dir_all(shelf.parent().expect("the scratch"));
    }

    /// **A reorder is ONE plan, and nothing reloads** (W4-integrate,
    /// done-when: "reordering all 32 loaded lanes gives 0 reloads, 0 engine
    /// round trips and 1 Plan"). Every row holds a loaded EQ; the first row
    /// is dragged to the bottom, so all thirty-two move. The tracks keep
    /// their lanes, so the bays the reconcile wants are the bays it already
    /// loaded, nothing is installed, removed or attached, and the one plan
    /// plays the tracks in their new order.
    #[test]
    fn a_reorder_is_one_plan_and_reloads_nothing() {
        let mut s = blank();
        kind(&mut s, &(0..CHANNELS).collect::<Vec<_>>(), ChannelKind::Audio);
        for ch in 0..CHANNELS {
            bay(&mut s, ch, 0, EQ_PATH);
        }
        let mut app = app_with(s);
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        p.sync(&app, &mut e);
        let ledger = wanted(&p, &app);
        assert_eq!(ledger.len(), CHANNELS, "a bay per lane");
        for ((rack, bay), path) in &ledger {
            e.load_insert_at(*rack, *bay, Some(Path::new(path)), None).expect("an EQ loads");
        }
        e.settle_offline();
        let before = p.stats();
        let lanes: BTreeMap<TrackId, LaneAt> =
            app.track_ids().iter().map(|id| (*id, p.lane_of(*id).expect("a lane"))).collect();

        app.move_channel(0, CHANNELS - 1);
        p.sync(&app, &mut e);

        let after = p.stats();
        assert_eq!(after.plans, before.plans + 1, "a reorder is not one plan");
        assert_eq!(
            (after.installs, after.removes, after.attaches),
            (before.installs, before.removes, before.attaches),
            "a reorder made the engine round trip"
        );
        assert_eq!(wanted(&p, &app), ledger, "a reorder changed what the reconcile wants loaded");
        for (id, at) in &lanes {
            assert_eq!(p.lane_of(*id), Some(*at), "track {id} changed lanes in a reorder");
        }
        let plan = p.published_for_test().expect("a plan");
        let order: Vec<Slot> = plan.lanes.iter().map(|l| l.slot).collect();
        let rows: Vec<Slot> = app.track_ids().iter().map(|id| lanes[id].slot).collect();
        assert_eq!(order, rows, "the plan does not play the tracks in their new order");
        assert_eq!((order.first(), order.last()), (Some(&Slot(1)), Some(&Slot(0))), "the drag did not move every row");
    }

    /// **A reorder and its undo keep every plugin state by its track, and
    /// touch no state file** (W4-integrate (B), done-when: "after (B) 0
    /// sidecar filesystem calls"). Rows 2, 3 and 4 hold a plugin on the
    /// chain, in the second rack and on the third pedal; Insert Track under
    /// row 1 brings the free row 5 up into the gap, which is a CYCLE (three
    /// rows shift down one), and the undo is its inverse. Every bay's state
    /// file is named by its track (`pstate-t3-c1-b1.bin`), so the names the
    /// desk holds are the same names after the reorder and after the undo,
    /// every file is found by them, and the shelf is not called once: the
    /// files the old builds moved row by row on every reorder are simply
    /// never asked to move.
    #[test]
    fn a_reorder_and_its_undo_keep_every_state_by_its_track() {
        let ids = tangent_ui::desk_legacy::identity_ids(32);
        let racks = [(Rack::Chain(0), 0u8), (Rack::Chain(1), 1), (Rack::Pedal(2), 0)];
        let mut s = blank();
        kind(&mut s, &[0, 1, 2, 3, 4], ChannelKind::Audio);
        for row in 2..5 {
            for (n, (rack, b)) in racks.into_iter().enumerate() {
                let sb = SeatBay::new(Node::Track(ids[row]), rack, b);
                let (seat, b) = app_seat_of(&ids, sb).expect("an app seat");
                bay(&mut s, seat, b, &format!("/x/row{row}-{n}.vst3"));
            }
        }
        let mut app = app_with(s);
        let dir = std::env::temp_dir().join(format!(
            "tangent-reorder-states-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let named = desk_bays(&app);
        assert_eq!(named.len(), 9, "the fixture seats nine plugins");
        for (sb, path) in &named {
            assert!(crate::shelf::write_state_in(&dir, *sb, path, path.as_bytes()), "a state file");
        }
        let files = |dir: &Path| -> BTreeMap<String, Vec<u8>> {
            std::fs::read_dir(dir)
                .expect("the shelf")
                .flatten()
                .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).expect("read")))
                .collect()
        };
        let before = files(&dir);
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        p.sync(&app, &mut e);
        // The desk before the reorder is a moment of its own, as the gesture
        // that came before it would have left it: the undo returns there.
        // (Escape closes whatever the first frame opened over the desk, which
        // would otherwise take the Cmd+Z.)
        key(&mut app, egui::Key::F24, egui::Modifiers::NONE);
        key(&mut app, egui::Key::Escape, egui::Modifiers::NONE);
        app.commit_undo_moment();

        let calls = crate::shelf::fs_calls();
        let mut retired = 0usize;
        app.move_channel(5, 2);
        p.sync_with(&app, &mut e, &mut |_, _, _| {
            retired += 1;
            Forgotten::new()
        });
        let moved = desk_bays(&app);
        let rows_moved = app.track_ids()[2..6].to_vec();
        key(&mut app, egui::Key::Z, egui::Modifiers::COMMAND);
        p.sync_with(&app, &mut e, &mut |_, _, _| {
            retired += 1;
            Forgotten::new()
        });
        let undone = desk_bays(&app);
        let rows_undone = app.track_ids()[2..6].to_vec();
        let shelf_calls = crate::shelf::fs_calls() - calls;

        assert_eq!(rows_moved, [ids[5], ids[2], ids[3], ids[4]], "fixture: the insert did not shift three rows");
        assert_eq!(rows_undone, [ids[2], ids[3], ids[4], ids[5]], "fixture: the undo did not put them back");
        assert_eq!(moved, named, "a reorder renamed a bay's state file");
        assert_eq!(undone, named, "its undo renamed a bay's state file");
        assert_eq!(retired, 0, "a reorder retired a lane");
        assert_eq!(shelf_calls, 0, "a reorder and its undo made {shelf_calls} shelf filesystem calls");
        for (sb, path) in &named {
            assert_eq!(
                crate::shelf::read_state(&dir, *sb, path).as_deref(),
                Some(path.as_bytes()),
                "{sb}'s plugin no longer finds its own state"
            );
        }
        assert_eq!(files(&dir), before, "a state file moved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A removed track's states are written before its plugins drop, and
    /// its undo brings it back with every state** (W4-integrate, done-when).
    /// A COMP dialled on row 1, the track removed: the host is asked for the
    /// lane's states while the plugin is still seated, and the lane goes
    /// home; the undo gives the row its track back, a fresh lane is built for
    /// it and its bay seated from what was written, and it plays the dial.
    #[test]
    fn a_removed_tracks_states_go_to_the_shelf_first_and_its_undo_restores_them() {
        let mut s = blank();
        kind(&mut s, &[0, 1, 2], ChannelKind::Audio);
        bay(&mut s, 1, 0, COMP_PATH);
        let mut app = app_with(s);
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        let dial = dialled(COMP_PATH, &[(0, 0.2), (1, 0.9)]);
        p.sync(&app, &mut e);
        let gone = app.track_ids()[1];
        let at = p.engine_bay(seat_bay_at(&app.track_ids(), 1, 0).expect("the bay")).expect("its seat");
        e.load_insert_at(at.0, at.1, Some(Path::new(COMP_PATH)), Some(&dial)).expect("the comp loads");
        let live = e.save_insert_state_at(at.0, at.1).expect("the comp's state");

        // The shelf the host keeps, by the track's bay.
        let mut shelf: BTreeMap<SeatBay, Vec<u8>> = BTreeMap::new();
        let mut asked: Vec<(TrackId, Slot, bool)> = Vec::new();
        // A frame first: the app keeps its undo line from the desk it drew.
        key(&mut app, egui::Key::F24, egui::Modifiers::NONE);
        app.apply_dialog_action(tangent_ui::dialogs::DialogAction::TrackRemove(1));
        assert!(!app.track_ids().contains(&gone), "the removal kept the track's id");
        let mut retire = |e: &Engine, id: TrackId, lane: LaneAt| {
            let (rack, bay) = lane_bay(lane.slot, Rack::Chain(0), 0).expect("the lane's first bay");
            let alive = e.insert_at(rack, bay).is_some();
            if let Some(state) = e.save_insert_state_at(rack, bay) {
                shelf.insert(SeatBay::new(Node::Track(id), Rack::Chain(0), 0), state);
            }
            asked.push((id, lane.slot, alive));
            Forgotten::new()
        };
        p.sync_with(&app, &mut e, &mut retire);
        assert_eq!(asked, vec![(gone, at_slot(at), true)], "the host was not asked for the lane's states while its plugin lived");
        assert_eq!(shelf.values().next(), Some(&live), "what reached the shelf is not what the comp was playing");
        assert_eq!(p.lane_of(gone), None, "the removed track kept a lane");
        assert_eq!(e.insert_at(at.0, at.1), None, "the removed lane's plugin outlived it");

        // The undo: the row's track is back, and wants a lane.
        key(&mut app, egui::Key::Z, egui::Modifiers::COMMAND);
        assert_eq!(app.track_ids().get(1), Some(&gone), "the undo did not bring the track back");
        for _ in 0..3 {
            p.sync_with(&app, &mut e, &mut |_, _, _| Forgotten::new());
        }
        let back = p.lane_of(gone).expect("the restored track has no lane");
        let sb = seat_bay_at(&app.track_ids(), 1, 0).expect("the bay");
        assert_eq!(wanted(&p, &app).get(&p.engine_bay(sb).expect("seat")), Some(&COMP_PATH.to_owned()), "the restored bay is not wanted at its lane");
        let (rack, bay) = lane_bay(back.slot, Rack::Chain(0), 0).expect("its first bay");
        e.load_insert_at(rack, bay, Some(Path::new(COMP_PATH)), shelf.get(&sb).map(Vec::as_slice))
            .expect("the comp comes back");
        assert_eq!(e.save_insert_state_at(rack, bay), Some(live), "the undo brought the comp back without its dial");
    }

    fn at_slot((rack, _): EngineBay) -> Slot {
        rack.lane().expect("a lane's rack")
    }

    /// **A track back before its lane was sent home takes the lane back, and
    /// nothing reloads** (design 7.13: "an id that reappears waits for its
    /// old lane"; "nothing was sent home, so nothing reloads"). A comp dialled
    /// on row 1 and an EQ beside it, the track removed on a running engine:
    /// the host's hook (`DesktopApp::forget_lane`) writes the lane's states
    /// down and forgets its bays while the lane fades under its RETIRING plan,
    /// and before the callback has let it go (no block rendered) the removal
    /// is undone. The track takes the same lane back, slot and generation,
    /// and what the hook forgot comes back with it (`Synced::reclaimed`,
    /// `DesktopApp::reclaim_lanes`): the bays the reconcile wants are exactly
    /// the bays the ledger holds, so it loads nothing, and the comp is the
    /// same instance still playing its dial. Forgotten for good, the
    /// reconcile reloaded every plugin of the lane in place from its file,
    /// and a bay whose state could not be asked for came back at defaults.
    #[test]
    fn a_track_back_before_its_lane_left_takes_it_back_and_reloads_nothing() {
        use crate::desktop::{DesktopApp, InsertLedger};
        let mut s = blank();
        kind(&mut s, &[0, 1, 2], ChannelKind::Audio);
        bay(&mut s, 1, 0, COMP_PATH);
        bay(&mut s, 1, 1, EQ_PATH);
        let mut app = app_with(s);
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        // Removals by the live road: the Remove waits for the callback to
        // take the plan that fades the lane, and no block is rendered here.
        p.stage_removals_for_test();
        p.sync(&app, &mut e);
        let gone = app.track_ids()[1];
        let lane = p.lane_of(gone).expect("a lane");
        let comp = p.engine_bay(seat_bay_at(&app.track_ids(), 1, 0).expect("the comp's bay")).expect("its rack");
        let dial = dialled(COMP_PATH, &[(0, 0.2), (1, 0.9)]);
        // The reconcile's loads: every bay the desk wants, and the ledger
        // that records them.
        let mut ledger: InsertLedger = wanted(&p, &app);
        for ((rack, bay), path) in &ledger {
            let state = (path == COMP_PATH).then_some(dial.as_slice());
            e.load_insert_at(*rack, *bay, Some(Path::new(path)), state).expect("a bay loads");
        }
        let playing = e.save_insert_state_at(comp.0, comp.1).expect("the comp's state");
        let shelf = scratch("reclaim-shelf");
        let before = p.stats();

        // A frame first: the app keeps its undo line from the desk it drew.
        key(&mut app, egui::Key::F24, egui::Modifiers::NONE);
        app.apply_dialog_action(tangent_ui::dialogs::DialogAction::TrackRemove(1));
        let mut forget = |e: &Engine, id: TrackId, at: LaneAt| DesktopApp::forget_lane(&mut ledger, &shelf, e, id, at);
        let left = p.sync_with(&app, &mut e, &mut forget);
        assert!(left.reclaimed.is_empty(), "a lane was taken back before its track returned");
        assert!(ledger.keys().all(|(rack, _)| rack.lane() != Some(lane.slot)), "the hook kept the leaving lane's bays");
        assert_eq!(p.lane_of(gone), None, "the removed track kept its lane");

        // The undo, while the lane is still on its way out.
        key(&mut app, egui::Key::Z, egui::Modifiers::COMMAND);
        assert_eq!(app.track_ids().get(1), Some(&gone), "fixture: the undo did not bring the track back");
        let mut forget = |e: &Engine, id: TrackId, at: LaneAt| DesktopApp::forget_lane(&mut ledger, &shelf, e, id, at);
        let back = p.sync_with(&app, &mut e, &mut forget);
        DesktopApp::reclaim_lanes(&mut ledger, &back);

        let after = p.stats();
        assert_eq!(back.reclaimed.len(), 1, "the returning track did not take its lane back");
        assert_eq!(p.lane_of(gone), Some(lane), "the returning track took another lane");
        assert_eq!(
            (after.installs, after.removes),
            (before.installs, before.removes),
            "the lane was sent home or another installed"
        );
        let want = wanted(&p, &app);
        let reloads: Vec<&EngineBay> =
            want.keys().chain(ledger.keys()).filter(|at| want.get(*at) != ledger.get(*at)).collect();
        assert!(reloads.is_empty(), "the reconcile would reload {reloads:?} in place");
        assert_eq!(
            e.save_insert_state_at(comp.0, comp.1),
            Some(playing),
            "the comp is not the instance that was playing the dial"
        );
        let _ = std::fs::remove_dir_all(&shelf);
    }

    /// **A plugin is reached through its rack's `HostRack`, by identity, at
    /// any slot** (design 8, item 8; W4-integrate (A) and (C)): the host
    /// asks the engine by `(RackAddr, bay)`, never by a seat, and the records
    /// the engine keeps of a bay's tenant live in that rack's one record. A
    /// lane at slot 40, past the frozen seat map's thirty-two rows: its
    /// chain, its extra rack and a pedal each seat a comp by identity, each
    /// answers what it holds and the state it plays, and so does a pad's
    /// rack past the map's pads, made by the load (W7-slot-gaps: it was
    /// named and never made, so a kit there played its pads dry). When the
    /// lane goes home, its racks' records go with it, the pad's included.
    #[test]
    fn a_plugin_is_reached_through_its_racks_host_record_at_any_slot() {
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let slot = Slot(40);
        e.install_lanes(vec![(slot, LaneSpec { kind: LaneKind::Audio, drum: true })]).expect("a lane at 40");
        e.settle_offline();
        let dial = dialled(COMP_PATH, &[(0, 0.2), (1, 0.9)]);
        let racks = [Rack::Chain(0), Rack::Chain(1), Rack::Pedal(3), Rack::Pad(5)];
        for rack in racks {
            let (addr, bay) = lane_bay(slot, rack, 2).expect("a rack of the lane");
            assert_eq!(addr.lane(), Some(slot), "{rack:?}: the rack is not the lane's");
            assert_eq!(addr.desk_rack(), Some(rack), "{rack:?}: the desk's name does not round-trip");
            e.load_insert_at(addr, bay, Some(Path::new(COMP_PATH)), Some(&dial))
                .expect("the comp loads")
                .expect("past the map, the rack is still there");
            assert_eq!(
                e.insert_at(addr, bay).map(|l| l.bundle.clone()),
                Some(PathBuf::from(COMP_PATH)),
                "{rack:?}: the bay does not say what it holds"
            );
            assert_eq!(e.save_insert_state_at(addr, bay), Some(dial.clone()), "{rack:?}: the bay lost its dial");
        }
        e.remove_lanes(&[slot]).expect("the lane goes home");
        let mut out = vec![0.0f32; 256 * 2];
        e.render_offline(&mut out);
        e.retire_tick();
        for rack in racks {
            let (addr, bay) = lane_bay(slot, rack, 2).expect("a rack of the lane");
            assert_eq!(e.insert_at(addr, bay), None, "{rack:?}: the host kept a record of a lane that went home");
        }
    }

    /// **On a running engine a leaving lane fades under a plan first**
    /// (design 7.13: "a Plan marking the lane RETIRING (a 10 ms ramp), then
    /// a Plan without it, then Remove"), and a new track never takes its
    /// slot while it fades: it WAITS for a legacy slot on its way home, and
    /// takes one below thirty-two once it is (`Bridge::install`; W6 had it
    /// take slot 32 at once, where the engine then kept a drum lane's pad
    /// racks only for the legacy slots, and an export gives the track its
    /// row's). Driven on an offline engine through the live road
    /// (`stage_removals_for_test`), a block rendered between frames for the
    /// callback that takes each op.
    #[test]
    fn a_leaving_lane_fades_under_a_plan_before_it_goes_home() {
        let mut s = blank();
        kind(&mut s, &[0, 1, 2], ChannelKind::Audio);
        let mut app = app_with(s);
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        p.stage_removals_for_test();
        let block = |e: &mut Engine| {
            let mut out = vec![0.0f32; 256 * 2];
            e.render_offline(&mut out);
            e.retire_tick();
        };
        p.sync(&app, &mut e);
        block(&mut e);
        let gone = app.track_ids()[1];
        let slot = p.lane_of(gone).expect("a lane").slot;
        app.apply_dialog_action(tangent_ui::dialogs::DialogAction::TrackRemove(1));
        // The removal deleted the row (W6), so the new track is one Add makes
        // while the lane fades.
        app.add_track();
        let fresh = app.selected_track().expect("Add selects its track");
        let before = p.stats();

        // Frame one: the lane is still there, fading.
        p.sync(&app, &mut e);
        let plan = p.published_for_test().expect("a plan");
        let retiring = plan.lanes.iter().find(|l| l.slot == slot).expect("the leaving lane left the plan at once");
        assert!(retiring.flags.contains(LaneFlags::RETIRING), "the leaving lane is not RETIRING");
        assert!(e.lane_gen(slot).is_some(), "the lane went home before its fade");
        assert_eq!(p.lane_of(fresh), None, "the new track took a slot while the legacy ones it can have are fading");
        assert_eq!(p.stats().removes, before.removes, "the lane was sent home under the plan that fades it");
        // The row the new track was made on was vacant, and its lane leaves
        // with its old identity (a new track is a new id): it fades beside
        // the removed track's, and goes home with it.
        let reused = 1;
        block(&mut e);
        std::thread::sleep(RETIRE_RAMP * 2);

        // Frame two: the plan without it, and home.
        p.sync(&app, &mut e);
        assert!(p.published_for_test().expect("a plan").lanes.iter().all(|l| l.slot != slot), "the lane is still planned");
        assert_eq!(p.stats().removes, before.removes + 1 + reused, "the lane was not sent home");
        assert!(e.lane_gen(slot).is_none(), "the removed track's lane is still live");
        block(&mut e);

        // Frame three: the slot is free and the new track has a lane, one
        // of the legacy slots that came home.
        p.sync(&app, &mut e);
        p.sync(&app, &mut e);
        let took = p.lane_of(fresh).expect("the new track never got a lane");
        assert!((took.slot.0 as usize) < LEGACY_LANES, "the new track took {:?}, past the legacy slots", took.slot);
        assert_eq!(e.plan_refused_for_test(), 0, "the callback refused a plan on the way");
    }

    /// **A freeze is of a TRACK, and lands on it wherever its row has gone**
    /// (design 7.13: "FreezeJob holds the TrackId and lands through
    /// `row_of`"). The track on row 1 is frozen; its stem is its own lane's
    /// (a silent track's freeze is silent); it is dragged to row 3 while the
    /// render runs; and the file lands on the row the track stands on.
    #[test]
    fn a_freeze_lands_on_its_track_after_a_reorder() {
        let mut s = blank();
        kind(&mut s, &[0, 1, 2, 3], ChannelKind::Audio);
        let mut doc = Arrangement::default();
        doc.add_clip(audio(1, "n1.wav", 0.0));
        let c = Corpus { name: "freeze-track", settings: s, doc, live: vec![], kits: vec![], freeze: vec![] };
        let dir = scratch(c.name);
        let mut app = open(&c, &dir);
        let id = app.track_ids()[1];
        let quiet = app.track_ids()[2];
        let spec = tangent_ui::ports::ProjectExportSpec {
            rate: RATE,
            master: false,
            stems: true,
            depth: tangent_ui::ports::ExportDepth::Float32,
            ..Default::default()
        };
        let loud = |app: &TangentApp, id: TrackId| {
            let built = full_build(app, RATE, Timebase::new(), BuildFor::Freeze(id)).expect("a freeze");
            let row = app.row_of(id).expect("the track's row");
            let files = render_files(built, app, &spec, Some(row), &dir.join("out").join(id.to_string()));
            files.iter().any(|(_, _, loud)| *loud)
        };
        assert!(loud(&app, id), "the frozen track's stem is silent");
        assert!(!loud(&app, quiet), "a track with nothing on it froze to sound: another lane's stem");
        let built = full_build(&app, RATE, Timebase::new(), BuildFor::Freeze(id)).expect("a freeze");
        assert_eq!(built.bridge.lane_of(id), built.bridge.lane(1), "the freeze is not of the track's lane");
        // Dragged while it renders.
        app.move_channel(1, 3);
        let row = app.row_of(id).expect("the track is still here");
        assert_eq!(row, 3, "the drag did not move the track");
        app.note_frozen(row, "media/frozen-02.wav".to_owned(), 1.0);
        assert!(app.frozen_lanes().iter().any(|z| z.lane == 3), "the freeze did not land on the track's row");
        assert_eq!(app.track_ids()[3], id, "the freeze landed on another track");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The harness hears every lane** (W4-integrate, done-when: "a test
    /// drives `TangentApp::with_license` + Bridge into an offline engine and
    /// hears every lane"): thirty-two audio tracks, each with a clip of its
    /// own, exported with stems through the bridge's builder - every stem
    /// sounds, and on its own row's file.
    #[test]
    fn the_bridge_hears_every_lane() {
        let mut s = blank();
        kind(&mut s, &(0..CHANNELS).collect::<Vec<_>>(), ChannelKind::Audio);
        let mut doc = Arrangement::default();
        for lane in 0..CHANNELS {
            doc.add_clip(audio(lane, &format!("n{}.wav", lane % 5), 0.0));
        }
        let c = Corpus { name: "every-lane", settings: s, doc, live: vec![], kits: vec![], freeze: vec![] };
        let dir = scratch(c.name);
        let app = open(&c, &dir);
        let spec = tangent_ui::ports::ProjectExportSpec { rate: RATE, master: false, stems: true, ..Default::default() };
        let built = full_build(&app, RATE, Timebase::new(), BuildFor::Export(spec.clone())).expect("an export");
        let files = render_files(built, &app, &spec, None, &dir.join("out"));
        assert_eq!(files.len(), CHANNELS, "a lane has no stem");
        let silent: Vec<&String> = files.iter().filter(|(_, _, loud)| !loud).map(|(n, ..)| n).collect();
        assert!(silent.is_empty(), "these lanes were not heard: {silent:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── the export's TRACKS and TIME rows ───────────────────────────────────

    /// A small desk for the export pickers: two tracks, a folder holding
    /// two more (both named "Gtr"), and a bus. Every track has a clip;
    /// `starts` says where each track's clip begins.
    fn picker_desk(tag: &str, starts: [f64; 4], edit: impl FnOnce(&mut Settings)) -> (PathBuf, TangentApp) {
        picker_desk_with(tag, starts, None, edit)
    }

    /// [`picker_desk`], with a clip of the folder's OWN on row 2 when
    /// `folder_clip` says where.
    fn picker_desk_with(
        tag: &str,
        starts: [f64; 4],
        folder_clip: Option<f64>,
        edit: impl FnOnce(&mut Settings),
    ) -> (PathBuf, TangentApp) {
        picker_desk_full(tag, starts, folder_clip, &[], edit)
    }

    /// [`picker_desk_with`], with the clips of the rows in `deleted` never
    /// placed: the desk a left-out track is measured against.
    fn picker_desk_full(
        tag: &str,
        starts: [f64; 4],
        folder_clip: Option<f64>,
        deleted: &[usize],
        edit: impl FnOnce(&mut Settings),
    ) -> (PathBuf, TangentApp) {
        let mut s = blank();
        kind(&mut s, &[0, 1, 2, 3, 4], ChannelKind::Audio);
        kind(&mut s, &[5], ChannelKind::Bus);
        for (row, name) in [(0usize, "Kick"), (1, "Bass"), (2, "Guitars"), (3, "Gtr"), (4, "Gtr"), (5, "Room")] {
            s.rows[row].name = name.to_owned();
        }
        s.rows[2].folder = true;
        s.rows[3].in_folder = true;
        s.rows[4].in_folder = true;
        edit(&mut s);
        let mut doc = Arrangement::default();
        if let Some(at) = folder_clip {
            doc.add_clip(audio(2, "n4.wav", at));
        }
        for (i, row) in [0usize, 1, 3, 4].into_iter().enumerate() {
            if deleted.contains(&row) {
                continue;
            }
            doc.add_clip(audio(row, &format!("n{i}.wav"), starts[i]));
        }
        let c = Corpus { name: "pickers", settings: s, doc, live: vec![], kits: vec![], freeze: vec![] };
        let dir = scratch(tag);
        let app = open(&c, &dir);
        (dir, app)
    }

    /// One export of `spec` off `app`, its files by name.
    fn export_of(app: &TangentApp, spec: &ExportSpec, dir: &Path) -> BTreeMap<String, (Vec<u8>, bool)> {
        let built = full_build(app, RATE, Timebase::new(), BuildFor::Export(spec.clone())).expect("an export");
        render_files(built, app, spec, None, dir).into_iter().map(|(n, b, loud)| (n, (b, loud))).collect()
    }

    /// A float WAV's interleaved samples, read straight off its `data`
    /// chunk.
    fn wav_samples(bytes: &[u8]) -> Vec<f32> {
        let at = bytes.windows(4).position(|w| w == b"data").expect("a data chunk");
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().expect("its length")) as usize;
        bytes[at + 8..at + 8 + len].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().expect("a sample"))).collect()
    }

    /// **Stems: never a folder, a bus as its own mix, only the checked
    /// tracks, and each file named for its TRACK.** The owner: "folders
    /// should not be exported" and "I'd like to see the stems be named
    /// after the track, not the project". Two tracks of one name are two
    /// files.
    #[test]
    fn stems_leave_out_folders_and_are_named_for_their_tracks() {
        let (dir, app) = picker_desk("stem-names", [0.0; 4], |_| {});
        let all = ExportSpec { master: false, stems: true, rate: RATE, ..Default::default() };
        let files: Vec<(usize, String)> = stem_files(&app, &all);
        assert_eq!(
            files,
            vec![
                (0, "Kick.wav".to_owned()),
                (1, "Bass.wav".to_owned()),
                (3, "Gtr.wav".to_owned()),
                (4, "Gtr 2.wav".to_owned()),
                (5, "Room.wav".to_owned()),
            ],
            "the folder has a stem, the bus has none, or a name is not its track's"
        );
        // Only the checked tracks.
        let some = ExportSpec { tracks: Some(vec![app.track_at(1).unwrap(), app.track_at(4).unwrap()]), ..all.clone() };
        assert_eq!(stem_files(&app, &some), vec![(1, "Bass.wav".to_owned()), (4, "Gtr 2.wav".to_owned())]);
        // And written so: the files a stems pass of those leaves on disk.
        let out = export_of(&app, &some, &dir.join("out"));
        let names: Vec<&String> = out.keys().collect();
        assert_eq!(names, vec!["stems/Bass.wav", "stems/Gtr 2.wav"]);
        assert!(out.values().all(|(_, loud)| *loud), "a checked stem is silent");
        // A stems pass with nothing to make a stem of is refused in words,
        // on both roads, rather than run to an empty folder.
        let nothing = ExportSpec { tracks: Some(vec![app.track_at(6).unwrap()]), ..all.clone() };
        assert!(stem_files(&app, &nothing).is_empty());
        assert!(no_stems_refusal(&app, &nothing).is_some());
        assert_eq!(no_stems_refusal(&app, &some), None);
        assert_eq!(no_stems_refusal(&app, &ExportSpec { master: true, ..nothing.clone() }), None, "a master is a file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A folder with clips of its OWN gets a stem of those alone**, named
    /// for the folder: not its children (they have their own), and not
    /// nothing (they are in no child's). A folder with nothing of its own
    /// still has no stem. Both regressed when folder stems went.
    #[test]
    fn a_folders_own_clips_are_a_stem_of_their_own_and_its_group_is_not() {
        let (dir, app) = picker_desk_with("folder-own", [0.0; 4], Some(0.0), |_| {});
        let all = ExportSpec { master: false, stems: true, rate: RATE, depth: tangent_ui::ports::ExportDepth::Float32, ..Default::default() };
        let files = stem_files(&app, &all);
        assert!(files.contains(&(2, "Guitars.wav".to_owned())), "the folder's own clip has no stem: {files:?}");
        let out = export_of(&app, &all, &dir.join("out"));
        assert!(out["stems/Guitars.wav"].1, "the folder's own stem is silent");
        // Byte for byte what the same clip writes on a plain track: the
        // folder's own row, with neither child summed into it.
        let (dir2, plain) = picker_desk_with("folder-own-ref", [0.0; 4], Some(0.0), |s| {
            s.rows[2].folder = false;
            s.rows[3].in_folder = false;
            s.rows[4].in_folder = false;
        });
        let spec = ExportSpec { tracks: Some(vec![plain.track_at(2).unwrap()]), ..all.clone() };
        let reference = export_of(&plain, &spec, &dir2.join("ref"));
        // To float rounding: the two roads multiply the row by unity in a
        // different order.
        let (got, want) = (wav_samples(&out["stems/Guitars.wav"].0), wav_samples(&reference["stems/Guitars.wav"].0));
        assert_eq!(got.len(), want.len());
        let worst = got.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1.0e-6, "the folder's stem is not its own row alone (off by {worst})");
        // No own content, no stem.
        let (dir3, bare) = picker_desk("folder-bare", [0.0; 4], |_| {});
        assert!(stem_files(&bare, &all).iter().all(|(row, _)| *row != 2), "a folder with nothing of its own has a stem");
        for d in [dir, dir2, dir3] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// **Leaving a folder's own row out silences that row and never its
    /// children**: the mix with the folder's box unchecked and its children
    /// checked is the mix of the desk with the folder's clip deleted.
    #[test]
    fn leaving_a_folders_own_row_out_silences_only_that_row() {
        let (dir, app) = picker_desk_with("folder-row-out", [0.0; 4], Some(0.0), |_| {});
        let ids: Vec<TrackId> = [0usize, 1, 3, 4].iter().map(|r| app.track_at(*r).unwrap()).collect();
        let spec = ExportSpec { rate: RATE, depth: tangent_ui::ports::ExportDepth::Float32, tracks: Some(ids), ..Default::default() };
        let left = export_left_out(&app, &spec).expect("something is left out");
        assert_eq!(left.rows, [app.track_at(2).unwrap()].into_iter().collect(), "the folder's row is not the one left out");
        assert!(!left.tracks.contains(&app.track_at(2).unwrap()), "the folder was left out whole, children and all");
        let got = export_of(&app, &spec, &dir.join("got"));
        let (dir2, deleted) = picker_desk_with("folder-row-ref", [0.0; 4], None, |_| {});
        let want = export_of(&deleted, &ExportSpec { tracks: None, ..spec.clone() }, &dir2.join("want"));
        assert!(got["mix.wav"].0 == want["mix.wav"].0, "the children were not heard whole, or the folder's clip was");
        let whole = export_of(&app, &ExportSpec { tracks: None, ..spec.clone() }, &dir.join("whole"));
        assert!(got["mix.wav"].0 != whole["mix.wav"].0, "the folder's own clip was still heard");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    /// **A mixdown of selected tracks hears those alone, as the desk would
    /// play them, and touches nothing of the desk's.** The master of "Kick"
    /// checked is the master of the desk with every other track's clip
    /// DELETED, sample for sample (not merely muted: a mute slews, so the
    /// head of a muted track is in the file, where a track left out is
    /// gone from the first frame); and every box checked is "Everything", the desk's
    /// own solo included - here a solo on Bass, so Kick checked alone is
    /// silence (the window refuses that before it is sent; this is the
    /// engine's half).
    #[test]
    fn a_mixdown_of_selected_tracks_hears_those_alone() {
        let (dir, app) = picker_desk("subset-mix", [0.0; 4], |_| {});
        let kick = app.track_at(0).unwrap();
        let spec = ExportSpec { rate: RATE, depth: tangent_ui::ports::ExportDepth::Float32, tracks: Some(vec![kick]), ..Default::default() };
        let left = export_left_out(&app, &spec).expect("something is left out");
        let out: Vec<usize> = (0..6).filter(|r| left.tracks.contains(&app.track_at(*r).unwrap())).collect();
        assert_eq!(out, vec![1, 3, 4], "the left-out set is not every unchecked track (the folder and the bus carry)");
        let got = export_of(&app, &spec, &dir.join("subset"));
        assert!((0..6).all(|r| !app.strip_heard_muted(StripRef::Row(r))), "the export wrote a mute on the desk");
        let (dir2, muted) = picker_desk_full("subset-ref", [0.0; 4], None, &[1, 3, 4], |_| {});
        let all = ExportSpec { tracks: None, ..spec.clone() };
        let reference = export_of(&muted, &all, &dir2.join("ref"));
        let whole = export_of(&app, &all, &dir.join("whole"));
        let mix = |m: &BTreeMap<String, (Vec<u8>, bool)>| m.get("mix.wav").expect("the master").clone();
        assert!(mix(&got).1, "the checked track was not heard");
        assert!(mix(&got).0 == mix(&reference).0, "the subset is not the desk with the others' clips deleted");
        assert!(mix(&got).0 != mix(&whole).0, "the subset is the whole desk");
        // Every box checked, with a solo standing on the desk: Everything.
        let (dir3, soloed) = picker_desk("subset-solo", [0.0; 4], |s| s.rows[1].solo = true);
        let every: Vec<TrackId> = (0..soloed.desk_extent()).filter_map(|r| soloed.track_at(r)).collect();
        let checked = export_of(&soloed, &ExportSpec { tracks: Some(every), ..spec.clone() }, &dir3.join("all"));
        let everything = export_of(&soloed, &all, &dir3.join("everything"));
        assert!(mix(&checked).0 == mix(&everything).0, "every box checked is not Everything");
        // A solo on a track left out silences the checked ones, as on the
        // desk: past the desk's own solo slew at the head, nothing.
        let kick_only = wav_samples(&export_of(&soloed, &spec, &dir3.join("kick"))["mix.wav"].0);
        let past_head = kick_only[(RATE as usize / 10) * 2..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(past_head < 1.0e-4, "a checked track was heard through the desk's solo on another ({past_head})");
        // A stems-only pass leaves nothing out of the render.
        assert_eq!(export_left_out(&app, &ExportSpec { master: false, stems: true, ..spec.clone() }), None);
        for d in [dir, dir2, dir3] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// **A track left out of an export keys nothing.** A kick keying a
    /// compressor on the bass: with the kick unchecked, the bass must sound
    /// as it does on a desk whose kick has no clip at all - not merely
    /// muted, since a muted source still keys by the desk's rule.
    #[test]
    fn a_left_out_kick_does_not_duck_a_checked_bass() {
        let desk_m = |kick: bool, muted: bool| {
            let mut s = blank();
            kind(&mut s, &[0, 1], ChannelKind::Audio);
            s.rows[0].mute = muted;
            s.rows[0].name = "Kick".to_owned();
            s.rows[1].name = "Bass".to_owned();
            bay(&mut s, 1, 0, COMP_PATH);
            s.insert_keys.set(legacy_sb(&s, 1, 0), Some(row(&s, 0)));
            let mut doc = Arrangement::default();
            if kick {
                doc.add_clip(audio(0, "kick.wav", 0.0));
            }
            doc.add_clip(audio(1, "n1.wav", 0.0));
            let live = vec![(1, 0, COMP_PATH, dialled(COMP_PATH, &[(0, 0.1), (1, 1.0)]))];
            Corpus { name: "key-left-out", settings: s, doc, live, kits: vec![], freeze: vec![] }
        };
        let desk = |kick: bool| desk_m(kick, false);
        let render = |c: &Corpus, tag: &str, only_bass: bool| {
            let dir = scratch(tag);
            let app = open(c, &dir);
            let (live, live_bridge) = live_engine(c, &app);
            let spec = ExportSpec {
                rate: RATE,
                depth: tangent_ui::ports::ExportDepth::Float32,
                tracks: only_bass.then(|| vec![app.track_at(1).unwrap()]),
                stems: true,
                ..Default::default()
            };
            let mut ctx = Furnish { live: Some((&live, &live_bridge)), ..Furnish::default() };
            let built = full_build_with(&app, RATE, Timebase::new(), BuildFor::Export(spec.clone()), &mut ctx).expect("an export");
            let files = render_files(built, &app, &spec, None, &dir.join("out"));
            let _ = std::fs::remove_dir_all(&dir);
            // The BASS's own processed stem: its compressor and nothing of
            // the kick's own sound, so only the key can move it.
            files.into_iter().find(|(n, ..)| n == "stems/Bass.wav").expect("the bass's stem").1
        };
        let with_kick = desk(true);
        let got = render(&with_kick, "key-out", true);
        let want = render(&desk(false), "key-none", false);
        let keyed = render(&with_kick, "key-whole", false);
        assert!(keyed != want, "the fixture's key ducks nothing, so this proves nothing");
        // The desk's own rule, which an export does not change: a MUTED kick
        // still moves the bass. Left out is not muted.
        let muted = render(&desk_m(true, true), "key-muted", false);
        assert!(muted != want, "a muted kick keys nothing here, so muted and left out cannot be told apart");
        assert!(got == want, "a left-out kick still ducked the bass");
    }

    /// **The live bridge leaves an export's unchecked tracks out while it is
    /// set, and puts the desk back when it is cleared**, measured on the
    /// AUDIO: the road a realtime pass takes, a live-shaped plan on an
    /// engine playing the desk's clips. Everything with a clip left out is
    /// silence; cleared, the same engine plays again.
    #[test]
    fn the_live_plan_leaves_an_exports_unchecked_tracks_out_while_it_runs() {
        let (dir, app) = picker_desk("live-left-out", [0.0; 4], |_| {});
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an engine");
        let mut b = Bridge::new();
        b.sync(&app, &mut e);
        let doc_path = app.clip_doc().expect("the clips' document");
        let (placed, midi, _) = export_clip_placement(app.arrangement(), &app.frozen_lanes(), RATE, |rel| {
            Arrangement::resolve(&doc_path, rel)
        });
        e.set_clips_slotted(b.slot_clips(placed));
        e.set_midi_slotted(b.slot_midi(midi));
        let play = |e: &mut Engine, b: &mut Bridge, out: Option<LeftOut>| {
            b.set_left_out(out);
            b.sync(&app, e);
            e.settle_offline();
            e.set_transport(1, 0);
            e.set_rolling(true);
            let mut block = vec![0.0f32; 2048 * 2];
            let mut energy = 0.0f64;
            for _ in 0..8 {
                block.fill(0.0);
                e.render_offline(&mut block);
                energy += block.iter().map(|v| f64::from(v.abs())).sum::<f64>();
            }
            energy
        };
        assert!(play(&mut e, &mut b, None) > 0.0, "the desk plays nothing, so this proves nothing");
        // Only the bus checked: every track with a clip left out.
        let spec = ExportSpec { tracks: Some(vec![app.track_at(5).unwrap()]), ..Default::default() };
        let out = export_left_out(&app, &spec);
        assert_eq!(play(&mut e, &mut b, out), 0.0, "a left-out track was heard live");
        assert!(play(&mut e, &mut b, None) > 0.0, "the desk did not come back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **Each TIME choice renders exactly its frames**: a span starts its
    /// file at its START (the clip at 1.5 s lands half a second in when the
    /// span opens at 1.0 s) and runs to its END plus the two-second tail,
    /// to the frame, where the whole song runs from 0:00 to the song's end
    /// plus the tail.
    #[test]
    fn each_time_choice_renders_exactly_its_frames() {
        let (dir, app) = picker_desk("time-span", [1.5, 9.0, 9.0, 9.0], |_| {});
        let base = ExportSpec {
            rate: RATE,
            depth: tangent_ui::ports::ExportDepth::Float32,
            tracks: Some(vec![app.track_at(0).unwrap()]),
            ..Default::default()
        };
        let first_sound = |s: &[f32]| s.chunks_exact(2).position(|f| f[0] != 0.0 || f[1] != 0.0).expect("a sound");
        let whole = export_of(&app, &base, &dir.join("whole"));
        let whole = wav_samples(&whole["mix.wav"].0);
        let song = app.arr_span_s();
        assert_eq!(whole.len() / 2, (((song + crate::desktop::EXPORT_TAIL_S) * f64::from(RATE)).ceil()) as usize);
        let at = first_sound(&whole);
        assert!(at.abs_diff(3 * RATE as usize / 2) <= 1, "the whole song's clip sounds at frame {at}");
        for (span, from) in [((1.0, 2.0), 1.0), ((1.25, 4.0), 1.25)] {
            let got = export_of(&app, &ExportSpec { span: Some(span), ..base.clone() }, &dir.join("span"));
            let got = wav_samples(&got["mix.wav"].0);
            let want = (((span.1 - span.0 + crate::desktop::EXPORT_TAIL_S) * f64::from(RATE)).ceil()) as usize;
            assert_eq!(got.len() / 2, want, "{span:?} rendered {} frames", got.len() / 2);
            let lands = ((1.5 - from) * f64::from(RATE)) as usize;
            let at = first_sound(&got);
            assert!(at.abs_diff(lands) <= 1, "{span:?}: the clip sounds at frame {at}, not {lands}");
            // And it is the same sound the whole render has there.
            assert!(
                got[at * 2..at * 2 + 64] == whole[(at + (from * f64::from(RATE)) as usize) * 2..][..64],
                "{span:?} is not the song from its START"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── the inputs: the pick table (W5-engine-memory-inputs) ────────────────

    use tangent_record::audio::{ChannelPick, Picks};

    /// An audio track's row as the pick table reads it. Not `row`: the
    /// desk tests above name a fixture desk's track by that word
    /// (W5-identity-maps), and two helpers of one name in this module
    /// stopped the wave's merged test target compiling.
    fn input_row(id: u32, spec: Option<ChannelPick>, armed: bool) -> InputRow {
        InputRow { id: TrackId(id), audio: true, spec, armed }
    }

    fn stream(dev: usize) -> InputStream {
        InputStream { device_channels: Some(dev), picks: Picks::EVERYTHING }
    }

    /// Each track's pick, by track.
    fn picks_of(t: &PickTable, ids: &[u32]) -> Vec<Option<u32>> {
        ids.iter().map(|id| t.pick_of(TrackId(*id)).map(|p| p.0)).collect()
    }

    /// **A named input's pick is its track's, through every change of
    /// shape** (design A5: "an explicit binding gets the lowest free PickId
    /// and keeps it"). The picks are numbered along the rows once, when the
    /// desk is first bound, and never again: a permutation, a track inserted
    /// above, a removal and a new binding each move no pick that stands, a
    /// new binding takes the lowest one free, and the stream's list of
    /// inputs, being by pick, does not change under a reorder.
    #[test]
    fn a_named_inputs_pick_is_its_tracks_through_every_change_of_shape() {
        use ChannelPick::{Mono, Stereo};
        let mut t = PickTable::default();
        let s = stream(8);
        // A, B, an audio track naming nothing, D.
        let mut rows = vec![
            input_row(10, Some(Mono(0)), true),
            input_row(11, Some(Stereo(2, 3)), false),
            input_row(12, None, false),
            input_row(13, Some(Mono(4)), false),
        ];
        t.bind(&rows, &s);
        assert!(t.explicit());
        assert_eq!(picks_of(&t, &[10, 11, 12, 13]), vec![Some(0), Some(1), None, Some(2)]);
        let listed = t.stream_picks();
        assert_eq!(listed, vec![Mono(0), Stereo(2, 3), Mono(4)], "the stream's inputs, by pick");
        // Reversed: nothing moves, not even the stream's list.
        rows.reverse();
        t.bind(&rows, &s);
        assert_eq!(picks_of(&t, &[10, 11, 12, 13]), vec![Some(0), Some(1), None, Some(2)], "a reorder moved a pick");
        assert_eq!(t.stream_picks(), listed, "a reorder would reopen the stream");
        // A track inserted above everything, naming an input: the next pick.
        rows.insert(0, input_row(14, Some(Mono(7)), false));
        t.bind(&rows, &s);
        assert_eq!(picks_of(&t, &[10, 11, 13, 14]), vec![Some(0), Some(1), Some(2), Some(3)]);
        // B removed: its pick is free, and a hole reads nothing.
        rows.retain(|r| r.id != TrackId(11));
        t.bind(&rows, &s);
        assert_eq!(picks_of(&t, &[10, 13, 14]), vec![Some(0), Some(2), Some(3)], "a removal moved a pick");
        assert_eq!((t.picks()[1].track, t.picks()[1].width), (None, 0), "a freed pick still reads");
        // A new binding takes the lowest pick free, which is B's.
        rows.push(input_row(15, Some(Mono(5)), false));
        t.bind(&rows, &s);
        assert_eq!(picks_of(&t, &[10, 13, 14, 15]), vec![Some(0), Some(2), Some(3), Some(1)]);
        // Each reads its own columns.
        let cols = |id: u32| t.picks()[t.pick_of(TrackId(id)).expect("a pick").0 as usize].cols;
        assert_eq!((cols(10), cols(13), cols(14), cols(15)), ([0, 0], [4, 4], [7, 7], [5, 5]));
        // An input the device does not have reads nothing, and keeps its pick.
        rows[0].spec = Some(Mono(9));
        t.bind(&rows, &s);
        let p = t.pick_of(TrackId(14)).expect("still bound");
        assert_eq!((p.0, t.picks()[p.0 as usize].width), (3, 0));
    }

    /// **A pick a record holds keeps the input it was reading, and goes to
    /// no other track, until the record lets it go** (design A5: the record
    /// is bound by identity). Its track removed mid-take, the pick still
    /// reads its column (the tee keeps writing the take) and feeds no lane;
    /// a new binding takes another pick. Released, it is free again.
    #[test]
    fn a_held_pick_keeps_its_input_and_goes_to_no_other_track() {
        use ChannelPick::Mono;
        let mut t = PickTable::default();
        let s = stream(4);
        let mut rows = vec![input_row(1, Some(Mono(2)), true), input_row(2, Some(Mono(3)), false)];
        t.bind(&rows, &s);
        let held = t.pick_of(TrackId(1)).expect("a pick");
        t.hold(&[held]);
        rows.remove(0);
        rows.push(input_row(3, Some(Mono(1)), false));
        t.bind(&rows, &s);
        let e = t.picks()[held.0 as usize];
        assert_eq!((e.track, e.from, e.width, e.cols), (None, Some(Mono(2)), 1, [2, 2]), "the held pick lost its input");
        assert_ne!(t.pick_of(TrackId(3)), Some(held), "a held pick was given to another track");
        t.release(&[held]);
        rows.push(input_row(4, Some(Mono(0)), false));
        t.bind(&rows, &s);
        assert_eq!(t.pick_of(TrackId(4)), Some(held), "a released pick is not free again");
    }

    /// **A held pick whose input left the device reads nothing, and is
    /// named** (W6: the capture tee made loud). An interface reopened with
    /// two inputs under a record that holds a pick on inputs 3 and 4: that
    /// pick reads nothing (the tee skips it, so its take stops growing), and
    /// `held_reading_nothing` names it and only it; a pick still reading, an
    /// unheld pick reading nothing, and a held pick whose input came back are
    /// not named.
    #[test]
    fn a_held_pick_whose_input_left_the_device_reads_nothing_and_is_named() {
        use ChannelPick::{Mono, Stereo};
        let mut t = PickTable::default();
        let rows = vec![
            input_row(1, Some(Stereo(2, 3)), true),
            input_row(2, Some(Mono(0)), true),
            input_row(3, Some(Mono(3)), false),
        ];
        t.bind(&rows, &stream(4));
        let (a, b) = (t.pick_of(TrackId(1)).expect("a pick"), t.pick_of(TrackId(2)).expect("a pick"));
        t.hold(&[a, b]);
        assert!(t.held_reading_nothing().is_empty(), "a held pick that reads was named");
        // The interface comes back with two inputs: 3 and 4 are gone, 1 is not.
        t.bind(&rows, &stream(2));
        assert_eq!(t.picks()[a.0 as usize].width, 0, "fixture: the pick on the lost inputs still reads");
        assert_eq!(t.held_reading_nothing(), vec![a], "the take on the lost inputs was not named, or another was");
        // Back to four: reading again, named no more.
        t.bind(&rows, &stream(4));
        assert!(t.held_reading_nothing().is_empty(), "a held pick whose input came back is still named");
        // Released, nothing records through it: a pick reading nothing is
        // nobody's take.
        t.release(&[a, b]);
        t.bind(&rows, &stream(2));
        assert!(t.held_reading_nothing().is_empty(), "a released pick was named");
    }

    /// **A pick a record holds on a stream opened whole keeps reading the
    /// whole device when the desk names its first input mid-take**, and
    /// keeps feeding the track recording through it (design A5). The desk
    /// turns explicit around it: the new binding takes another pick, the
    /// stream narrows to that binding (the device stays open), and the held
    /// pick reads the whole device's first pair throughout. It was held as
    /// `None`, which for a pick that is not the whole device reads nothing:
    /// width 0, which the tee skips, and the take stopped growing unsaid.
    #[test]
    fn a_held_whole_device_pick_reads_the_whole_device_when_the_desk_names_an_input() {
        use ChannelPick::Mono;
        let mut t = PickTable::default();
        let whole = stream(4);
        let mut rows = vec![input_row(1, None, true), input_row(2, None, false)];
        t.bind(&rows, &whole);
        assert!(!t.explicit());
        let held = t.pick_of(TrackId(1)).expect("a pick");
        let e = t.picks()[held.0 as usize];
        assert_eq!((e.from, e.width, e.cols), (None, 4, [0, 1]), "the first pick of a stream opened whole");
        t.hold(&[held]);
        // Track 2 names input 2 mid-take: the desk is explicit now.
        rows[1].spec = Some(Mono(1));
        t.bind(&rows, &whole);
        assert!(t.explicit());
        let narrowed = InputStream { device_channels: Some(4), picks: Picks::from_slice(&t.stream_picks()) };
        assert_eq!(t.stream_picks(), vec![Mono(1)], "the stream is not narrowed to the binding");
        for s in [&whole, &narrowed] {
            t.bind(&rows, s);
            let e = t.picks()[held.0 as usize];
            assert_eq!(
                (e.track, e.from, e.width, e.cols),
                (Some(TrackId(1)), None, 4, [0, 1]),
                "the held pick no longer reads the whole device for its track"
            );
            let other = t.pick_of(TrackId(2)).expect("the new binding's pick");
            assert_ne!(other, held, "a held pick was given to the track that named an input");
            assert_eq!(t.picks()[other.0 as usize].cols, [1, 1]);
        }
        // The recording track names an input of its own mid-take: it is
        // bound on the pick it holds, which still reads what the record reads.
        rows[0].spec = Some(Mono(3));
        t.bind(&rows, &narrowed);
        assert_eq!(t.pick_of(TrackId(1)), Some(held), "the recording track has another pick");
        assert_eq!(t.picks()[held.0 as usize].width, 4, "the held pick took the binding before the record let go");
        // Released, the same pick reads the track's own binding.
        t.release(&[held]);
        t.bind(&rows, &narrowed);
        let e = t.picks()[held.0 as usize];
        assert_eq!((e.track, e.from, e.cols), (Some(TrackId(1)), Some(Mono(3)), [3, 3]));
    }

    /// **A pick a record holds keeps its input and its track when the desk
    /// stops naming inputs mid-take** (design A5). The desk turns ordinal
    /// around it, and the stream, the desk naming nothing, goes back to the
    /// band's own channel (`devices::audio_selection`): every other audio
    /// row takes a pick nobody holds, in row order, each reading the stream
    /// at its place, and the held pick reads input 3 throughout. The ordinal
    /// walk ignored the held picks: the record's pick read the stream's
    /// first input, the band's channel a frame later, and the rest of the
    /// take was another microphone.
    #[test]
    fn a_held_pick_keeps_its_input_when_the_desk_stops_naming_inputs() {
        use ChannelPick::Mono;
        let mut t = PickTable::default();
        let mut rows = vec![input_row(1, None, false), input_row(2, Some(Mono(2)), true), input_row(3, None, false)];
        let named = InputStream { device_channels: Some(4), picks: Picks::from_slice(&[Mono(2)]) };
        t.bind(&rows, &named);
        let held = t.pick_of(TrackId(2)).expect("a pick");
        t.hold(&[held]);
        // Its input set to none: nothing on the desk names one now.
        rows[1].spec = None;
        let band = InputStream { device_channels: Some(4), picks: Picks::from_slice(&[Mono(0)]) };
        for s in [&named, &band] {
            t.bind(&rows, s);
            assert!(!t.explicit());
            assert!(t.stream_picks().is_empty(), "an ordinal desk tells the stream its inputs");
            let e = t.picks()[held.0 as usize];
            assert_eq!(
                (e.track, e.from, e.width, e.cols),
                (Some(TrackId(2)), Some(Mono(2)), 1, [2, 2]),
                "the held pick left its input or its track"
            );
            assert_eq!(picks_of(&t, &[1, 2, 3]), vec![Some(1), Some(0), Some(2)], "a held pick was given away");
        }
        assert_eq!(t.picks()[1].from, None, "a pick past the band's one input reads it");
        // Released: the ordinal walk, the k-th audio row on pick k.
        t.release(&[held]);
        t.bind(&rows, &band);
        assert_eq!(picks_of(&t, &[1, 2, 3]), vec![Some(0), Some(1), Some(2)]);
        assert_eq!(t.picks()[0].from, Some(Mono(0)));
    }

    /// **On a desk that never named an input, a held pick stays its track's
    /// and keeps its input** (design A5), where the ordinal walk numbers
    /// every other pick by place: a track inserted above the recording one
    /// takes the next pick nobody holds, and the band's inputs changed
    /// mid-take move every pick but the held one. Released, the walk is the
    /// one it always was.
    #[test]
    fn an_ordinal_held_pick_stays_its_tracks_through_an_insert_above_and_new_band_inputs() {
        use ChannelPick::Mono;
        let mut t = PickTable::default();
        let first = InputStream { device_channels: Some(8), picks: Picks::from_slice(&[Mono(4), Mono(5)]) };
        let mut rows = vec![input_row(1, None, true), input_row(2, None, false)];
        t.bind(&rows, &first);
        assert_eq!(picks_of(&t, &[1, 2]), vec![Some(0), Some(1)]);
        let held = PickId(0);
        t.hold(&[held]);
        rows.insert(0, input_row(9, None, false));
        let moved = InputStream { device_channels: Some(8), picks: Picks::from_slice(&[Mono(6), Mono(7)]) };
        for s in [&first, &moved] {
            t.bind(&rows, s);
            assert_eq!(t.pick_of(TrackId(1)), Some(held), "the recording track lost its pick");
            let e = t.picks()[held.0 as usize];
            assert_eq!((e.from, e.cols), (Some(Mono(4)), [4, 4]), "the held pick reads another input");
            assert_eq!(picks_of(&t, &[9, 2]), vec![Some(1), Some(2)], "the others do not take the free picks in row order");
        }
        assert_eq!(t.picks()[1].from, Some(Mono(7)), "a free pick does not read the stream at its place");
        t.release(&[held]);
        t.bind(&rows, &moved);
        assert_eq!(picks_of(&t, &[9, 1, 2]), vec![Some(0), Some(1), Some(2)]);
        assert_eq!(t.picks()[0].from, Some(Mono(6)));
    }

    /// **Today's assignment, transcribed** (`push_monitor_settings` at the
    /// base of this wave): the k-th eligible audio row takes pick k, where
    /// eligible is every audio row on a desk that names no input, and only
    /// the rows that name one on a desk where any does; pick k reads the
    /// stream's k-th honoured pick (or the whole device for a stream of
    /// none), by the running offset the renderer walked. Answers each
    /// pick's row, width and two device columns.
    fn todays_assignment(rows: &[InputRow], s: &InputStream) -> Vec<(usize, u8, [u16; 2])> {
        let any_explicit = rows.iter().any(|r| r.audio && r.spec.is_some());
        let dev = s.device_channels.unwrap_or(0);
        let chosen: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.audio && (!any_explicit || r.spec.is_some()))
            .map(|(i, _)| i)
            .take(32)
            .collect();
        let stream_picks = if any_explicit {
            Picks::from_slice(&chosen.iter().filter_map(|i| rows[*i].spec).collect::<Vec<_>>())
        } else {
            s.picks.clone()
        };
        let honoured = stream_picks.fitted(dev.max(1));
        let widths = honoured.widths(dev);
        let mut offset = 0usize;
        let ch_in = dev.max(1);
        chosen
            .iter()
            .enumerate()
            .map(|(k, row)| {
                let w = widths.get(k).copied().map_or(0, usize::from);
                let width = if dev == 0 { 0 } else { w.min(ch_in.saturating_sub(offset)) };
                let at = offset;
                offset += width;
                let col = |c: usize| honoured.source(at + c, ch_in).unwrap_or(at + c).min(ch_in - 1) as u16;
                let cols = if width == 0 { [0, 0] } else { [col(0), col(1.min(width - 1))] };
                (*row, width as u8, cols)
            })
            .collect()
    }

    /// **With no explicit binding anywhere, every fixture keeps today's
    /// input assignment** (W5, done when), and a desk that names its inputs,
    /// bound for the first time, numbers them as today did too. Over every
    /// settings era and DAW blob in the corpus and a few desks of its own
    /// (the shipped one, thirty-two audio tracks naming nothing, a desk
    /// mixing named inputs with NO INPUT rows), against streams of no
    /// device, a stereo box opened whole, and an eight-input box opened on
    /// two of its inputs: each pick's track, width and device columns are
    /// what the walk this replaces gave them.
    #[test]
    fn every_fixture_keeps_todays_input_assignment() {
        use ChannelPick::{Mono, Stereo};
        let mut desks: Vec<(String, Settings)> = Vec::new();
        let eras = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/eras");
        let mut names: Vec<PathBuf> = std::fs::read_dir(&eras)
            .expect("the era corpus")
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let n = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                (n.starts_with("settings-") || n.starts_with("daw-")) && n.ends_with(".json")
            })
            .collect();
        names.sort();
        assert!(names.len() >= 10, "the era corpus shrank to {}", names.len());
        for path in names {
            let text = std::fs::read_to_string(&path).expect("a fixture");
            desks.push((path.display().to_string(), Settings::from_json(&text)));
        }
        desks.push(("shipped".to_owned(), Settings::default()));
        let mut every = blank();
        kind(&mut every, &(0..CHANNELS).collect::<Vec<_>>(), ChannelKind::Audio);
        desks.push(("32 audio".to_owned(), every));
        let mut mixed = blank();
        kind(&mut mixed, &[0, 2, 3, 5, 9], ChannelKind::Audio);
        mixed.rows[2].input = "3".to_owned();
        mixed.rows[5].input = "0+1".to_owned();
        mixed.rows[9].input = "7".to_owned();
        desks.push(("mixed".to_owned(), mixed));
        let streams = [
            InputStream { device_channels: None, picks: Picks::EVERYTHING },
            InputStream { device_channels: Some(2), picks: Picks::EVERYTHING },
            InputStream { device_channels: Some(8), picks: Picks::from_slice(&[Mono(2), Stereo(4, 5)]) },
        ];
        let mut ordinal_desks = 0;
        for (name, settings) in desks {
            let app = app_with(settings);
            let rows = input_rows(&app);
            if !rows.iter().any(|r| r.audio && r.spec.is_some()) {
                ordinal_desks += 1;
            }
            for s in &streams {
                let mut t = PickTable::default();
                t.bind(&rows, s);
                let today = todays_assignment(&rows, s);
                // **Where today misread, the columns are not pinned to it.**
                // A named input the device does not have was dropped from
                // the stream's list, and every binding after it read the
                // NEXT one's columns: a track hearing another track's
                // microphone. Each pick reads its own input now, or nothing.
                let dev = s.device_channels.unwrap_or(0);
                let all_fit = rows.iter().filter_map(|r| r.spec).all(|p| p.fits(dev.max(1)));
                let explicit = rows.iter().any(|r| r.audio && r.spec.is_some());
                for (k, (r, width, cols)) in today.iter().enumerate() {
                    let e = t.picks().get(k).unwrap_or_else(|| panic!("{name} on {s:?}: pick {k} is gone"));
                    assert_eq!(e.track, Some(rows[*r].id), "{name} on {s:?}: pick {k} feeds another track");
                    if !explicit || all_fit {
                        assert_eq!((e.width, e.cols), (*width, *cols), "{name} on {s:?}: pick {k} reads other columns");
                    } else {
                        let own = rows[*r].spec.expect("a named input");
                        let want = if dev > 0 && own.fits(dev) { own.channels() as u8 } else { 0 };
                        assert_eq!(e.width, want, "{name} on {s:?}: pick {k} does not read its own input");
                    }
                }
            }
        }
        assert!(ordinal_desks >= 8, "only {ordinal_desks} desks took their inputs by order");
    }

    /// **The bridge binds every pick to its track's lane and holds a capture
    /// ring for each armed pick and no other** (design 7.12): rings come
    /// with the arm and go with it, and a desk with nothing armed holds
    /// none.
    #[test]
    fn the_bridge_rings_the_armed_picks_and_no_others() {
        let mut s = blank();
        kind(&mut s, &[1, 2, 3], ChannelKind::Audio);
        s.rows[1].input = "0".to_owned();
        s.rows[2].input = "1".to_owned();
        s.rows[3].input = "1".to_owned();
        s.rows[1].arm = true;
        s.rows[3].arm = true;
        let app = app_with(s);
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        p.sync(&app, &mut e);
        let st = stream(2);
        assert_eq!(e.capture_ring_bytes(), 0, "an engine holds a ring before anything was asked");
        p.sync_inputs(&app, &mut e, &st);
        let one = {
            let mut probe = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
            probe.sync_capture_rings(&[PickId(0)]).expect("a ring");
            probe.capture_ring_bytes()
        };
        assert_eq!(e.capture_ring_bytes(), 2 * one, "not a ring per armed pick");
        let ids = app.track_ids();
        let (a, b, c) = (ids[1], ids[2], ids[3]);
        assert_eq!((p.pick_of(a), p.pick_of(b), p.pick_of(c)), (Some(PickId(0)), Some(PickId(1)), Some(PickId(2))));
        assert!(p.pick_table().explicit());
        // Two tracks on one input are two picks of it.
        assert_eq!(p.pick_table().picks()[1].cols, p.pick_table().picks()[2].cols);
        // Disarmed, the rings go home: a frame for the callback to send them,
        // and the next sync drops them.
        let mut rows = input_rows(&app);
        rows.iter_mut().for_each(|r| r.armed = false);
        p.sync_input_rows(&rows, &mut e, &st);
        let mut out = vec![0.0f32; 512 * 2];
        e.render_offline(&mut out);
        p.sync_input_rows(&rows, &mut e, &st);
        assert_eq!(e.capture_ring_bytes(), 0, "a disarmed pick kept its ring");
    }
}
