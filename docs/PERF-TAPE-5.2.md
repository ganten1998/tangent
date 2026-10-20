# Tape's processor cost in 5.3

Measured 2026-10-19 on the M-series Mac the project is built on, release
build, each bench run alone with `--test-threads=1`. **Every 5.3 figure in
microseconds here was taken on a shared machine**: two other lanes were
compiling and running their suites at the same time, so each is a
ceiling, not the final word, and a quiet machine will read lower. What
decided each change is an interleaved A/B ratio (see "How it was
measured"), which a busy machine moves far less.

## Tangent Tape

| | 5.1 (main 825a91c3) | 5.2 (quiet, the day it shipped) | aa94a2be, where 5.3 started (shared) | 5.3 (shared) |
|---|---|---|---|---|
| `rt_bench` row "16 lanes x Tape", mean per block | 370.5 us (23.2 per insert) | 470.6 us (29.4 per insert) | 372.0 us (23.3 per insert) | 194.8 to 195.1 us (12.2 per insert) |
| `the_tape_costs_what_its_budget_says`, defaults, mean (warmed) | | 20.7 to 20.8 us | 22.5 to 22.8 us | 9.8 to 11.4 us |
| the same, fully engaged, p99 | | 34.7 to 35.8 us | 32.5 to 34.3 us | 17.5 to 20.3 us |

The design's budget was 16 us at the defaults (the bench's bars were 21
and 24 us) and never above 5.1's cost. 5.3 is under all of it: the
sixteen-lane row is 0.53 of 5.1's and 0.52 of the kernel this work
started from, the defaults are under the budget, and the p99 fully
engaged is under its 24 us bar. The base this
work started from (aa94a2be: 5.2 with the sixteen-tap play head and the
filters that glide while a control moves) was measured on the same
machine in the same hour, and that column is the one to compare with: the
5.2 row read 470.6 us the day it shipped, on a quiet machine and another
build of the engine, and the kernel this work started from reads 372.0
here today. The machine was a little slower when 5.3's rows were taken:
the Drive's row, which nothing here touches, read 572.1 us beside the
372.0 and 593.1 and 603.8 beside 5.3's two.

The single-unit bench now holds the mean at the defaults to the design's
16 us and the p99 fully engaged to 30 us (the shared-machine reading with
a margin); tighten them with quiet numbers, never raise them.

5.2 shipped with this cost as an open issue, to be optimized later; for
the Tape itself, 5.3 closes it.

## What 5.3 changed

**The record stage runs a chunk at a time** (`units::tape::record`). It
was about three quarters of what a Tape cost. Its polyphase pair ran a sample at
a time: every sample was pushed into a history newest first and the
whole window read back at once, before the processor could hand the load
the sample just stored, and every dot product waited on the one before.
Compiled, the dot products were unrolled into scalar multiply-adds with
the taps spilled to the stack, about one multiply-add a cycle, which is
why the polyphase pair's 226 multiply-adds cost what the old direct
form's 484 did.

`Tape::process` now runs a block as chunks that end at the next design
(32 frames) or at the end of the block, each in two halves around the
record stage. The first half runs frame by frame as far as the record
stage (the moving designs' turn, the glides, the heads, the transformer,
the erasure's envelope, the NAB and under-bias shelves) and leaves what
the second half needs of each frame in a `Cue`. The record stage then
takes the whole chunk on both channels: the interpolators and the
decimators as block filters (`fir`, `fir_halves`) whose outputs, eight at
a time, sit in the lanes of two vectors, and each channel's four
sub-phases as eight curves side by side a frame. The second half runs
frame by frame from there. Every output keeps its eight partial sums and
their tree, so not a bit of the sound moved.

How the filters are written mattered as much as what they compute. The
same arithmetic written as one array of eight lanes and inlined had every
tap hoisted out of every loop and spilled: 32 outputs of an 80-tap filter
took 348 ns, where the shape that shipped takes 56, within a tenth of the
processor's own vector instructions written by hand. And the 2x
decimator's first version picked each tap's half of its input with an
index worked out at run time, which kept its sums in memory; written as
one even and one odd sum a step, it fell from 18 % of a Tape's time to
4 %.

**The play head's dot product holds its eight sums as two halves of
four**, the same arithmetic in the same order. Held as one array of
eight, the compiler paired each sum of the read's two rows into a vector
of two lanes and shuffled every tap into place.

**Both are bit-exact.** A grid of 1408 renders outside the tree (every
control at its ends and between, each speed, both ranges, NaN and
out-of-range params, automation on the 4096-frame grid and inside blocks,
8 to 192 kHz, blocks of 1, 7, 128, 333 and 4096 frames, mono, stereo and a
wider stride, `clear()` mid-render, silence, impulses and non-finite
samples in the input) plays 42.4 million samples a side, bit for bit the
same as aa94a2be, on this machine and on the x86-64 baseline (built for
x86_64-apple-darwin and run under Rosetta, where `fma` is a multiply and
an add). A test in the tree holds the chunked record stage to the
sample-at-a-time pair, kept in the tests as its reference, bit for bit.

## How it was measured

A harness outside the tree includes aa94a2be's kernel byte for byte and
this tree's by path, and alternates the two on one thread at
user-interactive QoS (so it stays on a performance core), in rounds of
2000 blocks of 128 stereo frames at 48 kHz each, the order swapped every
round; each round gives a ratio of thread CPU time, and the median of the
ratios decides. Two kernels placed differently on the stack read up to 4 %
apart while running the same code, so both are placed alike on the heap,
and a kernel then reads within 1 % of itself (0.996 to 1.008).
Changes of a few percent were measured again with loops and functions
aligned to 64 bytes in both, to keep where the code lands out of it.

| change | defaults | fully engaged |
|---|---|---|
| the record stage a chunk at a time, against aa94a2be | 0.446 (IQR 0.443 to 0.451) | 0.523 (0.519 to 0.527) |
| the dot product as two halves, against the line above | 0.982 (0.976 to 0.988) | 0.992 (0.986 to 0.997) |
| 5.3, against aa94a2be (two runs of 100 rounds) | 0.436 and 0.447 (IQR 0.432 to 0.443, 0.435 to 0.457) | 0.543 and 0.535 (0.535 to 0.573, 0.521 to 0.544) |

## Measured and not kept

- **The per-frame record level worked out once a chunk while it stands
  still** (its `exp2`, the division and the line-up table), and the wear
  step's sine, tangent and power skipped at Age 0 and Damage 0: bit-exact,
  but 0.980 at the defaults and 1.034 fully engaged, so not kept. A memo of
  the same numbers read 0.978 and 1.030.
- **The play head's read in the processor's own vector instructions**:
  0.936 at the defaults and 0.957 fully engaged, bit-exact, but it needs
  unsafe loads in a unit that has none, for a gain no target needs.
  Inlining the read changed nothing (1.002 and 1.007).
- **Skipping exact wires**: at 30 ips the NAB pair designs to a section
  that passes a negative zero as a positive one and turns an infinity into
  a NaN state, so a skip is not the same bits in silence; at Bump 0 the
  contour's bells add nothing but their states run on, and a skip would
  freeze them. Neither is a wire.
- **A vector-friendly `tanh`** (not bit-exact) was not tried: every target
  is met with the sound unchanged.

## Where the time goes now

A sampling profile of 5.3 at the defaults: what is inlined into
`Tape::process` (the two frame-by-frame halves with the transport and
the heads, and the record stage's own bookkeeping around its filters and
its curve) about 47 %; the record stage's curve, 1024 calls of the
library's `tanh` a block, 18 %; the record stage's three filters 17 %;
the play head's reads 10 %; the record level's `exp2` and the wear
step's `sin`, `tan` and power most of the rest. The curve is the largest
single part left, and only a different `tanh` would make it cheaper.

## Tape start and stop

Measured 2026-10-19, release build, `tape_start_and_stop_costs_what_the_dial_costs_probe`:
the bench's sixteen lanes through a plate, grains on, 48 kHz, rolling at rest,
rolling with the dial at 0.75, and starts and stops at 0.1 s each way, the three
interleaved a tape cycle at a time over 31 rounds on a user-interactive thread.
Each ratio is the median of the rounds' own ratios, so the machine's other work,
which lands on all three alike, falls out. The ranges are three runs of each
engine in one sitting on a quiet machine (load about 4.5), alternated with the
way back's first draft (below).

| cycles against | 5.2 (aa94a2be) | 5.3 |
|---|---|---|
| the dial's worst block, 64 frames | 5.7x | 1.30 to 1.37x |
| the dial's worst block, 128 frames | 3.7x | 1.33 to 1.37x |
| the dial's p99, 64 frames | 1.21 to 1.27x | 1.46 to 1.52x |
| the dial's p99, 128 frames | 1.29 to 1.32x | 1.41 to 1.42x |
| rest's worst block, 64 / 128 frames | 5.0x / 2.9 to 3.1x | 1.17 to 1.19x / 1.13 to 1.14x |
| rest's p99, 64 / 128 frames | 1.00 to 1.06x / 1.05 to 1.09x | 1.25 to 1.28x / 1.16 to 1.17x |
| the deadline, worst block, 64 / 128 frames | 32 to 34 % / 17 % | 8 to 15 % / 7 to 9 % |
| the deadline, p99, 64 / 128 frames | 6.3 to 6.6 % / 8.3 to 8.5 % | 6.9 % / 5.6 to 5.7 % |

The worst blocks against the deadline are all rounds pooled, so a stall of the
machine's lands in them whole. The same probe's desk counted in lane-frames,
which no machine moves: the dearest block of a cycle renders 8384 at 64 frames
and 8448 at 128 in 5.2, against desk chunks of 1024 and 2048; 1664 and 2688 in
5.3.

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

**The fix: the motor's way back into the grain render brings the desk back a
share at a time, and onto the grain clock it would have kept**
(`Renderer::regrain_hold`). After the motor alone has left the varispeed path,
a lane with something to render and nothing ahead is held, if no span can
change its sound: clips through Tangent's own effects. A held lane renders its
chunk as a lane the clock does not grain does, and beside its ring keeps the
ring the clock would have given it, advanced by the clock's own decisions and
never rendered. A turn (`Renderer::regrain_turns`) renders from where its ring
ends to an end of that ring, never longer than the clock's own catch-up, and
the turn that reaches its newest end brings the lane level: its ring ends
where the clock's would and its look-ahead counts from where the clock's
would (`GrainKey::origin`), so from then on the clock plans it as any lane and
decides what it would have decided without the way back. A lane more than a
turn behind takes two, in consecutive chunks. A turn goes only when its chunk
stays within the desk's chunks and one turn more (a quarter of the desk's work
on a wide desk), and the block its phase next refills on does too, or stays
within what the clock itself renders there once every lane is back, the lanes
of the least crowded phase first. On the bench desk the first block back
renders 1472 to 1491 lane-frames where 5.2 rendered 8192 to 8496, no block of
the way back more than 1728, and the way back closes 17 to 19 blocks after the
hand-back, 23 to 25 ms at 64 frames.

**A lane with an instrument or a hosted plugin is not held**, and comes back as
it always did, its whole grain in the first block at rest, so on a desk of
instruments the start costs what it cost in 5.2. The Analog draws its
oscillators' wander once a chunk of its own frame count, so a span cut where
the clock would not cut it plays different dice; a hosted plugin may block
internally.

**The same sound.** Every grid below was rendered against aa94a2be:

- 96 Analog renders (4 and 16 lanes, notes held and notes every 700 frames, 64
  and 128-frame blocks, grains off, auto and 240 frames, and nothing, a locate
  or a stop after the start): every one the same bits.
- 594 audio renders (16 lanes, 12 of them playing, through a plate or Tape; 64,
  128 and 333-frame blocks; grains off, auto and 240 frames; 4 and 40-lane
  desks too; a stop, a locate, the dial, a loop, the motor or the grain switched
  1 to 400 blocks after the motor shows it is at speed): every render with
  nothing happening the same bits, and every event from 60 blocks on.
- The two grids the verifier built for the first draft, 852 and 504 renders
  (44.1, 48 and 96 kHz; 32 to 333-frame blocks; 4 to 40 lanes; plate, Tape, a
  chain, a compressor, drive, echo and dry; starts, stops, locates, loops, dial
  moves, grain and motor switches, and actions 1, 3 or 200 blocks after the
  tape reaches speed): every render the same bits but events 1 or 3 blocks
  after it, and a stop pressed just as the tape reached speed.

The renders that differ are events inside the way back, up to 30 blocks after
the tape reached speed: their held lanes' racks had been through less of the
look-ahead than 5.2's, so the tails take another path, at the same level (in
the 594-render grid, 0.002 dB on median from the first difference on and 0.03
dB at most; in 0.1 s windows above -40 dB re peak 0.07 dB on median and 1.9 dB
at most), and nearer the desk with the look-ahead off in all 74 that have one
to compare with (5.6 dB nearer on median).

**The first draft** let each lane in to a grain of its own and never back onto
the clock: its blocks cost a little less (the dearest of a probe cycle 1484
lane-frames at 64 frames and 2448 at 128, against 1664 and 2688), but every
stop, locate, wrap or dial move after a start played its tails along another
path until the rings next cleared, and an Analog desk played other dice for the
rest of the song, 15 to 37 dB under its notes, with nothing happening at all.
Alternated in the sitting above, bringing every lane back onto the clock costs
about 7 % on the worst block over that draft (1.23 to 1.28x the dial's at 64
frames and 1.24 to 1.27x at 128) and 9 % on the p99 (1.33 to 1.37x and 1.30 to
1.36x), all of it in the 17 to 19 blocks the way back is open.

**The design's bar, 1.10 times the dial's p99 and 1.25 times its worst, is out
of reach.** The dial at 0.75 renders three quarters of the program a block at
rest renders, so rest itself runs about 1.1 to 1.3 times the dial's p99, and a
third of a cycle's blocks are at rest (at speed or parked), its dearest ramp
blocks near full speed; and the probe's cycle is at speed for about 0.05 s, so
most of it is the way back. The worst block sits a little over the design's
1.25, where the first draft sat on it, and near rest's own. The probe holds
what was reached, with a margin: the median p99 at most 1.75 times the dial's
and 1.4 times rest's, the median worst at most 1.6 times the dial's and 1.35
times rest's, and the deadline bars it had (p99 at most 15 %, worst at most 50
%). The 5.2 engine fails the worst-block bars in every run, by 2.2 to 3.7
times.
