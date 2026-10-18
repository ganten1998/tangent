# Tangent 5.1 worklist

Derived from `docs/AUDIT-5.1.md` at `36b1f56` (5.0.0-beta.15). **This file is the working copy —
tick items as they land and move them to the bottom.** The audit is the reasoning; this is the list.

**The rule that keeps this useful:** Part 2 is as important as Part 1. Anything in *SETTLED* has
already been checked and is either deliberate, already fixed, or was a false alarm. Do not
re-investigate it, and do not accept a future audit that raises it without new evidence. If an
agent tells you one of those things again, the agent is wrong and the reason is recorded here.

Legend: `file.rs:NN` is where to go. **DONE WHEN** is the test that closes it.

---

## Part 1 — OPEN

### Tier 0 · irreplaceable

- [X] **W1 · Pull the supporter ledger off Fly.** MOOT 2026-09-07, the owner's ruling. The finding
  itself still reads true against the machine — `~/.tangent-signing/ledger.jsonl` carries sixteen
  rows and no email addresses — but the loss it was guarding against is not one worth guarding.
  Do not re-raise it; a future audit that finds the same shape has found the same non-problem.
  W33 (the Dropbox backup lagging the live file) is a separate item and stays open.

### Tier 1 · silent correctness bugs, every user, every platform

- [ ] **W2 · A corrupt project opens as an empty one.** `arrangement.rs:766` returns
  `Self::default()` on any parse failure and `:705` does the same on any read error. `open_project`
  then resets undo, so it is not dirty and no prompt fires.
  **DONE WHEN** missing, empty and malformed are three different outcomes and only the first opens silently.
- [ ] **W3 · "Nothing saves itself" is false.** Six non-test writers of the document, four of them
  implicit: `app.rs:6747` (finishing a recording), `:6762` (clearing the timeline), `:6780` (a clip
  length annotation), `:10191` (**every undo and redo**). Chained with W2 this is real data loss.
  **DONE WHEN** either the four are explicit, or the claim is removed from the docs.
- [ ] **W4 · Master bays 4–6 never load in the live engine.** `inserts_loaded` is sized
  `(STRIPS+1+DRUM_PADS)*INSERTS` = 444 at four sites in `desktop.rs` (1701, 1796, 2343, 5769), but
  `MASTER_EXTRA_SEAT` **is index 148** and needs slots 444–446. Settings and the export both use
  `INSERT_SPACE` (6207) correctly, so the plugin is silent while mixing and present in the bounce.
  Per-channel pad racks past the first channel are outside the array too.
  **DONE WHEN** all four sites use `recorder::INSERT_SPACE`, with a test.
- [ ] **W5 · The export ignores time-stretch.** `desktop.rs:5171` applies `clip.stretch` on the live
  path; `start_offline_bounce` (~`:4547`) does not. Same failure class the file already fixed once
  for `clip.gain` — the comment is three lines above.
  **DONE WHEN** one function is shared by both paths, with a bounce-length test.
- [ ] **W6 · Two open LV2 editors cross-talk.** `lv2ui.rs:87` is a process-global `EDITS` queue and
  every UI is instantiated with a NULL controller (`:356`), so `drain_edits` cannot tell them apart.
  Same EQ on channels 1 and 3 → turning a knob in the second edits the first.
  **DONE WHEN** the UI's own handle is the controller and `drain_edits` filters on it.
- [ ] **W7 · UI and engine can disagree about folder membership.** `app.rs:2647` requires a
  contiguous run; `app.rs:17747` never clears `nearest` when the run ends. A stale flag makes the UI
  draw a track flat while the engine folds it into the group.
  **DONE WHEN** a test asserts the two agree over random flag soup.
- [ ] **W8 · `make_folder` fails silently** when no spare channel exists (`app.rs:17705`). On a busy
  16-track session, grouping just does nothing.
- [ ] **W9 · Route 65 silently never sounds.** `Routes::add` has no cap; `set_routes` does
  `.min(MAX_ROUTES)`.

### Tier 2 · text that is false or sells the wrong product

- [ ] **W10 · The Windows Media Foundation claim.** `dialogs.rs:1567` tells Windows users encoding
  "runs on Media Foundation, so three 1080p30 streams cost the CPU almost nothing". There is no MF
  code; Windows runs the same `libx264` as Linux. Also in doc comments at `dialogs.rs:1558` and
  `recorder.rs:2318`.
  **DONE WHEN** the Windows arm folds into the software-encoder arm.
- [ ] **W11 · About still says "Simple MIDI Keyboard Monitor with Advanced Chord Detection"**
  (`dialogs.rs:5656`), directly above "Version 5.0.0-beta.15". Draw the Tonnetz mark there too.
- [X] **W12 · README sells the retired VST3 plugin** (`:37, :45, :53-64, :134-137`) and links
  Gumroad at the pre-rename permalink — the one commercial URL the rename was supposed to protect.
  *(2026-10-10: the plugin half is closed. The README no longer sells it (012aebd), and with the
  plugin crate removed its "every crate in this repo is MIT" line is true. 2026-10-18: so is the
  Gumroad half. The permalink is `/l/tangent`, the old one redirects to it, and the README,
  README.txt and the key dialog link the new one.)*
- [ ] **W13 · README claims the config path was never changed** so nobody's settings are orphaned.
  False in all three of its claims now (crates renamed, bundle id renamed, config path changed).
- [ ] **W14 · `docs/ARTIFACT-README.md`** — shipped as `README.txt` inside *every* artifact — says
  Windows has "no installer" (`:27`, there is one) and macOS "requires Apple Silicon" (`:23`, the
  build is universal).
- [ ] **W15 · `docs/RELEASE.md` is the stalest doc in the repo and it is the release checklist.**
  Old product name in the title, "ad-hoc signed only" (contradicted for many releases), old artifact
  names, and a trademark gate about the *previous* name. Following it ships the wrong thing.
- [X] **W16 · The key-paste error named the old product.** `license.rs:96, :102, :106` — the
  first thing a new supporter sees when a paste goes wrong. *(2026-08-31, 99c658ee: it says "a
  Tangent supporter key", and a test holds it there.)*
- [X] **W17 · Store copy names the wrong sender.** `STORE-CONTENT.md:174` tells buyers the key
  arrives from the old sender domain. *(2026-08-31, 2cf184b5: it names keys@tangentdaw.org.)*
- [ ] **W18 · `vst3_locations()` points FreeBSD at a path the scanner excludes.** `dialogs.rs:2222`
  else-arm names `/usr/lib/vst3`; `scan.rs:158` deliberately excludes it. Never mentions LV2, which
  on FreeBSD is the only format that exists.
- [ ] **W19 · `routing.rs:12` says a route "is additive and never steals".** The engine does the
  opposite (`instrument.rs:4061` zeroes `route_direct`). The dialog copy is correct; the module doc
  is backwards and is the first thing anyone reads before touching routing.
- [ ] **W20 · All four README screenshots predate the 5.0 batch.** They show a chord monitor: no
  recorder, mixer, arrangement, editor, drum machine or themes. `TANGENT_SHOT` already renders
  deterministic frames headlessly, so this is a script.

### Tier 3 · packaging and release machinery

- [ ] **W21 · Stock Ubuntu GNOME cannot start the app.** `liblilv-0.so.0` in `DT_NEEDED`, no
  RUNPATH, no bundled copy. Present by default everywhere else checked (see SETTLED S12).
  **DONE WHEN** the guard in W27 exists and either lilv ships with an rpath or the README declares
  it. Note `install.sh:511` copies only the binary to `~/.local/bin`, so a bare `$ORIGIN` resolves
  where the libraries are not — use `$ORIGIN:$ORIGIN/../lib/tangent`.
- [ ] **W22 · 5.0 cannot be published.** `publish-github.sh:203` refuses without `--notes-file`;
  CHANGELOG stops at 4.5.0 with `[Unreleased]` empty. Write one `## [5.0.0]` entry covering the arc
  plus a line saying 4.6–4.93 were internal. **Do not backfill ninety entries.**
- [ ] **W23 · `Tangent-macos.pkg` is built by no script in the release path.** The builder exists and
  is good; `release.sh`'s Darwin branch never calls it, and the supporter email hardcodes that link.
- [ ] **W24 · `setup.exe` is never Authenticode-signed.** `sign_windows_exe` signs the *inner*
  binary only; `build_windows` runs makensis and stops. `release.sh` fails an unsigned pkg but not
  an unsigned exe.
- [ ] **W25 · The Windows zip alias will 404 on the first stable publish.** `build-cross.sh:513`
  deletes the zip as a transient input, so the versionless alias is never uploaded. No beta has
  exercised this path. README `:52` still links it.
- [ ] **W26 · FreeBSD is outside the release machinery entirely.** `release.sh` exits on any non
  Darwin/Linux host, and the package filename matches neither the checksum glob nor the upload
  globs. Decide whether it ships.
- [ ] **W27 · No `DT_NEEDED` guard in `package_linux()`.** `build-freebsd.sh:174-191` is the template
  and its comment already states the reasoning. The tarball path never inherited it.
- [ ] **W28 · Free win: the glibc floor is self-imposed.** `build-cross.sh:21` pins 2.32; the true
  API floor is 2.30, driven by one symbol. Retargeting adds Ubuntu 20.04, Debian 11 and Leap
  15.4/15.5 at zero cost.
- [ ] **W29 · `THIRD-PARTY-LICENSES` is ~170 commits stale** and omits **41 shipped crates**. Also
  still says "SIX font files" while ten ship. `docs/RELEASE.md` names regenerating it as a hard gate
  that was not run.
- [ ] **W30 · LAME/LGPL attribution missing.** Statically linked on **macOS and FreeBSD** (not Linux
  — the cross build drops it). Needs the notice, the LGPL text, and a relink route.
- [ ] **W31 · ffmpeg corresponding source is not hosted.** A pointer to upstream is not the
  corresponding source of *that* build. Gumroad copy carries no licence notice at all.
- [ ] **W32 · No `TRADEMARK.md`.** MIT covers the code, not the name. `RELEASE.md`'s gate still asks
  about the *old* name's collision.
- [ ] **W33 · Ledger backup stale since 11 August**, Dropbox-only (violating the keygen tool's own
  threat model), and the folder still carries the old name.
- [ ] **W34 · Fulfilment runbook drifted from the service.** `SETUP.md` step 4 says
  `fly volumes create` with the pre-rename volume name; `fly.toml` mounts `tangent_data`.
  Following the runbook stands up a service whose ledger is ephemeral. Confirm the historical
  ledger rode across the rename.
- [ ] **W35 · Double-minting bug.** `tangent-fulfil/src/main.rs:113` uses `?` on a per-line parse
  *inside* the loop, so one blank or malformed row makes every later sale look unfulfilled and mint
  a second key. The sibling function skips bad lines deliberately.
- [ ] **W36 · `sale_is_genuine` has no product-id and no refund/dispute check**, and keys are
  perpetual with no revocation path.

### Tier 4 · discoverability — why 170 commits are invisible

- [ ] **W37 · Zero automation entries in `keys.rs`.** Every automation gesture is discoverable only
  by clicking.
- [ ] **W38 · Tempo-lane gestures on no card** (shift-add, alt-remove, shift-ramp).
- [ ] **W39 · `keys.rs:461` says shift-drag stretches "a MIDI clip"** — false since audio stretch
  landed. `keys.rs:474` has the mirror problem for glue.
- [ ] **W40 · No persistent help affordance.** Hold-H is taught once in the Welcome card and never
  again; no corner glyph, no "Keyboard Shortcuts" menu row.
- [ ] **W41 · No empty-state hint on a blank timeline** — the one place the missing affordance costs
  most, because it is the moment of need.
- [ ] **W42 · The window title is the constant "Tangent".** `project_name()` and `project_dirty()`
  both already exist and are never composed into it.
- [ ] **W43 · In reader mode the card lies about Tab.** egui takes Tab for the overlay; F6 is the key
  that actually cycles views and it is `shown: false`, so it prints nowhere.
- [ ] **W44 · Chorded bindings are still hand-written literals** (`keys.rs:441-505`). The drift test
  is one-directional, substring-only, skips `S-1/2/3` and `C-Z`, and does not cover the Mixer card.

### Tier 5 · accessibility correctness (fix as bugs; do not market)

- [ ] **W45 · macOS never detects a screen reader.** `desktop.rs:6487` is literally
  `#[cfg(target_os = "macos")] { false }`. **The stated reason is stale and this is now cheap:** the
  comment says "`NSWorkspace.isVoiceOverEnabled` needs objc2, which this crate does not take", but
  `tangent/Cargo.toml:258` takes `objc2 = "0.6"` and `:259` `objc2-app-kit`. Read
  `isVoiceOverEnabled`, or observe `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification`.
  **DONE WHEN** a Mac with VoiceOver running gets reader mode without touching the menu.
- [ ] **W46 · The only door to reader mode is a right-click row**, gated to the desktop, with no
  keybind — so a keyboard-only Mac user can reach neither the menu nor the toggle.
- [ ] **W47 · Master automation rows read "automation, none" to a reader** — the label goes through
  `view.lanes`, which has only `CHANNELS` entries.
- [ ] **W48 · The one-shot reader-mode migration wipes a deliberate setting.** A macOS VoiceOver user
  who set it by hand loses it on first launch of this build, with no notice.

### Tier 6 · visual

- [ ] **W49 · The drum machine reaches no theme** — `drumpad_panel.rs:78` hard-codes face and ink.
  On Porcelain or Iris the whole app moves and it stays walnut.
- [X] **W50 · The export progress window is hard-coded bone** (`dialogs.rs:2877`) and is on screen
  for minutes, with no stated rationale (unlike the ReaStream dialog, which has one).
  *(2026-08-31, 99c658ee: it wears the desk's face, and a test holds it there.)*
- [ ] **W51 · There is no dark theme.** All sixteen factory backgrounds are light; "Dusk" is a pale
  lilac. Decide the light/dark question and add two or three.
- [ ] **W52 · The 16 px icon is an illegible smudge** — the source art has no silhouette at that
  size. Also the ico frames are non-square (256×255 / 128×127).
- [ ] **W53 · `theme::seed_example()` is dead code**, so the "user themes" folder is empty until you
  save one — and its example text names the retired palette set.
- [ ] **W54 · `palgen.py` no longer reproduces the shipped palettes.** It emits the retired abstract
  names; the Rust has 16 theme-named ones. The shipped table is not regenerable from the tool.
- [ ] **W55 · Hand-picked channel colours skip `legible_surface`** (`mixer_panel.rs:1551`,
  `arrangement_panel.rs:4530`), so three dead-zone swatches can be chosen manually.

### Tier 7 · process and hygiene

- [ ] **W56 · Nothing runs the tests.** No CI, no `.github/`, no hooks, and no build or release
  script invokes `cargo test`, `clippy` or `fmt`. 1,795 tests depend on one person's habit.
- [ ] **W57 · Add a clean-box boot to the release path** — a container per platform with no dev
  packages, unpack, run, assert a window. **This is the higher-value process change**, because it
  catches W21 and the missing-installer class that bit 4.92, which `cargo test` cannot.
- [ ] **W58 · No `rust-toolchain.toml`**, so the toolchain floats and nothing re-validates the lint
  policy. This is the root cause of W59.
- [ ] **W59 · The lint policy's numbers are three majors stale.** It claims the build stays
  warning-free (it emits ~670, or 998 with `--all-targets`, mostly one cosmetic lint it enabled
  itself), claims 33 unwraps against 242, and cites version 2.1.0. `cargo fmt --check` is 2,169
  hunks against a documented 171.
- [X] **W60 · The rename is incomplete in code.** The app struct (now `TangentApp`), the
  old-prefix env vars (now `TANGENT_*`), `rustfmt.toml:1`'s old-name header,
  `tests/golden/README.md`. *(2026-10-18: all four are done.)*
- [ ] **W61 · Stray tracked files:** `docs/FREEBSD-5.0-FINDINGS.md-e`,
  `tangent-record/examples/rtlatency.rs-e`, four `Cargo.toml.orig` under vendor/.
- [ ] **W62 · ~25 dead items**, including `fn luma` defined twice and dead in both
  (`mixer_panel.rs:2509`, `arrangement_panel.rs:5119`).
- [ ] **W63 · Agent-to-agent correspondence in permanent history** — about 8 of the last 170
  commits.
- [ ] **W64 · There is no way to learn 5.0 is broken on someone else's machine.** Logging is
  stderr-only at Warn with no file sink; the panic hook shows a dialog and exits writing nothing; no
  telemetry (deliberate), no update check (deliberate), **no contact address anywhere in the app**,
  no issue link in the README, and no purchase link in the Supporter Key dialog.
- [X] **W65 · Settings migrations 0–18 are now unreachable.** DECIDED KEEP 2026-08-31 (migrate_from doc says why). The only place a pre-19 file could live
  was the 4.x directory, which 5.x never reads. Decide whether to keep them.
- [ ] **W66 · `main.rs:524` says plugin editors are not implemented on Windows and Linux.** False on
  all four platforms — and that false premise is what keeps those platforms on the glow renderer.
  Re-evaluate the renderer choice once the comment is corrected.

### Tier 8 · write down what is already decided

- [ ] **W67 · Comping.** The rationale exists, stranded at `RECORDER-PLAN.md:1996` under "Explicitly
  NOT in v1": *"Editing, trimming, or any timeline. The take is what was played."* The first clause
  is now dead. Lift the second to `recorder.rs` beside `CHANNELS` and `INSERTS`.
  **Why it matters:** intent does not fill a cell on a feature grid, but an *unexplained* gap makes a
  reader discount everything they have not personally verified, while an *explained* one discounts
  only itself.
- [ ] **W68 · 16 channels needs one more sentence.** `recorder.rs:562` explains the *fixedness*, not
  the *number* — a reader still asks "why not 24 fixed channels?"
- [ ] **W69 · Surround, MIDI output, scripting and ARA have no written rationale anywhere.**

---

## Part 2 — SETTLED · do not raise again

### S1 · Deliberate by design

Confirmed by the owner or argued in the code. These are **positions, not gaps**. They remain
competitive losses on a feature grid and that is fine; do not file them as work.

| | Where it is argued |
|---|---|
| No comping | recorder.rs, beside CHANNELS (lifted 2026-08-31) |
| 16 channels | `recorder.rs:562` — "a desk that changes shape under your hands" |
| 3 inserts per channel | `recorder.rs:596` — "artistically limiting on purpose" |
| No autosave | `app.rs:9998` — the cost is stated and accepted |
| No telemetry, no network, no update check | Load-bearing positioning |
| An unused channel is drawn as a plus | Same fixed-shape argument |
| `cargo fmt` not run | `rustfmt.toml`, measured at three width settings before deciding |
| ReaStream dialog hard-coded light | Owner asked for it; `dialogs.rs:6356` records it |

### S2 · Investigated and found FALSE

**Do not accept these from a future audit without new evidence.**

- **S10 · "Accessibility is literally zero."** (5.0 audit) AccessKit is in the tree and enabled;
  `a11y.rs` is 481 lines; the keyboard nudge routes through each panel's own hit-test. What remains
  is W45–W48, which are specific bugs, not an absence.
- **S11 · "The window is fixed-size."** (5.0 audit) `settings.rs:4781` `RESIZABLE = true`, and the
  comment there is explicitly the answer to that audit finding.
- **S12 · "The Linux build does not start" / "liblilv is not installed by default on any mainstream
  distro."** (my first draft — **wrong**) It is present by default on Debian (PipeWire →
  libspa-0.2-modules), Fedora Workstation (mandatory `gstreamer1-plugin-libav` → libavfilter), Arch
  (`pipewire-audio`), **Void** (`pipewire` run_depends), openSUSE, Mint, Pop, and every Ubuntu
  *flavour*. Absent **only** on stock Ubuntu GNOME, for a structural reason: liblilv is in universe,
  `ubuntu-desktop` is in main, main may not depend on universe, so Canonical builds PipeWire with
  `-Dlv2=disabled` (Launchpad #2054223). **Your Void box is representative, not an outlier.** What
  survives is W21.
- **S13 · "The Ed25519 seeds are unbacked-up."** (5.0 audit) A backup exists and was verified to be
  the real minting key. What survives is W33 (stale ledger, Dropbox-only, old folder name).
- **S14 · "libGL/libEGL degrade rather than blocking startup."** (my error) Two consumers exist —
  the compositor's loader degrades, glutin's does not. Changes nothing in practice because those
  libraries are in every graphical session.
- **S15 · "Plugin editors are mac-only."** (`main.rs:524`, still in the source) They work on all four
  platforms. The comment is W66.
- **S16 · "liblilv only arrives with another DAW."** No — it arrives with PipeWire or ffmpeg. Ardour
  is on ~1,759 Debian popcon machines; liblilv is on ~151,000.

### S3 · Closed since the 5.0 audit — do not re-audit

Automation (four modes, plugin parameters, master, folder-aware) · plugin delay compensation
(folder-aware, both formats) · tempo track with closed-form ramps, read by ruler/grid/snap/click/
export · folder buses that are real latency-correct buses · routing (hardware outs, channel sends,
dependency-ordered, cycle-safe) · project lifecycle (save prompt, derived dirtiness, discard
actually quits) · resizable window · scoped resets with confirmation *and* undo · undo/redo on every
help card · click-sounds-a-note for mouse-only users · per-view derived help cards · LV2 hosting
(Linux/FreeBSD) · VST3 multi-output-bus folding · plugin-crash quarantine · Linux tarball
completeness · `release.sh` Linux artifact checks.

---

## Part 3 — how this file stays true

1. Tick items here as they land; do not delete them, move them under a `## Done` heading with the
   commit hash, so the next audit can see what was addressed and when.
2. When something turns out to be deliberate, add it to **S1** with the file:line where it is
   argued. If it is not argued anywhere yet, that is a W-item, not an S-item.
3. When a claim turns out to be false, add it to **S2** with the evidence that killed it. That entry
   is what stops it coming back.
4. Anything asserting a runtime failure that nobody observed is a hypothesis, not a finding. S12 is
   in this file precisely because that distinction was not made.
