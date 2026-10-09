//! **The one seam between the app and the engine** (C9 in
//! `docs/design/unlimited-tracks.md`).
//!
//! Two jobs, each of which used to be spelled inside `DesktopApp` where no
//! test could reach it:
//!
//! - [`Bridge::sync`]: the desk's STRUCTURE, pushed into the live engine
//!   once a frame. The routing, key and bypass tables and the mute/solo/send
//!   desk on change (the `PushedDesk` gate that used to sit in
//!   `push_monitor_settings`), and beside them the words a frame has always
//!   stored whole: the arm and live-MIDI masks, the MIDI routes, the
//!   punches, the frozen mask and the render-ahead, the instrument and drum
//!   strip masks, the folders, polarity and the curves' suspend mask. The faders, the click, the effects and the
//!   instruments' own instances stay with the host for now: they are atomic
//!   stores or ring work that no structural edit changes.
//! - [`furnish_offline`]: a separate OFFLINE engine furnished from the app
//!   alone, the one builder an export, a freeze and the eligibility probe
//!   all come through ([`BuildFor`] says which). It used to be two hand
//!   copies of the live push, `start_offline_render`'s and the probe's, and
//!   the probe's copy had drifted (no effects, no automation, no instruments,
//!   no polarity, no frozen mask), so a click hunt over a real song rendered
//!   a song nobody was listening to.
//!
//! **No behaviour change**, held to bytes rather than to a reading: the
//! harness below scripts desk edits through a real app into an offline
//! engine and compares what the engine holds after each frame against the
//! golden `DesktopApp` produced before the move
//! (`tests/fixtures/exports/bridge.json`), and the export goldens hash every
//! file a corpus of built-in desks exports (`golden.json`). **One change
//! on purpose, after the move**: the frozen mask and the render-ahead are
//! stored every frame instead of behind the change gate (see
//! `Bridge::sync`), so `bridge.json` departs from the base recording in
//! exactly those two words, from the frames that change them. W4 grows this
//! file into the broker (`TrackId -> Slot`, plans, `full_build`) and holds
//! it to the same two files.

use std::collections::BTreeMap;
use std::sync::Arc;

use tangent_record::audio::Timebase;
use tangent_ui::app::TangentApp;
use tangent_ui::desk_id::TrackId;
use tangent_ui::recorder::{ChannelKind, Desk, CHANNELS, INSERTS};
use tangent_ui::routing::{KeyEdges, Routes};

use crate::desktop::{
    arm_stem_tap, effect_params_from, export_clip_placement, frozen_lane_mask, furnish_mirror_bay,
    is_instrument_sentinel, keys_patch_of, leaf_of, mirror_sampler_samples,
};
use crate::instrument::Engine;

/// What an export is asked for: the Export dialog's own spec.
pub type ExportSpec = tangent_ui::ports::ProjectExportSpec;

/// **Which offline engine [`furnish_offline`] is building.** The three
/// differ in exactly what the comments on each arm say and nowhere else, so
/// a fix to the furnishing reaches all three by construction.
#[derive(Clone, Debug)]
pub enum BuildFor {
    /// A full-speed export: no live input of any kind, the click out, the
    /// stem tap armed as the spec asks.
    Export(ExportSpec),
    /// The eligibility and click-hunt probe: the arm and live-MIDI state
    /// the session actually has (`set_armed(0)` and every lane deaf would
    /// make every lane eligible by construction, which is the one question
    /// the probe asks), plus the knobs in [`Furnish::probe`].
    #[cfg_attr(not(test), allow(dead_code, reason = "the probe is a test harness until W4's full_build"))]
    Probe,
    /// A freeze of one track: the export's engine with that lane made
    /// pre-fader (unity, centre, every mute and solo off, its own fader
    /// rides left out) and only its wet stem tapped.
    ///
    /// **A `TrackId`, read as its row by the identity rule** (legacy row r
    /// is `TrackId(r)`; section 3 of the design) until W3's `row_of` gives
    /// the bridge a row map: no structural edit exists yet that could move a
    /// track between the press and the render.
    Freeze(TrackId),
}

impl BuildFor {
    /// The lane a freeze neutralises, by the identity rule.
    fn frozen_lane(&self) -> Option<usize> {
        match self {
            Self::Freeze(id) => usize::try_from(id.0).ok(),
            Self::Export(_) | Self::Probe => None,
        }
    }
}

/// The probe's own knobs - `TANGENT_SOLO`, `TANGENT_BYPASS`, `TANGENT_SKIP`,
/// `TANGENT_NO_INSERTS`, `TANGENT_GRAIN` and `TANGENT_PROBE_FANOUT`, read by
/// the harness that drives it. Inert for every other [`BuildFor`].
#[derive(Clone, Debug, Default)]
pub struct ProbeKnobs {
    /// Lanes soloed on top of the desk, so one track can be heard through
    /// the same master chain the whole mix goes through.
    pub solo: Vec<usize>,
    /// `(seat, bay)`s taken out of circuit on top of the desk's own table.
    pub bypass: Vec<(usize, usize)>,
    /// `(seat, bay)`s left EMPTY: a bypassed bay still holds its plugin,
    /// still reports latency; this is the other question.
    pub skip: Vec<(usize, usize)>,
    /// No bays at all: a song with analog-modelled units does not render
    /// the same twice, and stripped of them it is arithmetic two builds can
    /// be differenced on.
    pub no_inserts: bool,
    /// Render-ahead in milliseconds, over the app's own setting.
    pub grain_ms: Option<f64>,
    /// Spread the render over the cores, as an export always does.
    pub fanout: bool,
}

/// What [`furnish_offline_with`] is handed beyond the app, and what it hands
/// back beside the engine.
#[derive(Default)]
pub struct Furnish<'a> {
    /// **The live engine, whose racks are wearing the knobs as dialled this
    /// second** - see `desk_insert_state`. An export furnished from the
    /// sidecars alone rendered every unit dialled in since the last save at
    /// its old settings. `None` reads the shelf.
    pub live: Option<&'a Engine>,
    /// Each DX7 lane's voice, already read out of its strip's OWN cartridge
    /// by whoever keeps the cartridge cache (the desktop does). A lane
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

/// [`furnish_offline_with`] with nothing handed in: no live engine (states
/// from the shelf), every DX7 cartridge read from disk, no probe knobs.
#[cfg_attr(not(test), allow(dead_code, reason = "the host hands its live engine in; W4's full_build is the caller"))]
pub fn furnish_offline(
    app: &TangentApp,
    rate: u32,
    timebase: Timebase,
    build: BuildFor,
) -> Result<Engine, String> {
    furnish_offline_with(app, rate, timebase, build, &mut Furnish::default())
}

/// **An offline engine furnished exactly like the live one**: desk, racks
/// with the knobs the live racks are wearing, clips, pads, samplers, the
/// instruments' instances and voices - built from the app alone, so an
/// export, a freeze and the probe run without a `DesktopApp`.
///
/// The transport is left where a fresh engine has it: the caller places
/// and rolls it, because where the render starts is the caller's span.
pub fn furnish_offline_with(
    app: &TangentApp,
    rate: u32,
    timebase: Timebase,
    build: BuildFor,
    ctx: &mut Furnish<'_>,
) -> Result<Engine, String> {
    let freeze = build.frozen_lane();
    let probe = matches!(build, BuildFor::Probe);
    let mut e = Engine::offline(rate, timebase)?;
    // **Spread the lanes over the other cores.**
    //
    // A bounce and a freeze both come through here, and until now both
    // ran on one core: measured on 96 hosted reverbs, 13.11 s of wall
    // for twelve seconds of audio against 13.32 s of CPU. With this it
    // is 3.79 s. The threads are ordinary ones at ordinary priority -
    // see `LanePool::new` for why a bounce must not be real-time - and
    // the fold sums lanes in ascending order either way, so the file is
    // the same file. `an_exported_file_is_the_same_whether_or_not_it_forked`
    // holds that against a real render rather than against a bench. The
    // probe forks only when asked, so a real project can be rendered both
    // ways and the two files compared byte for byte.
    if !probe || ctx.probe.fanout {
        e.set_offline_fanout(true);
    }
    // ── the desk, exactly as the live walk pushes it ──
    {
        let mut desk = app.desk();
        if freeze.is_some() {
            // Nothing may silence the lane being frozen, and a mute or a
            // solo on the desk would - through its own bit or a folder's.
            desk.muted.iter_mut().for_each(|m| *m = false);
            desk.soloed.iter_mut().for_each(|m| *m = false);
        }
        if probe {
            for &lane in &ctx.probe.solo {
                if let Some(f) = desk.soloed.get_mut(lane) {
                    *f = true;
                }
            }
        }
        e.set_desk(&desk);
    }
    let gains = app.gains();
    for ch in 0..CHANNELS {
        e.set_channel_gain(ch, gains.channels[ch]);
        e.set_channel_pan(ch, app.channel_pan(ch));
    }
    if let Some(lane) = freeze {
        // Post-rack and PRE-fader: unity gain, centre pan (unity there).
        e.set_channel_gain(lane, 1.0);
        e.set_channel_pan(lane, 0.0);
    }
    if probe {
        // **The live arm and MIDI state, which is the whole point**, through
        // the same `live_midi_off` rule the live walk uses.
        let armed = app.armed_mask();
        e.set_midi_off(live_midi_off(app, armed));
        e.set_armed(armed);
    } else {
        // No live input of any kind: every arm down, every port silent.
        e.set_midi_off(u64::MAX);
        e.set_armed(0);
    }
    e.set_monitor_on(false);
    e.set_drums(app.drums_gain(), app.drums_pan());
    // **The routing table, like every other desk value.** Pruned first
    // against what the device actually has: an interface swapped for a
    // smaller one leaves routes pointing at jacks that are gone, and a
    // fader that moves and does nothing is worse than a route that
    // admitted it went away. See `Routes::prune`.
    //
    // **In an EXPORT this drops the hardware routes and keeps the
    // sends**, which is exactly right: a rendered file has no jacks, but
    // a channel sent through another channel's reverb is part of the
    // mix and has to be in the file.
    publish_tables(app, &e);
    publish_freeze_and_grain(app, &e);
    if probe {
        if !ctx.probe.bypass.is_empty() {
            let mut bypass: Vec<(usize, usize)> = app.insert_bypass().to_vec();
            bypass.extend(ctx.probe.bypass.iter().copied());
            engine_set_insert_bypass(&e, &bypass);
        }
        if let Some(ms) = ctx.probe.grain_ms {
            e.set_grain_ms(ms);
        }
    }
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
        // Polarity, one bit per strip — see `Shared::strip_phase`.
        e.set_strip_phase(app.strip_phase());
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
    e.set_automation(match freeze {
        Some(lane) => {
            // The lane's fader stays live, so its rides are not rendered.
            let mut out = tangent_ui::automation::Automation::default();
            for c in app.engine_automation().curves() {
                let fader = c.lane == lane
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
    });
    // The built-in: rendered by the engine itself when a channel's
    // first bay names it — the live walk's own rule. **Not on a frozen
    // lane**, as live: its file carries what the built-in played (see
    // `export_clip_placement`, which withholds the lane's MIDI too).
    let thawed = !frozen_lane_mask(app.frozen_lanes());
    let builtin_lanes = lanes_holding(app, tangent_ui::dialogs::BUILTIN_PATH);
    e.set_builtin_strips(builtin_lanes & thawed);
    // **Every lane holding the drumpad**, not the first: a drum
    // instrument loads a full instance per channel everywhere else.
    e.set_drum_strips(lanes_holding(app, tangent_ui::dialogs::DRUMPAD_PATH));
    // **And the modelled keyboard, in the EXPORT walk too.** An instrument
    // wired into the live engine and forgotten here is the worst kind of
    // export bug: everything sounds right while it is being played and the
    // rendered file is missing a track, which nobody finds until the mix
    // is somewhere else.
    let keys_lanes = lanes_holding(app, tangent_ui::dialogs::KEYS_PATH);
    e.set_keys_strips(keys_lanes & thawed);
    for ch in (0..CHANNELS).filter(|ch| keys_lanes & lane_bit(*ch) != 0) {
        e.set_keys_patch(ch, keys_patch_of(app.keys_patch_of(ch)));
        e.set_keys_params(ch, &app.keys_params_of(ch));
    }
    // Groups are part of the mix, so the BOUNCE has them too — an export
    // that ignored the folder buses would render a different record from
    // the one being listened to, which is the worst kind of export bug.
    e.set_folders(app.folder_parents());
    // Instances for every instrument lane — a fresh offline engine has
    // none — then each DX7 lane's sound out of its strip's OWN
    // cartridge. An export whose second DX7 track rendered with the first
    // track's bank would be the same class of quiet wrongness as the bar
    // lines being elsewhere.
    let (made_dx7, _) = e.sync_instrument_lanes(builtin_lanes, keys_lanes);
    for ch in (0..CHANNELS).filter(|ch| made_dx7 & lane_bit(*ch) != 0) {
        let v = match ctx.dx7.get(&ch) {
            Some(v) => *v,
            None => dx7_voice(app, ch),
        };
        e.set_builtin_voice(ch, v);
    }
    // ── every configured rack seat, saved state included ──
    //
    // **From the DESK, not from the shelf** — see `desk_insert_state`. The
    // mirror used to be furnished from the sidecars alone, so a unit
    // dialled in since the last save rendered at whatever its bay had been
    // wearing then — and when that was one of the settings a unit sits at
    // before anybody dials it, the "processed" stem came out identical to
    // the dry one. Refusals are collected, not said: see `Furnish::refused`.
    if !(probe && ctx.probe.no_inserts) {
        for i in 0..(crate::instrument::RACK_SEATS * INSERTS) {
            let (seat, bay) = (i / INSERTS, i % INSERTS);
            // Instrument sentinels only — effect sentinels load into the
            // offline engine's racks exactly as they do the live one's.
            let Some(path) = app.insert(seat, bay).filter(|p| !is_instrument_sentinel(p)) else {
                continue;
            };
            if probe && ctx.probe.skip.contains(&(seat, bay)) {
                continue;
            }
            if let Err(err) = furnish_mirror_bay(&mut e, ctx.live, seat, bay, path) {
                ctx.refused.push(format!("{} ({err})", leaf_of(path)));
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
        e.set_clips(placed);
        e.set_midi_clips(midi_placed);
    }
    // ── the pads, decoded at the export rate ──
    //
    // **Each drum lane's own kit.** The live path does the same; an export
    // that gave every lane one kit would render a song nobody was
    // listening to.
    let drum_lanes = lanes_holding(app, tangent_ui::dialogs::DRUMPAD_PATH);
    for lane in (0..CHANNELS).filter(|ch| drum_lanes & lane_bit(*ch) != 0) {
        let mut kit = crate::instrument::PadKit::default();
        for (i, path) in app.drumpad_paths_of(lane).iter().enumerate() {
            let Some(path) = path else { continue };
            if let (Ok(c), Some(slot)) = (tangent_record::decode::decode(path, rate), kit.pads.get_mut(i)) {
                *slot = Some(Arc::new(c));
            }
        }
        e.set_pad_kit(lane, kit);
    }
    for lane in (0..CHANNELS).filter(|ch| drum_lanes & lane_bit(*ch) != 0) {
        e.set_pad_params(lane, &app.drum_kit_params_of(lane));
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
    Ok(e)
}

/// **The stems an export writes, and what each file is called**: only the
/// lanes with something to SAY - clips, or an instrument in a bay (the
/// DX7, the drumpad, an SD3 following the transport). A kind with neither
/// is an armed input that renders silence offline, and a folder of silent
/// WAVs reads as a bug. `NN-<track name>.wav`, the name made safe for a
/// file system.
pub fn stem_files(app: &TangentApp) -> Vec<(usize, String)> {
    let clips = &app.arrangement().clips;
    (0..CHANNELS)
        .filter(|&lane| {
            clips.iter().any(|c| c.lane == lane) || (0..INSERTS).any(|b| app.insert(lane, b).is_some())
        })
        .map(|lane| {
            let name: String = app
                .channel_display_name(lane)
                .chars()
                .map(|c| if std::path::is_separator(c) { '-' } else { c })
                .collect();
            (lane, format!("{:02}-{}.wav", lane + 1, name.trim()))
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

/// **What the engine was last told about the desk's tables**, keyed on the
/// engine it was told (`epoch`) and the device width the routes were pruned
/// against (`outs`).
///
/// The live push used to publish all four every frame the band was open:
/// `set_routes` cleared and rewrote the route table, `set_insert_keys`
/// cleared all 13,167 key cells and rewrote them, `set_insert_bypass` loaded
/// 13,167 cells with a `contains` per cell, and `set_desk` rebuilt the
/// mute/solo masks — twenty-six thousand atomic operations a frame on cache
/// lines the audio thread is about to read, to publish tables that change
/// when somebody clicks. Each publish also opens the brief window in which
/// the audio thread can read an EMPTY route or key table (the count is
/// cleared first); publishing on change means that window opens when the
/// desk changes, as it always did, and not thirty times a second.
///
/// Compared field by field against the live settings BEFORE anything is
/// cloned (`desk_unchanged`), so the steady state costs a few equality
/// walks over small lists and no allocation; the clone happens once, when
/// something did change.
#[derive(Clone, Debug, PartialEq)]
struct PushedDesk {
    epoch: u64,
    outs: usize,
    routes: Routes,
    keys: KeyEdges,
    bypass: Vec<(usize, usize)>,
    desk: Desk,
}

/// Whether the tables the engine holds (`pushed`) are the ones the desk
/// shows now. `None` — nothing pushed yet, or a fresh engine — is never
/// "unchanged": a new engine starts empty and must be primed.
fn desk_unchanged(
    pushed: Option<&PushedDesk>,
    epoch: u64,
    outs: usize,
    routes: &Routes,
    keys: &KeyEdges,
    bypass: &[(usize, usize)],
    desk: &Desk,
) -> bool {
    pushed.is_some_and(|p| {
        p.epoch == epoch
            && p.outs == outs
            && p.routes == *routes
            && p.keys == *keys
            && p.bypass.as_slice() == bypass
            && p.desk == *desk
    })
}

/// **The host's half of the seam**: what the live engine was told, and the
/// one place it is told the desk's structure.
#[derive(Default)]
pub struct Bridge {
    /// Which engine `pushed` describes. Bumped by [`Self::engine_replaced`],
    /// so a fresh engine is never mistaken for the one that was primed.
    epoch: u64,
    pushed: Option<PushedDesk>,
    /// Where an arrangement record is replacing each lane's schedule, in
    /// frames, handed in by the host each frame (it owns the record).
    punches: [Option<u64>; CHANNELS],
    /// The desk the last [`Self::sync`] built, for the host's take facts:
    /// built ONCE a frame.
    desk: Desk,
}

/// What a [`Bridge::sync`] did that the host has to follow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Synced {
    /// The engine was primed from nothing - a fresh engine forgets the
    /// pads' knobs with everything else, so whatever the host caches as
    /// "what this engine was told" has to forget too.
    pub primed: bool,
}

impl Bridge {
    pub fn new() -> Self {
        Self::default()
    }

    /// A new engine is seated: everything it is told from here on is told
    /// from scratch. Called wherever the host replaces its engine.
    pub fn engine_replaced(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    /// **The live punch**: while the arrangement records over a lane,
    /// everything scheduled on it from the record start is being replaced
    /// and must not sound. Handed in every frame like the arms; cleared the
    /// same way when the take ends and the carve is real.
    pub fn punch_at(&mut self, punches: [Option<u64>; CHANNELS]) {
        self.punches = punches;
    }

    /// The desk the last sync built and compared.
    pub fn desk(&self) -> Desk {
        self.desk
    }

    /// **Push the desk's structure into `e`**, once a frame.
    ///
    /// The tables on CHANGE (see [`PushedDesk`]), compared against what this
    /// engine was last given, keyed on the engine and on the device width
    /// the routes are pruned against: an interface swapped for a smaller
    /// one changes `outs`, and the routes pointing at jacks that are gone
    /// are pruned again then. Every other word whole, every frame: each is
    /// one atomic store, so writing an unchanged value costs nothing, and a
    /// change-detection cache over them would be a second copy of the
    /// settings to get out of step with the first.
    pub fn sync(&mut self, app: &TangentApp, e: &mut Engine) -> Synced {
        let mut synced = Synced::default();
        self.desk = app.desk();
        let outs = e.output().channels as usize;
        if !desk_unchanged(
            self.pushed.as_ref(),
            self.epoch,
            outs,
            app.routes(),
            app.insert_keys(),
            app.insert_bypass(),
            &self.desk,
        ) {
            publish_tables(app, e);
            e.set_desk(&self.desk);
            synced.primed = self.pushed.as_ref().is_none_or(|p| p.epoch != self.epoch);
            self.pushed = Some(PushedDesk {
                epoch: self.epoch,
                outs,
                routes: app.routes().clone(),
                keys: app.insert_keys().clone(),
                bypass: app.insert_bypass().to_vec(),
                desk: self.desk,
            });
        }
        // **What is frozen, and how far ahead the effects render**, every
        // frame and NOT behind the gate. Neither is in `PushedDesk`'s key, and
        // they used to ride inside the table publish, so a freeze (or a thaw,
        // or a render-ahead change) with nothing else edited never reached a
        // running engine: a frozen lane's rack went on processing its
        // already post-rack file, a second time, until some route, key,
        // bypass, mute, solo or send moved. Each is one atomic store.
        publish_freeze_and_grain(app, e);
        // **Armed, or alone in the editor** — the whole rule, in one place,
        // tested there. A frozen lane listens to nothing: its rack rests and
        // its file plays, so a note into it would sound through nothing
        // anyway.
        let armed = app.armed_mask();
        e.set_midi_off(live_midi_off(app, armed));
        // **And WHAT each lane is listening to**, pushed whole beside the
        // arm mask every frame. Two separate questions on purpose: `midi_off`
        // says whether a lane is listening at all (arm, the editor), this
        // says to which devices and which channel. Folding them would make a
        // keyboard being unplugged look like a disarm.
        e.set_midi_routes(&app.midi_routes());
        e.set_armed(armed);
        e.set_punches(self.punches);
        // The built-ins on a frozen lane stand down with its rack: the file
        // carries what they played. Their instances outlive the freeze - the
        // host keeps them - so thawing is instant.
        let (builtin_mask, keys_mask) = live_instrument_lanes(app);
        let thawed = !frozen_lane_mask(app.frozen_lanes());
        e.set_builtin_strips(builtin_mask & thawed);
        e.set_keys_strips(keys_mask & thawed);
        // The drumpad's lanes, by the bay-1 sentinel rule — any kind of
        // channel counts, because playback does not ask the arm. **Every**
        // lane holding it: `.find()` alone was the bug that left the second
        // drum lane silent while the shared pads lit up for the first.
        e.set_drum_strips(lanes_holding(app, tangent_ui::dialogs::DRUMPAD_PATH));
        // **The folder tree**, so a group made, broken or reordered takes
        // effect on the next block. Refused whole if it is not a forest —
        // see `Engine::set_folders` — because half a tree is a routing graph
        // nobody drew.
        e.set_folders(app.folder_parents());
        // Polarity, one bit per strip - see `Shared::strip_phase`. **Take or
        // no take**: the switch is a desk control like mute, and a take is
        // capture only. It once sat in the meter's take gate, so a flip
        // during a take lit and changed nothing until the take ended.
        e.set_strip_phase(app.strip_phase());
        // **Which lanes the curves must leave alone.** One atomic word, so
        // it costs a store and must NOT be change-detected: it has to take
        // effect on the next block or a fader being written fights the
        // curve under the hand.
        e.set_auto_suspend(app.auto_suspend_mask());
        synced
    }
}

/// **The live engine's two instrument masks**: every MIDI lane whose first
/// bay holds the built-in DX7, and every one holding the modelled keyboard,
/// gathered in one walk so the two cannot disagree about the desk.
///
/// **MIDI lanes only, where the offline walk takes any lane** - the live
/// rule and the export's have always differed in this, and the move keeps
/// both exactly: W4's broker is where they become one.
pub fn live_instrument_lanes(app: &TangentApp) -> (u32, u32) {
    let mut builtin = 0u32;
    let mut keys = 0u32;
    for ch in (0..CHANNELS).filter(|ch| app.channel_kind(*ch) == Some(ChannelKind::Midi)) {
        match app.insert(ch, 0) {
            Some(tangent_ui::dialogs::BUILTIN_PATH) => builtin |= lane_bit(ch),
            Some(tangent_ui::dialogs::KEYS_PATH) => keys |= lane_bit(ch),
            _ => {}
        }
    }
    (builtin, keys)
}

/// Every lane whose first bay holds `sentinel`, as the engine's mask.
fn lanes_holding(app: &TangentApp, sentinel: &str) -> u32 {
    (0..CHANNELS)
        .filter(|ch| app.insert(*ch, 0) == Some(sentinel))
        .fold(0u32, |m, ch| m | lane_bit(ch))
}

/// One lane's bit in a 32-wide engine word, and NO bit for a lane past it.
///
/// The desk is 32 lanes, so every lane reaching here has its own bit and
/// this is exactly `1 << ch`. It used to clamp the lane to 31 before the
/// shift, which folds a 33rd lane onto the 32nd's bit: a clamp is an alias,
/// and this file is where a wider desk will first arrive.
fn lane_bit(ch: usize) -> u32 {
    u32::try_from(ch).ok().and_then(|c| 1u32.checked_shl(c)).unwrap_or(0)
}

/// Which lanes are shut off from LIVE MIDI - `recorder::live_midi_off`'s
/// three rules - with every frozen lane shut too.
fn live_midi_off(app: &TangentApp, armed: u32) -> u64 {
    let kinds: Vec<_> = (0..CHANNELS).map(|ch| app.channel_kind(ch)).collect();
    let midi_off = tangent_ui::recorder::live_midi_off(&kinds, armed, app.midi_editor_lane());
    u64::from(midi_off) | u64::from(frozen_lane_mask(app.frozen_lanes()))
}

/// **The routing, key and bypass tables**, onto `e`: what the live push
/// publishes on change and an offline engine is built with.
///
/// The routes are pruned against the device that is open: the picker knows
/// only what the device offered when it opened, and the pruned copy is
/// never written back to the desk — so a route survives an interface being
/// unplugged and replugged, and is dropped only against the device that is
/// actually open. What the engine does with the tables — clear the count,
/// write the cells, publish the count — is `Engine::set_routes`' own
/// argument, and it is worth reading before touching either end.
fn publish_tables(app: &TangentApp, e: &Engine) {
    let outs = e.output().channels as usize;
    let mut routes = app.routes().clone();
    routes.prune(outs, CHANNELS);
    e.set_routes(&routes);
    // **The sidechain key table rides beside the routes**, whole on
    // every publish and on the same bargain: absent means cleared, so
    // the engine never keeps an edge the desk no longer has. The desk
    // speaks (strip, slot) so saved edges survive seat renumbering; the
    // engine speaks (seat, bay), and THIS is the one place the
    // translation happens — through `seat_bay`, like every other slot
    // address.
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
    engine_set_insert_keys(e, &keys);
    // **And the bypassed bays**, whole on every publish and on the same
    // bargain: absent means in circuit. Already in SEAT terms - a bypass
    // is stored at the plugin's own address - so unlike the keys above
    // there is no `seat_bay` translation here to get wrong.
    engine_set_insert_bypass(e, app.insert_bypass());
}

/// **The frozen mask and the render-ahead**, onto `e`: two single words the
/// live push stores every frame (they are not in the change gate's key, see
/// `Bridge::sync`) and an offline engine is built with.
fn publish_freeze_and_grain(app: &TangentApp, e: &Engine) {
    e.set_frozen(frozen_lane_mask(app.frozen_lanes()));
    // **How far ahead a track's effects are rendered.** One store of a
    // frame count; see `Settings::render_ahead_ms` for what it trades.
    e.set_grain_ms(app.render_ahead_ms());
}

/// The whole key table onto the engine — `(seat, bay, source)`, absent
/// means cleared, the `set_routes` idiom. One thin seam kept so the UI lane
/// and the engine lane could land apart.
fn engine_set_insert_keys(engine: &Engine, keys: &[(usize, usize, usize)]) {
    engine.set_insert_keys(keys);
}

/// The whole bypass table onto the engine - `(seat, bay)`, absent means in
/// circuit, the `set_routes` idiom.
fn engine_set_insert_bypass(engine: &Engine, bays: &[(usize, usize)]) {
    engine.set_insert_bypass(bays);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    use tangent_ui::arrangement::{ArrClip, ArrNote, Arrangement, ClipKind, FrozenLane};
    use tangent_ui::automation::{AutoParam, Automation, Curve, Point, Shape};
    use tangent_ui::host::Caps;
    use tangent_ui::recorder::STRIPS;
    use tangent_ui::routing::{Route, Target};
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

    /// One scripted edit: a name, the desk after it, the punches the
    /// arrangement record would hand the frame, and the lanes' automation
    /// modes (app state, not settings: see `set_arr_auto_mode_for_test`).
    struct Step {
        name: &'static str,
        settings: Settings,
        punches: [Option<u64>; CHANNELS],
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
        let none = [None; CHANNELS];
        let mut modes: Vec<(usize, tangent_ui::automation::AutoMode)> = Vec::new();
        let mut out = vec![Step { name: "desk", settings: s.clone(), punches: none, modes: modes.clone() }];
        let mut edit = |name: &'static str, f: &dyn Fn(&mut Settings, &mut Vec<_>), punches| {
            f(&mut s, &mut modes);
            out.push(Step { name, settings: s.clone(), punches, modes: modes.clone() });
        };
        edit("arm", &|s, _| s.channel_armed = Some(1 << 0 | 1 << 4), none);
        edit("mute", &|s, _| s.strip_muted |= 1 << 2, none);
        edit("solo", &|s, _| s.strip_soloed |= 1 << 4, none);
        edit("phase", &|s, _| s.strip_phase |= 1 << 1 | 1 << 34, none);
        edit(
            "folder",
            &|s, _| {
                s.strip_is_folder[5] = true;
                s.strip_in_folder[6] = true;
                s.strip_in_folder[7] = true;
                s.strip_muted |= 1 << 5;
            },
            none,
        );
        edit(
            "route",
            &|s, _| {
                s.routes.add(Route::new(3, Target::Channel(4)));
                s.routes.add(Route::new(6, Target::Hardware { first: 0, stereo: true }));
                // Past the offline engine's two outputs: pruned at the door.
                s.routes.add(Route::new(7, Target::Hardware { first: 4, stereo: true }));
            },
            none,
        );
        edit("route-gain", &|s, _| s.routes.set_gain(0, 0.5), none);
        edit("key", &|s, _| s.insert_keys.set(4, 0, Some(3)), none);
        edit("bypass", &|s, _| s.insert_bypass.0.push((4, 1)), none);
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
            none,
        );
        edit(
            "auto-modes",
            &|_, m| {
                use tangent_ui::automation::AutoMode;
                m.push((3, AutoMode::Off));
                m.push((4, AutoMode::Write));
                // The master's lane is bit 32: past a u32, which is the
                // aliasing `auto_suspend_mask` was widened to stop.
                m.push((tangent_ui::arrangement_panel::MASTER_LANE, AutoMode::Touch));
            },
            none,
        );
        edit(
            "freeze",
            &|s, _| s.frozen_lanes.push(FrozenLane { lane: 0, path: "media/f.wav".into(), len_s: 1.0 }),
            none,
        );
        let mut punched = none;
        punched[3] = Some(48_000);
        edit("punch", &|_, _| {}, punched);
        edit("unpunch", &|_, _| {}, none);
        edit("render-ahead", &|s, _| s.render_ahead_ms = 5.0, none);
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
        let mut p = Bridge::new();
        let mut seen: Vec<(String, serde_json::Value)> = Vec::new();
        let mut last = None;
        for step in steps() {
            let app = step_app(&step);
            p.punch_at(step.punches);
            p.sync(&app, &mut e);
            seen.push((step.name.to_owned(), e.desk_readback()));
            last = Some((app, step.punches));
        }
        let (app, _) = last.expect("steps");
        // An unchanged frame: a route planted behind the gate stays.
        let mut planted = Routes::default();
        planted.add(Route::new(9, Target::Channel(10)));
        e.set_routes(&planted);
        p.sync(&app, &mut e);
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
    /// inside the table publish, behind a change gate whose key holds
    /// neither, so the engine's frozen bit waited for some unrelated route,
    /// key, bypass, mute, solo or send edit: until then a frozen lane's rack
    /// processed its already post-rack file a second time. A route planted
    /// behind the gate and still there at the end proves the gate never
    /// opened, so it is the per-frame store that carried each one.
    #[test]
    fn a_freeze_reaches_a_running_engine_with_nothing_else_edited() {
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
        let mut p = Bridge::new();
        let mut s = blank();
        kind(&mut s, &[0, 1], ChannelKind::Audio);
        bay(&mut s, 1, 0, COMP_PATH);
        p.sync(&app_with(s.clone()), &mut e);
        let mut planted = Routes::default();
        planted.add(Route::new(9, Target::Channel(10)));
        e.set_routes(&planted);
        let behind_gate = e.desk_readback()["routes"].clone();
        let read = |e: &Engine, k: &str| e.desk_readback()[k].clone();
        assert_eq!(read(&e, "frozen"), serde_json::json!(0));

        s.frozen_lanes.push(FrozenLane { lane: 1, path: "media/f.wav".into(), len_s: 1.0 });
        p.sync(&app_with(s.clone()), &mut e);
        assert_eq!(read(&e, "frozen"), serde_json::json!(1 << 1), "a freeze waited for some other edit");

        s.render_ahead_ms = 10.0;
        p.sync(&app_with(s.clone()), &mut e);
        assert_eq!(read(&e, "grain"), serde_json::json!(RATE / 100), "a render-ahead change waited for some other edit");

        s.frozen_lanes.clear();
        p.sync(&app_with(s.clone()), &mut e);
        assert_eq!(read(&e, "frozen"), serde_json::json!(0), "a thaw waited for some other edit");
        assert_eq!(read(&e, "routes"), behind_gate, "the change gate opened, so this proves nothing about the per-frame store");
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
            for (lane, file) in stem_files(app) {
                let name = format!("{}/{file}", spec.stems_folder());
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

    /// **Every effect bay a corpus desk seats has its state in the live
    /// engine.** A bay the live engine is not wearing is furnished from the
    /// shelf (`desk_insert_state` -> `saved_insert_state`), and the shelf is
    /// `Settings::path().parent()`: the REAL `~/.config/tangent` in a binary
    /// crate's test, unless the run is sandboxed. A golden recorded in the
    /// sandbox would then depend on whatever the person running the suite
    /// last dialled into that bay, and a sidecar that will not load would
    /// trip `every_corpus_export_has_sound_in_it`'s refusal check. Instrument
    /// sentinels are not furnished as bays, so they need no state.
    fn every_corpus_bay_is_live(c: &Corpus, app: &TangentApp) {
        for i in 0..(crate::instrument::RACK_SEATS * INSERTS) {
            let (seat, bay) = (i / INSERTS, i % INSERTS);
            let Some(path) = app.insert(seat, bay).filter(|p| !is_instrument_sentinel(p)) else {
                continue;
            };
            assert!(
                c.live.iter().any(|(s, b, unit, _)| (*s, *b, *unit) == (seat, bay, path)),
                "{}: bay ({seat}, {bay}) seats {path} with no live state, so its knobs would come from the real shelf",
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
                let build = match freeze {
                    Some(lane) => BuildFor::Freeze(TrackId(lane as u32)),
                    None => BuildFor::Export(spec.clone()),
                };
                let mut ctx = Furnish {
                    live: Some(&live),
                    dx7: (0..CHANNELS)
                        .filter(|ch| app.insert(*ch, 0) == Some(tangent_ui::dialogs::BUILTIN_PATH))
                        .map(|ch| (ch, factory_voice(&app, ch)))
                        .collect(),
                    ..Furnish::default()
                };
                let e = furnish_offline_with(&app, RATE, Timebase::new(), build, &mut ctx)
                    .expect("an offline engine");
                assert!(ctx.refused.is_empty(), "{}: a bay refused: {:?}", c.name, ctx.refused);
                assert!(ctx.unread.is_empty(), "{}: the corpus names a file it did not write: {:?}", c.name, ctx.unread);
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

    /// **The desk's tables go to the engine on change, not every frame.**
    ///
    /// The pure half: nothing pushed (or a fresh engine, or a narrower
    /// device) is never "unchanged", an identical desk is, and a moved route
    /// gain, a new key edge, a bypassed bay or a mute each count as change.
    #[test]
    fn the_desk_tables_are_pushed_on_change_only() {
        use tangent_ui::routing::{KeyEdges, Route, Routes, Target};
        let mut routes = Routes::default();
        routes.add(Route::new(0, Target::Hardware { first: 0, stereo: true }));
        let keys = KeyEdges::default();
        let bypass: Vec<(usize, usize)> = Vec::new();
        let desk = Desk::default();
        assert!(!desk_unchanged(None, 1, 2, &routes, &keys, &bypass, &desk), "nothing pushed read as unchanged");
        let pushed = PushedDesk {
            epoch: 1,
            outs: 2,
            routes: routes.clone(),
            keys: keys.clone(),
            bypass: bypass.clone(),
            desk,
        };
        assert!(desk_unchanged(Some(&pushed), 1, 2, &routes, &keys, &bypass, &desk));
        assert!(!desk_unchanged(Some(&pushed), 2, 2, &routes, &keys, &bypass, &desk), "a new engine read as unchanged");
        assert!(!desk_unchanged(Some(&pushed), 1, 4, &routes, &keys, &bypass, &desk), "a wider device read as unchanged");
        let mut moved = routes.clone();
        moved.set_gain(0, 0.5);
        assert!(!desk_unchanged(Some(&pushed), 1, 2, &moved, &keys, &bypass, &desk), "a route gain read as unchanged");
        let mut keyed = keys.clone();
        keyed.set(0, 0, Some(1));
        assert!(!desk_unchanged(Some(&pushed), 1, 2, &routes, &keyed, &bypass, &desk), "a key edge read as unchanged");
        assert!(!desk_unchanged(Some(&pushed), 1, 2, &routes, &keys, &[(3, 0)], &desk), "a bypass read as unchanged");
        let mut muted = desk;
        muted.muted[0] = true;
        assert!(!desk_unchanged(Some(&pushed), 1, 2, &routes, &keys, &bypass, &muted), "a mute read as unchanged");
    }

    /// And the gate's wiring, by scan: inside `Bridge::sync` the table
    /// publish and `set_desk` sit under the `desk_unchanged` gate, the
    /// frozen mask and the render-ahead sit after it (they are not in its
    /// key), the gate is keyed on the bridge's count of engines so a fresh
    /// engine is primed, and the desk is built once.
    #[test]
    fn the_sync_publishes_the_tables_under_the_change_gate() {
        let src = include_str!("engine_bridge.rs");
        let src = &src[..src.find("#[cfg(test)]\nmod tests {").expect("the tests module")];
        let at = src.find("pub fn sync(&mut self").expect("Bridge::sync");
        let body = &src[at..at + src[at..].find("\n    }\n").expect("its end")];
        let gate = body.find("if !desk_unchanged(").expect("the change gate is gone");
        let tables = body.find("publish_tables(app, e);").expect("the table publish is gone");
        let desk = body.find("e.set_desk(&self.desk);").expect("set_desk is gone");
        let shut = body[gate..].find("\n        }\n").map(|n| gate + n).expect("the gate's end");
        assert!(gate < tables && tables < shut && gate < desk && desk < shut, "a table publish sits outside the change gate");
        let words = body.find("publish_freeze_and_grain(app, e);").expect("the frozen mask and the render-ahead are not pushed");
        assert!(words > shut, "the frozen mask and the render-ahead went back behind the change gate");
        assert!(body[gate..tables].contains("self.epoch"), "the gate is not keyed on the engine");
        assert_eq!(body.matches("app.desk()").count(), 1, "the desk is built more than once a frame");
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
            let e = furnish_offline(&app, RATE, Timebase::new(), build).expect("an offline engine");
            let r = e.desk_readback();
            (r["armed"].as_u64().expect("armed"), r["midi_off"].as_u64().expect("midi_off"))
        };
        let (armed, off) = read(BuildFor::Probe);
        assert_eq!(armed, 1 << 1 | 1 << 2, "the probe was not told the session's arm");
        assert_eq!(off & 0b11, 0b01, "the probe's armed MIDI lane is not the one listening");
        for build in [BuildFor::Export(ExportSpec::default()), BuildFor::Freeze(TrackId(1))] {
            assert_eq!(read(build.clone()), (0, u64::MAX), "{build:?} hears something live");
        }
    }

    /// **A lane past the engine's word has no bit**, where the old clamp to
    /// 31 gave it the 32nd lane's: the alias a wider desk would hit first.
    #[test]
    fn a_lane_bit_is_its_own_or_none() {
        for ch in 0..32 {
            assert_eq!(lane_bit(ch), 1u32 << ch);
        }
        for ch in [32, 33, 63, 64, usize::MAX] {
            assert_eq!(lane_bit(ch), 0, "lane {ch} aliases another lane's bit");
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
        assert!(render.contains("crate::engine_bridge::furnish_offline_with("), "the export builds its own engine");
        assert!(render.contains("crate::engine_bridge::BuildFor::Freeze("), "a freeze is not a freeze arm");
        assert!(render.contains("crate::engine_bridge::BuildFor::Export("), "an export is not an export arm");
        let probe = body_of(tests, "fn furnish_probe_engine(");
        assert!(probe.contains("crate::engine_bridge::furnish_offline_with("), "the probe builds its own engine");
        assert!(probe.contains("crate::engine_bridge::BuildFor::Probe"), "the probe is not the probe arm");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .map(|l| format!("{l}\n"))
            .collect();
        let moved = [
            "set_desk(", "set_armed(", "set_frozen(", "set_midi_off(", "set_drum_strips(",
            "set_builtin_strips(", "set_keys_strips(", "set_auto_suspend(", "set_strip_phase(",
            "set_folders(", "set_punches(", "set_midi_routes(", "set_routes(", "set_insert_keys(",
            "set_insert_bypass(", "set_grain_ms(",
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

    /// **What the four table publishes cost per frame**, release, on an
    /// offline engine — the per-frame bill `PushedDesk` retires — against
    /// the compare that replaces it. `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn what_the_desk_publish_costs_per_frame_probe() {
        use std::time::Instant;
        use tangent_ui::routing::{KeyEdges, Route, Routes, Target};
        let e = Engine::offline(48_000, Timebase::new())
            .expect("offline engine");
        let mut routes = Routes::default();
        for ch in 0..8 {
            routes.add(Route::new(ch, Target::Hardware { first: 0, stereo: true }));
        }
        let mut keys = KeyEdges::default();
        keys.set(2, 1, Some(0));
        let bays = vec![(6usize, 0usize), (9, 1)];
        let key_rows: Vec<(usize, usize, usize)> = keys
            .0
            .iter()
            .map(|k| {
                let (seat, bay) = tangent_ui::recorder::seat_bay(k.strip, k.slot);
                (seat, bay, k.from)
            })
            .collect();
        let desk = Desk::default();
        let n = 2_000u32;
        let t = Instant::now();
        for _ in 0..n {
            let mut pruned = routes.clone();
            pruned.prune(2, CHANNELS);
            e.set_routes(&pruned);
            e.set_insert_keys(&key_rows);
            e.set_insert_bypass(&bays);
            e.set_desk(&desk);
        }
        let publish = t.elapsed().as_secs_f64() * 1e6 / f64::from(n);
        let pushed = PushedDesk {
            epoch: 1,
            outs: 2,
            routes: routes.clone(),
            keys: keys.clone(),
            bypass: bays.clone(),
            desk,
        };
        let t = Instant::now();
        let mut same = 0u32;
        for _ in 0..n {
            same += u32::from(desk_unchanged(Some(&pushed), 1, 2, &routes, &keys, &bays, &desk));
        }
        let compare = t.elapsed().as_secs_f64() * 1e6 / f64::from(n);
        eprintln!(
            "desk publish per frame: routes+keys+bypass+desk {publish:.1} us; the change compare {compare:.3} us ({same} unchanged)"
        );
    }
}
