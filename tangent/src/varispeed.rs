//! True varispeed: the whole desk played faster or slower, with the pitch
//! following, by running the program clock at a different rate from the
//! device's and resampling at the boundary.
//!
//! # Why this and not something cheaper
//!
//! There are three ways to make a DAW play fast and this is the expensive
//! one. The other two do not work here:
//!
//! - **Read the clips faster.** Clip playback is already a resample, so this
//!   is nearly free — and it moves nothing else. A hosted synth would keep
//!   its pitch while the audio around it rose, which is not varispeed; it is
//!   a bug with a knob on it.
//! - **Tell everything the sample rate is different.** Exactly right, and
//!   completely correct for the built-in voice, the click and the clips. It
//!   cannot be done to a VST3: a plugin's rate is fixed at activation, so
//!   changing it means deactivate, re-activate and lose its state — a click
//!   and a reset every time the dial moves.
//!
//! So the program runs at `rate × ratio` and what leaves is resampled back to
//! the device's rate. Everything in the mix moves together, plugins included,
//! because none of them knows anything has happened.
//!
//! # Which way round
//!
//! At 115%, one second of device output needs 1.15 seconds of program. The
//! renderer is asked for MORE frames than the device wants and they are
//! squeezed down, which is what makes it faster and sharper.
//!
//! The live input goes the other way for the same reason: `frames` real
//! frames arrive per callback and `frames × ratio` are needed, so it is
//! STRETCHED on the way in. The two cancel for a player listening to
//! themselves, and that is correct rather than a cop-out — the tape is what
//! is running fast, not the room. It is also the only arrangement that does
//! not starve the input ring: at 115% a desk that read the input at program
//! rate would ask for fifteen percent more than the interface ever delivers,
//! every callback, for ever.
//!
//! # On the audio thread
//!
//! Every buffer here is allocated once, at construction. Nothing here
//! allocates, locks, or panics.

/// How many past frames each stream keeps.
///
/// Four-point interpolation needs one frame before the read position and two
/// after, so ONE would do for a read that never falls behind the block it is
/// reading. It can: the number of program frames rendered is rounded UP to
/// cover the last read, so the next block usually starts a frame or three
/// *inside* what was already rendered, at a negative index. Four covers that
/// with room, and the plan below refuses to hand out a position that would
/// reach past it.
const HIST: usize = 4;

/// The fastest the dial can be set to, as a ratio. Mirrors
/// `tangent_ui::varispeed::HARD_MAX`; it is written down here because the
/// buffer sizes depend on it and a mismatch would be an overrun rather than a
/// wrong number.
pub const MAX_RATIO: f64 = 2.0;

/// Four-point, third-order Hermite. **The interpolator, and it matters
/// which.**
///
/// Linear interpolation is two multiplies and sounds like a tape machine with
/// a blanket over it: its error is a low-pass whose corner moves with the
/// ratio, so the top of the mix dulls and undulls as the dial turns. Hermite
/// is four multiplies and is flat enough across the audio band that what you
/// hear is the resampling and not the arithmetic.
#[inline]
fn hermite(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c0 = y1;
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + c0
}

/// One stream's interpolation state.
///
/// A stream per destination — the device output, the take's tap, the
/// scope-and-ReaStream pair, the live input — because each carries a
/// different signal and needs its own tail. **The phase is NOT here.** All
/// the output-side streams are read at the same positions, and one phase
/// shared between them is what keeps them sample-aligned; see [`Phase`].
#[derive(Debug)]
pub struct Stream {
    hist: Vec<f32>,
    next: Vec<f32>,
    channels: usize,
}

impl Stream {
    pub fn new(channels: usize) -> Self {
        let ch = channels.max(1);
        Self {
            hist: vec![0.0; HIST * ch],
            next: vec![0.0; HIST * ch],
            channels: ch,
        }
    }

    /// One sample from the source, where index 0 is `src`'s first frame and
    /// negative indices reach into the history.
    ///
    /// Past the END clamps to the last frame rather than reading zero: a
    /// read that runs a fraction past what was rendered should hold the
    /// waveform, not put a step in it.
    #[inline]
    fn at(&self, src: &[f32], src_frames: usize, i: isize, ch: usize) -> f32 {
        if i >= 0 {
            if src_frames == 0 {
                return 0.0;
            }
            let i = (i as usize).min(src_frames - 1);
            src.get(i * self.channels + ch).copied().unwrap_or(0.0)
        } else {
            // −1 is the newest history frame, −HIST the oldest.
            let back = (-i) as usize;
            if back > HIST {
                return 0.0;
            }
            self.hist
                .get((HIST - back) * self.channels + ch)
                .copied()
                .unwrap_or(0.0)
        }
    }

    /// Resample `src_frames` frames into `dst_frames`, reading at
    /// `frac + j·ratio`, and keep the tail for the next block.
    pub fn run(
        &mut self,
        src: &[f32],
        src_frames: usize,
        dst: &mut [f32],
        dst_frames: usize,
        frac: f64,
        ratio: f64,
    ) {
        for j in 0..dst_frames {
            let x = frac + j as f64 * ratio;
            let floor = x.floor();
            let t = (x - floor) as f32;
            let i = floor as isize;
            for ch in 0..self.channels {
                let y0 = self.at(src, src_frames, i - 1, ch);
                let y1 = self.at(src, src_frames, i, ch);
                let y2 = self.at(src, src_frames, i + 1, ch);
                let y3 = self.at(src, src_frames, i + 2, ch);
                let Some(slot) = dst.get_mut(j * self.channels + ch) else {
                    return;
                };
                *slot = hermite(y0, y1, y2, y3, t);
            }
        }
        self.keep_tail(src, src_frames);
    }

    /// **Resample with a ratio that moves**: `run`, with the ratio linear in
    /// output time from `r0` at the first frame towards `r1`, `r_j = r0 +
    /// (r1 - r0) j / dst_frames`, so the read position is its exact sum,
    /// `x_j = frac + j r0 + j (j - 1) (r1 - r0) / (2 dst_frames)`.
    ///
    /// **Per sample, not a staircase.** The tape's motor changes the speed
    /// every pass, and one ratio a call steps it by whole percent of full
    /// speed at a time: at 512 frames and 48 kHz a 0.5 s start moves 2.6 %
    /// a callback, a 0.85-semitone jump every 10.7 ms at half speed (a
    /// 94 Hz zipper), and near the bottom whole octaves a callback. With
    /// equal ends the correction is exactly zero and this is `run`, bit for
    /// bit.
    #[allow(clippy::too_many_arguments)]
    pub fn run_ramp(
        &mut self,
        src: &[f32],
        src_frames: usize,
        dst: &mut [f32],
        dst_frames: usize,
        frac: f64,
        r0: f64,
        r1: f64,
    ) {
        let bend = (r1 - r0) / (2.0 * dst_frames.max(1) as f64);
        for j in 0..dst_frames {
            let jf = j as f64;
            let x = frac + jf * r0 + jf * (jf - 1.0) * bend;
            let floor = x.floor();
            let t = (x - floor) as f32;
            let i = floor as isize;
            for ch in 0..self.channels {
                let y0 = self.at(src, src_frames, i - 1, ch);
                let y1 = self.at(src, src_frames, i, ch);
                let y2 = self.at(src, src_frames, i + 1, ch);
                let y3 = self.at(src, src_frames, i + 2, ch);
                let Some(slot) = dst.get_mut(j * self.channels + ch) else {
                    return;
                };
                *slot = hermite(y0, y1, y2, y3, t);
            }
        }
        self.keep_tail(src, src_frames);
    }

    /// The last [`HIST`] frames of what was just read.
    ///
    /// Built into a second buffer and then swapped in, because a block
    /// shorter than the history reads FROM the history while writing to it —
    /// and doing that in place would fold a frame onto itself.
    fn keep_tail(&mut self, src: &[f32], src_frames: usize) {
        for row in 0..HIST {
            let i = src_frames as isize - (HIST - row) as isize;
            for ch in 0..self.channels {
                let v = self.at(src, src_frames.max(1), i, ch);
                // Nothing but the source and the old history is read above,
                // so this write cannot disturb either.
                if let Some(slot) = self.next.get_mut(row * self.channels + ch) {
                    *slot = if i >= src_frames as isize { 0.0 } else { v };
                }
            }
        }
        self.hist.copy_from_slice(&self.next);
    }

    /// The interleave this stream reads and writes: its source's and its
    /// destination's stride, fixed at construction.
    ///
    /// **The input's width is not known until a device is chosen**, and it
    /// changes when somebody swaps interfaces mid-session, so the input's
    /// stretch is built for its ring, on the host, and arrives with it
    /// (`lane_bank::MonitorPlan::vs_in`). It used to be re-widened in place
    /// on the audio thread, which allocated there.
    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn clear(&mut self) {
        self.hist.fill(0.0);
        self.next.fill(0.0);
    }
}

/// The output side's phase, shared by every stream that leaves.
///
/// One accumulator, because the device output, the take's tap and the scope's
/// pair are three views of the same instant and reading them at different
/// positions would put the recording a fraction of a frame away from what
/// came out of the speakers.
pub struct Phase {
    /// Where the next output frame reads from, relative to the next program
    /// frame that will be rendered. In `(-(HIST as f64), max(ratio, 1.0)]`:
    /// at most 1 on the output side, where the ratio is at most 2 and the
    /// count rounds up, and up to a step ahead on the input side's large
    /// steps near the tape motor's floor (see [`Phase::advance`]).
    frac: f64,
}

impl Phase {
    pub fn new() -> Self {
        Self { frac: 0.0 }
    }

    pub fn frac(&self) -> f64 {
        self.frac
    }

    /// **How many program frames are needed to produce `dst` output frames**,
    /// at this ratio and from where the phase now stands.
    ///
    /// The last output frame reads at `frac + (dst-1)·ratio` and the
    /// interpolator wants two frames past that, so the answer is that
    /// position's floor plus three. It is never negative, and for a callback
    /// the renderer's cap allows never above its `MAX_CALLBACK_FRAMES`.
    pub fn program_frames(&self, dst: usize, ratio: f64) -> usize {
        if dst == 0 {
            return 0;
        }
        let last = self.frac + (dst - 1) as f64 * ratio;
        let need = last.floor() + 3.0;
        if need <= 0.0 {
            0
        } else {
            need as usize
        }
    }

    /// **How many program frames a ramped read needs**: `program_frames`
    /// for [`Stream::run_ramp`], the floor of its last read position,
    /// `x_(dst-1)`, plus three. With equal ends it is `program_frames`, bit
    /// for bit.
    pub fn program_frames_ramp(&self, dst: usize, r0: f64, r1: f64) -> usize {
        if dst == 0 {
            return 0;
        }
        let j = (dst - 1) as f64;
        let last = self.frac + j * r0 + j * (j - 1.0) * ((r1 - r0) / (2.0 * dst as f64));
        let need = last.floor() + 3.0;
        if need <= 0.0 {
            0
        } else {
            need as usize
        }
    }

    /// Advance past a block: `dst` output frames were taken and `src`
    /// program frames were rendered.
    ///
    /// The result is usually NEGATIVE — more was rendered than consumed,
    /// because the count above rounds up — which is why the streams keep a
    /// history and why `at` accepts a negative index. Clamped so it can never
    /// reach further back than there is history to reach into: a phase that
    /// did would read zeros and put a hole in the audio, and the only way to
    /// get there is a ratio outside the dial's own limits.
    ///
    /// **And never further ahead than one step** (`ratio.max(1.0)`), which
    /// only the input side reaches. There the ratio is a STEP, real frames
    /// per program frame, and the result lies in `[step - 3, step - 2)`:
    /// never above zero for the dial's steps (at most 2), but up to 125
    /// frames ahead near the tape motor's floor, where a step is up to 127.
    /// That lead is a read position, not slack: clamped to 1, as it was, it
    /// threw away `step - 4` real frames and more every chunk. A NaN ratio
    /// is 1 to `f64::max`, so the clamp's bounds are always in order.
    pub fn advance(&mut self, dst: usize, src: usize, ratio: f64) {
        let moved = self.frac + dst as f64 * ratio - src as f64;
        self.frac = if moved.is_finite() {
            moved.clamp(-(HIST as f64) + 1.0, ratio.max(1.0))
        } else {
            0.0
        };
    }

    /// `advance` past a ramped block: the read position one past its last
    /// frame, `x_dst = frac + dst r0 + (dst - 1) (r1 - r0) / 2`, less the
    /// program frames rendered, held to the history behind and the larger
    /// end's step ahead. With equal ends it is `advance`, bit for bit.
    pub fn advance_ramp(&mut self, dst: usize, src: usize, r0: f64, r1: f64) {
        let n = dst as f64;
        let moved = self.frac + n * r0 + (n - 1.0) * (r1 - r0) / 2.0 - src as f64;
        self.frac = if moved.is_finite() {
            moved.clamp(-(HIST as f64) + 1.0, r0.max(r1).max(1.0))
        } else {
            0.0
        };
    }

    /// Back to the top. A locate, a device change, a dial that has just come
    /// back to centre: in every case the next block is not continuous with
    /// the last one and pretending otherwise puts a click in it.
    pub fn reset(&mut self) {
        self.frac = 0.0;
    }
}

impl Default for Phase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ramp, so an interpolated value can be checked against what it OUGHT
    /// to be rather than against itself.
    fn ramp(frames: usize, ch: usize, from: f32) -> Vec<f32> {
        (0..frames * ch)
            .map(|i| from + (i / ch) as f32)
            .collect()
    }

    #[test]
    fn a_ratio_of_one_is_the_identity() {
        // **The property everything else rests on.** The dial's detent puts
        // it at exactly 1.0, the engine skips the whole path there, and this
        // is what says the two agree: if unity through the resampler were not
        // the same samples out as in, then the moment the path switched off
        // there would be a step.
        let mut s = Stream::new(2);
        let ph = Phase::new();
        let src = ramp(64, 2, 0.0);
        let mut dst = vec![0.0; 64 * 2];
        let n = ph.program_frames(64, 1.0);
        assert!(n >= 64);
        s.run(&src, 64.min(n), &mut dst, 64, ph.frac(), 1.0);
        // From frame 1: frame 0's window reaches into an empty history. See
        // `a_ramp_resamples_to_a_ramp`.
        for j in 1..60 {
            assert!(
                (dst[j * 2] - src[j * 2]).abs() < 1e-4,
                "frame {j}: {} against {}",
                dst[j * 2],
                src[j * 2]
            );
        }
    }

    #[test]
    fn a_ramp_resamples_to_a_ramp() {
        // A straight line is the case an interpolator cannot fudge: every
        // read lands exactly on `frac + j·ratio` or it does not.
        for ratio in [0.5_f64, 0.85, 1.15, 2.0] {
            let mut s = Stream::new(1);
            let src = ramp(256, 1, 0.0);
            let mut dst = vec![0.0; 64];
            s.run(&src, 200, &mut dst, 64, 0.0, ratio);
            for j in 0..64 {
                // **Skip the frames whose window reaches before the source.**
                // Four-point interpolation reads one frame BEHIND the read
                // position; at the very start of a fresh stream that frame is
                // the history, which is silence, and a curve drawn through a
                // zero and then the ramp is not the ramp. That is the
                // interpolator being right — the signal really did start
                // there — and `two_blocks_join_without_a_step` is what proves
                // the history works once there is one.
                if (j as f64 * ratio).floor() < 1.0 {
                    continue;
                }
                let want = (j as f64 * ratio) as f32;
                assert!(
                    (dst[j] - want).abs() < 1e-3,
                    "ratio {ratio}, frame {j}: {} against {want}",
                    dst[j]
                );
            }
        }
    }

    /// **The seam between two blocks.**
    ///
    /// The one thing that cannot be tested inside a single call, and the one
    /// most likely to be wrong: a block boundary the phase does not carry
    /// across is a discontinuity every 5 ms, which is a buzz at 200 Hz on top
    /// of the music.
    #[test]
    fn two_blocks_join_without_a_step() {
        for ratio in [0.5_f64, 0.85, 1.15, 2.0] {
            let mut s = Stream::new(1);
            let mut ph = Phase::new();
            // One long ramp, read in two halves, against the same ramp read
            // in one go.
            let src = ramp(4096, 1, 0.0);
            let mut whole = vec![0.0; 200];
            {
                let mut s1 = Stream::new(1);
                s1.run(&src, 4096, &mut whole, 200, 0.0, ratio);
            }
            let mut out = Vec::new();
            let mut consumed = 0usize;
            for _ in 0..2 {
                let n = ph.program_frames(100, ratio);
                let mut part = vec![0.0; 100];
                s.run(&src[consumed..], n, &mut part, 100, ph.frac(), ratio);
                ph.advance(100, n, ratio);
                consumed += n;
                out.extend_from_slice(&part);
            }
            for j in 0..200 {
                assert!(
                    (out[j] - whole[j]).abs() < 1e-3,
                    "ratio {ratio}, frame {j}: two blocks gave {}, one gave {}",
                    out[j],
                    whole[j]
                );
            }
        }
    }

    /// **The rate is the ratio**, which is what makes it varispeed and not a
    /// time-stretch: a tone in comes out at `ratio` times the frequency.
    #[test]
    fn the_pitch_follows_the_speed() {
        for ratio in [0.85_f64, 1.15, 2.0] {
            let rate = 48_000.0_f64;
            let hz = 500.0_f64;
            let mut s = Stream::new(1);
            let mut ph = Phase::new();
            let mut out: Vec<f32> = Vec::new();
            let mut at = 0usize;
            for _ in 0..40 {
                let n = ph.program_frames(256, ratio);
                let src: Vec<f32> = (0..n)
                    .map(|i| {
                        (std::f64::consts::TAU * hz * (at + i) as f64 / rate).sin() as f32
                    })
                    .collect();
                let mut part = vec![0.0; 256];
                s.run(&src, n, &mut part, 256, ph.frac(), ratio);
                ph.advance(256, n, ratio);
                at += n;
                out.extend_from_slice(&part);
            }
            // Count zero crossings over the settled part, which gives the
            // frequency without needing an FFT.
            let tail = &out[2_048..];
            let crossings = tail
                .windows(2)
                .filter(|w| (w[0] <= 0.0) != (w[1] <= 0.0))
                .count();
            let seconds = tail.len() as f64 / rate;
            let got = crossings as f64 / 2.0 / seconds;
            let want = hz * ratio;
            assert!(
                (got - want).abs() / want < 0.02,
                "ratio {ratio}: {got:.1} Hz where {want:.1} was wanted"
            );
        }
    }

    /// The count is enough, and not wastefully more.
    #[test]
    fn the_plan_asks_for_what_it_needs() {
        let ph = Phase::new();
        for ratio in [0.5_f64, 1.0, 1.15, 2.0] {
            for dst in [1_usize, 64, 256, 4_092] {
                let n = ph.program_frames(dst, ratio);
                let last = ph.frac() + (dst - 1) as f64 * ratio;
                // Every read, including the interpolator's two-frame reach,
                // is inside what was rendered.
                assert!(
                    (n as f64) >= last + 3.0 - 1.0,
                    "ratio {ratio}, {dst} frames: {n} rendered is not enough"
                );
                // ...and never more than a handful past it, or the engine is
                // doing work nobody hears.
                assert!(
                    (n as f64) <= last + 4.0,
                    "ratio {ratio}, {dst} frames: {n} rendered is wasteful"
                );
            }
        }
    }

    /// **The phase never reaches past the history**, whatever it is fed.
    ///
    /// A read at −5 with four frames of history is four zeros and a hole in
    /// the audio. Nothing the dial can do gets there; this is what makes that
    /// true rather than likely.
    #[test]
    fn the_phase_stays_inside_the_history() {
        let mut ph = Phase::new();
        for ratio in [0.5_f64, 1.0, 2.0, MAX_RATIO] {
            for _ in 0..1_000 {
                let n = ph.program_frames(256, ratio);
                ph.advance(256, n, ratio);
                assert!(
                    ph.frac() > -(HIST as f64) && ph.frac() <= 1.0,
                    "ratio {ratio} walked the phase to {}",
                    ph.frac()
                );
            }
        }
        // And nonsense does not poison it: a NaN frac is a NaN read position,
        // which is silence at best and a panic in an index at worst.
        ph.advance(256, 0, f64::NAN);
        assert!(ph.frac().is_finite());
    }

    /// Silence in, silence out — including across the seam, where a history
    /// left over from a previous ratio could otherwise leak.
    #[test]
    fn a_reset_leaves_nothing_behind() {
        let mut s = Stream::new(2);
        let loud = vec![0.9_f32; 64 * 2];
        let mut dst = vec![0.0; 64 * 2];
        s.run(&loud, 64, &mut dst, 64, 0.0, 1.0);
        s.clear();
        let quiet = vec![0.0_f32; 64 * 2];
        s.run(&quiet, 64, &mut dst, 64, 0.0, 1.0);
        assert!(
            dst.iter().all(|v| v.abs() < 1e-6),
            "the old block bled through a clear"
        );
    }

    /// The read map of a ramped block: where output frame `j` reads, from a
    /// phase of `frac`, with the ratio linear from `r0` across `dst` frames.
    fn read_at(frac: f64, j: usize, dst: usize, r0: f64, r1: f64) -> f64 {
        let j = j as f64;
        frac + j * r0 + j * (j - 1.0) * (r1 - r0) / (2.0 * dst as f64)
    }

    /// **A ramp with equal ends is the constant ratio, bit for bit.**
    ///
    /// The renderer takes the ramped calls only while the tape's motor is
    /// changing speed and the constant ones otherwise, so the dial alone
    /// runs today's arithmetic; and where the two meet (a motor at a steady
    /// speed under a ramp's road) they must be the same samples, the same
    /// count and the same phase, or the switch between them is a step.
    /// Held over three blocks from four phases the dial leaves behind, so
    /// the history each keeps is compared too.
    #[test]
    fn a_ramp_with_equal_ends_is_the_constant_ratio_bit_for_bit() {
        let src: Vec<f32> = (0..8192 * 2).map(|i| ((i / 2) as f32 * 0.37).sin() * if i % 2 == 0 { 1.0 } else { -0.5 }).collect();
        for ratio in [0.5_f64, 0.85, 1.0, 1.15, 2.0] {
            for frac in [0.0_f64, -1.75, -0.4, 0.6] {
                let (mut a, mut b) = (Phase { frac }, Phase { frac });
                for dst in [1_usize, 64, 256, 4_092] {
                    assert_eq!(a.program_frames(dst, ratio), b.program_frames_ramp(dst, ratio, ratio), "ratio {ratio}, {dst} frames");
                }
                let (mut s1, mut s2) = (Stream::new(2), Stream::new(2));
                let mut at = 0usize;
                for block in 0..3 {
                    let n = a.program_frames(256, ratio);
                    assert_eq!(n, b.program_frames_ramp(256, ratio, ratio));
                    let (mut d1, mut d2) = (vec![0.0f32; 512], vec![0.0f32; 512]);
                    s1.run(&src[at * 2..], n, &mut d1, 256, a.frac(), ratio);
                    s2.run_ramp(&src[at * 2..], n, &mut d2, 256, b.frac(), ratio, ratio);
                    assert!(
                        d1.iter().zip(&d2).all(|(x, y)| x.to_bits() == y.to_bits()),
                        "ratio {ratio} from {frac}: block {block} differs between run and run_ramp"
                    );
                    a.advance(256, n, ratio);
                    b.advance_ramp(256, n, ratio, ratio);
                    assert_eq!(a.frac().to_bits(), b.frac().to_bits(), "ratio {ratio}: the phases parted");
                    at += n;
                }
            }
        }
    }

    /// **A ramped read lands on the integrated position.**
    ///
    /// A ramp source is the case an interpolator cannot fudge, as in
    /// `a_ramp_resamples_to_a_ramp`: frame `j` must read exactly the read
    /// map's `x_j`, the sum of every ratio before it, or the speed the tape
    /// plays is not the speed the motor decided. Each frame's step from the
    /// last is the ratio itself, linear across the block, and the count asked
    /// for covers the last read with the interpolator's reach and no more.
    #[test]
    fn a_ramped_read_lands_on_the_integrated_position() {
        let (r0, r1) = (0.2_f64, 0.6_f64);
        let src = ramp(512, 1, 0.0);
        let n = Phase::new().program_frames_ramp(256, r0, r1);
        let last = read_at(0.0, 255, 256, r0, r1);
        assert!((n as f64) >= last + 2.0 && (n as f64) <= last + 4.0, "{n} program frames for a last read at {last}");
        let mut s = Stream::new(1);
        let mut dst = vec![0.0f32; 256];
        s.run_ramp(&src, n, &mut dst, 256, 0.0, r0, r1);
        for j in 0..256 {
            let want = read_at(0.0, j, 256, r0, r1);
            // Skip the frames whose window reaches the empty history.
            if want.floor() < 1.0 {
                continue;
            }
            assert!((f64::from(dst[j]) - want).abs() < 1e-3, "frame {j}: read {} where {want} was the position", dst[j]);
            if read_at(0.0, j - 1, 256, r0, r1).floor() >= 1.0 {
                let ratio = r0 + (r1 - r0) * (j - 1) as f64 / 256.0;
                assert!((f64::from(dst[j] - dst[j - 1]) - ratio).abs() < 1e-3, "frame {j}: the step is not the ratio");
            }
        }
    }

    /// **Two ramped blocks join without a step.**
    ///
    /// The seam a single call cannot show: one continuous change of speed
    /// read in two passes, each with its own two ends, must be the same
    /// samples as the one read, or every pass boundary of a start or a stop
    /// is a click. Across a rise and a fall, and down to the motor's floor.
    #[test]
    fn two_ramped_blocks_join_without_a_step() {
        let floor = crate::motor::FLOOR;
        for (ra, rb) in [(0.3_f64, 0.9_f64), (0.9, 0.3), (0.05, 1.0), (1.0, floor)] {
            let src = ramp(4096, 1, 0.0);
            let mut whole = vec![0.0f32; 200];
            Stream::new(1).run_ramp(&src, 4096, &mut whole, 200, 0.0, ra, rb);
            let mid = ra + (rb - ra) * 0.5;
            let (mut ph, mut s) = (Phase::new(), Stream::new(1));
            let (mut out, mut consumed) = (Vec::new(), 0usize);
            for (r0, r1) in [(ra, mid), (mid, rb)] {
                let n = ph.program_frames_ramp(100, r0, r1);
                let mut part = vec![0.0f32; 100];
                s.run_ramp(&src[consumed..], n, &mut part, 100, ph.frac(), r0, r1);
                ph.advance_ramp(100, n, r0, r1);
                consumed += n;
                out.extend_from_slice(&part);
            }
            for j in 0..200 {
                assert!(
                    (out[j] - whole[j]).abs() < 1e-3,
                    "{ra} to {rb}, frame {j}: two blocks gave {}, one gave {}",
                    out[j],
                    whole[j]
                );
            }
        }
    }

    /// **The phase keeps its place down to the motor's floor**, on both
    /// sides of the path.
    ///
    /// The output side plays the desk at ratios down to the floor with the
    /// dial at its slowest (`0.5 x FLOOR`, 0.00786), where a pass may need
    /// no program at all; its phase stays inside the history and never
    /// passes 1. The input side STRETCHES by the inverse, steps up to 127.2
    /// real frames per program frame, and there the phase leads by up to
    /// the step: what it consumes over a thousand passes must equal what the
    /// read map moved, the integral of the step, to within that lead and the
    /// interpolator's reach (`max step + 3`). The clamp at 1 it replaces
    /// lost over 120 real frames a chunk at the floor. Constant passes use
    /// the constant calls, as the renderer does, and ramped ones the ramps.
    #[test]
    fn the_phase_keeps_its_place_down_to_the_floor() {
        let slowest = 0.5 * crate::motor::FLOOR;
        let max_step = 1.0 / slowest;
        for frames in [32_usize, 256] {
            // A thousand passes: held at the floor, a rise from it to full
            // speed, a fall back, and held at the floor again.
            let ratio = |k: usize| -> f64 {
                match k / 250 {
                    0 => slowest,
                    1 => slowest + (1.0 - slowest) * (k - 250) as f64 / 250.0,
                    2 => 1.0 - (1.0 - slowest) * (k - 500) as f64 / 250.0,
                    _ => slowest,
                }
            };
            let mut out = Phase::new();
            let mut inp = Phase::new();
            let (mut consumed, mut moved) = (0.0_f64, 0.0_f64);
            for k in 0..1_000 {
                let (r0, r1) = (ratio(k), ratio(k + 1));
                let n = if r0 == r1 { out.program_frames(frames, r0) } else { out.program_frames_ramp(frames, r0, r1) };
                if r0 == r1 {
                    out.advance(frames, n, r0);
                } else {
                    out.advance_ramp(frames, n, r0, r1);
                }
                assert!(out.frac().is_finite() && out.frac() > -(HIST as f64) && out.frac() <= 1.0, "pass {k} of {frames}: the output phase is {}", out.frac());
                let (s0, s1) = (1.0 / r0, 1.0 / r1);
                let real = if s0 == s1 { inp.program_frames(frames, s0) } else { inp.program_frames_ramp(frames, s0, s1) };
                if s0 == s1 {
                    inp.advance(frames, real, s0);
                } else {
                    inp.advance_ramp(frames, real, s0, s1);
                }
                consumed += real as f64;
                moved += frames as f64 * s0 + (frames as f64 - 1.0) * (s1 - s0) / 2.0;
                assert!(inp.frac().is_finite() && inp.frac() > -(HIST as f64) && inp.frac() <= max_step, "pass {k} of {frames}: the input phase is {}", inp.frac());
            }
            assert!(
                (consumed - moved).abs() <= max_step + 3.0,
                "{frames}-frame passes: the input consumed {consumed} real frames where the read map moved {moved:.1}"
            );
        }
    }
}
