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

Instruments, same block: Mini V4 47.2, Piano V3 134.8, Jup-8 V4 140.2, **the modelled piano 316.1 us (11.9% of a whole 128-frame block, per instance)**.

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
| Plus one the modelled piano lane | +316 us | +11.9% |

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
fan-out goes. The look-ahead half of it is not being built, and Lane 6 in the
appendix says why, but "not being built" is not the same as "addressed": gap
1's stated fix names both halves, so it should be read as half done.

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

**That rung does NOT settle Lane 6. Corrected 2026-09-19, same day.** It was
written here that it did, on a p99 of 390.3 us, 14.6% at 128 frames. That was
the wrong rung, and the claim is withdrawn.

A convolver bay is BURSTY: it costs nothing for three blocks and everything
on the fourth, and `CONV_SPREAD` levels those bursts across the desk. Lane 6's
gate names 96 instances of Rev PLATE-140, which costs the same amount on every
block, so its peak IS its mean and there is no phase to stagger. Per bay per
block the two rungs differ by about 3.8x, and the single largest contributor
to the 14.6% was a lever no hosted plugin has.

**The gate's own rung has now been run**, with that plugin, at that layout,
32 lanes x 3 bays (`rt_bench.rs`, the `GATE_PLUGIN` block):

| 96 x Rev PLATE-140, 128 frames | serial | forked |
|---|---|---|
| mean | 4657.1 us | 1031.3 us |
| p99 | 6393.4 us | 1338.0 us |
| p99 as a share of the deadline | 239.8% | **50.2%** |
| blocks over deadline, of 2000 | 2000 | 1 |

At 64 frames, forked: p99 997.3 us, **74.8%**, 6 blocks over.

**The gate is p99 under 30%. The fork measures 50.2%. It is not met.** Two
caveats in both directions. The plugin costs 48.1 us per instance inside this
engine against the 28.2 us the sweep measured for it standalone, a 1.7x
discrepancy worth its own look; scaling the plugin part back to 28.2 us puts
the forked p99 at about 30%, i.e. exactly on the line. And the forked figures
are taken through `TANGENT_FANOUT=force`, which is plain timeshare threads, so
a real-time pool should do better than this reads.

What the number does and does not change. It does NOT reopen the build
decision: every non-arithmetic reason in the Lane 6 DECISION block stands, and
the strongest of them is not in that block at all (see the PDC note there). It
does mean this desk, 96 commercial reverbs with nothing armed, is a shape the
fan-out alone does not comfortably fit, and that is the first concrete session
where look-ahead would be the remaining mechanism.

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

- **WITHDRAWN, same day.** This originally read that the gate is already met
  by the item before it, on a 14.6% measured on a convolver desk and a 22%
  projection in section 4.1. Both were wrong. The convolver desk is the wrong
  rung by about 3.8x per bay, the 22% is a mean quoted against a p99 gate and
  rests on a 4.5x fork projection the shipped fork measured at 2.2 to 3.3x,
  and section 4.1's own gate is 70%, not the 30% of this clause. The gate's
  rung has now been run: **50.2%**, and 74.8% at 64 frames. See section 3.0.
  The gate is NOT met, and this item is deferred on risk, not on that number.
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

**The reason that actually decides it is not any of the above, and it was
found only after the gate claim was withdrawn.** `DelayLine::run` advances its
write cursor by `frames` on every call (`instrument.rs:1372`), and both PDC
lines run INSIDE `render_lane`. So a block that is rendered ahead and then
discarded has already advanced that lane's delay line, permanently, against
the mix. The ring is discarded by a loop-edge drag at 60 Hz, a tempo change, a
seek, a plugin swap and an arm toggle. This is a property of look-ahead
itself, not of worker ownership, and no narrowing of the feature removes it:
not a one-block depth, not frozen lanes only, not rendering ahead on the
real-time thread. Until both delay lines are clocked at CONSUMPTION in the
fold rather than at render, the feature cannot be made correct in ANY variant,
and that work has never been scoped. It is also why there is no cheap spike: a
shadow render that throws its result away still advances every delay line,
every compressor envelope and every convolver fill cursor it touches, and
`BuiltinUnit::gain_reduction_db` is a destructive drain (`units/comp.rs:356`),
so a shadow steals the meter reading of the block that will actually be heard.

**Both things that would teach something real have now been done, and the
second one answers the question.**
1. The gate rung. See section 3.0: 50.2%, the gate is not met.
2. **The eligibility probe. Built, run, and it reports zero.** See below.

### Lane 6: what the eligibility probe measured (2026-09-19)

`TANGENT_PROBE=1` turns on a read-only counter in the decide and render walks
(`ProbeCells`, `Renderer::note_eligibility`). It renders nothing early, spawns
no thread, uses no unsafe and changes no output sample. It reports, in
NANOSECONDS actually spent inside `render_lane` and not in lanes, how much of
a block's work a look-ahead would have been allowed to move off the deadline.
Three tiers: as the engine stands; assuming a per-lane k-deep future for clip
audio and MIDI is built; and assuming the instrument-instance handoffs stop
writing a lane's seat from the callback.

Over the owner's own projects, twelve seconds each from the head, 128 frames,
via `the_eligibility_probe_over_real_projects` (all 189 project files verified
byte-identical afterwards):

| project | lanes rendering | us/block in lanes | eligible now | + source | + instruments |
|---|---|---|---|---|---|
| knowersomething1 | 7.3 | 934 | **0.0%** | 100% | 100% |
| moscow1 | 7.0 | 45 | **0.0%** | 100% | 100% |
| demo1 | 1.0 | 88 | **0.0%** | 100% | 100% |
| benjamin1 | 1.0 | 51 | **0.0%** | 100% | 100% |
| demo2 | 1.0 | 13 | **0.0%** | 100% | 100% |

And over all 23 synthetic rungs in `rt_bench`, including the 96-bay commercial
reverb desk: **eligible now 0.0% on every single one.** The one that is not
100% at the next tier is the all-frozen desk (96.2%) and the stopped desk
(59.9%), which is the resting rule already having taken those lanes away.

**Zero is not "small", and it is structural rather than an artefact of a
conservative mask.** `clip_lane` is one block deep for the WHOLE desk and
`clip_lanes` is a whole-timeline mask, so every lane that renders has clips
and no lane that has clips has a future to read. A lane with nothing under
the playhead does not render at all any more, because the resting rule landed
first. The two features overlap exactly: the work look-ahead would have moved
off the deadline is the work the rest rule already stopped doing.

**So the entire value of the feature is behind the piece nobody has scoped.**
The gap between the first column and the second is 100 points, and crossing it
means giving every anticipated lane its own k-deep clip seed and splitting a
single pitch-keyed `u128` note ledger per lane. That is not a term in the
contract; it is a different engine. Until it exists, the measured ceiling on
the whole feature is zero.

### What a look-ahead WOULD be worth, if the source rewrite is accepted

Asked 2026-09-19 by the owner, who does not mind an overhaul that touches the
effects and previous projects. That removes the objection the zero above rests
on, so the question becomes what the ceiling is. The probe was extended to
time the WHOLE block against the part inside `render_lane`, because the rest
of the block is the decide walk, the fold, the sends, the buses, the master
and the taps, and no look-ahead moves any of it. That share is the floor.

| desk | lane work | look-ahead floor |
|---|---|---|
| 96 x Rev PLATE-140 | 99.1% | 0.9% |
| 16 x Drive | 96.5% | 3.5% |
| 8 x Analog, 8 voices | 96.4% | 3.6% |
| 32 lanes x 3 IR | 95.8% | 4.2% |
| 16 x denoise | 89.0% | 11.0% |
| 16 x [EQ, comp, plate] | 85.7% | 14.3% |
| knowersomething1 (real) | 97.1% | 2.9% |
| moscow1 (real) | 70.4% | 29.6% |
| 32 bare lanes | 10.7% | 89.3% |

**On any desk heavy enough to matter, 96 to 99% of the block is work a
look-ahead could move.** The floor is 1 to 4%. So the ceiling is real and
large, and the earlier zero was entirely about the source, not about the
shape of the engine.

What that converts to, on this machine at 128 frames, for the 96-reverb desk:

| | serial | forked (shipped) | prerendered (ceiling) |
|---|---|---|---|
| p99 as a share of the deadline | 239.8% | 50.2% | about 1.5% |
| late blocks, of 2000 | 2000 | 1 | 0 |

**But the gain is not more plugins at 128 frames.** The addressable grid is 32
lanes x 3 primary bays plus 3 on an extra seat, so 192 bays and no more. At
the commercial average of 20.0 us a bay the full grid is 3840 us serial, which
the shipped fork already carries at roughly half a deadline. The fork covers
the whole product at 128 frames. The headroom a look-ahead adds there cannot
be spent, because there are no more slots to fill.

**Where it can be spent, in order:**
1. **Buffer size, which is the real prize.** Halving the buffer doubles a
   desk's share of the deadline today: the 96-reverb desk is 50.2% forked at
   128 frames and 74.8% with six late blocks at 64. Prerendered, the block
   carries only the spine, which is small at every buffer, and the limit
   becomes total CPU, which does not change with buffer size at all. That is
   32 or 64 frame monitoring with a full mix loaded, which today needs freezing.
2. **Heavy instruments.** Sixteen the modelled piano lanes is 5058 us serial, about 55%
   of a 128-frame deadline forked, and about 2% prerendered.
3. **Headroom against the machine being busy with something else.**

**The limit that replaces the deadline is throughput, and it is generous but
finite.** The 96-reverb desk needs 4657 us of CPU per 2667 us of audio, which
is 1.75 of five performance cores. Doubling the desk to the full grid is 3.5
cores. A look-ahead converts a per-block wall into a throughput wall, and on
this machine that wall is roughly three times further out.

**What it would cost, priced for someone who accepts the overhaul.** The
owner said go on 2026-09-19; progress is marked here.

1. **DONE.** Move both PDC delay lines out of `render_lane` and clock them at
   CONSUMPTION in the fold. Required for every variant, standalone correctness
   improvement, and the one piece that survives a revert as a gain.
   `Renderer::run_lane_pdc` runs at the head of the fold's per-lane body;
   `LaneSeats` no longer carries a `DelayLine` and `LanePlan` no longer
   carries `pdc_max`, `folder_lat` or `pedal_latency`, so a worker cannot
   reach a ring the timeline clocks. Behaviour-preserving today, proven by
   the whole suite including the four compensation tests, the grouped-latency
   test and the bit-exact fork null test. Pinned by
   `a_lane_worker_can_no_longer_reach_a_delay_line`.
2. **DONE.** A per-lane clip source, replacing the one-block whole-desk
   `clip_lane`. Built as a PULL rather than a ring: the buffer is deleted, not
   deepened, and a lane asks the schedule for its own span for whatever block
   it is rendering. Because nothing is stored, the source's invalidation list
   is EMPTY rather than the four terms a ring would need or the sixteen the
   render contract carries: a seek, a loop edge moving at drag rate, a clip
   list swap, a punch, varispeed and a short final block all need no action.
   Five commits: the pure clip-major fill with a bit-identity oracle, the
   per-lane pull with the room as first consumer, the seed pulling and the
   buffer deleted, the lane-sorted index, and this measurement.

   **The probe, re-run after it.** Audio lanes have a future to read now:

   | desk | eligible before | eligible after | floor |
   |---|---|---|---|
   | 96 x Rev PLATE-140 | 0.0% | **100.0%** | 0.6% |
   | 32 lanes x 3 IR | 0.0% | **100.0%** | 3.1% |
   | 16 x Drive | 0.0% | **100.0%** | 2.9% |
   | 16 x [EQ, comp, plate] | 0.0% | **100.0%** | 11.9% |
   | knowersomething1 (real) | 0.0% | **61.7%** | 3.0% |
   | moscow1 (real) | 0.0% | 0.0% | 35.3% |
   | 8 x Analog, 8 voices | 0.0% | 0.0% | 3.7% |

   The two that did not move are MIDI. moscow1 is seven Rhodes lanes and the
   Analog rung plays from MIDI clips; what still pins them is the pitch-keyed
   `sounding` word, the shared `active_clip` list and the flat `midi_cursor`,
   which is step 3. knowersomething1's 61.7% is the audio share of a song that
   is part audio and part MIDI, which is what a mixed session should read.

   Two notes for whoever reads these numbers next. The "us/block in lanes"
   column ROSE, because the clip fill moved inside the per-lane timing window
   when it moved into the render. Nothing is slower; the accounting boundary
   moved, and it moved the right way, because that share is exactly what a
   look-ahead can take off the deadline. And the walk itself got cheaper: it
   was frame-major over the whole clip list, so a block cost frames times
   clips whatever the clips were doing, with a single looped clip expanded by
   the host into as many as four thousand passes.
3. **DONE.** Per-lane MIDI. Six commits. The note-ONs turned out to be
   position-pure exactly as the audio was, so `midi_cursor` is DELETED (it was
   never anything but a cache of
   `notes.partition_point(|n| start + n.at < from)`), along with the
   desk-wide `active_clip` list, its 256-entry cap, `clip_parked`, and the
   clip half of the pitch-keyed `sounding` word. The MIDI schedule is
   lane-sorted and indexed like the audio one.

   **One piece of genuine state survives, and naming why matters.** A
   note-off is a promise made to a stateful instrument: it is owed because an
   on was SENT, not because the timeline says a note ended. The shipped code
   already knew this and said so by keeping the sounding list across a clip
   list swap while resetting every cursor, so a note from a retired
   arrangement still gets its off. What replaces it is per lane and is a model
   of the INSTRUMENT rather than of the playhead: `told` (a bit per pitch),
   `told_off` (when each is due) and `park_from`, about 33 KB against the 4 KB
   pool it replaces.

   **Two shipped bugs fell out of it.** The sounding word had two unrelated
   owners, the live keyboard and the clip walk, so a clip note ending on pitch
   P cleared the bit a held live key on P had set and that key then got no off
   at the next stop or locate. And the flush's note-offs were never delivered
   at all: the per-lane walk reads the live list zipped against a list of lane
   masks, a zip stops at the shorter side, and the flush pushed to the notes
   and never to the masks. A DX7 survived only via a blanket sweep beside the
   flush, which reaches no Keys instance, no Analog and no hosted plugin, so
   those rang on through a stop.

   **The probe, re-run.** No lane is pinned by its source any more:

   | desk | after step 2 | after step 3 |
   |---|---|---|
   | knowersomething1 (real) | 61.7% | **100.0%** |
   | moscow1 (real) | 0.0% | **100.0%** |
   | benjamin1, demo1, demo2 (real) | 0.0% | **100.0%** |
   | 8 x Analog, 8 voices | 0.0% | **100.0%** |
   | every audio rung | 100.0% | 100.0% |

   The only rungs below 100% are the all-frozen desk at 94.6% and the stopped
   transport at 61.8%, both of which are pinned on purpose.

   **What this does and does not mean.** It means no lane lacks a future to
   read: the source question is closed. It does NOT mean a rendered block may
   be thrown away, which is a different question about a stateful sink and is
   step 5's.
4. **THE TERMS ARE BUILT AND GREEN. THE SWITCH IS NOT.** Everything that had
   to become true before a worker may hold a track across a block boundary is
   done, each on its own, each with a test that sets the `leased` mask
   directly and asserts the behaviour. The mask is zero in every build,
   because nothing leases yet.
   - `a8ea7c7` **the automation answers for the position it is asked about.**
     This was the review's find and the one that would have been silent. It
     was resolved once per block for ONE position into a single desk-wide
     table, so a track worked on ahead and the tracks on the deadline could
     not both be fed. The position-dependent half is a pure function now, for
     the desk's own fader, pan, send and mute and for every automated plugin
     parameter. The one-shot knob and preset road stays, and a track it lands
     on is pinned by `param_lanes` anyway.
   - `d58e6b1` **the pool is stopped before the desk is freed.** Field order
     had the rack bank, the DX7s and the Keys dropped before the pool, so
     teardown freed every seat and then stopped the threads that render into
     them. An explicit `Drop` takes the pool first.
   - `10dd7b2` **an instrument arriving for a held track waits.** The
     sampler, DX7 and Keys rings and the patch queue all learned their track
     only after taking the arrival. They look first now. The patch queue had
     to change shape from `drain` to `retain`, because a drain takes every
     entry before any track is looked at and the obvious gate would have
     dropped a patch rather than deferring it.
   - `7463ff6` **a plugin dropped onto a held track waits for it.** The swap
     walk skips a held track's seats. The review's own design used a mask on
     the shared state and had two faults in it; which track a seat belongs to
     is arithmetic, so there is no table to keep in step and neither fault
     exists.

   **What is left is the switch itself, and it is one coherent piece:**
   1. The per-track instrument feed moves to the worker. The decide walk
      writes each track's instrument every block (Keys `set_patch`,
      `set_params`, `set_pedal`, the DX7's pedal, and every note push). It is
      not behind a ring and cannot be peeked. Step 3 is what made it
      movable: it is per track now, where it used to touch a desk-wide pitch
      ledger and a shared 256-entry list.
   2. A second dispatch shape in `rtwork`. `LanePool::run`'s soundness
      argument IS its barrier, stated three times in its own doc, so a
      free-running worker needs `'static` per-track work records owned by the
      pool, per-track ready flags, and an explicit happens-before edge in
      place of the one the barrier supplies for free.
   3. A per-track output ring, k blocks deep, keyed by SAMPLE POSITION and
      block LENGTH rather than by block index.
   4. The revoke and acknowledge handshake, and the rule that a track which
      has not acknowledged is rendered by NOBODY that block. "Render it
      serially instead" is the aliasing the handshake exists to forbid.
   5. Step 5's invalidation contract, and the setting, default off.

   **Two things to carry into that work.** No test in this repository can
   detect a data race: there is no loom, no miri and no thread sanitiser
   wiring, and the code is audio callbacks, plugin FFI and real-time threads,
   so none of the three is cheap to add. The protocol has to be argued; what
   is testable is gate coverage, and the four commits above are exactly that.
   And `k` should be expressed in milliseconds rather than blocks: the
   design's block-count formula gave a constant 10.67 ms at 512, 256, 128 and
   64 frames and halved at 32, which is the one case worth having.

   **The original design, for the record, did not survive its attack.**
   Three live faults it turned up on the way ARE fixed and shipped, because
   each stands alone and none of them is about the look-ahead:
   - `74c60fb` a worker that is late could claim a lane belonging to the NEXT
     round and run it through the previous round's closure, which is a
     `mix_channels` stack frame that has returned. One callback is several
     chunks, so the rounds are microseconds apart. The claim word carries the
     round now and a stale claim is refused.
   - `cf942dd` a panic anywhere inside a lane's render skipped the line that
     marks the lane done, and the barrier has no timeout and no exit, so the
     audio callback spun and yielded for ever. The lane is marked from a
     destructor now. The readiness count is handed back the same way.
   - `cf942dd` a test asserting a workgroup-less pool is never ready was made
     false on three platforms by step 6 and had no `cfg` guard.

   **What the attack found in the protocol itself**, all of it fixable but
   none of it built:
   - The automation plan is impossible as written. `resolve_automation` runs
     once per chunk for ONE position, clears the single desk-wide
     `auto_params`, and destructively drains the `ui_params` ring, so a leased
     lane at `at + k` and the pinned lanes at `at` cannot both be fed. That is
     silent wrong audio. The fix is the move that worked twice already: the
     CURVE half is position-pure (`curve.value_at(secs)`) and can be pulled
     per lane for any position, while the `ui_params` half is one-shot and
     already pins its lane through `param_lanes`. That split is step 4's real
     first commit.
   - The per-lane swap mask leaks a permanently sticky bit on
     `hand_off_exchange`'s push-failure rollback, and cannot bound the walk it
     replaces because `lane_of_seat` answers `None` for bus, master and pad
     seats.
   - Nothing in the state machine forces a leased lane back to free, so every
     deferred operation can starve on a busy lane.
   - Promoting `pinned_lanes` out of the probe loses three guards that are
     only in its caller: a stopped transport, varispeed, and a stems export.
   - The `k` formula gives a constant 10.67 ms at 512, 256, 128 and 64 frames
     and 5.33 ms at 32, which is the one case the design calls the prize.

   **The largest piece of ground nobody had named**, and it only became
   possible because of step 3: the decide walk writes each lane's instrument
   every block (Keys `set_patch` / `set_params` / `set_pedal`, the DX7's
   pedal, and every note push). That is not behind a ring and cannot be
   peeked; for a leased lane it has to MOVE to the worker. It could not have
   moved before step 3, because it touched a desk-wide pitch ledger and a
   shared 256-entry list. Now it is per lane and can travel with the lane.

5. **NOT BUILT.** The invalidation contract for the RENDER. Note that the
   SOURCE's contract is empty after steps 2 and 3, which is a different and
   much smaller thing. The discard rate is still the one number nobody has,
   and it can only come from a live session: the counters ship behind
   `TANGENT_PROBE=1` and read out in the audio panel.
6. **DONE, and it applies to the already-shipped fan-out, not only to the
   look-ahead.** Windows gets the Multimedia Class Scheduler Service ("Pro
   Audio" at CRITICAL, with the handle returned on the thread that took it);
   Linux and FreeBSD get `SCHED_FIFO` at a quarter of the way up the class,
   deliberately below the callback it serves. The readiness bar is unchanged
   in kind: a worker counts only with a real-time policy, and the workgroup
   half is tested only where a workgroup exists, which is macOS. `EPERM` on a
   desktop with no `RLIMIT_RTPRIO` means no pool, which is the same answer
   macOS gives a refused join.

   Verified by cross-compiling: the Linux target type-checks clean through
   `scripts/build-cross.sh check-linux`, and the Windows branch type-checks
   against `x86_64-pc-windows-msvc` directly (the full Windows crate cannot
   cross-check here, because an unrelated C dependency needs a toolchain this
   machine does not have). Neither has been RUN on its platform.

**The one number still missing is the tax**, and it is the only one that can
make this not worth it: how often the ring is thrown away. Every automation
point and knob move on a prerendered track discards its work, and on a heavy
track a 60 Hz automation lane could eat the gain outright. The counters ship
and read out in the audio panel.

**The discard half of the probe cannot be measured offline and reads a
truthful zero.** Every event that would throw a ring away is a user-interface
write, and a headless render of a saved document makes none. The counters
ship (param, transport, loop, tempo, swap, arm) and would be read after a live
session with the owner's hands on the desk. That is the one number still
missing, and it can only lower the answer, not raise it.

**A read-only eligibility and discard probe.** In the decide walk, OR the
   complement of the pinned set into an `eligible: u32` from terms that loop
   already computes, count the discard triggers, publish three cells beside
   `RtLoadCells` and read them out behind the existing env-var door. No
   thread, no unsafe, not one output sample changed, so the bit-exact null
   test stays green unconditionally. It measures the two numbers all four
   design reads guessed at and none measured: how many lanes in a real song
   would qualify at all, and how often the ring would be thrown away.

**If it is ever revisited**, it is a setting, default off, and the changelog
has to say three things plainly: a knob on a pre-rendered track lands up to a
look-ahead late, arming mid-roll can click, and every seek and loop wrap pays
its first block on the real-time thread. Note also that a default-off setting
buys less than it sounds: it gates the ring, but the per-lane clip seed, the
`sounding` split, the `DeskRef` copy and the swap arrival mask all ship
unconditionally, on every platform, including the three that can never reach
the feature.

**Goal.** Stage two: render playback-only lanes N blocks AHEAD of the callback into per-lane sample-addressed rings, which is the mechanism REAPER's 15 worker threads actually use. This is what takes non-armed lanes off the per-block deadline entirely and turns their cost into average throughput over all cores, rather than merely dividing it by five.

**Files.** `tangent/src/instrument.rs`, `tangent/src/transport.rs`

**Depends on.** parallel-lanes

**Contract.** THE PINNED (real-time) SET is the union of: lanes whose midi_off bit is CLEAR; lanes in Shared.armed; lanes with a live audition slot (Shared.audition is read for EVERY lane every chunk with no arm gate, and the patch browser names an arbitrary strip); lanes the punch still names (rec.midi/rec.audio are snapshotted ONCE at the record press and never rewritten, and nothing guards the arm toggle mid-take, so a disarmed lane can stay punched); drum lanes; folders; send destinations; sidechain-keyed lanes; the Fx bus; and both master seats. It is NOT the armed set: an open MIDI editor makes its own clip's lane the sole live lane and deafens every armed lane. MUST NOT look ahead at all while the varispeed ratio is not 1.0, because render sizes each chunk from a phase accumulator and the chunk length itself is then unpredictable. MUST invalidate on the FULL set, which is larger than the original map's: the transport generation (seek), the rolling falling edge, loop_a/loop_b, a clip-list swap, a MIDI-list swap, an automation swap, a plugin swap on either of the lane's seats, insert_bypass / frozen / insert_key word changes, ui_params arrivals, keys_patch and keys_params stores, pending_voices patches, AND, missing from the original map and each independently sufficient to serve wrong audio: a TEMPO change (Shared::bpm is read live every chunk by run_rack's arp Transport, the click period and the bus, master and room effect calls, and set_tempo writes it with no clip re-push), a tempo/meter MAPS swap, a click-sample swap, a Keys INSTANCE swap, an auto_suspend change, and a loop wrap. MUST key the ring by SAMPLE POSITION, not block index. MUST NOT reuse room_rack's one-chunk-late mask as a cross-thread handoff: it is an ordering between two passes on the SAME thread and carries no proof a worker has stopped touching that Rack; ownership needs an epoch plus an acknowledgement, and the RT thread may never wait for it. MUST invalidate only the span past a moved loop edge, not the whole ring: loop_b changes 60 times a second during a time-selection drag, and a full discard across 16 lanes is 18 to 84 ms of CPU per discard, which at 60 Hz consumes 21 to 100% of the entire P-core cluster re-rendering audio nobody hears. MUST re-sample the GR meters, insert tap and learn snapshots at CONSUMPTION, since run_rack publishes them and a pre-rendered lane's would lead the audio. MUST NOT open desktop.rs, settings.rs, any unit or analog file, tangent-host, or any themes-lane file. PROVEN BY: (a) a torture test of 10000 blocks with a randomised event stream (seeks, loop wraps, tempo changes, arm toggles, knob turns, plugin swaps, varispeed engagement) whose output matches the serial reference to within -120 dBFS and in which the frame index of every consumed ring block is asserted to equal the block being rendered, so a stale frame can never be served silently; (b) the 96 x Rev PLATE-140 gate with 90 of the 96 bays on non-armed lanes: p99 must fall below 30% of the 2666.7 us deadline; (c) an arm-during-roll test asserting the lane is back on the real-time path within one block and the discontinuity is bounded and counted; (d) the device-bench parity gate: with REAPER as shipped (anticipative, workthreads=15) on the ladder at 128 frames / 48 kHz with the same saved plugin state on both sides, Tangent's non-armed ladder must sustain at least the instance count REAPER sustains on its ARMED ladder, with zero underruns over 60 s, and the stretch target is parity with REAPER's default ladder.

**Expected gain.** Structural rather than arithmetic: it removes non-armed lanes from the per-block deadline entirely, which is the only mechanism that closes the remaining distance to REAPER's shipped configuration. Stage one divides the serial sum by about 5; stage two makes the per-block figure the PINNED set only. In a mixing session, where the owner's own usage is 0 to 2 armed lanes, that is nearly the whole desk. In a tracking session with 8 to 18 armed inputs on the Scarlett it buys close to nothing, and stage one is what carries that case; state that in the acceptance so the win is not oversold. No measurement supports a specific multiplier here, because no look-ahead exists to measure; the gate is the 30%-of-deadline rung and the device-bench parity number, not a projection.

**Risk.** HIGHEST IN THE PLAN, and the evidence says so directly: three of the four threading-seam findings were refuted by BOTH verifiers, every one of them for an incomplete enumeration that would have shipped a bug (room_rack cannot authorise a future block; key_prev does not exist for one; tempo is a live per-block input; varispeed makes chunk lengths unpredictable; the punch and the MIDI-editor rule mean the live set is not the armed set). Build nothing here until lane 1's meter can score it and lane 5's fan-out is green, and derive the eligibility mask from the corrected rule in the contract rather than from the original map. Behaviour changes the owner WILL notice, all of which REAPER also has and which must be accepted deliberately: a knob turned on a pre-rendered lane lands up to a look-ahead late or forces a re-render spike at the moment of touch; arming mid-roll discards blocks a stateful chain has already consumed, so a compressor envelope or reverb tail belongs to audio nobody heard and a click is possible; the first live note after arming can land a look-ahead late (8 chunks at 128 frames is 21 ms, 43 ms at 256), which is the same perceptual territory as the 3 to 10 ms delay the MIDI fast lane was built to kill; every loop wrap and seek re-renders its first block on the real-time thread. Memory: 200 ms of look-ahead is about 75 KB per lane and 2.46 MB for 32 lanes, which is nothing. The handoff machinery itself measured 0.31 us per block for 16 lanes, 0.012% of the deadline, so none of the risk is in the bookkeeping.

---

# 2026-09-19: the head-to-head, at last, and what it says

The sweep had no REAPER number. It has one now, taken on the owner's own
machine with the same plugin at the same settings on both sides.

## The shape

32 tracks, 3 FabFilter Pro-R 2 per track (96 instances), 12 s of noise,
48 kHz. The plugin and its state come from the owner's own
`jaco1.RfxChain`, so REAPER's copy and Tangent's copy are the same preset:
REAPER's `<VST>` payload is a 68-byte wrapper around the plugin's own
`FFBS` blob, and that blob was re-wrapped in a `TGST` container and handed
to `Engine::load_insert` through the new `TANGENT_GATE_PLUGIN` /
`TANGENT_GATE_STATE` doors on `rt_bench`.

REAPER is driven by `-renderproject` on a project file written here. It
needs no ReaScript and touches nothing of the user's.

**Two ways to get this wrong, both hit before the number was believed.**
A `<VST>` block with no base64 payload is silently ignored: the render came
out FASTER than the no-FX control with identical CPU, and the output file
differed only because REAPER dithers, which two renders of the identical dry
project also do. And `RENDER_SRATE`, `RENDER_CHANNELS` and `RENDER_TAILMS`
are ReaScript property names, not project-file tokens: REAPER answers them
with a modal "3 elements not understood", renders at its last-used rate
(44.1 kHz, resampling every track), and the run takes 413 s. The project
sample rate is `SAMPLERATE 48000 1 0`, where the middle field forces it.

## The numbers

REAPER, wall clock, average of repeats:

    32 tracks, no plugins                      1.77 s
    32 tracks x 3 Pro-R 2 (96 instances)       5.61 s    25.5 s of CPU
    => the plugins cost                        3.3-3.8 s of wall per 12 s

Tangent, `rt_bench`, mean us per block x blocks per 12 s:

    grain    mean us   us/frame   wall/12 s   % of a 128 fr deadline
    64          1114.2    17.41      10.03 s      126.5%   (41/2000 over)
    128         1583.9    12.37       7.13 s       83.8%   (0 over)
    256         2162.8     8.45       4.87 s       44.6%   (0 over)
    512         2791.5     5.45       3.14 s       33.3%   (0 over)

## What it says, which is not what either side of the argument expected

**The DSP is level.** At the grain REAPER's offline render chooses for
itself, Tangent renders the same desk in 3.14 s against REAPER's 3.3 to
3.8 s. There is no engine deficit to close.

**The whole difference is GRAIN SIZE**, and it is large: 5.45 us per frame
at 512 frames against 17.41 at 64, a factor of 3.2. Tangent's grain is the
callback's chunk, capped at `MAX_BLOCK`; REAPER's offline render picks its
own, and REAPER's LIVE grain is picked by anticipative FX, which is the
feature this document is about.

So the case for the look-ahead is not "Tangent is slower than REAPER". It
is exactly and only this: **it would give a 64-frame session the CPU
efficiency of a 512-frame one**, measured at 3.2x on this desk, while the
output latency stays at 64 frames. On the same desk today, 64 frames misses
the deadline on 41 blocks in 2000 and 512 frames uses a third of it.

That is also the honest limit of the claim. On the 96 x Rev PLATE-140 rung
the fork alone already passes at 43.2% of the deadline at 128 frames and
58.1% at 64, so the feature buys that desk nothing. It pays on the heavier
desk, at the smaller buffer, and nowhere else.

# 2026-09-19: the pipeline design, and why it is not the one in the plan above

Speculate-and-discard - render a future block, throw it away when the desk
moves - cannot work, and the reason is not in the plan above. The plan says
a discard is free because step 1 moved the PDC delay line to consumption.
That is true of the delay line and of nothing else: **every stateful unit in
the lane's rack has also advanced**, a reverb's delay lines, a compressor's
envelope, a hosted VST3's everything. None can be rewound and none can be
snapshotted on the audio thread. Re-rendering after a discard runs those
units over the same samples twice.

The replacement is a PIPELINE: each lane rendered exactly once, n grains
before the fold needs it, nothing speculative and nothing discarded. Five
adversarial reads of it against this code found it does not survive as
written either, and the findings are worth more than the design was:

1. **There is no ring, and the ring is the design.** `lane_out[ch]` is ONE
   row that is the render's seed, the render's output, the PDC's in-place
   buffer at consumption and the fold's source (`:12813`, `:11778`,
   `:13036`). Same for `lane_dry`. `LanePrep` and `lane_folds` are
   single-slot decide-walk state read at consumption, so the fold would
   apply the wrong grain's decisions. They must travel WITH the grain.
2. **Raw pointers into `Renderer`'s fields are invalidated by the next
   callback.** The plan rules out `*mut Renderer` and then commits the same
   error one level down: `mix_channels` re-carves the whole rack bank with
   three `split_at_mut` and six `iter_mut` EVERY chunk for all 32 lanes
   (`:12805-12815`). The lane banks have to leave `Renderer` for a
   pool-owned `Arc<LaneBank>` of `UnsafeCell`s the callback never reborrows.
3. **`leased` is not the universal gate it is described as.** It gates four
   arrival rings and `swap_plugins`. It does not gate `mix_monitor`'s
   `&mut self.racks[ch]`, `fold_folders`, the flush's `all_notes_off` sweep
   over every instrument (`:8427`), the decide walk's per-lane instrument
   feed, or the decide walk's own `racks[ch].is_empty()` /
   `holds_a_voice()` / `rack_holds_amp()` - and that last is every block.
4. **`Gate` cannot be shared between the two dispatch shapes.** `run`
   STORES the whole `owed` word and zeroes `done` (`rtwork.rs:655`), so a
   still-outstanding lane loses its bit and its `MarkDone` makes `done`
   never equal `lanes`: the callback spins for ever.
5. **The pin set is one chunk LATE and the pipeline needs it n chunks
   EARLY.** `room_rack` and `board_live` say so in their own docs
   (`:5264`, `:7545`, `:7614`): "good enough to COUNT with and not good
   enough to render on". Arming a track, plugging in a guitar or dragging a
   send is a present-tense action. The best available answer is that arming
   becomes a flush point with one chunk of dry monitoring, which is a
   user-facing behaviour change and has to be accepted deliberately.

And one correction to the cost list above: a flush is not merely "n grains
of rack state past the abandoned position". It deletes note-offs whose
`told` entry the decide walk has already cleared, pedal edges already marked
sent, and knob points already popped destructively off `ui_params` - so a
locate would leave a stuck note, a stuck damper and a lost plugin edit. The
decide walk's edge state has to commit at COLLECT, not at dispatch.

---

# 2026-09-19: the grain render, and a defect it uncovered

## What shipped

A lane eligible for it renders in grains of `render_ahead_ms` instead of
once per callback, with the grain boundaries STAGGERED across lanes so each
callback refills a share of the desk. Everything stays inside one callback
under the existing fork and barrier: no worker outlives the callback that
dispatched it, so there is no new ownership model, no second pool, no
revoke handshake, and nothing this repository cannot test.

    96 x FabFilter Pro-R 2, 64 frames    p99 1762 us -> 872 us
                                         132.1% -> 65.4% of the deadline
                                         41 blocks over 2000 -> 1
    96 x FabFilter Pro-R 2, 128 frames   83.8% -> 52.1%

The saving is arithmetic, not scheduling: the fixed cost of a block is
per-INSTANCE. 96 plugins cost 765 us before a single frame is rendered and
5.46 us a frame after it, so eight times fewer `process` calls for the same
audio is the whole of it.

## What is NOT eligible, and why each one

`pinned_lanes` already refuses armed lanes, anything listening to live
MIDI, punched lanes, frozen lanes, folders, send destinations, drum lanes,
sidechain-keyed lanes, room-owned lanes and the lane the insert tap names.
An auditioning lane is added to that list here: an audition is a LEVEL
reconciled against the playhead, and there is no such thing for a span the
playhead has not reached.

**And a lane holding a VOICE, which is the one that is not about grains.**

## The defect: a built-in instrument is not block-size invariant

`is_a_builtin_instrument_the_same_at_any_buffer_size` has no grains in it
at all. Two engines, the same notes, one fed 128-frame buffers and one fed
512, compared as one stream:

    Analog, notes held                 128 vs 512:  -240.0 dBFS  (bit-exact)
    Analog, a note every 700 frames    128 vs 512:   -15.5 dBFS
                                       against a peak of -17.1
    clips only                         128 vs 512:  -240.0 dBFS  (bit-exact)
    clips x [EQ, comp]                 128 vs 512:  -240.0 dBFS  (bit-exact)

**An Analog track already sounds different at one buffer size than at
another**, and the difference is louder than the signal. It only appears
when notes start and stop INSIDE a block - held notes are bit-exact - and
it begins as one ULP at frame 371 and compounds from there.

That shape is a modulation source sampled once per block from
`transport.beat` rather than accumulated per sample. Worth noting beside
it: the beat handed to a rack comes from `Shared::transport_pos`, which
`run_transport` stores AFTER advancing, so it is the block's END and not
its start.

Until that is fixed a lane holding a voice stays on the callback's own
block, where every buffer size gives what it gives today. It is written
down here rather than worked around quietly, because the fix belongs to the
instrument and would be worth making on its own account.

---

# 2026-09-20: making it hermetic, and the four defects that found

The grain render shipped with three equivalence tests, all of them run on a
desk that rolls forward from zero, at one buffer size, with nothing
changing. A desk does none of those things for long. Four tests were
written for the conditions it actually meets, and all four failed on the
first run, on the build that had already been installed.

## The four

**1. A loop.** `clip_grain` - a grain may not cross `loop_b` - was the one
line of the plan that had never been executed. Two separate faults in it:

- The clip cut the catch-up as well as the look-ahead. The wrap is taken at
  BLOCK granularity (`run_transport` folds `pos` back only once the whole
  block is out), so the block that straddles the loop end really does play
  up to a block past `loop_b`. A lane whose grain was cut short at the
  boundary had nothing to render the rest of it with, and the straddling
  block came out SILENT. The clip now shortens the look-ahead and never the
  span the fold is owed.
- The ring kept its grains across the wrap, on the reasoning that both
  sides of the boundary are still the timeline's. They are, but a wrap
  moves `at` BACKWARDS, so every grain in the ring is then ahead of a
  playhead on a lap that has not been played. A wrap now empties the rings,
  consumed on the block after the wrap rather than the one that arms it,
  because that block is still owed the audio either side of the boundary.

**2. The ring needed a third slot.** With grains clipped at the loop end, a
lap's last grain is short - 48 frames, in the case that found this - and a
128-frame block then needs THREE spans at once: the tail of the full grain,
the short one, and the frames past `loop_b`. With two slots the render of
the third destroyed the first, and only on the SECOND lap, because on the
first the reused slot happened not to be one the fold was standing on.
`GRAIN_SLOTS` is 3. Three bounds it: at most two grains already in the ring
plus the one being rendered.

**3. A lane that stops being eligible mid-roll re-ran its rack.** 15.9 dBFS
wrong, measured with the insert tap, which pins a lane and changes no audio
at all and is therefore the cleanest probe there is. Arming a track,
punching in and opening a plugin's face all do the same thing to the mask.
The lane's rack had already rendered forward to the end of its newest
grain; rendering the chunk at the playhead handed it audio it had already
been through.

**4. The knob itself.** Same shape, 0.7 dBFS, and one more way in: a
callback that arrives WIDER than the grain makes `grain <= frames` true for
that block alone, which turned the feature off for one block without
draining the ring (2.4 dBFS).

3 and 4 are one fix: **a lane never renders a span its ring already holds.**
Serve while the ring can answer, then go live from where it ENDS - which is
the rule the catch-up bug taught in the first place, applied to the
ineligible path as well.

## And one thing that is not a defect

`the_grain_survives_a_callback_that_changes_size` measured an Analog lane
14.9 dBFS away from its reference against a -13.9 peak, which reads like a
disaster. **With no grain render involved at all**: an Analog desk fed
17/128/80/33/256-frame callbacks differs from the same desk fed 128s by
that much, and is bit-exact against 64s, 512s, or any sequence of multiples
of 32.

The synth recomputes its per-chunk work every 32 frames off an absolute
counter, and one of those per-chunk quantities is a random draw: the
oscillators' slow wander, three cents of it. A span that ends off the grid
splits a chunk, the walk draws twice, and the two renders detune by a
fraction of a cent and beat against each other. Same instrument, different
dice. It is inaudible as a defect and it is not the grain's.

It did change the design, though. Grain ends are now pushed out to the next
32-frame line (`Renderer::to_grid`), which costs at most 31 frames of extra
look-ahead and makes every grain the feature produces land on the grid. 5 ms
at 48 kHz asks for 240 frames and gets 256. Every shape is then bit-exact at
every callback size a device can actually hand over.

## What is now held

| Test | What it holds |
|---|---|
| `a_grained_lane_renders_what_the_chunked_lane_rendered` | five shapes, bit-exact, and the locate |
| `a_grain_that_is_not_a_whole_number_of_blocks_is_the_same_audio` | eight grain lengths against a 128-frame block |
| `a_builtin_instrument_is_the_same_at_any_buffer_size` | 128 against 512, four shapes |
| `a_loop_wraps_the_same_way_grained_as_chunked` | four shapes, two grains, ten laps of a 3000-frame loop, and that it wrapped |
| `the_grain_survives_a_callback_that_changes_size` | three shapes, two grains, six callback patterns |
| `a_lane_that_becomes_pinned_mid_roll_does_not_re_run_its_rack` | the tap on, moved, and off, while rolling |
| `the_grain_knob_may_be_turned_while_the_transport_rolls` | five turns including off, mid-roll |
| `grain_ab` (a bench) | paired A/B, drift-proof, wall AND process CPU |
| `probe_analog_sizes` (a probe) | the instrument's own block-size landscape |

## Still not held

- **Stems, varispeed and a stopped transport** clear the rings and fall
  back to the chunk, which is asserted by construction and not by a test.
- **Frozen lanes, folders and sends** are pinned, so they are covered by
  the pin test only in the sense that they take the same path.
- **Offline export and freeze** do not set a grain at all.
- **A device that changes buffer size mid-stream** is tested offline, where
  the engine sees the same sequence of spans. The driver's own
  reconfiguration path is not.

---

# 2026-09-20: where this actually stands against REAPER

Same desk, same plugin, same preset as the head-to-head above: 32 tracks,
three FabFilter Pro-R 2 each, 96 instances, the state out of the owner's
own `jaco1.RfxChain`.

**The machine was not quiet.** A video call was running for all of it, load
average 5 to 7. The first attempt at this was a plain sweep and it was
worthless: the row the grain cannot touch - 512 frames, which is
`MAX_BLOCK`, so the grain can never exceed the callback - held to 2% across
four runs while the 128-frame row moved 37%. `grain_ab` was written for
that: every condition is timed back to back inside one repeat, the ratio is
taken inside the repeat, and process CPU is recorded beside the wall clock.
The per-repeat ratio ranges below are the evidence that it worked.

Every Tangent figure here is therefore PESSIMISTIC. REAPER's was taken this
afternoon on a quieter machine.

## The numbers

Tangent, 64-frame callback, fan-out forced, 10000 blocks per condition:

| render ahead | p50 us | % of 1333 us | wall / 12 s | CPU / 12 s | late blocks |
|---|---:|---:|---:|---:|---:|
| off | 1213.9 | 91.0% | 10.92 s | 51.54 s | 840 / 10000 |
| 5 ms (256 fr) | 650.0 | 48.8% | 5.85 s | 25.10 s | **0** |
| 10.7 ms (512 fr) | 498.0 | 37.4% | 4.48 s | 18.67 s | **0** |

At a 128-frame callback: 63.1% off, 48.2% at 5 ms, 37.0% at 10.7 ms.

Paired ratios, which are what survives a busy machine: **1.87x** at 5 ms
(per-repeat range 1.86 to 1.87) and **2.44x** at 10.7 ms (2.41 to 2.44).
At 128 frames, 1.31x and 1.70x.

REAPER, the same project, `-renderproject`, this afternoon:

| | wall / 12 s | CPU / 12 s |
|---|---:|---:|
| 32 tracks x 3 Pro-R 2 | 5.61 s | 25.5 s |
| 32 tracks, no plugins | 1.77 s | not recorded |

## What it says

**Tangent at a 64-frame live buffer now renders this desk faster, and on
less CPU, than REAPER renders it OFFLINE.** 4.48 s of wall and 18.67 s of
CPU against 5.61 s and 25.5 s, with a video call running on Tangent's side
and not on REAPER's, and with Tangent holding a 1.33 ms deadline that an
offline render does not have.

That is a different claim from the one this document opened with, and the
gap it closes is the one it identified: the whole difference was grain
size, measured at 3.2x, and the grain render recovers 2.44x of it.

**What is NOT claimed.** These are `Engine::offline` with the fan-out
forced, not a device callback: no driver interleave, no monitor input pass,
no xrun accounting, and a plain thread rather than a time-constraint one.
REAPER's CPU includes its UI and writing the file; Tangent's includes
neither. The two are close enough that the remaining honest statement is
"level, and no longer behind", not a ranking.

## What a loop costs

A wrap empties every lane's ring, so the block after one rebuilds the whole
desk's look-ahead. Measured, 10000 blocks at 64 frames with a one-second
loop: **10 late blocks at 5 ms and 11 at 10.7 ms**, against 0 with no loop,
and against 840 to 1115 with the feature off. One late block a lap.

**An attempt to remove it failed to measure.** Rebuilding each lane only as
far as its own phase, so the desk comes back up staggered rather than all
on one block, halves the post-wrap work on paper. Measured like for like it
was indistinguishable: 54 late blocks against 11, on a run whose OFF
control - which the change cannot touch - was itself 4.65x worse (3909
against 840). The two scale together to within 5%, and the p50s scale
together to within 0.2%. Not shipped. It is worth one run on a quiet
machine before it is either taken or dropped.

---

# 2026-09-20, later: the rest of the hardening, and the final numbers

## Three more tests, and what they found

`a_stopped_transport_stays_silent_and_comes_back_clean`,
`varispeed_and_stems_fall_back_to_the_chunk_mid_roll` and
`the_pins_that_arrive_mid_roll_do_not_re_run_a_rack` cover the modes that
were previously "asserted by construction". All three failed first time,
and all three were the same thing, which is worth stating once:

**A grained lane's rack is up to one grain AHEAD of the playhead.** Any
operation that suspends or re-points it - a stop, varispeed, a stems
export, a freeze, calling a lane a drum lane - resumes it from a point
further along than a chunked desk's. For a dense reverb that is a
PERMANENT decorrelation: two plates whose delay lines differ at all never
reconverge, so every block after the transition compares at -12 to -15
dBFS for ever.

And it is inaudible, which the measurement says precisely: **the total
energy of the two streams differs by 0.01 to 0.06 dB**, worst window 3.21
dB. Same tail, same loudness, different noise. The samples were the wrong
thing to compare, exactly as they are for a locate and for a built-in
instrument's wander.

What IS bit-exact, and is asserted as such: arming a track, punching in,
and the insert tap. Those leave the rack running, so the drain covers them
completely and not one sample moves. The distinction is now in the test as
a field, `keeps_the_rack`, rather than a hope.

## The phase count was wrong for small buffers

`GRAIN_PHASES` was a flat 8: eight groups of lanes refilling at eight
points across a grain. At a 64-frame callback with a 512-frame grain that
is exactly one phase per callback, which is why it looked right.

At a **32-frame** callback the same grain spans sixteen callbacks, so two
phases landed on one block. Measured: **74.9% of the deadline and 270 late
blocks in 4200 at a 512-frame grain, against 58.4% and 26 at 256** - a
bigger grain came out worse on a desk where it used LESS CPU. The
throughput was fine; the work was lumpy.

It is now `grain / frames`, capped at the lane count. Same desk, same
machine, after: **68.7% and 28 late at 512, 55.6% and 2 at 256.** The off
control moved 10% between the two runs; the late-block count moved 9.6x.

## The final numbers

96 x Pro-R 2, fan-out forced, 4200 blocks per condition, and a video call
still running, so still pessimistic.

| callback | render ahead | p50 us | % of deadline | wall / 12 s | CPU / 12 s | late |
|---|---|---:|---:|---:|---:|---:|
| 32 fr | off | 1025.0 | 153.8% | 18.45 s | 90.13 s | 4200 |
| 32 fr | 5 ms | 370.5 | **55.6%** | 6.67 s | 28.47 s | 2 |
| 32 fr | 10.7 ms | 458.0 | 68.7% | 8.24 s | 20.81 s | 28 |
| 64 fr | off | 1382.5 | 103.7% | 12.44 s | 59.54 s | 2607 |
| 64 fr | 5 ms | 713.8 | 53.5% | 6.42 s | 27.58 s | **0** |
| 64 fr | 10.7 ms | 539.5 | **40.5%** | 4.86 s | 21.11 s | **0** |
| 128 fr | off | 1807.8 | 67.8% | 8.13 s | 38.35 s | 0 |
| 128 fr | 5 ms | 1373.7 | 51.5% | 6.18 s | 25.96 s | 0 |
| 128 fr | 10.7 ms | 1028.0 | **38.6%** | 4.63 s | 19.97 s | 0 |

REAPER, same project, offline, this afternoon: **5.61 s wall, 25.5 s CPU.**

- At **128 frames**: 4.63 s and 19.97 s. Ahead of REAPER's offline render.
- At **64 frames**: 4.86 s and 21.11 s. Still ahead, at a 1.33 ms deadline.
- At **32 frames**: 6.67 s and 28.47 s at the setting that suits it. Behind
  REAPER's offline figure by about 15%, at a 0.67 ms deadline - and
  possible at all, which it was not this morning.

**At 32 frames, 5 ms is the better setting and 10.7 ms is not**, which is
the one place the knob's largest value is not its best. Worth saying in the
panel one day; not worth an automatic choice, because the right answer
depends on the desk as well as the buffer.

---

# 2026-09-20: the feature run against the owner's own songs

The bench desks are a bench author's idea of a session. These are the
sessions. `the_master_has_no_discontinuities` gained a `TANGENT_GRAIN`
door so a real project can be rendered both ways and the two files
compared.

## knowersomething1: clean, and the project is not deterministic

| | total energy | worst 700-frame window |
|---|---:|---:|
| control, render-ahead off twice | +0.016 dB | 7.93 dB |
| test, off against 5 ms | +0.022 dB | 9.33 dB |

The feature adds nothing that the project does not already do to itself
between two identical runs. **The control is the finding**: this song does
not render the same twice, by 7.9 dB in a window, whatever the setting.

## moscow1: a real difference, and it is not what it looks like

Five Rhodes Anthology tracks. Soloing one and rendering 15 seconds of it:

| start | off | 5 ms | difference |
|---|---:|---:|---:|
| 58 s | -24.65 dBFS | -23.05 dBFS | **+1.604 dB** |
| 60 s | -24.10 dBFS | -25.78 dBFS | **-1.675 dB** |
| 63 s | -23.84 dBFS | -22.25 dBFS | **+1.595 dB** |

Real, reproducible, and **the sign flips with where the render starts**,
which is the whole answer. A systematic gain error does not change sign. A
one-time perturbation at the start of a render does, because Rhodes
Anthology is a SAMPLED instrument: perturb it once and it picks a
different round robin from then on, and the RMS of a different take over
fifteen seconds lands a decibel and a half either way.

Everything else agrees with that reading:

- **The amount does not depend on the grain**: +1.433 dB at 128, 256 AND
  512 frames, to three decimals. A span error would scale; a one-time
  divergence does not.
- **It is exactly 0.000 dB when the grain is below the callback**, where
  the feature cannot engage. The harness is sound.
- **The plugins are block-size invariant**: the same track through a
  64-frame callback and a 256-frame one is +0.000 dB with no grain at all.
  So the plugin is not the inconsistent one.
- **Each mode is self-consistent**: two grained renders of the same span
  agree to 0.48 dB in the worst second, two chunked ones to 0.58 dB.
- **Rhodes on a grained lane, in isolation, is +0.000 and -0.011 dB** -
  `a_hosted_instrument_is_the_same_grained`, seated from moscow1's own
  sidecar. The instrument path is fine; it is the locate that perturbs it.

And the thirteen seconds before the music starts, where the grained render
has nothing and the chunked one has -50 dBFS, is the noise floor of
plugins idling with no notes, gated differently. No note is lost: from the
first note at 57.1 s the two agree.

**What bounds it.** `set_grain_ms` has exactly two callers: `publish_routes`,
which is the LIVE engine, and the probe's test door. Export and freeze
build their own offline engine and never set a grain, so **nothing you
bounce is affected by this setting**. It is a live-playback difference on
sampled instruments, and it is the same class of difference those
instruments already produce between any two performances.

## Nine tests written chasing it, all of which pass

Sends to the effects bus. Folders, including desks with so few children
that whole callbacks render no lane at all. Delay compensation across
lanes with unequal latency. Clips that start after a silence, audio and
MIDI. Tail inserts, including an empty lane whose only live rack is the
tail seat - which is what moscow1's lane 16 is. A hosted VST3 instrument
seated from a real sidecar. None of them reproduced it, and all of them
are now held.

---

# 2026-09-20: the quiet run, and the knob that picks itself

The owner cleared the machine. Load 1.98, no call, no Tangent, no
`appstoreagent` loop. These are the numbers everything above was
approximating, and the per-repeat ranges say so: 1.64 to 1.64 at 128
frames, against ranges four times as wide on the busy runs.

## 96 x Pro-R 2, forked, 4200 blocks per condition

| callback | render ahead | p50 us | % of deadline | wall / 12 s | CPU / 12 s | late |
|---|---|---:|---:|---:|---:|---:|
| 32 fr | off | 825.3 | 123.8% | 14.86 s | 70.65 s | 4200 |
| 32 fr | 5.3 ms (256) | 317.7 | **47.7%** | 5.72 s | 23.20 s | 0 |
| 32 fr | 10.7 ms (512) | 417.8 | 62.7% | 7.52 s | 16.76 s | 0 |
| 64 fr | off | 1108.2 | 83.1% | 9.97 s | 47.25 s | 140 |
| 64 fr | 5.3 ms | 619.1 | 46.4% | 5.57 s | 24.03 s | 0 |
| 64 fr | 10.7 ms | 482.0 | **36.1%** | 4.34 s | 17.39 s | 0 |
| 128 fr | off | 1553.1 | 58.2% | 6.99 s | 32.87 s | 0 |
| 128 fr | 5.3 ms | 1215.1 | 45.6% | 5.47 s | 23.54 s | 0 |
| 128 fr | 10.7 ms | 946.6 | **35.5%** | 4.26 s | 18.69 s | 0 |
| 256 fr | off | 2164.1 | 40.6% | 4.87 s | 22.86 s | 0 |
| 256 fr | 5.3 ms | 2166.5 | 40.6% | 4.87 s | 22.91 s | 0 |
| 256 fr | 10.7 ms | 1871.7 | **35.1%** | 4.21 s | 18.09 s | 0 |

The 256-frame row at a 256-frame grain is the harness auditing itself: the
grain cannot exceed the callback, so the feature is off by construction and
the ratio comes out 1.00x with a per-repeat range of 0.99 to 1.00.

## Against REAPER

REAPER, same project, offline: **5.61 s wall, 25.5 s CPU** per 12 s.

| Tangent, LIVE buffer | wall | CPU |
|---|---:|---:|
| 128 fr at 10.7 ms | 4.26 s | 18.69 s |
| 64 fr at 10.7 ms | 4.34 s | 17.39 s |
| 32 fr at 5.3 ms | 5.72 s | 23.20 s |

At 64 and 128 frames Tangent renders this desk in three quarters of
REAPER's wall clock and two thirds of its CPU, while holding a deadline an
offline render does not have. **At 32 frames it is level** - 5.72 s against
5.61 s, and less CPU - which is the row that did not exist this morning,
because at 32 frames without this every single block was late.

## The eight is measured, and the knob now applies it itself

The winner at every buffer is the setting nearest **eight callbacks**: 8x32
= 256, 8x64 = 512, and past that the 512-frame cap. It is the same eight
`grain_phases` divides by, and that is not a coincidence - eight phases
across thirty-two lanes puts four lanes on each callback, which is about
what the fan-out has cores for. Fewer and the pool idles; more and two
boundaries land on one block.

A bigger grain is still cheaper in TOTAL - at 32 frames a 512-frame grain
costs 16.76 s of CPU against 23.20 - and it is the PEAK that is worse. A
deadline is made of peaks.

So the panel has an **Auto**, which is a negative in the setting and is
resolved per callback by the engine rather than fixed when it is chosen.
Measured end to end against the fixed settings it lands on the best one
every time and is never worse:

| callback | auto picks | auto | best fixed |
|---|---|---:|---:|
| 32 fr | 256 fr | **47.9%** | 48.2% |
| 64 fr | 512 fr | **36.1%** | 36.1% |
| 128 fr | 512 fr | **35.7%** | 35.7% |

`auto_picks_the_grain_the_measurements_picked` writes that table into an
assertion, so the rule and the numbers that justify it cannot drift apart.

## What a loop costs, on a quiet machine

At 64 frames with a one-second loop: **8 late blocks in 4200 at 5.3 ms and
7 at 10.7 ms**, against 0 with no loop and 133 with the feature off. One
late block a lap, and the ungrained desk is no better off for looping.

## The post-wrap stagger: measured twice, dead both times

Rebuilding each cold lane only as far as its own phase, so a desk coming
back from a locate or a loop wrap does not do it all on one block, halves
the post-wrap work on paper. It does nothing at all in fact.

| | late / off control | share |
|---|---|---|
| without the stagger | 7 / 133 | 5.3% |
| with it | 20 / 385 | 5.2% |

The second run's machine was 2.9x worse by its own OFF control, and the
stagger's late count is 2.9x higher. Normalised they are the same number
to a tenth of a percent, which is the second time this has measured as
exactly neutral - once on a busy machine, once on a quiet one. Reverted,
and written down here so nobody spends an evening on it again.

The reason it cannot help is probably the fork: spreading the rebuild over
more blocks lowers the peak per block but also leaves fewer lanes with
work on each of them, and the pool wants four to fill it. The two effects
cancel, which is the same tension `GRAIN_AUTO_BLOCKS` is balanced on.

---

# 2026-09-20: the verification pass, and two errors it found in the section above

The owner asked whether the headline deserved a closer look. It did, and
it was wrong in two ways that both flattered Tangent. Both are corrected
here rather than edited out of the table above, because the table above is
what was believed at the time.

## Error 1: REAPER's figure included launching REAPER

`-renderproject` launches the application, loads the project, renders,
writes the file and quits, and `/usr/bin/time` was timing all of it.
Tangent's bench times render calls after a warm-up and nothing else.

The scale series answers it without needing to separate launch from render
by hand: render the same project at 0, 3, 12, 24, 48 and 96 plugins and fit
a line. The slope is the per-plugin cost and the intercept absorbs whatever
is fixed.

    plugins   wall     CPU
          0   1.72 s   1.52 s
          3   1.90     1.56
         12   1.94     3.21
         24   2.63     5.81
         48   3.51    11.83
         96   5.52    26.45

    wall: 39.8 ms per plugin, intercept 1.66 s
    CPU:  262.9 ms per plugin, intercept 0.38 s

**So REAPER's render is 3.82 s of wall, not 5.55.** The other 1.66 s was
the application starting up. Its CPU figure barely moves - 25.24 s against
26.45 measured - because launching costs wall clock and almost no CPU.

Re-measured on the quiet machine, three runs, output confirmed 48 kHz and
12.0 s: 5.51, 6.13, 5.55 s. The afternoon's 5.61 s was sound. The error
was in what it was compared against, not in the number.

## Error 2: Tangent's export does not use the fan-out at all

The comparison put Tangent's LIVE, FORKED throughput beside REAPER's
OFFLINE render and called it a win. But `Engine::offline` builds no pool -
`let pool = (!offline || fanout_mode() == Fanout::Force)` - so the export
and the freeze render on ONE core.

Measured, same desk, the export's own 512-frame blocks, quiet machine:

| | wall / 12 s | CPU / 12 s | cores used |
|---|---:|---:|---:|
| REAPER, render only | **3.82 s** | 25.24 s | 6.6 |
| Tangent export, as it ships | **13.11 s** | 13.32 s | 1.0 |
| Tangent export, fan-out forced | **3.79 s** | 17.96 s | 4.7 |

**REAPER bounces this desk 3.4x faster than Tangent does today**, and the
reason is not the DSP: Tangent does the same work for half the processor
time (13.32 s against 25.24 s). It is that the exporter is single
threaded while REAPER's is not.

With the pool it already has, the same export is 3.79 s against REAPER's
3.82 s - level - on 29% less CPU. `the_fork_renders_what_the_single_walk_
rendered` already holds that the forked walk is bit-identical to the
serial one, so the correctness argument is made; what is missing is a
reason for `Engine::offline` to be given a pool, and a byte-comparison of
a real exported file with and against it before anybody's bounces change.

## What survives

Everything about LIVE playback, which is what render-ahead is for and what
every deadline number measures. At a 64-frame buffer this desk went from
83% of the deadline and 140 late blocks in 4200 to 36% and none; at 32
frames, from every block late to none. Those are Tangent against itself on
one machine and no part of them depends on REAPER.

What does not survive is "Tangent renders it faster than REAPER". On
throughput the honest statement is: **Tangent's engine does the same work
for a third to a half less processor time, and REAPER finishes sooner
because it spreads that work over more cores. Tangent's exporter spreads
it over one.**

---

# 2026-09-20: the export spreads over the cores, and three platforms agree

## What changed

`Engine::offline` built no pool, and the reason written in the source was
that "an export has no deadline and wants every core for the render it is
already running flat out". It was not running flat out. It was running on
one core.

`LanePool::new` now takes `Option<RtPlan>`. `None` is a bounce: no
real-time policy asked for and none that can be refused, so the worker
counts as ready on its own. That is deliberate twice over. A bounce must
not outrank the user interface or a device that is still open - it has no
deadline it could miss. And it means the offline fan-out needs no grant,
which is why it works where the live one cannot (see below).

The export and the freeze both call `set_offline_fanout(true)`.

## What it is worth, and it is not one number

Render calls only, quiet machine, serial against forked:

| desk | serial | forked | gain |
|---|---:|---:|---:|
| 8 lanes x Analog, 8 voices | 501.3 us | 150.9 us | **3.3x** |
| 32 lanes x 3 IR, 0.5 s | 646.8 | 220.7 | 2.9x |
| 16 lanes x denoise | 171.1 | 65.3 | 2.6x |
| 16 lanes x [EQ, comp, plate] | 137.5 | 54.5 | 2.5x |
| 96 bays x Pro-R 2 @ 512 fr | 11657.6 | 3372.1 | **3.5x** |
| 32 lanes x 3 EQ (cheap) | 60.6 | 41.5 | 1.5x |

**It scales with how much work the tracks carry.** A desk of cheap EQs
gets 1.5x because there is not enough per lane to pay for the dispatch.

And a warning about measuring it on a real project: timing the probe
rendering 40 s of `moscow1` gives 8.80 s serial against 8.14 s forked,
which looks like nothing. The reason is in the same line of output -
`user=4.35` against `real=8.80`. Half that wall is the process starting
and Rhodes loading its samples, and the render is a fraction of what is
being timed. It is the same error that put REAPER's launch into its
figure. Time the render, not the program.

## Is it the same audio

Four ways, each stronger than the last:

- `the_fork_renders_what_the_single_walk_rendered` - four built-in shapes,
  bit-exact, as before.
- `the_fork_and_the_grain_are_one_sound` - NEW, and it closes a real hole:
  the fork was only ever tested with the grain off and the grain only ever
  on one thread, while a live session runs both. All four corners against
  the plain walk, bit-exact.
- `the_fork_renders_what_the_single_walk_rendered_with_hosted_plugins` -
  NEW, a real VST3, forked AND grained.
- Two of the owner's own songs rendered through the export path both ways.

That last one needed the control that the rest of this document keeps
teaching. Forked against serial: knowersomething1 -19.3 dBFS, moscow1
-41.4. Serial against serial: -18.2 and -25.0. **On both songs the fork
changes the output LESS than rendering twice serially does.**

The hosted test needed the same lesson built into it. Rev PLATE-140 fails
bit-exactness against the fork - and it fails it against ITSELF, by -26 to
-36 dBFS with the first difference on a different frame every run, which
is a reverb modulating its delay lines with noise. So the test renders two
SERIAL engines as a control and only demands bits where the control earned
them. Pro-R 2 earns them and is held to them; PLATE-140 is held to its own
spread.

## Three platforms

All 20 grain tests pass on macOS (ARM), Linux x86 (void, Ivy Bridge, no
FMA and no AVX2) and FreeBSD x86 (GhostBSD, Tiger Lake, AVX-512). A
different float environment on each and the same answers.

**The real-time grant is where they differ, and it decides whether the
LIVE fan-out works at all:**

| | grant | live fan-out | offline fan-out |
|---|---|---|---|
| macOS | mach policy + audio workgroup | yes | yes |
| Linux | `ulimit -r` 99 via `@audio`, rtkit present | yes | yes |
| FreeBSD | `rtprio` refused to a normal uid - root only | **no** | **yes** |

On FreeBSD `rtprio 10 /usr/bin/true` fails with RTP_SET: Operation not
permitted for uid 1001, because `PRIV_SCHED_RTPRIO` is root's. A worker
that cannot take the policy does not count toward `ready`, so the live
fan-out there collapses to the serial walk. The offline pool asks for no
policy and is unaffected - which is the argument for having built it that
way rather than a happy accident.

## The bounce was asking the wrong question about cores

The tester on `dresden` measured the fan-out at exactly 1.00x there and
went and read why rather than reporting a number. `fanout_peers` is
`performance_cores() - 1`, zero below three, and the non-macOS
`performance_cores` is `available_parallelism() / 2`. Four threads halve
to two, two is below three, and **no pool was ever built** - so the export
fan-out shipped an hour earlier was a no-op on that whole class of machine.
Worse on a box with four REAL cores and no hyperthreading, where the same
halving throws away half of a correct answer.

The gate is right for the LIVE fork and wrong for a bounce, and they are
different questions. A worker on an efficiency core in a real-time fan-out
is slower than not forking, because the block waits for the slowest hand.
An export has no block to wait on and no deadline to miss: an efficiency
core doing some of the work is strictly better than it doing none.

`offline_peers` asks for every core less one. Measured here, bouncing 96
hosted reverbs at 512-frame blocks:

| | wall / 12 s | CPU / 12 s |
|---|---:|---:|
| serial | 13.72 s | 14.04 s |
| performance cores, 4 peers | 3.79 s | 18.10 s |
| **every core, 14 peers** | **1.87 s** | 23.82 s |

Twice as fast again, 7.3x over serial, and **REAPER's 3.82 s is now twice
this**. The CPU figure rises because an efficiency core does less per
watt-second; for a bounce that is the right trade, because wall clock is
what somebody is waiting through.

**A measurement that nearly went in the document wrong.** The first run of
that table reported 13.46 s for the fourteen-peer case - no gain at all -
and the explanation was that the door turning it on had never reached
disk. `git status` showed the file unmodified while the edit had reported
success. The lesson is the cheap one: when a measurement says a change did
nothing, check the change is there before believing the measurement.

## And the hosted-plugin test found its own bar

Rev PLATE-140 modulates its delay lines with noise, so it differs from
ITSELF. Holding the fork to a margin over the control's worst SAMPLE does
not work either, because that figure swings between -24 and -40 dBFS run
to run. Energy does: across five runs the two serial renders differ by
0.00 to 0.01 dB of total energy while their samples differ by up to 8 dB
of spread. Different tail, same amount of it, every time. That is the bar
the fork is held to for a plugin like this, and Pro-R 2 - which is
deterministic - is still held to bits.
