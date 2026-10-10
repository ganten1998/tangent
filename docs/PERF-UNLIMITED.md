# Unlimited tracks at scale: what a lane costs, measured

W7-scale-and-budgets, 2026-10-10. The design is `docs/design/unlimited-tracks.md` (sections 7.12 and 11.8, and the lane's own section under 13). Every figure here is measured, none is asserted in a test: the tests hold the arithmetic (bit for bit), the numbers are read.

## How it was measured

- **Machine.** Apple M5 Pro, 5 performance and 10 efficiency cores, 24 GB, macOS (Darwin 27.0.0). The owner's quiet window: Tangent, Navicord and every audio app closed, a terminal, an editor and a browser open.
- **Engine.** The offline engine (`Engine::offline`, `Engine::offline_sized`) pulled by hand, release profile (opt-level 3, fat LTO), 48 kHz, 128-frame blocks (2,667 us deadline) unless a row says otherwise. It is the renderer the CoreAudio callback drives, minus the device: no workgroup to join, so a forked figure is plain threads.
- **Serial** is `TANGENT_FANOUT=0`. **Forked** is `TANGENT_FANOUT=force`: the live pool (real-time policy, the performance cores less one, four peers here). An **export pool** is `Engine::set_offline_fanout` (every core less one, fourteen here) or `TANGENT_SCALE_PEERS=n`.
- **One process at a time**, nothing else of the lane compiling, each started by a runner that waits (up to 150 s) for the one-minute load average to fall under 2 and writes it beside the row. Runs that started at 2 or above were run again and are not reported here, except where a row says so. The idle floor during the window sat between 1.5 and 2.0; Spotlight indexing the bench binaries pushed it over 3 twice.
- **Medians.** `rt_bench::rt_bench` rows are the median of several runs of 2,000 blocks each (n given). The new rungs (`scale_rungs`, `bus_and_folder_rungs`) are the median of 5 readings of 1,000 blocks (3 of 600 where marked), each from frame zero after its warm-up, with the lowest and highest reading as the spread.
- **Base.** "Base" is main `624d78a`, the engine before unlimited tracks (the build in the bench-base worktree, `23cec3c`, is that commit plus the design document). "Before" is this lane's base `452a9f5` (waves 1 to 7 merged) with only the new bench rows. "After" is this lane.

Runners, logs and binaries: `~/Library/Caches/tangent-ul-scale-and-budgets-lab/notes/bench/`.

### The earlier tries

The two earlier tries (wave 1, 2026-10-09) ran on a busy machine. One logged load averages of 6 to 12 at its starts and read 16 x [EQ, comp, plate] at about 250 us a block against 55 us today; in it the base binary was run through a runner that put cargo under `taskpolicy -b`, which the bench process inherits (background QoS keeps a process on the efficiency cores). Either is reason enough; neither was isolated.

## 1. Today's shapes against wave 1

`rt_bench::rt_bench`, 128 frames. The budget: within 5% of wave 1.

| | base | before | after | after vs base |
|---|---|---|---|---|
| **forked** 16 x [EQ, comp, plate], mean | 55.3 (53.3 to 57.2, n=8) | 61.8 (n=4) | 52.1 (51.6 to 52.1, n=3) | **-5.8%** |
| p99 | 64.0 | 69.6 | 60.5 | -5.5% |
| **forked** 32 x 3 EQ, mean | 42.8 (41.8 to 43.2, n=8) | 46.7 (n=4) | 43.8 (43.4 to 43.8, n=3) | **+2.3%** |
| p99 | 49.1 | 62.7 | 51.0 | +3.9% |
| **serial** 16 x [EQ, comp, plate], mean | 132.8 (n=6) | 136.4 (n=3) | 128.0 (n=3) | **-3.6%** |
| **serial** 32 x 3 EQ, mean | 57.7 (n=6) | 63.8 (n=3) | 59.8 (n=3) | **+3.6%** |

All in microseconds per block. **Before this lane, the forked shapes were 9 to 12% over wave 1** and the serial 32 x 3 EQ 10.6% over; the slot engine's per-chunk and per-lane walks had grown. After it, both shapes are inside 5% both ways. Most of the recovery is the legacy desk's idle lanes: `audio_desk` installs all thirty-two lanes, so "16 lanes" is sixteen playing and sixteen idle, and those now rest (1 lane, no inserts: 15 us serial on the base, 9 us after).

The wave-1 figures written in the design (66 us mean and 104 us p99, 57 us) were measured on 2026-09-18 at `c9ff152`, three weeks of engine work before the base. The base above is the engine as it stood when unlimited tracks began, measured in the same window as the lane's builds, which is the comparison that means something.

## 2. A bare lane, at width

`scale_rungs`, every lane playing noise through no plugin, serial. The marginal cost is the difference between two rungs over the lanes between them.

| | 32 | 64 | 256 | 1024 |
|---|---|---|---|---|
| before, us a block | 27.1 | 47.9 | 174.8 | 703.7 (26.4%) |
| after | 23.6 | 39.7 | 141.6 | 576.8 (21.6%) |
| before, marginal us a lane | | 0.65 | 0.66 | 0.69 |
| after, marginal us a lane | | 0.50 | 0.53 | 0.57 |

Forked (before / after): 38.6 / 31.5, 58.4 / 51.3, 157.9 / 127.0, 587.0 / 468.9 us. A playing lane's floor is its render (the clip pull, the delay line, the fold over 128 frames), so it stays O(frames); what came off is the fader slew that ran per frame for lanes whose fader had not moved (section 5).

## 3. Idle lanes: the active list

`scale_rungs` idle rung: every lane has a clip a minute in, warmed 9.6 s so racks have passed their 8 s park window, nothing under the playhead. Serial. The budget: 1,024 idle lanes at most 4% of a 128-frame deadline (106.7 us), reported, not asserted.

| | 32 | 64 | 256 | 1024 |
|---|---|---|---|---|
| before | 16.8 | 26.4 | 84.3 | 318.9 (12.0%) |
| settled faders and the pedal walk, resting off | | | 25.5 | 82.4 (3.1%) |
| after (resting on) | 8.9 | 10.3 | 19.4 | **55.3 (2.1%)** |

Marginal per idle lane: 0.30 us before, 0.047 us after. Forked: 331.3 us before, 56.0 us after. **The budget is met**, by two changes measured apart: a settled fader no longer walks 128 frames to stand still (318.9 to 82.4 us), and a lane with nothing to do is no longer visited by the grain clock, the pedal walk, the decide walk or the fold (82.4 to 55.3 us). What is left is per chunk (the master, the plan's walks) and a few bit tests per resting lane.

## 4. Plugins at width (before the change)

`scale_rungs`, one unit on every lane, or [EQ, comp, plate]. Measured on the lane's base with the bench rows only (every lane plays, so the active list does not bear on these); the serial run started at load 2.69 and the forked at 1.89, both climbing over 3 as the rungs ran, so read these as indicative.

| us a block | 32 | 64 | 256 | 1024 |
|---|---|---|---|---|
| serial x EQ | 39.6 | 73.6 | 273.3 | 1,199.3 |
| serial x comp | 62.3 | 118.1 | 454.5 | 1,919.7 |
| serial x plate | 205.2 | 404.4 | 1,618.9 | 7,018.7 |
| serial x [EQ, comp, plate] | 261.6 | 518.2 | 2,094.0 | 9,040.5 |
| forked x EQ | 42.9 | 59.7 | 181.6 | 736.4 |
| forked x comp | 46.1 | 68.9 | 223.7 | 903.1 |
| forked x plate | 76.2 | 131.6 | 472.1 | 2,006.9 |
| forked x [EQ, comp, plate] | 87.4 | 152.7 | 567.8 | 2,436.0 (91.4%, 32 late of 5,000) |

A built-in plate costs about 6.3 to 7.0 us a lane serial and 1.7 to 2.0 forked; 1,024 lanes of [EQ, comp, plate] fit the 128-frame deadline only forked, at 91%. The serial 1,024 x plate rung, run again clean (load 1.98), read 6,921.8 us.

## 5. What changed in the engine

All in `tangent/src/instrument.rs`, all bit-identical to the walk they replace:

- **A settled fader is one step, not 128** (`slew`). A step that leaves the value's bits where they were leaves every later step there, since each is the same arithmetic on the same three numbers; so one step is computed and compared, and the walk runs only when it moves. Used by the route-read level in `mix_clips`, the idle and drum exits of the decide walk, and the pre-fader gate in `clear_send_buffers`.
- **The pedal walk reads a lane's own run of the MIDI schedule** (`clip_pedal_level`, `next_pedal_edge`, and the stopped desk's `lane_has_clips`), through the schedule's bounds table: each read every MIDI clip on the desk for every lane every chunk, lanes times clips. The answers are a `max`, a `min` and an `any`, so the order moves nothing.
- **The active list** (`Renderer::fill_active`). A lane whose decide visit took the idle exit with nothing owed (a settled fader, no note held, parked or auditioned, no pedal standing, no MIDI clip, nothing in its grain ring the playhead can reach again, and none of the kinds or pins a visit treats apart) rests: the grain clock, the pedal walk, the decide walk and the fold pass it by, through a dense list in rank order. It wakes, per lane, the chunk anything it reads changes (`still_resting`: a clip under the playhead, a route landing on it, a knob or a curve, an arm, a kind, a pin, an audition, its fader's target), and every lane wakes on a locate, a stop or a start, an op, a plugin, a sample or an instrument arriving, a clip list or a MIDI schedule swapped, or a change of loop, grain, slew rate, stems, varispeed, probe or sidechain keys. The one thing a skipped visit would have counted, the park rule's quiet frames, is paid when the lane wakes (`LaneBody::rest_from`), by a clock that counts what each skipped visit would have added: the chunk, or the grain the clock widens an idle lane's span to (`Renderer::rest_inc`).
- **Tests.** `rt_bench::a_resting_lane_renders_what_the_full_walk_rendered`: two engines of one 48-lane desk, one resting and one walking every lane, the same events between blocks (a resting lane's fader moved, an audition held on a lane carrying an instrument and let go, a plugin seated in a resting lane, a solo and a mute through a new plan and back, a clip list swapped in, a locate, a loop that wraps and is taken away, a fader moved again near the end), compared sample by sample for 3,400 blocks with the grain render off and on; then every lane's fader and paid-up park counter. It requires at least 30 lanes to have rested and one to have woken, and asks the engine directly that a plugin arriving woke the lane it landed in (nothing in the sound can show it while that lane has nothing to play). `instrument::tests::a_settled_slew_is_the_walk_it_skips` holds `slew` against the plain walk over 20,000 starts, targets, rates and lengths, a few ulps off the target included. `Engine::set_active_list(false)` is the full walk.
- **Mutations** (each rule broken on purpose, the tests run against it): no quiet debt paid on waking (caught: a lane's park counter drifted); a clip under the playhead not waking a lane (caught: the sound differed at the clip); the debt counted by the chunk while grains are served (caught: the rest refuses a visit whose span the clock does not count, so nothing rested); an audition not waking a lane (caught: its counter, and the instrument's note); a moved fader's target not waking a lane (caught: its fader compared at the end), a plugin's arrival not waking one (caught: the lane it landed in, asked directly), and a tolerance in place of the bit compare in `slew` (caught: `a_settled_slew_is_the_walk_it_skips`). One term is not reached by any test: a grain ring holding audio the playhead can reach again (after a loop wrap) keeps its lane awake. It guards grains rendered ahead of a loop's end, and no desk here builds one.

## 6. The bus and folder rungs

`bus_and_folder_rungs`, 128 frames. Fifty returns, each a bus with a plate fed by its own track's send; twenty-five folders side by side, each holding two tracks with [EQ, comp, plate] in its own bays.

| | serial | forked, levels walked | forked, levels forked |
|---|---|---|---|
| 50 returns x plate | 389.4 / 391.2 (14.6%) | 419.5 (15.7%) | 191.8 (7.2%) |
| 25 folders x [EQ, comp, plate] | 256.0 / 256.2 (9.6%) | 270.7 (10.2%) | 131.0 (4.9%) |

(Serial shows both settings, which are the same walk there.) The level fork halves both, 2.2x and 2.1x, against the 2,667 us deadline. Measured before the active list, which does not touch the post-fork levels. Forked here is the live pool (`TANGENT_FANOUT=force`); the rungs forked on the export pool before this lane, and `TANGENT_SCALE_POOL=export` still does.

## 7. The pool against the serial walk (the carried wave-1 question)

Same window, same binaries, interleaved:

| | base serial | base forked | after serial | after forked |
|---|---|---|---|---|
| 16 x [EQ, comp, plate] | 132.8 | 55.3 (2.4x) | 128.0 | 52.1 (2.5x) |
| 32 x 3 EQ | 57.7 | 42.8 (1.35x) | 59.8 | 43.8 (1.37x) |
| 16 lanes, no inserts | 21.4 | 26.8 (slower) | 15.9 | 21.7 (slower) |
| 1,024 x [EQ, comp, plate] (before) | 9,040.5 | 2,436.0 (3.7x) | | |

**Conclusive for the offline bench on a quiet machine:** the pool wins wherever there is plugin work to divide (1.35x to 3.7x) and costs 5 to 6 us a block on a desk with none, which is the join. Still unmeasured: a live device, with the workers joined to its workgroup, which is the case the readiness gate exists for.

## 8. The pool's width, and its cap (kept at 16)

1,024 lanes, each a plate, the export pool (plain threads), 3 readings of 600 blocks:

| peers | 0 (serial) | 1 | 2 | 4 | 8 | 14 |
|---|---|---|---|---|---|---|
| us a block | 6,921.8 | 3,759.2 | 2,710.2 | 1,948.4 | 1,587.1 | 1,285.3 |
| against serial | 1.00x | 1.84x | 2.55x | 3.55x | 4.36x | 5.39x |

Still 1.23x from 8 to 14 peers, with the last six on efficiency cores. `rtwork::MAX_WORKERS` stays 16: no machine it has been measured on reaches it (the live pool here is 4 and a bounce 14; the tester boxes the fan-out was swept on have 4 threads), and a wider LIVE pool is a bet that can switch the fork off: the fork needs every worker real-time and in the device's workgroup, the workgroup names a width of its own (`os_workgroup_max_parallel_threads`, 10 here), and whether a join past it is refused (which would leave the pool never ready) has not been tried. A bounce on a 32-core machine would want more; raise it for bounces alone, on a machine that has the cores to measure it.

## 9. The grain at scale

256 lanes of [EQ, comp, plate], forked, 3 readings of 2,000 blocks, at small callbacks (where `GRAIN_AUTO_BLOCKS` decides anything: at 128 frames the auto grain is capped at 512 frames either way).

| 32-frame callback (667 us) | off | 128 fr (4 blocks) | 256 fr (8, auto) | 512 fr (16) |
|---|---|---|---|---|
| mean | 188.0 | 173.7 | 176.6 | 184.0 |
| p99 | 215.8 | 205.5 | 201.3 | 202.1 |

| 64-frame callback (1,333 us) | off | 128 fr (2) | 256 fr (4) | 512 fr (8, auto) |
|---|---|---|---|---|
| mean | 303.9 | 297.0 | 294.4 | 306.6 |
| p99 | 333.1 | 324.0 | 330.4 | 352.2 |

Built-in units pay little per block, so the grain moves them a few percent either way, and one run of three readings does not separate the settings (at 64 frames the auto setting's p99 is the worst of four, by 6%, inside what a single run varies by). **`GRAIN_AUTO_BLOCKS` stays 8**: the measurement that chose it, on hosted reverbs whose per-instance block cost is what a grain saves, is still the one that decides, and it was not repeated at scale here. **The grain phase cap** was `CHANNELS` (the fixed desk's width) and is now `GRAIN_PHASE_CAP`, the same 32, argued from the grain (at most `MAX_BLOCK` frames, so 32 phases is a 16-frame callback's whole grain) rather than from a lane count; no grain moves.

## 10. Not done, and why

- **The fold split** (each lane's fader, pan, peak and stem-column work moved into its hand, design 7.8): optional, and not done. The fold is serial by rank because `mix` and `aux` are summed in that order; moving the per-lane half into the hands is possible and is the next thing to try if a forked 1,024-lane desk is over its deadline with the work evenly split.
- **Need-sized delay lines:** the measurements did not ask for it. Idle lanes are now 0.05 us each and a playing lane's floor is its render, not its memory.
- **Hosted plugins at scale** (the grain retune's real question) and **a live device** (the pool's real question) are the two measurements this window did not take.

## 11. Seen in passing

`render_golden::every_desk_renders_its_golden` fails under `--release` at this lane's base `452a9f5`, before any change here, on one desk: `auto-pan: e585ba2096f0df42 (recorded 5728be85fdd7db3a)`. It passes in the debug build the gate runs (this lane's gate included), so the goldens agree with a debug build and one desk's pan curve comes out differently optimised. Not chased here.
