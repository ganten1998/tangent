# Request to the main session: kick off the build

From the GhostBSD box, 2026-08-30. Companion to `FREEBSD-BRIEF-FOR-MAIN.md` —
read that first if you have not. Branch `freebsd` @ `311ab1f`, pushed, 38
commits ahead of `main`. `main` untouched.

**The ask:** the tester wants a build to put hands on. Below is what you can
produce, what you cannot, and the one decision that has to be yours before
either happens.

---

## 1. The one thing blocking a merge

`tangent-host` now links `liblilv-0.so` on **Linux as well as FreeBSD**. No
build script, no bindgen, no pkg-config — `lilv-sys` ships pre-generated
bindings and a bare `#[link(name = "lilv-0")]`, so the property your crossbuild
depends on (building for another target needs no C toolchain) is intact at
compile time. What is *not* free is the link step: cross-building **Linux** from
the Mac now needs a Linux `liblilv-0.so` plus `libserd`, `libsord`, `libsratom`
and `libzix` reachable by the linker.

Three ways out, in the order I would try them:

1. Put those five in the zig sysroot. Smallest change, one build host, Linux
   users get LV2.
2. Build Linux on Linux, if there is CI.
3. An `lv2` cargo feature, default-on for FreeBSD, off for a cross-built Linux.
   Cheapest, and the one I would take last — it ships a Linux binary that cannot
   load the format most Linux plugins use, and nothing in the UI would explain
   why the picker is short.

**I have not made that call.** It depends on how your sysroot is assembled,
which I cannot see from here. Windows and macOS are unaffected — the dependency
is `cfg`-gated to `linux`/`freebsd`, `cargo tree --target x86_64-pc-windows-msvc`
is unchanged, and `check-firewall.sh` passes.

## 2. What you can build, and what you cannot

| target | from the Mac? | notes |
| --- | --- | --- |
| Windows, macOS | yes, unchanged | firewall verified clean on this branch |
| Linux | yes, **after** the §1 decision | linker needs target-side lilv |
| **FreeBSD** | **no** | see below |

The FreeBSD artifact cannot come off the Mac, and this is not a toolchain gap
that effort closes:

- `scripts/build-freebsd.sh` calls `pkg create` and `pkg fetch`. Both are
  FreeBSD-native and there is no cross mode.
- `pkg` derives the ABI from the **build host** and enforces it at install time
  (`FreeBSD:15:amd64`). One artifact per FreeBSD major, built on that major.
- The Mac M5 is `aarch64`; the tester is `amd64`.
- Nothing off the OS can test the parts that actually broke here — OSS
  `/dev/dsp*`, the alsa-lib shim, `/dev/umidi`, X11 embedding of a plugin's own
  window.

The binary alone *is* cross-buildable with real effort — a FreeBSD sysroot with
base plus ports libraries, `PKG_CONFIG_ALLOW_CROSS`, and a C cross-compiler for
`mp3lame-sys`, which is the one dependency that compiles C. That still does not
get you an artifact, and it does not get you a test. If you want FreeBSD in CI,
the answer is a FreeBSD amd64 runner or VM, not a cross toolchain.

## 3. So, concretely

**If the build you want is FreeBSD:** say so and it gets built here, off
whatever tree you name. The current artifacts in `dist/` are already built and
installed on this box from `311ab1f` — the tester is running one now, so unless
you land changes there is nothing to rebuild:

| file | size |
| --- | --- |
| `tangent-5.0.0.b9-freebsd-FreeBSD-15-amd64.pkg` | 8.1 MB |
| `…-desktop.tar.gz` | 64 MB |
| `…-complete.tar.gz` | 662 MB |

**If the build you want is everything else:** make the §1 call, merge or rebase
`freebsd` as you see fit, and kick the crossbuild off there. If §1 lands as a
sysroot change, nothing on this branch needs to move. If it lands as a cargo
feature, that is a source change and I would rather it came from you than be
guessed at here.

Either way the box is free and the tester is waiting, so name the tree and the
target and it happens.

## 4. What I would not merge blind

- **The vendored cpal.** Still the largest thing on this branch and still wants
  an upstream PR rather than a permanent fork.
- **LV2 `state:` for plugins holding data beyond their ports** — a sampler with
  a loaded file. Control ports are done; that class needs the live instance back
  across the hand-off, and nothing in the FreeBSD ports tree is in that class
  today.
- The rest of "What I did not do, and why" in `FREEBSD-HANDOFF-BACK.md`.

Suite on this branch: **1677 passed / 0 failed.** Firewall intact.
