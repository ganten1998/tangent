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
