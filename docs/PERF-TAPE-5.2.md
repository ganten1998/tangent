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

`tape_start_and_stop_costs_what_the_dial_costs_probe`, sixteen lanes through a
plate, grains on, 48 kHz: while ramping, p99 6 to 9 % of the deadline (the
dial engaged: 4 to 6 %), worst block 32 % at 64 frames and 17 % at 128. The
design's bar was 1.10 times the dial's p99 and 1.25 times its worst; the motor
runs about 1.4 to 2 times the dial's p99 and up to 5 times its worst, because
each ramp engages and leaves the varispeed path and wakes every resting lane on
the way in and out. Safe against the deadline, which is what the bench now
holds (p99 at most 15 %, worst at most 50 %).

The owner shipped both as they are in 5.2 and wants them optimized later.
