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
The folder MATERIALISES (`materialize_project_dir`) at the first thing
worth keeping: a Save, a kit edit, a recorded take (whose writer creates
`media/` itself). Renaming before that repoints alone, so the folder is
born under the chosen name; "Untitled" reaches disk only if the user
saves while keeping it. The launch sweep
(`sweep_disposable_untitleds`) clears strays that provably hold no work —
unnamed, clipless, stock kit, no takes; any doubt spares the folder.

Related invariants that keep this working:

- The document writes ONLY on Save (autosave is gone by the owner's word);
  `switch_project` gates the doc save on `project_dirty()` but always
  shelves plugin states — and `copy_states_to_project` stands down while
  the folder is unmaterialised.
- **Save As Template is not a save of the session** (2026-09-22). A
  scratch's template is written from memory (`write_template_into`) into
  a staging folder and swapped onto the shelf; only a REAL folder is saved
  and copied. The old road saved the scratch into `Projects/Untitled` first,
  which was the pile's main source. Every template carries a
  `drumpad.json`, because `kit_is_custom` compares against it.
- **Every door out asks, and "Don't Save" sticks.** The Project window's New
  and Open go through `ask_before` like the menu's; a "Don't Save" answer
  reaches the host as `take_outgoing_discarded` and gates the document
  save-back AND the plugin-state shelving in `switch_project`, the New arm
  and `on_exit`, and it survives New's detour through the scratch warning
  (`rearm_outgoing_discarded`). A scratch THIS session made real
  (`made_real_this_session`), named Untitled, unnamed and clipless ON DISK,
  off the shelf and holding no recording (`rec-*`) is removed when discarded
  (`discard_scratch_made_here`) - its kit, imports and shelf were the
  session's own litter.
- **One door makes a scratch real**: `materialize_project_dir`, which brings
  every lane's kit samples and writes the kit sidecar. Imports, pad loads,
  arrangement takes with audio, freezes and exports go through it first; a
  MIDI-only take makes no folder, and an UNDO never does (it writes the kit
  only into a folder that is already real).
- **The template's kit is adopted, not the last project's**: `adopt_template`
  loads the kit beside the template once it has repointed. A template opened
  from the Open panel is read without being inhabited, so it keeps the
  session's own kit, where every kit write goes.
- **No test resolves the owner's media folder**: `Settings::media_root`
  honours `TANGENT_MEDIA_ROOT` in every build and is a per-thread temp root
  under `tangent-ui`'s tests. One test wrote a blank document over the real
  `Projects/Untitled` on every `cargo test` until 2026-09-22.
- **The sweep counts takes, not litter**: `count_media` skips dotfiles
  (Finder's `.DS_Store`), freeze renders and byte-copies of the template's own
  media (length-checked first, so no take is read). A kit is compared by its
  parsed kits, not its bytes; with no template kit, only a kit holding a
  sample, or bytes nobody can parse, is work.
- The canonical document name is `project.tan`; `project.tangent` reads
  forever. `arrangement::doc_in` is the one resolver; no call site may
  bare-join either name.
