# Lane brief: the theme redo (2026-09-18)

Two agents are working the repo at once. This lane owns the THEMES. The
other lane (the session that wrote this file) owns the PERFORMANCE sweep
against REAPER. The file lists below are the contract that makes the merge
mechanical. Read `docs/spec/ui-spec.md` section 15 and the module docs at the
top of `tangent-ui/src/theme.rs` and above the AUTO-PALETTES table in
`tangent-ui/src/mixer_panel.rs` before touching anything.

## The ask, verbatim from the owner

1. Valentine: keep, but more red, like the original TUI application.
2. Studio: replace with something more inspired and not yet covered by the
   rest of the themes.
3. Aurora: same as Studio.
4. Sandstone: keep, but make even light mode a darker sand colour; it is
   too harsh now.
5. Centennial: keep, but make the recorder colour slightly darker grey.
6. Blueprint: replace like Studio (it is essentially an alternate Navi).
7. Porcelain: replace like Studio.
8. Dusk and Iris are too close: replace them.
9. Nocturne is also just Navi: replace.

"All themes must be color checked, and have their ratios fully subjected to
all theming spec and suites - I want them legible the first time."

## Rules already in the code that constrain the redo

- A theme is a set of `Settings` colour roles (`theme.rs` module doc). The
  factory list is code; `factory()` order is an INDEX that saved desks
  persist as `palette_choice`, and the AUTO-PALETTES table in
  `mixer_panel.rs` is in the same order. Replace a theme IN ITS SLOT; never
  remove or reorder. The colour runs behind `base`/`master_at`/`ext`/`ext2`
  are append-only history: append new runs, never renumber old ones.
- Ink is a property of the SURFACE, never of a role or a threshold. A
  surface whose luminance sits in the 0.12 to 0.22 dead zone cannot carry
  legible ink either way: move the surface, not the ink.
- `scripts/palgen.py` and `scripts/velgen.py` generate palette material and
  the per-theme velocity colours; run them before hand-editing Rust, and
  keep their tables in step with `factory()`.
- The acceptance is the existing suite. Every `every_theme_*` test in
  tangent-ui (notify, mixer_panel x3, shell, midi_editor, theme, faces,
  face, staff, piano x4, arrangement_panel x2, busy, theory_panel, dialogs,
  menu) must pass UNCHANGED. Do not loosen a threshold to admit a colour;
  change the colour. The full gate is `scripts/check.sh` (workspace tests,
  core, differential sweep, clippy with `-D warnings` on our five packages).
- Never `cargo fmt -p tangent`. The repo's legacy `unused_qualifications`
  backlog is not to be swept; only lines you add must be clean.
- No em dashes in anything the owner will read (changelog, docs, commit).

## Files this lane MAY write

- `tangent-ui/src/theme.rs` (the factory themes and their doc comments)
- `scripts/palgen.py`, `scripts/velgen.py`
- `tangent-ui/src/mixer_panel.rs`: ONLY the AUTO-PALETTES table (the
  `Palette { name: ... }` rows, about lines 925 to 950) and any run data it
  appends to. Nothing else in that file.
- `tangent-ui/src/piano.rs`: ONLY the per-theme name lists (about line 949,
  `["Nocturne", "Phosphor"]`).
- `tangent-ui/src/app.rs`: ONLY lines that name a theme by string. Known
  sites: 7191 (Centennial check), the tests near 52876, 52941, 52970, 53546,
  58779 (Iris and Centennial). If a replaced name appears in a test, rename
  the test's choice; do not restructure the test.
- `docs/spec/ui-spec.md`: section 15 (colour reference) only.
- `docs/LANE-THEMES-2026-09-18.md` (this file): append a `## Handoff`
  section at the end with the CHANGELOG text for the redo. Do NOT edit
  `CHANGELOG.md`; the perf lane folds your handoff text in at merge time so
  the two lanes never race on the same hunk.
- Theme screenshots under `docs/img/` if you make any.

## Files this lane must NOT write

- `CHANGELOG.md`, `Cargo.toml`, `Cargo.lock`
- `tangent/**` (the host, engine, units, plugin hosting), `tangent-host/**`,
  `tangent-record/**`, `tangent-core/**`
- `tangent-ui/src/app.rs` outside the theme-name lines above
- `tangent-ui/src/mixer_panel.rs` outside the palette table
- `tangent-ui/src/{arrangement*.rs, recorder*.rs, instrument*.rs,
  settings.rs, desktop*.rs}` and every `every_theme_*` test body
- `scripts/check.sh`, `scripts/build-macos.sh`, `scripts/install-macos.sh`

Read anything you like.

## Where to work

Cut a worktree by hand from `main` HEAD, OUTSIDE Dropbox (Dropbox breaks
cargo's target dir and a worktree inside it inherits that), on a branch
named `themes-2026-09-18`:

    cd ~/Library/CloudStorage/Dropbox/Projects/Apps/tangent
    git worktree add -b themes-2026-09-18 ~/Library/Caches/tangent-lane-themes main
    cd ~/Library/Caches/tangent-lane-themes
    export CARGO_TARGET_DIR=~/Library/Caches/tangent-lane-themes-target

If `main` still shows uncommitted changes in the Dropbox tree when you cut
the worktree, they belong to the perf lane's previous batch and are not in
your base; that is fine, none of them touch your files.

Use the rustup toolchain, not Homebrew's cargo:
`export PATH=$HOME/.rustup/toolchains/1.97.1-aarch64-apple-darwin/bin:$PATH`
(check `rustup which cargo` if the version moved).

## Handing back

1. `scripts/check.sh` green in the worktree.
2. `git rebase main` in the worktree (main will have moved), gate again.
3. Commit per theme or per small group with the prefix `theme:`; end each
   commit with the attribution lines your session was given.
4. Append the `## Handoff` section here (what changed per theme, the
   CHANGELOG paragraph, anything the owner should look at first).
5. Tell the owner the branch is ready. The perf lane merges it with
   `git merge --no-ff themes-2026-09-18`, folds the changelog text in, runs
   the gate on the merged tree, builds and installs. Never launch Tangent
   to check a build; install and report, the launch is the owner's.
