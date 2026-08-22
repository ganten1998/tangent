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

## Proposed (draft's own calls — veto anything)

- **Dropping audio on an EMPTY lane makes that channel AUDIO** and lands
  the clip. Dropping on a MIDI lane refuses with one status-line sentence.
- **v12 → v13 migration**: a remembered `track_path` becomes an audio clip
  at 0:00 on the first unused channel, which becomes AUDIO, named BACKING,
  carrying the old track gain. Nobody's backing track vanishes at launch.
- **Arm is persisted** in settings (like mute/solo), not session-only.
- **Recorded clip files** land under `record_root/arrangement/` (WAV per
  clip; MIDI clips serialize into the arrangement doc itself).
- **What an armed lane records**: an AUDIO channel records its input pick;
  a MIDI channel records the notes played into it (same routing the
  monitor already uses).
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

## Order of work

1. **Desk change**: retire `Strip::Track`, settings v13 (+ migration),
   M S R on the mixer, arm state end to end.
2. **Doc + playback**: clip schedules in the engine (audio, then MIDI),
   fades, and the band scrubber reading the doc (locate/scrub live here).
3. **The view**: lanes, headers, ruler, zoom/scroll, selection, the five
   clip operations, drag-and-drop import.
4. **Recording split**: arrangement record on the red button; capture as
   its own control driving the old take machinery untouched.

Each phase ships releasable; the suite grows with each.
