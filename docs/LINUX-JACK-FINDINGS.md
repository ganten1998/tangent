# Linux: JACK beside ALSA — what it buys, and what it does not

> | Question | Answer |
> |---|---|
> | Does JACK work on Linux now? | Yes. `cpal`'s `jack` feature, on for `target_os = "linux"`. Picked in Audio Setup → SYSTEM. |
> | Does the artifact require libjack? | **No.** `readelf -d` gains nothing; `jack` 0.13 `dlopen`s it. One conditional vendored into `jack-sys` is what makes that true. |
> | Is it lower latency? | **Not measured, and the one measurement taken says the opposite** — 23.4 ms against ALSA's 5.6 ms on the monitor path. Both figures are unsound, for two different reasons. §3. |
> | So why ship it? | Named, patchable ports and no default-source roulette. §5. |
> | What would settle the latency question? | `rtclick` on a wired loopback. §6. |

Test box: 2012 MacBook Air (Ivy Bridge, 2 cores), Void Linux, XFCE+i3,
PipeWire 1.6.7 + WirePlumber, settled graph **44100 Hz / quantum 512**,
`clock.force-quantum 0`. Focusrite Scarlett Solo 3rd gen. **No `jackd` is
running and none was started**: `libjack.so.0` resolves to
`/usr/lib/pipewire-0.3/jack/libjack.so.0` via `/etc/ld.so.conf.d/pipewire-jack.conf`,
ahead of the jack2 copy also installed, so every "JACK" row below is
**PipeWire's JACK layer**.

## 1. Why this was expected to win

On a PipeWire desktop, cpal's ALSA host is not talking to hardware. `aplay -L`
says it plainly — `default` is "Default ALSA Output (currently PipeWire Media
Server)" — and the interfaces do not appear in the ALSA device list at all,
because PipeWire holds every card exclusively. The audio therefore crosses
pipewire-alsa, an emulation layer, and pays the four-period ring
`BUFFER_PERIODS` had to add for it (`LINUX-4.16-FINDINGS.md` §2–3: a stock
256-frame request underran six times in thirty seconds, and RT promotion made
it 75).

A JACK client has none of that shape. It is a node in the graph, its callback
runs on a thread the server created and scheduled, and the period is the
graph's. Deleting a stage ought to beat tuning one.

## 2. The runs

`cargo run --release -p tangent-record --example rtlatency -- 512 <secs>`,
which brackets the SOFTWARE path with the driver's own capture and playback
timestamps. `SYSTEM=JACK` selects the host; `IN_BUFFER` overrides the input
size that Tangent's own rule would pick.

| Host | in / out asked | round trip, median | output delay alone | stale dropped |
|---|---|---|---|---|
| ALSA | 128 / 512 | **5.62 – 5.88 ms** | **none reported** | 0 |
| ALSA | 512 / 512 | 12.88 ms | none reported | 512 |
| JACK | 128 / 512 | **23.38 – 26.38 ms** | **11.20 – 11.52 ms** | 0 / 384 |
| JACK | 512 / 512 | 31.88 ms | 11.4 ms | 0 |

Read naively: JACK is four times worse. Neither column supports that reading.

## 3. Both numbers are wrong, in opposite directions

### 3a. pipewire-alsa reports a PCM delay of ZERO

cpal's ALSA host computes the playback instant as
`callback + frames_to_duration(delay_frames)`, where `delay_frames` comes from
`snd_pcm_status_get_delay` (`vendor/cpal/src/host/alsa/mod.rs`, `process_output`).
The probe's new "output delay alone" line counts every output callback whose
`playback` is later than its `callback` — and on ALSA it counted **none at all**,
in every run, printing *this host reports none (playback == callback)*. Not a
small delay: no delay, on every callback. The same line on JACK, same box, same
graph, counts all 516–517 of them at 11.2–11.5 ms.

A PCM cannot play a buffer the instant the application hands it over. So the
ALSA round trip above is **capture-timestamp to callback-entry with the entire
output leg counted as free**. It is not 5.6 ms of anything; it is 5.6 ms plus
an unmeasured output stage. This is the same family of fault `rtclick`'s module
doc already records for FreeBSD's alsa-lib-over-OSS shim ("the driver reports a
capture buffer `0.00 ms` behind the callback, which cannot be true").

**This has a consequence beyond the comparison.** Anything in Tangent that
reads the output delay — `Engine::output_delay_ns`, the take manifest's latency
fields, delay compensation — is being told zero on every PipeWire desktop,
which is every desktop. That is a bug of its own and is not caused by, or fixed
by, this work. It is called out here because this is where it was found.

### 3b. cpal opens TWO JACK clients, and nothing orders them

cpal's JACK backend says so itself: *"Until we have duplex streams two clients
will be created adding 'out' or 'in' to the name"* (`host/jack/mod.rs`). So
Tangent appears in the graph as `Tangent_in` and `Tangent_out`, two independent
nodes with no edge between them. Tangent's monitor ring is written by one and
read by the other.

Nothing makes the input node run first. When the output node is scheduled
first, every captured frame waits a **full extra graph cycle**, and the ring
never drains below whatever the stale-drop policy permits. The arithmetic
matches exactly:

- `in_flight = out_block + in_block` = 512 + 128 = 640 frames → 23.4 ms measured
- `in_flight` = 512 + 512 = 1024 frames → 31.9 ms measured
- difference 384 frames = 8.7 ms; measured difference 8.5 ms

The steady state is set by whichever client's first callback wins a startup
race, and then persists — which is why run-to-run medians move (23.38, 26.38)
while within a run the spread is tight.

Two consequences worth separating:

1. **A duplex JACK client would not pay this**, and cpal's own comment says a
   duplex API is the missing piece. This is a cpal limitation, not PipeWire's.
2. **JACK cannot do Tangent's asymmetric trick.** `open_input` runs the input
   at a quarter of the output (floor 64) precisely because its callback only
   copies into a ring — that is worth ~9 ms on the ALSA row above. The server
   runs ONE period for every client, so under JACK the input is 512 whatever is
   asked.

## 4. What is honestly established

- **JACK reports its output delay correctly**: 11.2–11.5 ms for a 512-frame
  period at 44.1 kHz, which is one period, which is right. ALSA on this machine
  reports none at all.
- **The artifact gains nothing to carry.** `readelf -d` on the release build
  lists `libasound`, `libgcc_s`, `libc` — no `libjack.so.0`, ever. No new crate
  enters the dependency graph either; `jack` and `jack-sys` were already in
  `Cargo.lock` for the FreeBSD build, and the only lockfile change is
  `jack-sys` becoming a path dependency.
- **A JACK client sees the interface by name.** `Scarlett Solo (3rd Gen.)
  Pro:capture_AUX0`, `:capture_AUX1`, `:playback_AUX0`, `:playback_AUX1`,
  alongside `Built-in Audio Analog Stereo:*`.

## 5. Why it ships anyway

Nothing above is a latency claim, and the case does not rest on one:

- **The default-source roulette ends.** Through ALSA there is exactly one
  openable PCM, named `pipewire`, and it follows whatever the desktop's default
  source happens to be — the fault that once recorded the laptop microphone
  with a Scarlett plugged in. Through JACK the ports are named and stay put.
- **Tangent becomes patchable.** It appears in qjackctl, Carla or Patchance and
  can be wired into a DAW sample-synchronously, which is the reason to be
  running a pro-audio session at all.
- **It is free.** See §4. A machine with no pro-audio stack runs exactly the
  binary it ran before and never opens the library.

And it is not the default, deliberately: `default_host()` on Linux is a
hardcoded `AlsaHost::new()` whatever features are compiled in. Given §3, that
is now load-bearing rather than merely cautious.

## 6. What would settle it

`rtclick` — the instrument this project already keeps for drivers that
misreport — on a **wired loopback** (Scarlett output → Scarlett input). It
emits a click and measures the wall clock until it comes back, assuming nothing
about either driver's timestamps, which is exactly the assumption §3a shows
cannot be made here.

    cargo run --release -p tangent-record --example rtclick -- 20 512 44100
    SYSTEM=JACK cargo run --release -p tangent-record --example rtclick -- 20 512 44100

(`rtclick` does not yet take `SYSTEM`; `rtlatency` does. Adding it is the same
dozen lines.)

Two other things worth measuring before believing anything here:

- **Whether the two JACK clients' order can be observed.** If `Tangent_in`
  reliably runs after `Tangent_out`, §3b is the whole story and a duplex client
  would recover ~11.6 ms.
- **The ALSA output delay against real hardware.** It could not be taken here:
  PipeWire holds the cards, so `DEVICE=sysdefault` answers "no output device".
  A box with PipeWire stopped would say whether zero is pipewire-alsa's answer
  or cpal's.

## 7. What a user has to install

Nothing, to run Tangent. To use the JACK entry: PipeWire's JACK layer, which
most distributions ship by default and some package separately (`pipewire-jack`
on Debian/Ubuntu, `pipewire-jack-audio-connection-kit` on Fedora,
`libjack-pipewire` on Void). Where the distribution keeps that `libjack` off
the default loader path so it will not shadow a real jackd, starting Tangent as
`pw-jack tangent` is the supported way round it — the same instruction those
distributions give for every other JACK application. Choosing JACK where no
server answers falls back to ALSA rather than leaving the app silent.
