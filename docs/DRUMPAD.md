# The Drumpad — design and integration notes

Built on the `drumpad` branch, in a separate worktree, against `main` as of
"Every input of every interface" (4.48.0-era). Written for whoever merges
it — the session that owns the arrangement and the coming media manager.

## What it is

Sixteen one-shot sample pads in a floating popup, modelled on the RS5k
manager's rack. `W` raises it over whichever view is open (gated on the
band, because the engine lives and dies with the band); Esc, the ×, `W`
again, or a press on the world outside puts it away. It is a POPUP in the
MIDI editor's exact family — same face, ink, strip, chips and close glyph —
and it is draggable by its title strip.

- **Pads count up from the BOTTOM-LEFT at note 36 (C2)** — the MPC's
  reading order and the General MIDI kick. Pad *n* answers note `36 + n`.
- **Samples arrive by dropping.** A file from Finder onto a pad loads it;
  N files land on N consecutive pads; a drop on an occupied pad replaces
  its sample and keeps the knobs. The file is copied into the project's
  `media/` like every import, and the kit is saved in a `drumpad.json`
  sidecar beside the project document — a project folder stays
  self-contained, and old projects (no sidecar) open with an empty kit.
- **A pad strikes on the press** (velocity 112), selects for the inspector,
  and lights from the AUDIO — the engine's own peak — so a hardware strike
  shows exactly like a mouse one. Strikes land in a rolling take's `.mid`
  via `capture_app_midi`, like auditions.
- **MIDI channel 10 is the pads' address.** While a kit is loaded, the
  engine diverts channel-10 notes to the pads and they never reach the
  lanes (a kick that also plays a piano C is a surprise, not a layer).
  With no kit loaded the diversion never happens, so nobody's channel-10
  rig is eaten. While the panel is up with its MIDI chip lit, live notes
  36–51 from the keyboard are ALSO mirrored onto channel 10 host-side, so
  a controller plays the pads without any routing ceremony.
- **Per pad**: gain, pan, tune (±12 st, varispeed), and a choke group
  (–/A/B/C/D; pads sharing a group cut each other — the hat pattern). Per
  kit: one master fader. Knobs drag vertically (150 pt = full travel,
  Shift = fine), double-click resets. A retriggered pad self-chokes
  through a ~5 ms stealing fade, so rolls never turn to mush and cuts
  never click.

## The machinery (where everything lives)

| Piece | File | Notes |
|---|---|---|
| Panel painter | `tangent-ui/src/drumpad_panel.rs` | pure painter + `Layout`/`targets`-style `hit_test`, geometry tests |
| Kit document | `tangent-ui/src/drumpad.rs` | `drumpad.json` sidecar, write-then-rename, lenient load, tests |
| Shared contract | `tangent-ui/src/recorder.rs` | `DRUM_PADS`, `DRUM_PAD_BASE`, `DRUM_PAD_VELOCITY`, `DrumPadParams`, `DrumKitParams`, `RecorderRequest::PadHit` |
| Host→UI status | `tangent-ui/src/ports.rs` | `PadStatus { loaded, missing, wave }` |
| App wiring | `tangent-ui/src/app.rs` | state fields by `midi_edit`'s; the method cluster sits just above `close_midi_editor`; draw call after `draw_setup`; `drumpad_input` runs before `midi_editor_input` |
| Key | `tangent-ui/src/keys.rs` | `W` → `ToggleDrumpad`, gated on `recorder_shown`, clustered with Enter/Space at the table's end |
| Engine | `tangent/src/instrument.rs` | `PadKit` (clips' swap protocol exactly), `PadVoice`/`PadPlay`, channel-10 branch in `collect_notes`, `mix_pads` between `mix_monitor` and the click, `Shared` pad atomics, `Engine::{set_pad_kit, set_pad_params, pad_peaks}`; render tests at the bottom |
| Host | `tangent/src/desktop.rs` | `reload_pads` (+ its own `pad_cache` — `track_cache` evicts by what the ARRANGEMENT references), `PadHit` arm by `Audition`'s, params push in `push_monitor_settings`, peaks read in `after_frame`, the live mirror in the `pump_midi` closure |
| Screenshot | `tangent/src/composite.rs` | `shot::drumpad` — `IVORY_SHOT=/tmp/dp.png cargo test -p tangent --bins shot::drumpad -- --ignored --nocapture` |

The seams follow the house pattern end to end: the UI marks
`drumpad_dirty`, the host drains `take_drumpad_dirty` and re-decodes;
`PadHit` goes through `RecorderRequest`; knobs are pushed whole every
frame beside `set_desk`; the pads sum onto `mix` upstream of the bus
return, the room split and the master insert, so they are limited,
mastered, heard and taped like every other source, and the audio thread
never allocates, locks or frees a sample.

## The media manager's half of the bargain

`drumpad_panel.rs` declares the drag payload:

```rust
pub struct DraggedMedia(pub std::path::PathBuf);
```

When the media manager starts an internal drag, it calls
`egui::DragAndDrop::set_payload(ctx, DraggedMedia(path))`. That is ALL it
has to do. While a payload of that type is in flight the drumpad
highlights the pad under the pointer, and on release it takes the payload
(`DragAndDrop::take_payload`) and loads the pad — same code path as an OS
drop, copy-into-`media/` included. If the manager wants other drop
TARGETS later (lanes, bays), the same payload type should stay the
crate-wide currency for "a media file being dragged".

## Merge notes for the other session

- `instrument.rs` was untouched by 4.48's work, so the engine half should
  merge clean. The app half touches `app.rs`, `keys.rs`, `recorder.rs`,
  `ports.rs`, `lib.rs`, `desktop.rs`, `composite.rs` in small, commented
  blocks — each anchored beside an existing landmark named in the table
  above.
- If `W` has been claimed in the meantime, `keys.rs`'s uniqueness test
  will say so on merge; the free letters at branch time were J, Q, X, Y.
- One keys test was extended (`the_card_does_not_list_space_...`): the
  band now adds THREE rows (W, Enter, Space), not two.
- The drumpad stands down for nothing and everything stands down for the
  drumpad: `drumpad_owns` mirrors `midi_edit_owns`, the arrangement's
  drop handler explicitly refuses drops inside the panel (drops can
  arrive with no interact position, so the pointer stand-down alone is
  not enough), and one Esc closes only the topmost popup.

## Known slack, written down on purpose

- **No sustained pads**: obey-note-off is OFF and not yet a control; pads
  are one-shots. `PadHit` already carries the release edge, so obey-off
  pads need no protocol change.
- **No per-pad send** to the effects bus; the kit rides dry (through the
  master insert). A kit-level send is the natural first follow-up.
- **No sample-start/end trim, no ADSR, no layers, no kits/database** —
  RS5k manager features deliberately out of v1.
- **Choke reads as classic groups** (A–D), not RS5k's per-pad
  "which pads cut me" table.
- **The panel's position is session state** (not saved), as is
  `drumpad_open` and the MIDI chip.
- A kit swap silences all sixteen voices for one block (documented and
  tested); a replace-one-pad load therefore cuts a ringing crash. Nobody
  will notice while loading samples, but a per-pad swap would fix it.

## v2 (4.50.0)

The pad window doubled in width for a MEDIA EXPLORER pane on the right:
browse the filesystem (`..` walks up), drag an audio row straight onto a
pad (`DraggedMedia`, the same drop contract). The inspector row gained
three FX bays and a pan per pad — the bays ride the desk's insert space
at `pad_rack_index(pad) = STRIPS+1+pad`, so loading, editors and state
sidecars are the desk's own machinery; each pad renders through its rack
(live racks keep processing silence so tails ring out), the sixteen sum
through the DRUM BUS rack and fold with `drums_gain/drums_pan`. The bus
itself is a desk strip now (`Strip::Drums`, settings v14, left of the
click) — its fader, pan, mutes and inserts live on the desk, not here.
The volume slider answers horizontal drag (it is horizontal); the
inspector knobs read value-top/label-bottom like every dial in the app.
