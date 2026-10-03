# Folders and sessions — the two rulings

2026-09-01 · at `ea77eeb` · the owner's executive calls from the beta push,
recorded so the next audit inherits them as law rather than rediscovering
them as bugs.

## Folders are GROUPERS, not tracks

A folder is a per-channel flag (`strip_is_folder`), membership is
POSITIONAL (`strip_in_folder` over a contiguous run under the folder —
`folder_of` walks it, stale flags self-heal at read time), and collapse is
a third flag (`strip_folder_shut`). Consequences, all deliberate:

- **Made and unmade by Cmd/Ctrl+F alone.** Multi-selection groups into a
  new folder; an empty selected slot becomes an empty folder; a selected
  folder dissolves. There is no toggle box anywhere — the arrangement's
  was retired.
- **No input selector, no record arm, no theory note.** A folder cannot
  record and cannot carry its own MIDI. In the arrangement the second
  rack takes the input selector's band (three bays on three); in the
  mixer the switch row is M S alone.
- **Six FX bays**, the master's own pattern: one appended rack seat per
  channel (`recorder::folder_extra_seat`), plain order (slot == bay,
  unlike `master_bay`), run at THE-FOLD only, carried by every
  permutation, state copy and latency figure.
- **Collapse is one control, one job**: the tab at the mixer strip's name
  plate top-left, the arrow at the arrangement header's bottom-left.
  Collapsing HIDES children in both views — the mixer lays out by rank
  among the shown, so columns close.
- **The chop is the indent.** A mixer folder strip wears a 90° notch off
  its top-right corner; its underside height (`CHILD_INDENT`) is exactly
  where children's panels begin.
- **One drop grammar in both views**: a strip/row's trailing quarter
  (`folder_drop_strip` — bottom in arrange, right in the mixer) snaps
  into the folder; landing on a body takes the landing row's membership;
  a drag that comes home EJECTS in place (tail child by flag, middle
  child just past the run). Both gestures are pinned by event-level
  simulations in `app.rs` tests.

## No Untitled folders. Period.

Nothing is written at launch or New. The template is adopted IN MEMORY
(`adopt_template`), aimed at a prospective folder that does not exist.
The folder MATERIALISES at the first thing worth keeping, always through
`materialize_project_dir` (the one door, below): a Save (the first save
moves it to the chosen name), a kit edit or pad load, an import, an
arrangement take with audio, a freeze, a glue, or a realtime export pass
(its staging copy lands in the project's `export/`); a MIDI-only take
makes no folder. Renaming before that repoints alone, so the folder is
born under the chosen name; any of those writers makes `Untitled` real
without a save. The launch sweep (`sweep_disposable_untitleds`) removes,
WHOLE, every folder directly under the project root named `Untitled` or
`Untitled <n>` (`is_scratch_name`) whose `media/` holds nothing
`count_media` counts, whose kit is not custom, whose document is missing,
empty, or unnamed and clipless, and which holds nothing but what Tangent
writes into a scratch (`holds_only_scratch`: the document, the kit and its
write temp, `state/`, `export/`, `media/` with files only, dotfiles). A
subfolder, a nested project or anybody's own file keeps the folder; it used
to go with it. New's reclaim (`fresh_project_dir`) and Don't Save
(`discard_scratch_made_here`) apply the same contents rule.

Related invariants that keep this working:

- The session's document is written by a save the hand asked for: Save,
  the prompt's Save, the first save (`save_project_first`, which MOVES a
  materialised scratch to the chosen name) and Save As (a copy the session
  then stands on). Two side roads write it too: Save As Template from a
  REAL folder (`save_project_and_states`, before the copy), and the
  save-back on New and Open (`switch_project`, the New arm), which writes
  only when `project_dirty_exact()` and the prompt was not answered Don't
  Save. `on_exit` never writes it. Plugin states always go to the global
  shelf first (`save_plugin_states`); `copy_states_to_project` mirrors them
  into the project's `state/` only when the answer was not Don't Save, and
  stands down while the folder is unmaterialised.
- **Save As Template never makes a scratch real** (2026-09-22). A session
  whose folder does not exist yet is not written: its template is built
  from memory (`write_template_into`) into a staging folder and swapped
  onto the shelf (`swap_in_template`), and the scratch is marked as needing
  no save (`mark_template_saved`). A REAL folder (a saved project, or a
  scratch something already materialised) is still saved first
  (`save_project_and_states`) and copied whole, as it always was. The old
  road saved the scratch into `Projects/Untitled` first, which was the
  pile's main source. Every template Save As Template writes carries a
  `drumpad.json`, an empty kit included, because `kit_is_custom` compares
  against it; one seeded by a fresh install has none, and with no template
  kit `kit_is_custom` calls a kit work only when it holds a sample or
  cannot be parsed.
- **New, Open and Quit ask, and "Don't Save" sticks.** The menu's and the
  Project window's New and Open go through `ask_before`, as Quit does
  (Settings > Reset does not: The Default Template replaces the session
  without asking, and Everything opens a fresh one through New with the
  session DISCARDED - `reset_settings` commits the reset as a moment, then
  gives Discard's answer, so the emptied desk is never written into the
  open project's document - and keeping, so a leftover Untitled with takes
  is never reclaimed unasked). The
  prompt's Discard reaches the host as `take_outgoing_discarded` and gates
  the document save-back AND the project plugin-state mirror in
  `switch_project`, the New arm and `on_exit`, and it survives New's detour
  through the scratch warning (`rearm_outgoing_discarded`). Then a scratch
  THIS session made real (`made_real_this_session`), named `Untitled` or
  `Untitled <n>`, unnamed and clipless ON DISK, off the shelf, holding
  nothing but Tangent's own files, still on the template's kit and with no
  recording (`rec-*`) is removed WHOLE (`discard_scratch_made_here`): its
  kit sidecar, the samples and imports it copied in, freeze renders, glued
  files, `state/` and the export staging. A kit built there keeps the
  scratch, as it keeps it from New's reclaim and the sweep: the
  conservative answer, pending the owner's ruling on whether Don't Save may
  take a kit. Open, Quit and
  New all leave a scratch the discard refuses on disk: New starts beside
  it (`new_starts_beside`, the next free `Untitled <n>`). A clean session
  gets no prompt, so New asks about its scratch's takes with the scratch
  warning first, as it does for a leftover one (`new_asks_about_scratch`);
  it used to reclaim it, a take undone off the timeline included.
- **A running arrangement record ends in the session it was recorded in.**
  Every road out finishes it first (`finish_arr_record`): a window close
  before the quit question, so the question covers the take; Open before
  the save-back, which keeps it; New before it decides anything, asking
  New's question again when the take is unsaved work nobody was asked
  about (`ask_before_new`); and `on_exit` for one still running. The
  roads take the save prompt's answer before finishing and put it back
  (`finish_arr_record_keeping_the_answer`), since the finish commits a
  moment and a new moment clears Don't Save. A close used to drop the take
  unwritten, and a New or an Open carried it into the next session. New's
  reclaim treats a document holding clips as work, as the sweep and the
  discard do, so a scratch Open wrote back with a take is never reclaimed.
- **One door makes a scratch real**: `materialize_project_dir`, which brings
  every lane's kit samples and writes the kit sidecar. Imports, pad loads,
  arrangement takes with audio, freezes, glues and realtime exports go
  through it first; a
  MIDI-only take makes no folder, and an UNDO never does (it writes the kit
  only into a folder that is already real).
- **The template's kit is adopted, not the last project's**: `adopt_template`
  loads the kit beside the template once it has repointed. A template opened
  from the Open panel is ADOPTED the same way (`open_project`), into the
  first free `Untitled` name that is not on disk, and its plugin states come
  off its own `state/` (`switch_project`). It used to be read and not
  inhabited: the session stayed in the previous project's folder, so the
  next Save moved that folder to the name typed and wrote the template over
  its document, and the next Open or Quit mirrored the template desk's
  plugin states into that project's `state/`. And a first save never moves
  a folder whose document on disk is named (`save_project_first`): an undo
  past a first save leaves the session unnamed inside a kept project, and
  that folder is copied from, not moved.
- **No test resolves the owner's media folder**: `Settings::media_root`
  honours `TANGENT_MEDIA_ROOT` in every build and is a per-thread temp root
  under `tangent-ui`'s tests. One test wrote a blank document over the real
  `Projects/Untitled` on every `cargo test` until 2026-09-22.
- **The sweep counts takes, not litter**: `count_media` skips dotfiles
  (Finder's `.DS_Store`), freeze renders and byte-copies of the template's own
  media (a file is read whole only when the template holds one of the same
  name and length). A kit is compared by its
  parsed kits, not its bytes; with no template kit, only a kit holding a
  sample, or bytes nobody can parse, is work.
- The canonical document name is `project.tan`; `project.tangent` reads
  forever. `arrangement::doc_in` is the one resolver; no call site may
  bare-join either name.
