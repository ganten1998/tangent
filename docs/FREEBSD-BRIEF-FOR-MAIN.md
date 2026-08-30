# Brief: the FreeBSD branch is ready, and one thing lands on your crossbuild

**Branch `freebsd`, pushed, 38 commits ahead of `main`. `main` untouched.**
<https://codeberg.org/ganten1998/tangent/compare/main...freebsd>

You asked for LV2 wired in fully — the picker and the plugin UIs, not just the
hosting — and then an artifact the tester can run. Both are done. This is the
short version; the long ones are `docs/FREEBSD-5.0-FINDINGS.md` §9a and the
addendum at the end of `docs/FREEBSD-HANDOFF-BACK.md`.

---

## 1. Read this part even if you read nothing else

`tangent-host` now links `liblilv-0.so` **on Linux as well as FreeBSD**. Before
you kick off the crossbuild, the exact shape of that:

- **No build script, no bindgen, no libclang, no pkg-config.** `lilv-sys` ships
  pre-generated bindings and a bare `#[link(name = "lilv-0")]`; the vendored
  `lv2-sys` is the same. The property `cargo-zigbuild` and `cargo xwin` exist to
  protect — that building for another target needs no C toolchain — is **intact
  at compile time**. I checked the crate rather than assuming.
- **The linker still wants the library for the TARGET.** Cross-building Linux
  from the Mac needs a Linux `liblilv-0.so` (plus `libserd`, `libsord`,
  `libsratom`, `libzix`) reachable by the linker. That is the whole cost.
- **Windows and macOS are untouched.** The dependency is gated to
  `cfg(any(target_os = "linux", target_os = "freebsd"))`.
  `cargo tree --target x86_64-pc-windows-msvc` is as clean as it was and
  `check-firewall.sh` passes unchanged.

Three ways out, in the order I would try them:

1. **Put the Linux `liblilv-0.so` and its four friends in the zig sysroot.**
   Smallest change, one build host, Linux users get LV2.
2. **Build Linux on Linux.** If there is CI, this stops being a question.
3. **A `lv2` cargo feature, default-on for FreeBSD, off for a cross-built
   Linux.** Cheapest to implement and the one I would take last: it ships a
   Linux binary that cannot load the format most Linux plugins use, and nothing
   in the UI would explain why the picker is short.

**I have not made that call.** It depends on how your zig sysroot is assembled,
which I cannot see from here.

---

## 2. Why LV2 exists on this branch at all

The FreeBSD ports tree contains **no VST3 plugins**. Not few — none. A host that
speaks only VST3 arrives on this platform able to load nothing a user can
install. On Linux it is the same argument one notch weaker: lsp-plugins, Calf,
x42, Dragonfly and Rubber Band are all LV2 first.

So this is not a portability shim. It is the difference between the plugin
picker having entries and being permanently empty.

## 3. What works

Discovery and the picker (grouped by URI authority, the closest thing LV2 has
to a vendor field), hosting as voice or effect, MIDI in and out, automation,
the plugin's own X11 UI embedded in one of ours, control values carried
host-side, state saved and restored by port symbol, controls published to the
automation lanes with real engineering units, and latency read from the
designated output port.

Verified through the app, not a harness: picker → *Dragonfly Plate Reverb* →
Load → the plugin's own window opens as the bay's editor → drag Decay to 4.0 s
→ close the editor → the sidecar reads `decay=3.9639997` → relaunch → the knob
is at 4.0 s.

Suite **1677 passed / 0 failed**. Firewall intact.

## 4. The bug worth five minutes of your time

An LV2 UI reports a control by its **absolute port index**; `set_param` and the
automation road take its **position in the control-input list**. Those differ
for any plugin with an audio or atom port ahead of its first control, which is
nearly all of them. I passed one where the other was wanted, so every edit
moved a *different knob*.

It survived a live session of dragging Dragonfly's knobs and watching the
plugin's own spectrogram grow a longer tail — because a parameter really was
changing, just not the one under the mouse. **A sound that changes is what "it
works" looks like.** It died the moment two independent descriptions of the
same state were compared: the screen said Decay 4.0 s and the state file said
`decay=0.4`.

Both numbers are now on `lv2::ParamInfo` and the translation happens at the one
boundary that needs it. There is a test that asserts the two genuinely differ
on an installed plugin, so a deletion of the translation cannot pass.

## 5. Things I corrected rather than shipped

- **The package did not declare `lilv`.** That artifact would have failed to
  start on a clean box — the binary links `liblilv-0.so` outright, exactly as it
  links `libasound`. Declared now, and `scripts/build-freebsd.sh` asserts both.
- **A false promise in the install message.** It told the tester that a plugin
  without an embeddable window "gets a generated panel with their real controls
  and units". There is no such panel, for either format. I went looking for it
  to photograph it. The message now says what is true: those controls reach the
  automation lanes by name and in the plugin's own units.
- **`has_ui` built an entire lilv world per call**, and the menu asks it once
  per bay per frame — every manifest on the system, reparsed hundreds of times a
  second. One cached scan now.
- **A window destroyed from outside never counted as closed**, so the editor
  stayed "open" for the session and the save that closing triggers never ran.

## 6. The artifact

`dist/`, FreeBSD 15 / amd64:

| file | size | for |
| --- | --- | --- |
| `tangent-5.0.0.b9-freebsd-FreeBSD-15-amd64.pkg` | 8.1 MB | anyone with a network |
| `…-desktop.tar.gz` | 64 MB | a machine already running a desktop |
| `…-complete.tar.gz` | 662 MB | FreeBSD + Xorg and nothing else |

15-only is deliberate and not a limitation to design around: `pkg` derives the
ABI from the build host and enforces it at install time, so one artifact per
FreeBSD major is the shape of the problem. A 14 artifact needs a 14 build host;
nothing else about the port changes.

The desktop bundle went 8.5 MB → 64 MB paying for LV2. lilv depends on
libsndfile, which brings FLAC/Vorbis/Opus/mpg123, and on the `lv2` port, which
carries the spec bundles plugins genuinely need plus a Python toolchain they do
not. Both are pkg's graph rather than a choice available here.

Installed from the artifact and run: `ldd` reports nothing missing, it launches
and draws, and the LV2 path works from the installed binary.

## 7. What I would still not merge blind

- **The vendored cpal.** Still the largest thing on this branch and still
  wanting an upstream PR rather than a permanent fork.
- **LV2 `state:` for plugins holding data beyond their ports** — a sampler with
  a loaded file. Control ports are done; that class needs the live instance back
  across the hand-off. Nothing in the ports tree is in that class today.
- The items in "What I did not do, and why" in `FREEBSD-HANDOFF-BACK.md`.
