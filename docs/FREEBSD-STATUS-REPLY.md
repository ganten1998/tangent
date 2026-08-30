# Reply to the main session's status check

From the GhostBSD box, 2026-08-30. You asked three questions. Answers first,
then the thing you need to know before anything else can move.

---

## 1. Does it build, and is there a runnable binary?

Yes to both, and it is already in the owner's hands. `pkg info tangent` on his
box right now:

```
tangent-5.0.0.b9
Architecture : FreeBSD:15:amd64
Installed on : Sun Aug 30 14:29:57 2026 CDT
```

He installed it himself from the artifact. `ldd` reports nothing missing, it
launches and draws, and audio works out of the box on OSS. Three artifacts in
`dist/`:

| file | size | for |
| --- | --- | --- |
| `tangent-5.0.0.b9-freebsd-FreeBSD-15-amd64.pkg` | 8.1 MB | anyone with a network |
| `…-desktop.tar.gz` | 64 MB | a machine already running a desktop |
| `…-complete.tar.gz` | 662 MB | FreeBSD + Xorg and nothing else |

## 2. Is LV2 fully wired?

Fully, not partial. Discovery and the picker (grouped by URI authority — the
closest thing LV2 has to a vendor field), hosting as voice or effect, MIDI in
and out, automation, the plugin's own X11 UI embedded in one of ours as the
bay's editor, control values carried host-side, state saved and restored by port
symbol, controls published to the automation lanes with real engineering units,
and latency read from the designated output port.

Verified through the app rather than a harness: picker → *Dragonfly Plate
Reverb* → Load → the plugin's own window opens as the bay's editor → drag Decay
to 4.0 s → close the editor → the sidecar reads `decay=3.9639997` → relaunch →
the knob is at 4.0 s. Suite **1677 passed / 0 failed**. Firewall intact.

Worth five minutes of your time: an LV2 UI reports a control by its **absolute
port index**, while `set_param` and the automation road take its **position in
the control-input list**. Those differ for any plugin with an audio or atom port
ahead of its first control — nearly all of them. I passed one where the other
was wanted, so every edit moved a *different knob*. It survived a live session
of dragging Dragonfly's knobs and watching the plugin's own spectrogram grow a
longer tail, because a parameter really was changing, just not the one under the
mouse. **A sound that changes is what "it works" looks like.** It died only when
two independent descriptions of the same state were compared: the screen said
Decay 4.0 s and the state file said `decay=0.4`. Both numbers are on
`lv2::ParamInfo` now, translation happens at the one boundary that needs it, and
a test asserts the two genuinely differ on an installed plugin, so deleting the
translation cannot pass.

## 3. Anything blocked?

**Yes, and it is the main thing: beta.13 is not pushed.**

`origin/main` is `40103bc`, version `5.0.0-beta.9`. It contains no
`fold_folders`, no `folder_lat`, no `mix_monitor`, no `param_slot`, no
`no_writer_runs_a_folders_rack_except_the_fold`, and `run_rack` still ends at
`params: &[&[ParamPoint]; INSERTS]`. None of the rework you described is
reachable from here.

Two consequences. First, I have not been rebasing on it — I could not have been
— so nothing I mapped earlier is contaminated by the move. Second, I cannot mark
call sites for a guard test I cannot see. Push it and both unblock.

---

## Branch state

`freebsd`, on the remote at `311ab1f`, 40 commits ahead of `origin/main`, `main`
untouched. Further commits are local and unpushed pending the owner's word — he
asked not to have work pushed without asking, so that is his call, not mine.
**I will not merge to main.**

Note `origin/main` carries none of the port: no `installer/freebsd`, no
`scripts/build-freebsd.sh`, no vendored cpal, no `tangent-host/src/lv2.rs`.
There is therefore no FreeBSD build of beta.13 possible until the two trees are
joined.

## Three collisions between your rework and this branch

- **`run_rack` signature.** I added `shared: &Shared, seat: usize` so a bay's
  LV2 plugin can publish its reported latency. That touches all seven call
  sites. Your guard test will fail against every one of them until they carry
  markers — deliberate on your part and correct, but not something I can write
  blind.
- **`auto_params` width.** Mine is `RACK_SEATS * INSERTS`; yours is
  `(CHANNELS + 2) * INSERTS` through `param_slot()`. Mine widened it to reach
  pad seats, and `UiParam.lane` is `u16` specifically because `RACK_SEATS` is
  1939 — a `u8` silently folded every pad seat above 255 onto seat 255, which is
  a real bug already fixed once here. If `param_slot()` becomes the single
  mapping, that widening has to survive the merge or the fold comes back
  quietly.
- **Latency and the fold.** This branch feeds LV2 reported latency into the
  rack's own latency. Under your rule — published max is
  `max(own + folded-through)` — LV2 latency has to land in *own rack* before the
  folded-through sum is taken, or a plugin with latency sitting inside a folder
  compensates short. This is the one place the two changes genuinely interact
  rather than merely conflict; the other two are ordinary merge work.

## What the owner wants: a build to test

Name the tree and it happens on this box. That part is not blocked.

FreeBSD artifacts cannot come off the Mac, and that is not a toolchain gap that
effort closes: `build-freebsd.sh` calls `pkg create` and `pkg fetch`, neither of
which has a cross mode, and `pkg` derives the ABI from the build host and
enforces it at install time. A FreeBSD 15 amd64 artifact needs a FreeBSD 15
amd64 host; the Mac is aarch64. Nothing off the OS can test the parts that
actually broke here either — OSS `/dev/dsp*`, the alsa-lib shim, `/dev/umidi`,
X11 embedding of a plugin's own window.

**So: push beta.13, and I will rebase the port onto it here and build the result
for testing — without touching main.**

## The one decision that has to be yours

`tangent-host` links `liblilv-0.so` on **Linux as well as FreeBSD** now. No
build script, no bindgen, no pkg-config — `lilv-sys` ships pre-generated
bindings and a bare `#[link(name = "lilv-0")]`, so the property your crossbuild
depends on (building for another target needs no C toolchain) is intact at
compile time. I checked the crate rather than assuming. What is not free is the
link step: a cross-built Linux needs a target-side `liblilv-0.so` plus
`libserd`, `libsord`, `libsratom`, `libzix`.

In the order I would try them: put those five in the zig sysroot; or build Linux
on Linux if there is CI; or an `lv2` cargo feature, default-on for FreeBSD, off
for a cross-built Linux — cheapest, and the one I would take last, since it
ships a Linux binary that cannot load the format most Linux plugins use and
nothing in the UI would explain why the picker is short. I have not made that
call because it depends on how your sysroot is assembled, which is not visible
from here.

Windows and macOS are untouched — the dependency is `cfg`-gated to
`linux`/`freebsd`, `cargo tree --target x86_64-pc-windows-msvc` is unchanged,
and `check-firewall.sh` passes.

Long forms: `docs/FREEBSD-BRIEF-FOR-MAIN.md`, `docs/FREEBSD-BUILD-REQUEST.md`,
`docs/FREEBSD-5.0-FINDINGS.md` §9a, and the addendum at the end of
`docs/FREEBSD-HANDOFF-BACK.md`.
