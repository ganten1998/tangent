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
/// shared between them is what keeps them sample-aligned; see [`Varispeed`].
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
    /// frame that will be rendered. In `(-(HIST as f64), 1.0)`.
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
    /// position's floor plus three. It is never negative and never above the
    /// bound in `Varispeed::MAX_PROGRAM_FRAMES`.
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

    /// Advance past a block: `dst` output frames were taken and `src`
    /// program frames were rendered.
    ///
    /// The result is usually NEGATIVE — more was rendered than consumed,
    /// because the count above rounds up — which is why the streams keep a
    /// history and why `at` accepts a negative index. Clamped so it can never
    /// reach further back than there is history to reach into: a phase that
    /// did would read zeros and put a hole in the audio, and the only way to
    /// get there is a ratio outside the dial's own limits.
    pub fn advance(&mut self, dst: usize, src: usize, ratio: f64) {
        let moved = self.frac + dst as f64 * ratio - src as f64;
        self.frac = if moved.is_finite() {
            moved.clamp(-(HIST as f64) + 1.0, 1.0)
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
}
