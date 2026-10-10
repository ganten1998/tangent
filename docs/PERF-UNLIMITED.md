# Unlimited tracks at scale: what a lane costs, measured

W7-scale-and-budgets, 2026-10-10. The design is `docs/design/unlimited-tracks.md` (sections 7.12 and 11.8, and the lane's own section under 13). Every figure here is measured, none is asserted in a test: the tests hold the arithmetic (bit for bit), the numbers are read.

## How it was measured

- **Machine.** Apple M5 Pro, 5 performance and 10 efficiency cores, 24 GB, macOS (Darwin 27.0.0). Tangent, Navicord and every audio app closed; a terminal, an editor and a browser open.
- **Engine.** The offline engine (`Engine::offline`, `Engine::offline_sized`) pulled by hand, release profile (opt-level 3, fat LTO), 48 kHz, 128-frame blocks (2,667 us deadline) unless a row says otherwise. It is the renderer the CoreAudio callback drives, minus the device: no workgroup to join, so a forked figure is plain threads.
- **Serial** is `TANGENT_FANOUT=0`. **Forked** is `TANGENT_FANOUT=force`: the live pool (real-time policy, the performance cores less one, four peers here). An **export pool** is `Engine::set_offline_fanout` (every core less one, fourteen here) or `TANGENT_SCALE_PEERS=n`.
- **Three engines, one window.** Every figure below was taken on the afternoon of 2026-10-10, between 12:56 and 13:27, in the 43 runs the appendix lists. "Base" is main `624d78a`, the engine before unlimited tracks (the build in the bench-base worktree, `23cec3c`, is that commit plus the design document). "Before" is this lane's base `452a9f5` (waves 1 to 7 merged) with only the new bench rows. "After" is this lane's engine, `fd675e4`, in one binary built during the round that fixed the review's findings (which touches tests and comments, not the engine), so no table mixes builds.
- **The first version of this report is replaced.** It was measured in the owner's morning window (from about 11:05), and the review upheld three things against it: one reported run had started at a load of 2.69 (the serial per-unit rungs, and with them the 9,040.5 us that carried the pool question's widest row); the per-unit and bus and folder rungs were given only for the engine before the change; and no load stood beside a row. Its logs (`A-` to `G-`) stay beside these.
- **One process at a time**, nothing else of the lane compiling or measuring. Each run is started by a runner that holds it until the one-minute load average is under 2 (up to ten minutes; it marks a run that gave up, and none did) and logs the load average (one, five and fifteen minutes) at the run's start and end. **Every run here started under 2**, between 1.74 and 1.98; between runs the machine sat near 2, and the runner waited 1,000 s in all, at most 120 s for one start.
- **The load beside every row.** A row gives the one-minute load when its first rung began and when its last ended, read off the rungs' own `[load a -> b]` in the log; an `rt_bench::rt_bench` row gives the range over the runs its median is taken from. A run's own threads lift the average as it goes (toward one over a long serial run, toward five over a forked one), so a load that climbs along a row is mostly the bench itself, and the rows late in a long run carry it (the idle rows of a forked run begin near 4.7). The load at a run's start is what measures everything else on the machine, and the appendix gives it for every run.
- **Medians.** `rt_bench::rt_bench` rows are the median of three runs of 2,000 blocks each, base, before and after interleaved. The new rungs (`scale_rungs`, `bus_and_folder_rungs`) are the median of 5 readings of 1,000 blocks (3 of 600 or of 2,000 where marked), each from frame zero after its warm-up, with the lowest and highest reading as the spread.

Runners, logs and binaries: `~/Library/Caches/tangent-ul-scale-and-budgets-lab/notes/bench/` (`roundH.sh`, `roundH-extra.sh` and `roundH-extra2.sh` through `runq2.sh`; logs `H-*.log`; binaries in `bin/`, `after-fix` the after engine).

### The earlier tries

The two earlier tries (wave 1, 2026-10-09) ran on a busy machine. One logged load averages of 6 to 12 at its starts and read 16 x [EQ, comp, plate] at about 250 us a block against 54 us today; in it the base binary was run through a runner that put cargo under `taskpolicy -b`, which the bench process inherits (background QoS keeps a process on the efficiency cores). Either is reason enough; neither was isolated.

## 1. Today's shapes against wave 1

`rt_bench::rt_bench`, 128 frames, three runs of each engine, interleaved. The budget: within 5% of wave 1.

| | base | before | after | after vs base |
|---|---|---|---|---|
| **forked** 16 x [EQ, comp, plate], mean | 53.7 (53.0 to 55.3) | 58.6 (57.2 to 60.7) | 51.7 (51.0 to 51.8) | **-3.7%** |
| p99 | 61.7 | 68.2 | 60.8 | -1.5% |
| **forked** 32 x 3 EQ, mean | 42.9 (40.8 to 43.0) | 46.6 (43.4 to 46.8) | 43.8 (38.8 to 44.2) | **+2.1%** |
| p99 | 48.6 | 54.6 | 52.0 | +7.0% |
| **serial** 16 x [EQ, comp, plate], mean | 133.4 (133.3 to 139.3) | 139.3 (135.1 to 140.0) | 130.7 (130.0 to 131.5) | **-2.0%** |
| p99 | 153.0 | 161.5 | 150.6 | -1.6% |
| **serial** 32 x 3 EQ, mean | 58.1 (57.6 to 60.1) | 64.1 (63.1 to 65.8) | 59.9 (59.5 to 60.0) | **+3.1%** |
| p99 | 71.0 | 73.9 | 70.0 | -1.4% |
| load at start, forked runs | 1.92 to 1.97 | 1.87 to 1.94 | 1.85 to 1.95 | |
| load at end, forked runs | 1.87 to 2.21 | 1.85 to 2.36 | 1.78 to 2.43 | |
| load at start, serial runs | 1.78 to 1.98 | 1.78 to 1.98 | 1.83 to 1.96 | |
| load at end, serial runs | 1.78 to 1.98 | 1.83 to 2.21 | 1.92 to 2.44 | |

All in microseconds per block. **Before this lane the shapes were 4 to 10% over the base** (forked +9.1% and +8.6%, serial +4.4% and +10.3%): the slot engine's per-chunk and per-lane walks had grown. After it, every mean is inside 5% either way. The one figure past 5% is the forked 32 x 3 EQ p99, 7.0% over the base's median and inside the spread of the after engine's own three runs (45.0 to 59.2 us, against the base's 47.8 to 49.5); the design gives that shape a mean, not a p99. Most of the recovery is the legacy desk's idle lanes: `audio_desk` installs all thirty-two lanes, so "16 lanes" is sixteen playing and sixteen idle, and those now rest (1 lane, no inserts: 15.1 us serial on the base and 9.3 after, 23.6 and 9.8 forked).

The wave-1 figures written in the design (66 us mean and 104 us p99, 57 us) were measured on 2026-09-18 at `c9ff152`, three weeks of engine work before the base. The base above is the engine as it stood when unlimited tracks began, measured beside the lane's builds, which is the comparison that means something.

## 2. A bare lane, at width

`scale_rungs`, every lane playing noise through no plugin. The marginal cost is the difference between two rungs over the lanes between them.

| us a block | 32 | 64 | 256 | 1024 | load at start | load at end |
|---|---|---|---|---|---|---|
| before, serial | 27.1 | 47.6 | 171.4 | 693.2 (26.0%) | 1.88 | 2.05 |
| **after, serial** | 23.2 | 39.6 | 143.8 | 575.1 (21.6%) | 1.74 | 2.00 |
| before, forked | 39.0 | 54.5 | 158.5 | 588.8 (22.1%) | 1.97 | 1.90 |
| after, forked | 30.2 | 44.7 | 127.4 | 475.6 (17.8%) | 1.96 | 1.96 |

| marginal us a lane | 32 to 64 | 64 to 256 | 256 to 1024 |
|---|---|---|---|
| before, serial | 0.641 | 0.645 | 0.679 |
| **after, serial** | 0.513 | 0.543 | 0.562 |
| before, forked | 0.484 | 0.542 | 0.560 |
| after, forked | 0.453 | 0.431 | 0.453 |

A playing lane's floor is its render (the clip pull, the delay line, the fold over 128 frames), so it stays O(frames); what came off is the fader slew that ran per frame for lanes whose fader had not moved (section 5).

## 3. Idle lanes: the active list

`scale_rungs` idle rung: every lane has a clip a minute in, warmed 9.6 s so racks have passed their 8 s park window, nothing under the playhead. The budget: 1,024 idle lanes at most 4% of a 128-frame deadline (106.7 us), reported, not asserted.

| us a block | 32 | 64 | 256 | 1024 | load at start | load at end |
|---|---|---|---|---|---|---|
| before, serial | 16.9 | 26.7 | 84.9 | 319.0 (12.0%) | 2.89 | 2.98 |
| after, resting off (`TANGENT_SCALE_REST=0`) | 9.5 | 11.8 | 25.6 | 82.7 (3.1%) | 1.92 | 2.09 |
| **after, serial** | 9.1 | 10.5 | 20.2 | **58.0 (2.2%)** | 2.13 | 2.13 |
| before, forked | 18.1 | 28.0 | 89.2 | 330.2 (12.4%) | 4.77 | 4.39 |
| after, forked | 9.5 | 11.0 | 20.6 | 58.9 (2.2%) | 4.68 | 4.70 |

The idle rows are the last shape of a full `scale_rungs` run, so their loads carry the run's own threads (those runs started at 1.88 and 1.74 serial, 1.97 and 1.96 forked); the resting-off row is a run of its own. Marginal per idle lane, 256 to 1,024: 0.305 us before, 0.074 with resting off, 0.049 after. **The budget is met**, by two changes measured apart in one binary: a settled fader no longer walks 128 frames to stand still (319.0 to 82.7 us, resting off), and a lane with nothing to do is no longer visited by the grain clock, the pedal walk, the decide walk or the fold (82.7 to 58.0 us). What is left is per chunk (the master, the plan's walks) and a few bit tests per resting lane.

## 4. Plugins at width

`scale_rungs`, one unit on every lane, or [EQ, comp, plate]. Every lane plays, so the active list does not bear on these; what moved is the fader slew.

| us a block | 32 | 64 | 256 | 1024 | load at start | load at end |
|---|---|---|---|---|---|---|
| EQ, serial, before | 39.2 | 73.1 | 271.9 | 1,182.4 (44.3%) | 2.05 | 2.02 |
| EQ, serial, after | 35.3 | 64.4 | 241.6 | 1,073.6 (40.3%) | 2.00 | 1.93 |
| EQ, forked, before | 42.8 | 59.8 | 181.6 | 734.7 (27.6%) | 1.90 | 1.82 |
| EQ, forked, after | 33.4 | 50.6 | 150.8 | 627.1 (23.5%) | 1.96 | 2.70 |
| comp, serial, before | 62.3 | 118.8 | 458.6 | 1,936.8 (72.6%) | 2.02 | 2.46 |
| comp, serial, after | 58.6 | 111.2 | 429.5 | 1,829.2 (68.6%) | 1.93 | 2.32 |
| comp, forked, before | 42.5 | 68.8 | 223.2 | 894.4 (33.5%) | 1.82 | 2.14 |
| comp, forked, after | 38.4 | 60.4 | 190.6 | 781.9 (29.3%) | 2.70 | 3.12 |
| plate, serial, before | 208.9 | 413.9 | 1,653.5 | 7,065.4 (265.0%) | 2.46 | 3.22 |
| plate, serial, after | 203.9 | 403.7 | 1,610.5 | 6,885.0 (258.2%) | 2.32 | 2.80 |
| plate, forked, before | 76.2 | 131.8 | 469.9 | 1,991.7 (74.7%) | 2.14 | 3.56 |
| plate, forked, after | 72.0 | 123.7 | 440.2 | 1,878.2 (70.4%) | 3.12 | 4.02 |
| [EQ, comp, plate], serial, before | 263.0 | 518.2 | 2,105.4 | 9,068.0 (340.0%) | 3.22 | 2.89 |
| [EQ, comp, plate], serial, after | 257.6 | 509.7 | 2,066.5 | 8,902.5 (333.8%) | 2.80 | 2.13 |
| [EQ, comp, plate], forked, before | 88.3 | 153.5 | 568.1 | 2,433.8 (91.3%) | 3.56 | 4.77 |
| [EQ, comp, plate], forked, after | 83.2 | 145.7 | 551.6 | 2,344.0 (87.9%) | 4.02 | 4.68 |

| marginal us a lane | 32 to 64 | 64 to 256 | 256 to 1024 |
|---|---|---|---|
| EQ, serial, before / after | 1.059 / 0.909 | 1.035 / 0.923 | 1.186 / 1.083 |
| EQ, forked, before / after | 0.531 / 0.538 | 0.634 / 0.522 | 0.720 / 0.620 |
| comp, serial, before / after | 1.766 / 1.644 | 1.770 / 1.658 | 1.925 / 1.823 |
| comp, forked, before / after | 0.822 / 0.688 | 0.804 / 0.678 | 0.874 / 0.770 |
| plate, serial, before / after | 6.406 / 6.244 | 6.456 / 6.285 | 7.047 / 6.868 |
| plate, forked, before / after | 1.738 / 1.616 | 1.761 / 1.648 | 1.982 / 1.872 |
| [EQ, comp, plate], serial, before / after | 7.975 / 7.878 | 8.267 / 8.108 | 9.066 / 8.901 |
| [EQ, comp, plate], forked, before / after | 2.038 / 1.953 | 2.159 / 2.114 | 2.429 / 2.334 |

A full `scale_rungs` run takes its shapes in order in one process (bare, EQ, comp, plate, the chain, idle), so each row's loads carry the shapes before it (the serial runs started at 1.88 before and 1.74 after, the forked at 1.97 and 1.96). A built-in plate costs about 6.2 to 6.9 us a lane serial and 1.6 to 1.9 forked. At 1,024 lanes the after engine is 2 to 9% faster than the before serial and 4 to 15% forked: the slew it no longer walks is a fixed saving a lane, so it shows most beside the cheapest unit, and most of all where the fork leaves the serial walks as the larger share. 1,024 lanes of [EQ, comp, plate] fit the 128-frame deadline only forked: 2,344.0 us (87.9%, 13 of 5,000 blocks late) as the last shape of this run, and 2,300.9 us (86.3%, none late) in a run of its own that started at 1.85 (section 7).

## 5. What changed in the engine

All in `tangent/src/instrument.rs`, all bit-identical to the walk they replace:

- **A settled fader is one step, not 128** (`slew`). A step that leaves the value's bits where they were leaves every later step there, since each is the same arithmetic on the same three numbers; so one step is computed and compared, and the walk runs only when it moves. Used by the route-read level in `mix_clips`, the idle and drum exits of the decide walk, and the pre-fader gate in `clear_send_buffers`.
- **The pedal walk reads a lane's own run of the MIDI schedule** (`clip_pedal_level`, `next_pedal_edge`, and the stopped desk's `lane_has_clips`), through the schedule's bounds table: each read every MIDI clip on the desk for every lane every chunk, lanes times clips. The answers are a `max`, a `min` and an `any`, so the order moves nothing.
- **The active list** (`Renderer::fill_active`). A lane whose decide visit took the idle exit with nothing owed (a settled fader, no note held, parked or auditioned, no pedal standing, no MIDI clip, nothing in its grain ring at or past the playhead, or past the loop's head while a loop stands, and none of the kinds or pins a visit treats apart) rests: the grain clock, the pedal walk, the decide walk and the fold pass it by, through a dense list in rank order. It wakes, per lane, the chunk anything it reads changes (`still_resting`: a clip under the playhead, a route landing on it, a knob or a curve, an arm, a kind, a pin, an audition, its fader's target), and every lane wakes on a locate, a loop's wrap, a stop or a start, an op, a plugin, a sample or an instrument arriving, a clip list or a MIDI schedule swapped, or a change of loop, grain, slew rate, stems, varispeed, probe or sidechain keys. The one thing a skipped visit would have counted, the park rule's quiet frames, is paid when the lane wakes (`LaneBody::rest_from`), by a clock that counts what each skipped visit would have added: the chunk, or the grain the clock widens an idle lane's span to (`Renderer::rest_inc`).
- **Tests.** `rt_bench::a_resting_lane_renders_what_the_full_walk_rendered`: two engines of one 48-lane desk, one resting and one walking every lane, the same events between blocks (a resting lane's fader moved, an audition held on a lane carrying an instrument and let go, a plugin seated in a resting lane, a solo and a mute through a new plan and back, a clip list swapped in, a locate that lands inside a loop set with it, four wraps of that loop, the loop taken away, a fader moved again near the end), compared sample by sample for 3,400 blocks with the grain render off and on; then every lane's fader and paid-up park counter. It requires at least 30 lanes to have rested and one to have woken, counts the wraps off the transport (four, or it fails), and asks the engine directly for the two wakes nothing in the sound can show while the lane woken has nothing to play: a plugin arriving wakes the lane it lands in, and a wrap wakes every lane resting when its flush lands (34 met each wrap, at least 20 are required). **The first version's loop never wrapped:** it was set 300 blocks after the locate, with the playhead already 0.3 s past the loop's end, and a playhead past a loop plays on (`run_transport`). The review caught it. `rt_bench::a_lane_does_not_rest_while_its_ring_still_holds_a_tail`: the one visit that reaches the idle exit with audio still ahead in a lane's ring is a grained look-ahead. A plate on a lane of the grain clock's first phase (which asks for its next grain on the first chunk of the one being served) loses its clip once its tail is under the park floor, so its next look-ahead visit takes the idle exit with three chunks of tail still to come; the two engines are compared bit for bit while the full walk folds that tail out, and then the lane must rest. `instrument::tests::a_settled_slew_is_the_walk_it_skips` holds `slew` against the plain walk over 20,000 starts, targets, rates and lengths, a few ulps off the target included. `Engine::set_active_list(false)` is the full walk.
- **Mutations** (each rule broken on purpose, the tests run against it as they now stand): no quiet debt paid on waking (caught: a lane's park counter drifted); a clip under the playhead not waking a lane (caught: the sound differed at the clip); the debt counted by the chunk while grains are served (caught: the rest refuses a visit whose span the clock does not count, so nothing rested); an audition not waking a lane (caught: the sound, at the audition); a moved fader's target not waking a lane (caught: its fader compared at the end); a plugin's arrival not waking one (caught: the lane it landed in, asked directly); a tolerance in place of the bit compare in `slew` (caught: `a_settled_slew_is_the_walk_it_skips`); **a loop's wrap not waking the resting lanes** (`flush` out of `fill_active`'s wake; caught: lane 10 slept through the first wrap, asked directly); **a lane resting with audio still ahead in its ring** (`dead_before` out of the rest; caught by the ring test: the resting walk gave 0 where the full walk folded the tail). One term no test can catch, by construction, and its comment says so rather than claim one: the loop's head in the ring's floor (`rest_floor`). A wrap clears every lane's ring before the first chunk at the loop's head reads one (`run_transport`), as a locate does, so a ring is never read behind the playhead and the term only keeps awake, until the wrap, a lane that could rest; taken out, the four wraps still render bit for bit, as that argument says they must. The first version of this report called it "a guard no test reaches" and put that down to no desk building a grain ahead of a loop's end; the reason is the clear. Ring test and wrap check were each written against their mutation: the ring test's first draft put the plate on the grain clock's last phase, which asks on a grain's last chunk with nothing left to drop, and passed with `dead_before` gone.

## 6. The bus and folder rungs

`bus_and_folder_rungs`, 128 frames. Fifty returns, each a bus with a plate fed by its own track's send; twenty-five folders side by side, each holding two tracks, with [EQ, comp, plate] in the folder's own bays. Before and after, each rung with its share of the 2,667 us deadline and its load.

| | before | load | after | load | after vs before |
|---|---|---|---|---|---|
| 50 returns x plate, serial, levels walked | 397.6 (14.9%) | 1.93 to 1.93 | 373.8 (14.0%) | 1.92 to 1.92 | -6.0% |
| 50 returns x plate, serial, levels "forked" (the same walk) | 398.0 (14.9%) | 1.93 to 1.85 | 375.4 (14.1%) | 1.92 to 2.09 | -5.7% |
| 50 returns x plate, forked, levels walked | 425.8 (16.0%) | 1.86 to 1.86 | 395.0 (14.8%) | 1.87 to 1.87 | -7.2% |
| 50 returns x plate, **forked, levels forked** | 194.6 (7.3%) | 1.86 to 1.86 | **166.1 (6.2%)** | 1.87 to 1.87 | -14.6% |
| 25 folders x [EQ, comp, plate], serial, levels walked | 256.4 (9.6%) | 1.85 to 1.85 | 247.2 (9.3%) | 2.09 to 2.09 | -3.6% |
| 25 folders x [EQ, comp, plate], serial, levels "forked" (the same walk) | 256.3 (9.6%) | 1.85 to 2.03 | 247.6 (9.3%) | 2.09 to 2.24 | -3.4% |
| 25 folders x [EQ, comp, plate], forked, levels walked | 268.9 (10.1%) | 1.86 to 1.79 | 258.4 (9.7%) | 1.87 to 2.44 | -3.9% |
| 25 folders x [EQ, comp, plate], **forked, levels forked** | 131.4 (4.9%) | 1.79 to 1.79 | **122.3 (4.6%)** | 2.44 to 2.44 | -6.9% |

The four runs started at 1.93 and 1.86 (before, serial and forked) and 1.92 and 1.87 (after). The level fork divides both: 2.38x on the returns and 2.11x on the folders after (2.19x and 2.05x before), all well inside the deadline. The after engine is 3 to 15% faster here, though every lane plays and none rests: the settled-fader early-out (`slew`) runs in `clear_send_buffers`' pre-fader gate and in the route-read level, which these desks exercise on every send and route. Forked here is the live pool (`TANGENT_FANOUT=force`); the rungs forked on the export pool before this lane, and `TANGENT_SCALE_POOL=export` still does.

## 7. The pool against the serial walk (the carried wave-1 question)

Same window, same binaries; the three shapes from section 1's interleaved runs (loads there), the widest from runs of its own:

| | base serial | base forked | after serial | after forked |
|---|---|---|---|---|
| 16 x [EQ, comp, plate] | 133.4 | 53.7 (2.48x) | 130.7 | 51.7 (2.53x) |
| 32 x 3 EQ | 58.1 | 42.9 (1.35x) | 59.9 | 43.8 (1.37x) |
| 16 lanes, no inserts | 21.3 | 26.9 (slower) | 15.9 | 21.4 (slower) |
| 1,024 x [EQ, comp, plate], each its own run | | | 8,686.9 (load 1.93 to 2.85) | 2,300.9 (3.78x; load 1.85 to 2.84; none of 5,000 late) |

**Conclusive for the offline bench on a quiet machine:** the pool wins wherever there is plugin work to divide (1.35x to 3.78x) and costs about 5.5 us a block on a desk with none, which is the join. The widest row is the after engine in two runs that each started under 2 and timed nothing but that rung, not the last rung of a long run whose own threads had lifted the load. Still unmeasured: a live device, with the workers joined to its workgroup, which is the case the readiness gate exists for.

## 8. The pool's width, and its cap (kept at 16)

1,024 lanes, each a plate, the export pool (plain threads), 3 readings of 600 blocks, each width a run of its own:

| peers | us a block (lowest to highest) | p99 | against serial | load at start | load at end |
|---|---|---|---|---|---|
| 0 (serial) | 6,729.7 (6,701.2 to 6,758.9) | 7,394.2 | 1.00x | 1.97 | 2.13 |
| 1 | 3,760.7 (3,759.8 to 3,768.9) | 3,988.8 | 1.79x | 1.86 | 2.10 |
| 2 | 2,705.9 (2,704.4 to 2,707.9) | 2,882.8 | 2.49x | 1.96 | 2.21 |
| 4 | 1,924.9 (1,924.1 to 1,928.5) | 2,107.3 | 3.50x | 1.86 | 2.43 |
| 8 | 1,499.2 (1,498.7 to 1,502.8) | 1,597.0 | 4.49x | 1.87 | 2.60 |
| 14 | 1,285.7 (1,283.2 to 1,290.8) | 1,558.4 | 5.23x | 1.89 | 1.89 |

Still 1.17x from 8 to 14 peers, with the last six on efficiency cores. `rtwork::MAX_WORKERS` stays 16: no machine it has been measured on reaches it (the live pool here is 4 and a bounce 14; the tester boxes the fan-out was swept on have 4 threads), and a wider LIVE pool is a bet that can switch the fork off: the fork needs every worker real-time and in the device's workgroup, the workgroup names a width of its own (`os_workgroup_max_parallel_threads`, 10 here), and whether a join past it is refused (which would leave the pool never ready) has not been tried. A bounce on a 32-core machine would want more; raise it for bounces alone, on a machine that has the cores to measure it.

## 9. The grain at scale

256 lanes of [EQ, comp, plate], forked, 3 readings of 2,000 blocks, each setting a run of its own, at small callbacks (where `GRAIN_AUTO_BLOCKS` decides anything: at 128 frames the auto grain is capped at 512 frames either way).

| 32-frame callback (667 us) | off | 128 fr (4 blocks) | 256 fr (8, auto) | 512 fr (16) |
|---|---|---|---|---|
| mean | 187.2 | 175.0 | 179.0 | 184.1 |
| p99 | 211.5 | 200.0 | 197.6 | 201.6 |
| load at start, end | 1.89, 2.30 | 1.94, 1.94 | 1.94, 2.51 | 1.93, 1.93 |

| 64-frame callback (1,333 us) | off | 128 fr (2) | 256 fr (4) | 512 fr (8, auto) |
|---|---|---|---|---|
| mean | 302.3 | 296.1 | 291.3 | 300.6 |
| p99 | 350.3 | 329.0 | 318.3 | 329.1 |
| load at start, end | 1.93, 2.34 | 1.90, 1.90 | 1.90, 2.07 | 1.93, 1.93 |

Built-in units pay little per block, so the grain moves them a few percent either way, and one run of three readings does not separate the grained settings (at 32 frames the auto setting's p99 is the best of the four; at 64 frames it is 3% behind 256 frames, inside what a single run varies by). **`GRAIN_AUTO_BLOCKS` stays 8**: the measurement that chose it, on hosted reverbs whose per-instance block cost is what a grain saves, is still the one that decides, and it was not repeated at scale here. **The grain phase cap** was `CHANNELS` (the fixed desk's width) and is now `GRAIN_PHASE_CAP`, the same 32, argued from the grain (at most `MAX_BLOCK` frames, so 32 phases is a 16-frame callback's whole grain) rather than from a lane count; no grain moves.

## 10. Not done, and why

- **The fold split** (each lane's fader, pan, peak and stem-column work moved into its hand, design 7.8): optional, and not done. The fold is serial by rank because `mix` and `aux` are summed in that order; moving the per-lane half into the hands is possible and is the next thing to try if a forked 1,024-lane desk is over its deadline with the work evenly split.
- **Need-sized delay lines:** the measurements did not ask for it. Idle lanes are now 0.05 us each and a playing lane's floor is its render, not its memory.
- **Hosted plugins at scale** (the grain retune's real question) and **a live device** (the pool's real question) are the two measurements neither window took.

## 11. Seen in passing

`render_golden::every_desk_renders_its_golden` fails under `--release` at this lane's base `452a9f5`, before any change here, on one desk: `auto-pan: e585ba2096f0df42 (recorded 5728be85fdd7db3a)`. It passes in the debug build the gate runs (this lane's gate included), so the goldens agree with a debug build and one desk's pan curve comes out differently optimised. Not chased here.

## Appendix: every run behind a figure

One process each, in the order they ran, with the load average (one, five and fifteen minutes) the runner logged at the run's start and end. `waited` is how long the runner held the start for the one-minute load to fall under 2. The settings drop the `TANGENT_` and `TANGENT_SCALE_` prefixes; every `rt_bench` run also set `TANGENT_GATE_PLUGIN=/nonexistent`.

| # | started | log | engine | test | settings | waited | load at start | load at end |
|---|---|---|---|---|---|---|---|---|
| 1 | 12:56:10 | `H-scale-serial` | after | `scale_rungs` | FANOUT=0 | 0 s | 1.74, 1.84, 2.42 | 2.13, 2.10, 2.42 |
| 2 | 12:59:17 | `H-scale-force` | after | `scale_rungs` | FANOUT=force | 5 s | 1.96, 2.06, 2.41 | 4.70, 2.86, 2.68 |
| 3 | 13:02:00 | `H-busfold-serial` | after | `bus_and_folder_rungs` | FANOUT=0 | 100 s | 1.92, 2.42, 2.53 | 2.24, 2.48, 2.55 |
| 4 | 13:02:29 | `H-busfold-force` | after | `bus_and_folder_rungs` | FANOUT=force | 20 s | 1.87, 2.38, 2.51 | 2.44, 2.49, 2.55 |
| 5 | 13:03:16 | `H-before-scale-serial` | before | `scale_rungs` | FANOUT=0 | 40 s | 1.88, 2.36, 2.50 | 2.98, 2.71, 2.61 |
| 6 | 13:07:12 | `H-rt-base-force` | base | `rt_bench` | FANOUT=force | 40 s | 1.94, 2.48, 2.53 | 1.87, 2.46, 2.52 |
| 7 | 13:07:16 | `H-rt-before-force` | before | `rt_bench` | FANOUT=force | 0 s | 1.87, 2.46, 2.52 | 2.36, 2.55, 2.55 |
| 8 | 13:07:51 | `H-rt-after-force` | after | `rt_bench` | FANOUT=force | 30 s | 1.95, 2.43, 2.51 | 2.43, 2.52, 2.54 |
| 9 | 13:08:25 | `H-rt-base-force` | base | `rt_bench` | FANOUT=force | 30 s | 1.97, 2.40, 2.50 | 2.21, 2.45, 2.51 |
| 10 | 13:08:39 | `H-rt-before-force` | before | `rt_bench` | FANOUT=force | 10 s | 1.94, 2.38, 2.49 | 2.27, 2.44, 2.51 |
| 11 | 13:08:54 | `H-rt-after-force` | after | `rt_bench` | FANOUT=force | 10 s | 1.92, 2.36, 2.48 | 1.92, 2.36, 2.48 |
| 12 | 13:08:58 | `H-rt-base-force` | base | `rt_bench` | FANOUT=force | 0 s | 1.92, 2.36, 2.48 | 1.93, 2.35, 2.48 |
| 13 | 13:09:02 | `H-rt-before-force` | before | `rt_bench` | FANOUT=force | 0 s | 1.93, 2.35, 2.48 | 1.85, 2.33, 2.47 |
| 14 | 13:09:07 | `H-rt-after-force` | after | `rt_bench` | FANOUT=force | 0 s | 1.85, 2.33, 2.47 | 1.78, 2.31, 2.46 |
| 15 | 13:09:11 | `H-rt-base-0` | base | `rt_bench` | FANOUT=0 | 0 s | 1.78, 2.31, 2.46 | 1.96, 2.33, 2.46 |
| 16 | 13:09:21 | `H-rt-before-0` | before | `rt_bench` | FANOUT=0 | 0 s | 1.96, 2.33, 2.46 | 2.21, 2.37, 2.48 |
| 17 | 13:09:46 | `H-rt-after-0` | after | `rt_bench` | FANOUT=0 | 15 s | 1.92, 2.30, 2.45 | 2.31, 2.37, 2.47 |
| 18 | 13:10:15 | `H-rt-base-0` | base | `rt_bench` | FANOUT=0 | 20 s | 1.92, 2.28, 2.44 | 1.78, 2.24, 2.42 |
| 19 | 13:10:25 | `H-rt-before-0` | before | `rt_bench` | FANOUT=0 | 0 s | 1.78, 2.24, 2.42 | 1.96, 2.26, 2.43 |
| 20 | 13:10:35 | `H-rt-after-0` | after | `rt_bench` | FANOUT=0 | 0 s | 1.96, 2.26, 2.43 | 2.44, 2.35, 2.46 |
| 21 | 13:11:29 | `H-rt-base-0` | base | `rt_bench` | FANOUT=0 | 45 s | 1.98, 2.25, 2.41 | 1.98, 2.24, 2.41 |
| 22 | 13:11:39 | `H-rt-before-0` | before | `rt_bench` | FANOUT=0 | 0 s | 1.98, 2.24, 2.41 | 1.83, 2.20, 2.39 |
| 23 | 13:11:48 | `H-rt-after-0` | after | `rt_bench` | FANOUT=0 | 0 s | 1.83, 2.20, 2.39 | 1.92, 2.22, 2.39 |
| 24 | 13:11:58 | `H-rest0` | after | `scale_rungs` | FANOUT=0 SHAPES=idle,bare REST=0 | 0 s | 1.92, 2.22, 2.39 | 2.16, 2.26, 2.41 |
| 25 | 13:13:08 | `H-width` | after | `scale_rungs` | FANOUT=0 SHAPES=plate WIDTHS=1024 REPEATS=3 BLOCKS=600 | 60 s | 1.97, 2.21, 2.38 | 2.13, 2.24, 2.39 |
| 26 | 13:13:44 | `H-width` | after | `scale_rungs` | SHAPES=plate WIDTHS=1024 REPEATS=3 BLOCKS=600 PEERS=1 | 15 s | 1.86, 2.18, 2.36 | 2.10, 2.22, 2.37 |
| 27 | 13:14:40 | `H-width` | after | `scale_rungs` | SHAPES=plate WIDTHS=1024 REPEATS=3 BLOCKS=600 PEERS=2 | 45 s | 1.96, 2.19, 2.35 | 2.21, 2.23, 2.37 |
| 28 | 13:15:29 | `H-width` | after | `scale_rungs` | SHAPES=plate WIDTHS=1024 REPEATS=3 BLOCKS=600 PEERS=4 | 40 s | 1.84, 2.16, 2.34 | 2.43, 2.27, 2.37 |
| 29 | 13:16:00 | `H-width` | after | `scale_rungs` | SHAPES=plate WIDTHS=1024 REPEATS=3 BLOCKS=600 PEERS=8 | 25 s | 1.87, 2.15, 2.33 | 2.60, 2.30, 2.38 |
| 30 | 13:16:39 | `H-width` | after | `scale_rungs` | SHAPES=plate WIDTHS=1024 REPEATS=3 BLOCKS=600 PEERS=14 | 35 s | 1.89, 2.17, 2.32 | 1.89, 2.17, 2.32 |
| 31 | 13:16:43 | `H-grain-32` | after | `scale_rungs` | FANOUT=force SHAPES=chain WIDTHS=256 REPEATS=3 BLOCKS=2000 FRAMES=32 GRAIN=0 | 0 s | 1.89, 2.17, 2.32 | 2.30, 2.25, 2.35 |
| 32 | 13:17:00 | `H-grain-32` | after | `scale_rungs` | FANOUT=force SHAPES=chain WIDTHS=256 REPEATS=3 BLOCKS=2000 FRAMES=32 GRAIN=2.6667 | 15 s | 1.94, 2.17, 2.32 | 1.94, 2.17, 2.32 |
| 33 | 13:17:02 | `H-grain-32` | after | `scale_rungs` | FANOUT=force SHAPES=chain WIDTHS=256 REPEATS=3 BLOCKS=2000 FRAMES=32 GRAIN=5.3334 | 0 s | 1.94, 2.17, 2.32 | 2.51, 2.28, 2.36 |
| 34 | 13:17:25 | `H-grain-32` | after | `scale_rungs` | FANOUT=force SHAPES=chain WIDTHS=256 REPEATS=3 BLOCKS=2000 FRAMES=32 GRAIN=10.6667 | 20 s | 1.93, 2.17, 2.32 | 1.93, 2.17, 2.32 |
| 35 | 13:17:27 | `H-grain-64` | after | `scale_rungs` | FANOUT=force SHAPES=chain WIDTHS=256 REPEATS=3 BLOCKS=2000 FRAMES=64 GRAIN=0 | 0 s | 1.93, 2.17, 2.32 | 2.34, 2.25, 2.34 |
| 36 | 13:19:30 | `H-grain-64` | after | `scale_rungs` | FANOUT=force SHAPES=chain WIDTHS=256 REPEATS=3 BLOCKS=2000 FRAMES=64 GRAIN=2.6667 | 120 s | 1.90, 2.17, 2.30 | 1.90, 2.17, 2.30 |
| 37 | 13:19:32 | `H-grain-64` | after | `scale_rungs` | FANOUT=force SHAPES=chain WIDTHS=256 REPEATS=3 BLOCKS=2000 FRAMES=64 GRAIN=5.3334 | 0 s | 1.90, 2.17, 2.30 | 2.07, 2.20, 2.31 |
| 38 | 13:20:15 | `H-grain-64` | after | `scale_rungs` | FANOUT=force SHAPES=chain WIDTHS=256 REPEATS=3 BLOCKS=2000 FRAMES=64 GRAIN=10.6667 | 40 s | 1.93, 2.17, 2.29 | 1.93, 2.17, 2.29 |
| 39 | 13:20:27 | `H-chain1024-serial` | after | `scale_rungs` | FANOUT=0 SHAPES=chain WIDTHS=1024 | 0 s | 1.93, 2.16, 2.29 | 2.85, 2.38, 2.36 |
| 40 | 13:22:38 | `H-chain1024-force` | after | `scale_rungs` | FANOUT=force SHAPES=chain WIDTHS=1024 | 70 s | 1.85, 2.21, 2.30 | 2.84, 2.41, 2.37 |
| 41 | 13:23:44 | `H-before-scale-force` | before | `scale_rungs` | FANOUT=force | 50 s | 1.97, 2.25, 2.31 | 4.39, 2.92, 2.55 |
| 42 | 13:26:35 | `H-before-busfold-serial` | before | `bus_and_folder_rungs` | FANOUT=0 | 95 s | 1.93, 2.49, 2.42 | 2.03, 2.49, 2.42 |
| 43 | 13:26:49 | `H-before-busfold-force` | before | `bus_and_folder_rungs` | FANOUT=force | 5 s | 1.86, 2.45, 2.41 | 1.79, 2.43, 2.40 |
