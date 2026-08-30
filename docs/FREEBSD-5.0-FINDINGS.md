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
> | Audio in | ok on a USB interface (Scarlett Solo); the laptop's own mic is impossible, §6 |
> | MIDI in, no server, nothing configured | ok — native `/dev/umidi`, verified against a Kawai MP6, §8 |
> | `cargo test --workspace --features recorder` | **1619 passed, 0 failed**, 56 ignored |
> | `scripts/check-firewall.sh` | green, Minimal build included |
> | Installer | ok — a real `.pkg` plus two offline bundles, §9 |
> | Latency measured | **36.7 ms** round trip on the shim, **28.4 ms** through JACK, §7 |

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

### JACK, verified end to end

`jackd --no-realtime -d oss -r 48000 -p 1024 -w 16 -C /dev/dsp0 -P /dev/dsp0`
starts on this box and reports `DSP_CAP_REALTIME`, `DSP_CAP_TRIGGER`,
`DSP_CAP_MMAP` against "FreeBSD native OSS ABI" — the pro-audio path is a real
driver, not an emulation. cpal then opens both directions through it:

| | `buffer_size` reported | capture latency claimed |
|---|---|---|
| ALSA shim | `1 ..= 4294967295` | `0.00 ms` |
| JACK | `1024 ..= 1024` | `0.10 ms` |

**JACK tells the truth where the shim does not**, which matters beyond taste:
the Audio Status panel reads these numbers. It also independently confirms
`BUFFER_PERIODS = 1`. Under JACK the clamp lands on the real 1024-frame period
and the app reports 1024. With Linux's `4` the same code would have reported
`1024 / 4 = 256` for a callback that is genuinely 1024 frames — the panel would
have been lying by a factor of four on the one backend that had given it
honest figures.

### What the shim actually does — measure this, do not assume it

`aplay -l` reports **no soundcards at all**; `aplay -L` offers only `null` and
`oss`. cpal consequently enumerates `["default", "oss"]` rather than device
names. Everything works, but:

* The shim reports **nonsense ranges**: `buffer_size: 1..=4294967295` and
  sample rates to 4294967295 Hz. This makes `pick_buffer`'s
  `.clamp(*min, *max)` a no-op — harmless today, but see §5.
* **There can be no device picker on this path, and the reason is in cpal.**
  Its ALSA enumeration (`host/alsa/enumerate.rs`) tries a hardcoded list —
  `["default", "pipewire", "pulse", "jack", "oss"]` — and then iterates ALSA
  *cards* to build `plughw:N` names. It never calls `snd_device_name_hint`.
  FreeBSD's shim exposes **no cards at all**, so only the built-ins can ever
  appear.

  This was tested rather than assumed: named PCMs added to `~/.asoundrc` with
  proper `hint { show on; description "…" }` blocks show up correctly in
  `aplay -L` — "Focusrite Scarlett Solo" and all — and remain invisible to
  cpal, because cpal is not looking there. So the choice of device on FreeBSD
  is a system setting, `sysctl hw.snd.default_unit=N`, and "default" silently
  means whichever device that names. Plugging the Scarlett in moved it.

  Two honest ways out: run JACK, which enumerates its own ports properly; or
  teach cpal's ALSA backend to enumerate hints as well as cards, which would be
  a small, self-contained upstream contribution benefiting custom PCMs on Linux
  too.

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

## 6. Audio in works; the laptop's own microphone cannot

**Resolved with a USB interface.** A Focusrite Scarlett Solo (3rd Gen) is
claimed by FreeBSD's `uaudio` with no driver work at all — it appears as `pcm3`
and, because `hw.snd.default_auto` is 2, becomes the default device on plug-in.
Capture through it is verified with real signal on a wired loopback (§7), and
playback through it was confirmed by ear.

What follows is why the machine's OWN microphone is not an option, kept because
it cost an hour and would cost the next person the same.



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

## 7. Latency, measured on a wired loopback

Measured with `examples/rtlatency` against a **Focusrite Scarlett Solo (3rd
Gen)** — FreeBSD's `uaudio` claims it natively, no driver work needed — with a
1/4" cable from a line out into the instrument input, so there is no acoustic
path and no microphone noise floor in the figure. Round trip is app-writes-a-
sample to app-sees-it-again: output buffering, DAC, cable, ADC, input
buffering.

| Path | Round trip (median) | Spread |
|---|---|---|
| ALSA shim, `BufferSize::Default` | 199.7 ms | 0.3 ms |
| ALSA shim, `Fixed(1024)` | 37.11 ms | 0.3 ms |
| ALSA shim, `Fixed(512)` | 37.14 ms | 0.8 ms |
| ALSA shim, `Fixed(256)` | 37.18 ms | 0.1 ms |
| ALSA shim, `Fixed(128)` | 37.19 ms | 0.7 ms |
| **JACK (direct OSS), 256 frames** | **28.40 ms** | 0.6 ms |
| JACK, 128 frames | driver will not start | — |

### The three things this says

**1. Below 1024 frames, the requested buffer size does not change the
latency.** Asking for 128 and asking for 1024 both land on ~37 ms. This is the
same fact the callback-size measurement showed from the other side (§2): the
shim does not carve a ring into periods and does not honour a period request —
it hands over whatever is available. So the Audio Setup buffer control is, on
this path, a control over how much work happens per callback and **not** over
latency. That is worth knowing before anybody tunes it expecting otherwise.

Changing sample rate to the device's native 48 kHz (removing the `plug`
resampler) moved it by 0.4 ms. Setting `hw.snd.latency=0` moved it by 0.03 ms.
Neither knob is where the time goes.

**2. The shim costs about 8 ms.** jackd's OSS driver opens `/dev/dsp3`
directly, so the JACK figure is the same hardware and the same buffer size with
the alsa-lib layer removed: 28.4 ms against 36.7 ms. That is the price of the
translation the handoff was right to be suspicious of — real, consistently
reproducible, and smaller than the 199 ms a naive default costs.

**3. Jitter is not the problem.** Every configuration held a spread under a
millisecond across ten to fifteen clicks. The shim adds latency; it does not
add instability.

### What it means for this app

Tangent's headline path is MIDI in, synth out — and that is **output only**.
The 37 ms round trip contains both directions plus two conversions, so the
figure that governs how a played note feels is roughly half of it. The full
round trip governs monitoring a live input, which is the recorder's use.

For context, the same class of interface reaches roughly 15-20 ms round trip
under Linux/ALSA and 10-15 ms under CoreAudio, so FreeBSD is about twice the
latency of the platforms Tangent already ships on. Usable, honestly worse, and
now a number rather than a worry.

**This is the evidence for a native OSS cpal backend, if one is ever wanted.**
It would remove the 8 ms the shim costs and — more interestingly — restore
meaning to the buffer-size control, since OSS itself negotiates fragments
properly. It remains a genuine upstream contribution and is still not on the
critical path: JACK already reaches 28 ms today for anyone who needs it, and
the out-of-the-box path works with nothing configured.

### One caveat on the numbers

256 frames is the floor for jackd's OSS driver on this device — `-p 128` fails
with "Cannot initialize driver" — and `--excl` (exclusive, direct access,
bypassing FreeBSD's vchan mixing) changed nothing measurable. So 28 ms is the
best this box does, not a number to be tuned down further.

---

## 8. MIDI: FreeBSD reads its own, and stops needing JACK

The port first shipped MIDI through `midir`'s `jack` feature, because that is
the only arm in midir's backend table not keyed to an OS. It compiled, and it
was the wrong shape: libjack became a link-time dependency of the whole
application, and **no keyboard worked until the user installed JACK and started
a server**. For a DAW that is a fair ask; for a MIDI keyboard monitor it is the
product not working when you plug a keyboard in.

`tangent/src/umidi.rs` reads `/dev/umidi*` directly. The whole platform
difference at the call site is one `use` alias — `midi.rs` is otherwise
untouched — and midir moves to `cfg(not(target_os = "freebsd"))`.

**The central question was settled by measurement before a line was written**,
because it had two plausible answers and guessing wrong yields a parser that
silently drops every message on real hardware. Captured from a Kawai MP6 on
`/dev/umidi0.0`:

```
b0 40 11  b0 40 7f  90 47 33  80 47 40  90 4a 3d  ...
```

That is a **raw MIDI byte stream**, not the USB-MIDI 4-byte event packets the
USB layer carries (which would read `09 90 47 33`). FreeBSD's driver unpacks
the USB framing, so what is needed is an ordinary stream parser. The capture is
`tests/fixtures/kawai-mp6-umidi.raw` — 2628 bytes, 876 messages — and it is a
test, not a souvenir: every byte must be consumed, every message well formed,
and the result identical whether the parser is fed 1 byte at a time or 1023,
because `read(2)` **will** split messages across buffers.

### Two bugs the JACK path was carrying, found on the way out

Worth recording because both were silent, and neither would have been found by
using the app:

* **midir's JACK backend invokes the callback on JACK's realtime process
  thread.** Tangent's MIDI callback allocates (`bytes.to_vec()`), takes a
  `Mutex` and sends on an `mpsc` — three realtime violations per message, on
  the thread least able to afford them.
* **midir's JACK backend stores `ignore_flags` and never applies them.** So
  `input.ignore(Ignore::TimeAndActiveSense)` was a no-op, and every keyboard
  sending Active Sensing at ~3 Hz would have put roughly four thousand junk
  messages into a twenty-minute take — the exact bug that call exists to
  prevent, reintroduced silently by the platform.

### Naming, which the device would not do for us

The MP6 reports `iManufacturer = <no string>` and `iProduct = "USB-MIDI"`. A
port list saying "USB-MIDI" is barely better than "umidi0.0" and is wrong
outright with two keyboards attached. The good name `usbconfig` prints comes
from the USB ID database the system already ships, so the backend does the
same: vendor and product from the driver's `%pnpinfo` sysctl — no USB ioctls,
no libusb — resolved against `/usr/share/misc/usb_vendors`. The picker reads
**"MP6 Stage Piano"**.

### Honestly worse than midir in one place

midir hands the callback a **device** timestamp, and `midi.rs` explains at
length why that matters. FreeBSD's MIDI device does not offer one: `read(2)`
returns bytes and there is nowhere in the interface for a timestamp to come
from. The stamp is therefore taken when the read completes, and every message
from one read shares it. Bounded and small — 1 kHz USB polling, no work between
the `poll` wake and the stamp — but not zero, and the recorder timestamps takes
with these numbers.

Also found: **a MIDI node allows one reader at a time** (`EBUSY` on a second
open), so the error names the cause and the fix rather than saying "Device
busy".

---

## 9. The installer

Three artifacts from `scripts/build-freebsd.sh`:

| Artifact | Size | For |
|---|---|---|
| `tangent-<v>-freebsd-<abi>.pkg` | 8.3 MB | anyone with a network |
| `…-desktop.tar.gz` | 8.5 MB, 3 pkgs | offline, on a machine running a desktop |
| `…-complete.tar.gz` | 653 MB, 167 pkgs | offline, on a machine with only Xorg |

**A package, not a shell script — the opposite of the Linux answer, on
purpose.** `installer/linux/install.sh` argues its own case: "a .deb reaches
Debian and Ubuntu. The same argument then repeats for rpm, for Arch, and for
Void." That reasoning is about fragmentation and it inverts here. FreeBSD has
exactly one package format and every derivative uses the same `pkg`, so one
artifact reaches all of them and `pkg delete tangent` is a complete uninstall
the system tracks. The shipped `install.sh` is a wrapper that calls pkg
correctly and explains what happened.

**Offline works because of a specific, verified pkg behaviour**: `pkg add`
never contacts a repository, and when the named package has unmet dependencies
it searches *the directory the file is in* for archives that satisfy them
(`pkg-add(8)`). `pkg install ./file.pkg` goes to the network instead — the
right command for someone who has one, the wrong one for a bundle that already
carries everything. `pkg install -l` is local-only but does **not** scan the
directory, so it is the wrong tool despite sounding like the right one.

**The trap that would have broken it on the tester's machine.** The build host
is GhostBSD, which packages the base system as `GhostBSD-clibs`,
`GhostBSD-runtime`, `GhostBSD-zlib` and `GhostBSD-libexecinfo`. Those own
`libc.so.7` and friends *here* and own nothing at all on plain FreeBSD, where
the same files ship in base. A manifest naming them would install only on
GhostBSD. `build-freebsd.sh` asserts their absence and refuses to ship
otherwise — a failure that appears only on a clean machine, which is the
machine nobody building it is sitting at.

Two smaller things, both found only by running it: a `.pkg` stores its paths
**absolute**, so a `--home` extraction asking tar for `usr/local` matches
nothing and silently yields an empty install; and `du` reports *allocated*
blocks, so on compressed ZFS it called an 8.3 MB package 5.7 MB and a
just-written tarball 512 bytes.

No post-install script is needed: `update-desktop-database` and
`gtk-update-icon-cache` are system-wide pkg **triggers** that fire for any
package writing to `share/applications` or `share/icons`. Advisory text lives
in a `messages` block instead, because scripts can be suppressed with
`pkg add -I` and guidance nobody reads is not guidance.

A no-root install cannot go through pkg at all — `PKG_DBDIR` must be owned by
`root:wheel` with no override — so `install.sh --home` is a plain file copy
into `~/.local`, and owns its own uninstall.

---

## 10. What is worth doing next, in order

1. **A FreeBSD 14 artifact**, if the tester is on 14. ABI is enforced at
   install time, so it needs a 14 build host — nothing else about the port
   changes. This is the only item that could block a real user today.
2. **Device selection.** `["default", "oss"]` is the last visible wart, and §1
   establishes it cannot be fixed from this side: cpal never asks alsa-lib for
   hints. Teaching its ALSA backend to enumerate hints as well as cards is a
   small upstream patch that would also help custom PCMs on Linux. Until then,
   `sysctl hw.snd.default_unit=N` is the answer and the Audio Setup panel
   should probably say so.
3. **A native OSS backend for cpal**, if 37 ms is ever judged too slow. §7 puts
   a number on what it would buy — about 8 ms, plus a buffer-size control that
   means something. Still a real upstream contribution, still not on the
   critical path.
4. Plugin editor and camera, per §5, when there is hardware to test them with.
5. The `audio.rs:1027` `max_sample_rate()` landmine in §5, which wants a fix
   that can be tested rather than a blind one.

---

## 11. Reproducing

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
