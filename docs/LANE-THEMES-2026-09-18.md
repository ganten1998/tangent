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
- `tangent-ui/src/settings.rs`: ONLY the lines that name a theme by
  string. Known sites: the backup round-trip near 7125 and 7133, and the
  reset test near 7574. Added to this list on 2026-09-18 after the theme
  lane found them: `Iris` is one of the nine being replaced, and it is
  named in these tests as a factory theme, so the gate cannot go green
  without them. Rename the test's choice; do not restructure the test. The
  perf lane confirmed it edits no theme-naming test in this file.
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


## Handoff

Branch `themes-2026-09-18`, cut from `main` at 3c76e4b. `scripts/check.sh`
green. Three commits, all prefixed `theme:`.

### What changed, per theme

Three were retuned in place at the owner's word and seven were replaced in
their slots. No slot moved, so every saved desk's `palette_choice` still
means what it meant, and the seven retired runs keep every byte they had in
`STRIP_COLORS`.

| Slot | Was | Is | What the owner asked for, and what was done |
|---|---|---|---|
| 1 | Valentine | Valentine | "More red, like the original TUI application." The marks were always the TUI's own; what read purple was the SURFACES. The band moved from hue 314 to 342 and the desk from 319 to 338, the lines, strings and key cuts with them, and the dark ground went from a violet black to a plum one. |
| 5 | Studio | **Ochre** | The pigment as the band: the one warm band in the set with real chroma, chalk page, bone knobs, cornflower for what sounds and indigo for what is held. |
| 6 | Aurora | **Reed** | The reed electric piano this app models, in the avocado the 200 wore: olive band, cream keys, the script's coral for what sounds, pale sage for what is held, an amber pilot lamp on the master. |
| 7 | Sandstone | Sandstone | "Make even light mode a darker sand colour; it is too harsh now." The page came down from a near-white at 0.90 luminance to sand at 0.63, the desk and the neck with it. The band stayed near where it was on purpose, and the dark ground went deeper so the piano still never has to lift its sharps. |
| 8 | Centennial | Centennial | "Make the recorder colour slightly darker grey." Seven percent down, with the two gold marks deepened a step so they keep 3:1 on it. |
| 9 | Blueprint | **Zinc** | Blueprint was "essentially an alternate Navi", so the one hue the replacement must not be is blue. Zinc weathers to a blue-violet grey and nothing in the set is violet at all since Dusk and Iris went, so the band is that patina with brass for what is held and a pale zinc lilac for what sounds. |
| 10 | Porcelain | **Celadon** | The glaze: the third light desk, cool where Sandstone is warm and green where Centennial is grey, with plum for what sounds and pine for what is held, both dark, as marks on paper are. |
| 14 | Dusk | **Prism** | Charcoal, and one colour per control, the slider caps of a polysynth: cyan sounding, magenta held, yellow knobs, a red master, green on the scope, orange on the chip. |
| 15 | Iris | **Storm** | Slate and the bolt: a blue-grey band darker than any other in the set, cloud-white knobs, and yellow, the one hero colour nothing else uses, for what sounds and for the pitch dial. |
| 16 | Nocturne | **Amber** | Nocturne was "also just Navi". Amber is the other phosphor: Phosphor is the green console and this is the amber one beside it, designed dark the way that pair is, with the same warning red on the master. |

Seven palette runs were designed for the seven new themes and appended
whole, the way Silverface's was, so `STRIP_COLORS` went from 708 entries to
946 and no existing index moved. The velocity pairs were re-solved for all
nineteen themes against each theme's own roll.

### What to look at first

**Zinc began as a mid-tone desk and could not stay one**, and it is the one
place where a design lost to a test rather than to taste. The first design
for that slot was a pewter band at 0.30 luminance carrying dark ink: a
daylight desk that does not glare, which is genuinely not covered by the
rest of the set and is what Sandstone's "too harsh" is really asking for.
Every colour gate passed. `dialogs::every_theme_dresses_the_export_window`
did not: a well in the middle of the luminance range sits within 3:1 of this
app's one fixed transport red, so the playhead had to be recoloured on all
four of that theme's dressings, where the whole set is allowed seven. A desk
that makes the app move its own red everywhere is a desk fighting the
transport, so the concept yielded and the metal stayed. If a daylight desk
is still wanted, it needs that red question answered first.

**The scripts do more than they did.** `palgen.py --themes` now carries the
app's own derivations rather than only its role colours: the piano's key
swap and sharp lift, the marker strip, the varispeed readout at rest, the
sampler's browser rows under their two veils, the strip face in both modes,
and the export window's playhead tally. Seven of the nine themes passed
every gate in that file and then failed five suites in `tangent-ui`, because
those suites measure surfaces DERIVED from the roles and nothing in the
script knew how to derive them. They are ported now, named for the tests
they answer, so the Python fails first. `velgen.py` reads `theme.rs`
directly; it used to read a JSON dump of the theme table left in a scratch
folder by the session that first ran it, and that folder was gone.

### A correction that predates this branch

`scripts/palgen.py --check` was red at 3c76e4b, two failures, and it is
green now. Commit 0a0f57b changed `STRIP_COLORS` index 300 (Default's
per-palette master) from `#0a5fc2` to `#b02a37` without updating the frozen
copy in the script, so the script and the table disagreed about a colour.
The table is the source of truth and the runs are frozen history, so the
script's copy moved to match. Two things fell out of it: the byte-identity
claim now lowercases hex literals before comparing (index 300 is spelled
with capitals in the Rust and every other row with lowercase, and a
generator that insists on one spelling is pinning a typographic habit), and
the reporter was counting errors it never printed.

That second one is worth stating plainly, because a script whose failure
count is not the count of what it shows you is a script whose green runs
prove nothing. The tail filtered on "the prefix is not a run name", and
every byte-identity, master and extension error in the file is prefixed
with a run's name, so none of them could ever be printed. At 3c76e4b that
was the whole of the output: `CHECK FAILS (2)` and not one word about
which two or why. Both were the index 300 drift. The reporter prints
whatever the per-palette loop did not, and the honest run today, on this
branch, reports zero errors across all twenty-seven runs in the table,
retired ones included. It is its own commit,
`theme: palgen's frozen copy of index 300 follows the Rust`.

### CHANGELOG text

```
### Changed

- **Ten themes: seven replaced and three retuned.** The owner's list, in
  his words: Valentine keeps its slot with more red, "like the original TUI
  application"; Sandstone keeps its slot with a darker sand, because "even
  light mode" was "too harsh"; Centennial keeps its slot with a slightly
  darker recorder. Studio, Aurora, Blueprint and Porcelain were replaced
  with "something more inspired and not yet covered by the rest of the
  themes"; Dusk and Iris were "too close" to each other; Nocturne was "also
  just Navi". In their slots now: Ochre, the pigment as the band, with
  cornflower and indigo; Reed, the reed electric piano this app models, in
  the avocado the 200 wore; Zinc, the patina metal and the only violet in
  the set, with brass for what is held; Celadon, the glaze, cool where
  Sandstone is warm; Prism, charcoal with one colour per control; Storm,
  slate and the bolt, the only theme built on yellow; and Amber, the other
  phosphor console beside the green one. Every theme in the set was colour
  checked in both modes against the whole suite before it shipped, which is
  what the owner asked for: legible the first time.

  No slot moved. A theme's position is the index a saved desk keeps as its
  palette choice, so each new theme took the retired one's row and each
  retired run kept every byte it had in the strip table; a desk that painted
  a track from one of them still paints it the same colour. The seven new
  palettes were appended whole, and the velocity lane's colours were
  re-solved for all nineteen themes against each theme's own roll.
```

### Verification

- `scripts/check.sh` green in the worktree, rebased on `main`.
- `tangent-ui` alone: 1436 passed, 0 failed. Every `every_theme_*` suite
  passes with no test body changed; the only edits to `app.rs`,
  `settings.rs` and `piano.rs` are theme names inside tests, which the brief
  and the perf lane allow.
- `python3 scripts/palgen.py --check`, `--themes`, `--redo` and
  `python3 scripts/velgen.py report` all pass.
- The script's designs and the Rust table were compared role by role for
  every theme designed in Python: no mismatches.
- Tangent was not launched.
