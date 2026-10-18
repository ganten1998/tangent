# Review notes for `freebsd-beta13`

From the GhostBSD box. Answers to the three things you asked for before
reading, plus one thing you did not ask about and need to know.

Branch: `freebsd-beta13`, a merge of `freebsd` into `main` @ `edde2e2`.
`main` untouched. 36 commits ahead of `origin/main`.

---

## 0. Read this first: there is an 87 MB core dump in the history

`tangent.core` — a FreeBSD ELF core file — was swept into commit `e52826f` by a
`git add -A` of mine, weeks of context ago. It is address space from a crash
while I was working on the LV2 load path. It has never been anything anybody
would read.

It is **removed from the tree** in `058a0ad` and `*.core` is now ignored. But it
is still in this branch's HISTORY, and it is **already on `origin/freebsd`**,
which was pushed before I noticed. Getting it out of either needs a rewrite and
a force-push, which is not mine to do. Pack cost is smaller than the size
suggests — a core is mostly zeroes, and the whole pack is ~19 MiB — so this is
tidiness rather than an emergency. Flagging it because you are about to fetch
it and would reasonably wonder.

## 1. What is in the 36 commits

**FreeBSD port proper — build plumbing, cfg gates, audio and MIDI backends.**
`b5907ca` `9533d3b` `a5df7b3` `e90ab08` `c7a40f1` `515e09f` `602be16` `77d5391`
`36f919b` `2e58b37` `465ad3d` `174c87a` `07b78ae`. The port compiling and
running, a native **OSS host for cpal** (vendored — 27.5 ms out of the box
became 8.1 with no sound server), a native **`/dev/umidi` MIDI backend** so the
box does not need JACK to have a keyboard, and the **pkg installer**. The
vendored cpal is the largest single thing on the branch and is still the thing I
would least like to see merged permanently — it wants an upstream PR.

**LV2 — hosting, picker, editor windows, host glue.** `b92e7d0` `bd9cf7d`
`e52826f` `f8f98e1` `5c99e8c` `ef7ab2a` `9a9dd3e` `63ad59d`. Hosting, then the
picker, then the plugin's own X11 window embedded as the bay's editor, then
state, the parameter list and latency.

**Docs.** `9533d3b` `602be16` `082ca35` `015c6f0` `ede23e5` `f53a8a1` `7c173c4`
`846ae1f` `cabd18f` `39d15b2` `311ab1f` `bc5fb25` `28c0f3d` `d3766ee`. Findings,
handoffs, and the notes to you. No code.

**Shared code that macOS and Windows also compile — the group you asked about.**
Smaller than the diffstat implies, and here it is in full:

| where | what | gated? |
| --- | --- | --- |
| `instrument.rs` | `run_rack` grew `shared: &Shared, seat: usize` | **no** |
| `instrument.rs` | `Backend::reported_latency()` | signature no, LV2 arm yes |
| `instrument.rs` | `auto_params` is a row per rack SEAT | **no** |
| `instrument.rs` | `param_slot` answers in seats | **no** |
| `instrument.rs` | `UiParam` + the `ui_params` ring | **no** |
| `instrument.rs` | `seat_params` at each `run_rack` call site | **no** |
| `app.rs`, `dialogs.rs` | plugin-list tuple widened 2 → 3 | **no** |
| everything else | LV2 backend arms, editor door, discovery | yes |

What that costs the other two platforms, concretely:

- **`run_rack`'s two new arguments are dead weight there and nothing else.**
  `reported_latency()` returns `None` for `Backend::Vst3`, so the
  `Rack::set_latency` call inside never fires. VST3 latency still arrives the
  way it always did, on the main thread through a `LatencyHandle`. LV2 has no
  such route — the number is on a control OUTPUT port the plugin writes during
  `run()`, so it can only be read on the audio thread after a block.
- **`auto_params` and `param_slot` are a real semantic change on every
  platform.** See §3 — this is the part I would read closely.
- **`UiParam`** is a second writer into `auto_params`, from plugin editors. It
  exists on all three; only LV2 currently pushes to it.
- **The tuple widening** adds an optional display name. `None` everywhere except
  LV2 and behaviour is identical — a VST3's bundle stem IS its name, an LV2 URI's
  last segment is a slug ("dragonfly-reverb" for "Dragonfly Hall Reverb").

## 2. The suite, on the merged tree, on FreeBSD

Both run here, on `freebsd-beta13`, after the merge:

```
cargo test --workspace --all-features   1707 passed / 0 failed   exit 0
./scripts/check-firewall.sh             firewall intact          exit 0
```

`cargo tree -p tangent --target x86_64-pc-windows-msvc --edges normal` contains
no lilv and no lv2.

### The guard test — yes, it failed, and here is exactly what I did

It failed **once**, at one site, and I **invented no new marker and moved no
rule**. Two call sites needed attention:

**The site that failed: the send-destination rack** (`mix_sends`, the
`if !self.racks[dest].is_empty()` call). Its marker is the **`SEND-BARGAIN:`**
one that main already wrote, and it is still that. What broke was distance, not
meaning: this branch adds a `seat_params` binding above each `run_rack`, and
that binding pushed your marker from inside the 24-line window to outside it.
The fix was to **hoist the `seat_params` binding above the marker comment**, so
the marker sits immediately above the call it speaks for. The comment is
unchanged, the semantics are unchanged, and the test reads the source, so it
now sees it.

**The site that did not fail but is the one worth checking: `fold_folders`.**
Your fold is new to this branch and had `NO_AUTOMATION`. I gave it the seat's
real queue, keyed on `ch`. It sits under your **`THE-FOLD:`** marker, which I
did not touch. The reasoning: a folder is one of the sixteen — it has a lane and
a strip — and the only unusual thing about it is *where* its rack runs. Handing
it the empty queue would have meant **automation silently stopped working the
moment a track became a group**, which is not a symptom anybody would connect to
grouping.

If you disagree with that second one, it is one line and it is the line to
argue about.

## 3. The change I would review hardest

`auto_params` is a row per **rack seat** on this branch, because a plugin's own
window is a writer and it can sit in any seat the desk has — a pad, the click,
the drum bus — whereas a curve only ever addresses a lane. Your `param_slot`
files the master **one and two rows past the channels**. That is safe in a table
that stops at the channels. It is not safe in this one: **in the seat space
those two rows are the DRUM BUS and the CLICK.** Every master curve would have
been read back by the drum bus's rack.

`param_slot` therefore answers in seats now, and your second copy of the rule —
the inline `slot_lane` bend in the bus writer — is deleted, so the master's
shape is written down once.

**One trap, flagged because I fell into it first.** I initially delegated this
to `recorder::seat_bay`, which also maps the master's six bays. It compiles and
survives a round-trip check, and it is **wrong**: `seat_bay` is the panel's
DISPLAY order (the extra rack is drawn above the original, and a rack runs top
to bottom), while `master_bay` and `param_slot` are the PERSISTENCE order (keys
0..2 already meant the original rack in every document ever saved). Using
`seat_bay` quietly moves every master curve in every existing session onto the
other rack. Both functions are correct; they answer different questions. It is
now written down on `param_slot`, because the next person will reach for it too.

The mapping test names seats, and additionally asserts the master's rows are
neither the drum bus's nor the click's, and that its two racks are two distinct
seats.

## 4. Is LV2 fully wired, or partial? Plainly.

**Fully wired. Nothing on this list is stubbed.**

Working: discovery; the picker, grouped by URI authority; hosting as voice or
effect; MIDI in and out; automation; **the plugin's own X11 UI embedded as the
bay's editor**; control values carried host-side; state saved and restored by
port symbol; controls published to the automation lanes with real engineering
units; latency read from the designated output port.

Verified through the app, not a harness: picker → *Dragonfly Plate Reverb* →
Load → the plugin's own window opens as the bay's editor → drag Decay to 4.0 s →
close the editor → the sidecar reads `decay=3.9639997` → relaunch → the knob is
at 4.0 s.

**The honest limits**, none of which is a stub:

- **A plugin with no embeddable X11 UI gets no generated panel.** Its controls
  reach the automation lanes by name and in the plugin's own units, and that is
  all. There is no such panel for VST3 either. An earlier version of the install
  message claimed there was; `cabd18f` is me taking that claim back out.
- **LV2 `state:` for plugins holding data beyond their ports** — a sampler with
  a loaded file — is not done. Control ports round-trip; that class needs the
  live instance back across the hand-off. Nothing in the FreeBSD ports tree is
  in that class today.
- **Automation is applied as the last value per parameter per block**, not
  sample-accurately inside it. LV2 control ports are a value written before
  `run()`, not a timed point within it.

## 5. The artifact

`dist/tangent-5.0.0.b13-freebsd-FreeBSD-15-amd64.pkg`, 9.0 MB, built from this
branch. Self-checks pass: `lilv`, `alsa-lib` and `alsa-plugins` declared;
`liblilv-0.so.0` and `libasound.so.2` detected in the binary; no GhostBSD-only
dependencies. The offline bundles in `dist/` are still b9 — you said you did not
need them rebuilt.

The two joins between your rework and this branch, which are the two things
worth putting hands on: **a plugin in a folder's rack** (does it stay clean, or
turn to static), and **automating a master bay** (does it move the plugin you
meant). Both are places where a mistake sounds like something other than what it
is.
