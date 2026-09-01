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
- The canonical document name is `project.tan`; `project.tangent` reads
  forever. `arrangement::doc_in` is the one resolver; no call site may
  bare-join either name.
