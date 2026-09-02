//! The insert tap's UI-side ring: the audio the one open plugin face is
//! looking at.
//!
//! The engine keeps ONE positionable tap — `Engine::set_insert_tap` aims it at
//! a rack seat's bay, and the host drains it once a frame into
//! `TangentApp::insert_tap_feed`, which lands here. This is the ring half of
//! [`crate::scope::Scope`] with the painting left out, because two different
//! faces read it two different ways: the EQ's analyzer wants recent MONO to
//! transform, the imager's vectorscope wants the raw STEREO pairs, and the
//! correlation readout wants one number. Keeping the samples once and
//! answering all three questions is the whole reason this is its own struct
//! rather than a second `Scope`.
//!
//! Pure state, no painting: the faces are painters and take `&self`; all
//! mutation happens in [`TapRing::tick`], which the app calls once a frame
//! with whatever the engine's ring delivered — the same split the scope keeps
//! and for the same reason.

/// Stereo, matching the engine's `TAP_CHANNELS`. The firewall keeps the
/// constant itself out of reach, so this is a duplicated fact, like the
/// scope's: the tap has been stereo for the life of the engine.
const CHANNELS: usize = 2;

/// The rolling window kept, in frames — ~340 ms at 48 kHz, the scope's own
/// figure: enough history for an analyzer FFT of any sensible size plus a
/// vectorscope trail, small enough to never matter.
pub const CAPACITY: usize = 1 << 14;

/// How many frames the correlation readout integrates — ~85 ms at 48 kHz.
///
/// Long enough that a bass note's cycle does not swing the needle, short
/// enough that pulling a width knob answers under the hand. Fixed, because
/// the running sums below are maintained per-sample as audio arrives; a
/// caller-chosen window would make every read a full walk of it.
pub const CORR_WINDOW: usize = 1 << 12;

/// A ring of the most recent tapped stereo frames, plus the running sums
/// that make the correlation readout O(1) per read.
///
/// The sums are `f64` on the house rule: they are design/view arithmetic, not
/// the audio path, and a subtract-what-leaves running sum in `f32` drifts
/// audibly wrong within minutes of program material.
pub struct TapRing {
    /// Interleaved stereo, a ring. `head` is the next write, so the newest
    /// frame is just behind it — the scope's own layout.
    buf: Vec<f32>,
    head: usize,
    filled: usize,
    rate: f32,
    /// Σl·l, Σr·r and Σl·r over the newest [`CORR_WINDOW`] frames, kept by
    /// adding each arriving frame's products and subtracting the products of
    /// the frame that slides out — O(1) per sample, O(1) per read.
    sum_ll: f64,
    sum_rr: f64,
    sum_lr: f64,
    /// Frames currently inside the correlation window, saturating at
    /// [`CORR_WINDOW`]. Separate from `filled` so a `clear` and the warm-up
    /// share one honest count.
    corr_filled: usize,
}

impl TapRing {
    pub fn new(rate: f32) -> Self {
        Self {
            buf: vec![0.0; CAPACITY * CHANNELS],
            head: 0,
            filled: 0,
            rate: rate.max(8_000.0),
            sum_ll: 0.0,
            sum_rr: 0.0,
            sum_lr: 0.0,
            corr_filled: 0,
        }
    }

    /// The rate the samples were tapped at — what turns "frames back" into
    /// seconds and bins into Hz for whichever face is reading.
    pub fn rate(&self) -> f32 {
        self.rate
    }

    /// A device restart changes the rate; the picture survives, the window
    /// math follows — the scope's own bargain.
    pub fn set_rate(&mut self, rate: f32) {
        self.rate = rate.max(8_000.0);
    }

    /// How many frames are held, up to [`CAPACITY`]. A face that wants a
    /// window must not reach past this.
    pub fn frames(&self) -> usize {
        self.filled
    }

    /// Feed what the engine's tap delivered since last frame — interleaved
    /// stereo, oldest first. An odd trailing sample cannot arrive (the engine
    /// pushes whole frames only) and would be ignored if it did.
    pub fn tick(&mut self, interleaved: &[f32]) {
        for pair in interleaved.chunks_exact(CHANNELS) {
            // The frame about to slide out of the correlation window loses
            // its products BEFORE the write, while it is still readable.
            if self.corr_filled == CORR_WINDOW {
                let [l, r] = self.stereo_back(CORR_WINDOW - 1);
                self.sum_ll -= f64::from(l) * f64::from(l);
                self.sum_rr -= f64::from(r) * f64::from(r);
                self.sum_lr -= f64::from(l) * f64::from(r);
            } else {
                self.corr_filled += 1;
            }
            let at = self.head * CHANNELS;
            self.buf[at] = pair[0];
            self.buf[at + 1] = pair[1];
            self.head = (self.head + 1) % CAPACITY;
            self.filled = (self.filled + 1).min(CAPACITY);
            self.sum_ll += f64::from(pair[0]) * f64::from(pair[0]);
            self.sum_rr += f64::from(pair[1]) * f64::from(pair[1]);
            self.sum_lr += f64::from(pair[0]) * f64::from(pair[1]);
        }
    }

    /// Forget everything — the tap was re-aimed or the face closed, and the
    /// first frames of the new target must not be painted onto the tail of
    /// the old one.
    pub fn clear(&mut self) {
        self.head = 0;
        self.filled = 0;
        self.sum_ll = 0.0;
        self.sum_rr = 0.0;
        self.sum_lr = 0.0;
        self.corr_filled = 0;
        // The buffer itself need not be zeroed — `filled` gates every read —
        // but the sums were rebuilt from nothing, so the window count was too.
    }

    /// The stereo frame `i` frames BEHIND the newest. Out of reach — past
    /// `frames()`, or an empty ring — answers silence rather than panicking:
    /// this is called from paint code, where the panic hook raises a dialog.
    pub fn stereo_back(&self, i: usize) -> [f32; 2] {
        if i >= self.filled {
            return [0.0; 2];
        }
        let idx = (self.head + CAPACITY - 1 - i) % CAPACITY;
        let at = idx * CHANNELS;
        [self.buf[at], self.buf[at + 1]]
    }

    /// Mono frame `i` frames behind the newest — the L/R mean, the fold the
    /// scope paints and the fold an analyzer transforms.
    pub fn mono_back(&self, i: usize) -> f32 {
        let [l, r] = self.stereo_back(i);
        (l + r) * 0.5
    }

    /// The stereo correlation over the newest [`CORR_WINDOW`] frames:
    /// +1 for mono, 0 for unrelated channels, −1 for a polarity flip.
    ///
    /// Zero-mean form — Σlr / √(Σll·Σrr) — which is what hardware
    /// correlation meters read: audio is zero-mean at any window this long,
    /// and subtracting means would make silence plus DC read as agreement.
    /// Silence answers 0.0, the centre of the meter, on the grounds that no
    /// signal is no evidence either way.
    pub fn correlation(&self) -> f32 {
        // The subtractive sums can land a hair below zero on their way back
        // to silence; a negative under the square root is drift, not signal.
        let den = (self.sum_ll.max(0.0) * self.sum_rr.max(0.0)).sqrt();
        if den <= 1.0e-12 {
            return 0.0;
        }
        (self.sum_lr / den).clamp(-1.0, 1.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Interleave two closures over `frames` frames.
    fn feed(ring: &mut TapRing, frames: usize, f: impl Fn(usize) -> [f32; 2]) {
        let mut v = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let [l, r] = f(i);
            v.push(l);
            v.push(r);
        }
        ring.tick(&v);
    }

    #[test]
    fn the_ring_keeps_the_newest_frames_and_answers_backwards() {
        let mut ring = TapRing::new(48_000.0);
        assert_eq!(ring.frames(), 0);
        assert_eq!(ring.stereo_back(0), [0.0; 2], "an empty ring is silence");
        // More than CAPACITY frames, each stamped with its index, so what
        // survives proves both the wrap and the direction of `back`.
        let total = CAPACITY + 100;
        feed(&mut ring, total, |i| [i as f32, -(i as f32)]);
        assert_eq!(ring.frames(), CAPACITY);
        for back in [0usize, 1, 7, CAPACITY - 1] {
            let want = (total - 1 - back) as f32;
            assert_eq!(ring.stereo_back(back), [want, -want], "at {back} back");
            assert_eq!(ring.mono_back(back), 0.0, "the fold of x and -x");
        }
        // Past the fill is out of reach, not a panic.
        assert_eq!(ring.stereo_back(CAPACITY), [0.0; 2]);
    }

    #[test]
    fn correlation_reads_the_meter_ends_mono_inverted_and_silence() {
        let sine = |i: usize| (i as f32 * 0.05).sin() * 0.5;
        // Mono — identical channels — is +1.
        let mut mono = TapRing::new(48_000.0);
        feed(&mut mono, CORR_WINDOW * 2, |i| [sine(i), sine(i)]);
        assert!((mono.correlation() - 1.0).abs() < 1.0e-3, "{}", mono.correlation());
        // A polarity flip is −1.
        let mut flip = TapRing::new(48_000.0);
        feed(&mut flip, CORR_WINDOW * 2, |i| [sine(i), -sine(i)]);
        assert!((flip.correlation() + 1.0).abs() < 1.0e-3, "{}", flip.correlation());
        // Silence is the centre of the meter, not an end of it.
        let mut quiet = TapRing::new(48_000.0);
        feed(&mut quiet, CORR_WINDOW * 2, |_| [0.0, 0.0]);
        assert_eq!(quiet.correlation(), 0.0);
        // Quadrature — sin against cos — integrates to about zero.
        let mut quad = TapRing::new(48_000.0);
        feed(&mut quad, CORR_WINDOW * 2, |i| {
            [(i as f32 * 0.05).sin() * 0.5, (i as f32 * 0.05).cos() * 0.5]
        });
        assert!(quad.correlation().abs() < 0.05, "{}", quad.correlation());
    }

    #[test]
    fn the_correlation_window_slides_so_old_audio_stops_counting() {
        // A long stretch of inverted audio, then a window's worth of mono:
        // the readout must be answering for the NEW material only.
        let sine = |i: usize| (i as f32 * 0.05).sin() * 0.5;
        let mut ring = TapRing::new(48_000.0);
        feed(&mut ring, CORR_WINDOW * 4, |i| [sine(i), -sine(i)]);
        assert!(ring.correlation() < -0.99);
        feed(&mut ring, CORR_WINDOW, |i| [sine(i), sine(i)]);
        assert!(
            (ring.correlation() - 1.0).abs() < 1.0e-3,
            "the flip still counts: {}",
            ring.correlation()
        );
        // And a clear forgets everything, sums included.
        ring.clear();
        assert_eq!(ring.frames(), 0);
        assert_eq!(ring.correlation(), 0.0);
    }
}
