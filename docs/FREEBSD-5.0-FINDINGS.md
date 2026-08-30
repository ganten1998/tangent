# Tangent 5.0.0-beta.9 — FreeBSD port report (ganten-ghostbsd, 2026-08-29)

> **Status: the port compiles, launches, draws and makes sound.** Branch
> `freebsd`. The three build-breakers named in the 5.0 audit are fixed, and a
> fourth failure — a hard crash on startup that only became reachable *once* it
> compiled — is fixed with it.
>
> | Bar | Result |
> |---|---|
> | `cargo build --release -p tangent --features recorder` | ok, 5m36s |
> | App launches and draws | ok, glow on X11 |
> | Audio out, stock system, nothing configured | ok, 0 underruns in 6721 interrupts |
> | Audio in | **blocked on hardware** — this laptop has no usable input, see §6 |
> | `cargo test --workspace --features recorder` | **1596 passed, 0 failed**, 56 ignored |
> | `scripts/check-firewall.sh` | green, Minimal build included |
> | Latency measured | probe written (`examples/rtlatency.rs`), blocked with audio in |

Test box: HP laptop, GhostBSD 26.1 / FreeBSD 15.0-RELEASE-p2 amd64, i5-1135G7
(8 threads), 8 GB, Realtek ALC236 via `snd_hda`, Iris Xe with DRI + Intel Vulkan
ICD, X11 (no Wayland), MATE. Real hardware, not a VM.

---

## 0. Before anything: the toolchain does not link

`pkg install rust` produces a compiler that cannot link a hello-world:

```
ld: error: unable to find library -lexecinfo
```

GhostBSD ships `libexecinfo` as a **separate base package**, and only the
runtime half is installed by default, so the `.so` symlink rustc's target spec
passes `-lexecinfo` against does not exist. Nothing to do with Tangent — it
kills any Rust build on the box:

```
pkg install GhostBSD-libexecinfo GhostBSD-libexecinfo-dev
```

Worth putting first in any FreeBSD build instructions, because it looks like a
project problem and is not one. Full prerequisite list:

```
pkg install rust git pkgconf alsa-lib alsa-plugins alsa-utils jackit \
            GhostBSD-libexecinfo GhostBSD-libexecinfo-dev
```

---

## 1. The audio route, and why

**Recommendation: cpal's ALSA host over FreeBSD's alsa-lib-to-OSS shim for
out-of-the-box, JACK available beside it for pro audio. No new backend, no
patched cpal.** This is what the branch does.

Two upstream facts decide it, both verified in the vendored crate sources
rather than taken on trust:

* **cpal 0.16 already supports FreeBSD.** `src/platform/mod.rs`,
  `src/host/mod.rs` and its `Cargo.toml` all gate on
  `linux|dragonfly|freebsd|netbsd`. FreeBSD gets the ALSA host with no patch.
  Every `#[cfg(target_os = "linux")]` in this repo that really meant "the cpal
  ALSA backend" was therefore a trap: FreeBSD got ALSA's *semantics* and the
  *else* arm written for macOS.
* **cpal's `default_host()` for that platform is a hardcoded `AlsaHost::new()`
  whatever features are enabled.** So turning on `cpal/jack` cannot steal the
  default. It only adds `"JACK"` to `cpal::available_hosts()` — which is
  precisely what `audio::systems()` enumerates into the Audio Setup panel. The
  pro-audio path is a picker entry that costs **zero UI code**, exactly the way
  ASIO works on Windows.

Measured on the stock system, nothing configured: `aplay`/`arecord` open the
default PCM through `pcm.!default → plug:oss`, 128- and 256-frame periods are
accepted, and Tangent itself ran **6721 interrupts with zero underruns** while
holding a chord. A native OSS backend for cpal is **not** needed to hit the
out-of-the-box bar. It remains the right long-term answer if the shim's
raggedness (§2) ever proves to cost real latency, and it would be a genuine
upstream contribution, but it is not on the critical path.

JACK on FreeBSD has a native OSS driver (`jackd -d oss -C /dev/dsp0 -P
/dev/dsp0`), so the pro-audio story is real and not an emulation. Note it wants
`mac_priority(4)` loaded and the user in the `realtime` group for RT priority.

Confirmed on the box, straight out of `examples/rtlatency`:

```
hosts available : ["JACK", "ALSA"]
host            : ALSA
```

Both hosts enumerate — so the Audio Setup panel offers JACK — and the default
is still ALSA, so a user who has never heard of JACK is unaffected. That is the
whole design in two lines.

One number to distrust: cpal reports **`capture buffer is 0.00 ms behind the
callback`** through this shim. A capture path with genuinely zero latency does
not exist, so the driver's own timestamps are not usable as a latency source
here. This is exactly why §7's measurement is by click and not by asking.

### What the shim actually does — measure this, do not assume it

`aplay -l` reports **no soundcards at all**; `aplay -L` offers only `null` and
`oss`. cpal consequently enumerates `["default", "oss"]` rather than device
names. Everything works, but:

* The device picker will show "default" and "oss", not "Realtek ALC236". A
  cosmetic wart with a real support cost.
* The shim reports **nonsense ranges**: `buffer_size: 1..=4294967295` and
  sample rates to 4294967295 Hz. This makes `pick_buffer`'s
  `.clamp(*min, *max)` a no-op — harmless today, but see §5.

---

## 2. `BUFFER_PERIODS` — the named trap, and the audit's fix was backwards

The 5.0 audit predicted FreeBSD would fall into the `else` arm (`1`) and
recommended widening the Linux arm to `linux|freebsd|dragonfly|netbsd`.
**Measurement says the opposite.** Real callback sizes, 44.1 kHz, ALC236:

| Requested | Callback frames observed | Modal |
|---|---|---|
| `Fixed(256)` | 205 – 256 | **256** |
| `Fixed(512)` | 177 – 512 | 206 |
| `Fixed(1024)` | 264 – 500 | **441** |

Linux's `4` exists because cpal's ALSA host reads `Fixed(v)` as the whole ring
and carves it into four periods, so asking for 1024 yields 256-frame callbacks.
**FreeBSD's shim does not carve a ring into periods at all** — it hands the
callback whatever `snd_pcm_avail` reports, so the period is ragged by
construction and multiplying the request just asks for a bigger ragged one.
Adopting Linux's arm would have roughly doubled the callback size and the
latency with it.

So FreeBSD gets an **explicit arm holding `1`** — the same value the else-arm
would have given by accident. The point of writing it out is that the next
person can see it was chosen. The trade it declines is a deeper safety ring: if
a loaded FreeBSD box underruns at `1`, this is the number to raise, knowing the
cost is a larger ragged callback.

---

## 3. The three build-breakers

**A1 · midir has no FreeBSD backend.** Its table declares modules for
windows/macos/ios/linux/wasm/android and nothing else; on FreeBSD the crate
itself fails to compile and no cfg in our source can help. The one arm not
keyed to an OS is `all(feature = "jack", not(target_os = "windows"))`, so the
FreeBSD build asks for `jack`. **No fork, no vendoring.** MIDI stays in the
build — this app is a MIDI keyboard monitor first, and cfg'ing it out would
ship something that is not the app.

Honest cost: libjack becomes a build- and link-time dependency of the FreeBSD
build, and MIDI ports only appear while a JACK server runs. Audio does **not**
inherit that — cpal still defaults to ALSA-over-OSS and makes sound with
nothing running. See §7 for the native answer.

**A2 · `CONTENTS_SUBDIR`.** Added `x86_64-freebsd` / `aarch64-freebsd` from the
spec's `<cpu>-<os>` shape. There is no Steinberg arm for BSD and no meaningful
population of FreeBSD-built VST3s to sample, so this is a spec-consistent guess
and is labelled as one in the source. `binary_in_bundle` therefore *also* looks
in the Linux-named directory when the correct one is absent: frameworks that
support FreeBSD mostly do it by pretending to be Linux, and a native ELF
misfiled under `x86_64-linux` would otherwise be reported as no plugin at all.
Safe in the direction that matters — a genuine Linux `.so` cannot be `dlopen`ed
here, so the fallback can only rescue a misfiled native plugin.

**A3 · the module entry symbol.** FreeBSD takes the Linux spelling
(`ModuleEntry`) and, deliberately, the Linux *semantics*: optional, a missing
symbol is not an error, unlike macOS's `bundleEntry`.

---

## 4. The crash that only existed once it compiled

With the three fixed, the app died on the way up, before drawing a frame:

```
called `Result::unwrap()` on an `Err` value: BadAccess
  at wgpu-hal-27.0.4/src/gles/egl.rs:305
```

`composite.rs` already explains why a wgpu-managed GL adapter is unusable —
wgpu-hal brackets its GL work with `eglMakeCurrent(ctx)` / `eglMakeCurrent(NONE)`
on the window's own thread, which unbinds rather than restores. What that
comment does not say, because Linux never reaches it, is that **the damage is
done during enumeration**, before any adapter comes back to be refused.
Building the adapter is what constructs the EGL context, and
`EglContext::make_current` unwraps.

On Linux `renders_on_the_cpu()` returns early via `owned_gl_is_hardware()` and
never builds the default instance. FreeBSD has no owned-GL path, so
`request_adapter` reached the GL backend while eframe's glow context was
already current on that thread; EGL permits one current context per thread, and
the call came back `BAD_ACCESS` into that unwrap.

Fix: the compositor's probe instance **excludes the GL backend on FreeBSD**. It
costs nothing that was ever available — a GL adapter reaching the check is
refused two lines later — and it is the difference between the app starting and
not. Vulkan (hardware, or mesa's lavapipe) is what composites here.

Note `IVORY_RENDERER=glow` is **not** an escape hatch on this platform: the
renderer is hardcoded to Glow on everything but macOS, so the variable does
nothing here. The crash was never the main window.

---

## 5. The else-arms that named the wrong platform

Fixed on the branch:

* `native_dialogs_work()` asserted `true` on FreeBSD, reintroducing the
  Void/XFCE silent-dead-button bug verbatim. The probe was already portable —
  session bus, `dbus-send`, `zenity` — and its file fallback already reads
  `/usr/local/share/xdg-desktop-portal/portals`, the FreeBSD prefix, which was
  dead code until the gate included it. On this box: no FileChooser portal
  (only `gnome-keyring.portal`), `zenity` present, so it correctly answers yes.
* `picker_needs_windowed()` — an X11 problem, not a Linux one.
* VST3 search paths contributed nothing on FreeBSD, so the plugin picker was
  empty unless the user set `VST3_PATH` they had no reason to know about. Added
  `~/.vst3` + `/usr/local/lib/vst3`; deliberately **not** `/usr/lib/vst3`,
  which is base-system territory where a VST3 will never be.
* The ffmpeg install hint told FreeBSD users to run `apt`.
* The compositor's "install lavapipe" hint was Linux-gated, so FreeBSD got the
  error without the advice.

Found and **deliberately left alone**, with reasons:

* **`audio.rs:1027` picks `range.max_sample_rate()`** when no preferred rate
  matches. On FreeBSD that is 4294967295 Hz. It does not fire today because the
  device default (44100) always falls inside the shim's range, so `rate_hits`
  is true — but any device whose `default_input_config()` fails would take it.
  Latent, severe, and not fixable blind: changing device-selection logic
  affects every platform and I could not construct the failing case here.
* **Plugin editor (`EDITORS_SUPPORTED`) stays false.** The audit is right that
  widening it looks easy — `x11rb` is pure Rust and `libc::poll` is portable,
  so it is a gate change in `editor.rs` (:88, :90, :98, :325, :327, :1161,
  :1649, :1709) plus `tangent-host/Cargo.toml:82`. But there is no VST3 plugin
  on this box to test with, and flipping the flag replaces an honest
  "Unsupported" with an untested path whose failure mode is a crash while
  hosting somebody's plugin. Worth doing with a plugin in hand; not worth doing
  blind.
* **Camera stays stubbed.** `linuxvideo` encodes **Linux** `_IOC` numbers, and
  FreeBSD's ioctl bit layout differs (3-bit dir / 13-bit size vs 2/14). The
  vendored crate needs its constants re-derived against `multimedia/v4l_compat`
  before webcamd's V4L2 could work. Correctly reports "unsupported" today.

---

## 6. Audio in is blocked by the hardware, not by the port

`arecord` opens and returns samples, so the capture path works. It returns
near-silence (peak 79/32768 at 0.9 gain) because **this laptop has no
microphone the OS can reach.** The ALC236's only Mic pin is:

```
nid25_config: 0x04a11020  device=Mic  conn=Jack  loc=Right
```

— the external 3.5 mm jack. There is no `loc=Internal` mic pin anywhere on the
codec. On Tiger Lake the internal array mic is a DMIC behind Intel's SOF/AVS
audio DSP, which FreeBSD has no driver for. Selecting `speaker` as the
recording source (the only other OSS rec device) records noise, so there is no
loopback either.

Not a Tangent bug and not fixable in software. Audio-in and the round-trip
measurement need a USB interface or a mic in the black jack. Everything else in
the input path — cpal enumeration, `default_input_config()`, the record channel
Tangent opens (`1r:1v` in `sndstat`) — is already proven.

---

## 7. What is worth doing next, in order

1. **Round-trip latency**, once there is an input. `examples/rtlatency.rs` is
   on the branch: it clicks, listens, and reports min/median/mean against
   cpal's own reported figures, so a gap between measured and claimed is itself
   a finding. This is the number that decides whether the shim is good enough
   or an OSS backend is required.
2. **A `/dev/umidi*` MIDI backend.** FreeBSD's native MIDI is raw char devices;
   no Rust crate wraps them, which is the *real* reason midir has no arm. A
   small backend reading `/dev/umidi*` would give out-of-the-box MIDI without a
   JACK server, and it is the one piece that would make this a first-class port
   rather than a working one. Would mean owning a midir fork or an internal
   backend — worth a decision, not a drive-by.
3. **Device names.** `["default", "oss"]` in the picker is the most visible
   remaining wart.
4. Plugin editor and camera, per §5, when there is hardware to test them with.

---

## 8. Reproducing

```sh
pkg install rust git pkgconf alsa-lib alsa-plugins alsa-utils jackit \
            GhostBSD-libexecinfo GhostBSD-libexecinfo-dev
git clone ssh://git@codeberg.org/ganten1998/tangent.git && cd tangent
git checkout freebsd
cargo build --release -p tangent --features recorder
./target/release/tangent

# make it sound without MIDI hardware:
IVORY_DEMO_NOTES=60,64,67,71 ./target/release/tangent
```
