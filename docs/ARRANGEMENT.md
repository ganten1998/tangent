# The Arrangement View — design draft

Drafted with the owner, 2026-08-22, question by question. Everything in
"Decided" was answered explicitly; everything in "Proposed" is the draft's
own filling and is flagged for veto before implementation.

## Decided (the owner's answers)

**Placement.** The arrangement lives where the mixer lives: it toggles over
the piano area while the band stays on top. One keystroke cycles
piano / mixer / arrangement.

**Lanes are the desk.** Lane N *is* channel N — eight lanes for the eight
general channels, sharing name, colour, kind, arm, mute and solo with the
mixer strip. The click and the master have no lanes. Eight fixed,
equal-height lanes in v1, **with the caveat that a variable count may come
later** — nothing may bake "8" in so deep that growing it is a rewrite.

**The BACKING strip retires.** The desk becomes 8 general channels + click
+ master (the backing track's job is now "an audio clip on a lane"). The
default desk stays: DX7 on channel 1, seven empty.

**Track headers (left column), from the owner's REAPER theme.** Medium
density: track number, record-arm dot, name, a small volume knob, a slim
meter, and M/S. The FX chip and input-selector row from the REAPER theme
stay the mixer's job.

**Ruler.** Bars & beats, counted from the tempo and time signature the band
already owns. The clock readout still says minutes:seconds.

**The band's scrubber (replaces the backing-track box).** Ableton's
arrangement overview, studied: the WHOLE song in miniature — each lane a
thin bar of clips in its channel colour — with the playhead through it and
a bright viewport rectangle marking the span the arrangement view shows.
Click = locate. Drag = scrub. The viewport rectangle can be dragged and
stretched, and the arrangement follows it.

**Zoom and scroll — both idioms.** Wheel/trackpad scrolls time. Dragging
DOWN on the bars ruler zooms in around the pointer, UP zooms out
(Ableton's gesture), AND Cmd/Ctrl+wheel zooms around the pointer. The
scrubber viewport is the third way.

**Arm on the mixer.** The mute/solo row becomes three switches: **M S R**,
R lit record-red when armed. The same row appears on the arrangement
header (the header's arm dot and the strip's R are one state).

**Recording is TWO separate functions, never entangled:**

1. **Arrangement record** — the round red transport button. Records ARMED
   lanes into the arrangement as clips starting at the playhead; unarmed
   lanes play back. This is the DAW half.
2. **Capture** — the teaching/content tool, a visually distinct control
   beside the transport (not the red button). Starts/stops the existing
   whole-take machinery: "record whatever happens" — window video, all
   audio, the `.mid` — completely independent of the arrangement. Capture
   over a playing or even recording arrangement is legal and changes
   nothing about it.

**2026-09-22, the ruling that keeps them apart.** A take is capture only.
The cassette starts and ends it and nothing else does: it rolls no
transport, Stop and Space do not end it, and the band's transport, the
loop, the chord audition, undo/redo (once its count-in is over) and track
moves all work while it runs (the owner: "the main buttons need to be
intact while recording takes"). The two machines share ONE count-in in the
engine, which is the one place they touch: a take pressed during the red
button's count joins it from where it began, the red button pressed during
a take's count joins that one, and Play during either starts the song on
the downbeat (Pause or Space takes a waiting Play back, and the count goes
on). A take that counted on that downbeat is told the song started there,
so its `take.json` places it even with its count in the file
(`timeline_rolling` with `head_seconds`). The realtime EXPORT is the one
take that owns the transport, the lane order and the folders; those
refusals key on `exporting` (the host's `bounce`), not on the take state.
Every road to the lane order refuses under an export or an arrangement
record, which both bind lane indices at their press: the lane drags (a
move, a snap into a folder, a drag home that ejects a track), Insert Track,
Paste Track Below and Cmd/Ctrl+F grouping each ask `lane_order_held` before
they change anything, and it says why. Under an export Cmd/Ctrl+F makes and
dissolves no folder either (`folders_held`): the export chose its stems by
the folders at its press. No track is removed under an export or an
arrangement record, by any road (`tracks_held`): a removal empties a row
the export or the record is writing, and renumbers the input picks the
record captures by. And no road replaces the session while a take counts
in or an export runs (`session_held`): a take rolls on through New and
Open, but the incoming desk's tempo would retime the click under its
count, and an export plays this project into this project's folder.
Settings > Reset > Devices and Folders, which puts the tempo, the signature
and the count-in back with the devices, waits while the tempo is held (a
take's count-in, the arrangement's record count, or an export) for the
same reason. So does the Undo a reset's box offers, whatever the scope,
since it puts every setting back, the tempo included; the box comes back
with its Undo, so it can be pressed once the hold ends. A frozen track
stays frozen under a realtime export (`freezes_held`): a thaw puts the lane
back on its clips and rack in place of the file the export is playing.
Routes are not added or removed, and no bay's sidechain is re-keyed,
under one either (`routes_held`): each moves a lane's signal onto another
destination partway through the file, as a folder's bus does; a route's
gain is a fader and still rides. And Settings > Reset > The Desk, which
empties every channel in place, waits as Remove Track does, under an
export or an arrangement record.

**Clip operations, v1, with their gestures (all owner-specified):**

- **Move** — mouse-drag the clip body along its lane.
- **Trim** — mouse-drag either end.
- **Delete** — the Delete key (selection first; same menu-not-deed care as
  the mixer where a gesture could destroy work).
- **Split** — the `s` key, at the playhead.
- **Fades** — drag DOWN-IN from a clip's top corners: top-left drags a fade
  in, top-right a fade out.
- **Edge-drag auto-scroll** — hold a drag at the edge of the field and the
  view goes that way, so a clip can be moved past what is currently visible
  without resizing anything first. Sideways it walks the window, downward it
  walks the lane stack; a track carried in the header column scrolls the
  stack too. Same law on all three surfaces (the MIDI editor's roll, the
  lanes, the mixer's desk): the speed ramps from a floor at the zone's inner
  lip to a ceiling at the rail and SATURATES past it, and it is measured in
  points per SECOND, so the same hold covers the same ground on any machine.
  View gestures (alt-drag pan, the navigator) are exempt by an allow-list —
  a view that also auto-scrolled would run away from the hand steering it —
  and so are the marquees, whose anchors are screen points.

**Import.** Drag a file from Finder and DROP IT ON A LANE — the clip lands
where it was dropped. That is the one way in; no picker, no menu row.

## Projects (owner's answer to the migration question — the bigger, right thing)

- **A project is a directory** holding the arrangement doc and a `media/`
  folder into which ALL referenced media is auto-copied — imported files
  and recorded clips alike. A project folder is self-contained: move it,
  zip it, hand it to someone, it opens.
- **The app reopens the arrangement you last had open**, exactly as it
  was — **except every record arm is OFF, save one: the default Tangent
  DX7 channel**. Arm is what makes a channel LIVE (see below), and a
  project must be openable without headphones and without being blasted.
  Arm state is therefore session state, never saved into the project.
- **Recorded and imported clip media lands in the current project's
  `media/`** (supersedes the earlier `record_root/arrangement/` call).
- First v13 launch with no project yet: a default project is created, and
  a remembered v12 `track_path` becomes an audio clip at 0:00 on the first
  unused channel (named BACKING, old track gain kept) inside it — nobody's
  backing track vanishes.
- **Every launch stands on the default template (2026-09-13).** The owner:
  "make it so every launch opens the default session, and doesn't open the
  previously open session." The remembered `project_path` is not reopened
  at launch any more; the last session is a File > Open away, and a
  materialised Untitled stays on disk for that panel, unless the session
  that made it real discards it with Don't Save while it is unnamed and
  clipless on disk and holds no recording (`rec-*`), no kit of its own and
  nothing Tangent did not write (`holds_only_scratch`): then it was that
  session's litter and is removed (`discard_scratch_made_here`,
  2026-09-22; see docs/FOLDERS-AND-SESSIONS.md). New keeps one too: it
  starts beside a scratch the discard refuses, and asks about a clean
  session's scratch with takes in it before reclaiming it (the New arm of
  `run_project_request`). The launch sweep (`sweep_disposable_untitleds`)
  also clears an Untitled that holds nothing it counts as work.
  Third answer to this question — the history, including the "moscow"
  scare that reversed the second one, is on `open_project_at_launch`.
- New / Open / Save As / Recent live under a PROJECT subject in the menu
  (the 18-row top-level budget holds; these are rows *under* a subject).

## Arm = live (decided by the "no headphones blast" rule)

Record-arm is also the LIVE switch: an armed MIDI channel sounds when the
keyboard is played, an armed AUDIO channel monitors its input. Unarmed
channels play back their clips and nothing else. "Ready to play" and
"ready to record" are one light — the R switch.

## Proposed (draft's own calls — veto anything)

- **Dropping audio on an EMPTY lane makes that channel AUDIO** and lands
  the clip. Dropping on a MIDI lane refuses with one status-line sentence.
  *(Approved by the owner.)*
- **What an armed lane records**: an AUDIO channel records its input pick;
  a MIDI channel records the notes played into it (same routing the
  monitor already uses).
- **Which channel opens armed**: the one whose bay holds the built-in DX7;
  if none does, the first MIDI channel carrying any instrument; if none,
  nothing opens armed.
- **Click in empty lane space = locate the playhead there** (and clear the
  selection). Clicking a clip selects it.
- **Clip drawing**: body in the channel's colour, waveform (audio) or note
  miniature (MIDI) in ink, name top-left, fade wedges drawn as translucent
  triangles from the top corners.
- **Capture control glyph**: a small tape-reel/REC-dot pair, right of the
  setup cog, drawn in its own colour so it can never be mistaken for the
  arrangement's red.

## The machinery underneath (implementation shape)

- **Doc** (`tangent-ui/src/arrangement.rs`, already stubbed: versioned,
  atomic save, `TANGENT_ARRANGEMENT_PATH`): clips carry
  `(lane, start_s, len_s, source, source_offset_s, fade_in_s, fade_out_s,
  name, id)`. Lanes are plain indices — variable count later stays cheap.
- **Engine**: the single backing `Placed` generalises to per-channel clip
  SCHEDULES (sorted, sample-stamped, fades as ramps), walked against the
  same transport/generation machinery Locate already uses. MIDI clips are
  a sample-stamped event feed into the channel's rack. The audio thread
  still never allocates: schedules swap in whole behind an Arc, like the
  track did.
- **Recording path**: armed channels tee into clip writers during
  arrangement record; files finalised and doc updated on stop.
- **View** (`tangent-ui/src/arrangement_panel.rs`, new): pure painter +
  `Layout::targets`/`hit_test` single-source-of-truth, exactly like every
  other panel; the firewall stays intact (file drops arrive as PATHS via
  a port; decode is the host's).
- **Band**: the timeline box's SeekTimeline/Locate plumbing is the
  scrubber's input path (it was built as "one lane of a thing that will
  one day show several" — this is the day). `ImportTrack`,
  `RecorderRequest::ClearTrack` and the TrackInfo plumbing retire with the
  BACKING strip.

## Where it stands

- **Phase 1 SHIPPED (4.43.0)**: BACKING retired, settings v13, M S R,
  arm-as-live.
- **Phase 2 SHIPPED (4.44.0)**: projects (directory + `media/` auto-copy,
  reopen-last, v12 backing seeded as the first clip, New / Open / Duplicate
  in the menu under a PROJECT subject), the doc at v2 (trims, fades, names,
  ids, relative paths), and the engine playing a LIST of placed, trimmed,
  faded clips through the old track player's swap protocol. The band's
  timeline now draws FROM the document (one clip's wave + the project span),
  gained a drag-to-scrub, and its × empties the timeline. Deliberately
  deferred: the lanes-and-viewport look of the scrubber arrives WITH the
  arrangement view (there is no viewport to steer until the view exists),
  and MIDI clip schedules arrive when something can create MIDI clips.
- **Phase 3 SHIPPED (4.45.0)**: the VIEW. `arrangement_panel.rs` — lanes,
  REAPER-style headers (number, arm, name, knob, meter, M/S, delegating to
  the mixer's own hits), bars ruler, clips with waves, fades and selection;
  Tab cycles piano / mixer / arrangement. All five clip operations with the
  owner's gestures (mouse move/trim, corner-drag fades, Delete, S — which
  the arrangement owns while open; the supporter card keeps it elsewhere).
  Zoom three ways (wheel pan, Cmd/Ctrl-wheel and ruler-drag zoom), file
  drag-drop onto lanes (the MIDI-lane refusal lives in `import_audio_at`),
  and the band scrubber became the Ableton overview: lane blocks, lit/dim
  playhead, and a viewport rectangle that pans the arrangement when picked
  up. The engine plays clips PER LANE — fader, mute, solo, send, peaks —
  with the rack unification still owed (one rack must run once per block
  for live input AND clips). AMENDED against the draft: a drop does NOT
  flip an empty lane to AUDIO — audio-kind lanes bind interface picks by
  ordinal, and the flip would have silently re-bound real inputs (the
  owner's second Scarlett input among them). A lane with clips and no kind
  is a CLIP lane: used, waveform icon, no live source. Explicit per-channel
  input binding is the follow-up that would make the flip safe.
- Deferred within phase 3: cross-lane clip drag (a clip stays on its lane),
  viewport edge-stretch zoom, grid snapping, lane rename in the header.
  (Snapping has since SHIPPED with the toolbar: magnet + gridbox, `g` and
  the wheel over the gridbox.)
- **Phase 4 SHIPPED (4.46.0)**: the RECORDING SPLIT. The red button records
  ARMED lanes into the arrangement: MIDI lanes collect the note stream —
  one identical note clip per armed MIDI lane, stamped on the tap's own
  clock against the press — and AUDIO lanes tee their input picks RAW
  (pre-fader, pre-rack, pre-arm) off `mix_monitor` into per-input rings the
  host drains every frame and writes as Float32 WAVs into the project's
  `media/`. Stop, the red button again, or the band closing all finalise
  the same way: clips into the document, one save, and `reload_clips`
  pushes the new schedule so the take SOUNDS immediately. The CAPTURE —
  the whole-take machinery untouched, count-in included — moved to its own
  control: a tape-amber cassette right of the cog, lit while writing, and
  Enter during a count-in still cancels (the cancel follows the count to
  the cassette). And MIDI clips PLAY: `PlacedMidi` schedules swap into the
  render thread by the audio clips' own protocol, `mix_channels` assembles
  each lane's notes per block — the live stream while armed, the clips'
  whenever the transport stands on them, because playback never asks the
  arm — and note-offs ride an active ledger that survives a jump landing
  exactly on a note.
- Known slack in phase 4, written down on purpose: capture start is
  block-quantised (no input-latency compensation yet); and a recorded WAV's
  tail can be one device buffer short, because the tee drains what has
  arrived when the record ends rather than waiting.
- **The sustain pedal is no longer on that list.** It was — "not written
  into recorded MIDI clips, it rides the live path only" — and the symptom
  was a part played with the damper down coming back dry, because the clip
  kept only the instants the keys came up. All three pedals are now recorded
  as `ArrPedal` edges beside the notes, at their raw values so half-pedalling
  survives, and carried to the instrument at playback as a per-lane LEVEL
  rather than a stream of events: `PlacedMidi::pedal` holds the loop-expanded
  edges and `clip_pedal_level` answers where they stand at a frame, which is
  what makes a locate into the middle of a held pedal land held. Outside a
  clip the level is zero, so the end of a clip, a locate past it and a stop
  all release the damper by one rule with no special case for any of them.
  The rack is handed that lane's own control list rather than the live one,
  which is what gives a VST3 instrument a door the clip's pedal can reach.
- **Post-phase-4 batch SHIPPED (4.47.0)**, all owner-directed:
  - **Snap holds the playhead too**: clicking a lane or the ruler with the
    magnet on lands the locate exactly on the grid line.
  - **Alt = move the viewport** (the owner's correction of the earlier
    "alt-wheel scrubs"): alt+wheel pans, alt-drag on empty room grabs the
    view. Ctrl+Alt+wheel turns the gridbox from anywhere. Help card
    updated per view.
  - **The take is visible while it happens**: every recorded lane wears a
    growing ghost from the press to the playhead — red-rimmed, untouchable
    (id 0), the MIDI ones with their notes so far (keys still down sound
    to now). MIDI clips in the arrangement draw real NOTE MINIATURES now;
    the dashed staff remains only for empty items.
  - **The band's backing-track box retired**: the empty timeline is quiet
    (audio arrives by dropping it on a lane) and the box's left is the
    REAPER-style LCD cluster from the owner's screenshot — big
    bars|beats|thousandths playhead, min:sec under it, Start/End/Length of
    the selected clip beside it, all in tape-green on black.
  - **THE MIDI EDITOR, v1** (`tangent-ui/src/midi_editor.rs`): double-click
    a MIDI item opens a piano-roll popup over the arrangement (a rim of
    lanes stays visible; a press outside, Esc or the × closes it, saving).
    Notes: click/drag draws (one grid step, snapped, vel 100), body-drag
    moves in time AND pitch, right-edge stretches, right-click erases,
    Delete removes the selected. The keys column auditions through the
    live path. Wheel grammar mirrors the arrangement: wheel zooms around
    the pointer, alt+wheel moves the editor's own viewport (pointer-over
    decides which surface answers), ctrl+wheel walks the roll up and down,
    ctrl+alt+wheel turns the grid. Keys while the hand is on it: `g` the
    editor's own magnet, `o` swing grid, `p` triplet grid — swing and
    triplet are exclusive, drawn AND snapped by one authority
    (`snap_t`/`grid_lines`, swing at 58% of the pair, triplet 2/3 of the
    step), with the shape in the gridbox word (`1/8T`, `1/8 sw`). Top
    strip: name, SWG/TRP/SNAP chips, gridbox, ×. Separate settings
    (`midi_snap/midi_grid_exp/midi_swing/midi_triplet`) so note grids and
    clip grids never surprise each other.
  - **Edge-drag auto-scroll in the roll**: a note dragged to any of the
    four edges takes the view with it — sideways along the clip, up and
    down the keyboard — so a note can be moved to a time or a pitch that is
    off screen without zooming out first. The window stops at the song's
    start and at the clip's own end plus its pad; the key range stops where
    the wheel's does. Same rate law as the arrangement and the mixer.
  - Editor still owed: velocity editing, note selection in bulk, a bar
    ruler inside the popup, quantise-existing, and the arrangement-ruler
    numbers reflecting inside the roll.

- **Input exposure (4.48.0)**: ANY device wider than one channel — two
  included — offers per-channel exposure; a fresh device auto-checks
  every mono and every ADJACENT stereo pair; an audio channel's menu
  offers only the chosen device's inputs.
- **4.49.0**: the `drumpad` branch merged (docs/DRUMPAD.md); the hidden
  piano no longer takes primary presses under an open mixer/arrangement
  (stand-down + regression test — painting was always exclusive, only
  the hit-test leaked); each view's `h` card lists ONLY keys that exist
  on that page; tool-nav corner (outer=song, inner=viewport, drag to
  zoom+scroll); gridlines follow the gridbox.
- **The DAW-session batch (4.50.0)**, owner-directed, built while they
  were away:
  - **Sessions carry the desk** (doc v4 `DeskSnap`): kinds, names,
    colors, inserts (with per-seat state sidecars copied through the
    project dir), sends, gains, pans, inputs, tempo/sig, metronome,
    master — captured on every save, applied on explicit open only.
  - **Project opener**: seventh transport slot (folder) → New / Open /
    Save / Save As / Save As Template / Export. Templates live at
    `Projects/Templates/<name>`; `default` is seeded at launch and New
    clones it; naming a template "default" replaces it.
  - **Export**: the whole track (realtime master bounce to
    `export/track.wav`) or per-lane stems in ONE pass (`stems_on` +
    `stem_scratch` tap in all three folds → per-used-lane WAVs at
    `export/stems/NN-name.wav`). The drum bus is not yet a stem.
    (Superseded 4.52.0: the Export dialog replaced both rows. Its realtime
    pass stages the take at `export/track.wav` and delivers into the
    folder and shapes the dialog names; the rows' last code went
    2026-10-04.)
  - **The band is one LCD**: the whole box is the REAPER-style cluster —
    bars|beats|thousandths + min:sec left, Start/End/Length of the
    TIMELINE (the viewport, accurate with no clips) right, every part a
    draggable field (8px/unit ratchet; Start pans, End resizes, Length
    zooms around centre and counts duration from zero). The overview
    scrubber retired. Transport: Rec → Play → Stop → Pause (its own
    button), and Space stops ANY recording — take, arrangement record,
    or rolling transport. (Superseded 2026-09-22: a take is capture only.
    Space never reaches one, and only the cassette starts or ends it.)
  - **Recording over material is DESTRUCTIVE** (`arr_carve`): the new
    clip punches out whatever it overlaps — remove, trim either edge, or
    split with the tail's offset advanced — so behind the playhead is
    the new take, ahead the old.
  - **The drum bus is a desk strip** (settings v14, `Strip::Drums` left
    of the click, its own colour; sidecars shifted 8..10 → 9..11 on
    migration). Pads each carry THREE insert bays + pan riding the
    strip-indexed insert space (`RACK_SEATS = STRIPS+1+16`), summed
    through the bus rack, folded with `drums_gain/drums_pan`.
  - **Drumpad v2**: twice the width; a real media explorer (browse the
    filesystem, drag audio straight onto pads); per-pad FX chips and
    pan; the bus controls live on the desk strip, not the pad window.
  - **Pan everywhere**: equal-power `pan_gains` (unity centre) applied in
    every fold and the stem tap; knobs on every mixer strip AND every
    arrangement header.
  - **In-app input menu**: the rectangle under a channel's icon opens a
    GUI menu — None / Device → Mono & Stereo (every exposed pick, all
    pairs of any ≥2-channel device) / MIDI (All + each port). Explicit
    binding (`channel_inputs` specs, `channel_picks` verbatim in
    `audio_selection`); no more system dialogs.
  - **MIDI editor v2**: bar ruler, velocity lane (drag to set; nearest
    note or the whole selection), multi-select (shift-click, cmd-drag
    marquee), move/resize the selection as a set, Q quantise, Cmd+D
    duplicate a grid step ahead, arrows nudge (alt = fine), Cmd+A, Z fit
    clip, Delete; draw previews audition through the live path.
  - **Right-click a channel** (either view) → Remove Track / colour
    swatches / auto-palette. Auto-palette is ON by default (five staged,
    among them EMBER, TIDE, MOSS and NEON), colours are per SLOT, and manual
    paint overrides per strip. The mixer × retired with it.
  - **The dial standard**: every dial in the program — band, mixer
    strips, arrange headers, drumpad inspector — reads VALUE on top,
    LABEL on the bottom, always visible. Click-to-type opens PREFILLED
    with the current readout, caret at the end, arrows/click move it.

- **4.51.0 — the owner's morning-after list**: the TRACK menu answers a
  right-click ANYWHERE on a strip (the strip was wall-to-wall controls, so
  "empty space" never existed; a loaded insert bay keeps delete/replace;
  the old in-mixer swatch overlay retired outright — `Hit::Paint`/
  `Palette`, `palette_open`, `palette_over` all gone). DRUMS → **DRUM
  BUS**, dark violet by authority (`DRUMS_COLOR = 21`, no palette uses
  it), subtitle dropped. The input chip is a bone PANEL now and its press
  actually opens the menu (`InputChip`/`InputRow` were missing from the
  act-on-press list); the Device page lists THE INTERFACES when none is
  open (the recovery door: a lost device selection could previously only
  be repaired through Audio Setup); the auto-expose marker is versioned
  ("2|key") so devices seeded under the 4.48 natural-pairs rule re-seed
  once with every adjacent pair. The **LPF turned round**: ascending
  hertz, 20 kHz fully clockwise and OUT OF THE WAY (limiter-style top-
  rest; `LPF_HZ` flipped, `Sends`/`Shared` rest at 1.0, settings v15
  flips stored positions); "20k" types as twenty thousand (NumEdit
  accepts k on the filter fields, the seed sieve keeps it). The cog is
  the LAST transport slot and opens a two-tier SETTINGS panel — AUDIO
  above TAKE. Root cause of "input monitoring dead since 4.49": the
  owner's `record_audio_device` key had vanished from settings.json (a
  save while the field was empty — writer unidentified; the malformed-
  load guard was already in place and unfired), so no input stream ever
  opened; the menu's device rows are the fix and the recovery.

- **4.52.0 — the morning-after batch, whole**: stop STAYS at the
  playhead (takes too; play resumes there); viewport length ceiling an
  hour; multi-bar grids draw coarser (strong line = the snap grid);
  live PUNCH (`Shared::punch` — a recording lane's clips fall silent
  from the record start, so the overdub is never heard against what it
  replaces); input monitoring ON by default (arm-off carries the
  feedback guard) and the open device mirrors back into settings every
  reconcile; THE INPUT MENU is a floating hover tree (Mono/Stereo/MIDI
  submenus right, None; the input IS the kind — REAPER model — binding
  adds the track, instruments bind midi:all, the kind ask and cycle are
  gone, v16 migrates); arrange headers wear the chip on every lane,
  double-click renames, the header seam drags (persisted); dark mode
  scoped to THEORY+ARRANGE (arrange defaults grey-cream); the band's
  columns reach the camera's line; mixer knob caps live inside their
  band; drumpad explorer follows symlinks (~/Dropbox!) at a readable
  size; **EXPORT**: one dialog (source master/stems/both, rate,
  stereo/mono, dir+browse, name, wav+mp3 primary/secondary) rendering
  FULL SPEED by default on an OFFLINE ENGINE — `Engine::offline`, no
  device, same racks/clips/pads/desk, pulled ~80 ms per UI frame —
  with realtime as the option (mp3 = LAME at 320k, round-trip tested);
  **THE OSCILLOSCOPE** (sibling session's widget): the LCD's right half
  shows the speakers' mix WITHOUT the click (pre-click tap mirrors the
  stems tap), LCD glass both modes; Start/End/Length cells and their
  drag retired (navigator/wheel/ruler carry the viewport).

- **4.53.0 — the 4 AM five**: the input menu SNAPSHOTS its rows at open
  (device/port enumeration was running up to three times a frame — the
  lag); Space is PLAY on the desk surfaces and stop everywhere, and STOP
  RETURNS TO THE EDIT CURSOR (`Transport::cursor_s` = the last explicit
  locate; play from 9, stop at 12, the playhead is back at 9); the
  arrange THEME is scoped to the lane field + ruler alone (headers and
  toolbar keep the desk's fixed wood/bone; light shades darkened; dark
  mode gets its own ruler strip); TIME SELECTION — drag sideways on the
  ruler (vertical still zooms, a click still locates and clears), drawn
  as an amber veil, offered in the export dialog's TIME row and honored
  by both render paths; and with nothing chosen the input opens the
  SYSTEM OUTPUT's own box (name-matched input side; explicit choices
  and "MIDI only" still win; the bare system mic is still never taken).

- **4.54.0 — the 5 AM batch**: THE MIDI EDITOR WAS UNREACHABLE — the
  double-click check ran in the press branch while egui fires the flag on
  the second RELEASE; it (and the lane rename) moved to a release-edge
  handler, proven by an input-simulation test. The input menu's rows are
  cached the moment the DEVICE arrives (`tend_menu_cache`, key = uid +
  width, 5 s TTL for hot-plug) and submenu rows light under the pointer.
  The pre-scope CLUSTER lives in the LCD's LEFT half again — big
  position + clock, Start/End/Length rows, every cell draggable
  (`lcd_part_cells` stacked layout) — the scope keeps the right. The +
  is a CIRCLE and the circle alone adds a BARE track (armed, send up,
  kind None, NO INPUT chip; `tracks_added` bitmask; Ctrl/Cmd+T = first
  free slot; both views wear the circle) — the chip binds later; the
  rest of an empty strip is quiet ground for the right-click menu.
  Knob tick skirts joined the size budget (labels never overdraw);
  the master ladder runs to the VU tops and stepped left for the heart;
  cmd/ctrl+wheel no longer duplicates alt-pan; the navigator's inner box
  keeps one size wherever the viewport stands; the Track dialog is 480
  tall so Done exists.

- **4.55.0 — the lag, measured and killed**: `sample` on the live app
  showed the main thread blocked in coreaudiod IPC — `tend_menu_cache`
  read `AudioSetup::input_channels()` EVERY FRAME, and that call
  enumerates every input device through cpal, which **builds a whole
  AudioUnit per device per query** (`audio_unit_from_device` inside
  `default_input_config`). Hundreds of ms of blocked mach IPC per frame.
  The UI now reads a stored width (`app.input_width`) pushed ONCE on the
  reconcile edge (`devices::wanted_device_width` — the DEVICE's width,
  not the open stream's, so a one-pick stream still lists every input);
  the menu's open-frame check reads the same field. Also: the default
  output-match now PROMOTES the matched uid into `sel.wanted`, so the
  selection settles like any chosen device instead of comparing
  `None != open` forever.

- **4.56.0 — the seven-AM pass, seventeen items**: DRUM BUS badge (side
  drum, crossed sticks) and the click's metronome DROP AND CENTRE (no
  input chip below them); the arrange field is ONE grey palette (rows
  ~#37/#3b, gridlines darker than the field, ruler strip #4a; dark mode
  does not toggle it for now); empty slots in both views sit on their
  own #2e2e2e ground under the circled plus; the Track dialog's Done is
  measured inside; DRUMS_COLOR brightened (22); the MIDI editor's face
  lifted, fills the WHOLE view under the band (pop-out button beside ×
  floats it, `midi_editor_floating`), a plain click draws exactly ONE
  GRID STEP (the create drag floored at 0.05 s — the "16th" bug),
  double-click removes a note, and edge drags CRAWL the window; clicks
  at the left edge land exactly 1|1|000 at any zoom; the ruler carries
  an h:mm:ss row; channels SELECT on click (faint rim, groundwork);
  Ctrl+A/M/V jump views (cards name only the others); the playhead
  timecode drags by part (row 3 = locate); the nav domain floors at
  four viewports so POSITION shows on short songs; the arm dot keeps a
  fixed seat off the title. Export grew DEPTH (24-bit PCM default,
  32-float, 16-bit; offline writes it directly, realtime re-encodes on
  delivery) and Save As starts its native panel in Projects. ⚠️ TEXT
  FIELDS SWALLOW KEYS now — typing "tangent" into a rename fired
  T/A/G/E as global binds; and G answers nowhere but the piano view.
  Stems: engine-level offline tap PROVEN by test; the owner's silent
  run left no artifacts anywhere — stage breadcrumbs added to name the
  break on the next run.

- **4.57.0 — drums on the desk, swing with a number**: the DRUMPAD is a
  BUILT-IN PLUGIN now (`@tangent-drums`, second row of the picker) and
  TRACK 8 ships with it loaded — a MIDI clip on its lane strikes the
  pads (keys 36–51, the editor's roll plays the kit), the bay's face
  opens the drumpad window, and both the fresh-install desk and an
  untouched default template carry it (settings v17 reclaims an empty
  track 8; a stock-seeded template heals itself). COUNT-IN and SIG sit
  in the settings panel's AUDIO tier too, and the red button honours
  the count-in for arrangement records (mid-count press cancels).
  Chord ties FIXED: same-key back-to-back clip notes drain their offs
  before the ons, so the second chord sounds. The editor's SWG chip
  opens a MENU — Swing Percentage (one decimal, 66.7 default) plus
  Straight (1:1) 50%, Septuplet (4:3) 57.1%, Quintuplet (3:2) 60.0%,
  Golden Ratio (1 : 0.618) 61.8%, Triplet (Shuffle) (2:1) 66.7%,
  Dotted (Hard) (3:1) 75.0% — the percentage IS the grid: lines,
  magnet, note creation (long-short durations), per-note nudges and
  pair-wide duplicates all follow it. The gridbox is a READOUT (wheel
  still walks it; a click does nothing). New sessions land on
  "Untitled" and RECLAIM a scratch nobody kept — only a project saved
  on purpose (doc flag `named`) blocks the name — and the Project
  window's title is a FIELD: type a name, press Save, the session is
  renamed and saved as that, no panel in between.

- **4.58.0 — clips travel**: a clip DRAGS ACROSS LANES as well as
  along them (the row under the hand retargets it; the import rules
  hold — MIDI never lands on an audio channel, audio never on a MIDI
  one, an unused lane takes either), and `.mid`/`.midi`/`.smf` files
  DROP IN from the outside world as MIDI clips — a hand-rolled SMF
  reader in `tangent-ui/src/smf_read.rs` (format 0/1, running status,
  tempo map, metrical division only; the firewall keeps `midly` out),
  notes inline in the document, nothing copied to `media/`. Audio
  drops keep their existing road.

- **4.59.0 — the drum bus retires, the pads grow up**: the DRUM BUS
  column is GONE from the mixer (the kit renders through ITS LANE's
  strip — fader, pan, mute, solo, send, meter and STEM; the hidden bus
  rows remain as engine fallback); silent lanes get NO stem (clips or
  a loaded bay = a voice, else skipped). Tracks REORDER by drag: the
  arrange header's number cell and the mixer strip's name both carry
  the track to a new row/column, everything riding along (kind, input,
  icon, colour, name, fader, send, arm, bays with their saved states —
  the host permutes the sidecars — and every clip). The Track menu
  grew a 4-slot ICON picker (auto/drum/guitar/bass, new glyphs) and
  finally fits itself. Arrange: empty lanes are etched SLOTS on a
  raised ground; gridline weights were INVERTED all along — strong
  lines now full-ink and wide. Right-drag = marquee select (notes AND
  clips; Delete takes the set), notes resize from BOTH edges snapped
  to the grid, and the editor ruler shows beat decimals (16, 16.2,
  16.3...). The drumpad: right-click an occupied pad for its MENU —
  waveform TRIMMER (start/end handles) and OBEY NOTE-OFFS (per pad,
  default off, releases fade the voice); pads DRAG to other pads
  (slots swap whole); the media explorer AUDITIONS on click (engine
  preview voice, lit row); the inspector lost the name and grew the
  sliders — click to TYPE a value, double-click resets, drag moves;
  and the open window finally SWALLOWS its clicks (the mixer under it
  was adding tracks). Help cards split into GENERAL vs <VIEW> PANEL
  sections; stale V/W binds retired (the band's toggle returned to the
  menu); the welcome card says how help works now. Project/export name
  fields type in WHITE. And the heart's mother is lowercase, as asked.

- **4.60.0 — the trust release**: UNDO/REDO — one shared line of time
  across editor, arrange and mixer (whole-document moments with the
  desk baked in, committed on saves and gesture ends; Ctrl+Z back,
  Ctrl+Shift+Z forward, Cmd works too; refused mid-take; caps at 100).
  COPY/PASTE for notes and clips (system chord via egui `command`;
  paste lands at the playhead, rebased to the earliest, fresh set
  selected). The adversarial review's 21 confirmed findings are FIXED:
  pad offs run before hits (legato on an obeying pad no longer chokes),
  reorder saves plugin states under the LOADED bundle and aborts on
  unreadable sidecars (no more factory-reset plugins), auditions join
  AFTER the take tap (never printed into a recording, and no longer
  cached unboundedly), stale carries/marquees can't fire over the
  drumpad, Escape peels one layer, Backspace while typing a knob value
  no longer deletes clips, icons persist honestly (derived for old
  docs/settings), and the name fields' wells are pinned dark under
  their white ink. Editor: dragging a selection retriggers EVERY note,
  narrow notes are all body (the "only the top note moved" trap),
  arrange group-move carries the whole banded set, and the band's LCD
  timecode drags work with the editor open. Arrange headers expose the
  THREE BAYS (small input chip, same doors as the mixer); reorder
  grabs any bare header/strip ground. Drumpad: pad names on the
  editor's KEYS (inverse ink), drum-lane auditions strike the PADS
  (the DX7 leak), the trimmer menu is big and high-res (240 buckets),
  sliders fit, metronome only sounds under play/record (count-in
  keeps its own voice). Meters grew ballistics (instant rise, ~0.94/
  frame fall). Tempo types inline like every other dial. DARK MODE is
  PER-VIEW: arrange and the editor share one wardrobe (greys light,
  warm browns dark), D toggles the open view's own stored flag
  (dark_mode / arr_dark / me_dark). Band default colour 97,55,58.

- **4.61.0 — the closing items before the release audit**: COPY/PASTE
  actually fires now (the winit layer delivers the platform chord as
  `Event::Copy`/`Event::Paste`, never as a C/V keypress — the binds
  listen for the events). View navigation reworked for Windows' sake:
  Ctrl+A/M/V retired (Ctrl+V is paste there); Tab cycles THEORY →
  ARRANGE → MIXER; Shift+1/2/3 jumps straight to each; help cards
  updated. Arrange headers stack TOP-DOWN: label, INPUT strip, then
  the THREE BAYS — both full column width. Empty slots wear full-width
  separators (header through field). Drumpad: the DEF chip (left of
  MIDI) remembers the CURRENT folder as the explorer's default across
  launches; the pad menu grew an editable NAME row (falls back to the
  file stem; a new file clears it); the editor's keys column widened
  66pt so those names fit. Arrange meters LIVE (the ballistics update
  had shipped inside the drumpad's branch — meters only moved with the
  pads open) and draw a stereo PAIR on stereo sources, one bar on
  mono. Drag-retrigger capped to ONE CHORD (the grabbed note's start
  names it — forty notes retriggering scaled awfully). The Track
  dialog's Done sits in an ABSOLUTE seat at the frame's corner with
  the content scrolling above it — no arithmetic left to clip it.

- **4.62.0 — the record works again, everywhere you play**: the
  arrangement record was DEAF to the app's own surfaces — only
  hardware MIDI ever reached it, so the on-screen piano, the key rows
  and the pads recorded silence on every platform (the void box has no
  cable at all). Every app-sounded note the cassette hears now feeds
  the arrangement too; an ARMED clip-lane (kind None — pasted or
  dropped .mid) records instead of being silently skipped; an EMPTY
  take still CARVES what it covered (recording silence over a clip is
  an erase — the punch promised destruction); and a .mid landing on a
  bare lane binds it midi:all, the cmd-draw's own ritual (that gesture
  was shift until 2026-09-07 and shift is now unbound on a lane). The
  gridboxes are CLICK-THEN-DRAG steppers now (up = finer, ~14 px a
  notch; right-click steps coarser; wheels — fragile on the Linux
  box — still work). Defaults: 1/4 grid in both views, DARK arrange
  and editor (theory stays light), band colour 97,55,58 from last
  time. MIDI clips go ANYWHERE — draw, drop, drag, paste — an audio
  channel just doesn't sound them. Arrange dials reset on
  double-click like the desk's; paste lands on the SELECTED channel;
  Shift+1 works (the chord arrives as `!`, a key egui names); the
  heart answers on every drawn pixel; a seam at every channel
  boundary. And the WINDOWS BUILD EXISTS: mp3 became a cargo feature
  (LAME's autotools cannot cross under xwin) — the Windows leg ships
  without it and the export delivery says so instead of failing.

- **4.63.0 — hardened for Windows, with ASIO**: the tester's round,
  every item run to its root by a six-way investigation. SILENT
  RECORDINGS: Windows never auto-opened an input — WASAPI names the
  two ends "Speakers (X)" / "Microphone (X)" and the exact-name match
  could never fire; the match reads the hardware inside the
  parentheses now. PADS STRUCK THE DX7: the kit was dropped on the
  floor when the audio ring was full (or the engine absent) while the
  ghosts drew anyway — set_pad_kit reports delivery and reload_pads
  retries every frame until the kit lands; pads only light when it
  HAS. NO PLUGINS FOUND: the scan already took single-file .vst3s;
  what it lacked was a mouth — discover_reported says per root what
  happened, the band note counts the finds, and when it finds 0 but
  sees .dll files it says the words VST2 out loud (the reaplugs
  trap). Roots grew W6432/x86/literal/exe-relative; bundle binaries
  are picked by NAME never directory order; LoadLibraryExW with the
  altered search path loads plugins whose dependencies sit beside
  them; scan paths stay unprefixed (no \\?\ line noise). SHOW
  SUMMONED THE PICKER: the setup panel's press leaked into the mixer
  pass that runs after it — inline band popups own the whole frame
  now. BREAKUP AT 44.1k/256: denormals flushed (FTZ+DAZ) on both
  audio threads, and Windows gets an ASIO-style double buffer —
  Fixed(256) had been landing as a 5.8 ms TOTAL WASAPI buffer,
  smaller than one engine period. AND ASIO ITSELF: cpal's backend
  compiled in (SDK bridged through cargo-xwin with VCINSTALLDIR +
  bindgen args), preferred automatically when a driver is present,
  WASAPI the fallback, Steinberg's notice in the zip. Paths: names
  split on both separators, home via `dirs`, the media explorer
  climbs above a drive root into a drives view. mp3 stays a feature
  the Windows leg goes without, honestly.

- **4.64.0 — the Sonoma round: latency, one arm, and the drives**:
  the 18.7 ms round trip against Live's 6.6 was two AudioUnits paying
  three buffers where a duplex host pays two — and the un-picked
  input opening at the DEVICE's default (512 on Sonoma, counted as
  1024 by the ring's in-flight math). On macOS the input now runs at
  a QUARTER of the chosen buffer (floor 64, clamped to what the
  device admits): its callback only copies into a ring, so the small
  block costs nothing and the monitor backlog shrinks with it —
  ~9 ms at 256, and 128 lands under Live. Windows (WASAPI total-
  buffer, ASIO single-setting) and ALSA keep symmetric buffers. ONE
  ARM AT A TIME: arming one midi track recorded three — every lane
  still armed from its birth or first bind rode into the take. A
  plain press on an arm TOGGLES one lane and carries the strip
  selection with it; the exclusive "this lane alone" moved to ALT on
  2026-09-07 (and alt again on the lane that is already alone stands
  everything down), Shift+arm toggles ONE lane without the selection,
  for building a set a lane at a time, and binding an input arms that
  lane alone; both help cards say so. AND THE DRIVES: off Windows the roots view was one bare
  "/" — external disks sat three levels down where nobody looks. The
  roots view now lists every mount it can find (/Volumes, /media,
  /run/media, /mnt), skipping the boot volume's own alias.

- **4.65.0 — the MIDI fast lane, and the audition tamed**: a hardware
  note used to take the scenic route — midir thread → RawMidiTap →
  a once-per-UI-frame drain at the END of the egui pass →
  Engine::send_midi → audio callback: 3-10 ms typical, a whole
  repaint interval worst, stacked on the audio buffer, and worse for
  pitch bend (which never woke the UI at all). The midir callback now
  plays straight into the audio callback through `midi::MidiFastLane`
  — an app-lifetime slot each fresh engine seats its ring in, pads'
  channel-10 mirror included, past-stamped so every event lands at
  the block's first frame. The UI drain keeps only what needs the UI
  thread: the arrangement's record and the take's .mid. All
  platforms benefit; the "DX7 has more" perception had no code basis
  — it renders inline in the same block. THE DEAFENING AUDITION: the
  out-pass added the media-explorer preview (and the metronome click)
  to EVERY device output channel while the mix occupies only the
  first pair — all twelve Babyface outputs lit at once — and the
  preview joined after the master had already scaled the mix, raw at
  unity. The pair is gated now, and the audition rides the drum bus
  fader and the master like a loaded pad would (mute/solo ignored:
  clicking a file is a question). Daniel joined the thanks card.

- **4.66.0 — time bends: the document follows the tempo**: a MIDI
  clip written at 120 was "made unusable at 180" — its seconds stood
  still while the bars shrank. A tempo change now rescales every
  MIDI clip (start, length, trim, notes) by old/new and re-seats
  every audio clip's bar position (start only — audio itself cannot
  stretch); mid-drag the knob rescales tick by tick and the doc goes
  to disk once at release. TAP: a small chip beside the tempo knob —
  each press one beat, the reading from the run of taps since the
  last 2.5 s silence, capped at eight. STRETCH: shift-drag from a
  note's edge scales the whole editor selection around its fixed far
  end (seven notes into the space of four, the dragged edge landing
  on the grid); shift-drag from a MIDI clip's edge in the arrange
  view stretches the selected MIDI clips the same way, notes, trims
  and fades riding along — audio clips stay out. ARM, the correction:
  a plain press TOGGLES again (the exclusive default lasted one
  build); the silent birth/bind arms stay exclusive — they were the
  phantom multi-record — and Shift+arm is "this lane alone". VU
  honesty: the clip lamp and the band meter read the PROGRAM mix —
  the click and audition join after the limiter by design and used to
  light the lamp over a correctly limited master. Strip-meter fall is
  time-based now (32 dB/s at any frame rate; it rode the repaint
  clock before). Empty arrange slots wear a visible channel-shaped
  border in both palettes (the etched seam was darker than the dark
  header wood — invisible exactly where it was needed).

- **4.67.0 — the round of being believed**: the empty-slot borders
  really draw now — the cause was PAINT ORDER, not ink: the header
  section fills its whole column with wood and each empty panel
  again, burying every earlier border the frame it was drawn; the
  boxes paint LAST now, and a test walks a real frame's shape list
  asserting no opaque fill lands on them after. Adding a track
  touches NOBODY's arms — not the standing ones (the exclusive birth
  arm silently disarmed whatever was up) and not its own (no self-arm
  at birth or first bind; R is one click away). The VU nameplate
  outlives the numbers: "VU" drew only above the ten-digit gate
  (fh≥40) and vanished under full window size — it has its own floor
  now, a face worth naming.

- **4.68.0 — the checklist round**: RIGHT-CLICK ON THE ARRANGEMENT
  (and the MIDI editor) opens the GLOBAL cut of the app menu —
  Project, Plugin folders, MIDI input, Window, Colors and the tail;
  nothing about the piano, the theory band or the neck. A motionless
  right click on the lanes or the empty roll asks for it; the drag
  gestures (marquee, erase) are untouched, and a menu-content test
  pins the cut. DELETE takes the SELECTED TRACK in arrange (when no
  clip is selected) and in the mixer — the Track menu's Remove, on
  the key. ALT+CLICK on any insert bay EMPTIES it — mixer, arrange
  headers, and the pads' chips, one gesture through the one door
  (`open_insert_face`). The GRIDBOX answers a plain left CLICK with
  one step finer (right click coarser, drag still ranges) in both
  views. The empty-slot borders slimmed to a half-point hairline. The
  settings cog's AUDIO tier carries two new doors on the count-in
  row's free half: PROJECT (the project dialog) and PLUGINS
  (add-a-folder picker).

- **4.69.0 — the menu lands where you clicked, and the card grew**:
  the arrange/editor right-click menu opened at the screen's top-left
  (or clean off the display) — the menu is its own OS window and the
  opener was handed WINDOW-local coordinates; it converts through
  `main_inner_origin` now, the piano path's own arithmetic. The
  SETTINGS card was "too small and a pain": width grew from 24%
  clamp 290-430 to 34% clamp 380-580 (row text sizes itself to its
  rectangle, so width IS legibility), and the PROJECT/PLUGINS doors
  moved off the count-in row's crammed half onto a full row of their
  own. The DRUMPAD SLIDERS split their halves: click the NUMBER to
  type, drag the track to slide, double-click the TRACK for the
  default — a motionless click on the track means nothing now, and a
  double on the number is just a hasty click into the field.

- **4.70.0 — the dismissal is spent, the sliders reset, the sends
  rest dry**: closing a context menu EATS the click — either button
  dismisses, and the press never reaches the surface underneath (a
  left-click out of the editor's menu was also drawing a note under
  the pointer; `menu_ate_press` latch, cleared each frame, honoured
  by the arrange/editor/drumpad inputs). The drumpad slider's
  double-click-to-reset moved to the RELEASE — `button_double_clicked`
  fires on the second release (the 4.54 landmine), so the press-site
  check could never see it. The SETTINGS headline and OK shrank
  (0.34 title share, the button smaller than its band). NEW SENDS
  REST AT ZERO: only the shipped DX7 arrives wired to the reverb —
  fresh binds and bare births set 0%, and the shipped default itself
  is 1.0 on strip 0 alone. RIGHT-CLICK ZONES scoped: a knob's
  right-click answers on the DIAL ITSELF (circle + tick skirt, via
  `on_knob_circle`) — the cell is mostly margin and it was eating
  every press near a knob; and the TRACK menu no longer opens on an
  EMPTY slot in either view (arrange falls through to the app menu,
  the mixer does nothing).

- **4.71.0 — fine that integrates**: every dial AND slider drag
  (band knobs, mixer faders and dials, arrange header dials, drumpad
  sliders) prices each frame's MOTION at that frame's Shift state and
  re-anchors — the old math scaled the whole accumulated distance,
  so pressing or releasing Shift mid-drag re-priced everything the
  hand had done (the tempo dial flew to an extreme when Shift let
  go). Shift now works in either order, engages and releases cleanly
  mid-drag, and FINE_DRAG went from a sixth to a TENTH: approximate
  at full speed, land the exact number. Empty mixer slots answer
  right-click with the app menu (room, no track), and the mixer
  stands down while a menu is up or just dismissed. THE ZOOM TRIO,
  both views: `-` out and `=` in, in steps; `W` fits the biggest
  clip (arrange; the shipped zoom when empty) or the clip (editor);
  and the WHEEL zooms AT THE PLAYHEAD now, not the pointer — click
  bar five, the wheel zooms there. Help cards carry all of it. AND
  THE FLASH OF TWO CHORDS: transposing a Space-held chord drew old
  and new notes together for a beat — `display_notes` is the lit set
  union `sounding`, and `sounding` waited for the frame's end. The
  transpose reconciles the sound immediately now: every old note is
  OFF before the picture shows the new ones.

- **4.72.0 — the flash for real, and one press of U**: the 4.71
  reconcile ran AFTER `rebuild_voicing`, so the neck's solver had
  already been handed old-union-new and held both chords until the
  next voicing tick — the reconcile runs FIRST now, pinned by a test
  on the display set AND the solved voicing. And U took two presses
  on every fresh launch because the group toggle counted the STAFF
  names flag, which ships true while the visible pair ships false:
  the first press "turned off" a label nobody could see. The
  decision reads only the piano and neck flags now; the staff still
  follows.

- **4.73.0 — the tester round, part two**: the RECORDING GHOST kept
  its shape — the notes miniature (and the audio wave) mapped time
  onto the viewport-CLAMPED rect, so a take crossing the right edge
  watched its own notes slide leftward; both map through the clip's
  full pixel span now. Closing the MIDI editor with a click SPENDS
  the click (the menu's own latch) — it no longer also locates the
  playhead underneath. The PLUGIN PICKER lists by DEVELOPER: vendor
  read from moduleinfo.json without loading a byte of plugin code
  (`vendor_of` in the scan), named vendors alphabetical, unnamed at
  the bottom — and the filter box already searched vendors. ONE
  STAVE AT A TIME: the "Staves" category became "Stave", an
  exclusive pick — Grand Staff at the top and default, then each
  single clef; the multi-staff stack returns as a later feature.
  CLIPS carry their own right-click menu: Delete take / Copy take,
  and dismissing it spends the press. And dist/ went from 10 GB /
  371 items to the current release + build inputs, stamped
  `.metadata_never_index` so Spotlight leaves it alone (2.x-era
  artifacts archived to ~/Dropbox/Archive/tangent-dist-legacy/).

- **4.74.0 — folders, and the take on screen whole**: while a take
  rolls the window GROWS to hold all of it — the viewport and the
  growing clip scale together, engaging only when the take crosses
  the right edge. Closing the MIDI editor spends the click on EVERY
  path now (the latch moved into `close_midi_editor` — the X button
  sits over the lanes and its press was still landing as a locate).
  The plugin picker draws real VENDOR FOLDERS: a heading per maker,
  plugin rows bare underneath, the unnamed gathered under "Other" —
  and `vendor_of` grew a composite: moduleinfo.json, then the
  SDK's own `VST3/Vendor/…` folder convention, then the macOS bundle
  identifier — measured 161 of 161 named on the owner's disk. The
  detection COUNT is the format, not a bug: the scan finds every
  VST3 on the machine; REAPER's longer list is its AU (93 here) and
  VST2 (91) backends, which a VST3-only host cannot load.

- **4.75.0 — the folders close**: the picker's vendor folders are
  DROPDOWNS now, every one collapsed at open — except the folder
  holding the loaded plugin, so the preselected row has somewhere to
  be seen. A click on the heading toggles it (▸/▾), and an active
  filter overrides the collapse: a search that found things inside
  closed folders must not read as a search that found nothing.

- **4.76.0 — leave without running anyone else's destructors**: the
  owner's machine-wide audio wedge, root-caused from the crash logs:
  quitting Tangent with Kontakt 8 loaded ran Kontakt's Qt static
  destructors in the atexit phase, which SEGFAULT (`~QApplication` →
  `qAccessibleCleanup`) — the process died mid-teardown, coreaudiod
  saw a force-killed client holding the interface, and every app's
  audio wedged until `killall coreaudiod`. Tangent's own teardown was
  already clean (engine dropped, streams released, everything saved
  before `run_native` returns) — so `main` now flushes stdio and
  leaves via `_exit(0)`, skipping the atexit chain where loaded
  plugins' static destructors detonate. Every serious host does the
  same, for the same reason.

- **4.77.0 — REAPER's dead strip, and the bays' own menu**: between
  the header's resize grip and TIME ZERO now stands a tiny strip of
  nothing, painted the recorder's wood — the lanes read as outlined
  by it, and overshooting a click at bar one lands in the strip,
  which MEANS the start (the owner had to aim near-pixel-perfect to
  return the playhead to 1). The grip itself is REAPER's splitter:
  hover-lit, with the resize cursor. And the arrange headers' loaded
  insert bays answer right-click with their own two-row menu —
  Delete FX / Replace FX, the mixer's offer — dismissal spending the
  press as every overlay does. THE PADS COVER THE KEYBOARD: pad `n`
  IS note `n` (128 of them, racks and knobs and all), the window
  shows one 4×4 PAGE, and a bank band on its left steps whole pages
  — never rows — with the current page outlined and each pad wearing
  its note name. Pages anchor at 4+16k so the classic kit (36..51)
  sits whole on its page, the default; the last page pads out past
  127 with dead cells for neatness. Old sixteen-slot sidecars load
  at offset 36 (they WERE keys 36..51); settings v18 relocates the
  pad insert seats (+36) and the host renames their state files
  once; project docs relocate by LENGTH. And THE PLUGIN IS RETIRED:
  Tangent is a DAW and does not load through another — nothing
  builds, stages, or offers the VST3 (installers are one-option
  standalone on all three platforms; release checks now FAIL a
  build that still carries it; the plugin workspace stays in-tree
  for a possible visualizer-only return).

- **4.78.0 — squares, shades, and the strip alone**: the bank band's
  page cells are SQUARES now (the band's width follows from stacking
  eight of them; the window's floor widened to fit), the drumpad and
  explorer's near-black shades lifted so face, pads and rows read
  apart, and a stale slot-based note caption that doubled the key
  names left. The arrange divider: the extra dead gutter is GONE —
  the resize STRIP alone stands between the tracks and the window,
  a real 7-point wood-faced strip, hover-lit, its own cursor — and a
  motionless CLICK on it sends the playhead home, keeping the
  generous back-to-bar-one target.

- **4.79.0 — the ring, the whole keyboard, and i3 squared away**:
  THE SELECTION RING — a keytoggle click's audition is never a
  clipped tick again. The click that selects a note is shorter than
  any note anybody meant to hear, and on an instrument that damps on
  note-off (a sampled piano; the DX7's own tails hid it for months)
  the sound died with the button. The off now waits out
  `SELECTION_RING` (1.5s from the press; a longer hold releases
  normally) while ONLY the sound waits: the note leaves `sounding`
  with the gesture, so the display and the neck's solver never draw
  a note the hand let go of — no visual retrigger, no shortened
  audition. Piano, neck, staff and the theory band all ring; a
  re-struck note cancels its pending off. TRANSPOSE ACROSS THE WHOLE
  KEYBOARD: the range guard applied the whole offset to placed notes
  that already live in display space, double-counting it — the
  further a chord had been carried the sooner the wall, the owner's
  "limited to 2 octaves". Each kind is guarded by its own motion now
  (hardware notes wear the new offset whole; placed notes move by
  the step), `TRANSPOSE_MAX` grew from 24 to 127 as a backstop, and
  a chord steps until a note would leave MIDI — C-E-G-B gets all 56
  up and 60 down. I3'S BORDERLESS OFFSET: toggling B re-parents the
  window a beat later on X11 and the surface kept painting at the
  old offset — black strip one edge, cutoff the other — until the
  next click. The decoration toggle now arms a 300ms nudge that
  re-pins the size triple each frame, which is the configure the
  click was accidentally providing. And a keytoggle note click in a
  borderless window no longer falls through to `StartDrag` — a press
  that PLAYED must not also grab the window.

- **4.80.0 — the Windows tester's round**: eight the tester found,
  most cross-platform. WINDOWS AUDIO no longer opens on ASIO by
  default — an unset preference resolves to the platform default
  (WASAPI), the door every Windows machine has; ASIO stays a
  deliberate choice in settings. The output stream builder gained
  I24 (a Realtek ASIO output's native depth — its absence was the
  "output speaks i24, not supported" that left the app silent on
  first launch), plus F64/I64. The banner's × now clears the INPUT
  error too (it outranks the others, so the × looked dead exactly
  when an input had failed), and a saved input that will not open
  under the current host FALLS BACK to the default input instead of
  a sticky "is not connected". COPY/PASTE across platforms: every
  internal copy now stamps the SYSTEM clipboard, because winit
  swallows Ctrl/Cmd+V whole when the OS clipboard holds no text —
  paste was inert on any fresh machine and "worked" on the owner's
  Mac only by luck. A cross-track GROUP copies whole (the clip menu
  cleared the selection first — "pasting the first of the group")
  and pastes keeping its lane spread (each lane was clamped
  separately and piled on the last track). THE MARQUEE painted pure
  white over the clips: `from_rgba_premultiplied` with RGB far above
  the alpha is an out-of-range premultiplied color that composites
  near-additively — unmultiplied cream now, arrangement and editor
  both. DRUMPAD after the 128-key rescale: media drops and pad
  REORDER hit-tested a panel SLOT (0..16) and passed it as a KEY
  (0..128) — a drop landed on the wrong pad, a reorder swapped the
  wrong seats; both map through `drumpad_key_of` now, and a
  reordered pad's FX bays ride with it (seats swapped, host state
  files permuted). A new session no longer inherits the last
  scratch's pads — `new_project` reclaimed the Untitled dir but left
  its `drumpad.json`; the sidecar is dropped now. EXPORT glitching:
  the decoder's output is VERIFIED against the container it produced
  rather than trusted — a Windows ffmpeg that ignored `-ar`/`-ac`
  wrote wrong-speed, wrong-channel audio and nothing could tell,
  because the engine never resamples a clip; decode reads a WAV now,
  reads the real rate and layout from its `fmt ` chunk, and
  resamples or folds to the ask. THE TIME SELECTION IS A LOOP ZONE:
  the orange span loops playback (wrapped on the audio thread, so
  the wrap lands on a block boundary), and Escape clears it. The
  Windows camera picker's message lost its garbled runs of literal
  spaces.

- **4.81.0 — the macOS tester's round**: five from the 4.80 test
  pass, all cross-platform. THE LOOP KEEPS SOUNDING: a looped MIDI
  clip went silent after the first wrap — the note scheduler's
  cursor is forward-only and rewinds only on a jump, and the loop
  moved the clock back without flagging one, so audio clips survived
  (they read straight off the block position) while MIDI/DX7 clips
  did not. The wrap now arms a flush for the NEXT block (where the
  wrapped position is what the cursors rewind against). NEW SESSION
  IS EMPTY: New reclaimed the current unsaved Untitled, and
  `switch_project` then saved the session being discarded back over
  the fresh template — resurrecting its clips — while the stale
  `drumpad.json` was never cleared. New now empties the reclaimed
  folder and, when it is discarding the open session, opens without
  the save-back. DRUM FX WORK: a pad's insert bays stored a plugin
  and the host dispatched it, but `load_insert` capped at `STRIPS`
  and silently refused every pad seat (they live above it) — the
  engine's `mix_pads` was already wired to run per-pad racks; they
  just never got filled. RIGHT-CLICK ON A PAD no longer crashes: the
  pad menu stored a KEY and indexed the 16-slot layout array with it
  ("len is 16 but the index is 40"); it maps to the current-page
  slot now, and closes when its pad scrolls off-page. And the BANK
  STRIP lights each page's occupied 4×4 cells brighter, so the whole
  keyboard's loaded pads read at a glance.

- **4.82.0 — sixteen channels, eight in view**: the desk grew from
  eight channels to SIXTEEN. Eight show at a time in both the mixer
  and the arrangement; the rest scroll — no scrollbar, the wheel
  does it. In the mixer the wheel anywhere over the desk moves the
  channels while the CLICK and the MASTER stay pinned at the right;
  in the arrangement the wheel over the TRACK HEADERS moves the
  lanes while the timeline keeps its zoom. The MIDI editor's piano
  scrolls the note range under the wheel too, no modifier. Growing
  the count renumbered the drum bus, the click, the fx return, the
  master and all 128 pad-insert seats — every one steps up eight —
  so a SETTINGS v19 migration and the project desk-snapshot path
  both lift an eight-channel desk into the sixteen-channel shape
  (drums/click/fx/master and each pad's FX land where they belong,
  not scattered onto the new empty channels), and the host renames
  the seat-numbered insert-state sidecars once. The bitmask masks
  stay u32 — sixteen is well inside their room. Pinned by a v18→v19
  grow test that carries a pad's FX across, and by mixer/arrange
  layout tests for the window and the pinned master.

- **4.83.0 — Windows carries its own runtime, and the installer wears
  the logo**: a fresh Windows 11 box (no dev tools, no VC++ redist)
  refused to launch `tangent.exe` — "VCRUNTIME140.dll was not found".
  The msvc target links the C runtime dynamically by default, so every
  cross-built release to date carried a latent dependency on a redist
  most machines happen to have and a clean one does not. The Windows
  target links the CRT STATICALLY now (`+crt-static`, scoped to
  `x86_64-pc-windows-msvc` alone) — the runtime rides inside the exe,
  no redist, no app-local DLLs, no installer step. The bundled
  `tangent-ffmpeg.exe` is BtbN's mingw-static build and needs no
  redist either. And the NSIS installer wears the app's own logo now:
  `MUI_ICON`/`MUI_UNICON` point at the same `tangent.ico` build.rs
  embeds into the exe, so the setup, the app, and the Add/Remove
  entry are one mark instead of NSIS's placeholder.

- **4.84.0 — zoom the desk, and the drum moves up**: Shift+wheel now
  ZOOMS how many channels/lanes/keys show at once — one per tick, over
  the mixer, the arrangement's track headers, and the MIDI editor's
  roll (the note count in that case). Plain wheel still scrolls one at
  a time; the two gestures share the same surfaces. The counts persist
  like the grid, clamped to a legible band, and default to eight
  channels / eight lanes / three octaves. The DRUM TRACK now ships on
  channel 2 instead of channel 8 — with sixteen channels there was no
  sense stranding the kit in the middle; a pre-v17 file still migrates
  its kit to its historical seat. The arrange track header sheds
  controls in the owner's order when a lane is short: the insert bays
  go first, then the input selector, and never the name. (The default
  grid was already 1/4 in both the arrangement and the editor — no
  change needed there.)

- **4.85.0 — the zoom unsticks, rename stops deleting tracks, and
  Windows gets MP3**: the arrangement's Shift+wheel lane zoom only
  zoomed IN both directions — holding Shift makes the OS turn the
  wheel HORIZONTAL, so the notch arrived on X, and the lane-zoom read
  Y alone (always the same branch). It reads the combined X+Y delta
  now, exactly as the mixer's zoom (which worked) already did; the
  MIDI editor's Shift+wheel key-zoom had the same flaw and the same
  fix. RENAME FIELDS now swallow the frame's keys through one
  `text_entry_active` gate every shortcut path consults: typing "DDD"
  or Backspace into a track title, or H, no longer deletes the track
  or slides the help card — only Enter, Escape and a mouse press act.
  (The lane rename reuses the mixer's naming state, which the arrange
  key gate did not know about.) WINDOWS MP3 EXPORT works now: the
  cross-build cannot link LAME, so MP3 routes through the bundled
  ffmpeg (its GPL build carries libmp3lame) — a temp WAV, then a 320
  kbps transcode. And a camera UID synced from a Mac no longer parks
  a permission-flavoured banner on Windows: `Unsupported` is a fact
  about the build, not an error, so the pane just shows "no camera".

- **4.86.0 — the low-pass ships open, and the pedal stops glitching**:
  the master LOW-PASS default read 200 Hz (the bottom of its sweep)
  instead of the shipped-open 20 kHz — the six mix knobs never
  advertised their own defaults in `effect_defaults`, so a reset or
  the default marker fell to the fallback. They carry their real
  defaults now (reverb/delay/chorus off, HPF open, LPF open at 20
  kHz, limiter off). And the DX7 SUSTAIN PEDAL no longer glitches
  under rapid notes: with the damper down, re-striking a key
  allocated a fresh voice every time because `note_on` matched only
  keys that were still `on`, not ones the damper was holding — so
  rapid repeats stacked identical voices until the sixteen-voice pool
  ran out and stealing chopped ringing notes. A repeat reuses the
  held voice now, one per pitch, the way a real damper behaves.

- **4.87.0 — the camera works on Windows**: Windows had only a
  camera STUB; now it captures, through the bundled `tangent-ffmpeg`'s
  DirectShow input. `camera/windows.rs` enumerates devices from
  `ffmpeg -list_devices` (the stable `@device_pnp_…` alternative name
  is the UID, the friendly name is display), reads each device's modes
  from `-list_options`, and on open spawns one ffmpeg that decodes the
  chosen mode to rgba on stdout while a reader thread stamps and
  publishes each frame to the shared slot — the same delivery contract
  the Linux and macOS backends use, with no new native COM code and
  nothing extra to cross-compile. It honours the mjpeg/raw split a
  webcam forces (720p is mjpeg-only; raw yuyv422 caps at 480p), so a
  request for an mjpeg-only size passes `-vcodec mjpeg` rather than
  letting dshow fall back to a lower raw mode. The picker's Windows
  "no camera" text now points at the privacy setting instead of
  saying the feature does not exist.

- **4.88.0 — the take manifest tells the whole truth**: the band's
  live "N video frames lost" counter reads the ENCODER's dropped
  frames, but `take.json` only recorded the CAMERA-side numbers
  (superseded, dropped-late) — so a take where the band said "5
  dropped" wrote a manifest that said zero, a contradiction a tester
  would rightly report. `VideoReport` carries `frames_encoder_dropped`
  and `frames_padded` now, written from the same numbers the band
  shows, so the counter and the manifest can never disagree. (Found
  during the Windows camera validation — the camera itself passed end
  to end on a clean box: live preview, correct colors, a video take
  with the camera embedded.)
- **4.89.0 — Save As no longer opens under the menu**: the project
  menu is a child viewport, and clicking Save As or Save As Template
  closes it (`dialog = None`) but does not destroy its window the same
  frame — a child viewport dies the frame AFTER it stops being drawn.
  The save panel opened inside `after_frame`, before that menu-less
  frame was even on screen, so the OS panel landed BEHIND the still-up,
  still-always-on-top menu window: "the dialog loads below the menu",
  on macOS and Windows alike. The save panel now takes the same one
  extra frame the file pickers take (`arm_panel`) and lets the menu
  window go first. Both save panels, both platforms.
- **4.90.0 — ReaStream to OBS, and ASIO stops dying silently**: two
  things. FIRST: Project menu grew "Stream to OBS..." — a native
  ReaStream SENDER (the wire protocol Cockos's plug-in speaks: UDP
  58710, `MRSR` packets, float32 non-interleaved, 32-byte identifier),
  so OBS running the genuine ReaStream VST2 in Receive picks up
  Tangent's master out over localhost. Tangent hosts VST3 and
  ReaStream ships VST2-only, so Tangent speaks the PROTOCOL instead of
  loading the plug-in; the receiver stays the real thing. Byte layout
  pinned against two reference implementations, plus a live UDP
  loopback test through the real sender thread. The master pair is
  tapped where the scope reads (what leaves), pushed to a lock-free
  ring only while streaming — the audio thread never touches a socket.
  SECOND, the ASIO silence, root-caused to two stacked teardowns and
  both killed: (1) every device lookup built a FRESH `cpal::Host`, and
  a fresh ASIO host's empty driver-guard lets the SDK's process-global
  loader unload the driver a running stream is playing through — so
  input enumeration tore down the live output. One cached host per
  system now (`with_host`), verified against asio-sys 0.2.6: through a
  shared instance the guard returns the live driver for its own name
  and refuses all others, so enumeration while running lists exactly
  the live driver and can tear down nothing. (2) an ASIO driver has
  ONE buffer set and cpal keys stream state to the `Device` OBJECT:
  opening the input through a second object destroys the running
  output's buffers (`ASIOCreateBuffers`, per asio-sys's own doc) with
  no error anywhere. The input now opens on the ENGINE'S OWN device
  object under ASIO — cpal's real duplex path, input ADDED to the
  output's stream set. And because silence must never be silent again:
  a watchdog (`StallWatch`) declares an output whose callback count
  stops climbing while it claims to run, surfaces it in the band, and
  reopens through the standing retry path — the fault channel is read
  now too. `resolve_output` also refuses a dead default device (a
  registered driver with no hardware) and finds one that actually
  plays. Windows ASIO validation on the canary box follows this ship.
  Also: the plugin-scan and picker notes stop citing "ReaPlugs' VST3
  build" — no such build exists; ReaPlugs is VST2-only, and the notes
  now say the true thing. And double-clicking the band's LPF knob now
  rests it fully OPEN (20 kHz), like the limiter and for the same
  reason — its knob is a cutoff, so "not applied" is fully clockwise;
  the old reset slammed the filter shut, the opposite of a reset.
- **4.91.0 — ASIO asks for the plain period**: 4.90.0's ASIO round on
  the canary box crashed at startup — `index out of bounds: the len
  is 2 but the index is 2` inside cpal's ASIO output callback — and
  the arithmetic reconstructs exactly. `BUFFER_PERIODS` multiplies the
  requested buffer for WASAPI (where `Fixed(n)` is a ring to be carved
  into periods), so on Windows the OUTPUT asked ASIO for `Fixed(512)`
  while the INPUT side of the same driver translated the same setting
  plainly (`Default` → the panel's 256). An ASIO driver has one buffer
  set: when the input joined, `ASIOCreateBuffers` re-granted 256, and
  cpal's output callback — which derives channel count by dividing
  its build-time buffer by the CURRENT size — computed (512×2)/256 =
  4 channels on a stereo interface and indexed off the end of the
  two-entry buffer array. Under ASIO the created size IS the callback
  period, so `pick_buffer` now has an ASIO arm: plain `Fixed(asked)`
  clamped to the device range, `Default` when nothing was asked — the
  exact translation the input side uses on the same setting, so both
  directions request identically and the re-grant equals the original
  grant. The input side clamps identically under ASIO too (`Fixed`
  outside the range is a build error). The wins the crash proved are
  kept: the failure was LOUD (dialog + stderr, no silent silence) and
  the cached host loaded the driver once, cleanly.
- **4.92.0 — the device comes back on its own**: the USB-yank test
  passed the loud-failure bar and flunked the recovery one, twice
  over. FIRST, the freeze: a yank froze the UI "Not Responding" for
  10-30 seconds, because the watchdog's timed retries ran
  `Engine::start` against dead hardware ON THE UI THREAD, and five
  driver timeouts stack to exactly that range. SECOND, the dead end:
  by the time the owner replugged, the five retries were long spent
  and nothing re-armed them — only a manual buffer-size nudge brought
  audio back. The owner's word: "an auto recheck would be nice." So: a
  watchdog-declared death now takes NO timed retries; a background
  `Reprobe` thread watches for the hardware's return — probing the
  PLATFORM DEFAULT host only, never ASIO, because loading ASIO
  drivers from a second thread is the teardown bug and a half-dead
  driver can hang a probe for ten seconds, all of which is fine on a
  thread nobody joins and fatal on the UI thread — and the one
  UI-thread reopen happens the frame the device is seen. A transient
  stall passes its first probe immediately, so the harmless case
  recovers as fast as the old retries did. The reprobe also picks up
  after the timed loop exhausts, so a replug always recovers. Also:
  the band's message chain now puts the ENGINE's story ahead of the
  input's (a dead output is the whole app silent; the yank test
  showed the sticky input note masking it), and the owner's two
  stream-UI rulings land — the ReaStream dialog is ALWAYS the light
  theme ("keep the obs stream menu light"), and its door moved from
  the Project menu to the settings cog's AUDIO tier ("move it to the
  settings cog in audio section"): a STREAM row beside PROJECT and
  PLUGINS that reads out on/off and opens the dialog.

- **4.93.0 — the ultra sweep, and the tarball keeps its promises**: the
  owner ran the cloud multi-agent review over the 4.82-4.92 diff; 49
  findings confirmed, verified, and fixed (3 candidates refuted and left
  alone), ~30 new regression tests riding along. The ones a person would
  have actually met: the FIRST EDIT of every session was un-undoable
  (launch reopen never seeded the undo baseline — data loss, the
  critical); arrow keys in the MIDI editor also TRANSPOSED THE
  INSTRUMENT, persistently; double-click note delete could never fire
  (checked at the press, delivered at the release); dismissing a popup
  clicked THROUGH into the mixer or arrangement; Z (fit clip) also
  toggled fullscreen; a camera stall over 2 s permanently desynced A/V
  for the rest of the take; a clip at capacity dropped a note FOREVER
  instead of parking it across the block; ordinal input picks could
  record the WRONG INPUT against explicit bindings; the reprobe reopened
  the microphone with the band closed; seven DX7 algorithms (10, 12, 18,
  20, 21, 28, 30) had mistranscribed feedback routing and sounded wrong;
  voice stealing judged loudness by OP1 alone and leaked sounding
  voices; the reverb rebuilt heap buffers ON THE AUDIO THREAD; the v17
  drumpad migration was clobbered on upgrade; partial WAV appends
  desynced the write cursor; Bass (5)/(6)/(7) voicings got guitar
  fingerings; and a supporter-key install() reported success when the
  persist failed. Full sweep in the session log; suite 1,235+.
  SECOND: the Linux tarball keeps its promises now. The native path — the
  recommended one — shipped 4.92.0 without `install.sh` (its own
  README's first instruction) and without the bundled encoder; a
  tester's literal first step failed on a file that was not there.
  build-linux-native.sh stages both (vendor cache, fetch fallback,
  SKIP_FFMPEG escape for source builds), build-linux-remote.sh pushes
  the checksum-pinned encoder cache to the build host and verifies the
  result, release.sh refuses a linux artifact missing either piece, and
  build-installer.sh's linux branch drops the retired-VST3 assertions
  that made it exit 1 against correct artifacts forever.

- **5.0.0-beta.1 — themes, routing, and an export you can watch**:
  the first beta of 5.0, and the version number moves once for the
  whole set rather than once per feature — the owner's pacing call.
  THEMES: eight factory sets (Default, Valentine, Hydra, Navi, Mono,
  Studio, Aurora, Sandstone), three of them faithful to the owner's
  own applications' palettes, all eight gated by a contrast test that
  caught three real defects in them. User themes are FILES in a
  `user themes` folder beside settings.json, so Reset Settings to
  Default cannot delete somebody's work; partial themes are legal and
  an example ships. Seventeen roles, ten of which were literals in a
  painter before this. ROUTING: a channel can go somewhere besides the
  master — out of the interface on its own jacks, or into another
  CHANNEL through whatever is loaded on it. The second one is real:
  put Pro-R on channel 9, send 3 to 9, and 3 comes back wet, because
  a rack turned out to be a general audio processor that happens to
  be fed notes. Sends ride the destination's fader; hardware routes
  never take a channel off the master; a mono jack sums; one hop per
  block so a loop cannot feed itself. EXPORT: a window that floats
  over the app and draws the waveform as it renders, because a
  percentage says a render is moving and a waveform says what it is
  rendering. The render was never the freeze — the format pass was,
  and it announces itself now. Plus: dark mode moves the KEYBOARD as
  well as the theory content; every typed field in every themed
  dialog is legible again (the colour picker was black on black);
  the retired VST3 plugin cleans itself up on upgrade from all four
  install paths; Create Project Subdirectory (ticked by default, and
  media has always travelled inside the project); Open Folder When
  Done on both export paths; and the transport strip is one evenly
  gapped row spanning the LCD above it, with Stop standing exactly
  where it stood when a take starts.

  LOOPS, 2026-09-07. Every clip is born a loop over its own length,
  audio and MIDI alike, so the first pull of any clip's right edge
  repeats it instead of revealing empty time or stopping at the end of
  the file; a loop as long as its clip is one pass and sounds like no
  loop at all, which is why arming it at birth costs nothing. The clip
  menu's Loop row turns it off and back on, and turning it off leaves
  the clip's LENGTH alone: eight bars pulled out and then unlooped
  stays eight bars long and plays its material once. THE SEAM IS A
  SHAPE, not a line: where one pass ends and the next begins, a
  triangular divot is taken out of the clip's top and bottom edge — a
  tenth of the lane deep, which is the band the waveform and the note
  miniature are both kept clear of. It was two diagonal strokes for
  one day and the owner asked for the cutout instead; taking the
  material away also means there is no colour to choose and no guess
  about the lane behind it. GLUE welds the selection into one clip
  that loops over the whole of what it swallowed, and ONE clip is
  enough if it repeats: gluing a single looped clip flattens its own
  repeats into it, so C D E F looping to C D E F / C D becomes a clip
  holding those six notes, one pass, no seam, and the next pull of its
  edge puts a divot after the second D. The MIDI editor's loop bar
  sets the pass length, clamped to the clip and never growing it, and
  it edge-scrolls horizontally like every other drag on the roll —
  horizontally only, because a loop length is a time and has no pitch.

  QUANTIZE, AUDIO GLUE, AUTOMATION NODES, 2026-09-21..24. `Q` opens the
  quantize window on both surfaces (`open_quantize_window`; Shift+Q
  unbound). It is NOT modal, states its own grid (the `quantize_*`
  settings, carried per session by `DeskSnap::view`), and moves NOTES while
  the MIDI editor is open (`me_quantize_with`: the selection, or the whole
  clip) and AUDIO otherwise (`ProjectRequest::QuantizeClips`: the selection
  or, with none on a picked track, that track's audio clips, through
  `adopt_track_clips_for_quantize`). Audio is WARPED, never split:
  `transient` finds the attacks; `warp` renders the clip with each attack
  copied as recorded and only the sustain stretched (`Fit::Stretch`), moves
  segments untouched (`Fit::Slice`), or asks `stretch::percussiveness` once
  per clip (`Fit::Auto`). The map lives on the clip (`warp`, `warp_fit`,
  and `warp_rate`, the rate of the decode it was measured in, from which
  every reader rescales it to its own decode with `warp_anchors`; 0, in a
  document older than the field, means as it stands), is in the undo
  fingerprint, renders on the decode crew (`Work::Warp`) BEFORE
  any stretch, and the drawn wave is the warped render's. Each clip is
  quantized ALONE: `quantize_clip` detects on that clip's own file and
  writes a map from its own attacks. So the takes of a multi-mic kit
  quantized together (a drums folder's row selects them all) are not moved
  as one: a hit two mics both catch is pulled onto the line in each, which
  takes out the 1-3 ms the sound took to travel between them, and a hit
  only one mic's detector keeps leaves its bleed in the others to move with
  their own corrections, so the kit flams and combs against itself. A kit
  kept phase-coherent needs one detection for the group (a reference clip,
  or their sum), one map rebased onto each clip, and a way to say which
  clips are one kit; none of it exists yet. While the window
  is open, `quantize_preview` asks the host for marks: amber where an
  attack was found, a fainter tick in the clip's ink where the grid pulls
  it. GLUE welds audio too (`RecorderRequest::GlueAudio`): the host lays
  each clip out as playback does, mixes with `pull_lane_clips`, writes a
  Float32 WAV into `media/`, and hands back one plain clip looping over its
  own length, by MIDI glue's rules. AUTOMATION: two nodes may share an
  instant (a step: arrive at the first, leave from the second; `value_at`
  is right-continuous and the engine reads the same function); a plain
  drag on a row's empty room is a box that chooses nodes
  (`ArrDrag::AutoMarquee`), and a still press still locates, at the
  release; a drag carries every chosen node from an anchor; Delete, copy
  and paste reach chosen nodes before clips and tracks; Cmd/Ctrl+A opens
  the selected track's automation menu (what its wrench opens). The MIDI
  editor's playhead folds into the pass the roll draws inside a looping
  clip (origin = start - offset).

## Order of work

1. **Desk change**: retire `Strip::Track`, settings v13 (+ index
   migration), M S R on the mixer, arm-as-live end to end (engine gates
   live input by arm; DX7 channel armed at launch). The band's timeline
   keeps playing the remembered backing file untouched until phase 2's
   scrubber replaces it.
2. **Projects + playback**: the project directory (doc + `media/`,
   auto-copy on import/record/save, reopen-last-at-launch, v12 backing →
   clip in the default project), clip schedules in the engine (audio,
   then MIDI), fades, and the band scrubber reading the doc.
3. **The view**: lanes, headers, ruler, zoom/scroll, selection, the five
   clip operations, drag-and-drop import.
4. **Recording split**: arrangement record on the red button; capture as
   its own control driving the old take machinery untouched.

Each phase ships releasable; the suite grows with each.
