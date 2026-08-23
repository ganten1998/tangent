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

**Clip operations, v1, with their gestures (all owner-specified):**

- **Move** — mouse-drag the clip body along its lane.
- **Trim** — mouse-drag either end.
- **Delete** — the Delete key (selection first; same menu-not-deed care as
  the mixer where a gesture could destroy work).
- **Split** — the `s` key, at the playhead.
- **Fades** — drag DOWN-IN from a clip's top corners: top-left drags a fade
  in, top-right a fade out.

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

- **Doc** (`ivory-ui/src/arrangement.rs`, already stubbed: versioned,
  atomic save, `IVORY_ARRANGEMENT_PATH`): clips carry
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
- **View** (`ivory-ui/src/arrangement_panel.rs`, new): pure painter +
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
  block-quantised (no input-latency compensation yet); the sustain pedal is
  not written into recorded MIDI clips (it rides the live path only); and a
  recorded WAV's tail can be one device buffer short, because the tee
  drains what has arrived when the record ends rather than waiting.
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
  - **THE MIDI EDITOR, v1** (`ivory-ui/src/midi_editor.rs`): double-click
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
  - **The band is one LCD**: the whole box is the REAPER-style cluster —
    bars|beats|thousandths + min:sec left, Start/End/Length of the
    TIMELINE (the viewport, accurate with no clips) right, every part a
    draggable field (8px/unit ratchet; Start pans, End resizes, Length
    zooms around centre and counts duration from zero). The overview
    scrubber retired. Transport: Rec → Play → Stop → Pause (its own
    button), and Space stops ANY recording — take, arrangement record,
    or rolling transport.
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
    swatches / auto-palette. Auto-palette is ON by default (five staged:
    IVORY, EMBER, TIDE, MOSS, NEON), colours are per SLOT, and manual
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
  scoped to THEORY+ARRANGE (arrange defaults grey-ivory); the band's
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
