# Tape's processor cost in 5.2 (open issue)

Measured 2026-10-18 on the M-series Mac the project is built on, release build,
quiet machine, each bench run alone with `--test-threads=1`.

## Tangent Tape

| | 5.1 (main 825a91c3) | 5.2 (tape/machine) |
|---|---|---|
| `rt_bench` row "16 lanes x Tape", mean per block | 370.5 us (23.2 per insert) | 470.6 us (29.4 per insert) |
| `the_tape_costs_what_its_budget_says`, defaults, mean (warmed) | | 20.7 to 20.8 us |
| the same, fully engaged, p99 | | 34.7 to 35.8 us |

The design's budget was 16 us at the defaults and never above 5.1's cost; the
bench's own bars were 21 and 24 us. On the sixteen-lane row, which warms both
builds the same way, the new Tape costs about 27 % more than 5.1's: a little
over 1 % of one core per insert at 48 kHz and 128 frames. The single-unit
bench now holds the measured figures with a margin (26 and 44 us); lower the
bars as the cost comes down. Unwarmed, a fresh binary's first seconds read
about 30 % dearer (the thread starting on an efficiency core, or the clock
ramping), so the bench runs a second untimed first.

Where the time goes, from a throwaway profile that switched stages off:

- the record stage (the 4x polyphase pair and the four `tanh` a channel-sample)
  is most of it: without it a Tape costs about a fifth;
- the transport and the wear grid, the contour bells and the per-sub-block
  design are each about 1 to 2 us;
- a rational `tanh` saved only about 1 us, so the curve itself is not the cost;
- the polyphase pair does 226 multiply-adds against the old direct form's 484
  and still costs what that did, so how the compiler lays out the record stage
  (inlining, vectorisation of the 4x pair) is the first lead.

## Tape start and stop

Measured 2026-10-19, release build, on a shared machine (other lanes building
and testing), so the microseconds are shared-machine figures and the ratios are
what decides. `tape_start_and_stop_costs_what_the_dial_costs_probe`: the
bench's sixteen lanes through a plate, grains on, 48 kHz, rolling at rest,
rolling with the dial at 0.75, and starts and stops at 0.1 s each way, the three
interleaved a tape cycle at a time over 31 rounds on a user-interactive thread.
Each ratio is the median of the rounds' own ratios, so the machine's other work,
which lands on all three alike, falls out; the ranges are seven runs of each
engine, the two alternated.

| cycles against | 5.2 (aa94a2be) | 5.3 |
|---|---|---|
| the dial's worst block, 64 frames | 4.1 to 6.2x | 1.17 to 1.32x |
| the dial's worst block, 128 frames | 3.1 to 3.9x | 1.15 to 1.31x |
| the dial's p99, 64 frames | 1.24 to 1.42x | 1.36 to 1.52x |
| the dial's p99, 128 frames | 1.29 to 1.39x | 1.22 to 1.36x |
| rest's worst block, 64 / 128 frames | 4.1 to 5.3x / 2.8 to 3.3x | 0.97 to 1.14x / 1.00 to 1.10x |
| rest's p99, 64 / 128 frames | 1.03 to 1.11x / 1.05 to 1.17x | 1.14 to 1.23x / 1.05 to 1.13x |
| the deadline, worst block, 64 / 128 frames | 32 to 35 % / 16 to 25 % | 8 to 16 % / 6 to 11 % |
| the deadline, p99, 64 / 128 frames | 6.1 to 8.3 % / 8.1 to 9.6 % | 6.2 to 7.4 % / 5.2 to 6.6 % |

The worst blocks against the deadline are all rounds pooled, so a stall of the
machine's lands in them whole: one 5.3 run at 64 frames read 52 %, with rest at
419 % in the same run, and is left out of that row.

Where the time went, from a throwaway profile that classified every pass:

- **The worst block of every start was not a ramp's.** It was the first block
  at rest after it. While the tape changes speed every lane renders the chunk
  it is asked for and holds nothing ahead, so the first block back at speed
  found every ring cold and every lane rendered a whole grain at once: 8423
  lane-frames where a block at rest renders 1024, 423 us against 63 at 64
  frames, and the block after it refilled two phases at once (2048
  lane-frames, 112 us). At 128 frames the same, 424 us against 114. A Play with
  the motor off pays the same catch-up (up to 434 us at 64 frames); the motor
  had moved it to the end of the start.
- **The rest is small.** The hand-back pass costs 4 to 10 us over a block at
  rest (one more stream through the resampler and the crossfade). Waking the
  resting lanes on the way in and out costs nothing measurable here, the bench
  desk's resting lanes being empty; on a 256-lane desk whose 240 resting lanes
  each carry a plate, a wake costs up to 22 us in its block, about what that
  desk pays in every block to carry them, so it is left as it is. The motor's
  plan, its stage, the hold and `clear_varispeed` are each under a
  microsecond, and `max_callback_frames` halves a cap of 8192 frames, which no
  64 or 128-frame block reaches.
- **A ramp costs what the dial costs at its speed.** A ramp's pass renders its
  speed's share of program, so it climbs from almost nothing to the cost of a
  block at rest (991 lane-frames, 65 us, between 0.9 and full speed).

**The fix: the motor's way back into the grain render lets the desk in a share
at a time** (`Renderer::regrain_hold`). After the motor alone has left the
varispeed path, a lane with something to render and nothing ahead renders its
chunk until it is let in, a quarter of the desk's work in extra work a chunk
and at least one lane; a lane let in renders its next grain when its ring runs
out, so the lanes let in one after another refill one after another. On the
bench desk that is one lane a chunk: the first block back renders 1486
lane-frames, 87 us at 64 frames, the desk's chunks and one lane's grain, and
the excess falls away chunk by chunk until all sixteen are back sixteen chunks
later. One lane's grain is the least any way back can pay, since some lane has
to be first; on a wider desk it is a smaller share (a quarter at most).

Renders with the motor off, parked or at speed are the same bits as 5.2 over a
grid of 432 (44.1, 48 and 96 kHz; 32 to 256-frame blocks; grains off, auto and
240 frames; plate, compressor, drive and Tape). With ramps, the first stop, the
start and the stretch at speed after it are the same bits too; a later stop's
tail is as loud (within 0.014 dB, 0.69 dB in the worst tenth of a second) and
its reverb takes a different path, as every stop on a grained desk already does
against an ungrained one: how far ahead each lane had rendered when that stop
came is what changed, for a lane the way back let in never more than a grain
where it was up to two.

**The design's bar, 1.10 times the dial's p99 and 1.25 times its worst, is half
out of reach.** The dial at 0.75 renders three quarters of the program a block
at rest renders, so rest itself runs 1.14 to 1.26 times the dial's p99, and a
third of a cycle's blocks are at rest (at speed or parked), its dearest ramp
blocks near full speed. The worst block now sits on the design's bar, either
side of it from run to run, and at rest's own. The probe holds what was reached,
with a margin: the median p99 at most 1.75 times the dial's and 1.4 times rest's,
the median worst at most 1.6 times the dial's and 1.35 times rest's, and the
deadline bars it had (p99 at most 15 %, worst at most 50 %). The 5.2 engine
fails the worst-block bars by two to four times in every run.
