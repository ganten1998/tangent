# FreeBSD port — handoff back for review

**Branch `freebsd`, 12 commits, `main` untouched.**
<https://codeberg.org/ganten1998/tangent/compare/main...freebsd>

Real hardware: HP laptop, GhostBSD 26.1 / FreeBSD 15.0-RELEASE-p2 amd64,
i5-1135G7, Realtek ALC236, Iris Xe, X11/MATE. A Kawai MP6 and a Focusrite
Scarlett Solo were attached for the MIDI and audio-in work. Every measurement
below is in `docs/FREEBSD-5.0-FINDINGS.md`.

## The bar

All seven met. Build 5m36s; the app draws; audio out and in verified;
`cargo test --workspace --features recorder` **1619 passed / 0 failed**;
`check-firewall.sh` green including Minimal; latency measured — and then taken
from 36.7 ms to 8.1 ms.

## Where the brief was right, and where the audit was not

The three build-breakers were exactly as described, and are fixed the way you
asked: a FreeBSD arm beside the Linux one, never widening Linux's to `unix`.

Two things in the 5.0 audit turned out backwards.

**1. `BUFFER_PERIODS`.** The audit says widen Linux's `4` to cover FreeBSD.
Measured, that makes it *worse*. Linux's 4 compensates for cpal's ALSA host
carving `Fixed(v)` into four periods; FreeBSD's shim does not carve at all — it
hands the callback whatever `snd_pcm_avail` reports. `Fixed(256)` → 205–256
frame callbacks, `Fixed(1024)` → 264–500. The else-arm value of `1` was already
right; it is now an explicit arm that says so.

**2. The compositor is a hard crash, not "degraded video".**
`renders_on_the_cpu()` builds a default wgpu instance whose GL enumeration calls
`eglMakeCurrent` while eframe's glow context is current on that thread →
`BAD_ACCESS` inside wgpu-hal's unwrap, before a frame is drawn. Linux never
reaches it because `owned_gl_is_hardware()` returns first. Fixed by excluding
the GL backend from the probe on FreeBSD, which costs nothing — a GL adapter is
refused two lines later anyway.

Also: **`IVORY_RENDERER=glow` is a no-op here.** The renderer is hardcoded to
Glow on everything but macOS, so that lever does not exist. Worth knowing before
sending the tester after it.

## The thing that is not our bug and looks like it

`pkg install rust` on GhostBSD produces a compiler that cannot link a
hello-world: `ld: error: unable to find library -lexecinfo`. libexecinfo ships
as a separate base package whose dev half is not installed by default.

```
pkg install GhostBSD-libexecinfo GhostBSD-libexecinfo-dev
```

Very likely part of what the tester hit, and it reads as a project failure. It
leads the findings document for that reason.

## MIDI — the fork decision you asked to be consulted on

You asked to be checked with before vendoring. **I did vendor**, and the owner
approved the maintenance burden explicitly — so flag it if you disagree.

midir's `jack` feature compiled but was the wrong shape twice over: libjack
became a link-time dependency of the whole application, and no keyboard worked
until the user installed and started a JACK server. For a MIDI keyboard monitor
that is the product not working when you plug a keyboard in.

`tangent/src/umidi.rs` reads `/dev/umidi*` directly; midir moves to
`cfg(not(target_os = "freebsd"))`. The whole platform difference at the call
site is one `use` alias — `midi.rs` is otherwise untouched.

The device was captured before a line was written, because the central question
had two plausible answers and the wrong one yields a parser that silently drops
every message: it is a **raw MIDI byte stream**, not USB-MIDI 4-byte packets.
That capture is now a fixture (2628 bytes, 876 messages) asserting every byte is
consumed and that the result is identical whether the parser is fed 1 byte at a
time or 1023.

**Two silent bugs fell out of removing the JACK path**, and they are the real
argument for the work:

* midir's JACK backend invokes the callback on JACK's **realtime process
  thread**, where our callback allocates (`bytes.to_vec()`), takes a `Mutex` and
  sends on an `mpsc` — three realtime violations per message.
* midir's JACK backend **stores `ignore_flags` and never applies them**, so
  `Ignore::TimeAndActiveSense` was a no-op and any Active-Sensing keyboard would
  have put ~4000 junk messages into a twenty-minute take.

Verified live: chords from the MP6 named correctly, synth audible.

## Audio — the recommendation changed once it was measured

Shipped: ALSA-over-OSS as the default, with JACK and a **native OSS host**
beside it. The panel reads `["JACK", "ALSA", "OSS"]`.

The shim floors at **27.5 ms and nothing moves it**: buffer size 128–1024, the
native rate, `hw.snd.latency`/`usefrags`/`bitperfect`/vchans, and — the
informative one — the raw `oss` PCM instead of `default`, which removes
alsa-lib's `plug` layer entirely and lands on the same number with worse jitter.
Two paths to the same hardware agreeing means `plug` is not the cost; alsa-lib's
own buffering is, and it exposes no knob.

So I wrote the native OSS host you said to flag if it proved necessary:
`tangent-record/vendor/cpal`, cpal 0.16.0 plus one host, everything behind
`cfg(freebsd)` so other platforms compile byte-identical code. 256 frames →
20.7 ms, 64 frames → **8.1 ms**, and the buffer control finally does something.
It also fixes the device picker (real names from `dev.pcm.N.%desc`) which §1 had
written off as unfixable. **Not the default** — the shim stays default because
it *mixes*, and on a machine somebody just installed that is worth more than
19 ms.

**The biggest single win is one tunable, not code.**
`hw.usb.uaudio.buffer_ms` 4 → 1 takes ~9 ms off *every* path and drops the
driver's minimum block from 192 frames to 48, which is what makes small JACK
periods legal at all. 68,884 interrupts / 0 underruns under four CPU hogs.
`install.sh` applies it as a marked, reversible block in `/etc/sysctl.conf`.

**A number I nearly reported and should not have.** The OSS host first measured
5.05 ms, flat across every buffer size — the same signature as the shim's fake
buffer control. `SNDCTL_DSP_GETODELAY` reports only the hardware queue, so the
host's own timestamp under-reported and the probe believed it. Added
`IVORY_RAW_TIMING`, which trusts no backend's claims and measures wall clock
callback-to-callback. Worth knowing if you review the probe.

**vs Ardour**, checked rather than assumed: `ardour-8.12.0_3` on FreeBSD ships
exactly two backends — `libdummy_audiobackend.so` and
`libjack_audiobackend.so` — and depends on `jackit`, not `alsa-lib`. It cannot
make a sound on a fresh install. REAPER has no FreeBSD port at all. Tangent is
lower latency than Ardour's floor *and* the only one of the three that works
when a musician installs it.

## Installer — the Linux reasoning inverted on purpose

`installer/linux/install.sh` argues "a .deb reaches Debian and Ubuntu; the same
argument repeats for rpm, Arch, Void". That is about *fragmentation* and it does
not survive the crossing: FreeBSD has one package format every derivative uses.
So it is a real `.pkg`, and `pkg delete tangent` is a complete uninstall the
system tracks.

```
tangent-<v>-freebsd-<abi>.pkg               8.3 MB
…-desktop.tar.gz          8.5 MB,   3 pkgs   offline, machine with a desktop
…-complete.tar.gz       653 MB, 167 pkgs     offline, only Xorg installed
```

Offline works because `pkg add` never contacts a repository and searches its own
directory for unmet dependencies. `pkg install -l` sounds like the right tool
and is not — local-only, but it does not scan.

**The trap worth knowing:** GhostBSD packages the base system as
`GhostBSD-clibs`/`-libexecinfo`/etc. Those own `libc.so.7` here and nothing at
all on plain FreeBSD. A manifest naming them installs only on GhostBSD — exactly
backwards for a port whose one known tester may be on anything.
`build-freebsd.sh` asserts their absence and refuses to ship otherwise.

Verified by doing it: installed for real (both desktop triggers fire, so no
post-install script is needed at all), removed, reinstalled **offline** from the
bundle, plus `--home` and `--uninstall` end to end.

## What I did not do, and why

* **Plugin editor stays `Unsupported`.** The audit is right that it is
  gate-widening, but with no VST3 on the box, flipping `EDITORS_SUPPORTED` swaps
  an honest "Unsupported" for an untested path whose failure mode is a crash
  inside somebody's plugin. Exact gate list is in §5.
* **`CONTENTS_SUBDIR` is `x86_64-freebsd`** from the spec's `<cpu>-<os>` shape
  and labelled a guess in the source — there is no Steinberg BSD arm to check
  against. `binary_in_bundle` also looks in the Linux-named directory, since
  frameworks that support FreeBSD mostly do it by pretending to be Linux.
* **`audio.rs:1027`** falls back to `range.max_sample_rate()`, which is
  4294967295 Hz on FreeBSD. Latent (the 44100 default always matches), severe if
  it fires, and not fixable blind since it touches every platform. Your call.
* **Camera stays stubbed:** `linuxvideo` encodes Linux `_IOC` numbers and
  FreeBSD's bit layout differs (3-bit dir/13-bit size vs 2/14).
* **ABI is `FreeBSD:15:amd64`.** A 14 artifact needs a 14 build host; the owner
  opted to assume 15. This is the one remaining thing that could block a real
  user.

## Caveat on process

The box hard-hung once mid-session — no panic, no crash dump. The most likely
cause was mine: repeated root-owned **realtime** jackd instances plus a
`usbconfig reset` of the interface. Nothing that ships starts a realtime
process, and `tangent-jack` stops what it starts. Worth knowing before anyone
reproduces the RT measurements.

## Open questions

1. **Upstreaming the OSS host to cpal** is what removes the vendored copy.
   FreeBSD is already a claimed cpal target, so this fills in a platform rather
   than adding one.
2. The `audio.rs` `max_sample_rate` landmine — fix blind, or leave?
3. Whether the plugin editor is worth doing once there is a VST3 to test with.
