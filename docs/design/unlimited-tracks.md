# Unlimited tracks: the build-ready design

Revision 2, 2026-10-09. Repo `/Users/ganten/Library/Caches/tangent-unlimited`, branch `feat/unlimited-tracks` = main `d821bb3` (re-verified with `git log -1`). Code is located by SYMBOL (`file:symbol`), never by line.

This file is the whole contract between lanes. It supersedes `notes/chief-architect/contracts.txt`: section 12 holds the binding signatures and the appendix holds the frozen tables. Section 15 maps every critique blocker to its resolution. All 19 blockers were checked against the code and accepted; five fixes are refined rather than taken verbatim, and section 15 says why.

---

## 0. Shape of the design

**Backbone: identity first, engine on slots.**

- Every track row has a `TrackId`, every plugin bay a `SeatBay`, every route, send and key endpoint a `Node`. A position survives only inside three units that are rewritten together with the row order: a clip's lane, a curve's lane, a folder's contiguous run.
- Session state that names a track or a bay is keyed by `TrackId` or `SeatBay` too: the selection, automation-row state, the drum panel, open faces, pending loads. A field-classification scan keeps it that way.
- Settings and documents share one format-2 desk serializer. Every format-2 side effect reads the one switch `desk_format::WRITE_V8`: the stamps, the backups, the drumpad.json keys, the pstate names and the `legacy_states` marker.
- A new build never renames or deletes a legacy plugin-state file.
  - The global shelf is copied once, through a composed map of the frozen renames.
  - Project and template `state/` folders are read in place, through the document's durable `legacy_states` marker and the provenance of the lift that moved each plugin.
- The engine is a slot spine of `Box<Lane>`, driven by one validated, immutable `DeskPlan` per block, and it keeps today's per-bay `Rack` rings.
  - A lane owns exactly what its own hand touches.
  - Desk-wide buffers stay in the spine and are written serially.
  - Bus, folder and return racks run level by level in parallel after the lane fork.
- Internal builds run against a staging config and staging media root from wave 1. Nothing on the branch touches the owner's release install until wave 8.

**Kept from earlier rounds:**

- Wave-1 real-time guard rails (a counting allocator and an alias harness, inside the tangent crate).
- The exhaustive destructure guard on structural edits.
- `TrackRefusal` and the full road list.
- One bridge file between app and engine.
- `Bridge::full_build` as the single builder for export, probe and freeze.
- The committed real `tests/fixtures/supporter.key`.
- Render goldens.
- Lazy pads.
- The cfg(test) cap sentinel in `undo_commit`.
- The per-crate test census.
- `TangentApp::with_license`.
- The phase-mask lift fix.
- Picks as many as the bound tracks.
- Sound changes ride only on the new Bus kind, so there is no `mix_rules` version.

---

## 1. Owner decisions as rules

- **Free tier:** 32 counted tracks.
- **Supporter tier: no track limit; memory is the bound.** Supporter means `tangent-core/src/license.rs:LicenseStore::is_supporter()`: a real key covering this major, or the Konami `cheat.unlock` marker. `MAX_ROWS = 65,536` is a hostile-file sanity bound, not a tier.
- **Never counted, unlimited on both tiers:** buses, returns, sends, routes, sidechain keys, hardware taps.
- **The fixed-desk principle is reversed** (section 2).
- **Old files:**
  - New builds open every old settings file, document, template, plugin-state shelf and DAW blob without loss.
  - Old builds opening new files is not supported.
  - No legacy keys are written.
- **Every stage is internal.** Nothing ships until wave 8 is green. Internal builds keep their own config and media root (section 6.9).
- **No interim wider `const CHANNELS` is ever built.** `Shared` is about 117 KB against its 128 KB stack_budget ceiling and fails near 35 lanes, and Windows stacks are 1 to 2 MiB.

---

## 2. The reversed principle

This text replaces the comment above `tangent-ui/src/recorder.rs:CHANNELS`. The constant itself is deleted in W6.

> The desk is the list of tracks that exist, plus one place to add the next. It grows by appending and the machine is its only limit: no part of its shape is a compile-time number. A licence decides how many tracks an install may CREATE: thirty-two free, no limit with a supporter key or a certain code. It never hides, silences or truncates a track that exists: a project with more tracks than this install may create opens whole, plays whole and saves whole, and only the next new track is refused. Buses, returns, sends and routes are unlimited on every tier and are not tracks. Every track, bus and plugin seat has an identity that does not depend on where it sits, so adding, removing or reordering tracks renumbers nothing on disk and moves no plugin.

These are rewritten too:

- the no-comping note's "Like the fixed desk below";
- `mixer_panel.rs:MixerView::strips`;
- `routing.rs:MAX_ROUTES`;
- the `arrangement.rs` module doc;
- `tangent-record/src/audio.rs:MAX_PICKS` and `Picks::from_slice`;
- `TangentApp::active_channels`, which still says twenty-four;
- the `LicenseStore` cheat doc ("the wide desk, the folded mixer").

The `INSERTS` doc stays until owner call 1.

---

## 3. Identity (C1, C2)

- **`TrackId(u32)`**: per document, monotonic (`next_id`), never reused.
- **Identity rule: legacy row r is TrackId(r) in every era.** Every era lift inserted its new channels after the old ones, and the v13/v14 strip changes sat above the channels, so no legacy lane index ever moved. After migration `next_id = 32`, so in a migrated document any id below 32 is a legacy row.
- **Minting.** New rows mint an id on:
  - add, insert, paste and import;
  - the header row `make_folder` creates;
  - a new bus.

  A cleared slot that is reused gets a fresh id. Undo and redo restore rows with their ids. Templates pass their ids on.
- **Types:**
  - `Node::{Track(TrackId), Drums, Click, Fx, Master}`
  - `Rack::{Chain(u16), Pedal(u8), Pad(u8)}`
  - `SeatKey { node, rack }`
  - `SeatBay { seat, bay }`
- **One strict text grammar** (Appendix C), used for JSON keys, sidecar names (`pstate-<seatbay>.bin`, body unchanged), env vars and logs.
- **Two resolvers keep the master's two bay orders apart for good:**
  - `slot_key` maps a DISPLAY slot. The master's extra-first quirk lives only here.
  - `auto_key` maps a persisted or automation bay, uniformly, and equals `instrument.rs:param_slot`.
- **Engine slots are not TrackIds.** The bridge owns `TrackId -> Slot`. A desk loaded from disk installs row r at `Slot(r)` (vacant rows get no lane), so slots equal today's lane indices for every legacy desk. Later adds take the lowest free slot.

This one rule removes, by construction:

- every widening shuffle;
- the routes defect: `reorder_channels` remaps `insert_keys` but not `routes`, and `remove_channel_inner` drops keys but not routes;
- pad FX left behind on reorder: `recorder.rs:seat_order` never moves `pad_rack_index` seats;
- the multi-second reorder: per-bay `hand_off_exchange` round trips under a 250 ms `RETIRE_TIMEOUT`, plus `permute_insert_states_in`;
- `compose_orders`' length heuristic.

---

## 4. The desk model (C4)

`DeskModel { next_id, rows: Vec<TrackRow>, specials, plugins: BTreeMap<SeatBay, String>, bypass: BTreeSet<SeatBay>, routes, keys, legacy_states: Option<DeskEra> }`. A row's position is its lane.

- Per-row booleans replace every mask: the u32 `armed` and `tracks_added` are full at 32, u64 strip masks stop near 60, and `1 << n` overflows past them.
- Scalars that never lived in strip space keep their keys.
- Kits become `BTreeMap<TrackId, DrumpadDoc>`.

**Structural edits have one home:** `append`, `insert_rows`, `remove_rows`, `permute`.

- Each is guarded by an exhaustive `let TrackRow { .. } = row;` test.
- Each rewrites the positional units itself.
- Each calls `TangentApp::on_desk_shape_change()`, which clears the transient aims: `input_menu`, `mixer_naming`, `mixer_typing`, `mixer_fx_menu`, `mixer_grab`, `mixer_reorder`, `arr_drag`, `arr_pending`, `arr_drop`, `arr_move_orig`, `pedal_stack`.
- Under cfg(test), each runs `DeskModel::check()`, which lists every dangling reference.

**Session state is keyed by identity.** Every `TangentApp` field whose type carries a lane or seat index must fall into one of three classes, enforced by a source scan that fails on a new unclassified field:

- **(a) Keyed by `TrackId` or `SeatBay`.** For example `arr_auto_open`/`armed`/`bypassed`/`focus`/`modes`/`latched`/`last`/`sel`/`nodes`/`hot`, `auto_clipboard_from`/`rows`, `selected_channel`/`tracks`, `sel_anchor`, `drumpad_lane`, `Moment.rows_closed`. Seat-keyed fields from W5: `fx_params`, `builtin_faces(_live)`, `pending_inserts`, `insert_gr`, `sampler_info`, `sampler_load`, `preset_sample`, `fx_clipboard`, `insert_state_copy`, `open_insert_on_load`, `picker_insert`, `bay_carry`, `arr_fx_menu`, `patch_audition`.
- **(b) Cleared by `on_desk_shape_change`.**
- **(c) Not a lane, with a one-line reason.**

Entries keyed by a removed id are inert and come back with its undo. They are purged on a session switch.

**Row classes** (cached in `DeskIndex`):

| Class | What it is | Drawn | Counted | Engine lane |
|---|---|---|---|---|
| Vacant | no kind, not added, not folder or in_folder, no clips, no curve, no kit, no endpoint, not frozen, no CHAIN plugin (pedal-only residue counts as Vacant) | no | no | no |
| Placeholder | only `in_folder` set (today's indented plus) | yes | no | yes |
| Bus | kind is Bus | yes | no | yes |
| Counted | MIDI, AUDIO, bare added, folder, any row with clips | yes | yes | yes |

- Migration never deletes a row. A legacy 32-wide desk keeps its vacant rows, hidden.
- Pedal-only rows are residue of today's `remove_channel`, which deliberately leaves pedals loaded. They stay lossless in the model and are never reused, because reusing one would orphan or adopt a stranger's pedalboard.

**From W6:**

- Add (Ctrl/Cmd+T, header plus, HdrAdd, mixer +, trailing +) inserts after the selection, or appends with none. A fully empty Vacant row exactly at the target is reused with a fresh id.
- Remove Track deletes the row. Its plugin states go to the global shelf first (section 7.13), and its pedalboard leaves with it.
- During an arrangement record or a realtime export, Add appends at the end and every other shape change waits (section 7.13).

---

## 5. Paywall, Konami, downgrade (C5)

### 5.1 The gate

`TangentApp::may_add_tracks(k) -> Result<(), TrackRefusal>`, with `TrackRefusal::{FreeCap { have, cap }, Ceiling { max }, Held}`.

- It reads only `recorder::track_cap(self.license.is_supporter())`. It never calls `licensed()`, which answers "may the code toggle rather than brand", and never reads the key's tier. A scan pins this.
- A gesture is refused if and only if it would raise `counted_tracks()` above the cap.
- The gate is asked before any mutation and before `begin_one_moment`, so a refusal leaves no undo moment.
- Batches are all-or-nothing, and the refusal line says how many fit.
- `active_channels` becomes `desk_extent()`, which never reads the licence.

### 5.2 Counting

- **Folders count.** A folder row can carry clips of its own (`instrument.rs:a_folders_own_row_is_faded_once_and_not_twice`), and `make_folder` spends a row.
- Bus, Placeholder and Vacant rows do not count.

### 5.3 Roads

The lane re-derives this list by grepping every caller of `add_bare_track`, `open_lane_below`, `paste_track`, `set_channel_kind` and `make_folder`, and every write of `tracks_added` and `channel_kinds`. Roads to gate:

- `add_bare_track` (Ctrl/Cmd+T in both views, the header's circled plus, `HdrAdd`, the mixer +);
- `add_track_first_free`;
- `insert_track_below`, `paste_track_below`, `paste_track_at`;
- `import_audio_at`, `insert_media_files_loop`;
- both branches of `make_folder`;
- `set_channel_kind` from none or Bus to MIDI/AUDIO;
- MIDI import that creates lanes;
- from W6, a media drop below the last lane or onto a placeholder.

**Never gated:** open, revert, undo, redo, template adoption, New, Reset, migration, persistence, export, freeze, the engine, drawing, bus creation, and edits to rows that already exist.

**Leak closed in W1.** `open_lane_below` searches `n+1..CHANNELS`, and `add_bare_track` checks only `n < CHANNELS`. On a full free desk, Insert or Paste Track below spends lane 16, and the document-is-wide rule then opens all 32.

### 5.4 Refusal lines

No em dash in any of these. Wording follows owner call 9's default.

- **FreeCap:** "The free desk holds 32 tracks. A supporter key (or a certain code) removes the limit. Buses, returns and sends are unlimited either way." Ctrl/Cmd+T at the cap says it; today it silently does nothing.
- **Ceiling** (W1 to W5 only, a supporter at 32 on an internal build): "This build's desk holds 32 tracks."
- **From W6:** the trailing + draws locked at the cap and opens the Supporter Key dialog.

### 5.5 Konami path

`keys.rs:Konami` → `Dialog::Cheated`/`CheatFun` → `DialogAction::AcceptCheat`, which calls `unlock_cheat` for a keyless user and toggles the marker for a licensed one.

- `is_supporter()` is `cheat || tier >= TIER_SUPPORTER`, so no new standing is needed.
- The cap reads standing live, so it lifts on the next gesture in the same session.
- `reset_settings(Everything)` calls `relock_cheat()` and raises `ProjectRequest::NewKeeping`. The reset is the discard: the outgoing session is not saved. A keyless cheater is free again; a SAVED 40-track project still opens whole.
- A buyer's toggle never changes standing.
- license.rs does not change, except for its cheat doc.

### 5.6 Downgrade rule

Covers no key, a key capped below this major, a relocked cheater, and a supporter's project on another machine.

- A 40-track project opens, draws, plays, edits, reorders, deletes, saves and exports whole. Buses are always allowed.
- New tracks are refused while counted >= 32. Deleting to 31 allows one more.
- Templates with more than 32 tracks open whole.
- Persistence, the engine and drawing never read the licence (scan).

### 5.7 Deterministic tiers

`TangentApp::new` calls `LicenseStore::load()`, which reaches the real config dir: `tangent-core` is not built under cfg(test) when tangent-ui's tests compile. Today `the_heart_animates_for_a_window_that_is_looked_at_and_rests_for_one_that_is_not` writes and deletes the real `cheat.unlock`, and `reset_everything_resizes_the_desktop_window_now` deletes it. Therefore:

- `TangentApp::with_license(ctx, settings, caps, store)` is added; `new` is production only.
- Every test in both crates uses a per-test temp store:
  - free;
  - cheater, via `unlock_cheat`;
  - supporter, via `install` of the committed, genuinely signed, perpetual `tests/fixtures/supporter.key` (name Ganten).
- The key text is `include_str!`'d only under cfg(test).
- A cfg(test) sentinel in `undo_commit` fires when a free app ends a non-exempt gesture above max(previous count, cap).
- About knows the standing: `Dialog::About { supporter: bool }` is set at `MenuAction::ShowAbout` (W1), and `about_lines(caps, supporter)` writes the copy (W3).

---

## 6. Persistence and migration (C3, C4, C10, C12)

### 6.1 Format 2 and the one switch

`desk_format::{write, read}` emit one `desk` object for settings.json and for documents (JSON in Appendix D).

- Rows are written in order, vacant rows included.
- `plugins` and `bypass` are sorted by SeatBay. Routes, keys and sends name Nodes.
- Curves carry `lane` for rows and `target: "master"` for the master. Plugin automation keys stay `fx:<bay>:<id>`, resolved through `auto_key`.
- Kits live in drumpad.json `track_kits`.
- Legacy desk keys are consumed with `shift_remove` and never written.
- Legacy snapshots carry a `Says` bitset.
- Fingerprint terms are APPENDED.
- `Routes::from_json` reads every route, keeping dedup.

`WRITE_V8` stays false until W4-integrate checkpoint (B). It is the ONLY switch:

- `SETTINGS_VERSION = if WRITE_V8 { 23 } else { 22 }`
- `ARRANGEMENT_VERSION = if WRITE_V8 { 8 } else { 7 }`
- the settings and document desk writers;
- the drumpad.json writer;
- the host's pstate names;
- the backup and pending-marker hooks.

Tests reach the format-2 writers through internal `*_as(Format::V2)` entry points and never flip the const. So nothing is stamped 23 or 8 before the file really is format 2, and one commit flips everything.

### 6.2 Read pipeline (dispatch on shape, never on the stamp)

1. `desk.format == 2` → `desk_format::read`. Length fingerprints are never consulted.
2. Otherwise the proven chain runs, with every live-constant read frozen to literals:
   - `adopt_desk`'s and `lift_eras`' two grows;
   - `Arrangement::from_map`'s `remap_lane(16, CHANNELS)`;
   - `shift_insert_states_for_v20`'s `CHANNELS - 16`;
   - `move_legacy_pad_racks`' live STRIPS base, now legacy path only;
   - `migrate_from`'s was<16 and was<17 grows.

   Bay moves go through `desk_lift::lift_bays<B: LiftBay>`, so one code path yields the lifted desk and its provenance.
3. → `DeskModel::from_v22(identity ids)`. A document read this way gets `legacy_states = Some(era)`.

### 6.3 Errors never cost a file

`read` → `Ok(DeskRead { model, dropped })` or `Err(DeskRefused { why: Structural, raw })`.

- **Referential or local errors drop that one entry**, list it, and raise one notice. These are: a plugin, bypass, route, key, send or curve naming no row or special; a Pedal or Pad not on a track; a bay >= 3; a chain >= `MAX_CHAIN`.
- **Repaired silently:** `next_id` not above every id.
- **Structural errors refuse the DESK only.** These are: more than `MAX_ROWS` rows, duplicate ids, a desk not of the documented shape.
  - **settings.json:** every other setting is kept. The desk becomes the default desk, and the raw desk JSON goes to `settings.desk-refused.json` (first free `-N`) with a notice. Never `Settings.unreadable`, never a first launch.
  - **document or template:** the open is refused with a message; nothing is written over it.
  - ~~**DAW blob (`Settings::from_json`):** every other setting is kept, the desk becomes the default desk, and the raw desk is kept under `extra["desk_refused"]`, so the DAW session keeps it across saves.~~ Removed 2026-10-10 with the plugin crate (`lane/ul-w8-plugin-gone`): the DAW session was the only thing that kept a blob. `from_json` is a test road now; it keeps every other setting and the default desk, and leaves the refused desk in `desk_refused` for its caller.
- Nothing is allocated by a key's numeric value. `LEGACY_MAX_STRIPS`, `INSERT_SPACE` and `TOTAL_SEATS` bound only the legacy reader.
- **Writer side:** under cfg(test), `DeskModel::check()` runs after every structural edit and in the undo sentinel.

### 6.4 Era table (frozen; full in Appendix E)

| Source | Shape | Lift |
|---|---|---|
| settings and DAW blobs, stamp ≤ 18 | pre-5.0 | frozen `migrate_from` arms |
| settings v19 | 16 ch, 19 sends | V16 |
| settings v20, 24-channel window (9224522..936f3bb, 2026-08-31) | 27 sends | NEW V24 arm |
| settings v20 to v22 | 35 sends | none |
| documents v4/v5 | 11 sends (84-bay pad sub-era), 18, 19 | V8Pads16 / V8 / V16NoDrumBus / V16 |
| documents v6 | 27 or 35 sends | V24 or none |
| documents v7 | head + tail | none |
| settings v23 / documents v8 | format 2 | direct |

- `master_auto_lane(version, era)` gives 16 below v6, 24 for V24, 32 otherwise, and maps that lane to the master. Any other curve at or past the era width is dropped, as today.
- Undo moments live in memory and need no migration.

### 6.5 Pre-existing losses fixed

- **`strip_phase`:** `adopt_desk` lifts `strip_muted` and `strip_soloed` but not `strip_phase`, which arrived in 82b62e6 inside the 16-channel era. `lift_strip_mask` moves phase as well.
- **The 24-channel window** had no arm, so its buses and master landed on channels 24..26. It gets the V24 arm and a V24 shelf map.
- **8- and 16-era project shelves** reopened bus, master and pad knobs at defaults (see the `lift_eras` comment). Provenance fixes this.
- **More than 64 routes** were cut on read.
- **The one-launch shuffles** ran AFTER `open_project_at_launch`, behind flags that lived only in memory while `Settings::load_with_reader` can save at load. They are replaced by the copy-only conversion with a durable source marker (6.6).

### 6.6 Plugin-state shelves

**Rule: a new build never renames or deletes a legacy `insert-state-S-B.bin`.**

**Global shelf:**

- `desk_lift::legacy_shelf_seat(ShelfShuffles, seat) -> Option<usize>` composes the frozen in-place renames (v13, v14, v18, v19, v20 and the new v24; Appendix B) into a pure map.
- **The source set is durable.** `Settings::save_to` writes `<config>/.shelf-v23.pending` (`{"settings_version": was, "shuffles": [..]}`) before the first format-2 settings write, unless `.shelf-v23` exists. It stores the SET of shuffles that load raised, not the stamp, because v19, v20 and v24 are raised by the desk's sends length (11, 19, 27) and not by `settings_version`.
- **`convert_global_shelf`** runs in `DesktopApp::new` before `open_project_at_launch`:
  - the source set is the pending marker if present, else this launch's `Settings::shelf_shuffles()`;
  - for each legacy file, its seat is mapped, then `decode_v32` + identity ids give a SeatBay;
  - the file is copied to `pstate-<sb>.bin` via temp + rename, unless that name exists;
  - `.shelf-v23` is written last, and the pending marker is removed.
- The conversion is idempotent and crash-safe at every point, and a pstate file deleted later never comes back.
- The in-place shuffle functions are deleted.
- The Minimal build never converts but still writes the pending marker, so the next full build converts with the right set.

**Project and template `state/`:**

- **Marker.** A document migrated from a legacy desk carries `desk.legacy_states = "<era>"` on every save.
- **Reading a shelf.** For each plugin SeatBay, use its pstate file if present. Otherwise, if the marker is set, try the legacy name: `encode_v32(Row(id) | special, rack, bay)` mapped through the era's provenance. The identity rule makes ids below 32 legacy rows. The owner check (bundle path) stays on every read.
- **Clearing the marker.** The host clears it only after a save road's mirror, and only when every plugin in the ON-DISK document has a pstate file in `state/`. It patches that one key in the on-disk JSON (temp + rename) and clears it in memory. It never re-serializes the session to do so.
- **Any road can write a v8 document safely.** That includes the host roads, which write the document first by design (`save_project_and_states`, `switch_project`, the New arm of `run_project_request`, `on_exit`), and the tangent-ui roads that cannot write pstate at all (`SaveChangesSave`, `save_project_first`, `save_project_as`). None of them can leave a plugin at defaults.
- **`TangentApp::seed_template`'s named-flag heal** patches `named` in the file's JSON and keeps its format.
- **`build_template`'s copy** skips `state/insert-state-*` and `*.pre-v8`. The template gets a full pstate mirror from the live shelf, and its document carries no marker.
- Nothing in a project or template folder is renamed or deleted at open, mirror or save. Every state write is temp + rename.

### 6.7 Backups

Backups are triggered by the first FORMAT-2 write over a legacy file, never by a stamp. Each is written once, via temp + rename:

- `settings.pre-v23.json`: the legacy settings file exactly as it was.
- `<doc>.pre-v8`: beside each legacy document, templates included.

`holds_only_scratch` and the Untitled sweeps accept `<doc>.pre-v8` as Tangent's own file. Save As Template never copies one. Legacy sidecars are never deleted, so restoring `<doc>.pre-v8` hands a release build its project back whole.

### 6.8 drumpad.json

- **Reader:**
  - `track_kits` (`"t<id>": kit`) plus `primary_track` when present;
  - otherwise the legacy `lane_kits`, the top-level kit and `primary_lane`, read as TrackIds by the identity rule (only legacy builds ever wrote them).
- **Writer:**
  - With `WRITE_V8`: `track_kits` keyed through `track_ids()`, later through the id-keyed map, plus `primary_track`. `lane_kits` and `primary_lane` are never written again.
  - Without `WRITE_V8`: today's bytes.

### 6.9 Staging isolation (C12)

`tangent_core::config::STAGING` is true on the branch from W1 and deleted in W8. With no override:

- **Config dir** is `~/.config/tangent-staging`: settings, global shelf, licence, cheat marker.
- **Media root** is `<Documents>/Tangent Media Staging`: Projects, Templates, Takes, Exports.
- **`pin_legacy_media_root` is off.**

`TANGENT_CONFIG_DIR` and `TANGENT_MEDIA_ROOT` override these, so pointing an internal build at real files is a deliberate act.

---

## 7. Engine: the audio-thread strategy (C6, C7, C8)

### 7.1 Ownership rule

- A `Lane` box holds only state that its own hand reads and writes during the fork.
- What other lanes read during the fork, or what is written for all lanes outside it, lives in the spine's `DeskBuffers` or in the plan.
- What the host also reads lives behind an `Arc` of ATOMICS ONLY.

### 7.2 Lane

- **`shared: Arc<LaneShared>`, atomics only:**
  - today's per-lane atomics (gain, pan, peaks, meters);
  - `pending: AtomicU32` (mail: the host bumps it before pushing into any of this lane's rings);
  - `racks: [RackShared; 9]` (per-bay latency, gr, bypass and key bits).
- **Racks: `chains: [Rack; 2]` (own, extra) and `board: [Rack; 7]`.**
  - `Rack` keeps today's `slots`, per-bay `incoming`/`retiring` rings and `Rack::swap`'s room-first rule.
  - It gains `auto: [ParamQueue; 3]`: audio-owned `ParamPoint` queues, capacity reserved at build, filled by the serial `resolve_automation` and read by this lane's hand. Today `resolve_automation` clears and pushes `auto_params: Vec<Vec<ParamPoint>>` every chunk, so these can never sit behind a shared `Arc`.
- **`pads: Option<Box<PadBank>>`**, attached by `EngineOp::Attach`. `PadBank` owns the 128 pad racks and `pad_shared: Arc<PadShared>` (the seven 128-wide atomic tables); the host holds the other clone.
- **An instrument slot** with its own two-deep in/retire ring pair, replacing the shared `CHANNELS + 2`-deep rings.
- **Per-lane render state:** `pdc` and `dry_pdc` DelayLines, `LaneGrains`, the dry rows, the decide outputs, note and control banks, pedal state, `told`, `told_off`, `park_from`.
- **`rendered: bool`** and **`claim: AtomicU64`** (7.6).
- **NOT in a lane:** the key snapshot, stems, dry stems, the mix/aux/send accumulators, route arrivals.

### 7.3 Spine and desk buffers

- **`Spine { lanes: Vec<Option<Box<Lane>>>, gens: Vec<u32>, desk: DeskBuffers, check: LaneSet, rank_scratch }`.** One capacity covers everything: it starts at 64 and doubles.
- **`DeskBuffers { stem_scratch, dry_scratch, key_prev, key_prev_frames }`.**
  - Each is `MAX_BLOCK (512) x capacity x 2` floats, indexed by SLOT: 4 MB each at 1024 lanes.
  - Written only in serial passes: the fold, the level publish, the snapshot copy.
  - Borrowed only as `&` by the fork, so a keyed lane reads another lane's last-chunk stem without aliasing anything. Today `Renderer::key_prev` is shared this way through `DeskRef.key_prev`/`KeyFeed` in `render_lane`, and that is kept.
- **`Grow(Box<SpineShell>)`.** The host builds a larger spine with every buffer allocated. The callback moves lane pointers across, memcpys `key_prev` (key latency stays one chunk) and retires the old shell.
- **`LaneScratch`** (about 16 KB) is per HAND (width + 1 sets).
- **Deleted:** the per-block `jobs: [Option<LaneWork>; CHANNELS]` array, `LaneCells`, `lane_seats`, the three `split_at_mut` carves.

### 7.4 Ops

**Variants.** One op ring and one retire ring carry `EngineOp`:

- `Grow`
- `Install(Box<[(Slot, Box<Lane>)]>)`
- `Remove { slots, out }`
- `Attach { slot, gen, part: LanePart }`, where `LanePart::{Pads(Box<PadBank>), Kit(Box<PadKit>), Chain { index, rack }}` (Chain only after owner call 1)
- `Plan(Box<DeskPlan>)`
- `Monitor(Box<MonitorPlan>)`
- `Capture { pick: PickId, ring: Option<Producer<f32>> }`

Ops are applied in FIFO order at the top of the callback, before the first chunk.

**Rules at apply:**

- **Retire room first.** An op is taken only when the retire ring has room for everything it displaces. Otherwise it waits a block. Never `let _ = push`.
- **Install** refuses an occupied slot.
- **Attach** checks the slot's gen and retires the part on a mismatch.
- **Remove** moves the boxes out whole.
- **Plan validation.** A plan whose lane list repeats a slot, names an empty slot, or carries a stale gen is refused. It is retired whole and `plan_refused` is counted: the host shows the stall line, and tests assert the count stays zero. The previous plan keeps playing. The check is O(N), using `Spine.check`.
- **Capture** is refused while that pick's capture flag is set.
- **Monitor** carries the device ring and its layout together. The displaced ring and plan retire. Today `Renderer` assigns `self.monitor = Some(ring)`, which frees the old `Consumer` on the audio thread, and reads the layout with `try_lock`. W3 fixes both while keeping the pub API.

### 7.5 What is slot-bound, and how it is checked

Two rules:

1. **Desk-wide schedule lists are validated at USE.**
   - Clips, MIDI and automation keep today's whole-list swap: two-deep in/retire, with a parked retry in `retire_tick`.
   - Entries carry `(slot, gen)`, sorted by slot on the host, with a bounds table.
   - A lane renders only entries whose gen equals its own. Mismatches are skipped every chunk and never dropped on arrival.
   - So a list that lands before its Install sounds from the block the Install lands.
2. **Per-lane parts ride FIFO behind their Install.**
   - Kits, pads and chains travel on the op ring as `Attach`.
   - Plugins and instruments travel on the lane's own rings, which exist from the lane's build.
   - Nothing can reach a lane before the lane exists.

Also slot-bound and gen-checked at use:

- `UiParam { slot, gen, rack, bay, id, value }`. A mismatch is dropped: it is a knob edit to a lane that is gone.
- Tap aims.
- `MonitorPlan` pick bindings `(PickId -> slot, gen, width)`.
- The stem-arm list.
- Punch binds.

No re-send protocol is needed.

### 7.6 DeskPlan and the fork

**DeskPlan.** Built on the UI thread by `build_plan`, moved in boxed, retired whole. It carries:

- the lanes in RANK order, with gen and flags;
- parents by rank;
- punches;
- MIDI routes;
- picks;
- `RouteTable`: today's Kahn order (gain > 0, ascending tie-break, cycle members appended one block late), CSR inputs, transfer gains computed iteratively;
- `SendTable` and `KeyTable`;
- `levels` (7.8);
- `gains: Arc<[AtomicU32]>`, carried across plans by edge identity;
- `any_solo`.

**Republishing.**

- Order-deciding inputs republish the plan: route existence, a gain crossing zero, the hardware flag, folder parents, kinds and arm.
- Scale-only inputs stay atomics.
- The bridge writes each gain change into EVERY plan it has published and not yet seen retired, so no write is lost across a swap.

**Fork.**

- `run_list` runs over rank indices.
- The claiming hand maps rank to slot and swaps `lane.claim` to the round. If the lane already held this round, the hand skips it. This is the take-once guarantee that `LaneCells::take` gave, so two hands can never hold one lane.
- A lane whose hand panics keeps `rendered == false` and folds nothing that block. The serial decide walk clears `rendered` before every fork.

**Swap polling.**

- A lane's rack rings are polled only when its `pending` mail is non-zero, behind today's global `pending_swaps` gate.
- Automation queues are cleared through a dirty list.

### 7.7 Heard, per chunk

`resolve_heard(plan, auto_mute, out)` runs on the audio thread once per chunk.

- O(N) and allocation-free.
- Exactly `channel_is_heard` + `strip_is_heard` + `Renderer::muted_now`, including `AutoParam::Mute` curves.
- Property-tested against frozen copies of today's functions.
- Removes the `n.min(63)` aliasing.

### 7.8 After the fork: levels

**What `DeskPlan.levels` is.** The topological layering of the post-fork graph:

- folders (children into the folder);
- Bus rows (routes and sends in);
- legacy channel-route destinations (their second rack run).

Cycle members go in one final level, in today's order, one block late.

**How a level runs.**

- Its live nodes run through `run_list` when two or more are live, else serially.
- Each node owns its input and output buffers in its Lane box and uses its hand's scratch.
- Inputs are summed in exactly today's serial order: a folder sums its own row, then its children ascending (`Renderer::fold_folders`); a route destination sums in Kahn arrival order.
- Outputs are published serially, in today's order, into `stem_scratch`, `mix` and `aux` before the next level starts.

**What stays serial.**

- Return 0 (Fx) and the master.
- The lane fold into `mix`/`aux`, in rank order. f32 `+=` is not associative, so this order is the sum.

W7 may move each lane's fader, pan, peak and stem-column work into its own hand, keeping only the ranked sums serial.

### 7.9 Order rule (bit-exactness)

- **Rank order** governs:
  - the decide visit order (caps choose their victim by it);
  - the fold;
  - Kahn tie-breaks;
  - level publish order.
- **Slot** keys the grain-stagger phase: `plan_grains`' `ch % phases` becomes `slot % phases`. A reorder therefore never touches grain scheduling.
- **Legacy desks** install row r at `Slot(r)` and skip vacant rows, which never folded anyway. Slots equal today's lane indices and ranks keep today's relative order, so the goldens stay bit-identical.
- MIDI_OFF edges are found per lane at the plan swap, so held notes are released (`live_midi_off`'s rule).

### 7.10 rtwork

- `run_list(&[u32], &(dyn Fn(lane, hand) + Sync))`.
- The claim word is `round << 32 | cursor`.
- Guided batches: `max(1, remaining / (2 * hands))`.
- `done` is a count, and `MarkDone` adds its whole batch on drop.
- The stale-round guarantee is unchanged.
- The pool forks only for 2 or more lanes (a join measures 6 us).
- The uncontended mutex in `LanePool::run` is out of scope.

### 7.11 Masks and clamps

- Every u32/u64 lane mask in the walks becomes a `LaneSet` or a per-lane flag.
- Every `.min(31)`, `.min(63)` and `min(CHANNELS - 1)` goes in ONE change, behind a ban scan. Release builds have no overflow checks: lane 32 aliases lane 0, and a clamp folds lane 40 onto 31.
- The 8-bit `pack_route`/`pack_key` fields go.
- `UiParam`'s u16 seat goes.
- `arr_capture` becomes a per-pick flag set.

### 7.12 Memory sized by use

- **Stem ring:** built at arm time for the armed slots. Today it is `rate*CHANNELS*2*4` floats, about 49 MB, always allocated.
- **Capture rings:** built per armed pick. Today they are about 1.5 MB per input.
- **Picks:** heap-backed, one per bound audio track. `MAX_PICKS` and `INPUTS` are deleted.
- **Per-lane cost:** an idle non-drum lane is pinned at ≤ 256 KB at build. Today it is about 3.5 MB.

### 7.13 Live scenarios

- **Add while playing:** Install, then Plan. N silent lanes change no output bit.
- **Remove:** a Plan marking the lane RETIRING (a 10 ms ramp), then a Plan without it, then Remove.
  - The lane comes home whole. The host writes each bay's state to its pstate file on the global shelf, then drops it. Undo reloads from those files, with no DisplacedShelf dependency.
  - An id that reappears before its old lane is home waits for it (a frame or two).
  - Offline engines skip the ramp.
- **Reorder:** one Plan. No plugin moves, no renames, no round trips.
- **A lane becomes a drum lane:** Attach(Pads) then Attach(Kit). Nothing reloads.
- **Take:** capture only (owner ruling), so add, remove and reorder are allowed. Picks are stable, so the input stream is never reopened by a shape change.
- **Arrangement record:**
  - It binds `(TrackId, slot, gen, PickId)` at the press and resolves rows at the finish.
  - While it records, `desk_shape_held` makes insert, remove, reorder, kind changes and input-binding changes wait with the line. Add appends at the end, so no row and no pick moves.
  - Picks are keyed by TrackId: explicit bindings get the lowest free PickId and keep it. Ordinal mode, for files that never named an input, is unchanged.
- **Realtime export:** stems bind `(slot, gen)` at the press. Add appends. Today's holds (`lane_order_held`, `tracks_held`, `folders_held`, `routes_held`) fold into `desk_shape_held` and keep their lines.
- **Offline export and freeze:** `Bridge::full_build` builds a separate engine with its lanes installed synchronously.
  - A freeze is `full_build(Freeze(TrackId))` plus a one-slot stem tap.
  - `FreezeJob` holds the TrackId and lands through `row_of`.
- **Session switch:** one Remove batch, one Install batch, then per-bay reconcile.
- **Device rebuild:** a new engine and a full Install.
- **Stall:** publishing stops behind the unacknowledged op and the existing stall line shows. Nothing is lost, because the model is the source of truth.

### 7.14 Invariants

1. **The callback only moves pointers.** No allocation, free or new lock, and no last drop of an Arc, Box or PluginBox.
2. **Only finished things cross:** buffers sized, plugins warmed.
3. **Retire room first.**
4. **Everything slot-bound carries the gen it was built for**, checked as in 7.5.
5. **The host holds the last reference** of every LaneShared, RackShared and PadShared and of every plan's gains.
6. **A lane holds only what its own hand touches.** Desk-wide buffers are written only in serial passes.
7. **A plan is validated before it plays.**

---

## 8. Host: the bridge (C9) and the shelf (C10)

`tangent/src/engine_bridge.rs:Bridge` is the one translator. Each frame, `sync`:

1. diffs ids against `TrackId -> Slot`;
2. builds lanes for new ids, sized, with no plugins, and installs them in one batch;
3. removes vanished ids in two phases, writing their states to the global shelf;
4. builds a plan only when its inputs changed (the `PushedDesk`/`desk_unchanged` pattern);
5. writes continuous values into LaneShared;
6. ships the desk-wide clip, MIDI and automation lists with `(slot, gen)` and their bounds computed here;
7. ships kits and pads as `Attach`;
8. reconciles plugins by SeatBay, one bay per frame, through one `HostRack` per bay. This replaces the nine `Vec<[_; INSERTS]>` tables on `Engine`.

Further rules:

- **Picks.** `sync_inputs` owns the `PickTable` (TrackId → PickId) and the MonitorPlan.
- **Flat seats until W5.** The bridge decodes the app's flat seats with `desk_legacy::decode_v32` + `track_ids`. From W5 it reads the SeatBay maps directly.
- **One offline builder.** W2 extracts today's offline furnishing into `engine_bridge::furnish_offline(app, rate, timebase, BuildFor)`, used by both `start_offline_render` and `furnish_probe_engine`. Their documented arm and MIDI difference becomes a `BuildFor` arm. W4 grows it into `full_build`.
- **Deleted in W4:** the reorder carry, `Engine::permute_channel_inserts`, `permute_loaded_rows`, `permute_insert_states_in`, `permute_displaced`, reload-on-reorder.
- **Bay moves become explicit SeatBay relabels:** FX drag, copy and paste FX, `trade_inserts`, the pad-FX swap. The instances travel through the bay rings, and both pstate files are rewritten read-all-then-write.
- **The engine stops reading tangent-ui capacity constants** (W4-integrate (C)):
  - cfg(test) shims use an engine-local `LEGACY_LANES = 32`;
  - host loops read `app.desk_extent()` or the Bridge;
  - `midi.rs` reads the pad count through `desk_id::PADS`.

  W5 and W6 can then delete the tangent-ui constants without touching engine files.

---

## 9. Routing, buses, returns, sends: no limits on either tier

- **Bus rows** (`ChannelKind::Bus`, byte 3, append-only):
  - no clips (a drop makes a new track), no input chip, never hears MIDI;
  - solo-safe, never frozen, not counted;
  - a fader, pan, mute, solo, six bays, automation, colour and name;
  - routes onward, and can be a key source;
  - its rack runs ONCE over what arrives;
  - it runs in its topological level, in parallel with its peers (7.8).
- **Fx is Return 0**, a special. It keeps the built-in reverb, delay, chorus and phaser and its fader, bit-identical.
- **Sends:** `Vec<Send { to, level, pre }>`, any number, to Fx or to any bus row.
  - Today's knob becomes the Fx entry, and `AutoParam::Send` stays the Fx send.
  - `AutoParam::SendTo { to }` is appended, with key `send:t<id>`.
- **Routes:** `RouteEdge { from: Node, to: Node | Hardware { first, stereo }, gain }`, unbounded, with MOVE and COPY semantics. Deleted: `MAX_ROUTES`, `Routes::add`'s refusal, the `from_json` cut, the full-table notice, the engine route arrays and the 8-bit packing.
- **Sound changes ride only on the new kind.**
  - A legacy channel-to-channel route keeps today's double rack run (the SEND-BARGAIN) and its cycle rule, bit for bit.
  - A Bus destination runs once and gets PDC on its paths.
- **Cycles:** pickers grey out any target that can already reach the source (`desk_topology::would_cycle`). A legacy saved cycle opens and sounds the same, and is drawn as feedback.
- **Solo:** a return or bus is never silenced by another strip's solo. A soloed bus hears its feeders.
- **Keys:** `SideKey { seat, from: Node }`, from any track or bus.
- **Folders** stay positional runs.

**What free gets: exactly what paid gets.**

- The owner said "No limits here".
- REAPER, Bitwig and Ardour do not ration these. Ableton Lite's 2 returns is the cap people resent; Standard allows 12.
- Any track is already a submix through a channel route, so a bus cap could be routed around.
- A bus cannot hold clips, inputs or notes, so the track count is the one lever that cannot be bypassed.
- Every extra capped resource is another "a licence must never silence music" surface.

---

## 10. UI

- **Typed identity (W2):**
  - `Column::{Track(usize), Click, Master}` replaces `Column(usize)` and `COLUMNS`;
  - `StripRef::{Row(usize), Drums, Click, Fx, Master}` accessors become the only home of strip-index math until W5;
  - `LaneId::{Row(usize), Master}` replaces `MASTER_LANE` and `MASTER_COLUMN`.
- **DeskIndex:** caches the shown order, ranks, classes, parents, child counts and clip counts. Its generation is bumped by every row, clip, folder, kind or plugin mutation and by undo, redo and load. Views carry only the visible window, the pinned strips and totals, so build, layout, hit test and a11y are O(visible).
- **Geometry:** strip and lane size follow the requested visible count, never the track count.
  - `VISIBLE_MAX = 64`; `COUNT_LADDER` gains 48 and 64; `DEFAULT_MAX_STRIPS = 16`.
  - Thin scrollbars in both views.
  - Pickers filter as you type and label by name; a11y ids come from TrackIds.
- **Reversed visuals (W6):**
  - rows that do not exist are not drawn;
  - one trailing + (left of the pinned click and master in the mixer; a row under the last lane in the arrangement), drawn locked at the free cap;
  - dropping media below the last lane makes a track;
  - Remove deletes the row.
- **Palette:** `% PALETTE_SLOTS` (32), so tracks 0..31 keep today's colours.
- **Copy (W3):**
  - About is tier-aware: "32-track desk" free, "unlimited tracks" with a supporter. Minimal keeps its chord-display line (the plugin did too, until its crate was removed on 2026-10-10).
  - The Supporter Key dialog says: "Tangent is free, with up to 32 tracks. A supporter key (or a certain code) takes the track limit away and lights the heart. Buses, returns, sends and routes have no limit for anyone."
  - The same facts go into README, STORE-CONTENT, a new GUMROAD page, the tangent-fulfil email and CHANGELOG [Unreleased].
  - Keys already sold gain unlimited tracks (they are perpetual: `max_major: 0`).

---

## 11. Tests

### 11.1 Tiers

- `with_license` everywhere, with the three fixtures.
- Scans: no test calls `TangentApp::new`; the gate reads `is_supporter()`.
- The suite gives identical results with HOME redirected (CARGO_HOME and RUSTUP_HOME pinned) and with a real `license.key` present.

### 11.2 Konami and paywall suite

The gate parts land in W1 under `force_free_cap(4)`; everything above 32 lands in W6, in the tangent crate through Bridge.

1. **Cheater adds:** a cheater adds tracks 33, 64, 65, 100 and 200 through every road. Each draws after scrolling in both views and is audible through an offline engine.
2. **Free refuses:** a keyless free app refuses the 33rd track on every road with the line, changes nothing and leaves no moment.
3. **Relock (restated):**
   - A keyless cheater SAVES a 40-track project, then runs Reset Everything.
   - Assert `cheated()` is false and `project_request == NewKeeping`.
   - Reopen the saved document through the host's apply path and assert: 40 rows, each audible offline, `counted_tracks() == 40`, the 41st add refused with FreeCap, one add allowed after deleting to 31.
4. **Buyer toggle:** a buyer toggling Konami stays unlimited, as does a buyer who removes the key while the marker stands.
5. **Marker only:** a directory holding only `cheat.unlock`, reloaded fresh, is unlimited.
6. **Key feed:** Konami typed through the key feed lifts the cap in the same session.
7. **Repeat:** re-entering the code as a cheater changes nothing.
8. **Capped key:** a version-capped key is free.
9. **Downgrade:** a free app opens, plays, edits and saves a 200-track cheater project whole, with bytes round-tripping, and refuses the next add.
10. **Undo:** undo and redo work across the cap boundary.
11. **Tier-blind:** persistence, the engine and drawing are tier-blind (scan).
12. **Sentinel:** the `undo_commit` sentinel stays silent.

### 11.3 Era corpus

Lives in `tests/fixtures/eras/`. It is synthetic, built from the frozen tables, and runs in tangent-ui so the gate runs it. It holds:

- settings v12, v18, v19 (with a phase-inverted bus and master), v20 at 27 sends, v20, v21, v22;
- documents v4/v5 (11, 18, 19 sends, the 84-bay shape), v6 at 27 and 35, v7, with master curves at 16, 24 and 32;
- DAW blobs from 3.x, 4.x and 5.x;
- V8, V16, V24 and V32 shelf folders;
- drumpad.json in both shapes;
- a template;
- error fixtures: one dangling plugin, bypass, route, key and send; one structural refusal per kind.

Rules:

- Assertions are LITERALS.
- Idempotence: legacy → v8 → read → v8 gives equal bytes.
- `legacy_shelf_seat` equals running the frozen in-place shuffles on synthetic shelves.
- A local `#[ignore]` test over `TANGENT_CORPUS_DIR` copies real projects read-only and hashes their folders before and after.

### 11.4 Goldens

- **Render goldens (W1):** about 24 desks of built-in units, FNV over the output f32 bits. They cover folders, routes, a cycle, hardware, sends, drums with pad FX, DX7, Keys, the sampler, automation (mute and master included), PDC, solo in a folder, phase, frozen lanes, stems, a key, grains on and off, the fork on and off.
- **Export goldens (W2):** master, stems and dry stems as WAV bytes plus file names, through `furnish_offline`.
- Both are aarch64 macOS only, with cfg_attr ignore elsewhere.
- In-binary invariants run on every arch:
  - fork equals serial at 33, 100 and 257 lanes;
  - N silent lanes change no bit;
  - the active-list walk equals the full walk;
  - level fork equals level serial.

### 11.5 Real-time

- **`rt_alloc`:** zero allocations and frees in the callback across Install, Remove, Grow, Attach, Plan, Monitor, Capture and bay swaps at 33, 65, 200 and 1000 lanes.
- **Alias harness** at 100 and 300 lanes (lanes 31, 32, 33, 63, 64, 65, 255, 256, 257 against 0, 1, 31).
- **Keys across hands:** a keyed lane reading a source rendered on another hand matches the serial render.
- **Delayed Install:** a clip-bearing lane whose Install is delayed a block is audible within 2 blocks.
- **Bad plans:** a duplicate-slot plan is refused; a forced double claim renders once.
- **rtwork:** exactly-once claims at 33, 1000 and 4096 lanes; a stale round claims nothing; a panic mid-batch finishes the block.
- **Generations:** a mismatch renders silence.
- `Renderer::for_tests(capacity)` replaces `test_renderer`.
- `stack_budget` keeps its ceilings, plus a `size_of::<Lane>()` ceiling.

### 11.6 Width and identity suites

- **Widths:** {1, 32, 33, 64, 65, 200, 512}.
- **Refusal tests split:** every refusal-when-full test splits into a supporter test that grows and a free test that stops at 32.
- **Session state follows its track:** removing or inserting a row above the open drum panel, a latched automation row, an open face and a pending load leaves each with its own track, through undo and redo.
- **Field-classification scan** (section 4).
- **Owner-ruling pins kept verbatim:** no two-row fold, the dozen at 100%, the eviction order, the three solo regressions.

### 11.7 Persistence crash tests

Kill points:

- after a settings save, before the conversion;
- mid-copy;
- after some copies;
- after the document write and before the mirror, on each of the eight save roads.

Every relaunch lands every state. No new-build path renames or deletes a legacy sidecar (scan plus a filesystem audit).

### 11.8 Performance (measured, never asserted in time)

- **Today's shapes:** rt_bench at 128 frames stays within 5% on 16x[EQ, comp, plate] (about 66 us mean, 104 us p99) and on 32x3 EQ (about 57 us).
- **New rungs:**
  - 64, 256 and 1024 lanes (marginal us per lane);
  - 50 returns x plate;
  - 25 folders x [EQ, comp, plate];
  - with the level fork on and off.
- **Asserted structural counts:** views built ≤ visible + pinned + 2; a reorder makes 0 sidecar filesystem calls and 0 reloads.

### 11.9 Census, copy, platforms

- **Census:** `scripts/test-census.sh` fails when a crate's `#[test]` count drops without a removed → replacement ledger in the commit body.
- **Copy:** the phrase scan (W3; folded into check.sh in W8).
- **Gate:** check.sh on macOS ARM.
- **Other platforms:** void, bsdbox and Windows run with `--no-fail-fast`; the 4 known ARM-only failures are expected there.
- ~~**Plugin:** `cargo check --manifest-path plugin/Cargo.toml` from W1.~~ Removed 2026-10-10 with the plugin crate (`lane/ul-w8-plugin-gone`).

---

## 12. Contracts between lanes (binding)

**C1 tangent-ui/src/desk_id.rs** (W1-identity-kernel)
- `TrackId(pub u32)`; `Node { Track(TrackId), Drums, Click, Fx, Master }`; `Rack { Chain(u16), Pedal(u8), Pad(u8) }`; `SeatKey { node, rack }`; `SeatBay { seat, bay: u8 }` (all Copy + Ord + Hash).
- Consts: `BAYS = 3`, `STRIP_RACKS = 2`, `AMP_PEDALS = 7`, `PADS = 128`, `MAX_ROWS = 65_536`, `MAX_CHAIN = 2` (raised with owner call 1 together with `AutoParam::Plugin`'s u8 bay), `STATE_PREFIX = "pstate-"`.
- `slot_key(Node, usize) -> Option<SeatBay>`; `display_slot(SeatBay) -> Option<usize>`; `auto_key(Node, usize) -> Option<SeatBay>`; `auto_bay(SeatBay) -> Option<usize>`; `state_file_name(SeatBay) -> String`; `parse_state_file_name(&str) -> Option<SeatBay>`.
- Strict Display/FromStr (Appendix C).

**C2 tangent-ui/src/desk_legacy.rs** (W1-identity-kernel)
- `V32_CHANNELS = 32`, `V32_STRIPS = 35`, `V32_TOTAL_SEATS = 4_392`, `V32_INSERT_SPACE = 13_176`, `V32_HEAD_BAYS = 108`.
- `LegacyOwner { Row(u16), Drums, Click, Fx, Master }`.
- `decode_v32(usize) -> Option<(LegacyOwner, Rack, u8)>` (a bijection over 0..13_176), `encode_v32`, `decode_v32_strip`.
- `bind(LegacyOwner, &[TrackId]) -> Option<Node>`, `unbind`, `identity_ids(usize) -> Vec<TrackId>`.

**C3 tangent-ui/src/desk_lift.rs** (W1-freeze-eras)
- `DeskEra { V8Pads16, V8, V16NoDrumBus, V16, V24, V32 }` with text names `v8-pads16 v8 v16-no-drum-bus v16 v24 v32`; `era_of(sends_len, inserts_len)`.
- `trait LiftBay`; `lift_bays<B: LiftBay>(DeskEra, Vec<B>, settings_path: bool) -> Vec<B>`; `lift_strip_rows`; `lift_strip_mask` (phase included); `master_auto_lane(u64, DeskEra) -> usize`.
- `Provenance { era, to_legacy: BTreeMap<u32, u32> }`; `provenance_of(era) -> Provenance`; `provenance_of_document(&Value) -> Option<Provenance>`.
- `ShelfShuffles` (set of `V13 V14 V18 V19 V20 V24`); `legacy_shelf_seat(ShelfShuffles, usize) -> Option<usize>`.

**C4 tangent-ui/src/desk_model.rs + desk_format.rs** (W2-desk-format)
- Types: `RowKind { Midi, Audio, Bus }`, `Send { to: Node, level: f64, pre: bool }`, `TrackRow { id, kind, added, icon, name, color, gain, pan, input, midi, mute, solo, phase, arm, theory, folder, in_folder, shut, sends, dx7, keys, frozen }`, `Special(s)`, `RouteTo { Node(Node), Hardware { first, stereo } }`, `RouteEdge { from, to, gain }`, `SideKey { seat, from }`, `DeskModel { next_id, rows, specials, plugins, bypass, routes, keys, legacy_states: Option<DeskEra> }`, `RowClass { Vacant, Placeholder, Bus, Counted }`, `RowUse`, `V22Desk`.
- `DeskModel::{class, counted, row_of, mint, append, insert_rows, remove_rows, permute, check -> Vec<Dropped>, from_v22, to_v22}`.
- `desk_format::{DESK_FORMAT = 2, WRITE_V8 = false, write, read -> Result<DeskRead, DeskRefused>, is_format2}`.
- `DeskRead { model, dropped: Vec<Dropped> }`, `Dropped { what: &'static str, key: String }`, `DeskRefused { why: Structural, raw: Value }`, `Structural { TooManyRows(usize), DuplicateId(u32), BadShape(&'static str) }`.
- `WRITE_V8` readers: the settings/document desk writers, `SETTINGS_VERSION`, `ARRANGEMENT_VERSION`, the drumpad writer, the host's pstate names, the backup and pending hooks. No other switch.

**C5 paywall** (W1-gate-konami)
- `recorder::{FREE_TRACKS = 32, DEFAULT_MAX_STRIPS = 16, PALETTE_SLOTS = 32, track_cap(bool) -> Option<usize>}`.
- `TrackRefusal { FreeCap { have, cap }, Ceiling { max }, Held }`.
- `TangentApp::{with_license(ctx, settings, caps, LicenseStore), new (production only), track_cap, counted_tracks, may_add_tracks(usize) -> Result<(), TrackRefusal>, desk_extent}`, plus cfg(test) `force_free_cap`.
- `Dialog::About { supporter: bool }`; `dialogs::about_lines(caps, supporter)`.
- Fixtures per crate: dir = `temp_dir()/tangent-lic-<pid>-<thread>-<tag>`; free = `load_from(dir/license.key)`; supporter = free + `install(include_str!(tests/fixtures/supporter.key))` under cfg(test) only; cheater = free + `unlock_cheat()`.

**C6 tangent/src/rtwork.rs** (W1-rtwork-list)
- `LanePool::run_list(&self, &[u32], &(dyn Fn(usize, usize) + Sync))` (lane, hand; caller is hand 0); `hands()`; `run(u32, ..)` survives as a shim until W4.
- Gate.owed = `round << 32 | cursor`; `done` is a count.

**C7 engine kernels** (W1-rt-kernels-goldens; extended in W3 and W6)
- `LaneSet { words: Box<[u64]> }`: `with_capacity`, `capacity`, `insert` (false past capacity), `remove`, `contains`, `clear`, `is_empty`, `iter`, `from_u32`, `from_u64`.
- `Slot(pub u32)`; `LaneFlags(pub u16)` with ARMED FROZEN MIDI_OFF DRUM BUILTIN KEYS MUTED SOLOED PHASE SUSPEND BUS RETIRING.
- `PlanLane { slot, gen, flags, parent: Option<u32>, punch: Option<u64>, midi, pick: Option<PickId> }`; `SpecialFlags`; `RouteTable`; `SendTable`; `KeyTable`; `Levels(Box<[Box<[u32]>]>)`.
- `DeskPlan { gen, lanes, by_slot, specials, routes, sends, keys, levels, gains: Arc<[AtomicU32]>, any_solo }`; `PlanInputs`.
- `build_plan(&PlanInputs, Option<&DeskPlan>) -> DeskPlan`; `resolve_heard(&DeskPlan, &[Option<bool>], &mut [bool]) -> SpecialHeard`.
- `EngineOp<L, P> { Grow(Box<SpineShell<L>>), Install(Box<[(Slot, Box<L>)]>), Remove { slots, out }, Attach { slot, gen, part: P }, Plan(Box<DeskPlan>), Monitor(Box<MonitorPlan>), Capture { pick: PickId, ring: Option<Producer<f32>> } }`.
- `Spine<L> { lanes, gens, desk: DeskBuffers, check: LaneSet, .. }`.
- `apply_ops(&mut Spine<L>, &mut Box<DeskPlan>, &mut Consumer<EngineOp<L, P>>, &mut Producer<EngineOp<L, P>>) -> ApplyReport { plan_refused: u32, .. }`.
- `rt_alloc::count_allocs`.

**C8 engine slot API** (W3-slot-engine)
- `RackOwner { Lane(Slot), Drums, Click, Fx, Master }`; `RackAddr { owner, rack }`; `LaneKind { Midi, Audio, Bus, Folder, Empty }`; `LaneSpec { kind, drum }`; `LanePart { Pads(Box<PadBank>), Kit(Box<PadKit>), Chain { index: u16, rack: Box<Rack> } }`.
- `Engine::{lane_capacity, install_lanes(Vec<(Slot, LaneSpec)>), remove_lanes(&[Slot]), attach(Slot, gen, LanePart), publish_plan(&PlanInputs), lane_shared(Slot), load_insert_at(RackAddr, bay, ..), unload_insert_at(RackAddr, bay), set_clips_slotted(Vec<SlotPlaced>), set_midi_slotted(Vec<SlotPlacedMidi>), set_automation_slotted(SlotAutomation), set_monitor_plan(MonitorPlan), set_capture_ring(PickId, Option<Producer<f32>>), offline_sized}`. All return `Result<_, EngineBusy>` where an op can wait.
- `UiParam { slot, gen, rack, bay, id, value }`; tap aims `(slot, gen, rack, bay)`.
- Today's pub API stays as shims onto slots 0..=31 until W4, then cfg(test) only.

**C9 tangent/src/engine_bridge.rs** (W2-engine-bridge; broker from W4)
- `Bridge::{new, sync(&mut self, &TangentApp, &mut Engine)}`.
- `furnish_offline(&TangentApp, rate, Timebase, BuildFor) -> Result<Engine, String>` (W2), which becomes `Bridge::full_build` (W4); `BuildFor { Export(ExportSpec), Probe, Freeze(TrackId) }`.
- `PickTable` (TrackId → PickId).
- From W5, `read_desk` (W5-identity-maps) and `sync_inputs` (W5-engine-memory-inputs) have separate owners.

**C10 tangent/src/shelf.rs** (W4-integrate)
- `state_path(SeatBay)`; `saved_state(SeatBay, &str)` (owner-checked); `write_state(SeatBay, &str, &[u8])` (temp + rename); `shelf_states(&Path)`.
- `ShelfNames { Pstate, PstateThenLegacy(Provenance) }`; `mirror_shelf(from, names, to)`.
- `convert_global_shelf() -> ShelfConversion`, reading `.shelf-v23.pending`, writing `.shelf-v23` last.
- `clear_legacy_marker(doc: &Path, plugins: &BTreeMap<SeatBay, String>) -> bool` (verified, JSON patch of one key).
- `DisplacedShelf` keyed (SeatBay, bundle), for bay relabels only.

**C11 app-host signals**
- **W3 adds:** `track_ids()`, `row_of(TrackId)`, `take_insert_state_copy_ids()`, `doc_legacy_states()`, `forget_legacy_states()`.
- **W4-lane-refs** keeps every lane-based accessor the host reads (`drum_kit_params_of(lane)` etc.) and adds id forms beside them.
- **W5 adds:** `plugins()`, `bypass()`, `take_bay_relabels()`.
- **W6 deletes:** `take_desk_state_perm`, `send_desk_perm`, `compose_orders`, `Moment.perm`.
- **Rule:** a UI lane never changes a host-facing signature unless it owns the host call site in that wave. It adds the new form beside the old; the host lane that stops using the old form deletes it.

**C12 staging** (W1-freeze-eras; deleted in W8)
- `tangent_core::config::STAGING: bool`; `dir()` honours `TANGENT_CONFIG_DIR`, else `~/.config/tangent-staging` under STAGING, else `~/.config/tangent`.
- `Settings::media_root()` honours `TANGENT_MEDIA_ROOT`, else `<Documents>/Tangent Media Staging` under STAGING.
- `pin_legacy_media_root` is a no-op under STAGING.

---

## 13. Waves and lanes

| Wave | Lanes (file owners in the stage list) |
|---|---|
| 1 | freeze-eras, gate-konami, identity-kernel, rtwork-list, rt-kernels-goldens |
| 2 | engine-lanesets, desk-format, views-windowed, engine-bridge |
| 3 | persist-v23-v8, slot-engine, copy |
| 4 | integrate (all host files), lane-refs (app.rs, drumpad.rs) |
| 5 | identity-maps, engine-memory-inputs |
| 6 | rows-unlimited, engine-buses-sends |
| 7 | buses-ui, scale-and-budgets |
| 8 | gates-and-readiness |

- **Critical path:** gate → views → persist → lane-refs ∥ integrate → maps → rows → buses-ui → gates.
- **Engine path:** rtwork/kernels → lanesets → slot-engine → integrate → memory → buses → scale.

**Lane hygiene:**

- **Worktrees:** cut each one by hand from the merged previous-wave tip, outside Dropbox: `git worktree add ~/Library/Caches/tangent-lane-<name> -b lane/<name> <tip>`. Never use isolation worktree, which cuts from origin/main.
- **Scratch:** each lane gets a private folder, `<scratchpad>/lane-<name>/`.
- **Limits:** at most 5 lanes at once, each at -j 4.
- **Checks:** every lane runs the whole check.sh, clippy -D warnings and the plugin check included.
- **Commits:** at named checkpoints, with lane-tagged messages. Report the branch, the sha and the base's git log -1.
- **Code:** locate code by symbol. Leave MERGE-ME stubs where a contract's other half is missing.
- **Merging:** before merging, run `git merge-tree --write-tree --name-only` and read every new scan test against the other lanes' diffs. Never use `--no-edit` after resolving conflicts.
- **The owner's machine:** never launch Tangent; builds and launches are the owner's. Run no benchmark or cargo job while the owner listens.

The stage list (goal, files, depends_on, parallel_ok_with, done_when per lane) follows.

### W1-freeze-eras

**Goal.**

- Make every historical migration land on the frozen 32-wide shape, whatever the live constants become.
- Add the missing V24 arm (27 sends) with its global-shelf map, the master-lane-by-era rule and the phase-mask lift.
- Make the lifts generic (C3), so one code path yields the lifted desk and its provenance.
- Add the composed legacy global-shelf map.
- Isolate internal builds from the owner's release install (C12).
- Pin today's results, DAW blobs included, in a literal era corpus that check.sh runs.

No behaviour change for any file that loads correctly today, apart from the named defect fixes.

**Files.**

- NEW `tangent-ui/src/desk_lift.rs`.
- `tangent-ui/src/settings.rs`:
  - `adopt_desk` and `LegacyDesk` go through desk_lift with literal targets;
  - `migrate_from`'s was<16 and was<17 grow to the era width;
  - `move_legacy_pad_racks` is frozen and runs on the legacy path only;
  - `Settings::shelf_shuffles()` is added;
  - `media_root` and `pin_legacy_media_root` become staging-aware;
  - the historical-migration ban scan also bans CHANNELS, STRIPS, TOTAL_SEATS, INSERT_SPACE, MASTER_LANE, MASTER_AUTO_LANE and INPUTS.
- `tangent-ui/src/arrangement.rs`: `lift_eras` goes through desk_lift; `from_map`'s master remap goes through `master_auto_lane`; the `read_automation` bound and `HEAD_BAYS` become literals; scans cover these.
- `tangent/src/desktop.rs`: only `shift_insert_states_for_v20` (literal 16) and its scan.
- `tangent-core/src/config.rs`: STAGING, TANGENT_CONFIG_DIR, and the staging-aware directory test.
- NEW `tangent-ui/src/era_corpus.rs` (cfg(test), DAW blob round trips included).
- NEW `tests/fixtures/eras/**`.
- `tangent-ui/src/lib.rs`: mod lines.

**Depends on.** None (main d821bb3).

**Parallel with.**

- W1-gate-konami. The desktop.rs fence: this lane edits only `shift_insert_states_for_v20` and its scan; that lane edits only test-module `TangentApp::new` sites.
- W1-identity-kernel, W1-rtwork-list, W1-rt-kernels-goldens.

**Done when.**

- check.sh is green, clippy included.
- **Corpus:** every fixture loads to LITERAL expected values: dense V32 arrays, seat numbers, every mask bit including phase, and the master curve lane. Fixtures:
  - settings v12, v18, v19 (with a phase-inverted drum bus and master), v20 at 27 sends, v20, v21, v22;
  - documents v4 (11 sends, the 84-bay sub-era), v5 (18 and 19 sends), v6 (27 and 35), v7, with master curves at 16, 24 and 32 and with tracks_added and armed;
  - DAW blobs 3.x, 4.x and 5.x.
- **New tests, red at the base:**
  - a 27-sends file lifts its buses, master and master curves;
  - a 16-era phase-inverted drum bus lands on strip 32.
- **Provenance:** `provenance_of_document` maps every lifted bay back to its legacy bay for every era fixture.
- **Shelf map:** `legacy_shelf_seat` equals running the frozen in-place shuffles (v13, v14, v18, v19, v20, and v24 = +8 over 24..=3100) on a synthetic shelf holding every seat, for every shuffle set an era can raise.
- **Staging:**
  - with no overrides, `config::dir()` is `~/.config/tangent-staging`, `media_root` is `<Documents>/Tangent Media Staging`, and `pin_legacy_media_root` pins nothing;
  - the env overrides win;
  - a scan finds no other code that spells the real config or media path.
- **Constant independence:** a local, uncommitted change of CHANNELS to 40 leaves the era tests green (recorded).
- **Ban scans:** each fails on a planted constant (revert check recorded).

### W1-gate-konami

**Goal.**

- One creation gate that reads `is_supporter()` (C5).
- The free desk becomes 32 now. Close the `open_lane_below`/`add_bare_track` leak. Ctrl/Cmd+T at the cap says why.
- Deterministic tiers in both crates, through `with_license` and temp stores.
- Default zoom, palette period and inputs stop deriving from tier constants.
- Routes follow reorder and delete.
- About knows the standing.
- The plugin check and the census join the gate.

**Files.**

- `recorder.rs`:
  - FREE_TRACKS = 32, replacing FREE_CHANNELS;
  - `track_cap`, DEFAULT_MAX_STRIPS = 16, PALETTE_SLOTS = 32;
  - INPUTS = 32 as a literal, documented as MAX_PICKS's twin;
  - delete the dead `active_channels(bool)`;
  - rewrite the CHANNELS doc to the reversed principle.
- `app.rs`:
  - `TrackRefusal`; `with_license`, with `new = with_license(load())`;
  - `track_cap`, `counted_tracks`, and `may_add_tracks` on every road in section 5, with the refusal lines;
  - `desk_extent` replaces `active_channels`;
  - cfg(test) `force_free_cap` and the undo sentinel;
  - `heart_wave` and `heart_palette_slot` over PALETTE_SLOTS; `default_mixer_visible` on DEFAULT_MAX_STRIPS;
  - `reorder_channels` and `remove_channel_inner` call `Routes::remap_channels` and `drop_channel`;
  - `MenuAction::ShowAbout` builds `Dialog::About { supporter }`;
  - `headless` and `headless_with` use a free temp store; the tier fixtures and tests.
- `routing.rs`: `Routes::remap_channels`, `Routes::drop_channel`.
- `mixer_panel.rs`: `Palette::slot % PALETTE_SLOTS`; the free-desk test re-pinned to 32.
- `dialogs.rs`: the palette swatch row and the `Dialog::About { supporter }` field only.
- `recorder_panel.rs`: the decorative empty field fixed at 8 rows.
- `tangent/src/composite.rs` and the test-module `TangentApp::new` sites in `tangent/src/desktop.rs`: `with_license` on a free temp store.
- `scripts/check.sh`: the plugin check and the census.
- NEW `scripts/test-census.sh`.

**Depends on.** None.

**Parallel with.** All wave-1 lanes.

**Done when.**

- check.sh is green, plugin check included.
- **Determinism:** the suite gives identical results with HOME pointed at an empty temp dir (CARGO_HOME and RUSTUP_HOME pinned) and with the owner's real license.key present.
- **Real config:** no test writes or deletes a real `cheat.unlock`.
- **Scans:**
  - no test in either crate calls `TangentApp::new`;
  - the gate reads `is_supporter()` and never `licensed()`;
  - the fixture key is `include_str!`'d only under cfg(test).
- **Forced cap:** with `force_free_cap(4)`, a free app refuses the 5th counted track on every road with the line, changes nothing and leaves no undo moment. Supporter and cheater stores pass the same walk to 32, then get Ceiling.
- **Konami:**
  - typed through the key feed on a free app, it lifts the forced cap in the same session;
  - a buyer toggling Konami off stays a supporter;
  - a dir holding only `cheat.unlock` reloads as a supporter;
  - Reset Everything relocks a keyless cheater: `cheated()` is false and `project_request == NewKeeping`.
- **Values:** `track_cap(true) == None` and `track_cap(false) == Some(32)`.
- **Zoom:** a full screen still paints DEFAULT_MAX_STRIPS strips.
- **Routes:** a reorder carries routes at both ends; a delete drops routes into and out of the row.

### W1-identity-kernel

**Goal.** Land C1 and C2 as pure code with no callers, and prove them against today's live seat functions while those still exist.

**Files.**

- NEW `tangent-ui/src/desk_id.rs`.
- NEW `tangent-ui/src/desk_legacy.rs`.
- `lib.rs`: mod lines.

**Depends on.** None.

**Parallel with.** All wave-1 lanes.

**Done when.**

- The suite is green.
- **Bijection:** `decode_v32`/`encode_v32` is a bijection over all 13,176 V32 bays, and agrees with `recorder::seat_bay`, `pad_rack_index`, `folder_extra_seat`, `amp_pedal_seat`, `bus_extra_seat`, `rack_owner` and `pedal_owner` for every input. These cross-checks are deleted in W8.
- **Display slots:** `slot_key` reproduces `seat_bay` for every (desk index, slot), the master's extra-first order included, and `display_slot` inverts it.
- **Text:** round-trips (property test). Strict rejection of leading zeros, upper case, out-of-range pedal/pad/bay, a pedal or pad on a non-track node, and an unknown owner.
- **Isolation:** no existing file changes except lib.rs.

### W1-rtwork-list

**Goal.** `LanePool` claims from a caller-owned lane list (C6):

- the claim word is `round << 32 | cursor`;
- batches are guided;
- `done` is a count, and `MarkDone` covers its whole batch;
- hand indices are stable.

`run(u32)` is re-implemented on `run_list`, so no caller changes.

**Files.** `tangent/src/rtwork.rs` only.

**Depends on.** None.

**Parallel with.** All wave-1 lanes.

**Done when.**

- The suite is green, and the existing rtwork tests pass unchanged through `run(u32)`.
- **New tests:**
  - every lane is claimed exactly once at 33, 1000 and 4096 lanes under contention;
  - a stale worker with a non-zero cursor claims nothing;
  - a panic mid-batch still releases the barrier;
  - hand indices are distinct and stable.
- The three fork bit-exact tests in rt_bench.rs are green.
- **Bench (measured and reported, never asserted):** rt_bench at 128 frames stays within 5% on 16x[EQ, comp, plate] (about 66 us mean, 104 us p99) and on 32x3 EQ (about 57 us).

### W1-rt-kernels-goldens

**Goal.**

- Measure before anything moves: render goldens, a counting allocator, and an alias harness over today's Engine API.
- Land the real-time building blocks unwired (C7):
  - LaneSet;
  - DeskPlan with `build_plan` (route order and levels) and `resolve_heard`;
  - the op/spine protocol, with Attach, Monitor, Capture, plan validation and the claim stamp.

**Files.**

- NEW `tangent/src/render_golden.rs` (cfg(test), about 24 desks of built-ins, FNV over f32 bits, cfg_attr ignore off aarch64 macOS, `TANGENT_GOLDEN_WRITE=1` regenerates).
- NEW `tests/fixtures/renders/golden.json`.
- NEW `tangent/src/rt_alloc.rs` (cfg(test) `#[global_allocator]`, ported verbatim from `tangent-host/src/instance.rs`):
  - a thread-local arm that also covers `tangent-lane-*` workers;
  - `count_allocs` and a module mutex;
  - a baseline around `render_offline` at 32 lanes, including a monitor ring and a layout arriving;
  - an alias harness at lanes 0, 1, 30 and 31 through today's setters.
- NEW `tangent/src/laneset.rs`, `desk_plan.rs`, `lane_bank.rs`.
- `tangent/src/main.rs`: mod lines.
- No other file.

**Depends on.** None (the goldens capture main d821bb3).

**Parallel with.** All wave-1 lanes.

**Done when.**

- The suite is green.
- **Goldens recorded for:**
  - folders nested 3 deep, a route chain, a route cycle, a hardware route;
  - sends through Return 0's built-ins, and a drum lane with pad FX;
  - DX7, Keys and the sampler;
  - automation on gain, pan, send, mute, the master and a built-in parameter;
  - a latency unit (PDC);
  - solo inside a folder, phase, a frozen lane;
  - wet+dry stems and a sidechain key;
  - grains on and off, fork on and off.
- **Allocation baseline:** recorded as zero, or with each pre-existing site named KNOWN together with the lane that removes it (the monitor-ring drop in Renderer's `pending_monitor` handoff is one). A test asserts the count never grows.
- **`resolve_heard`:** equals frozen copies of `channel_is_heard`, `strip_is_heard` and `muted_now` over random flag soups up to 63 lanes, and equals a `Vec<bool>` reference at 300.
- **`build_plan`:**
  - its route order equals a transcription of today's Kahn walk on 10,000 random graphs with cycles and zero gains;
  - its levels equal a reference topological layering, with cycle members in a final level in today's order.
- **`apply_ops`:**
  - allocates and frees nothing at capacities 33, 65, 200 and 1000;
  - leaves ops queued when the retire ring is full;
  - refuses a plan naming a slot twice, an empty slot or a stale gen (retired whole, counted, previous plan kept);
  - installs or retires Attach under the room-first rule;
  - a second claim of a lane in one round answers None.

### W2-engine-lanesets

**Goal.**

- Every u32/u64 lane mask in the Renderer and Shared walks becomes a LaneSet at capacity 32 or a per-lane flag.
- Every `.min(31)`, `.min(63)` and `min(CHANNELS - 1)` lane clamp goes in ONE change, behind a ban scan.
- `mix_channels` dispatches through `run_list`.
- The public Engine API is unchanged, and audio stays bit-identical.

**Files.**

- `tangent/src/instrument.rs`:
  - masks: `param_lanes`, `curve_lanes`, `clip_lanes`, `clip_under`, `board_ran`, `board_live`, `leased`, `room_rack`, the probe pins, `wanted`, `folded`, `receives`;
  - also `pinned_lanes`, `fill_automation_params`, `note_eligibility`, `plan_grains`' free mask, `claim_board`/`run_pedalboard`, `first_drum_lane`, `seat_is_frozen`;
  - the desk words are decoded once per chunk;
  - the ban test;
  - `auto_key == param_slot` for every lane and bay.
- `tangent/src/rt_bench.rs`: the mask setters.
- `tangent/src/laneset.rs`: fixes only.

**Depends on.** W1-rtwork-list, W1-rt-kernels-goldens, W1-identity-kernel.

**Parallel with.**

- W2-desk-format, W2-views-windowed.
- W2-engine-bridge, which calls only unchanged pub setters.

**Done when.**

- Goldens are bit-exact.
- **Green:**
  - both versions of `the_fork_renders_what_the_single_walk_rendered`;
  - `the_fork_and_the_grain_are_one_sound`;
  - `a_grained_lane_renders_what_the_chunked_lane_rendered`;
  - the alias harness.
- **Ban scan:** passes, and fails on a planted `.min(31)` (revert check recorded).
- **API:** no pub fn signature changed.
- `the_masters_suspend_bit_is_not_channel_31s` is kept.
- check.sh is green.

### W2-desk-format

**Goal.** Land DeskModel and the one format-2 reader/writer (C4) as pure, unwired code:

- the error policy: referential entries are dropped and listed, and structural problems refuse the desk only, never the file;
- the `legacy_states` field;
- validation.

Golden-test it over the wave-1 corpus.

**Files.**

- NEW `tangent-ui/src/desk_model.rs`.
- NEW `tangent-ui/src/desk_format.rs`.
- `lib.rs`: mod lines.

**Depends on.** W1-freeze-eras, W1-identity-kernel.

**Parallel with.** W2-engine-lanesets, W2-views-windowed, W2-engine-bridge.

**Done when.**

- The suite is green.
- **Round trips:**
  - every era fixture, run through the legacy chain then `from_v22(identity_ids)`, equals a literal DeskModel;
  - `write(read(x))` is byte-identical;
  - for every 32-wide fixture, `from_v22 -> write -> read -> to_v22` returns the dense input;
  - an 8-row desk with 3 buses is never classified by sends length.
- **Error policy:**
  - a desk with one dangling plugin, bypass, route, key and send reads with exactly those five dropped and listed;
  - a `next_id` behind the ids is repaired;
  - 65,537 rows, duplicate ids and a malformed desk return DeskRefused carrying the raw JSON;
  - a chain ≥ MAX_CHAIN is a dropped entry;
  - huge numeric keys allocate nothing.
- **Guards:** exhaustive-destructure tests over TrackRow and DeskModel guard `append`, `insert_rows`, `remove_rows` and `permute`. `DeskModel::check` finds every dangling reference.
- **Row classes:** RowClass matches section 4 on synthetic rows, a pedal-only row included.

### W2-views-windowed

**Goal.**

- Remove identity-by-index arithmetic from the views.
- Make view build, layout, hit test and a11y O(visible), at today's fixed 32 slots.
- The only visible changes are scrollbars and filterable, name-first pickers.

**Files.**

- `recorder.rs`: the Column enum; `StripRef`; `Strip::shown` is no longer rebuilt per `Column::strip` call.
- `app.rs`:
  - StripRef accessors become the only home of strip-index math;
  - every `Column(..)` site;
  - window-only `mixer_view` and `arrange_view` builders;
  - `arr_clip_views` culled, and `meter_hold` borrowed;
  - hit dispatch;
  - scroll and zoom clamps on VISIBLE_MAX = 64, and COUNT_LADDER gains 48 and 64;
  - a11y labels by name.
- NEW `tangent-ui/src/desk_index.rs`.
- `mixer_panel.rs`: Layout over a window Vec plus the pinned click and master and the totals; a scrollbar.
- `arrangement_panel.rs`: `lane_rows` and `header_rows` over the window, with a rank lookup replacing `from_fn(lane_of)`; `LaneId::{Row, Master}`; a scrollbar.
- `recorder_panel.rs`.
- `dialogs.rs`: route and key pickers, type-to-filter and name-first.
- `perf_fixtures.rs`: XXL 128 and XXXL 512 rungs on a synthetic DeskIndex.
- `lib.rs`.

Host-facing TangentApp signatures are unchanged.

**Depends on.** W1-gate-konami.

**Parallel with.** W2-engine-lanesets, W2-desk-format, W2-engine-bridge.

**Done when.**

- check.sh is green.
- **Scan:** none of these in tangent-ui view code outside the StripRef accessors: `CHANNELS +`, STRIPS, MASTER_LANE, MASTER_COLUMN, and `i >= CHANNELS` / `i < CHANNELS` column tests.
- **Owner-ruling pins unchanged:** `the_desk_never_folds_into_two_rows`, `automation_rows_shrink_the_tracks_then_evict_one_and_repeat`, `the_owners_screen_opens_a_dozen_lanes_and_ten_at_seventy_five`.
- **Bounds:** on a 512-row synthetic index, view constructions, layout rows, hit candidates and a11y nodes are bounded by visible + pinned + 2.
- **Invalidation:** DeskIndex invalidation is tested for every mutation kind, undo and redo included.
- **Repaint:** a headless test over 30 idle frames finds the same repaint causes (the existing `causes(&ctx)` helper) before and after. This replaces the PACE probe, which needs a running binary.
- **Re-pins:** the column/strip aliasing tests are re-pinned on the enums.

### W2-engine-bridge

**Goal.**

- One testable seam between app and engine (C9): move the desk-structure pushes into `Bridge::sync`, with no behaviour change.
- Extract today's offline furnishing (export and probe) into `furnish_offline`, so it runs without DesktopApp.
- Commit export goldens, so W4 has a baseline.

**Files.**

- NEW `tangent/src/engine_bridge.rs`:
  - `Bridge::new`;
  - `Bridge::sync`, taking over `PushedDesk`/`desk_unchanged` and `set_desk`, `set_armed`, `set_frozen`, `set_midi_off`, `set_drum_strips`, `set_builtin_strips`, `set_keys_strips`, `set_auto_suspend`, `set_strip_phase`, `set_folders`, `set_punches`, `set_midi_routes`, `publish_routes`, and the key and bypass pushes;
  - `furnish_offline(app, rate, timebase, BuildFor)`.
- `tangent/src/desktop.rs`: those call sites; `start_offline_render` and `furnish_probe_engine` call `furnish_offline`; `PushedDesk` moves out.
- `tangent/src/main.rs`: mod line.
- NEW `tests/fixtures/exports/golden.json`.

**Depends on.** W1-gate-konami (with_license for the harness).

**Parallel with.** W2-engine-lanesets, W2-desk-format, W2-views-windowed.

**Done when.**

- check.sh is green.
- **Harness:** a tangent-crate test builds `TangentApp::with_license(free)`, `Engine::offline` and Bridge. It scripts arm, mute, solo, phase, folder, route, key and bypass edits, syncs, and reads back engine state identical to a golden of what DesktopApp pushed at the base commit.
- **Export goldens:** committed for the corpus desks (built-ins only, aarch64 macOS): master, stems and dry stems as WAV bytes (FNV) plus file names, through `furnish_offline` + `render_offline` + the WAV writer.
- **Scans:**
  - `start_offline_render` and `furnish_probe_engine` both call `furnish_offline`, with their arm and MIDI difference as a BuildFor arm;
  - desktop.rs makes none of the moved set_* calls directly.

### W3-persist-v23-v8

**Goal.** settings.json and project.tan/.tangent/templates read both shapes and can write format 2 behind the one switch `WRITE_V8` (still false). Every format-2 side effect reads that switch:

- the stamps;
- backups on the first format-2 write;
- the global-shelf pending marker;
- drumpad.json id keys;
- the documents' `legacy_states` marker.

Also:

- Desk errors never cost a file.
- TrackIds live in memory: permuted by reorders, restored by undo, minted on add, paste and make_folder, fresh on a reused slot.
- Master automation is persisted as a target.
- Routes are never truncated on read.
- Id-keyed host signals (C11).

**Files.**

- `settings.rs`:
  - `from_map` dispatches on `desk.format`;
  - `to_map` goes through desk_format when WRITE_V8;
  - `SETTINGS_VERSION` is derived from WRITE_V8;
  - legacy keys are consumed with shift_remove;
  - `track_ids`, `next_track_id`;
  - a pre-write hook writes `settings.pre-v23.json` and `.shelf-v23.pending` before the first format-2 write;
  - the desk refusal policy (default desk, raw desk to `settings.desk-refused.json`, a notice, never `unreadable`), and `from_json` keeps a refused desk under `extra.desk_refused` (that half was removed on 2026-10-10 with the plugin crate; see 6.3).
- `arrangement.rs`:
  - DeskSnap ids, `next_id`, `Says`, `legacy_states`;
  - `to_value` and `from_value` go through desk_format;
  - `ARRANGEMENT_VERSION` derived;
  - `read_automation` targets master;
  - `DeskRef` and `hash_into` id terms APPENDED;
  - `<doc>.pre-v8` before the first format-2 write over a legacy document;
  - a structurally refused desk refuses the open and writes nothing.
- `automation.rs`: the persisted target key only.
- `drumpad.rs`: reader and WRITE_V8-gated writer per section 6.8.
- `routing.rs`: `Routes::from_json` reads every route, dedup kept; `add()` keeps its cap until W7.
- `app.rs`:
  - ids flow through `reorder_channels`, `add_bare_track`, `paste_track`, `copy_track` (`TrackCopy.from_id`), `make_folder`, `remove_channel_inner`, `apply_desk_snapshot`, `desk_view` and `Moment`;
  - `track_ids`, `row_of`, `take_insert_state_copy_ids`, `doc_legacy_states`, `forget_legacy_states`;
  - `seed_template`'s named heal patches only the `named` key in the file's JSON;
  - `holds_only_scratch` accepts `<doc>.pre-v8`.
- `desk_format.rs` and `desk_model.rs`: adapters.
- `tests/fixtures/eras`: v23 and v8 expectations, DAW blob v23 round trips.

**Depends on.** W2-desk-format, W2-views-windowed, W1-gate-konami, W1-freeze-eras.

**Parallel with.** W3-slot-engine, W3-copy.

**Done when.**

- check.sh is green, plugin check included.
- **Production unchanged:** production writes (WRITE_V8 false) are byte-identical to the base for every fixture, stamps included. Every legacy fixture opens to the same live desk as at the base (golden).
- **With the format-2 writers forced on inside tests:**
  - every fixture saves as v23/v8 and reopens equal (fixed point);
  - `settings.pre-v23.json` and `<doc>.pre-v8` are byte-equal to the originals and written once;
  - `.shelf-v23.pending` names the shuffle set and exists before the settings file changes;
  - a second format-2 write makes no backup.
- **Error policy:**
  - a v23 settings file, a v8 document and a DAW blob, each with one dangling plugin, bypass, route, key and send, open with those entries dropped, a notice, and everything else byte-identical;
  - a structurally refused desk keeps every other setting (settings), refuses the open and leaves the file untouched (document), and keeps the raw desk across a save (DAW blob; that case was removed on 2026-10-10 with the plugin crate).
- **Ids:**
  - reorder, undo and redo keep every row's id;
  - add, paste and make_folder mint fresh ids;
  - a reused cleared slot gets a fresh id.
- **drumpad.json (writers forced):** reorder, edit a pad without saving the document, save, reopen: every kit is on its own track. A file with `track_kits` ignores a stray top-level kit and `primary_lane`.
- **seed_template:** on a named legacy template, it changes exactly the `named` key (byte diff) and keeps the format.
- **Folders untouched:** opening any fixture project modifies no file in its folder (hash before and after).
- **Routes:** a file with 65 routes reads all 65.
- **Master curve:** a v6 27-sends document's master curve at lane 24 reads as the master.

### W3-slot-engine

**Goal.** Rebuild the renderer around `Box<Lane>` in a slot spine, with in-band ops (Grow, Install, Remove, Attach, Plan, Monitor, Capture) and one validated DeskPlan per block (C7, C8), under the ownership rules of section 7:

- lanes own only what their own hand touches;
- the key snapshot, stems and dry stems live in the spine and are written serially;
- shared lane and rack state is atomics only;
- automation queues are audio-owned;
- pads attach lazily;
- desk-wide schedule lists are validated at use;
- the monitor ring and its layout arrive through rings;
- the grain phase is keyed by slot;
- legacy desks install row r at `Slot(r)`.

The slot API sits beside today's pub API; today's API becomes shims onto slots 0..=31.

Two checkpoints, each green:

- (A) lane-major at a fixed 32, bit-exact;
- (B) slots, ops and plans.

**Files.**

- `tangent/src/instrument.rs`, `rt_bench.rs`, `laneset.rs`, `desk_plan.rs`, `lane_bank.rs`.
- No host file.

**Depends on.** W2-engine-lanesets, W2-engine-bridge (export goldens).

**Parallel with.** W3-persist-v23-v8, W3-copy.

**Done when.**

- check.sh is green. Every existing engine test passes through the shims. Render goldens are bit-exact and export goldens byte-identical at both checkpoints.
- **Aliasing:** alias tests at 100 and 300 lanes (31, 32, 33, 63, 64, 65, 255, 256, 257 against 0, 1, 31) cover arm, frozen, midi_off, mute, solo, phase, suspend, drum/builtin/keys, sends, routes, keys, leased, punch, clips and curves.
- **Keys across hands:** a keyed lane whose source renders on another hand matches the serial render bit for bit at 33 and 257 lanes. `key_prev` is not a Lane field (scan).
- **`rt_alloc`:** zero allocations and frees in the callback across render, Install, Remove, Grow, Attach, Plan, and monitor ring and layout arrival, at 33, 65, 200 and 1000 lanes. The KNOWN list is empty.
- **Becomes a drum lane:** a lane with chain and pedal plugins loaded becomes a drum lane mid-roll with 0 reloads and 0 callback allocations.
- **Delayed Install:** a clip-bearing lane whose Install waits a block for retire room is audible within 2 blocks of the Install landing. No clip, MIDI or automation list is dropped on arrival.
- **Bad plans:** a plan naming a slot twice is refused while the previous plan keeps playing. A forced second claim of a lane renders it once.
- **Rendering:**
  - fork equals serial at 33, 100 and 257 lanes;
  - N silent lanes change no bit;
  - a reorder is one Plan and reloads nothing;
  - a generation mismatch renders silence;
  - MIDI_OFF edges release held notes at the plan swap.
- **Gone:** LaneCells, the jobs array, `lane_seats`, the split_at_mut carves.
- **Test plumbing:** `Renderer::for_tests(capacity)` replaces `test_renderer`. `stack_budget` holds with Shared smaller, and a `size_of::<Lane>()` ceiling replaces `RACK_SEATS > 1000`.
- **Memory:** an idle non-drum lane allocates at most 256 KB at build (counted).

### W3-copy

**Goal.** Say truthfully what a key does, everywhere the product says it, with the default Konami wording ("a certain code", owner call 9). Catch every stale phrase by scan.

**Files.**

- `tangent-ui/src/dialogs.rs`:
  - `about_lines(caps, supporter)`, with Minimal keeping the chord-display line (the plugin too, until its crate was removed on 2026-10-10);
  - the Supporter Key dialog strings;
  - the about tests re-pinned for free and supporter.
- `README.md`.
- `docs/STORE-CONTENT.md`.
- NEW `docs/release/<next>/GUMROAD.md`. The 4.2 and 4.4 pages stay as history.
- `tools/tangent-fulfil/src/main.rs`: `email_body`.
- `CHANGELOG.md`: [Unreleased].
- NEW phrase-scan test in tangent-ui, or `scripts/copy-scan.sh` run by check.sh.

**Depends on.** W1-gate-konami (`Dialog::About { supporter }`), W2-views-windowed (dialogs.rs base).

**Parallel with.** W3-persist-v23-v8, W3-slot-engine.

**Done when.**

- **Every changed text says:**
  - 32 tracks free;
  - unlimited with a supporter key (or a certain code);
  - buses, returns, sends and routes unlimited for everyone;
  - keys already sold included.
- **Phrase scan** over README.md, docs/STORE-CONTENT.md, tools/tangent-fulfil/src and tangent-ui/src finds none of:
  - "thank-you, nothing more", "thank-you, not an unlock", "not an unlock";
  - "nothing else" or "nothing more" in a sentence naming the key;
  - "stays free" without the track limit beside it;
  - "Thirty-two channels", "Thirty-two tracks", "-channel mixer", "{}-channel".

  Each phrase fails the scan when planted (revert check recorded).
- No em dash in changed files.
- tools/tangent-fulfil builds in its own workspace.
- check.sh is green.

### W4-integrate

**Goal.** The host drives the engine only through Bridge, plugin state is named by identity everywhere, and the engine stops reading tangent-ui capacity constants. Three checkpoints, each committed green.

**(A) Bridge as broker, keeping legacy file names.**

- `TrackId -> Slot`, with legacy rows at `Slot(r)`.
- Install and Remove by id diff. Two-phase removal: a returned lane has every bay's state written to the global shelf before it drops, and an id that reappears waits for its old lane.
- Plans built on change; LaneShared writes.
- Desk-wide clip, MIDI and automation lists with `(slot, gen)`.
- Kits and pads through Attach.
- Plugin reconcile by SeatBay, decoded from flat seats (`decode_v32` + `track_ids`); one HostRack per bay.
- `full_build` (grown from W2's `furnish_offline`) serves export, probe and freeze.
- FreezeJob is bound to a TrackId and lands through `row_of`.

**(B) `shelf.rs` (C10) and the switch.**

- pstate names.
- `convert_global_shelf`:
  - copies through `legacy_shelf_seat`;
  - its source set comes from `.shelf-v23.pending`, else from this launch's `Settings::shelf_shuffles()`;
  - every copy is temp + rename, copy-if-absent;
  - `.shelf-v23` is written last;
  - it runs before `open_project_at_launch`.
- The in-place v13..v20 shuffles are deleted.
- Project and template `state/` are read pstate-then-legacy through `legacy_states` (provenance + `encode_v32`), with the owner check on every read.
- After a save road's mirror, `clear_legacy_marker` runs only when every plugin in the on-disk document has a pstate file, and patches only that key.
- `build_template`'s copy skips `state/insert-state-*` and `*.pre-v8`.
- `mirror_shelf` writes are temp + rename.
- `WRITE_V8 = true`.

**(C) Deletions and detachment.**

- Deleted: the reorder carry, `Engine::permute_channel_inserts`, `permute_loaded_rows`, `permute_insert_states_in`, `permute_displaced`, reload-on-reorder.
- Today's Engine pub API becomes cfg(test) shims on an engine-local `LEGACY_LANES = 32`.
- `run(u32)` is deleted.
- Every `tangent_ui::recorder` capacity constant use in `tangent/src` is replaced: host loops read `desk_extent()` or the Bridge, and `midi.rs` reads `desk_id::PADS`.

**Files.**

- `tangent/src/engine_bridge.rs`.
- NEW `tangent/src/shelf.rs`.
- `tangent/src/desktop.rs`:
  - the reconcile loop, the reorder arm and `start_engine`;
  - export, probe and freeze furnishing, including `furnish_probe_engine`;
  - `reload_clips`, and the automation and kit pushes;
  - the sidecar callers: `copy_states_to_project`, `copy_states_from_project`/`_shelf`, `save_project_and_states`, `build_template`, `swap_in_template`, `sampler_media_on_shelf`, the scratch sweeps;
  - FreezeJob;
  - TANGENT_BYPASS/TANGENT_SKIP seatbay syntax;
  - the `DesktopApp::new` launch order.
- `tangent/src/instrument.rs`: shims, `LEGACY_LANES`, `MASTER_AUTO_LANE` off the live path.
- `tangent/src/rtwork.rs`: `run(u32)` deleted.
- `tangent/src/rt_bench.rs`.
- `tangent/src/midi.rs` and `devices.rs`: constant reads only.
- `tangent-ui/src/desk_format.rs`: the WRITE_V8 line only.
- `tangent-host/src/ready.rs`: doc examples.

**Depends on.** W2-engine-bridge, W3-persist-v23-v8, W3-slot-engine, W1-freeze-eras.

**Parallel with.** W4-lane-refs. This lane edits no tangent-ui file except the WRITE_V8 line, and reads only app APIs that exist at W3 plus today's lane-based accessors.

**Done when.**

- check.sh is green at each checkpoint.
- **Goldens:** through Bridge, render goldens are bit-exact, and `full_build` exports are byte-identical to the committed W2 export goldens.
- **Reorder:** reordering all 32 loaded lanes gives 0 reloads, 0 engine round trips and 1 Plan, and after (B) 0 sidecar filesystem calls.
- **Era projects:** for each era fixture project (V8, V8 16-pad, V16NoDrumBus, V16, V24, V32), every plugin's state reaches its own bay after open, with the owner check passing and bytes equal. This covers buses, master, master-extra, folder-extra, pedals and pads.
- **Save-road crashes:** for each save road (`save_project_and_states`, `switch_project`, the New arm of `run_project_request`, `on_exit`, SaveChangesSave, `save_project_first`, `save_project_as`, `seed_template`'s heal), a kill between the document write and the mirror reopens with every state. The marker clears only after a verified mirror, and its patch changes only that key.
- **`convert_global_shelf`:**
  - a kill after the settings save and before conversion, mid-copy, or after some copies relaunches with every state on its V32 seat;
  - it is idempotent;
  - it never re-runs once `.shelf-v23` exists, so a deleted pstate never returns;
  - no new-build path renames or deletes a legacy sidecar (scan plus filesystem audit).
- **Stray files:** a format-2 project without `legacy_states` ignores stray `insert-state-*` files.
- **Folders untouched:** project and template folders are byte-identical after open, mirror and save, apart from pstate files and the documented marker patch.
- **Removal:** a removed lane's states are on the global shelf before its plugins drop, and undo restores the row with every state.
- **Freeze:** a freeze bound to a TrackId lands on that track after a reorder mid-render.
- **Scans:**
  - no seat arithmetic and no u32/u64 lane masks in desktop.rs or engine_bridge.rs;
  - none of `tangent_ui::recorder::{CHANNELS, STRIPS, TOTAL_SEATS, INSERT_SPACE, INPUTS, MASTER_SEAT, MASTER_EXTRA_SEAT, FOLDER_EXTRA_SEATS, AMP_PEDAL_SEATS, BUS_EXTRA_SEATS, DRUM_PADS}` in `tangent/src`.
- **Harness:** a test drives `TangentApp::with_license` + Bridge into an offline engine and hears every lane.

### W4-lane-refs

**Goal.**

- Session state that names a track names it by TrackId, so no structural edit can leave it on the wrong track.
- A field-classification scan keeps it that way.
- `drumpad_kits` in memory keyed by TrackId.

**Files.**

- `tangent-ui/src/app.rs`:
  - `arr_auto_open`/`armed`/`bypassed` (`HashSet<(TrackId, AutoParam)>`), `arr_auto_focus`, `arr_auto_modes`, `arr_auto_latched`, `arr_auto_last`, `arr_auto_sel`, `arr_auto_hot`, `arr_auto_nodes`;
  - `auto_clipboard_from`, `auto_clipboard_rows`;
  - `selected_channel`, `selected_tracks`, `sel_anchor`;
  - `drumpad_lane` (`Option<TrackId>`);
  - `track_clipboard` (`from_id`);
  - `Moment.rows_closed`/`ClosedRows`;
  - `on_desk_shape_change()`, called by every structural edit;
  - `reorder_channels` and `carry_automation_rows` lose the remaps they no longer need;
  - lane-based host accessors keep answering (C11);
  - the classification scan.
- `tangent-ui/src/drumpad.rs`: kits as `BTreeMap<TrackId, DrumpadDoc>`, with the writer keyed as W3 defined.

**Depends on.** W3-persist-v23-v8.

**Parallel with.** W4-integrate.

**Done when.**

- check.sh is green.
- **Follows its track:** reorder, insert-below, undo and redo keep these on their own tracks:
  - the drum panel;
  - every automation row state: open, armed, bypassed, mode, latch, last value, focus, clipboard aim;
  - the selection.
- `a_lanes_automation_mode_follows_its_track` passes unchanged.
- **Classification scan:** every TangentApp field whose type carries a lane or seat index is classified as identity-keyed, cleared by `on_desk_shape_change`, or not-a-lane with a reason. A planted unclassified field fails it (revert check recorded).
- **Signatures:** no host-facing signature changed.

### W5-identity-maps

**Goal.** The app holds identity, not positions:

- the specials leave strip space;
- plugins become `BTreeMap<SeatBay, String>`, and bypass `BTreeSet<SeatBay>`;
- keys become `{seat, from: Node}`;
- routes become `{from: Node, to: Node | Hardware}`;
- sends are kept per row and per special;
- every seat-keyed session field is keyed by SeatBay;
- `slot_key` is the only bay resolver in the UI;
- bay moves are emitted as SeatBay relabels;
- the bridge reads the maps directly.

**Files.**

- `recorder.rs`: the seat-map consts and fns retire to desk_legacy; the Strip/Desk pair feeding the bridge is replaced.
- `settings.rs`.
- `arrangement.rs`.
- `routing.rs`: `Target::Node`; Routes and KeyEdges by identity; labels by name; `remap_channels` deleted.
- `app.rs`:
  - `insert`, `set_insert`, `copy_fx`/`paste_fx`, `trade_inserts`, `move`/`swap_inserts`, `swap_pads`;
  - `slot_seat`, `auto_address_of`, `master_bay`;
  - `seed_amp_board`, `open_face_seats`, `pedal_stack_covers`, `settle_bay_curves`;
  - `routing_dialog`, `key_picker_dialog`;
  - StripRef accessors over Specials;
  - `plugins`, `bypass`, `take_bay_relabels`;
  - the seat-keyed session fields: `fx_params`, `builtin_faces`, `builtin_faces_live`, `pending_inserts`, `insert_gr`, `sampler_info`, `sampler_load`, `preset_sample`, `fx_clipboard`, `insert_state_copy`, `open_insert_on_load`, `picker_insert`, `bay_carry`, `arr_fx_menu`, `patch_audition`, `pedal_stack`;
  - the classification scan extended to seat types.
- `dialogs.rs`: the InsertFace key.
- `faces*.rs`.
- `mixer_panel.rs`.
- `tangent/src/engine_bridge.rs`: `Bridge::read_desk` and the plugin reconcile input only.

**Depends on.** W4-integrate, W4-lane-refs.

**Parallel with.** W5-engine-memory-inputs. The engine_bridge.rs fence: `read_desk` and the plugin input side here, `sync_inputs` there.

**Done when.**

- check.sh is green. Render, export and era goldens are unchanged.
- **Identity survives edits:**
  - a route and a key survive drag reorder, insert-below and delete exactly;
  - a bypass follows its plugin across reorders with no remap code;
  - pad FX follow their lane through a reorder and its undo (fixes `seat_order` leaving pad seats behind);
  - a pad-FX swap carries both instances and both pstate files;
  - an open face, a pending insert, a sampler load and a copied FX stay aimed at their own bay across a reorder.
- **Grep:** no seat arithmetic and no `CHANNELS + k` in tangent-ui outside desk_legacy.
- **Deleted:** `Routes::remap_channels` and `KeyEdges::remap_channels`.
- **Scan:** the classification scan passes with seat types included.

### W5-engine-memory-inputs

**Goal.** Every audio track can bind, arm, meter and record any device input:

- Picks are as many as the bound tracks, per the owner's ruling on MAX_PICKS.
- Picks are keyed by TrackId (a stable PickId), so no structural edit moves an input.
- The arrangement record is bound by identity.
- Capture rings are per armed pick, travel on the op ring, and are never replaced while capturing.
- The stem ring is built at arm time.
- MonitorPlan carries `(PickId -> slot, gen, width)` and no rings.
- Offline engines are installed directly.

**Files.**

- `tangent/src/instrument.rs`.
- `tangent-record/src/audio.rs`: Picks heap-backed; MAX_PICKS deleted.
- `tangent/src/record.rs`: sink widths and the monitor layout.
- `tangent/src/devices.rs`: no truncate to INPUTS.
- `tangent/src/desktop.rs`:
  - `push_monitor_settings`' picks binding moves to `Bridge::sync_inputs`;
  - `start_arr_record` and `finish_arr_record` bind `(TrackId, Slot, gen, PickId)` and resolve rows at finish;
  - the stem writer's width;
  - 3-digit stem names.
- `tangent/src/engine_bridge.rs`: `Bridge::sync_inputs` and the PickTable only.

**Depends on.** W4-integrate.

**Parallel with.** W5-identity-maps.

**Done when.**

- check.sh is green.
- **Inputs:**
  - 200 tracks bind inputs on fake 8-input and 64-input devices;
  - arming input 40 of a 64-input device records an arrangement take;
  - the capture callback allocates nothing.
- **Record under shape changes:** while an arrangement record captures, the bridge applies a permuted, grown and shrunk desk (rows inserted above, a row removed elsewhere, a second track armed). The WAV equals the reference input bit for bit, `cap_dropped` stays 0, and the take lands on its own track. A pick's ring is never replaced while its capture flag is set.
- **Ordinal mode:** with no explicit binding anywhere, every fixture keeps today's input assignment.
- **Memory:** idle stem and capture memory, measured, stays flat as lanes are added.
- **Stems:** a 120-stem export writes 3-digit names that sort.
- **Drums:** the third-kit-exported-silent test runs at 50 drum lanes.
- **`rt_alloc`:** covers MonitorPlan and capture-ring swaps.
- **Quantum:** capture rings are never smaller than the graph quantum (test).

### W5-save-gaps

**Added 2026-10-09 by the owner** ("yes fix those in wave 5"). Four save-road defects found by W4; three of them are on main today. Each fix gets a test that fails with the fix removed.

**Goal.** What a save writes is what the session shows, and no kill or Don't Save loses a kit or a plugin state.

1. **Save As and add_bare_track write a stale desk.** `save_project_as` writes `self.arrangement` without refreshing its desk from the live session, and `add_bare_track` does not refresh it either (W4-lane-refs proved it with a probe: add a track, Save As, reopen the copy, the row is missing; the copy's kit sidecar is written from the live rows, so its two files disagree). Fix: every save road refreshes the document's desk from the session at the moment it writes, through one function, and a scan or a road-list test proves no save road writes without it. Test: add a track (bare, MIDI, audio, folder), Save As, reopen the copy, every row and its kit are there; the same for Save and Save a Copy if it exists.
2. **Removing a drum track deletes its kit on disk at once.** The sidecar is rewritten without the kit immediately, so Don't Save after a removal reopens the project with the kit gone. Fix: a removal changes only the session; the sidecar is written with the document on the next save (W4-lane-refs already writes the sidecar with the document). Undo of the removal before a save brings the kit back from memory. Test: remove a drum track, Don't Save, reopen: kit present; remove, Save, reopen: kit gone; remove, undo, Save: kit present.
3. **drumpad.json backup.** W4-integrate (a3a89c9) keeps a legacy kit sidecar once beside it before the first format-2 write. Verify it covers every road that first writes format 2 (Save, Save As into an existing folder, template seeding, the launch reclaim), and that restoring `<doc>.pre-v8` plus the kept sidecar into a release build opens with its kits. Extend if any road is missed.
4. **A kill mid-save on the in-place roads.** W4-integrate (102a7b2) journals the two roads that move a session to a NEW folder (states owed before the document lands, paid at the next launch). Still open: `switch_project`, New, and `on_exit` when they write a never-written scratch's document for the first time, where the mirror lands after the document. Fix: the same owe-then-write journal on those roads (or mirror first, document last, if that is simpler and provably equivalent). Also: an owed mirror whose destination volume is unmounted at the next launch must stay owed (not be dropped) until it can be paid or the user discards the project. Tests: kill injected between every write on each road; the next launch pays what is owed; nothing is silently dropped.

**Files.** tangent-ui/src/app.rs (save roads, remove_track, add_bare_track), tangent-ui/src/drumpad.rs, tangent/src/desktop.rs (switch_project, New, on_exit, the owed-mirror journal from 102a7b2), tangent/src/shelf.rs (journal only). Coordinate: W5-identity-maps owns most of app.rs; keep hunks local to the save and remove roads.

**Done when.** The four tests above pass, each fails with its fix reverted, and the full gate is green.

### W6-rows-unlimited

**Goal.** The desk becomes the list of rows that exist:

- DeskModel.rows is the live model; the per-lane arrays and masks in Settings and DeskSnap are gone.
- Structural edits go only through `append`, `insert_rows`, `remove_rows` and `permute`; each calls `on_desk_shape_change` and, under cfg(test), `DeskModel::check`.
- Remove Track deletes the row.
- Vacant rows are hidden and uncounted, pedal residue included, and residue rows are never reused.
- One trailing add, drawn locked at the free cap.
- Clamps become grow-or-refuse.
- `desk_shape_held` replaces the order and removal holds during an arrangement record or a realtime export; Add appends during them.
- CHANNELS, STRIPS, INPUTS and MASTER_LANE are deleted from tangent-ui.
- Automation's master becomes `AutoLane::Master`.

Supporters (key or Konami) are unlimited end to end; free stays at 32.

**Files.**

- `desk_model.rs`: becomes live; `to_v22` deleted.
- `settings.rs`.
- `arrangement.rs`.
- `recorder.rs`: `live_midi_off` over rows returns a set; MidiRoute becomes a Vec; the constants deleted.
- `automation.rs`: `Curve.lane` becomes `AutoLane`.
- `app.rs`:
  - every `0..CHANNELS` walk bounded by `desk_width()`;
  - every `lane.min(CHANNELS - 1)` grows or refuses;
  - `open_lane_below` becomes `insert_rows`;
  - `take_desk_state_perm`, `send_desk_perm`, `compose_orders` and `Moment.perm` deleted;
  - `desk_shape_held`;
  - the Konami and width suites.
- `mixer_panel.rs`, `arrangement_panel.rs`, `recorder_panel.rs`, `desk_index.rs`: the trailing add and the locked add, hidden vacant rows, drop below the last lane.
- `dialogs.rs`: `Dialog::Track` bounds.
- `drumpad.rs`.
- `perf_fixtures.rs`.
- `tangent/src/engine_bridge.rs`: the rows input side.
- NEW `tangent/src/konami_e2e.rs` (cfg(test)).

**Depends on.** W5-identity-maps, W5-engine-memory-inputs.

**Parallel with.** W6-engine-buses-sends.

**Done when.**

- check.sh is green.
- **Width suites** at {1, 32, 33, 64, 65, 200, 512} cover reorder, delete above, insert above, undo/redo, folder runs, arm/mute/solo/phase, theory, inputs and kits. The drum panel and a latched automation row stay with their own tracks after a row above them is removed, through undo and redo.
- **Guards:** the exhaustive-destructure guard over TrackRow and every structural edit; the classification scan passes; `DeskModel::check` is silent across the suite.
- **Grep:** no CHANNELS in tangent-ui production code outside desk_legacy (path-qualified; the scope.rs and tapring.rs locals excepted).
- **Konami suite (11.2) green, with item 3 as restated:**
  - in the tangent crate through Bridge, a keyless cheater saves a 40-track project;
  - runs Reset Everything: `cheated()` is false and `project_request == NewKeeping`;
  - reopens the saved document through the host's apply path: 40 rows, each audible offline, `counted_tracks() == 40`;
  - the 41st add is refused with FreeCap, and one add is allowed after deleting to 31.
- **Holds:** during an arrangement record and a realtime export, insert, remove, reorder, kind and input-binding changes wait with the line, and Add appends without moving any row.
- **Undo:** the `undo_commit` sentinel is silent. Undo and redo across add, insert, paste and delete restore rows with their ids and every state, one gesture per moment.


**Carried in from wave 5 (2026-10-10, binding for this lane):**
- The strip-space relay W5-identity-maps left: `Bridge::desk()` still hands the host's take facts a strip-space desk, and `TangentApp::desk()`, `recorder::Desk` (V32Desk) and the meters' legacy order remain. Retire them by moving `desktop.rs` `push_monitor_settings`, `muted_out_of_the_take`, `armed_racks_pending` and the meter fill off strip space.
- The C11 seat-form adapters on TangentApp (`insert`, `set_insert`, `insert_bypassed`, `toggle_insert_bypass`, `set_insert_bypass`, `trade_inserts`, `open_insert_face`, `open_face_seats`, `set_insert_gr`, `set_builtin_face`, the seat `take_sampler_load` returns) go once the host stops calling them.
- `desk_shape_held` is still a stub (W5-engine-memory-inputs): input-binding changes on non-recording tracks take effect mid-record. Implement it.
- The capture tee skips a pick that reads nothing (`Renderer::mix_monitor`), so a capturing pick that loses its width mid-record (a device reopened narrower) stops its take silently. Make that loud.
- Owner default kept unless overturned: a recording track whose input is cleared keeps monitoring its take's input until the record ends.
- Design deviations to honour (recorded in the lane reports, not yet in sections 0-12): copy and paste are not relabels; a pad or FX swap is carried as SeatBay relabels (C11), not a seat permutation; `convert_global_shelf(launch)` takes the launch's shuffle set; `full_build` is a free function.

**Recorded by the W6-rows-unlimited fix round (2026-10-10):**
- *Deviation: a vacant row keeps an engine lane* (sections 3 and 7.9 say it gets none), so the words every engine is told stay the ones `bridge.json` pins. Consequence: on a desk whose 32 legacy rows all hold lanes (the shipped desk), a row inserted between two others lands past slot 31 live, while an export, built fresh with row r at slot r, puts it on its row's slot. A new track on a reused hidden row is not affected: the bridge takes the lowest free slot below 32 first and waits for one on its way home before any slot past them (`Bridge::install`), so it takes its row's slot, the export's.
- *Automation plays at every slot* (closes the MERGE-ME on `SlotAutomation`): `AutoNow` is one row per spine slot (`Wide::auto`), filled by a walk of the curves; the generation table is as wide as the widest slot a curve names; `param_slot` files a plugin curve on a lane past 31 on its own chain seats; `DeskLanes::suspend` takes every slot; and a Mute curve on a lane past 31 reaches `resolve_heard`.
- *Still bound to the legacy slots (closed: W7-slot-gaps fixed every item, and W7-join ran the two that waited for buses):* a lane past slot 31 lacked these, live and in an export: a drum lane's pad effect racks (the MERGE-ME on `PadBank`, now a sparse pad-seat range); sidechain keys (routes and sends past 31 sound since the wave-6 merge 09d600b; keys past 31 needed no fix, and a key to a comp on a bus past 31 is held live and exported since W7-join); a sampler bay's sample (the renderer's file table and `DesktopApp::sampler_bays_of` reach every slot); and the automation overlay on an open face (the seat-form adapters, `open_face_seats`, retired). Every item has a width test at {1, 31, 32, 33, 40, 200} in `tangent/src/slot_gaps.rs`, none ignored.
- *Holds:* a paste, a drawn MIDI item, or a `.mid` dropped or picked that would give an empty row a kind or an input asks the hold before its moment and changes nothing (`paste_held`, `midi_bind_hold_line`).
### W6-engine-buses-sends

**Goal.** The engine half of unlimited buses, returns and sends, plus parallel post-fork work:

- Bus lanes run their rack once over what arrives.
- Sends (pre or post, any number, to Fx or any bus) are gathered per return.
- The RouteTable is unbounded: MAX_ROUTES, the 8-bit fields of `pack_route`/`pack_key`, the per-block Kahn and `route_transfer`'s recursion all leave the audio thread.
- PDC applies on bus paths only.
- Buses are solo-safe.
- Legacy channel-to-channel routes keep the double rack run and the cycle rule, bit for bit.
- Bus, folder and route-destination racks run per topological level through `run_list`, with sum order and publish order transcribed from today's serial code.

**Files.**

- `tangent/src/instrument.rs`, `desk_plan.rs`, `lane_bank.rs` (if needed), `rt_bench.rs`.

**Depends on.** W4-integrate, W5-engine-memory-inputs (instrument.rs base).

**Parallel with.** W6-rows-unlimited.

**Done when.**

- check.sh is green.
- **Slot-API tests:**
  - the 1,000th route sounds;
  - 50 returns are each audible through their own rack;
  - a counting test unit proves a bus rack runs once per block;
  - send automation works per return;
  - PDC aligns a look-ahead built-in on a bus;
  - a soloed track keeps its return heard, and a soloed bus hears its feeders;
  - routes, sends and keys to and from slots 256 and up reach the right lane.
- **Bit-exactness:** every legacy golden is unchanged with the level fork on and off. Level-parallel equals level-serial bit for bit on 25 nested folders and 50 returns.
- **`rt_alloc`:** covers plan swaps that carry 1,000 routes, and level runs.

### W7-buses-ui

**Goal.** Users create buses and returns, send any strip to any number of them, see and edit sends on the strip, and route with typed endpoints. No cap anywhere, on either tier.

**Files.**

- `app.rs`:
  - New Bus and New Return;
  - kind changes are cap-checked;
  - each send gesture is one undo moment;
  - the `DialogAction::AddRoute` full-table notice deleted.
- `mixer_panel.rs`: send rows on the STRIP_ROUTES pattern with a +N chip; a bus icon; no input chip on a bus.
- `dialogs.rs`: the routing dialog lists buses and returns, with a filter and cycle greying.
- `routing.rs`: MAX_ROUTES and `Routes::add`'s refusal deleted; `a_sixty_fifth_route_is_refused_rather_than_stored_and_never_sounding` inverted.
- `automation.rs`: `AutoParam::SendTo { to }` appended, with key `send:t<id>`.
- `recorder.rs`: `ChannelKind::Bus = 3`.
- NEW `tangent-ui/src/desk_topology.rs`: `would_cycle`.
- `desk_model.rs` and `desk_format.rs`.
- `tangent/src/engine_bridge.rs`: bus kinds and sends into PlanInputs.

**Depends on.** W6-rows-unlimited, W6-engine-buses-sends.

**Parallel with.** W7-scale-and-budgets.

**Done when.**

- check.sh is green.
- **Workflow:** tests create a return, send three tracks to it, move a level and remove a send, then undo and redo each step.
- **Bus limits:** a bus refuses clips and inputs.
- **Free tier:** a free desk at 32 tracks creates 100 buses and 1,000 sends.
- **Cycles:** the picker refuses a new cycle, while a legacy saved cycle opens and sounds the same (golden).
- **Endpoints:** route, send and key endpoints survive reorder and delete.

### W7-slot-gaps

**Added 2026-10-10 at the wave-6 merge.** Everything a lane at slot 32 or above still lacks, live and in an export, plus the bus lane's mixing leftovers. A paid desk must not have a track 33 that is quietly less of a track than track 1.

1. **Pad effect racks past slot 31.** The MERGE-ME on `PadBank`: `RackOwner::Lane` `seat()` returns None for pads past slot 31; `first_drum_lane` and `pad_racks_live` filter to slots below CHANNELS. Give pads a sparse seat range (created on first use, design 7's lazy pads), named by SeatBay on the shelf like every other bay.
2. **A sampler bay's sample past slot 31.** `Renderer::sampler_samples` is CHANNELS long and `DesktopApp::sampler_bays_of` walks only LEGACY_LANES slots, live and in the export's `mirror_sampler_samples`.
3. **The automation overlay on an open built-in face past slot 31.** `open_face_seats` goes through the legacy seat map; retire the remaining C11 seat-form adapters on TangentApp with it (W5/W6 carry-in list).
4. **Sidechain keys past slot 31.** Prove a key from lane 40 to a compressor on lane 33 (and to a bus) sounds and lands on no other lane; fix if not.
5. **The send knob as a plan entry (from W6-engine-buses-sends).** Carry each strip's knob as its post-fader Fx entry (or carry no entry) and write knob moves through `Engine::set_send_level(SendFrom::Lane(slot), SendTo::Fx, level)`; `AutoParam::SendTo` maps onto `SendCurve`s; `Bridge::publish` carries send levels between plans, not only route gains. Coordinate with W7-buses-ui, which builds the send UI on top: this lane owns the bridge and engine half.
6. **Mixing leftovers (from W6-engine-buses-sends):** the dry stem of a node with a delay line is early by the main-path wait; a bare drum bus or click sending to a lookahead bus has no rank in the compensation graph; prove or fix each with a test. (A track in a folder that is also routed into a bus is summed by both, the old fold rule: leave it, but pin it with a test so a change is deliberate.)

**Files.** tangent/src/instrument.rs (pads, sampler, keys, PDC), tangent/src/engine_bridge.rs and tangent/src/desktop.rs (sampler walk, send levels), tangent-ui/src/app.rs (face overlay, adapters). Coordinate with W7-buses-ui on app.rs and the bridge.

**Done when.** A width test at slots {31, 32, 33, 40, 200} gives each item the same behaviour as on lane 1, live and in an export, each failing with its fix reverted; the full gate is green.

**Recorded by the W7-slot-gaps fix round (2026-10-10):**
- *Done-when was partial for two items until W7-buses-ui; W7-join finished both.* A key to a BUS (item 4) and a curve on a strip's send to a bus (item 5, `AutoParam::SendTo`) had no road from the desk while the bridge made no row a bus, and their width tests waited in `tangent/src/slot_gaps.rs` as ignored MERGE-ME tests. Both run now, un-ignored, at {1, 31, 32, 33, 40, 200}: `slot_gaps::send_to` answers `AutoParam::SendTo { to }` and `engine_bridge::send_bus_of` has its body. The key test passed as it stood once buses existed; the send-curve test failed on its LIVE road because the suite's live engine was never handed a curve (`push_automation` was a method the suite could not drive), so the host's push is now `DesktopApp::push_curves` and `Host::frame` takes it. The bridge's mapping test uses `send_bus_of` and real `SendTo` curves in place of its stand-in decoder.
- *Item 6 is held at the width slots in the engine* (`slot_engine::buses::*_at_every_width`) and, since W7-join, through the bridge, live and in the export: a lined node's dry stem (a folder and a bus node), a bare kit and the click each sent into a bus that looks ahead, and a folded track routed into a bus (`slot_gaps`). An export hears no live MIDI and leaves the click out of the file, so for those two the export holds that its plan carries the send (the bare kit) or that nothing of it is heard and the file is held back by nothing (the click).
- *The width suite drives the host's own roads*, not copies: `Host::frame` runs `DesktopApp`'s `bays_wanted`, `next_owed_bay`, `load_bay`, `deliver_samples`, `deliver_kits`, `push_pad_knobs`, `forget_moved_slots`, `set_insert_param` and (W7-join) `push_curves`, delivers the kits only when the host's own rule owes them (`Synced::owes_kits`, a retry, an edit), and `slot_gaps::the_frame_runs_the_roads_this_suite_drives` holds that the frame takes them.
- *A pad's bay has no engine name while its lane has no pad bank* (`Bridge::engine_bay`), and the engine makes no pad rack for such a lane (`HostSlot::banked`): a pad effect seated before its track's kit now waits for the kit rather than going home with its rack.
- *A lane that comes or goes leaves no record of a sampler's file at its slot* (`forget_moved_slots`): the file is the lane's own, and marking every installed slot owed made a wide desk's reload hand an empty file to each new lane first.

### W7-join

**Added 2026-10-10 at the W7-buses-ui + W7-slot-gaps merge (901b2b2).** What only the merged tree can finish: the two lanes' hand-offs to each other, the review findings both lanes' skeptics upheld but whose fix rounds left them (low severity), and one defect seen but not fixed. The merge itself already joined the plan's sends: `Bridge::plan_sends` = `knob_sends` (Fx entries) + `read_sends` (sends to buses), carried between plans by buses-ui's one `carry_scales` (keep it the only carry).

1. **Send curves play.** Give `engine_bridge::send_bus_of` its body (`AutoParam::SendTo { to } => Some(to)`), make `slot_gaps::send_to` answer `AutoParam::SendTo { to: bus }`, and set `tangent_ui::automation::SEND_CURVES_PLAY = true` in the same commit. Mind buses-ui's note at the switch: the engine names a send by its two ends, so one engine curve would drive a strip's pre- AND post-fader sends to one bus together, while the app's curve belongs to the post-fader send alone: make the engine curve drive only the send the app's curve names (or prove the twin cannot reach the engine and pin it). `bus_e2e::the_send_curve_switch_says_what_an_export_plays` and the app tests' "played" branches must pass with the switch on; the "not played" branches go (the switch can be removed outright if nothing else reads it).
2. **Un-ignore the two MERGE-ME width tests** in `tangent/src/slot_gaps.rs` (`a_key_reaches_a_comp_on_a_bus_past_slot_31_as_on_row_1_live_and_in_the_export`, `a_send_curve_to_a_bus_past_slot_31_plays_as_on_row_1_live_and_in_the_export`). They have never run: make them pass for real (fix the code, not the numbers), and add the bridge-level key-to-bus test and the live/export bridge tests for item 6 of W7-slot-gaps (dry stem, bare drum bus or click into a lookahead bus) now that the bridge marks bus lanes.
3. **Upheld review findings left open (W7-buses-ui):**
   - The UI and the bridge disagree on what a bus send is: the UI counts any send whose target is a Track node, the bridge plays only sends whose target row is a bus. Make them one rule, in one function both read (a send to a non-bus row stays legal in format 2 per the round-trip test, but it must not be offered, drawn or counted as a send in the UI).
   - Pasting a copied bus mints the row's id twice (`paste_track_inner` then `set_channel_kind`). Mint once.
   - `retarget_row` rewrites `AutoParam::SendTo { to }` curves but not the UI automation state keyed by (Node, AutoParam): `arr_auto_open`, `arr_auto_armed` and their kin. Rewrite those too.
4. **Seen, not fixed (W7-slot-gaps fix round):** picking the drumpad into a track's first bay does not mark the kit owed; if another drum lane's kit has already landed, `reload_pads` does not run and this track's kit is not sent until another kit edit or a reopen. Prove with a test, then fix.
5. **Tidy:** the `cfg_attr(not(test), allow(dead_code))` on `Engine::set_send_level` is stale (the bridge calls it in production): remove it. Update this document's "Still bound to the legacy slots" bullet and the W7-slot-gaps note to say what the join finished.

**Files.** tangent/src/engine_bridge.rs, tangent/src/slot_gaps.rs, tangent/src/bus_e2e.rs, tangent/src/instrument.rs, tangent/src/desktop.rs, tangent-ui/src/{app.rs, app_bus_tests.rs, automation.rs, desk_model.rs, mixer_panel.rs, dialogs.rs}, this document. One lane; no siblings.

**Done when.** Every item has a test that fails with its fix reverted; the two width tests run un-ignored at {31, 32, 33, 40, 200}; no MERGE-ME naming W7-buses-ui or W7-slot-gaps item 5 remains; the full gate is green.

**Recorded by the W7-join round (2026-10-10):**
- *Item 1, the switch removed outright:* `SEND_CURVES_PLAY` and `AutoParam::plays` are gone (nothing else read them), and the four doors they shut (the A menu's Sends group, a send row's writes, opening the row by its key, a lone base set on open) are as they were before the switch. The tests that held the switch keep their played branches: `bus_e2e::the_send_curve_switch_says_what_an_export_plays` is `bus_e2e::a_send_curve_plays_in_the_export`, and the app's two switch-named tests are `a_tracks_post_fader_sends_are_offered_for_automation_by_their_buses_names` and `a_send_curve_is_written_opened_and_heard_as_a_faders_is`.
- *Item 1, the twin reaches the engine, so the engine tells it apart:* a file can hold a strip's pre- and post-fader sends to one bus, so `instrument::SendCurve` carries `pre`, `resolve_send_curves` drives only the send on that side, and the bridge names the post-fader side (`slot_engine::buses::a_send_curve_drives_its_own_side_of_the_fader_and_never_its_twin`, `bus_e2e::a_send_curve_rides_its_own_send_and_leaves_the_cue_beside_it`).
- *Item 3, the one rule* is `desk_model::bus_sends` over `desk_model::buses_of`: `TangentApp::strip_bus_sends` (and `bus_ids`) for everything drawn, listed, offered and counted, and `Bridge::read_sends` for what plays. `desk_topology` reads it too (fixed at the join merge, after its review upheld the gap): a send aimed at a track that is no bus is no loop edge, so no picker greys a target and no route is marked feedback for a send that does not sound; made a bus, the row's send is an edge again (`a_send_to_a_track_that_is_no_bus_is_no_edge_until_the_track_is_a_bus`).
- *Item 3, retargeting:* the (owner, param) state follows a send's bus to its new id, and a choice by place on an owner whose rows the new id re-sorts goes (the bay move's rule). `arr_auto_rows` reads by owner through `auto_rows_of`.
- *Item 4 is the bridge's, so every road that seats the drumpad is covered:* `Synced::drummed` names each lane that has just become a drum lane (the set rebuilt every frame and cleared with a fresh engine), `Synced::owes_kits` is moved-or-drummed, and `push_monitor_settings` marks `pads_owed` by it.
- *Item 5:* the stale allow on `Engine::set_send_level` was already gone in the merged tree (W7-slot-gaps' d7eb020 removed it, and the merge kept that side).

### W7-scale-and-budgets

**Goal.** Measure and tune at scale:

- rt_bench rungs at 64, 256 and 1024 lanes (bare, EQ, comp, plate);
- 50 returns x plate, and 25 folders x [EQ, comp, plate], with the level fork on and off;
- idle lanes skip decide and fold in O(1), through a dense active list;
- GRAIN_AUTO_BLOCKS and the grain phase cap retuned by measurement;
- the rtwork width cap (16) reconsidered;
- optionally, each lane's fold work moved into its hand;
- delay lines need-sized only if the measurement asks for it.

**Files.**

- `tangent/src/rt_bench.rs`.
- `tangent/src/instrument.rs`: constants, the active list and the fold split only.
- `tangent/src/rtwork.rs`: the width cap.
- NEW `docs/PERF-UNLIMITED.md`.

**Depends on.** W6-engine-buses-sends, W6-rows-unlimited.

**Parallel with.** W7-buses-ui.

**Done when.**

- check.sh is green.
- **Report committed, with:**
  - 16x[EQ, comp, plate] and 32x3 EQ within 5% of wave 1 at 128 frames;
  - the marginal us per bare lane at 64, 256 and 1024 lanes;
  - the bus and folder rungs against the 2.67 ms deadline, level fork on and off;
  - the target of 1024 idle lanes costing at most 4% of a 128-frame deadline (reported, not asserted).
- **Bit-exactness:** the active-list walk and any fold split render bit-identically to the full serial walk.
- No benchmark or cargo job runs while the owner listens.

**The quiet window (2026-10-10, from about 11:05, roughly an hour).** The owner closed Tangent, Navicord and every audio app for this lane; the machine is otherwise lightly used (terminal, editor, a browser reading). Build and unit-test first; then run every timed measurement **serially, one cargo process at a time, with nothing else of yours compiling**, release profile, each rung repeated enough to report a median and a spread. Take the carried wave-1 question in the same window: the rtwork LanePool against the serial walk (the two earlier tries were skewed by a busy machine), and say plainly if it is still inconclusive. Record the load average before and after each run in the report; a run with a load average above 2 at its start is re-run, not reported.

### W8-gates-and-readiness

**Goal.**

- Fold every ban scan and the copy scan into check.sh.
- Rewrite the remaining fixed-desk principle comments.
- Delete the transitional code and STAGING.
- Finalize the copy.
- Run the census and the cross-platform suites.
- Hand the owner the out-of-repo checklist.

**Files.**

- `scripts/check.sh`, `scripts/test-census.sh`.
- Principle comments in `recorder.rs`, `mixer_panel.rs`, `arrangement.rs`, `routing.rs`, `app.rs`, `tangent-record/src/audio.rs`, and the cheat doc in `tangent-core/src/license.rs`.
- Dead transitional code:
  - `encode_v32` moves to cfg(test);
  - leftover shims;
  - the W1 cross-checks against the live seat arithmetic;
  - STAGING in `tangent-core/src/config.rs` and `settings.rs` (TANGENT_CONFIG_DIR stays as a developer switch).
- `CHANGELOG.md`.
- Final copy touch-ups.

**Depends on.** Every earlier stage.

**Parallel with.** None.

**Done when.**

- check.sh is green on macOS ARM.
- **Other platforms:** void, bsdbox and Windows `--no-fail-fast` runs show only the 4 known ARM-only failures; goldens are cfg-ignored off aarch64 macOS.
- **Census:** no test removed without a named replacement.
- **Copy:** no published text contains an em dash; the phrase scan is in check.sh.
- **Grep gates, path-qualified** (`recorder::`, `instrument::`, `tangent_ui::recorder::` and `use` lists), with the bare names checked outside the files that keep an unrelated local constant (`tangent-ui/src/scope.rs`, `tangent-ui/src/tapring.rs`). None of these appear in live code: CHANNELS, STRIPS, TOTAL_SEATS, RACK_SEATS, INSERT_SPACE, MASTER_AUTO_LANE, `seat_order`, `permute_channel_inserts`, `.min(31)`, `.min(63)`.
- **STAGING** is gone (scan).
- **The owner confirms** (publishing is the owner's; W8 is not done until each is confirmed):
  - tangent-fulfil redeployed on Fly;
  - the Gumroad listing replaced from STORE-CONTENT.md;
  - the ganten.neocities.org card updated;
  - the first post-W8 install is the moment real config and projects migrate (backups `settings.pre-v23.json` and `<doc>.pre-v8`; legacy sidecars never touched).

**Carried in from wave 7 (2026-10-10, at b2ca85c, integrated gate green: 3976 passed, 0 failed):**
- **The render goldens differ between debug and release.** `render_golden::every_desk_renders_its_golden` passes in the debug gate and fails under `cargo test --release` on one desk, `auto-pan` (`e585ba2096f0df42` against the recorded `5728be85fdd7db3a`); the scale lane saw it at its base, so it predates W7. Shipped builds are release, so find which arithmetic in the auto-pan road the optimiser changes (a curve or pan-law evaluation, a const fold, an inlined `mul_add`), and decide with evidence whether it is benign: make the goldens hold in both profiles (the same bits from both is the target; a road whose result depends on the profile is a defect unless proven to be only libm's last bit), and add `cargo test --release` of `render_golden` to check.sh.
- **The pool on a live device is unmeasured** (docs/PERF-UNLIMITED.md section 7): put it on the owner's checklist with exactly what to watch (a 1,024-lane idle desk and a 64-lane [EQ, comp, plate] desk at 128 frames, the overload counter), and do not run it yourself: launching Tangent is the owner's.
- Remote runs (void over `ssh void`, bsdbox, Windows): use them if reachable; if a box is not reachable, say so in the report and leave its item for the owner rather than inventing a result.

**Recorded by the W8 round (2026-10-10):**
- *The release goldens:* the auto-pan road's difference was `pan_gains`' sine and cosine of one angle, fused by the optimiser into Apple's `__sincosf_stret` (the release test binary imports it, the debug one does not). Over every f32 angle in [0, pi/2] the fused pair differs from `sinf`/`cosf` by one ulp at 828,043 angles (sine) and 272,987 (cosine), never more: libm's last bit, inaudible, but profile-dependent. The sine is out of line now (`pan_sine`, `#[inline(never)]`), so both profiles render the RECORDED goldens; nothing was re-recorded (revert check: without the attribute the release hash is e585ba2096f0df42 again). The export goldens hold in release too, and check.sh renders both in release.
- *The census was red on the branch:* 8f0efa7's ledger was written as wrapped bullets the census never matched. History is not rewritten; `scripts/test-census.ledger` holds late ledger lines keyed by full commit hash, and `test-census.sh` reads it.
- *The gates in check.sh:* `named_leg` runs a filtered test leg and refuses fewer tests than it names (a filter matching nothing is green in cargo). One leg names the 37 ban scans the waves left and this one added (with `--exact`), one renders the goldens in release. The tree-wide grep gate is `tangent-ui/src/ban_scan.rs` (a lexer, not grep: comments, literals, test items and test-only module files are not live code), reading through copy_scan's one lexer. The phrase scan refuses an em dash on every surface it reads; a sibling test holds every other crate's shipped strings (the desktop host's notices among them, which no scan had read, and tangent's build.rs, which writes the Windows version resource) to the dash rule alone; a third reads what the downloads and installers carry (next entry).
- *Published text no scan read (found in review):* "no published text contains an em dash" held only on the surfaces the scans read. The FreeBSD package's post-install message, `LICENSING.md` (the licence page of the macOS and Windows installers), both macOS installer pages (written `&mdash;`, which a search for the character misses), the font notice and Terminess's licence header, and the Linux and FreeBSD installer scripts and `tangent-jack` (their usage and progress lines, and the comments of scripts a download hands a person to read) had one on 48 lines, each rewritten by hand. `copy_scan::nothing_the_downloads_and_installers_carry_has_an_em_dash` now reads every file a download or an installer carries for a person, as it reaches them (`copy_scan::Reads`): what a person is handed is read whole, comments and all; what a program parses or renders (the pkg manifest, the NSIS script, the desktop entry, the installer's HTML and XML) is read without the comments it drops, character references resolved; and the text three build scripts write into a download (the Info.plist usage strings macOS shows, the FreeBSD bundle's README, ffmpeg's SOURCE.txt) is read out of the scripts. Not read, each for its reason: licence texts nobody here wrote (the MIT grant, the GPL, the OFL, ffmpeg's), and the macOS package's postinstall, which prints nothing. Revert checks: a mark planted on each of the sixteen surfaces (and a font licence added to the folder) fails the scan at its line; the em dashes still in comments that never ship (twelve in the manifest, three in the distribution file, two in the NSIS script, four in the pages' style sheets, two in the Info.plist's comment) pass.
- *Deviation, the engine's legacy names:* the gate bans CHANNELS, STRIPS and RACK_SEATS in live code, and the engine still had all three as names for its frozen V32 seat space (slots below 32). They are `LEGACY_LANES`, `LEGACY_STRIPS` and `LEGACY_SEATS`, the same numbers; `recorder`'s re-export of the frozen map's old names and `recorder::Desk` are gone, and every reader names `desk_legacy`.
- *Deviation, the clamp exemptions:* `.min(31)` stays legal on the DX7's algorithm (two files, 32 algorithms) and `.min(63)` on a VST3 bus's silence flags (one u64), each named with its reason in `ban_scan::NOT_LANES`; an exemption that stops matching fails the scan.
- *Deviation, `encode_v32` stays live* (the design moved it to cfg(test)): production still encodes identity to a legacy flat bay in two legacy READERS (`LegacyDesk.standing` through `dense_bays`, and `DeskSnap::from_map`'s row-exists reconstruction) and in the `Format::Legacy` writers, which `WRITE_V8` leaves reachable only through the `*_as` entry points. Moving it means making the legacy writers test-only (which takes away the one switch's road back) and rewriting two legacy readers, the code the risk register ranks first, for no change in behaviour. The exhaustive bijection test that justified the move still proves encode and decode agree on all 13,176 bays.
- *Deviation, the W1 cross-checks are kept, less their tautologies:* the "live seat arithmetic" was not deleted in W5, it was retired INTO `desk_legacy` as second formulations, and the engine still reads seven of them for its legacy slots (`pad_rack_index`, `folder_extra_seat`, `amp_pedal_seat`, `bus_extra_seat` and their inverses). `every_live_seat_function_agrees_with_the_decode` is what holds them to the frozen decode, and the `slot_key`/`auto_key` checks hold the live resolvers to the frozen display order, so they stay. What went is what could not fail: the frozen bases compared with their own aliases (`the_frozen_counts_are_still_the_live_desks` keeps the three live comparisons).
- *Deviation, `Spine::rank_scratch` is deleted:* section 7.3 lists it; no walk ever read it and every spine allocated it. The kernels' module-wide `#[allow(dead_code)]` (W1, "their consumers are the next waves") is gone; what only tests use is cfg(test).
- *Done beside the list:* the mixer's accessibility ids are keyed by the row's real TrackId (the MERGE-ME left for W3); the arrangement's are keyed by hit text and never claimed identity, left as they are. Two window titles use " - " as their siblings do. The changelog's [Unreleased] lost its 159 em dashes and four entries that described the fixed desk as the present, and gained the unlimited desk's entries.
- *Not run here:* void, bsdbox and Windows `--no-fail-fast` suites. The lane's rules confine cargo to the sandboxed local runners (a test run on those boxes would write their real `~/.config/tangent`), so they are on the owner's list below. Windows was cross-CHECKED here (`cargo xwin check`, tests included), which compiles and runs nothing.

**The owner's checklist, out of the repository** (W8 is not done until each is confirmed):
1. **Redeploy tangent-fulfil on Fly** (its key email says what a key does since W3).
2. **Replace the Gumroad listing** from `docs/STORE-CONTENT.md` and `docs/release/next/GUMROAD.md`.
3. **Update the ganten.neocities.org card**: 32 tracks free, unlimited with a supporter key, buses, returns, sends and routes unlimited for everyone, keys already sold included; no em dash, and no Konami mention (owner call 9).
4. **The first post-W8 install migrates the real files.** Before it, a copy of `~/.config/tangent` and `Documents/Tangent Media` costs nothing. On its first saves it writes `settings.pre-v23.json`, `<project>.pre-v8` and `drumpad.json.pre-v8` beside each legacy file, once, and copies the global plugin shelf to `pstate-*.bin` names (`.shelf-v23` marks it done); no `insert-state-*.bin` is renamed or deleted, so putting the kept files back hands a release build its projects whole. Older builds cannot open the new files. The internal builds' `~/.config/tangent-staging` and `Documents/Tangent Media Staging` are no longer read by anything and can be deleted once nothing staged is wanted.
5. **The gate needs no hand-made scratch HOME any more** (W8-safe-tests): a test build's every road to a home ends in a scratch folder of its own, and check.sh runs each test leg in a watched home besides (`scripts/check-home.sh`). A bare `cargo test` is safe by the build alone; only the `#[ignore]` probes over real projects read them, read-only, when run by name. Close Tangent before check.sh (release.sh runs it): anything but its log that it writes in its own folders during a run fails that run, since a test's write cannot be told from the app's without opening his files.
6. **The live-device pool** (docs/PERF-UNLIMITED.md section 7, never measured): with Tangent running on the interface at 128 frames, open a 1,024-lane idle desk, then a 64-lane desk with [EQ, comp, plate] on every lane; play each for a few minutes and watch Audio Setup's dropout line ("no dropouts since this stream opened") and that playback stays clean. This is the one measurement of the fork with its workers joined to the device's workgroup.
7. **The other platforms:** on void (`ssh void`), bsdbox and Windows, `cargo test --workspace --exclude linuxvideo --no-fail-fast` should fail only the four known ARM-only tests (`keys` and `dx7` first-strike hashes, `keys::board` cost, `analog` sixteen voices); the render and export goldens are ignored off aarch64 macOS. A timing test that fails in the full parallel run and passes alone (`keys::tine`'s strike cost is the known one) is load, not a finding.

### W8-safe-tests

**Added 2026-10-10 at the W8 merge.** The W8 review upheld these and its fix round (medium and up only) left them. Nothing is built for the owner until this lane is merged.

1. **No test reaches the owner's real files** (the serious one). With STAGING gone, the `tangent` binary crate's tests resolve `~/.config/tangent`, `Documents/Tangent Media` and `~/Movies/Tangent` for real, because tangent-ui's and tangent-core's `cfg(test)` redirects do not apply when those crates are linked into another crate's test binary; check.sh (which release.sh runs) therefore writes the owner's real config. Fix it in the build, not by convention: e.g. a `test-home` feature on tangent-core (and tangent-ui if it resolves paths) that sends every config, media, projects and legacy-media path to a per-process scratch folder, enabled ONLY through the `tangent` crate's `[dev-dependencies]` (resolver 2 unifies dev-dependency features only into test builds; prove `cargo build --release` and the shipped binaries do NOT carry it, e.g. by a test or a check.sh leg that inspects `cargo tree -e features` for the release build). Every road that names a real home path must go through the redirect. Then prove it: a check.sh leg (or test) that runs the workspace suite with a HOME it watches and fails if any file under the real `~/.config/tangent*`, `~/Documents/Tangent*`, `~/Movies/Tangent` or `~/Music/Tangent*` is created or modified; and remove the two known tests that delete the real `cheat.unlock` (design section 11) or point them at scratch. Keep `TANGENT_CONFIG_DIR` / `TANGENT_MEDIA_ROOT` working as developer switches. Never touch the real folders while proving this: watch them read-only (mtimes/listing before and after).
2. **The ban/copy scan's cfg test** (`is_test_cfg` in copy_scan.rs): a `cfg(all(..))` counts as test-only only if `test` is a whole cfg predicate (not a substring: `feature = "latest"`, `"test-harness"`, `target_env = "contest"` ship), and a `not(test)` inside never makes it test-only. Plant each and prove the scan reads them as live.
3. **A one-line `#[cfg(test)] mod x;`** (and the comment/blank-line-split form the implementer reported) is recognised as a test-only module file. Plant each and prove it.
4. **The media-path exemption in settings.rs** is back to naming exactly the lines it allowed (the chooser's real default and `test_path()`), so a second hard-coded `.join("Tangent Media")` fails the scan. Plant one and prove it.
5. **Copy the owner decides later, not now:** leave the installer pages' retired-plugin wording as is (owner's call, already on the checklist).
   **Overruled by the owner, 2026-10-10:** "remove those mentions of the vst plugin -- that's what it means to be dead", then "yes, delete the plugin crate too". The copy was removed in 012aebd and fb8d9f2 (`lane/ul-w8-vst-dead`), and the plugin crate, its build and verify scripts, `LICENSE-GPL-3.0` and its gate legs on `lane/ul-w8-plugin-gone`. The cleanup that removes an older install's `Tangent.vst3` stays.

**Files.** tangent-core/src/config.rs (and Cargo.toml), tangent-ui/src/settings.rs and Cargo.toml as needed, tangent/Cargo.toml, tangent-ui/src/copy_scan.rs, tangent-ui/src/ban_scan.rs, scripts/check.sh, any test that names a real home path, this document.

**Done when.** Each item has a test or gate leg that fails with its fix reverted; the full gate is green; a full workspace test run leaves every real Tangent folder's listing and mtimes exactly as before (recorded in the lane notes).

**Recorded by the W8-safe-tests round (2026-10-10, base 739efcb):**
- *What the suite did to a real home before the fix:* nothing it wrote, measured. The workspace suite run with HOME at a sandbox, once empty and once seeded with a real-looking config and media folder (a v22 settings file, taught chords, a licence and its Konami marker, a legacy plugin state, an Untitled project, a take), created, changed and deleted nothing under the Tangent folders. What it did do: every app a test built read the taught chords from the real `overrides.json` (`OverrideStore::load` in `TangentApp::with_license`), the binary's settings path, live plugin shelf, licence and media roots were the real ones, and each test stayed off them by choosing a temp path of its own. The claim that check.sh wrote the owner's config was the hazard, not an observed write; the fix is the same either way.
- *The redirect (item 1):* `tangent_core::config::home` is the one road to a person's home, with `documents`, `videos` and `TEST_HOME` beside it. A test build (that crate's `cfg(test)`, or the `test-home` feature) answers `<temp>/tangent-test-home-<pid>`, emptied on first use; every shipped build answers `dirs` as before, with HOME behind it. `test-home` is turned on only from `[dev-dependencies]` lines: the binary's, tangent-ui's, and (since the review round below) tangent-core's own, naming itself, for its integration tests. The roads moved onto it: `Settings::default_media_root`, the old take folder (`default_legacy_record_root`, split out of `legacy_record_root`), the explorer's `dirs_home` (app.rs), the desktop's `panel_start`, folder browser and `shorten_home`, and `acquire_single_instance` (main.rs). tangent-ui no longer depends on `dirs`, and the binary keeps it as a dev-dependency only (one ignored probe measures the old per-frame `dirs::home_dir`), so a second road through `dirs` is a compile error in a shipped build (std's own `home_dir` is not: the pinned toolchain compiles it without a warning); `ban_scan::no_live_code_finds_a_home_but_the_one_road` refuses one in every crate (since the review round: a home-finding crate by path, `home_dir` and the platforms' folder calls, or a home variable's name in a shipped string), keeping by name the VST3 SDK's search paths and `take::default_root` (a path nothing asks for, exempt only while nothing outside take.rs calls it).
- *The proof:* `scripts/check-home.sh run` runs every check.sh test leg with HOME at a fresh folder seeded with those decoys, and fails on anything created, changed or deleted under `.config/tangent*`, `Documents/Tangent*`, `Movies/Tangent*`, `Videos/Tangent*` or `Music/Tangent*`; the real folders are watched read-only too (a listing and mtimes against a stamp in the temp folder) and fail it too, Tangent running or not, the running app's own log aside (since the review round; any change was a note while Tangent ran). `scripts/check-home.sh features` reads tangent-core's features with `cargo tree` as a build resolves them: the binary with every feature on every target and the plugin carry `[learning, license]`, and each member's test build that links tangent-core carries `test-home` (the binary's and tangent-ui's `[learning, license, test-home]`, tangent-core's own `[test-home]`); and no script outside the gate builds a test, bench or example target, which would resolve the feature into the binary built beside it. Pinned by `config::a_test_build_keeps_every_home_path_in_its_own_scratch_folder`, `config::a_scratch_home_starts_empty_whatever_was_left_in_it`, `settings::the_media_choosers_build_on_the_one_road_to_a_home` (in check.sh's `-p tangent-ui` leg), `home_tests::this_test_build_keeps_every_home_path_in_a_scratch_folder` (in its `-p tangent` leg; a workspace run unifies either crate's dev-dependency line into the other) and tangent-core's `tests/test_home.rs` (in its `-p tangent-core` leg).
- *Revert checks (through the lane's sandboxed cargo, never the real home):* the redirect undone in config.rs fails the scratch-folder config pin, the settings pin and the binary's pin, and a planted test writing through the production roads (settings save, `LicenseStore::load` with unlock and relock, a project under `project_root`) then wrote the watched home and `run` failed naming the relocked marker, the overwritten settings (and the `settings.pre-v23.json` and `.shelf-v23.pending` its first format-2 write left), and the Untitled project; with the redirect in place the same plant wrote only its scratch home and passed. Either crate's dev-dependency without the feature fails its pin; `features` fails with the binary's dev line losing the feature, either shipped line gaining it, or a build script asking for `--all-targets`. One desktop road put back on `dirs` fails the home scan at its line and does not compile as the shipped binary.
- *Item 2:* `ban_scan::is_test_cfg` (the design said copy_scan.rs; copy_scan's lexer calls it) parses the predicate and evaluates it: `test` false, every other predicate free, test-only exactly when no choice builds it; unparseable or more than sixteen free predicates reads as shipping. Pinned by `ban_scan::a_cfg_that_only_spells_test_or_says_not_test_is_read_as_shipping` (twelve shipping attributes and eight test-only ones, each through both scans' lexer); the text rule it replaced read six of the twelve as tests.
- *Item 3:* `test_only_files` reads declarations a token at a time from the lexer's new `bare` view (comments and char literals blanked, strings and test items kept), so the one-line form, a cfg split from its `mod x;` by a comment, a block comment or a blank line, and a `#[path]` before the cfg are all found; a quoted or commented-out declaration declares nothing. It finds the same thirteen test-only files the line reader did on today's tree. Pinned by `ban_scan::a_test_only_module_is_found_however_its_declaration_is_laid_out`; the line reader finds one of its seven.
- *Item 4:* the real-path exemptions are exact lines spent once each (`era_corpus::REAL_PATH_LINES`): the media chooser's real default (its per-thread test root was the second until the review round, which reads it as the test code it is), and by the same rule the config chooser's `.config` and the XDG user-dirs file's, which were file-wide; an exemption whose line is gone fails too. Pinned by `era_corpus::a_second_media_root_beside_the_chooser_fails_the_scan_when_planted`; the fragment rule passed four of its six plants.
- *Deviations:* tangent-ui got no feature of its own: its three roads go through tangent-core's, so one feature covers both crates, and tangent-ui turns it on from its own dev-dependencies. The two tests that deleted the real `cheat.unlock` had been on temp stores since W1 (`with_license`); nothing to change there, and under the redirect even `LicenseStore::load` in a test lands in the scratch home. TANGENT_CONFIG_DIR, TANGENT_MEDIA_ROOT and IVORY_SETTINGS_PATH still win in every build, test builds included (developer switches); check-home.sh unsets them for the gate's runs. The two `#[ignore]` probes in desktop.rs that read the owner's real projects by `$HOME` (`the_eligibility_probe_over_real_projects`, `the_master_has_no_discontinuities`) still do, read-only and only when asked for by name: reading his work is their purpose.
- *Touched beside the lane's files:* `tangent-ui/src/app.rs` (`dirs_home`), `tangent/src/desktop.rs` (three roads), `tangent/src/main.rs` (one road and the appended `home_tests`), `tangent-ui/src/era_corpus.rs` (the scan item 4 names lives there), both lock files (tangent-ui's `dirs` line), and the new `scripts/check-home.sh`.

**Recorded by the W8-safe-tests review round (2026-10-10, six upheld findings):**
- *Test code, read one way (items 2 and 4):* the real-path scan (`era_corpus::real_path_spellings`) still skipped a column-0 `mod` block under any `#[cfg(` line holding the letters `test`, the rule item 2 took out of the other scans, so a `#[cfg(not(test))]` or `#[cfg(feature = "latest")]` module, which ships, was skipped with a second `.join("Tangent Media")` in it. It now reads the lexer's new `live` view (comments, char literals and every item under a cfg `is_test_cfg` reads as test-only blanked, shipped strings as written), the reading the ban and copy scans use. Pinned by `era_corpus::a_second_media_root_beside_the_chooser_fails_the_scan_when_planted` (four shipping module forms and four test ones planted beside the chooser as settings.rs writes it); revert check: the line reader passes the `not(test)` module.
- *tangent-core's own integration tests (item 1):* `tests/*.rs` link the ordinary library, and nothing turned `test-home` on when tangent-core was tested alone (check.sh's two `cargo test -p tangent-core` legs), so `config::home`, `dir` and `file` were real there; none of those tests reaches a home path today. tangent-core now dev-depends on itself with the feature (the lock file records the self edge; the plugin's and the two tools' lock files are unchanged, `--locked`). `check-home.sh features` reads every member's test build alone and fails on one that links tangent-core without it. Pinned by `tangent-core/tests/test_home.rs`; revert check: without the line it fails under `-p tangent-core`, and `features` names tangent-core's test build.
- *The real folders while Tangent runs (item 1, two findings):* a change there was a note whenever a process named `tangent` ran, which on this machine is most of the time, so that half of the proof was off when it mattered. It fails the run now, Tangent open or not, and says to close it: the app's writes cannot be told from a test's without opening the owner's files. One file is the running app's alone and is noted instead: its log (`tangent.log` and its `.1`), modified while Tangent runs, which only `main` attaches (on `config::dir`, a scratch folder in a test build). The first gate run under the stricter rule failed on exactly that: the owner's open Tangent logged an audio dropout mid-run (read back, one line, `tangent::desktop`), the gate's own load being the likely cause; the suite itself was green and its scratch home untouched. `scripts/check-home.sh selftest` (first in check.sh, about two seconds) holds `run` to its rules with fourteen stub commands in a home of its own, a `sleep` standing in for the app (`CHECK_HOME_APP`); revert checks: the old note passes four runs it wants failed, the log's exception widened to whatever the app touched passes two, and no exception fails the log-only run.
- *Home roads past `dirs` (item 1, two findings):* std's `home_dir` compiles without a warning on the pinned toolchain, so the record's "a second road is a compile error in a shipped build" held for `dirs` only (corrected above), and a home variable's name held in a `const`, split from its read by rustfmt, or read through `env!`, `getenv` or `vars()` passed the per-line `var` rule. `home_roads_in` now refuses thirteen home-finding crates by path, `home_dir` and the password-database and platform folder calls as words, and a shipped string that is a home variable's name wherever it stands (the variables now include APPDATA, LOCALAPPDATA and the XDG folders). Pinned by `ban_scan::a_home_found_off_the_one_road_fails_the_scan_when_planted` (61 plants, every list entry among them, and the split read at the name's line); revert checks: the per-line rule misses the split read and six plants, and with the lists cut back to `dirs` as well, 37 of the 61.
- *Deviations:* item 4 named two settings.rs lines, the chooser's real default and `test_path()`; the second is under `#[cfg(test)]`, which the scan now reads as the test code it is, so its exemption would be spent by nothing (which the scan fails), and it is gone: a `test_path()` line in shipped code is refused like any other. The reviewers' alternative for the real folders, a note for Tangent's log and settings.json, was taken for the log alone: settings.json is the file every app a test builds saves, so a note there would hide the likeliest stray write. The home scan keeps one string by name (`NOT_HOME_READS`): the sampler browser's "HOME" label beside `media_home` in app.rs, a word and not a read; the plant that stood for it as a non-hit is a hit elsewhere now.
- *The gate on the round's last code (e03274d):* the workspace suite inside `check-home.sh run` (Tangent open throughout) 3994 passed, 0 failed, 112 ignored, nothing reached in the watched home and nothing changed in the real folders (their listing, sizes and mtimes, by `stat` alone, exactly as before); the Minimal check, clippy `-D warnings`, the firewall (39 ok), both Windows cross-checks and `palgen.py --check` green; the census green over 139 commits; check.sh's named legs 30 of 30 (`-p tangent-ui`) and 14 of 14 (`-p tangent`) by their exact names; `features` and `selftest` green.
- *Touched beside the lane's files:* `tangent-ui/src/era_corpus.rs` (the real-path scan), `tangent-core/tests/test_home.rs` (new), `Cargo.lock` (the self edge).

---

## 14. Owner calls (each has a default; nothing blocks on them)

| # | Call | Default | Alternative |
|---|---|---|---|
| 1 | Inserts per strip | Keep two racks of three (the INSERTS principle). `MAX_CHAIN = 2`. | A + rack is one UI stage plus widening `AutoParam::Plugin`'s u8 bay. |
| 2 | Folders and the free 32 | Folders count. | Exempt pure folders and gate every road that puts content on a folder. |
| 3 | Remove Track | Deletes the row. | Leave a hidden hole. |
| 4 | Bus solo | Solo-safe, and heard when a feeder is heard. | |
| 5 | SEND-BARGAIN | Stays on legacy channel routes, bit-identical. | Changing it changes saved projects' sound and needs a listening test with no benchmarks running. |
| 6 | Pad plugins in pre-lanes documents | Today's placement. | |
| 7 | Vacant legacy rows (pedal residue included) | Hidden and uncounted. | An explicit Tidy. |
| 8 | Unnamed track labels | By shown rank. | |
| 9 | Copy and the Konami mention | An in-app hint only ("a certain code"); no README or store mention. | Name it. This is one string set to flip. |
| 10 | Pre-fader sends | Offered, post-fader by default. | |
| 11 | Removing a sounding track | A 10 ms fade. | |
| 12 | Internal branch builds | A staging config and media root until W8. | Point them at real files with TANGENT_CONFIG_DIR/TANGENT_MEDIA_ROOT. |

---

## 15. Critique resolution map

All blockers were verified against the code at d821bb3. None was wrong. Five fixes are refined rather than taken verbatim (marked REFINED, with the reason).

### Engine and RT

**A1. `key_prev` was inside Lane.**
- *Verified in code:* `Renderer::key_prev` is shared through `DeskRef.key_prev` and `KeyFeed` in `render_lane`.
- *Resolution:*
  - The key snapshot, stems and dry stems live in the spine's `DeskBuffers`, sized by capacity and swapped by Grow.
  - They are written only in serial passes and borrowed only as `&` in the fork.
  - A Lane holds only state its own hand touches.
- *Tests:* a cross-hand keyed render at 33 and 257 lanes, and a scan that `key_prev` is not a Lane field (7.1 to 7.3; W1, W3).

**A2. Auto params behind `Arc<RackShared>`; PadShared inside `Arc<LaneShared>`.**
- *Verified in code:* `Renderer::resolve_automation` clears and pushes queues every chunk.
- *Resolution:*
  - LaneShared, RackShared and PadShared hold atomics only.
  - `ParamQueue`s live in the audio-owned Rack and are filled by the serial decide walk.
  - PadShared is its own Arc, owned by PadBank and cloned by the host (7.2; W3).

**A3. No op attached a lazily built part.**
- *Resolution:*
  - `EngineOp::Attach { slot, gen, part: LanePart::{Pads, Kit, Chain} }` is gen-checked and follows the room-first rule.
  - It is in the rt_alloc and alias matrices.
- *Test:* a lane becomes a drum lane mid-roll with plugins loaded: 0 reloads, 0 allocations (7.4; W1, W3).

**A4. Slot payloads raced Install; MonitorPlan picks had no gen.**
- *Resolution:* two rules (7.5).
  - Desk-wide clip, MIDI and automation lists carry `(slot, gen)`, are validated at USE, and are never dropped on arrival.
  - Per-lane parts ride the op ring or the lane's own rings, so FIFO puts them behind their Install.
  - Gen also goes on MonitorPlan picks, stem arms, punches and tap aims.
- *Test:* Install delayed one block, audible within 2 (W3).
- *REFINED:* the suggested re-send after an Install ack is unnecessary under these two rules.

**A5. Ordinal capture binding broke on adds during a record.**
- *Verified in code:* `desktop.rs:push_monitor_settings`' `next_pick` walk; `start_arr_record`'s `(pick, ch, buf)`; devices' `set_channel_picks` reopens the stream on any change.
- *Resolution:*
  - PickTable keyed by TrackId: an explicit binding gets the lowest free PickId and keeps it; ordinal mode is kept only for files that never named an input.
  - ArrRec binds `(TrackId, slot, gen, PickId)`.
  - Capture rings are per pick on the op ring and never replaced while capturing; MonitorPlan carries no rings.
  - `desk_shape_held` makes inserts, removes, reorders, kind changes and binding changes wait during a record or realtime export, and Add appends (W5, W6).
- *REFINED:* holding every AUDIO or MIDI add is unnecessary, because an append moves no row and no pick.

**A6. The take-once guard was deleted.**
- *Verified in code:* `LaneCells::take`.
- *Resolution:*
  - `apply_ops` refuses a plan that repeats a slot, names an empty slot or carries a stale gen. The check is O(N) via a capacity LaneSet; the old plan keeps playing, and the refusal is counted.
  - Each Lane carries a round stamp that the claiming hand swaps, so a duplicate answers None (7.4, 7.6; W1, W3).

**A7. Bus, folder and return racks ran serially after the fork.**
- *Verified in code:* `Renderer::fold_folders` and the sends pass.
- *Resolution:*
  - DeskPlan carries topological levels. Each level's live racks run through `run_list` with per-hand scratch.
  - Inputs are summed and outputs published in today's transcribed order: bit-exact, with cycles in a final serial level and Fx and master serial.
  - Rungs: 50 returns x plate, 25 folders x 3 inserts (7.8; W1 data, W6 execution, W7 measurement).

**Lesser notes adopted:**
- (a) a per-lane `rendered` flag, cleared serially;
- (b) both `pending_monitor` and `pending_monitor_layout` move through rings in W3;
- (c) the bridge writes every live plan's gains;
- (d) per-lane mail counters and a dirty list for automation queues;
- (e) freeze = `full_build(Freeze)` plus a one-slot stem tap;
- (f) grain phase keyed by slot, with legacy rows at `Slot(r)`.

### Persistence

**B1. Whole-file refusal.**
- *Verified in code:* `Settings::load_with_reader` moves an unreadable file aside and starts a first launch; `Settings::from_json` falls back to defaults.
- *Resolution:* referential errors drop the entry with a notice; structural errors refuse only the desk.
  - settings: the default desk plus a `settings.desk-refused.json` side file;
  - document: the open is refused and the file untouched;
  - ~~DAW blob: the raw desk is kept under `extra.desk_refused`.~~ Removed 2026-10-10 with the plugin crate (see 6.3).
  - It never routes to `unreadable` or to the defaults fallback.
  - `DeskModel::check` runs in the sentinel (6.3; W2, W3).

**B2. A v8 document beside a legacy-only `state/`.**
- *Verified in code:* `seed_template`'s heal calls `fixed.save_to`; the host save roads write the document first; tangent-ui save roads cannot write pstate.
- *Resolution:*
  - The durable `legacy_states` marker gives a per-SeatBay legacy fallback through provenance + `encode_v32(Row(id))`, with the owner check.
  - The marker is cleared only after a verified mirror, by a one-key JSON patch.
  - `seed_template` heals by JSON patch.
- *Tests:* a kill between the document and the mirror on all eight roads (6.6; W3, W4).
- *REFINED:* "save_to never upgrades" is applied to `seed_template` only, because the marker makes an upgrade on any other road safe and forbidding upgrades everywhere would stall migration.

**B3 (= C1). Lane-keyed app state outside DeskModel.**
- *Verified in code:* `reorder_channels` and `carry_automation_rows` remap about 20 fields by hand.
- *Resolution:*
  - Rekey by TrackId in the new lane W4-lane-refs, and by SeatBay in W5.
  - `on_desk_shape_change` for transient aims.
  - The field-classification scan.
  - Width tests remove a row above the drum panel and above a latched row (section 4; W4, W5, W6).

**B4. drumpad.json.**
- *Verified in code:* `DrumpadDoc::save_all_beside` writes positional `lane_kits` and `primary_lane` on every kit edit.
- *Resolution:*
  - The writer is keyed to WRITE_V8: `track_kits` through `track_ids`, plus `primary_track`; legacy keys are never written.
  - Legacy keys are honoured only without `track_kits`.
- *Tested from W3 (6.8).*

**B5. The global shelf conversion was not crash-safe.**
- *Verified in code:* the shuffles run after `open_project_at_launch` behind in-memory flags; `load_with_reader` saves at load.
- *Resolution:*
  - No in-place renames in new builds.
  - `legacy_shelf_seat` composes the frozen renames plus v24.
  - Copies are temp + rename, copy-if-absent, with the marker written last.
  - The source comes from `.shelf-v23.pending`, written before the first format-2 settings write.
- *Tests:* kill points; this also covers the Minimal build (6.6; W1, W3, W4).
- *REFINED:* the marker stores the SHUFFLE SET, not the stamp, because v19, v20 and v24 are raised by the desk's sends length, not by `settings_version`.

**Lesser notes adopted:**
- removed lanes' states are written before drop (no DisplacedShelf eviction risk);
- STAGING;
- `build_template` exclusions;
- `holds_only_scratch` accepts `.pre-v8`;
- pedal-only legacy rows are Vacant and never reused;
- the Reset Everything test is restated.

### Plan and process

**C2. The engine mirrors tangent-ui constants.**
- *Verified in code:* `instrument.rs:RACK_SEATS`, `CHANNELS`, `INPUTS`, `STRIPS`; 69 uses in desktop.rs.
- *Resolution:* W4-integrate (C) removes every `tangent_ui::recorder` capacity constant from `tangent/src`, with a scan.

**C3. Plugin crate tests never ran.**
- *Verified in code:* check.sh only runs `cargo check` on the plugin.
- *Resolution:* the DAW blob fixtures and round trips live in tangent-ui's `era_corpus.rs`, and the plugin step stays `cargo check`. (The plugin crate and its step were removed on 2026-10-10; the fixtures stay.)

**C4. The Reset Everything test could not be checked as written.**
- *Verified in code:* `reset_settings(Everything)` raises `NewKeeping` and calls `relock_cheat`.
- *Resolution:* the test is restated in the tangent crate through Bridge: save first, reset, reopen. W1 asserts the relock and NewKeeping only.

**C5. Internal builds were not isolated.**
- *Verified in code:* `media_root` is independent of the config dir.
- *Resolution:*
  - STAGING lands in W1, redirecting both the config dir and the media root, with the legacy pin off.
  - Backups trigger on the first format-2 write.
  - Stamps derive from WRITE_V8.
  - STAGING is deleted in W8.
- *REFINED:* automatic redirection replaces "refuse to start without it". It is equally safe, needs no ritual, and real files remain one explicit override away.

**C6. The copy check was too weak.**
- *Verified in code:* stale lines in `tools/tangent-fulfil/src/main.rs`, STORE-CONTENT and README.
- *Resolution:* a phrase-list scan (W3, then check.sh in W8) plus the W8 owner checklist: fulfil redeploy, Gumroad, website.

**C7. W4 had no export baseline.**
- *Resolution:* W2-engine-bridge extracts `furnish_offline` and commits export goldens. W3 keeps them byte-identical; W4's `full_build` matches them.

**Lesser notes adopted:**
- the PACE probe is replaced by a headless repaint-causes test;
- path-qualified W8 gates;
- Minimal is covered by the pending marker;
- FreezeJob is bound to TrackId;
- `about_lines(caps, supporter)` with plumbing in W1;
- the Konami wording default is a single string set to flip;
- `MAX_CHAIN = 2` until owner call 1.

---

## Appendix A: frozen V32 seat map (flat bay = seat * 3 + bay)

| Seats | Owner |
|---|---|
| 0..=31 | Row(c) Chain(0) |
| 32 / 33 / 34 | Drums / Click / Fx, Chain(0) |
| 35 | Master Chain(0) (`MASTER_SEAT`; persisted bays 0..2, DISPLAYED as slots 3..5) |
| 36..=163 | Row(0) Pad(seat - 36) (legacy pad block) |
| 164 | Master Chain(1) (`MASTER_EXTRA_SEAT`; persisted bays 3..5, DISPLAYED as slots 0..2) |
| 165..=4132 | Row(1 + (seat-165)/128) Pad((seat-165)%128) |
| 4133..=4164 | Row(seat - 4133) Chain(1) (folder-extra) |
| 4165..=4388 | Row((seat-4165)/7) Pedal((seat-4165)%7) |
| 4389 / 4390 / 4391 | Drums / Click / Fx, Chain(1) |

- `TOTAL_SEATS = 4392`, `INSERT_SPACE = 13176`, `HEAD_BAYS = 108`.
- Strip space (colors and names 36, sends 35, mute/solo/phase u64 bits): 0..=31 rows, 32 drums, 33 click, 34 fx, 35 master.

## Appendix B: frozen legacy global-shelf shuffles (`legacy_shelf_seat`)

Applied to the seat; the bay is unchanged. Composition order: v13, v14, v18, v19, then v20 or v24. v20 and v24 are exclusive.

| Shuffle | Raised by | Map |
|---|---|---|
| v13 | the BACKING strip retired (pre-5.0) | seat 8 dropped; 9, 10, 11 → 8, 9, 10 |
| v14 | the drum bus at 8 (pre-5.0) | 8..=10 → +1 |
| v18 | was < 18 (pads to 128) | 12..=27 → 48..=63 |
| v19 | sends 11 (8 → 16 channels) | 8..=139 → +8 |
| v20 | sends 19 (16 → 32) | 16..=2068 → +16 |
| v24 (NEW) | sends 27 (24 → 32) | 24..=3100 → +8 |

The V24 map has 24 channels: drums 24, click 25, fx 26, master 27, ch0 pads 28..=155, master-extra 156, and extra pads from 157 + (c-1)*128 to 3100. No folder, pedal or bus extras existed then.

## Appendix C: text grammar (one grammar everywhere)

```
node    := "t" <u32> | "drums" | "click" | "fx" | "master"
rack    := "c" <u16> | "p" <0..=6> | "n" <0..=127>
seatkey := node "-" rack
seatbay := seatkey "-b" <0..=2>
```

Canonical decimal, no leading zeros, lower case. A string is accepted only if `format(parse(s)) == s`.

Examples: `t3-c0-b1`, `t12-n36-b2`, `t0-p4-b0`, `master-c1-b2`, `drums-c1-b2`. Sidecar: `pstate-<seatbay>.bin`, body `<bundle path> NUL <TGST state>`.

## Appendix D: format-2 desk

```json
"desk": { "format": 2, "next_id": 41,
  "legacy_states": "v16",
  "rows": [ { "id": 0, "kind": "midi", "added": true, "icon": 3, "name": "Piano", "color": 0,
              "gain": 0.8, "pan": 0.0, "input": "midi:all", "midi": {"devices": [], "channel": null},
              "mute": false, "solo": false, "phase": false, "arm": true, "theory": true,
              "folder": false, "in_folder": false, "shut": false,
              "sends": [ {"to": "fx", "level": 1.0, "pre": false} ],
              "dx7": {"cart": "", "patch": 0}, "keys": {"patch": 0, "params": [64 floats]},
              "frozen": null } ],
  "specials": { "drums": {}, "click": {}, "fx": {}, "master": {} },
  "plugins":  { "t0-c0-b0": "@tangent-dx7", "master-c1-b2": "/Library/.../X.vst3" },
  "bypass":   [ "t3-c1-b0" ],
  "routes":   [ {"from": "t3", "to": "t7", "gain": 1.0},
                {"from": "t2", "to": "hw", "first": 4, "stereo": true, "gain": 1.0} ],
  "keys":     [ {"seat": "t5-c0-b1", "from": "t2"} ] }
```

- `legacy_states` appears in documents only, and only until a verified mirror.
- Rows are written in lane order, vacant rows included.
- Scalar desk keys that were never strip space keep their keys.
- Curves: `{"lane": row}` for rows, `{"target": "master"}` for the master.
- drumpad.json: `"track_kits": {"t<id>": kit}`, `"primary_track": "t<id>"`.

## Appendix E: era table (every reader keeps opening these)

**settings.json and DAW blobs (`plugin/src/lib.rs #[persist = "settings"]`; the plugin crate was removed on 2026-10-10, and its blobs stay in the era corpus as `Settings::from_json` fixtures):**

- stamp ≤ 18 (pre-5.0; only a hand-copied file can carry one, since 5.x never reads the 4.x folder): the frozen `migrate_from` arms, with was<16 and was<17 growing to the ERA width; then lift by sends length.
- v19, 19 sends (16 ch): V16, strip masks including phase; shelf v20.
- v20, 27 sends (the 24-channel window): V24; shelf v24.
- v20 to v22, 35 sends (v21 head+tail): no lift.
- Then `from_v22(identity ids)`, `next_id = 32`.
- v23: `desk.format == 2`.

**Documents and templates:**

- v1..v3: no desk (the live desk stands).
- v4/v5: 11 / 18 / 19 sends (the 84-bay pad sub-era → V8Pads16).
- v6: 27 or 35.
- v7: head+tail.
- v8: format 2.
- Master curves via `master_auto_lane`.

**Shelves:**

- global: the composed map (Appendix B), then `decode_v32` + identity ids → pstate names, by copy;
- project and template `state/`: pstate first, then legacy names through `legacy_states` + provenance; never renamed or deleted.

---

## Risk register (ranked by how badly each could lose data or silently change sound)

1. **Plugin state lost or attached to the wrong plugin during migration. HIGHEST.**
   - *Causes:* era misdetection; lift and translation disagreeing; the master's two bay orders mixed; an interrupted first v8 save; a v8 document beside legacy-only `state/`; a crash during global conversion.
   - *Mitigations:*
     - lifts are generic over LiftBay, so provenance runs the reader's own code;
     - `decode_v32` is proven bijective against the live seat functions before they are deleted;
     - `slot_key` and `auto_key` are the only resolvers;
     - the durable `legacy_states` fallback, cleared only after a verified mirror;
     - a copy-only global conversion through a composed frozen map, with temp + rename and a durable pending source set;
     - legacy files are never renamed or deleted;
     - backups on the first format-2 write;
     - one switch flips every format-2 side effect;
     - kill-point tests on every save road.
   - *Residual:* a 24-window file, or a v19 phase-inverted bus, already re-saved by a 32-channel build carries that save's damage.
2. **Lane aliasing gives wrong audio without a panic.** Release builds have no overflow checks.
   - *Mitigations:* a one-change purge behind a ban scan in W2 before any growth; alias harnesses at word edges and at 255/256; CHANNELS is never raised as a constant.
3. **The audio thread allocates, frees, locks, aliases or sees a half-built lane.**
   - *Mitigations:*
     - the ownership rule (lanes own only their hand's state; desk-wide buffers serial-written and `&`-borrowed);
     - atomics-only shared state;
     - Attach for lazy parts;
     - FIFO ops with retire room first;
     - plan validation plus the claim stamp;
     - gen-at-use;
     - monitor and capture handoffs through rings;
     - rt_alloc over every op at 33, 65, 200 and 1000 lanes.
   - The cfg(test) global allocator itself is ported verbatim from tangent-host.
4. **Bit-exactness drifts in the 35k-line renderer.**
   - *Mitigations:*
     - aarch64 goldens (render and export) captured before change;
     - fork/serial, grain/chunk and level-parallel/serial invariants on every arch;
     - the rank rule;
     - grain phase by slot with legacy rows at `Slot(r)`;
     - transcribed sum and publish orders;
     - two checkpoints in W3;
     - sound changes only on the Bus kind.
   - *Residual:* goldens run on ARM only.
5. **W4 is the largest change.**
   - *Mitigations:* split into integrate (host) and lane-refs (app), with disjoint files; the bridge harness from W2 and the shims from W3; three checkpoints; export goldens; relaunch from the last checkpoint after a session-limit death.
6. **The paywall leaks, or Konami users end up capped.**
   - *Mitigations:* one gate on `is_supporter()` (scan); every road walked at a forced cap; the undo sentinel; the full Konami suite with the restated reset test; fixtures in both crates.
7. **A licence ever hides or silences music.**
   - *Mitigations:* persistence, the engine and drawing are tier-blind (scan); a free app opens, plays and saves a 200-track project whole.
8. **A dangling reference or a bad desk costs a whole file.**
   - *Mitigations:* the 6.3 error policy; `DeskModel::check` in the sentinel; side files for refused desks; ~~the DAW blob keeps the raw desk~~ (removed 2026-10-10 with the plugin crate).
9. **Session state lands on the wrong track after a structural edit.**
   - *Mitigations:* TrackId/SeatBay keys; `on_desk_shape_change`; the classification scan; width tests per field.
10. **Inputs: a wrong layout records the wrong device column with plausible audio.**
    - *Mitigations:* the stable PickTable; identity-bound arrangement records; rings never replaced while capturing; shape holds during records; fake 8- and 64-input devices; rings never smaller than the graph quantum.
11. **Internal builds touch the owner's release install.**
    - *Mitigations:* STAGING redirects config and media from W1; stamps derive from WRITE_V8; real files only by explicit override.
    - *Residual:* the first post-W8 install migrates real files, covered by the backups and by legacy files never being touched.
12. **Performance at large N.**
    - The serial decide/fold floor is about 0.45 us per bare lane, about 460 us of 2667 us at 1024 lanes and 128 frames.
    - Serial bus, folder and return racks are another floor.
    - *Mitigations:* level-parallel post-fork racks; a dense active list; per-hand scratch; rings sized by use; DeskIndex windowing; measured rungs including 50 returns and 25 folders; a 5% budget on today's shapes.
13. **Merge conflicts on app.rs (80k lines), desktop.rs (37k) and instrument.rs (35k).**
    - *Mitigations:* one heavy lane per file per wave; new-file kernels; function-level fences; MERGE-ME stubs; contracts C1 to C12; waves cut from the merged tip.
14. **Test churn weakens coverage.**
    - *Mitigations:* the census with replacement ledgers; parametrized widths that keep 32; literal era goldens; owner-ruling pins kept verbatim; revert checks.
15. **The plugin crate and the Minimal build.**
    - *Mitigations:* DAW blob tests in tangent-ui; the plugin `cargo check` in the gate; Minimal covered by the pending marker and its compile check. (The plugin crate was removed on 2026-10-10.)
16. **Copy contradicts itself across surfaces.**
    - *Mitigations:* the phrase scan; the W8 owner checklist (fulfil redeploy, Gumroad, website).
17. **Behaviour changes the owner will see** (Remove deletes the row, folders count, the pedalboard leaves with its row, pad FX follow their lane, vacant rows hidden, a trailing add, the removal fade, Add appends during a record, the copy).
    - *Mitigations:* each has a default and is listed in section 14.
18. **Undo across structural edits.**
    - *Mitigations:* moments carry rows with ids; removed lanes' states are written to pstate before drop; id-keyed session state comes back with the row; undo and redo tests at width 200.
