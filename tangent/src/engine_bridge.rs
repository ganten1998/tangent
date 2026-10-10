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
use tangent_ui::desk_legacy::{bind, decode_v32_strip};
use tangent_ui::recorder::{ChannelKind, Desk, MidiRoute, StripRef};
use tangent_ui::routing::Target;

use crate::desk_plan::{InKey, InLane, InRoute, LaneFlags, PlanInputs, RouteEnd, Slot, SpecialFlags};
use crate::desktop::{
    arm_stem_tap, effect_params_from, export_clip_placement, furnish_mirror_bay, is_instrument_sentinel,
    keys_patch_of, leaf_of, mirror_sampler_samples,
};
use crate::instrument::{
    Engine, LaneKind, LanePart, LaneSpec, PadBank, PadKit, Placed, PlacedMidi, RackAddr, RackOwner, SlotAutomation,
    SlotPlaced, SlotPlacedMidi, LEGACY_LANES,
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
pub fn full_build_with(
    app: &TangentApp,
    rate: u32,
    timebase: Timebase,
    build: BuildFor,
    ctx: &mut Furnish<'_>,
) -> Result<Built, String> {
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
                let fader = c.lane == row
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
            export_clip_placement(app.arrangement(), app.frozen_lanes(), rate, |rel| {
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
    match &build {
        BuildFor::Export(spec) => arm_stem_tap(&e, spec.stems, spec.stems_dry),
        BuildFor::Freeze(_) => arm_stem_tap(&e, true, false),
        BuildFor::Probe => {}
    }
    // Everything sent lands before the first block: an offline engine has
    // no callback to wait for.
    e.settle_offline();
    Ok(Built { engine: e, bridge })
}

/// **The stems an export writes, and what each file is called**, by ROW:
/// only the rows with something to SAY - clips, or an instrument in a bay
/// (the DX7, the drumpad, an SD3 following the transport). A kind with
/// neither is an armed input that renders silence offline, and a folder of
/// silent WAVs reads as a bug. `NN-<track name>.wav`, the name made safe for
/// a file system. The caller binds each row to its lane through the bridge
/// at the press (design 7.13: stems bind `(slot, gen)` at the press).
pub fn stem_files(app: &TangentApp) -> Vec<(usize, String)> {
    let clips = &app.arrangement().clips;
    (0..app.track_ids().len())
        .filter(|&row| {
            clips.iter().any(|c| c.lane == row) || (0..tangent_ui::desk_id::BAYS).any(|b| app.insert(row, b).is_some())
        })
        .map(|row| {
            let name: String = app
                .channel_display_name(row)
                .chars()
                .map(|c| if std::path::is_separator(c) { '-' } else { c })
                .collect();
            (row, format!("{:02}-{}.wav", row + 1, name.trim()))
        })
        .collect()
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

/// **Every bay the desk seats, by identity**: the app's own map (C11's
/// `plugins`), read directly. A track's bays are wherever the track is, so
/// a reorder changes nothing here; a bay whose track no row holds is not in
/// the map at all (a removal takes its track's bays with it).
pub fn desk_bays(app: &TangentApp) -> BTreeMap<SeatBay, String> {
    app.plugins().iter().filter(|(_, p)| !p.is_empty()).map(|(sb, p)| (*sb, p.clone())).collect()
}

/// **The bay at the host's `(seat, bay)`, by identity**, under row ids
/// `ids`: the frozen map's crossing (`desk_legacy::bay_at`), for the host's
/// roads that still hold a seat.
pub fn seat_bay_at(ids: &[TrackId], seat: usize, bay: usize) -> Option<SeatBay> {
    tangent_ui::desk_legacy::bay_at(ids, seat, bay)
}

/// **Where the frozen map keeps a bay**, `(seat, bay)`, under row ids
/// `ids`: [`seat_bay_at`]'s inverse. `None` for a track no row holds.
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

/// Whether row `row` has its bit in one of the app's remaining desk words
/// (the arm, polarity and curve-suspend masks it still keeps by position).
/// The bridge READS the app's words through this one door and builds none
/// of its own: what it hands the engine is per-lane flags in a plan.
fn row_bit(word: u64, row: usize) -> bool {
    u32::try_from(row).ok().and_then(|r| 1u64.checked_shl(r)).is_some_and(|b| word & b != 0)
}

/// **Whether row `row` is armed**, read through [`row_bit`], the one door
/// onto the app's arm word, so the host asks a row and never builds or
/// reads a mask of its own.
pub fn row_armed(app: &TangentApp, row: usize) -> bool {
    row_bit(u64::from(app.armed_mask()), row)
}

/// **The plugin path in a bay of a track, by identity**, read off the app's
/// map: the host names a track's bay and never works out a seat.
pub fn track_bay(app: &TangentApp, id: TrackId, rack: Rack, bay: u8) -> Option<&str> {
    app.plugin(SeatBay::new(Node::Track(id), rack, bay))
}

/// [`track_bay`] for the track on row `row`'s first bay: what holds its
/// instrument.
pub fn row_instrument(app: &TangentApp, row: usize) -> Option<&str> {
    track_bay(app, *app.track_ids().get(row)?, Rack::Chain(0), 0)
}

/// **The node a strip of the app's desk is**, for the roads that still hand
/// the host a strip (a face, a drop on a column): the frozen strip space
/// (`decode_v32_strip`) bound to the desk's rows, so a track is named by its
/// identity and no strip index is turned into a seat by hand.
pub fn strip_node(app: &TangentApp, strip: usize) -> Option<Node> {
    bind(decode_v32_strip(strip)?, app.track_ids())
}

/// **The bay of a strip's displayed slot, by identity** (`desk_id::slot_key`,
/// the one display-slot resolver: the master's two racks show the other way
/// round).
pub fn strip_slot(app: &TangentApp, strip: usize, slot: usize) -> Option<SeatBay> {
    tangent_ui::desk_id::slot_key(strip_node(app, strip)?, slot)
}

/// **The plugin path in a bay, by identity**, off the app's map.
pub fn bay_path(app: &TangentApp, sb: SeatBay) -> Option<&str> {
    app.plugin(sb)
}

/// **Every rack a track has**: its two chains, its pedal board and its pad
/// racks, as the frozen map gives a track (design Appendix A). What follows a
/// track wherever its row goes.
pub fn track_racks() -> impl Iterator<Item = Rack> {
    let chains = (0..tangent_ui::desk_id::STRIP_RACKS).map(|c| Rack::Chain(c as u16));
    let pedals = (0..tangent_ui::desk_id::AMP_PEDALS).map(|p| Rack::Pedal(p as u8));
    let pads = (0..tangent_ui::desk_id::PADS).map(|p| Rack::Pad(p as u8));
    chains.chain(pedals).chain(pads)
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
    /// The inputs of the last plan the engine took, for the change test.
    published: Option<PlanInputs>,
    /// The bypass table last stored, by the engine's names.
    bypass: Option<Vec<EngineBay>>,
    /// Where an arrangement record is replacing each row's schedule, in
    /// frames, handed in by the host each frame (it owns the record).
    punches: Vec<Option<u64>>,
    /// **The strip-space desk the last [`Self::sync`] built, for the host's
    /// take facts alone** (which strips a take was cut under, `Bridge::desk`):
    /// built ONCE a frame. The bridge itself reads the desk by what each
    /// strip is (`plan_inputs`, `store_levels`) and never this; it is relayed
    /// until the host's take road moves off the strip-space form (C11).
    desk: Desk,
    stats: BridgeStats,
    /// A test's ask to run an offline engine's removals through the live
    /// two-phase road (RETIRING, then home), which an offline engine skips.
    #[cfg(test)]
    staged: bool,
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
}

impl Bridge {
    pub fn new() -> Self {
        Self::default()
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

    /// The desk the last sync built and compared.
    pub fn desk(&self) -> Desk {
        self.desk
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
    pub fn engine_bay(&self, sb: SeatBay) -> Option<EngineBay> {
        match sb.seat.node {
            Node::Track(id) => lane_bay(self.lanes.get(&id)?.at.slot, sb.seat.rack, sb.bay),
            node => special_bay(node, sb.seat.rack, sb.bay),
        }
    }

    /// **The engine's name for the app's `(seat, bay)`**, under the app's
    /// rows now: the app still keeps its bays by position.
    pub fn engine_seat(&self, app: &TangentApp, seat: usize, bay: usize) -> Option<EngineBay> {
        self.engine_bay(seat_bay_at(app.track_ids(), seat, bay)?)
    }

    /// **The bay the engine names `at`, by identity**: the inverse of
    /// [`Self::engine_bay`], for what the engine reports by its racks. A
    /// lane's rack is its track's wherever the row is; one whose slot no
    /// track holds (a lane on its way home) is nobody's.
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

    /// **The app's `(seat, bay)` for the engine's bay**, under the app's rows
    /// now.
    pub fn app_seat(&self, app: &TangentApp, at: EngineBay) -> Option<(usize, usize)> {
        app_seat_of(app.track_ids(), self.bay_at_engine(at)?)
    }

    /// Every row whose first bay holds `sentinel`, with its lane: the drum,
    /// DX7 and keyboard lanes by the bay-1 sentinel rule.
    pub fn lanes_holding(&self, app: &TangentApp, sentinel: &str) -> Vec<(usize, LaneAt)> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, (id, _))| track_bay(app, *id, Rack::Chain(0), 0) == Some(sentinel))
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
    pub fn slot_automation(&self, auto: &tangent_ui::automation::Automation) -> SlotAutomation {
        let master = tangent_ui::arrangement_panel::LaneId::Master.auto_lane();
        let mut gens = Vec::new();
        let mut curves = Vec::new();
        for c in auto.curves() {
            if c.lane == master {
                curves.push(c.clone());
                continue;
            }
            let Some(at) = self.lane(c.lane) else { continue };
            let mut moved = c.clone();
            moved.lane = at.slot.0 as usize;
            gens.push((at.slot, at.gen));
            curves.push(moved);
        }
        gens.sort_unstable_by_key(|(s, _)| *s);
        gens.dedup();
        SlotAutomation { curves: tangent_ui::automation::Automation::from_curves(curves), gens, master }
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
        let ids: Vec<TrackId> = app.track_ids().to_vec();
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
        synced.moved.extend(self.install(app, e, &ids));
        self.attach_pads(app, e, &ids);
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
        // The host's take facts, relayed (see `desk`): nothing below reads it.
        self.desk = app.desk();
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

    /// **A lane for every track that has none**, at the lowest free slot
    /// below [`LEGACY_LANES`], in one Install. A track whose old lane is
    /// still on its way home waits for it; a track with no free slot waits
    /// for one (in this wave every slot past the legacy width lacks the pad
    /// racks and the automation lane a lane needs, so the bridge never
    /// reaches past it; its plugin racks and its instruments are already
    /// reached by identity at any slot). Answers the slots it installed at.
    fn install(&mut self, app: &TangentApp, e: &mut Engine, ids: &[TrackId]) -> Vec<Slot> {
        let mut taken: BTreeSet<Slot> = self.lanes.values().map(|h| h.at.slot).collect();
        taken.extend(self.leaving.iter().map(|l| l.at.slot));
        let mut batch: Vec<(TrackId, Slot, LaneSpec)> = Vec::new();
        for (row, id) in ids.iter().enumerate() {
            if self.lanes.contains_key(id)
                || self.leaving.iter().any(|l| l.id == *id)
                || batch.iter().any(|(b, _, _)| b == id)
            {
                continue;
            }
            let Some(slot) = (0..LEGACY_LANES as u32).map(Slot).find(|s| !taken.contains(s) && e.slot_free(*s))
            else {
                break;
            };
            taken.insert(slot);
            let kind = match app.channel_kind(row) {
                Some(ChannelKind::Midi) => LaneKind::Midi,
                Some(ChannelKind::Audio) => LaneKind::Audio,
                None if app.desk_index().is_folder(row) => LaneKind::Folder,
                None => LaneKind::Empty,
            };
            let drum = track_bay(app, *id, Rack::Chain(0), 0) == Some(tangent_ui::dialogs::DRUMPAD_PATH);
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
    /// Attach(Kit). Nothing reloads").
    fn attach_pads(&mut self, app: &TangentApp, e: &mut Engine, ids: &[TrackId]) {
        for id in ids {
            let Some(h) = self.lanes.get_mut(id) else { continue };
            if h.pads || track_bay(app, *id, Rack::Chain(0), 0) != Some(tangent_ui::dialogs::DRUMPAD_PATH) {
                continue;
            }
            if e.attach(h.at.slot, h.at.gen, LanePart::Pads(Box::new(PadBank::new()))).is_ok() {
                h.pads = true;
                self.stats.attaches += 1;
            }
        }
    }

    /// The lanes whose row holds `sentinel` in its first bay: on the live
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
    /// route gains that moved into every plan in flight (design 7.6: a gain
    /// that stays on its side of zero scales, it does not reorder). Answers
    /// what became of them.
    fn publish(&mut self, e: &mut Engine, inputs: PlanInputs) -> Published {
        let same_shape = self.published.as_ref().is_some_and(|p| structure(p) == structure(&inputs));
        if same_shape {
            if let Some(p) = self.published.as_ref() {
                for (old, new) in p.routes.iter().zip(&inputs.routes) {
                    if old.gain.to_bits() != new.gain.to_bits() {
                        e.set_route_gain(new.from, new.to, new.gain);
                    }
                }
            }
            self.published = Some(inputs);
            return Published::Same;
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
        let armed = if hears { u64::from(app.armed_mask()) } else { 0 };
        let frozen: BTreeSet<usize> = app.frozen_lanes().iter().map(|z| z.lane).collect();
        // **Armed, or alone in the editor** — the whole rule, in one place,
        // tested there. A frozen lane listens to nothing: its rack rests and
        // its file plays, so a note into it would sound through nothing
        // anyway. An export and a freeze hear no live input of any kind.
        let deaf = if hears {
            let kinds: Vec<_> = (0..n).map(|r| app.channel_kind(r)).collect();
            u64::from(tangent_ui::recorder::live_midi_off(&kinds, app.armed_mask(), app.midi_editor_lane()))
        } else {
            u64::MAX
        };
        let phase = app.strip_phase();
        let suspend = app.auto_suspend_mask();
        let parents = app.folder_parents();
        let routes_midi = app.midi_routes();
        // **The desk by what each strip is** (W5): a row's mute and solo as
        // the engine hears them (a folder's reaching its children), the
        // specials' their own, never an index into a strip-space desk.
        let soloed = |row: usize| {
            freeze.is_none()
                && (app.strip_heard_soloed(StripRef::Row(row)) || probe.is_some_and(|p| p.solo.contains(&row)))
        };
        let muted = |row: usize| freeze.is_none() && app.strip_heard_muted(StripRef::Row(row));
        let mut lanes = Vec::with_capacity(n + self.leaving.len());
        for (row, (id, at)) in self.rows.iter().enumerate() {
            let Some(at) = at else { continue };
            let first = track_bay(app, *id, Rack::Chain(0), 0);
            let is_frozen = frozen.contains(&row);
            let midi_ok = !live_rule || app.channel_kind(row) == Some(ChannelKind::Midi);
            let flags = F::default()
                .with(F::ARMED, row_bit(armed, row))
                .with(F::MIDI_OFF, row_bit(deaf, row) || is_frozen)
                .with(F::FROZEN, is_frozen)
                .with(F::DRUM, first == Some(tangent_ui::dialogs::DRUMPAD_PATH))
                .with(F::BUILTIN, midi_ok && first == Some(tangent_ui::dialogs::BUILTIN_PATH) && !is_frozen)
                .with(F::KEYS, midi_ok && first == Some(tangent_ui::dialogs::KEYS_PATH) && !is_frozen)
                .with(F::MUTED, muted(row))
                .with(F::SOLOED, soloed(row))
                .with(F::PHASE, row_bit(phase, row))
                .with(F::SUSPEND, row_bit(suspend, row));
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
        let master_lane = tangent_ui::arrangement_panel::LaneId::Master.auto_lane();
        let specials = SpecialFlags {
            drums: special(StripRef::Drums),
            click: special(StripRef::Click),
            fx: special(StripRef::Fx),
            master: special(StripRef::Master).with(F::SUSPEND, row_bit(suspend, master_lane)),
        };
        // **The wiring, by identity** (`read_desk`): the routing table pruned
        // against the device, and the sidechain keys, in the engine's names.
        let (routes, keys) = self.read_desk(app, outs);
        PlanInputs { lanes, specials, routes, sends: Vec::new(), keys }
    }

    /// **The continuous values, by slot**: each row's fader, pan and Fx send
    /// into its lane's shared numbers, the two sending specials' sends, and
    /// every keyboard lane's patch and knobs. Atomic stores, every frame:
    /// writing an unchanged value costs nothing, and a cache of "what the
    /// engine holds" would be one more thing to disagree with the desk.
    fn store_levels(&self, app: &TangentApp, e: &Engine, shape: Shape<'_>) {
        let gains = app.gains();
        let freeze = shape.freeze();
        for (row, (_, at)) in self.rows.iter().enumerate() {
            let Some(at) = at else { continue };
            // A freeze renders post-rack and PRE-fader: unity gain, centre
            // pan (unity there).
            let (gain, pan) = if freeze == Some(row) {
                (1.0, 0.0)
            } else {
                (gains.channels.get(row).copied().unwrap_or(1.0), app.channel_pan(row))
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
    /// Nothing that decides an order moved; the gains, if any, were written.
    Same,
    /// The op ring was full.
    Failed,
}

/// **What decides an order**, of a plan's inputs: everything but the route
/// gains' size, which only scale (a gain crossing zero does decide, so its
/// side is kept).
fn structure(inputs: &PlanInputs) -> PlanInputs {
    let mut s = inputs.clone();
    for r in &mut s.routes {
        r.gain = if r.gain.is_finite() && r.gain > 0.0 { 1.0 } else { 0.0 };
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    use tangent_ui::arrangement::{ArrClip, ArrNote, Arrangement, ClipKind, FrozenLane};
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
    use crate::instrument::MASTER_AUTO_LANE;

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
        s.plugins.clear();
        s.strip_sends = [0.0; CHANNELS];
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
        modes: Vec<(usize, tangent_ui::automation::AutoMode)>,
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
        s.strip_sends[3] = 0.5;
        let none: Vec<Option<u64>> = Vec::new();
        let mut modes: Vec<(usize, tangent_ui::automation::AutoMode)> = Vec::new();
        let mut out = vec![Step { name: "desk", settings: s.clone(), punches: none.clone(), modes: modes.clone() }];
        let mut edit = |name: &'static str, f: &dyn Fn(&mut Settings, &mut Vec<_>), punches: Vec<Option<u64>>| {
            f(&mut s, &mut modes);
            out.push(Step { name, settings: s.clone(), punches, modes: modes.clone() });
        };
        edit("arm", &|s, _| s.channel_armed = Some(1 << 0 | 1 << 4), none.clone());
        edit("mute", &|s, _| s.strip_muted |= 1 << 2, none.clone());
        edit("solo", &|s, _| s.strip_soloed |= 1 << 4, none.clone());
        edit(
            "phase",
            &|s, _| {
                s.strip_phase |= 1 << 1;
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
                s.strip_is_folder[5] = true;
                s.strip_in_folder[6] = true;
                s.strip_in_folder[7] = true;
                s.strip_muted |= 1 << 5;
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
                s.channel_midi_inputs = vec![tangent_ui::recorder::MidiInputSel::EVERYTHING; CHANNELS];
                s.channel_midi_inputs[1] =
                    tangent_ui::recorder::MidiInputSel { devices: vec!["Keystation".to_owned()], channel: Some(9) };
            },
            none.clone(),
        );
        edit(
            "auto-modes",
            &|_, m| {
                use tangent_ui::automation::AutoMode;
                m.push((3, AutoMode::Off));
                m.push((4, AutoMode::Write));
                // The master's lane is bit 32: past a u32, which is the
                // aliasing `auto_suspend_mask` was widened to stop.
                m.push((tangent_ui::arrangement_panel::LaneId::Master.auto_lane(), AutoMode::Touch));
            },
            none.clone(),
        );
        edit(
            "freeze",
            &|s, _| s.frozen_lanes.push(FrozenLane { lane: 0, path: "media/f.wav".into(), len_s: 1.0 }),
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

        s.frozen_lanes.push(FrozenLane { lane: 1, path: "media/f.wav".into(), len_s: 1.0 });
        p.sync(&app_with(s.clone()), &mut e);
        assert_eq!(read(&e, "frozen"), serde_json::json!(1 << 1), "a freeze waited for some other edit");
        assert_eq!(p.stats().plans, plans + 1, "a freeze is not one plan");

        s.render_ahead_ms = 10.0;
        p.sync(&app_with(s.clone()), &mut e);
        assert_eq!(read(&e, "grain"), serde_json::json!(RATE / 100), "a render-ahead change waited for some other edit");
        assert_eq!(p.stats().plans, plans + 1, "a render-ahead change rebuilt the plan");

        s.frozen_lanes.clear();
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
        s.routes.add(Route::new(row(&s, 4), Target::Node(row(&s, 5))));
        s.routes.add(Route::new(row(&s, 0), Target::Hardware { first: 0, stereo: true }));
        bay(&mut s, 5, 0, EQ_PATH);
        bay(&mut s, 3, 0, COMP_PATH);
        s.insert_keys.set(legacy_sb(&s, 3, 0), Some(row(&s, 2)));
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
        // The Trim at its defaults, and LIVE: with no live state the export
        // falls back to the shelf, which is the real home's sidecar outside
        // the sandbox (see `every_corpus_bay_is_live`).
        let live = vec![(0, 0, TRIM_PATH, dialled(TRIM_PATH, &[]))];
        out.push(Corpus { name: "automation", settings: s, doc, live, kits: vec![], freeze: vec![0] });

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
        if let Some(row) = freeze {
            let name = format!("frozen-{:02}.wav", row + 1);
            let path = dir.join(&name);
            files.push((name, path.clone(), WavWriter::create(&path, wav_spec, &bext).expect("freeze"), lane(row), false));
        } else if spec.stems {
            std::fs::create_dir_all(dir.join(spec.stems_folder())).expect("stems dir");
            for (row, file) in stem_files(app) {
                let name = format!("{}/{file}", spec.stems_folder());
                let path = dir.join(&name);
                files.push((name, path.clone(), WavWriter::create(&path, wav_spec, &bext).expect("stem"), lane(row), false));
            }
        }
        let freeze = freeze.map(lane);
        let ratio = f64::from(app.varispeed_ratio()).clamp(0.5, 2.0);
        let until = (((span + 2.0) / ratio) * f64::from(rate)).ceil() as u64;
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
            let (seat, bay) = app_seat_of(app.track_ids(), sb).expect("the bay's seat");
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
                        .filter(|ch| app.insert(*ch, 0) == Some(tangent_ui::dialogs::BUILTIN_PATH))
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
        s.strip_muted |= 1 << 1;
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
        assert_eq!(body.matches("app.desk()").count(), 1, "the desk is built more than once a frame");
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
        s.channel_armed = Some(1 << 1 | 1 << 2);
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

    /// **A row past the app's word has no bit**, where a clamp to the last
    /// bit would give it another row's: the alias a wider desk would hit
    /// first. The bridge reads the app's remaining words through this alone.
    #[test]
    fn a_row_bit_is_its_own_or_none() {
        for row in 0..64 {
            assert!(row_bit(1u64 << row, row), "row {row} lost its own bit");
            assert!(!row_bit(!(1u64 << row), row), "row {row} read another row's bit");
        }
        for row in [64, 65, usize::MAX] {
            assert!(!row_bit(u64::MAX, row), "row {row} aliases a row inside the word");
        }
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
        assert_eq!((s.strip_muted, s.strip_soloed, s.strip_phase), (1 << 1, 0, 0), "fixture: a special sat in a row's mask");
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
        let at = p.engine_bay(seat_bay_at(app.track_ids(), 1, 0).expect("the bay")).expect("its seat");
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
        let sb = seat_bay_at(app.track_ids(), 1, 0).expect("the bay");
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
        let comp = p.engine_bay(seat_bay_at(app.track_ids(), 1, 0).expect("the comp's bay")).expect("its rack");
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
    /// answers what it holds and the state it plays, and a lane's pad rack
    /// past the map's pads is named but not made yet, so it holds nothing.
    /// When the lane goes home, its racks' records go with it.
    #[test]
    fn a_plugin_is_reached_through_its_racks_host_record_at_any_slot() {
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let slot = Slot(40);
        e.install_lanes(vec![(slot, LaneSpec { kind: LaneKind::Audio, drum: false })]).expect("a lane at 40");
        e.settle_offline();
        let dial = dialled(COMP_PATH, &[(0, 0.2), (1, 0.9)]);
        let racks = [Rack::Chain(0), Rack::Chain(1), Rack::Pedal(3)];
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
        let (pad, bay) = lane_bay(slot, Rack::Pad(5), 0).expect("a pad rack is a rack of the lane");
        assert_eq!(
            e.load_insert_at(pad, bay, Some(Path::new(COMP_PATH)), None).expect("no error"),
            None,
            "a pad rack past the map's pads seated a plugin"
        );
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
    /// a Plan without it, then Remove"), and a new track waits for the slot
    /// until the lane is home. Driven on an offline engine through the live
    /// road (`stage_removals_for_test`), a block rendered between frames for
    /// the callback that takes each op.
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
        let fresh = app.track_ids()[1];
        let before = p.stats();

        // Frame one: the lane is still there, fading.
        p.sync(&app, &mut e);
        let plan = p.published_for_test().expect("a plan");
        let retiring = plan.lanes.iter().find(|l| l.slot == slot).expect("the leaving lane left the plan at once");
        assert!(retiring.flags.contains(LaneFlags::RETIRING), "the leaving lane is not RETIRING");
        assert!(e.lane_gen(slot).is_some(), "the lane went home before its fade");
        assert_eq!(p.lane_of(fresh), None, "the new track took a slot whose lane is still fading");
        assert_eq!(p.stats().removes, before.removes, "the lane was sent home under the plan that fades it");
        block(&mut e);
        std::thread::sleep(RETIRE_RAMP * 2);

        // Frame two: the plan without it, and home.
        p.sync(&app, &mut e);
        assert!(p.published_for_test().expect("a plan").lanes.iter().all(|l| l.slot != slot), "the lane is still planned");
        assert_eq!(p.stats().removes, before.removes + 1, "the lane was not sent home");
        block(&mut e);

        // Frame three: the slot is free and the new track has a lane.
        p.sync(&app, &mut e);
        p.sync(&app, &mut e);
        assert!(p.lane_of(fresh).is_some(), "the new track never got a lane");
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
}
