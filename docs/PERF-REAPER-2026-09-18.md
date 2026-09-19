<!-- Written 2026-09-18 by the perf sweep (workflow wf_354a6c6b-bd7: six
     source scouts, 78 adversarial verdicts, an in-process engine bench, a
     device harness that correctly declined to run, and a synthesis). Raw
     material, every probe and its output: ~/Library/Caches/tangent-perf-2026-09-18/ -->

# Tangent vs REAPER: the standing, the reason, and the road to parity

Machine: Apple M5 Pro (5 performance + 10 efficiency cores), 24 GB, macOS 27.0, Scarlett 18i20 USB.
Repo: tangent @ 76212f6, clean except `tangent/src/rt_bench.rs` (new, test-only) and a 4-line `mod rt_bench;` in `tangent/src/main.rs`.

---

## 1. The standing

### 1.1 The caveat that bounds everything below: no head-to-head run happened

**There is no REAPER measurement in this sweep.** The device-bench agent built the entire two-sided harness (a ReaScript underrun poller, a Tangent project generator, a CPU sampler, backup/restore with sha256 proof) and then refused to run it, correctly: Tangent (pid 55992, the owner's own session) held the Scarlett at 128 frames for the whole window, and the brief forbids two hosts on one device. It waited 18 minutes across two blocks and stopped. Nothing was launched, nothing of the owner's was touched.

So every statement about REAPER below comes from two places only, and is labelled as such:

- `~/Library/Application Support/REAPER/reaper.ini`: `coreaudiobs=128`, `coreaudiosrate=48000`, `workthreads=15`, `workbehvr=8` (anticipative FX on), in/out = Scarlett 18i20 USB.
- The article's description of the hybrid engine (pre-render non-monitored tracks ahead of the real-time thread; force record-armed and input-monitored tracks back onto it).

Everything about **Tangent** below is measured on this machine, and I say which harness produced each number.

Three further bounds on the Tangent numbers:

1. The engine bench ran `Renderer::render` on an ordinary cargo-test thread at ordinary priority, not on a CoreAudio time-constraint thread. Means and p99s are honest arithmetic; the `max` column includes plain preemption and the "0 blocks over deadline" column means nothing, because that thread was never asked to meet a deadline.
2. The offline path skips the driver interleave, the monitor input pass and all xrun accounting. Real callbacks carry those.
3. Every plugin was measured at its **default state**, and because of finding PH-2 no VST3 in Tangent has ever received an automation point. All plugin costs here are static-parameter costs. Pro-Q 4 and Brusfri measure 0.2 us because their defaults are passthrough, so their real DSP cost is unknown, not small.

### 1.2 What Tangent's engine costs, measured

In-process bench (`cargo test --release -p tangent --bin tangent -- rt_bench --ignored`), device-less `Engine::offline` at 48 kHz, 2000 blocks after 400 warm. Deadline at 128 frames is 2666.7 us.

| Shape | mean us | p99 us | p99 as % of deadline |
|---|---:|---:|---:|
| 1 lane, no inserts | 18.9 | 27.6 | 1.0% |
| 16 lanes, no inserts | 21.3 | 30.6 | 1.1% |
| 32 lanes, no inserts | 28.5 | 37.9 | 1.4% |
| 16 x 3 built-in EQ (48 bays) | 46.4 | 71.5 | 2.7% |
| the same, every lane MUTED | 40.5 | 53.7 | 2.0% |
| the same, every lane FROZEN | 20.4 | 26.0 | 1.0% |
| the same, transport STOPPED and parked | 11.9 | 15.5 | 0.6% |
| 16 x [EQ, comp, plate] (48 bays) | 140.9 | 164.8 | 6.2% |
| the same @ 64 frames (deadline 1333.3) | 74.1 | 90.5 | 6.8% |
| the same @ 256 frames (deadline 5333.3) | 273.1 | 303.9 | 5.7% |
| 16 x built-in denoise, spectral studio | 173.6 | 351.5 | 13.2% |
| 32 x 3 built-in EQ (96 bays) | 69.2 | 93.5 | 3.5% |
| 8 lanes x Analog, 8 voices each (64 voices) | 504.5 | 550.8 | 20.7% |
| 16 x FabFilter Pro-Q 4, default state | 35.3 | 50.0 | 1.9% |

Derived from the same table:

| Quantity | Measured |
|---|---:|
| Fixed cost of a 16-lane desk holding nothing | 14.1 us/block |
| Marginal bare audio lane (16 to 32) | 0.45 us/block |
| Marginal built-in EQ instance | 0.52 us/block |
| Marginal [EQ, comp, plate] lane | 7.48 us/block (2.49 us/instance) |
| Marginal denoise instance | 9.52 us/block |
| Marginal Analog voice | 7.59 us/block |
| Marginal hosted VST3 (Pro-Q 4, passthrough default) | 0.87 us/block |
| Per-frame slope, 16 x [EQ, comp, plate] | 1.0365 us/frame |
| Fixed per-callback overhead, block-size independent | 7.77 us/block |

### 1.3 What a plugin costs on this machine, measured

Headless probe against `tangent-host` (`baycost`, 4000 timed blocks, 128 frames, stereo), plus two independent in-repo benches for Tangent's own units.

| Insert (128-frame block) | median us | % of deadline |
|---|---:|---:|
| FabFilter Pro-Q 4 (default, passthrough) | 0.2 | 0.0% |
| Brusfri (default, passthrough) | 0.2 | 0.0% |
| Tangent built-in EQ | 0.5 | 0.0% |
| Tangent [EQ, comp, plate] average | 2.5 | 0.1% |
| FabFilter Pro-C 3 | 6.4 | 0.2% |
| Arturia Pre 1973 | 8.5 | 0.3% |
| FabFilter Saturn 2 | 8.6 | 0.3% |
| Tangent built-in denoise | 9.5 | 0.4% |
| Arturia Comp FET-76 | 13.7 | 0.5% |
| FabFilter Timeless 3 | 16.8 | 0.6% |
| FabFilter Pro-L 2 | 19.8 | 0.7% |
| Arturia Rev PLATE-140 | 28.2 | 1.1% |
| Arturia Delay TAPE-201 | 37.6 | 1.4% |
| FabFilter Pro-R 2 | 40.1 | 1.5% |
| **Tangent built-in Tape (defaults)** | **62.0** | **2.3%** |
| **Tangent built-in Drive (defaults)** | **88.3** | **3.3%** |

Instruments, same block: Mini V4 47.2, Piano V3 134.8, Jup-8 V4 140.2, **Pianoteq 9 316.1 us (11.9% of a whole 128-frame block, per instance)**.

Mean of the nine non-passthrough commercial effects: **20.0 us per instance per block**.

### 1.4 The standing, stated plainly

**Per instance, Tangent is competitive.** Its VST3 bridge adds about 0.35 to 0.9 us per instance per block on top of the plugin's own work (0.87 us measured for a passthrough Pro-Q 4 against a 0.5 us built-in EQ), its per-bay wrapper costs 0.057 us (measured, PH-4), and a 16-lane desk holding nothing costs 14.1 us. Its light built-ins are cheap: an EQ, comp and plate chain is 2.5 us per instance against 20.0 us for the commercial average.

**Per core, Tangent is one core.** Everything renders inside one CoreAudio callback. `tangent/src/instrument.rs:15530` is the only real-time caller of `Renderer::render`, and `instrument.rs` contains no thread spawn on that path at all. REAPER on this same machine is configured with 15 worker threads and anticipative FX on.

So the 16-lane, 3-insert desk the owner asked about, at 128 frames / 48 kHz, on one core:

| Desk contents (48 bays) | Serial cost | % of 2666.7 us deadline |
|---|---:|---:|
| Tangent [EQ, comp, plate] | 141 us (measured) | 5.3% |
| Light commercial (Pro-C, Saturn, Pre 1973) | 397 us | 14.9% |
| Commercial average (20.0 us/bay) | 981 us | 36.8% |
| Heavy commercial (Pro-R, TAPE-201, PLATE-140) | 1715 us | 64.3% |
| All Tangent Tape | 2997 us | **112%, dropout** |
| All Tangent Drive | 4259 us | **160%, dropout** |
| Plus one Pianoteq lane | +316 us | +11.9% |

And the ceiling, one core, keeping 30% headroom (1846 us usable after the engine floor):

| Bay contents | Bays before saturation |
|---|---:|
| Tangent [EQ, comp, plate] | 741 (capped at 96 to 192 by the insert grid) |
| Commercial average 20.0 us | 92 |
| Heavy commercial 35.3 us | 52 |
| Tangent Drive 88.3 us | 20 |

Tangent's addressable grid is 32 lanes x 3 bays on the primary seat, plus 3 more on an extra seat (`tangent-ui/src/recorder.rs:1202-1218`), so 96 to 192 bays. REAPER has no per-track FX cap. At 128 frames Tangent's single thread saturates at roughly the same point its own bay grid runs out; at 64 frames it saturates at about 45 commercial bays, well below the grid.

**The three-sentence answer to "where are we".**

1. Tangent's per-instance hosting overhead is competitive with REAPER's and is not where the gap is.
2. The gap is that Tangent uses one of fifteen cores and pre-renders nothing, while REAPER as shipped on this machine uses fifteen and pre-renders every non-monitored track ahead of the deadline. That is a factor of roughly 4 to 5 available from parallelism alone on this machine (measured), plus anticipation removing non-armed tracks from the per-block deadline entirely.
3. Two of Tangent's own built-in units, Drive at 88.3 us and Tape at 62.0 us per block, are more expensive than any commercial plugin on the machine (Pro-R 2, the heaviest, is 40.1 us). Drive is 10x FabFilter Saturn 2 for comparable work. That is a self-inflicted wound that a REAPER comparison would expose immediately and it is fixable with code already in this repo.

---

## 2. Why: the architecture that explains the gap

### 2.1 One thread against fifteen

`build_for` installs one cpal output callback (`tangent/src/instrument.rs:15505-15545`). It calls `flush_denormals`, takes one timestamp for event placement, and loops `renderer.render` over the device buffer. `render` calls `render_program` (`instrument.rs:5649`), which chunks at `MAX_BLOCK` = 512 and runs a strictly serial spine per chunk: transport, automation, clips, the 32-lane walk, the monitor pass, pads, the folder fold, the click, the Kahn-ordered sends pass, the sidechain key snapshot, the Fx bus, both master racks, master effects, then the per-frame out loop that fills the recorder tap, the scope, ReaStream and the device.

Inside the lane walk (`instrument.rs:9376`), each lane seeds `fx_in`, renders its DX7 or Keys instrument, runs seven pedal racks, `run_rack` on its own seat, `run_extra_rack` on its second seat, its PDC line, then folds into the mix. All of it in sequence, all on one thread. Per-block time is the **sum** over every rack run in the chunk, and that exceeds the lane count, because a send destination's rack runs a second time in the sends pass, a folder's rack runs at the fold, and a room-claimed rack runs in the monitor pass.

### 2.2 The shared scratch pair is the lock

`fx_in` and `fx_out` are allocated once for the whole desk (`instrument.rs:11899-11900`) and handed to every single `run_rack` call site: lanes, extra seats, pedals, pads, folds, sends, buses, the master, the room. `run_rack`'s own comment says it: `fx_out` is one buffer used by every rack on the desk. Even if a worker thread existed today, two racks could not run at once. The same is true of `key_io`, `dry_io`, `builtin_scratch`, `keys_scratch`, `send_io` and `pad_io`.

That is the structural fact behind the whole sweep: the engine is not slow, it is **serialized by construction**, and the serialization is one shared buffer set deep.

### 2.3 What a resting desk costs, and what a rolling one wastes

Stopped and parked, a 16-lane desk holding 48 inserts costs 11.9 us per block (0.45% of a core). That is excellent and it is not the problem.

Rolling, the same desk costs 46.4 us, and here is why that matters more than the 35 us difference suggests. While the transport rolls, `park_lane` is short-circuited by `rolling` (`instrument.rs:9858-9863`, `3482-3500`), so **every** lane with a rack and any clip **anywhere on the timeline** runs its full chain every block, including blocks with nothing under the playhead and lanes that are muted. The clip mask is whole-timeline by design (`instrument.rs:8357-8360`), and mute is applied only as a fader target after the rack has already run (`instrument.rs:10218-10222`).

Two measurements settle what this costs:

- Muting all 16 lanes saved only **12.7%** (46.4 to 40.5 us). Muted lanes pay their whole chain.
- Plugins cost the same on silence, measured: Pro-R 2 was 4% more expensive on silence than on signal (36.39 vs 34.93 us); across the set, silence is 82 to 104% of signal cost. Skipping a silent lane's chain is the only way to save that time.

Freezing all 16 lanes saved **56%** (46.4 to 20.4 us), landing exactly on the bare-lane figure, so the freeze stand-down (`instrument.rs:3854`) works, for the primary seat.

### 2.4 Per-block fixed overhead and small buffers

The 64/128/256-frame sweep gives a per-frame slope of 1.0365 us and an intercept of **7.77 us** that the engine pays once per callback whatever the buffer is. That is 0.29% of a 128-frame deadline and 0.58% of a 64-frame one: the fixed share doubles every time the buffer halves. It is not currently a problem, but it is the floor under any low-latency ambition, and it is the one number that gets worse rather than better as the owner chases REAPER's small-buffer behaviour.

### 2.5 What parallelism is actually worth here (measured, not assumed)

Pro-R 2 run concurrently on plain threads, same machine:

| Concurrent threads | Median per instance | Worst per-block max seen |
|---:|---:|---:|
| 1 | 31.7 us | 105 us |
| 5 | 34.8 to 35.0 us (+10%) | 130 us |
| 10 | 36.2 to 41.8 us (+15 to 30%) | 162 us |
| 15 | 36.8 to 53.4 us (split into two populations) | **2634 us** |

Two conclusions, both load-bearing for the plan:

1. **Five concurrent workers is the safe width on this machine** (5 P-cores, one of which is the real-time thread, so the RT thread plus four peers). A 16-lane fan-out over five ways at the measured 10% penalty is about **4.5x** on the plugin part, not the 8x that a naive lane-count argument gives.
2. **Going wider without a real-time policy is catastrophic.** At 15 plain threads the medians split into P-core and E-core populations and the per-block maxima reach 2.6 ms, a whole deadline lost. A grep for `os_workgroup`, `thread_policy_set`, `THREAD_TIME_CONSTRAINT`, `pthread_setschedparam` and `qos_class` across tangent, tangent-ui, tangent-record, tangent-host and tangent-core returns **zero hits**. None of that plumbing exists. A fork-join built without it would be slower and far jitterier than the serial engine it replaced.

---

## 3. The ranked gap list

Verdict key: **CONFIRMED** = both verifiers upheld the claim. **DOUBTFUL** = one verifier refuted it (stated). **DEAD AS STATED** = both refuted it; the corrected substance is given.

| # | ID | The gap | Verified cost | Verdict | Fix |
|---|---|---|---|---|---|
| 1 | BW-1 / PH-1 | Everything renders serially on one callback thread through one shared scratch set | 981 us of a 2667 us deadline at 16 lanes x 3 commercial inserts; 1715 us heavy; one core of fifteen | CONFIRMED | Per-worker scratch + lane fan-out + serial reduction, then ahead-of-time render |
| 2 | MI-1 / MI-2 | No per-block or per-bay timing exists anywhere on any audio thread | Instrumentation measured at 38 ns/block, 0.0014% of deadline | CONFIRMED | `Instant` pair in `Renderer::render` and around the `run_rack` bay call, published as atomics |
| 3 | BU-4 | Tape and Drive run 4x oversampling as full-length FIR dots on zero-stuffed input through a split ring that cannot vectorise | Drive 88.3 us/block, Tape 62.0; the FIR alone is 62 to 95% of each unit | CONFIRMED | The repo's own `analog/filter.rs` polyphase oversampler does the identical 4x traffic in 14.18 us, 3.89x |
| 4 | BW-2 | While rolling, every lane with a rack and a clip anywhere runs its chain every block, muted lanes included | Mute saves only 12.7% (measured); 130 to 800 us/block recoverable at 16 lanes with 10 idle, depending on chain | CONFIRMED | `lane_loud`-keyed idle skip with a tail window; `channel_is_heard` skip behind a setting |
| 5 | BU-1 | The IR convolver does four full complex 1024-point FFTs plus the whole IR MAC in one callback per 512 input frames, and instances are phase-locked | 32 us fixed FFT floor per step on every IR bay whatever the IR length; 16 x 2 s IRs = 1267 us (48%) at 128 frames, 95% at 64 | CONFIRMED | Real-input FFT plus a twiddle table cuts the floor 3 to 4x; that floor IS the burst for any IR under 1.45 s |
| 6 | RT-3 | Seven Engine setters sleep-poll up to 250 ms; `load_insert` hits the same wait per seat on the offline engine | 12.0 s of frozen UI before an export's first sample at 48 seats (measured 250.68 ms each); 2 to 6 ms per UI frame during an automation drag | CONFIRMED | Skip the wait when `self.offline.is_some()`; park-and-retire like `set_clips` already does |
| 7 | PH-2 | VST3 insert automation never reaches the plugin: `changes.clear()` runs after the params loop | Zero CPU. Every plugin curve the owner has ever drawn is inert | CONFIRMED | Move the clear above the loop; add a plugin-free queue-fill test |
| 8 | BW-4 | A frozen lane stands down only seat `ch`; its second rack and seven pedal racks keep running, on a stem that already baked them | Zero in the 3-insert case; 40 to 150 us/lane with bays 4 to 6 filled. Primary defect is audible double-processing | CONFIRMED | Key the frozen test on the lane, not the seat; land both latency sites and the lead re-derivation together |
| 9 | MI-3 / MI-4 | On macOS the Audio Status panel's dropout field is structurally `None`; only the log carries the HAL count, which is device-scoped | Zero CPU | CONFIRMED | Merge `overload.count()` into the panel field, keeping the Option; bench must read the log and never run both hosts on the Scarlett |
| 10 | PH-3 | VST3 latency reaches PDC only via a poll gated on the first 108 bays having an editor open | 0.137 to 0.553 us/block for the existing atomics walk | CONFIRMED | Widen the arming gate to all seats; defer applying a latency change until the transport stops |
| 11 | BU-3 | Analog pays three libm calls per voice per sample | 56.0 ns per voice-sample, 4.3% of a core at 16 voices; 504 us at 64 voices (20.7% of deadline) | DOUBTFUL (cost lens refuted "medium") | Take the `tan` Maclaurin only (2%, all 63 analog tests pass). Do NOT hoist the two `exp2`: audio-rate FM and a 1 ms 6-octave contour would alias |
| 12 | RT-1 / RT-2 / RT-4 | Three allocations on the audio thread: arp `Vec::clone`, a refused-block `format!`, a monitor ring freed in the callback | 7 ns, 20 ns and 208 ns measured. All noise | DOUBTFUL (all three cost-refuted) | Hygiene. Two-line fixes, fold into whichever lane owns the file |
| 13 | TS-1 to TS-4 | The threading seam and eligibility maps | n/a, these are design inputs | THREE OF FOUR DEAD AS STATED | See 3.1 |
| 14 | BW-3 | `mix_clips` is O(frames x clips) with a whole-project unsorted list | 22.6 us/block at realistic sizes (0.85%); 87 us (3.3%) at 1920 loop-expanded clips | DEAD AS STATED (both) | Keep the MIDI bucketing and the `clip_lanes` cache (cheap, safe, 40 us at XL). Refuse the audio cursor: its twin already caused silent permanent note loss (`instrument.rs:9519-9530`) |
| 15 | BU-2 | DX7 "powf x7 + fmodf x6 per voice per sample" | They compile to `__exp10f`/`exp2f`, never `powf`; whole render is 66 to 117 us at 16 voices (2 to 4.4% of deadline), not 0.3 to 0.6 ms | DEAD AS STATED (both) | Take the one-line `fmodf` replacement (49% of the DX7 by one lens, 17.6 us by the other). The proposed `db_to_amp` lookup table buys 2.5 us and is not worth it |
| 16 | PH-4 | Per-bay buffer copies and zero-fills | 0.057 us/bay, 2.75 us for 48 bays, 0.10% of deadline | DOUBTFUL (cost refuted) | **Do not fix.** The proposed removal of the output pre-zero drops the only guard against another lane's audio leaking through a summing or silence-flagging plugin, for 2 us of 2667 |

### 3.0 What has landed since this was written (2026-09-19)

Gaps 2, 3, 4, 5, 6, 7, 8 and 10 are done. Gap 1 is done as far as the
fan-out goes; the look-ahead half of it is not being built, and Lane 6 in the
appendix says why. That leaves no CONFIRMED gap unaddressed.

**Gap 10, the latency poll, was three defects rather than one.** The host
worked out its whole plugin-window state over `(STRIPS + 1) * INSERTS` bays,
the first 108 seats, and used it for three things: gating `poll_latency`,
noticing a window close so the preset behind it is saved, and repainting
while a window is open, which on Linux IS the plugin GUI's run loop. Every
folder extra rack and all seven amp pedal seats of every lane fall outside
that window, so a plugin seated there could go out of PDC alignment after a
preset click, lose its state on close, and freeze on Linux. The engine now
answers `open_insert_editors()` over `RACK_SEATS`, and the host reads the
count before and after the poll rather than filling a 108-element array a
frame.

**Gap 5, the convolver, was worse than the row above says and is now the
clearest win in the sweep.** The row priced the fixed transform floor. It
did not price what the phase lock does to the distribution, which turned out
to be the larger half. Two changes landed: the two channels now ride through
one complex transform in each direction instead of two, with the twiddles
tabulated rather than rotated in the inner loop; and each instance starts its
staging buffer at its own offset so the desk's bays no longer all step on the
same block. Per step, measured: a one-partition response 36.3 to 17.0 us, a
four-partition response 37.2 to 17.2, a two-second response 83.3 to 66.4.

On a desk of 96 bays, thirty-two lanes carrying three half-second responses
each, at 128 frames, serial (`rt_bench`, this machine):

| | before | after |
|---|---|---|
| p50 | 121.5 us | 755.3 us |
| p99 | 5488.8 us | 1136.6 us |
| p99 as a share of the deadline | 205.8% | 42.6% |
| blocks over deadline, of 2000 | 500 | 0 |

The p50 rising and the p99 falling by 4.8x IS the phase lock being undone:
the same work, spread instead of stacked. Exactly one block in four was late
before. None is now. At 64 frames the same desk went from 387.9% to 49.9%.

**That rung also settles Lane 6.** Its acceptance gate (clause b) is a 96-bay
heavy desk under 30% of the deadline. Forked, the rung measures p99 390.3 us,
**14.6%** at 128 frames and 19.1% at 64. The gate the riskiest item in the
plan was written against is met without it.

Two smaller items landed with it. The render load meter kept its distribution
in octaves, so 524 and 1048 us were one reading and it could not score its own
gate; it keeps eighths of an octave now. And `FANOUT_PEERS` was a hardcoded 4,
right for this machine's five performance cores and wrong on every larger one;
the pool asks the machine now.

### 3.1 The threading-seam map is the part you cannot build from as written

This matters more than any single microsecond, so it gets its own paragraph. All four threading-seam findings were refuted at least once, and **three of the four by both verifiers**. Every refutation was the same species: the mechanism is real, the enumeration is incomplete, and building on it as written would ship a bug. The corrected facts a fan-out or a look-ahead must respect:

- **The lane body is not a pure function.** It also writes `dry_io` (consumed by the fold through its own delay line), and `run_rack` publishes `seat_gr`, the learn cells, the insert tap ring and `seat_latency` (which feeds `pdc_max` back into the same function). A pre-rendered lane's gain-reduction meter and insert scope would lead the audio.
- **A lane's rack is clocked more than once per chunk on the real-time thread.** A send destination's rack runs again in the sends pass (`instrument.rs:6472`, the code's own "SEND-BARGAIN" comment), a drum lane's channel seat is run by `mix_pad_lane` (`instrument.rs:8969-8987`) while the lane pass skips it, a folder's runs at the fold, a room-claimed one runs in the monitor pass. Those lanes cannot be worker-owned.
- **`room_rack` is one chunk LATE by construction.** It is written by `mix_monitor`, which runs *after* the lane pass that consumes it. That staleness is fine for a same-block fork-join and cannot authorise rendering block N+k at all.
- **`key_prev` does not exist for a future block.** A keyed lane reads the previous chunk's finished whole-desk stems.
- **Tempo is a live per-block input.** `set_tempo` writes `Shared::bpm` straight, and the audio thread reads it every chunk for the arp `Transport`, the click period, and the bus, master and room effect calls. "Tempo needs no invalidation trigger" was false.
- **Varispeed makes the chunk length itself unpredictable.** `render` sizes each chunk from a phase accumulator. No look-ahead while ratio is not 1.0.
- **The live set is not the armed set.** An open MIDI editor makes its clip's lane the sole live lane and deafens every armed lane; the patch-browser audition is ungated by arm entirely; and the punch is snapshotted at the record press, so an unguarded R press mid-take leaves a lane punched but not live. A worker does need the punch.
- **Invalidation storms are the unpriced cost.** A time-selection drag while rolling changes `loop_b` 60 times a second; a full discard of 200 ms of look-ahead across 16 lanes is 18 to 84 ms of CPU per discard, which at 60 Hz is 21 to 100% of the entire P-core cluster spent re-rendering audio nobody hears.

None of this kills the idea. It sets the contract, and it is why the plan puts anticipation last and behind a torture test.

---

## 4. The plan

Six lanes. File ownership does not overlap between lanes, and **no lane touches any file the themes lane owns** (`tangent-ui/src/theme.rs`, `scripts/palgen.py`, `scripts/velgen.py`, `tangent-ui/src/mixer_panel.rs`, `tangent-ui/src/piano.rs`, `tangent-ui/src/app.rs`, `docs/spec/ui-spec.md`, `docs/LANE-THEMES-2026-09-18.md`). Note in particular that **no lane opens `app.rs` at all.**

| Lane | Files | Starts | Why this order |
|---|---|---|---|
| 1 meter | `instrument.rs`, `desktop.rs`, `tangent-ui/recorder.rs`, `tangent-ui/dialogs.rs`, `rt_bench.rs`, `main.rs` | immediately | Nothing else can be scored without it |
| 2 resting-desk | `instrument.rs`, `tangent-ui/settings.rs` | after 1 | Shares `instrument.rs` with 1; the biggest measurable win before any threading |
| 3 kernels | `units/tape.rs`, `units/drive.rs`, `units/convolver.rs`, `analog/filter.rs`, `analog/arp.rs`, `dx7/synth.rs` | immediately, in parallel | Touches nothing any other lane owns |
| 4 vst3-automation | `tangent-host/instance.rs` | immediately, in parallel | Touches nothing any other lane owns |
| 5 parallel-lanes | `instrument.rs` | after 2 | The fan-out must be measured against the post-skip sum, not today's |
| 6 anticipative | `instrument.rs`, `transport.rs` | after 5 | Highest risk; three of four seam findings were refuted |

Lanes 1, 3 and 4 run concurrently on day one. Lane 3 alone recovers more measured microseconds than anything except the threading work, and it carries no cross-lane risk at all.

### 4.1 What REAPER parity means, as a number

**Interim gate, needs no REAPER (this is the one to build against).**

A rung of **96 instances of Arturia Rev PLATE-140** (32 lanes x 3 bays, the primary-seat grid full, 28.2 us measured per instance at real default DSP) at 128 frames / 48 kHz must run with **RT p99 under 1867 us, that is 70% of the 2666.7 us deadline.**

Today that rung is roughly 2707 us serial, 101% of the deadline: it fails. After lane 5 at the measured five-way scaling it should land near 600 us, 22%. It is falsifiable, it needs neither REAPER nor the interface, and it sits exactly at Tangent's own structural bay cap.

Two supporting gates from the existing bench table, same conditions:

| Shape | p99 today | p99 after lane 5 must be |
|---|---:|---:|
| 16 x [EQ, comp, plate] | 164.8 us | <= 80 us |
| 8 lanes x Analog, 64 voices | 550.8 us | <= 200 us |
| 96 x Rev PLATE-140 (new rung) | ~2707 us (projected serial) | <= 1867 us |

**Final gate, needs the device bench.** Run the harness already built at `scratchpad/perf/run-all.sh`, both hosts at 128 frames / 48 kHz on the Scarlett, never simultaneously, with the same plugin loaded from the **same saved non-default state** on both sides (Rev PLATE-140 or Comp FET-76, never Pro-Q 4 at its passthrough default), 60 s roll, zero underruns.

- **Parity on the real-time path (after lane 5):** with every REAPER track record-armed and input-monitored (which forces its anticipative path off), Tangent must sustain **within 10%** of REAPER's instance count.
- **Parity on the full engine (after lane 6):** with REAPER as shipped (anticipative, `workthreads=15`), Tangent's non-armed ladder must sustain **at least** what REAPER's armed ladder sustained, and the stretch target is parity with REAPER's default ladder.

### 4.2 Two things worth saying to the owner before any of this lands

1. **The Drive and Tape fix is the cheapest credibility win in the sweep.** One built-in insert of Drive costs more than twice FabFilter's heaviest plugin. The replacement oversampler is already in this repo, in `analog/filter.rs`, built from Drive's own tap constants. Lane 3 is a day's work with no cross-lane risk and it takes Drive from 88.3 to roughly 47 us and Tape from 62.0 to roughly 22 us.
2. **Fixing PH-2 will change the sound of saved projects.** Every plugin automation curve the owner has ever drawn on a VST3 insert has been inert. When the clear moves, those curves start playing. That is correct, and it must be announced rather than discovered.

---

## 5. What remains unverified, and how to verify it

| Unverified | Why it matters | How to settle it |
|---|---|---|
| **Every REAPER number.** No REAPER process was ever launched | The whole comparison rests on config-file reading and the article | Run `scratchpad/perf/run-all.sh` when the Scarlett is free. Step 1, the REAPER ladder, is fully unattended and takes ~7 min. Steps 2 to 4 need an agent to read menu coordinates off a screenshot for each Tangent project. Budget ~25 min |
| Tangent's real-time behaviour under an actual deadline | The bench ran on an ordinary thread. "0 blocks over deadline" is meaningless | The device bench, reading dropouts from `grep 'audio dropouts' ~/.config/tangent/tangent.log` segmented at the last "audio out: the callback is running" line. The panel reads nothing on macOS until lane 1 ships |
| Whether worker threads joined to the CoreAudio workgroup behave | The 5-thread 10% penalty was measured on PLAIN threads; at 15 the maxima hit 2.6 ms. No `os_workgroup` or thread-policy code exists anywhere in the workspace | Lane 5 must measure per-worker p99 with and without `os_workgroup_join` before the fan-out is enabled by default. Gate it behind a setting until that number exists |
| Real DSP cost of Pro-Q 4 and Brusfri | Both measured 0.2 us because their defaults are passthrough. The "16 x Pro-Q 4 = 35.3 us" row measures Tangent's hosting, not FabFilter | Save a state with bands enabled and re-run `baycost`; use that same saved state on both sides of the device bench |
| Plugin cost under live automation | PH-2 means no VST3 has ever received a parameter point. Every measured cost is static | Re-run `baycost` after lane 4 with a moving curve on a coefficient-bearing parameter |
| Whether `kAudioDeviceProcessorOverload` is cross-process | The claim that Tangent's counter sees other clients' overloads is `outpath.rs:441-443`'s own prose, with no test | Two clients on the device and a deliberate overload in one. Not worth a bench run: the "one host at a time" rule stands on buffer ownership and CPU contention regardless |
| The Tape/Drive projection (88.3 to ~47, 62.0 to ~22) | This is a **projection** from a measured 3.89x on the FIR alone, not an end-to-end measurement | Lane 3's acceptance bench measures the whole unit after the swap |
| The 4.5x fork-join projection | Derived from a measured 10% concurrency penalty at 5 threads plus a serial tail; the join latency and the E-core contribution are estimates | Lane 5's acceptance rungs measure it directly |
| Disk streaming and media buffering | Not covered anywhere in this sweep. `Placed` holds fully decoded audio, so nothing here touches read-ahead. REAPER's media buffering is a separate axis | A separate pass, with real projects on real media, not fixtures |
| The insert grid width (96 vs 192 bays) | Two scouts read it differently: `recorder.rs:1202-1218` maps a lane's visible slots 4 to 6 to a second seat, but the device-bench agent read 96 from the document format | Seat 6 bays on one lane in a saved document, reopen, and count |
| Whether the Analog `tan` swap is bit-safe in a release build | 63 analog tests passed in a scratchpad copy, not in the repo | Lane 3 runs the same suite in-tree, including the 15-cent self-oscillation gate and the laned/scalar bit-identity test |

One housekeeping note: the in-process bench left `tangent/src/rt_bench.rs` (393 lines, `#[cfg(all(test, feature = "recorder"))]`) and a 4-line `mod rt_bench;` in `tangent/src/main.rs` uncommitted in the main checkout. Lane 1 owns and adopts both. Nothing else in the tree was modified, and `~/.config/tangent/settings.json` was not touched by anyone.

---

## Appendix: the lanes as contracts

### Lane 1: meter

**Goal.** Give Tangent the measurement it does not have: per-block real-time load (last, max, mean, p99 histogram, deadline) and per-bay FX time, published from Shared and read back through Engine accessors, plus an honest dropout readout on macOS and a latency poll that covers every seat. Nothing else in the sweep can be scored, ranked or proven without this lane, and REAPER's Performance Meter shows exactly these three numbers (RT CPU, RT longest block, per-track FX CPU).

**Files.** `tangent/src/instrument.rs`, `tangent/src/desktop.rs`, `tangent/src/rt_bench.rs`, `tangent/src/main.rs`, `tangent-ui/src/recorder.rs`, `tangent-ui/src/dialogs.rs`

**Contract.** MAY: add cells to Shared (placed at the END of the struct or in a #[repr(align(128))] sub-struct, because a reader sharing a cache line with the writer measured a 2.7x penalty); add Engine accessors beside xruns() at instrument.rs:14096; take one Instant pair in Renderer::render (instrument.rs:6919, renamed to render_inner behind a thin timing wrapper, since it has four early returns and borrows &mut self throughout) and one around the process call in run_rack's bay loop (instrument.rs:4031-4040); merge overload.count() into the panel's dropouts field at desktop.rs:3459-3462 keeping the Option shape (Some,Some)=>max, (Some,None)=>Some, (None,Some)=>Some, (None,None)=>None so Linux's honest Some(0) is not turned into None; widen the poll-arming gate at desktop.rs:4333-4339 past the 108-bay EDITOR_BAYS window; adopt rt_bench.rs and the mod line in main.rs. MUST NOT: change any rendering arithmetic, any signal path, any buffer size, any ordering. Audio output must be bit-identical. MUST NOT open tangent-ui/src/app.rs, theme.rs, mixer_panel.rs or piano.rs. The histogram bucket MUST be ((63 - ns.max(1).leading_zeros()) as usize).min(31): the raw 63 - ns.leading_zeros() underflows on a 0 ns block, which the dev_ch == 0 and program == 0 early returns actually produce, and a panic across the cpal boundary is undefined behaviour by this file's own comment. A widened latency poll MUST defer APPLYING a moved figure until the transport is stopped (log it immediately), because DelayLine::run clears its whole ring on any length change (instrument.rs:1301-1332) and max_latency re-lengths every lane, so an unguarded poll turns each missed latency move into a click across the whole mix, possibly mid-take. PROVEN BY: (a) the full existing suite green, cargo test --release -p tangent; (b) a new test that renders offline and asserts rt_ns_max > 0, that seat_ns for an empty bay stays 0, that seat_ns for a filled bay is > 0, and that a bypassed bay stays 0; (c) a test asserting AudioStatus.dropouts is Some when the HAL count is Some and that merge_dropouts(Some(0), 0) still reaches the panel as Some(0); (d) rt_bench rerun and every shape's mean within 2% of the baseline table (16 lanes bare 21.3, 16x3 EQ 46.4, 16x[EQ,comp,plate] 140.9, 8x Analog 504.5 us).

**Expected gain.** Zero microseconds of audio time by design. Measured instrumentation cost on this M5 Pro: Instant::now 14.2 ns, the five relaxed atomics 3.9 ns, the whole hook 38.0 ns per block = 0.0014% of the 2666.7 us deadline at 128 frames, rising to 99.5 ns (0.0037%) under pathological reader contention. Per-bay hook measured at 9 to 33 ns, so 48 filled bays cost 0.45 to 1.6 us per block, 0.02 to 0.06% of deadline, and exactly zero for empty and bypassed bays. Storage: 4 AtomicU64 + a 32-bucket AtomicU32 histogram (~160 B) plus RACK_SEATS*INSERTS AtomicU64 (13,176 cells, 103 KB) allocated once in Shared::new like seat_gr. The gain is not CPU, it is that lanes 2, 3, 5 and 6 acquire a pass/fail number and the owner acquires the readout REAPER has had for twenty years.

**Risk.** LOW. Real-time hazards: none in kind (Instant::now on Apple is a commpage read with no syscall, allocation or lock, and the callback already reads the clock once per block at instrument.rs:15514; the atomics are lock-free on aarch64). Three implementation traps, all named in the contract: the 0 ns bucket underflow; cache-line contention if the cells are spliced in beside xruns; and rt_ns_max as swap-on-read makes the peak single-consumer, so a FrameLog line and an on-screen meter would steal each other's peak (keep a second non-clearing cell for the UI). Portability trap: the 14 ns figure is Apple-only. On Windows/ASIO Instant::now is QueryPerformanceCounter and on FreeBSD clock_gettime, either of which can trap depending on hypervisor or kern.timecounter.hardware; at 1 us per read 96 bays would cost 192 us/block, 7% of deadline. Probe the timer once at startup or gate the per-bay hook behind a flag before it ships on those targets. The latency-poll half is the only part that can be heard, and the transport gate in the contract is what removes that.

### Lane 2: resting-desk

**Goal.** Stop paying for silence. Skip a lane's chain when it is provably making no sound and nothing is driving it; let a muted lane come off the peak block behind a setting; stop a frozen lane leaking its second rack and seven pedal racks (which are also being applied a second time on top of a stem that already baked them); and remove the UI-thread sleep-polls, whose worst case is twelve seconds of frozen window before an export renders its first sample.

**Files.** `tangent/src/instrument.rs`, `tangent-ui/src/settings.rs`

**Depends on.** meter

**Contract.** MAY: add a per-block clip_here mask beside clip_lanes in mix_clips (instrument.rs:8419-8460); add an idle term to the lane skip at instrument.rs:9872 keyed on measured pre-fader loudness (lane_loud, already computed at 10305 and today read only by park_lane) with a per-lane tail counter reusing lane_quiet; add a muted-lane skip keyed on channel_is_heard behind a new Settings field defaulting to OFF; key the frozen stand-down on the LANE rather than the seat by testing shared.frozen against ch in run_extra_rack (instrument.rs:3768-3812) and run_pedalboard (10451), and apply the same to the folder path at 10610-10611 and 10665-10690; skip every RETIRE_TIMEOUT deadline loop when self.offline.is_some() (instrument.rs:12778, 14197, 14298, 14332, 14352, 14876, 14927) since the offline renderer swaps on its first render_offline anyway; deepen pad_to_audio from 2 to CHANNELS (instrument.rs:11749) so an export with three or more drum lanes stops silently losing kits; give the displaced monitor Consumer a retiring ring instead of dropping it in the callback (instrument.rs:7134-7156); add a per-bay last-value memo in resolve_automation (instrument.rs:8055-8069) so a flat curve stops re-sending every block. MUST NOT: skip a lane whose measured pre-fader output is above PARK_FLOOR; skip a lane inside its tail window; skip a lane that is a sidechain key source for any other lane; skip a lane with a live automation curve on either of its seats (automation is delivered only inside run_rack, so a skipped block drops that block's ParamPoints and a lane waking under a moving curve jumps instead of ramps); skip a lane the room has claimed or that is armed or auditioning. MUST NOT open desktop.rs, dialogs.rs, tangent-ui/recorder.rs (lane 1 owns those) or any themes-lane file. The frozen change MUST land the run guard, BOTH latency sites (the per-lane PDC wait at 10168-10190 and the published figure at 8197-8215) and a re-derivation of offline_stem_lead (instrument.rs:11483, consumed at desktop.rs:9504) in ONE commit, or a frozen lane drifts against the mix by its bays-4-to-6 plus pedal latency, which is far more audible than the microseconds saved. PROVEN BY: (a) four new rt_bench rows: 16x3 EQ with 10 of 16 lanes off the playhead must fall to within 15% of the parked figure (11.9 us); the muted row must fall from 40.5 to within 15% of 20.4 us when the setting is on; a frozen-with-bays-4-to-6 row must equal the bare-lane figure; an export-setup row must complete Engine::offline plus 48 load_insert calls in under 1 s (today: 12.0 s); (b) a test that freezes lane 1 with a counting built-in in bay 4 and asserts zero process calls AND Rack::latency(folder_extra_seat(1)) == 0 AND pedal_latency(1) == 0; (c) a tail test: a lane holding a plate reverb whose clip ends must still decay to silence, with no truncation, over 8 s; (d) a sidechain test: a ducker keyed from a lane that is itself silent must still duck; (e) a null test: with the mute setting OFF, output is bit-identical to today.

**Expected gain.** Measured, not projected: muting all 16 lanes today saves only 12.7% (46.4 to 40.5 us), and plugins cost the same on silence (Pro-R 2 was 4% MORE expensive on silence, 36.39 vs 34.93 us; across the set silence is 82 to 104% of signal cost), so the skipped time is real. With 10 of 16 lanes idle the recoverable slice is about 60% of the block's insert time, which measures 130 us for clean EQ/comp chains, 392 us for analogue chains and 803 us for reverb/tape-delay chains, out of a 2666.7 us deadline. The muted-lane term is the one that also removes those lanes from the PEAK block rather than only the average. Frozen: measured 56% today for the primary seat, and this lane closes the 40 to 150 us per lane per block still leaking from bays 4 to 6 and the pedalboard. Offline setup: 12.0 s of frozen UI (48 seats x 250.68 ms measured) to approximately zero, plus 500 ms from maps and automation.

**Risk.** MEDIUM, and the risk is audible rather than computational. (1) Unmuting into a chain that has been skipped no longer lands in a live reverb tail or a settled compressor envelope. That is why the muted skip is a setting defaulting to OFF; REAPER keeps the same behaviour as a non-default option for the same reason. (2) A lane playing an amp sim into a compressor sits three orders of magnitude above PARK_FLOOR (1e-4) even on silence, per the code's own comment at instrument.rs:9822-9830, so the saving is smallest exactly where the per-lane cost is highest. State that in the acceptance rather than discovering it. (3) The frozen fix changes the sound of every already-frozen six-bay lane the moment a project opens, because those bays have been applied twice. That is the correct sound and must be announced, not slipped in. (4) The latency hazard in the frozen fix is the one that can ruin a mix silently; the one-commit rule in the contract exists for it. (5) Nothing here adds real-time work: the skip is a mask and a comparison, the offline skip is a removed wait, and the monitor retiring ring is one slots()-checked push that degrades to today's in-place drop when full.

### Lane 3: kernels

**Goal.** Make Tangent's own DSP cost what comparable commercial DSP costs. Drive at 88.3 us and Tape at 62.0 us per 128-frame block are the two most expensive inserts measured anywhere on this machine, above FabFilter Pro-R 2 at 40.1 us, and the fix is a faster oversampler that already exists in this repository. Plus the IR convolver's fixed FFT floor, the DX7's fmodf and the Analog's tan, all free.

**Files.** `tangent/src/units/tape.rs`, `tangent/src/units/drive.rs`, `tangent/src/units/convolver.rs`, `tangent/src/analog/filter.rs`, `tangent/src/analog/arp.rs`, `tangent/src/dx7/synth.rs`

**Contract.** MAY: replace the split-ring scalar Fir in tape.rs (496-506) and drive.rs (201-211) with the polyphase even/odd + History double-write + 8-accumulator fma dot already in analog/filter.rs (489-499, 576-598, 633-660, 745-790), generalising analog's Taps/History over tap counts so the tape's 79/21 fit beside the current hardcoded 32/16/64/32; replace the convolver's four full complex 1024-point transforms (convolver.rs:141-192, 329, 366) with a real-input transform plus a precomputed twiddle table; replace fast_sin's `x % TAU` in dx7/synth.rs:788 with `x - TAU * (x * (1/TAU)).round()`; replace tpt_g's tan in analog/filter.rs:69-72 with an odd Maclaurin to x^7 (the argument is bounded to [0.00033, 0.327] at 4x, so the .min(1.4) guard never fires); replace arp.rs:269's `self.down.clone()` with `self.pattern.clear(); self.pattern.extend_from_slice(&self.down);`. MUST NOT: change any unit's reported latency (tape OS_LATENCY 44, convolver latency() exactly 512 and constant, analog OVERSAMPLER_LATENCY 36); change any dial range, default or preset; tabulate clip_adaa's ln_cosh (ADAA divides a difference of two near-equal antiderivatives by dx, so a table's interpolation error does not cancel and is then multiplied by 1/dx, largest exactly where the signal is quiet: it would re-admit the aliasing ADAA exists to remove, for 10 us); hoist Analog's two per-sample exp2 to control rate (the mod bus carries oscillator three at AUDIO rate into pitch at up to 12 semitones and into cutoff at up to 4 octaves, and the filter contour sweeps 6 octaves in its 1 ms minimum attack, so a 1-in-8 update aliases every FM patch and zippers every percussive filter envelope, for a measured 7 ns per voice-sample); hoist the Analog pitch-EG tick (it is dB per SAMPLE, so block-rate ticking makes every pitch envelope 128x slower). MUST NOT open instrument.rs, desktop.rs, instance.rs or any themes-lane file. A re-baked DX7 patch hash (synth.rs:1482) is permitted ONLY with the measured maximum sample delta recorded in the commit message. PROVEN BY: (a) cargo test --release -p tangent green for units::, analog:: and dx7::, specifically including a_three_tap_ir_round_trips_exactly_through_any_chunking (convolver.rs:695-722, bit-identical across chunk sizes 1/100/513/4096), the_ladder_self_oscillates_at_the_dials_pitch (filter.rs:835-853, 15-cent gate), the_laned_frame_renders_the_scalar_frame_bit_for_bit (analog/mod.rs:1482), and the impulse-peak latency assert at filter.rs:983; (b) a NEW impulse test per changed unit asserting the reported latency equals the measured peak offset, ported from filter.rs:983 before the oversampler swap lands; (c) a new ignored per-unit bench printing us per 128-frame block, asserting Drive < 55 us, Tape < 30 us, and the convolver's n_part=1 step floor < 12 us; (d) an allocation-counting test around Arp::run in arp mode with keys held, asserting zero allocations.

**Expected gain.** Measured components, projected totals. The analog polyphase oversampler does the identical 4x traffic in 14.18 us against the split ring's 55.10 us: 3.89x, from 1.7x fewer MACs (256 padded vs 436) and 2.3x better per MAC (0.22 vs 0.49 ns). Since the FIR pair is 62% of Drive and 88 to 95% of Tape, the projections are Drive 88.3 to about 47 us and Tape 62.0 to about 22 us (57.3 to about 17 at rest). At the brief's 16 lanes x 3 inserts that is 4.24 ms to 2.26 ms for an all-Drive desk, which is the difference between a certain dropout and a working session. Convolver: the measured 32.2 us fixed floor is four scalar complex transforms paid identically by a 20 ms cab IR and a 20 s cathedral, and it IS the whole burst for any IR under 1.45 s; a real-input FFT plus twiddle table should cut it 3 to 4x on every IR bay in the desk, which at the measured phase-locked stack (16 x 2 s IRs = 1267 us, 48% of deadline at 128 frames and 95% at 64) is the difference between a glitch and headroom. DX7: measured 17.6 us of a 112 to 117 us render at 16 voices from the fmodf alone (49% by the source lens's measurement, 40% total with all three swaps), one line. Analog: about 2%, one line, all 63 tests passing in a scratchpad build. Arp: 7 ns per 32-frame chunk and one categorical RT-safety violation removed.

**Risk.** MEDIUM, concentrated in one place: latency. The analog decimator keeps the earlier of each pair (OVERSAMPLER_LATENCY = 36, pinned by an impulse-peak test) while the tape derives OS_LATENCY = 44 from its own tap counts and parks its play head at BASE_MS minus that figure. One sample of phase slip silently mis-reports latency_frames(), PDC then mis-aligns the lane, and it reads as a flam rather than as a bug. Port the impulse-peak test FIRST, before the swap. Second risk: the convolver's chunk-size bit-exactness contract is absolute (the test demands identical output at chunk sizes 1, 100, 513 and 4096), so nothing may be keyed to the host block, which the kernel never sees anyway (run_planar re-chunks to 512 and varispeed leaves 217/218-frame remainders). Third: the drive is a literal reuse of the analog filter (same constants, same windowed_sinc, and analog/filter.rs already imports them from units::drive), but the tape is not, and the Taps/History generalisation is the only real engineering in the lane. Fourth: output changes in the last bits from fma contraction and a different summation order; nothing in either unit's tests asserts bit-exactness (they assert dB levels, THD monotonicity and alias floors) so they should survive, but the DX7 hash test will not and must be re-baked deliberately. No real-time hazards: fixed arrays, const construction, no allocation, no locks, about 2 to 3 KB more state per instance.

### Lane 4: vst3-automation

**Goal.** Make VST3 insert automation reach the plugin. process_through pushes this block's ParamPoints into the parameter-change pool and then calls changes.clear(), which zeroes the pool's used count before process() runs, so every automation curve the owner has ever drawn on a hosted insert has been inert, and any post-clear CC or editor edit reclaims and erases the queue the points were written into.

**Files.** `tangent-host/src/instance.rs`

**Contract.** MAY: move self.changes.clear() (instance.rs:1598) above the automation loop at 1581-1586, preserving the documented ordering automation -> editor edits -> controls so a curve and a CC on one id share a queue in time order; extract the queue filling into a plugin-free helper (fn fill_changes(&mut ParamChanges, params, controls, handler)) so it can be unit-tested without a real plugin; add the missing params path for VST3 INSTRUMENT bays, whose Vst3 arm calls process_with_controls with an empty params slice (instance.rs:1506) while process_with_automation (1514-1523) has no caller in the workspace at all. MUST NOT: move self.scratch.events.clear() (instance.rs:1588), which is already correctly placed before the only loop that fills that list; introduce any allocation, lock, syscall or new panic surface on the process path; change the legacy CC fallback at 1630-1641 (a mapped control pushes its CC event unconditionally, so a starved queue pool degrades the sustain pedal to the legacy road rather than silencing it); change any file outside tangent-host/src/instance.rs, in particular NOT tangent/src/instrument.rs (lanes 1, 2, 5 and 6 own it) and NOT the Result<_, String> error type on Backend::process_voice/process_effect, whose LV2 arms are cfg(linux/freebsd) and cannot be compiled or tested on this machine. The per-bay changed-value memo in resolve_automation belongs to lane 2, not here. PROVEN BY: (a) a new unit test on the extracted helper asserting getParameterCount and getPoint return the automation points after the call, and that a CC on the same id lands after the curve point rather than erasing it; (b) a test that a mock processor returning kResultFalse renders 1000 blocks with zero allocations, using the while_counting harness already at instance.rs:3327; (c) cargo test --release -p tangent-host green; (d) a manual acceptance the owner performs once: draw a curve on a Pro-Q 4 band and hear it move.

**Expected gain.** Zero microseconds saved; this is the correctness half of the plugin path and it should be scored ZERO in the REAPER CPU-parity ledger. The waste today is 0.2 to 0.7 us per block in a 16-lane, 3-insert, 2-curves-per-bay session, under 0.03% of the deadline. Going forward the fix ADDS low single-digit microseconds per block (each delivered point costs the plugin a getParameterData/getPointCount/getPoint round trip plus whatever it does on a parameter change; a contrived 48-bay, 6-parameter worst case reaches about 1% of a 128-frame block), which lane 2's changed-value memo in resolve_automation cancels for flat curves. The gain is a feature the product advertises and does not have: insert_parameters publishes every VST3 parameter into the lane automation menu, so the owner can draw a curve on any Pro-Q 4 or Saturn 2 control and nothing happens.

**Risk.** LOW computationally, MEDIUM in perceived behaviour. Two changes the owner will notice and that must be announced rather than discovered. First, every saved project already carrying plugin curves changes how it sounds the moment this ships, because those curves have been inert. Second, an automated parameter will now fight the plugin's own editor: within a block the editor edit lands after the automation point and wins, and the next block the curve reasserts, so a knob turned in Pro-Q's window snaps back. There is no suspend for a plugin-editor gesture (the suspend bitmask is per lane and driven by desk fader holds), so this is standard read-mode behaviour arriving in an app whose UI currently implies the curve is drawn rather than performed. Worth a deliberate product decision. Queue-pool exhaustion needs more than 32 distinct automated parameters on ONE bay (MAX_PARAM_QUEUES = 32), which Pro-Q 4 makes possible but unlikely, and it degrades gracefully. This lane is fully parallel with every other lane: it touches exactly one file that nobody else opens.

### Lane 5: parallel-lanes

**Goal.** Stage one of the threading work: render independent lanes concurrently INSIDE the callback. Give each worker its own scratch set, make the lane body a job over (lane notes, lane controls, clip_lane column, automation rows, key_prev, shared atomics) writing only that lane's racks, PDC line, instruments and a private pre-fader stem, and turn the fold into a serial reduction on the real-time thread. This is where the measured 981 us of a 2666.7 us deadline at 16 lanes x 3 commercial inserts becomes about 250 us.

**Files.** `tangent/src/instrument.rs`

**Depends on.** resting-desk

**Contract.** MAY: privatise the shared scratch set per worker (fx_in and fx_out at instrument.rs:11899-11900 are the obvious pair, but the FULL list is fx_in, fx_out, key_io, dry_io, builtin_scratch, keys_scratch, send_io, pad_io and the single lane_notes/lane_controls Vec pair, plus disjoint &mut access to racks since run_extra_rack and run_pedalboard borrow &mut self.racks whole); split the note-list build (instrument.rs:9419-9580, which mutates active_clip, sounding and midi_cursor) into a serial first pass; give each lane a private post-rack pre-fader stem and move the fold (10220-10300) into a serial reduction; give each worker its own partial mix and aux accumulator, since mix and aux are shared += accumulators today (10278-10285); assign lanes in contiguous runs of 8 because stem_scratch is interleaved BY CHANNEL (stem[f*CHANNELS*2 + ch*2] += l), so one 64-byte line holds 8 lanes' stereo pair for one frame and scattered ownership ping-pongs it 128 times per block. MUST NOT: give a worker a lane that is armed, room-claimed, auditioning, punched, a folder, a send destination (its rack runs a SECOND time in the sends pass at instrument.rs:6472), a drum lane (mix_pad_lane clocks the channel seat again at 8969-8987 while the lane pass skips it), the Fx bus or either master seat. MUST NOT let the real-time thread block past a deadline on a worker: a late lane is COUNTED in a worker_late atomic beside xruns, never silently dropped and never spun on for hundreds of microseconds (300 us is 11% of the budget per late lane). MUST size the pool to the PERFORMANCE cores and MUST join every worker to the device's audio workgroup (kAudioDevicePropertyIOThreadOSWorkgroup, os_workgroup_join) and set a time-constraint thread policy: none of that code exists anywhere in the workspace today, and at 15 plain threads the measured per-block maxima reached 2634 us, a whole deadline. MUST call flush_denormals per worker per block: it is x86-only and per-thread, so a worker on the Windows tester would run without FTZ/DAZ and every test on this Mac would still pass. Keep swap_plugins before the fork (it already runs once at the head of render_program, instrument.rs:5660). Keep folds, sends, buses, master and taps serial after the join. MUST NOT open desktop.rs, settings.rs, dialogs.rs, tangent-ui/recorder.rs, transport.rs, any unit or analog file, or any themes-lane file. PROVEN BY: (a) the parity gate, a NEW rt_bench rung of 96 instances of Arturia Rev PLATE-140 (32 lanes x 3 bays, 28.2 us measured per instance at real default DSP) at 128 frames: p99 must be under 1867 us, 70% of the 2666.7 us deadline, against roughly 2707 us serial today; (b) the existing rungs, 16 x [EQ, comp, plate] p99 from 164.8 to at most 80 us, and 8 lanes x Analog 64 voices p99 from 550.8 to at most 200 us; (c) a worker_late counter reading exactly 0 over 2000 blocks at each rung; (d) a null test rendering the same project through the serial and parallel paths with a difference below -120 dBFS (bit-identity is NOT required, since per-worker partial sums change float association, but the change must be inaudible); (e) a per-worker p99 measured with and without the workgroup join, recorded in the commit, because the fan-out must not ship on the unjoined number.

**Expected gain.** Measured concurrency penalty on this machine: Pro-R 2 costs 31.7 us on one thread and 34.8 to 35.0 us per instance across 5 threads, a 10% penalty. With 5 P-cores, one of which is the real-time thread, 16 lanes fan out over the RT thread plus 4 peers, so the plugin part divides by about 5 at a 10% cost, plus join latency (condvar wake measured at 5.25 us mean, 19.8 us worst) and the unchanged serial tail. Arithmetic on the measured figures: 16 x [EQ, comp, plate] goes from 140.9 to about 68 us mean; a 48-bay commercial desk from 981 us (36.8% of deadline) to about 250 us (9.4%); the 96-bay PLATE-140 gate from roughly 2707 us (101%, a dropout) to about 600 us (22%). That is the single largest recoverable number in the sweep, and it is worth about 4 to 5x, NOT the 8 to 20x a lane-count argument suggests. It should be computed against the post-lane-2 sum, not today's, because most of the serial time today is spent on lanes making no sound.

**Risk.** HIGH, and the highest-consequence risk is one line of missing plumbing. Without the audio workgroup join and a real-time policy the parallel path is measurably WORSE than the serial engine it replaces: the 15-thread probe showed medians splitting into P-core and E-core populations (37 vs 52 us) with per-block maxima of 380 to 2634 us, so a fork-join defined by its slowest worker would lose whole blocks. That is why the contract makes the join mandatory and the acceptance requires the with/without number. Second risk, the privatisation list: three verifiers independently found scratch buffers the original map omitted (key_io, dry_io, builtin_scratch, keys_scratch, send_io, pad_io, lane_notes/lane_controls) plus the mix and aux += accumulators; missing any one is a data race, not a tuning question. Third, the eligibility set: the original map's rule admitted drum lanes, whose channel seat mix_pad_lane clocks a second time, which is exactly the double-clocking of a stateful chain that instrument.rs:10010-10038 exists to forbid. Fourth, keyed lanes must be ordered after their key source. Fifth, the master bus summation ORDER changes (float addition is not associative), so any bit-exact render regression test flips; that is inaudible and expected, and the -120 dBFS null test is what proves it. No behaviour the owner can hear changes if the contract is met: mute, solo, fader, pan and send are all applied by the serial fold after the join, so they stay instant.

### Lane 6: anticipative

**DECISION 2026-09-19: NOT BUILT, and the gate it was written against is
already met by the lane before it.** Four independent design reads of the
ownership, invalidation and consumption protocol were run before any of it
was written. All four returned `needs-terms`, and three of them found
crash-or-race breaks in the protocol as specified, not in a hypothetical
implementation of it:

- The RT thread cannot both wait for an acknowledgement before touching a
  lane's seats and fall back to rendering that lane itself when the
  acknowledgement has not arrived. The second rule is the exact aliasing the
  first forbids, and a worker that is merely descheduled is inside
  `render_lane` on that lane's `Rack` while the callback starts its own.
- `swap_plugins` walks every rack at the head of every callback and the
  arrival signal is one desk-wide counter with no lane in it. Dropping a
  plugin onto a track that is playing would free a VST3 instance while a
  worker is inside its `process`.
- The dx7, Keys, sampler and pending-voice handoffs all `mem::replace` a
  lane's seats from the RT thread and discover the lane only after popping.
- `DeskRef` is not stable across blocks: `auto_params` is cleared and pushed
  into every block, so a worker holding one past its own block reads a `Vec`
  whose length is being reset underneath it.
- A lane's clip audio and its MIDI have no future to read. `clip_lane` is one
  block deep for the whole desk, and the `sounding` ledger is a single
  pitch-keyed `u128`, so neither can be given a future one lane at a time.

Each is fixable, and together they are a sixteen-term invalidation contract,
an epoch handshake, position-keyed rings, a partial-span loop-edge rule and a
10,000-block randomised torture test. The fourth read priced that against
what it buys and the arithmetic does not support building it:

- **The acceptance gate above (clause b) is 96 x PLATE-140 with 90 bays on
  non-armed lanes at under 30% of the deadline. Section 4.1 projects LANE 5,
  which has landed, at about 22% on the same rung.** The gate written for the
  riskiest item in the plan is satisfied by the item before it.
- In the mixing case this feature is sold on, a 24-lane desk at three
  commercial bays is about 19% of the deadline after the fork. Anticipation
  takes it to roughly 7%. That is 12 points of a deadline that is already
  four-fifths empty, bought in the one session where the owner rides plugin
  knobs continuously, which is the session that invalidates the ring 60 times
  a second. One lane with six Pro-R bays under a knob drag is 116 ms of CPU a
  second re-rendering audio nobody hears.
- In the tracking case it buys close to nothing, as the goal above already
  concedes, and that is where a knob on a pre-rendered lane lands 10 to 21 ms
  late. The MIDI fast lane exists to kill a 3 to 10 ms delay.
- It would be macOS-only. Off macOS there is no workgroup, `RtReady` never
  rises and the engine renders serially. Tangent ships on Windows, Linux and
  FreeBSD.

**What was done instead**, in the order the read ranked them: the load
meter's resolution (it could not tell 600 us from 800 us, so it could not
score its own gate), the pool width (hardcoded to this machine's five
performance cores), the convolver's transform cost and its step phase, and a
bench rung heavy enough to score the fork. See the 2026-09-19 entries in the
CHANGELOG. `pinned_lanes`, the pure-logic half of this lane, was written and
tested and then reverted; the patch is not in the tree because the
invalidation read found the rule itself incomplete, in that the room's rack
claim outlives a disarm by about 138 ms while the rule drops the lane the
instant it is disarmed.

**If it is ever revisited**, it is a setting, default off, and the changelog
has to say three things plainly: a knob on a pre-rendered track lands up to a
look-ahead late, arming mid-roll can click, and every seek and loop wrap pays
its first block on the real-time thread.

**Goal.** Stage two: render playback-only lanes N blocks AHEAD of the callback into per-lane sample-addressed rings, which is the mechanism REAPER's 15 worker threads actually use. This is what takes non-armed lanes off the per-block deadline entirely and turns their cost into average throughput over all cores, rather than merely dividing it by five.

**Files.** `tangent/src/instrument.rs`, `tangent/src/transport.rs`

**Depends on.** parallel-lanes

**Contract.** THE PINNED (real-time) SET is the union of: lanes whose midi_off bit is CLEAR; lanes in Shared.armed; lanes with a live audition slot (Shared.audition is read for EVERY lane every chunk with no arm gate, and the patch browser names an arbitrary strip); lanes the punch still names (rec.midi/rec.audio are snapshotted ONCE at the record press and never rewritten, and nothing guards the arm toggle mid-take, so a disarmed lane can stay punched); drum lanes; folders; send destinations; sidechain-keyed lanes; the Fx bus; and both master seats. It is NOT the armed set: an open MIDI editor makes its own clip's lane the sole live lane and deafens every armed lane. MUST NOT look ahead at all while the varispeed ratio is not 1.0, because render sizes each chunk from a phase accumulator and the chunk length itself is then unpredictable. MUST invalidate on the FULL set, which is larger than the original map's: the transport generation (seek), the rolling falling edge, loop_a/loop_b, a clip-list swap, a MIDI-list swap, an automation swap, a plugin swap on either of the lane's seats, insert_bypass / frozen / insert_key word changes, ui_params arrivals, keys_patch and keys_params stores, pending_voices patches, AND, missing from the original map and each independently sufficient to serve wrong audio: a TEMPO change (Shared::bpm is read live every chunk by run_rack's arp Transport, the click period and the bus, master and room effect calls, and set_tempo writes it with no clip re-push), a tempo/meter MAPS swap, a click-sample swap, a Keys INSTANCE swap, an auto_suspend change, and a loop wrap. MUST key the ring by SAMPLE POSITION, not block index. MUST NOT reuse room_rack's one-chunk-late mask as a cross-thread handoff: it is an ordering between two passes on the SAME thread and carries no proof a worker has stopped touching that Rack; ownership needs an epoch plus an acknowledgement, and the RT thread may never wait for it. MUST invalidate only the span past a moved loop edge, not the whole ring: loop_b changes 60 times a second during a time-selection drag, and a full discard across 16 lanes is 18 to 84 ms of CPU per discard, which at 60 Hz consumes 21 to 100% of the entire P-core cluster re-rendering audio nobody hears. MUST re-sample the GR meters, insert tap and learn snapshots at CONSUMPTION, since run_rack publishes them and a pre-rendered lane's would lead the audio. MUST NOT open desktop.rs, settings.rs, any unit or analog file, tangent-host, or any themes-lane file. PROVEN BY: (a) a torture test of 10000 blocks with a randomised event stream (seeks, loop wraps, tempo changes, arm toggles, knob turns, plugin swaps, varispeed engagement) whose output matches the serial reference to within -120 dBFS and in which the frame index of every consumed ring block is asserted to equal the block being rendered, so a stale frame can never be served silently; (b) the 96 x Rev PLATE-140 gate with 90 of the 96 bays on non-armed lanes: p99 must fall below 30% of the 2666.7 us deadline; (c) an arm-during-roll test asserting the lane is back on the real-time path within one block and the discontinuity is bounded and counted; (d) the device-bench parity gate: with REAPER as shipped (anticipative, workthreads=15) on the ladder at 128 frames / 48 kHz with the same saved plugin state on both sides, Tangent's non-armed ladder must sustain at least the instance count REAPER sustains on its ARMED ladder, with zero underruns over 60 s, and the stretch target is parity with REAPER's default ladder.

**Expected gain.** Structural rather than arithmetic: it removes non-armed lanes from the per-block deadline entirely, which is the only mechanism that closes the remaining distance to REAPER's shipped configuration. Stage one divides the serial sum by about 5; stage two makes the per-block figure the PINNED set only. In a mixing session, where the owner's own usage is 0 to 2 armed lanes, that is nearly the whole desk. In a tracking session with 8 to 18 armed inputs on the Scarlett it buys close to nothing, and stage one is what carries that case; state that in the acceptance so the win is not oversold. No measurement supports a specific multiplier here, because no look-ahead exists to measure; the gate is the 30%-of-deadline rung and the device-bench parity number, not a projection.

**Risk.** HIGHEST IN THE PLAN, and the evidence says so directly: three of the four threading-seam findings were refuted by BOTH verifiers, every one of them for an incomplete enumeration that would have shipped a bug (room_rack cannot authorise a future block; key_prev does not exist for one; tempo is a live per-block input; varispeed makes chunk lengths unpredictable; the punch and the MIDI-editor rule mean the live set is not the armed set). Build nothing here until lane 1's meter can score it and lane 5's fan-out is green, and derive the eligibility mask from the corrected rule in the contract rather than from the original map. Behaviour changes the owner WILL notice, all of which REAPER also has and which must be accepted deliberately: a knob turned on a pre-rendered lane lands up to a look-ahead late or forces a re-render spike at the moment of touch; arming mid-roll discards blocks a stateful chain has already consumed, so a compressor envelope or reverb tail belongs to audio nobody heard and a click is possible; the first live note after arming can land a look-ahead late (8 chunks at 128 frames is 21 ms, 43 ms at 256), which is the same perceptual territory as the 3 to 10 ms delay the MIDI fast lane was built to kill; every loop wrap and seek re-renders its first block on the real-time thread. Memory: 200 ms of look-ahead is about 75 KB per lane and 2.46 MB for 32 lanes, which is nothing. The handoff machinery itself measured 0.31 us per block for 16 lanes, 0.012% of the deadline, so none of the risk is in the bookkeeping.
