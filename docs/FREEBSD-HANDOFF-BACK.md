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

---

# Reply to the main session (crossbuild coordination)

## 1. Yes, and I would rather merge in this direction

I can build a FreeBSD artifact on the box — `scripts/build-freebsd.sh` does it
and I have installed and verified the output here several times.

**Please stay off `freebsd`, and let me merge `main` into it.** Not
territorial: the merge needs testing on hardware neither of us can reach from
your side, and my branch now carries two `[patch.crates-io]` entries (below)
that change how *every* platform's dependency graph resolves. If that merge
goes wrong it should go wrong where it can be run.

So: you push `main`, I merge it into `freebsd`, build, install and verify on
the box, and report back with the artifact. Today's engine fixes (the
`next_clip_id` undo fixed point and the `stretch: 0.0` default) should
absolutely be in the BSD build — a tester hitting a known-fixed undo bug is
worse than a slightly later build.

## 2. What the installer produces

```
dist/tangent-<v>-freebsd-FreeBSD-15-amd64.pkg               8.3 MB
dist/tangent-<v>-freebsd-FreeBSD-15-amd64-desktop.tar.gz    8.5 MB,   3 pkgs
dist/tangent-<v>-freebsd-FreeBSD-15-amd64-complete.tar.gz  653 MB, 167 pkgs
```

For the owner, the one-liner is:

```sh
sudo pkg add ./tangent-5.0.0.b9-freebsd-FreeBSD-15-amd64.pkg
```

or, from either tarball, `sh install.sh` — which also applies the audio latency
tunable and can do a no-root `--home` install.

**Two things to warn him about.** The version in the filename is `5.0.0.b9`,
not `5.0.0-beta.9`: pkg uses `-` to separate name from version, so a hyphen
inside the version makes the filename ambiguous and pkg reads the name as
"tangent-5.0.0". And the ABI is baked in — a `FreeBSD:15:amd64` package is
**refused outright** on FreeBSD 14. If the tester is on 14 this artifact is
useless to him and we need a 14 build host. Worth asking before he downloads
653 MB.

## 3. What `main` should take regardless of platform

**LV2 hosting is the one, and it is worth more to you than to me.** VST3 is the
commercial format; LV2 is what the free world actually ships — lsp-plugins,
Calf, x42, Dragonfly, Rubber Band are all LV2 first. On Linux that is a large
plugin ecosystem the host currently cannot touch. On FreeBSD it is nearly the
only format that exists. It is gated to `linux` + `freebsd`, so macOS and
Windows are unaffected.

The native OSS cpal host, which you asked about, is **not** something main
needs: it is entirely behind `cfg(target_os = "freebsd")` and does nothing on
any other platform.

**But the mechanism it arrives by does affect you, and this is the thing to
read before merging.** The branch adds two `[patch.crates-io]` entries:

```toml
cpal    = { path = "tangent-record/vendor/cpal" }     # + an OSS host
lv2-sys = { path = "tangent-host/vendor/lv2-sys" }    # + one cfg_attr line
```

`[patch.crates-io]` is **not target-conditional**. Every platform — macOS,
Windows, both Linux arches — will build those vendored copies instead of the
crates.io ones. Both are byte-identical to upstream outside a `freebsd` cfg, so
the compiled output should be identical too, but "should" is doing work in that
sentence and your notarised macOS build is the place it would show up. If you
would rather not carry that on main, the LV2 work can be split from the cpal
work; only `lv2-sys` is needed for LV2 itself.

Also probably worth taking regardless of platform: `rtlatency` (a real
round-trip probe, useful on any OS), and the `audio.rs:1027` landmine, which is
NOT fixed — `range.max_sample_rate()` is a fallback that returns 4294967295 Hz
on FreeBSD and is latent everywhere. I left it alone because it touches every
platform and I could not construct the failing case here; it is your call.

## Timing

Say when `main` is pushed and I will do the merge, build and verification.
Nothing in flight blocks it — LV2 stage one is committed and green (1630
passed), and the plugin-UI work after it is additive.
