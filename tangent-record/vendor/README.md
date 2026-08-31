# Vendored `linuxvideo`

`linuxvideo/` is [`linuxvideo` 0.3.5](https://github.com/SludgePhD/LinuxVideo)
(0BSD) with three additions and no other changes. Upstream is unmodified
otherwise, deliberately: keeping the diff this small is what makes it
reviewable and what lets us drop the copy the moment the change lands upstream.

## Why

0.3.5 asks V4L2 for two capture buffers and gives the caller no say in it.
`ReadStream::dequeue` holds one buffer for as long as its callback runs, so the
driver has exactly one left to fill; a frame arriving in that window has
nowhere to go and overwrites one nobody has read.

That loss was also *unobservable*. V4L2 counts every frame the hardware
produces in `v4l2_buffer.sequence` whether the application collected it or not,
so a gap between consecutively dequeued buffers is an exact count of what was
lost — but 0.3.5 never surfaces the field. Without it, a camera dropping a
third of its frames and a camera dropping none look identical from up here.

The two are one change, not two: the buffer count is the fix and the sequence
number is the only way to tell whether the fix did anything.

## The diff

| Added | Where | What it is |
| --- | --- | --- |
| `VideoCaptureDevice::into_stream_with(count)` | `src/lib.rs` | `into_stream()` with the buffer count exposed. `into_stream()` now calls it with `DEFAULT_BUFFER_COUNT`, so existing callers are byte-identical in behaviour. |
| `ReadStream::buffer_count()` | `src/stream.rs` | How many buffers the driver actually granted. `VIDIOC_REQBUFS` is a request, not a demand — anything reasoning about queue depth has to read back what it got. |
| `ReadBufferView::sequence()` | `src/stream.rs` | The driver's frame counter for this buffer, already read by `dequeue` and previously discarded. |

`Cargo.toml` additionally carries a note and is excluded from the workspace by
the root `Cargo.toml`, so it stays out of ivory's lints and `cargo test
--workspace`.

Regenerate the diff with:

    diff -ru ~/.cargo/registry/src/*/linuxvideo-0.3.5/src tangent-record/vendor/linuxvideo/src

## What it measured

`cargo run --release -p tangent-record --example bufdepth` and `--example
seqprobe`, against the 2012 FaceTime HD at 1280x720 MJPG:

- The steady state loses **nothing at any depth**, two included. With the
  decode moved out of the dequeue closure and VA-API doing it in 2.3 ms, the
  capture thread no longer holds a buffer long enough to matter.
- Under a deliberate 300 ms stall every 15 frames, depth 2 loses 10 frames,
  depth 4 loses 5, and depth 6 and 8 lose none. Six is the knee, which is why
  it is the default.
- `sequence` **is** maintained by uvcvideo — it steps by exactly 1 across 35
  consecutive frames with a fast consumer — so the zeros above are a real
  result and not a dead field. `seqprobe` exists to keep that check honest on
  any other camera.
- A sustained slow consumer is not fixed by depth and cannot be: at a 120 ms
  hold, depth 8 loses 16 frames where depth 2 loses 27. A deeper queue takes
  longer to saturate; it does not stop a consumer that is slower than the
  producer.
- **Buffers cost the uncompressed rate even for MJPEG.** uvcvideo sets
  `sizeimage` to the YUYV worst case — 1,843,200 bytes at 720p, measured — so
  six buffers is 10.5 MB, not the ~1.5 MB a compressed-frame estimate suggests.

## Upstream

The change is small and general and should go back:
<https://github.com/SludgePhD/LinuxVideo>. Once it is released, delete this
directory, drop the `exclude` line from the root `Cargo.toml`, and put
`linuxvideo = "0.3.x"` back in `tangent-record/Cargo.toml`.

---

# Vendored `cpal`

`cpal/` is [`cpal` 0.16.0](https://github.com/RustAudio/cpal) (Apache-2.0) with
**one host added, one string changed, and one notification stopped being
reported as an error**. The host is behind `#[cfg(target_os = "freebsd")]`; the
other two are in the JACK backend, which compiles only where the `jack` feature
is on. So macOS and Windows compile the
same code they compiled from crates.io and produce the same binary. That
constraint is the whole discipline here: `[patch.crates-io]` is not
target-conditional, so this copy is what *every* platform builds, and the moment
it diverges where an unfeatured platform can see it, it stops being a patch and
becomes a fork of everybody's audio.

## Why

cpal already runs on FreeBSD — it picks the ALSA host, which works because
FreeBSD's `audio/alsa-lib` port implements the ALSA API over OSS. It is not
fast, and the measurements are in `docs/FREEBSD-5.0-FINDINGS.md` §7. Round trip
through the shim is **27.6 ms and immovable**: buffer sizes from 128 to 1024
frames change nothing, the device's native rate is worth 0.4 ms,
`hw.snd.latency`/`usefrags`/`bitperfect`/vchans are worth under a millisecond
between them, and opening the raw `oss` PCM instead of `default` — which
removes alsa-lib's `plug` conversion layer entirely — lands on the *same*
number with worse jitter.

That last one is what settles it. Two different paths to the same hardware
agree, so `plug` is not the cost; alsa-lib's own buffering is, and it exposes
no knob for it. Speaking OSS directly is the only way past.

## What the host does

`src/host/oss/mod.rs`, plus two wiring lines (`src/host/mod.rs` declares the
module, `src/platform/mod.rs` adds `Oss oss "OSS"` to `impl_platform_host!`).

- **`BufferSize::Fixed(n)` becomes a real `SNDCTL_DSP_SETFRAGMENT`**, so the
  buffer control finally does something. Measured app-to-app, at two fragments:
  256 frames → 20.7 ms, 128 → 12.5 ms, 64 → 8.1 ms, 32 → 8.1 ms (the hardware
  floor). Through the shim every one of those is 27.5 ms.
- **Devices have names.** The shim exposes no ALSA *cards*, and cpal's ALSA
  enumeration iterates cards, so it can only ever return `"default"` and
  `"oss"`. Here each `/dev/dsp*` is a device named from the kernel's own
  `dev.pcm.N.%desc` — "Focusrite Scarlett Solo USB".
- **The reported ranges are probed, not asserted.** The shim claims a buffer
  range of `1..=4294967295` and rates to 4.29 GHz; this asks the driver.

## What it deliberately does not do

It is **not** the platform default. `default_host()` is still ALSA, for two
reasons: this host opens `/dev/dsp` directly and so does not get the shim's
software mixing, which can lock a second application out; and the shim is what
every existing FreeBSD cpal user is already running. Fast is a choice, working
is the default — the same bargain JACK gets.

## The tradeoff, measured

Two fragments is the default because it is what makes an unchanged caller
better off: at cpal's usual 256-frame request it is 20.7 ms against the shim's
27.5. It buys that with less slack. Tangent playing a held chord under four CPU
hogs for 45 seconds:

| | latency at 256 frames | underruns under load |
|---|---|---|
| alsa-lib shim | 27.5 ms | 0 |
| OSS, 2 fragments | 20.7 ms | 28 |
| OSS, 3 fragments (granted 4) | ~31 ms | 7 |

Idle, all three run clean. `CPAL_OSS_FRAGMENTS` raises the count for a machine
that crackles, which is the right direction for an escape hatch: the default is
fast, the override is safe. The real answer for load robustness is realtime
scheduling, which FreeBSD has via `mac_priority(4)`.

## The second change: what the JACK client is called

`host/jack/mod.rs` seeds `Host::name` with `"Tangent"` where upstream has
`"cpal_client"`. cpal appends `_in`/`_out` per client, so this is the
difference between a patchbay showing `Tangent_out:out_0` and showing
`cpal_client_out:out_0` — the name of a library the user has never heard of, on
the ports they are being asked to connect. Naming your client is not an
oversight upstream: `Host::input_device_with_name` exists for exactly this. It
is unreachable here, because it is a method on the concrete backend and
`cpal::Host` is an opaque enum, so an app that gets its host from
`host_from_id` — which is every app that lets the user choose an audio system —
can only take the constant.

It is one line and it touches no behaviour: JACK resolves name collisions
itself and cpal already reads the granted name back out of the client
(`device.rs`, `client.name()`), so two Tangents on one graph work exactly as two
cpal_clients did.

This one wants an upstream fix rather than an upstream copy of the string — a
name on `HostId::Jack` at `host_from_id` time, or a `Host::with_name`. Until
then the constant is the only place it fits.

## The third change: a notification is not an error

`host/jack/stream.rs`'s `NotificationHandler::buffer_size` told the host, through
the ERROR callback, that the graph's buffer size had changed — having already
resized its scratch and gone on to return `Control::Continue`. By cpal's own
account nothing had failed. It reported that way because the JACK backend has no
notification channel and the error one was the only way out.

**It is not a cosmetic miscategorisation.** A host cannot distinguish it from a
real fault, and it fires on EVERY activation: `Stream::new_output` seeds
`buffer_size` from `client.buffer_size()` on a client that has not joined the
graph yet, and under pipewire-jack that is not the size the graph then runs. An
app that reads a stream error as "the device is gone" — the correct reading —
tears its engine down, reopens, and is told the same thing again. Measured here
before the change: **Tangent's JACK path produced no sound at all**, rebuilding
its engine about once a second, while a flat meter and a low idle CPU made it
look like an efficient one.

It now goes to `eprintln!`, which is how this backend already reports (`mod.rs`
prints its device-creation failures) and which costs no dependency — `cpal` does
not pull `log`, and adding one to a vendored copy is a wider divergence than the
message is worth. Deliberately not dropped: losing the fact is a smaller version
of the same mistake, and a graph that resized under you is worth knowing.

This is the most upstream-able of the three, and the one that is least about us:
every host embedding cpal's JACK backend has it.

## Upstream

This should go back: <https://github.com/RustAudio/cpal>. FreeBSD is a
first-class cpal target already — the ALSA host is selected there deliberately
— so a native OSS host is filling in a platform cpal has claimed rather than
adding a new one. When it lands, delete this directory, drop the `exclude` and
`[patch.crates-io]` lines from the root `Cargo.toml`, and go back to
`cpal = "0.x"`.


---

# Vendored `jack-sys`

`jack-sys/` is [`jack-sys` 0.5.1](https://github.com/RustAudio/rust-jack)
(MIT OR Apache-2.0) with **one conditional added to `build.rs`** and no other
change. It has its own `PATCH.md`, because unlike the two above it is not about
a platform building — it is about what ends up in the artifact's `DT_NEEDED`.

The short version: `dynamic_loading` is a default feature of the `jack` crate
and means libjack is `dlopen`ed at run time, which is the only reason a shipped
binary can offer JACK without requiring it. `jack-sys`'s build script probed
pkg-config for libjack anyway and `unwrap()`ed the result on Linux, which both
made the build need development files it never reads and — worse, because it is
silent — emitted `cargo:rustc-link-lib=jack`, putting the library back in
`DT_NEEDED` and producing an artifact that will not start on a desktop without
it. The probe now runs only when `dynamic_loading` is off.

Verified on the Linux x86_64 artifact: `readelf -d` gains nothing, and
`jack_sys::library()` opens `libjack.so.0` at run time where one exists.
