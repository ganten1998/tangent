//! Loudness to ITU-R BS.1770-4, which is what EBU R128 and every streaming
//! service's target are written against.
//!
//! **A peak meter and a loudness meter answer different questions and neither
//! can answer the other's.** The VU pair beside this says how close the signal
//! is to running out of headroom; this says how loud it will SEEM, which is
//! the number Spotify normalises against and the number a master is delivered
//! to. A mix can sit an inch under the ceiling on the peak meter and be six
//! decibels quieter than everything else in a playlist.
//!
//! Three readings, from the same measurement over three lengths of time:
//! momentary (400 ms), short-term (3 s) and integrated (gated, over the whole
//! programme). See [`crate::ivory_ui::recorder::LoudnessWindow`] — the
//! ladder shows one at a time and a click cycles them.
//!
//! # On the audio thread
//!
//! Everything here is fixed-size and allocated once. The integrated reading is
//! the reason: the gated mean is defined over EVERY 400 ms block since the
//! measurement started, and keeping them would be a `Vec` that grows for as
//! long as the app is open — an allocation on the audio thread the first time
//! it outgrew its capacity. A HISTOGRAM of block loudnesses answers the same
//! question in three kilobytes that never move, which is what libebur128 does
//! and what the standard's own tolerance is written to allow.

/// One channel's K-weighting: a high shelf, then a high-pass.
///
/// The two stages of BS.1770's pre-filter. The shelf is the head-related
/// transfer function — the lift the outer ear gives the top of the
/// spectrum — and the high-pass is the RLB curve, which is why a
/// subsonic rumble nobody can hear does not read as loudness.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn clear(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// The high shelf, stage 1 of the K-weighting.
///
/// **Derived at the actual sample rate rather than copied from the table.**
/// The standard prints coefficients for 48 kHz only, and a mix rendered at
/// 44.1 with 48 kHz coefficients is measured through a filter whose corner has
/// moved nine percent — small, and enough to disagree with every other meter
/// somebody checks against. These are the analog prototype's parameters, put
/// through the same bilinear transform the printed table came out of; at 48
/// kHz they reproduce it to the last digit, which is what the test asserts.
fn shelf(rate: f64) -> Biquad {
    let f0 = 1681.974450955533;
    let g = 3.999_843_853_973_347;
    let q = 0.707_175_236_955_419_6;
    let k = (std::f64::consts::PI * f0 / rate).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    Biquad {
        b0: (vh + vb * k / q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
        x1: 0.0,
        x2: 0.0,
        y1: 0.0,
        y2: 0.0,
    }
}

/// The RLB high-pass, stage 2.
fn highpass(rate: f64) -> Biquad {
    let f0 = 38.135_470_876_024_44;
    let q = 0.500_327_037_323_877_3;
    let k = (std::f64::consts::PI * f0 / rate).tan();
    let a0 = 1.0 + k / q + k * k;
    Biquad {
        b0: 1.0,
        b1: -2.0,
        b2: 1.0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
        x1: 0.0,
        x2: 0.0,
        y1: 0.0,
        y2: 0.0,
    }
}

/// How long one accumulation cell covers, in seconds.
///
/// **A hundred milliseconds, because that is the integrated measurement's own
/// step.** BS.1770 gates on 400 ms blocks overlapping by 75%, which is a new
/// block every 100 ms; making the cell the step means a block is exactly four
/// cells and the short-term window exactly thirty, and nothing has to be
/// re-summed.
const CELL_S: f64 = 0.1;

/// Cells in the ring: three seconds, the longest window that slides.
const CELLS: usize = 30;

/// Cells in one 400 ms gating block.
const BLOCK_CELLS: usize = 4;

/// The offset in the loudness formula. Not a fudge: it is what makes a
/// −20 dBFS sine at 1 kHz read −20 LUFS, which is the calibration the whole
/// standard hangs off.
const OFFSET: f64 = -0.691;

/// The absolute gate. Below this a block is silence as far as the integrated
/// reading is concerned and is dropped before anything else happens.
pub const ABSOLUTE_GATE: f32 = -70.0;

/// How far below the ungated mean the relative gate sits, in LU.
const RELATIVE_GATE_LU: f64 = 10.0;

/// The histogram's bottom, top and step, in LU.
///
/// Bottom is the absolute gate — anything under it is dropped, so there is
/// nothing to record. Top is well above full scale because loudness is a
/// MEAN and a hard-squashed master can read above zero.
const HIST_LO: f64 = -70.0;
const HIST_HI: f64 = 5.0;
const HIST_STEP: f64 = 0.1;
const HIST_BINS: usize = ((HIST_HI - HIST_LO) / HIST_STEP) as usize + 1;

/// A K-weighted loudness meter over a stereo signal.
pub struct Loudness {
    filters: [[Biquad; 2]; 2],
    /// Sum of squares per channel for each of the last [`CELLS`] cells.
    cells: [[f64; 2]; CELLS],
    /// How many frames each of those cells covers. The newest is still
    /// filling; the rest are full.
    counts: [u32; CELLS],
    /// Where the newest cell is.
    at: usize,
    /// Frames into the current cell.
    filled: u32,
    /// Frames one cell holds.
    cell_frames: u32,
    /// Gating blocks by loudness, 0.1 LU per bin. See the module note.
    hist: Box<[u32; HIST_BINS]>,
    /// The three readings, as LUFS.
    momentary: f32,
    short: f32,
    integrated: f32,
}

impl Loudness {
    pub fn new(sample_rate: f32) -> Self {
        let sr = f64::from(sample_rate.max(8_000.0));
        let one = [shelf(sr), highpass(sr)];
        Self {
            filters: [one, one],
            cells: [[0.0; 2]; CELLS],
            counts: [0; CELLS],
            at: 0,
            filled: 0,
            // At least one, so a nonsense rate cannot make the cell boundary
            // unreachable and freeze every reading at silence.
            cell_frames: ((sr * CELL_S) as u32).max(1),
            hist: Box::new([0; HIST_BINS]),
            momentary: ABSOLUTE_GATE,
            short: ABSOLUTE_GATE,
            integrated: ABSOLUTE_GATE,
        }
    }

    /// Push one stereo frame.
    ///
    /// **Returns whether a cell just closed**, which is when the readings
    /// moved and the only time a caller needs to publish them. Ten times a
    /// second, not forty-eight thousand.
    #[inline]
    pub fn push(&mut self, l: f32, r: f32) -> bool {
        for (ch, x) in [l, r].into_iter().enumerate() {
            let mut v = f64::from(x);
            for stage in &mut self.filters[ch] {
                v = stage.process(v);
            }
            self.cells[self.at][ch] += v * v;
        }
        self.filled += 1;
        if self.filled < self.cell_frames {
            return false;
        }
        self.counts[self.at] = self.filled;
        self.close_cell();
        true
    }

    fn close_cell(&mut self) {
        // The gating block is the four newest cells, this one included. Not
        // enough of them yet means the measurement has not been running for
        // 400 ms, and there is nothing to gate.
        let full = self.counts.iter().filter(|&&c| c > 0).count();
        if full >= BLOCK_CELLS {
            if let Some(l) = self.window_loudness(BLOCK_CELLS) {
                self.momentary = l as f32;
                // **Into the histogram only if it clears the absolute gate.**
                // The relative gate comes later, over everything that got
                // this far; dropping silence first is what stops a long
                // fade-out from dragging the whole programme's mean down.
                if l > HIST_LO {
                    let i = (((l - HIST_LO) / HIST_STEP).round() as usize).min(HIST_BINS - 1);
                    self.hist[i] = self.hist[i].saturating_add(1);
                }
                self.integrated = self.gated() as f32;
            }
        }
        if full >= CELLS {
            if let Some(l) = self.window_loudness(CELLS) {
                self.short = l as f32;
            }
        }
        self.at = (self.at + 1) % CELLS;
        self.cells[self.at] = [0.0; 2];
        self.counts[self.at] = 0;
        self.filled = 0;
    }

    /// The loudness of the newest `n` cells, or `None` if they hold nothing.
    ///
    /// **`None` for true silence, not `-inf`.** A log of zero is negative
    /// infinity, which stores into an atomic perfectly happily and then
    /// paints a ladder whose fill is `NaN`.
    fn window_loudness(&self, n: usize) -> Option<f64> {
        let (sum, frames) = self.window_energy(n);
        if frames == 0 {
            return None;
        }
        // **Both channels weighted 1.0.** BS.1770 gives the surround
        // channels 1.41 and the front pair 1.0; this desk is stereo, so the
        // weighting is written down as the multiplication it is and then not
        // performed.
        let mean = (sum[0] + sum[1]) / frames as f64;
        (mean > 0.0).then(|| OFFSET + 10.0 * mean.log10())
    }

    /// One CHANNEL's loudness over the newest `n` cells.
    ///
    /// **Not half of the pair's reading.** The pair's is the sum of both
    /// channels' energy against one frame count; a channel's own is its
    /// energy alone against the same count, which for a correlated stereo
    /// programme comes out about 3 LU below the pair. That is what every
    /// per-channel loudness meter shows and it is why the two bars sit lower
    /// than the number under them.
    fn channel_loudness(&self, n: usize, ch: usize) -> Option<f64> {
        let (sum, frames) = self.window_energy(n);
        if frames == 0 || ch >= 2 {
            return None;
        }
        let mean = sum[ch] / frames as f64;
        (mean > 0.0).then(|| OFFSET + 10.0 * mean.log10())
    }

    /// The per-channel energy in the newest `n` cells, and how many frames
    /// went into it. The half `window_loudness` and `channel_loudness`
    /// share, so the two cannot disagree about which cells a window is.
    fn window_energy(&self, n: usize) -> ([f64; 2], u64) {
        let mut sum = [0.0f64; 2];
        let mut frames = 0u64;
        for k in 0..n {
            let i = (self.at + CELLS - k) % CELLS;
            for ch in 0..2 {
                sum[ch] += self.cells[i][ch];
            }
            frames += u64::from(self.counts[i]);
        }
        (sum, frames)
    }

    /// The integrated reading: the mean of the blocks that clear both gates.
    fn gated(&self) -> f64 {
        let energy = |bin: usize| {
            let l = HIST_LO + bin as f64 * HIST_STEP;
            10f64.powf((l - OFFSET) / 10.0)
        };
        let mut sum = 0.0f64;
        let mut n = 0u64;
        for (bin, &count) in self.hist.iter().enumerate() {
            if count > 0 {
                sum += energy(bin) * f64::from(count);
                n += u64::from(count);
            }
        }
        if n == 0 {
            return f64::from(ABSOLUTE_GATE);
        }
        // The relative gate: ten LU below the mean of everything that cleared
        // the absolute one.
        let relative = OFFSET + 10.0 * (sum / n as f64).log10() - RELATIVE_GATE_LU;
        let mut sum2 = 0.0f64;
        let mut n2 = 0u64;
        for (bin, &count) in self.hist.iter().enumerate() {
            if count == 0 {
                continue;
            }
            let l = HIST_LO + bin as f64 * HIST_STEP;
            if l > relative {
                sum2 += energy(bin) * f64::from(count);
                n2 += u64::from(count);
            }
        }
        if n2 == 0 {
            return f64::from(ABSOLUTE_GATE);
        }
        OFFSET + 10.0 * (sum2 / n2 as f64).log10()
    }

    /// The three readings, in LUFS. [`ABSOLUTE_GATE`] means "nothing yet".
    pub fn read(&self) -> (f32, f32, f32) {
        (self.momentary, self.short, self.integrated)
    }

    /// **The stereo pair, per channel, for the two sliding windows.**
    ///
    /// `[momentary L, momentary R, short L, short R]`, in LUFS, with
    /// [`ABSOLUTE_GATE`] for "nothing measured".
    ///
    /// There is no integrated pair, and its absence is deliberate: the
    /// standard's integrated reading is gated on the PROGRAMME — a block
    /// counts or does not count for the whole measurement — so a per-channel
    /// integrated figure would be a number this app invented and no other
    /// meter would agree with. The bars fall back to the pair's own reading
    /// on that window; see `draw_lufs`.
    pub fn read_stereo(&self) -> [f32; 4] {
        let f = |v: Option<f64>| v.map_or(ABSOLUTE_GATE, |x| x as f32);
        [
            f(self.channel_loudness(BLOCK_CELLS, 0)),
            f(self.channel_loudness(BLOCK_CELLS, 1)),
            f(self.channel_loudness(CELLS, 0)),
            f(self.channel_loudness(CELLS, 1)),
        ]
    }

    /// Start the INTEGRATED reading again, leaving the two sliding windows
    /// alone.
    ///
    /// **Only the integrated one**, because only the integrated one has any
    /// memory to clear: momentary and short-term forget by themselves in 400
    /// milliseconds and three seconds. Dropping the filter state as well
    /// would put a transient through the K-weighting at the moment somebody
    /// pressed reset and record it as the first block of the new measurement.
    pub fn reset_integrated(&mut self) {
        self.hist.fill(0);
        self.integrated = ABSOLUTE_GATE;
    }

    /// Everything back to silence: a new device, a new rate, a stopped
    /// engine. The filters go too — this is a discontinuity, and their state
    /// belongs to a signal that is not coming back.
    pub fn clear(&mut self) {
        for ch in &mut self.filters {
            for stage in ch {
                stage.clear();
            }
        }
        self.cells = [[0.0; 2]; CELLS];
        self.counts = [0; CELLS];
        self.at = 0;
        self.filled = 0;
        self.hist.fill(0);
        self.momentary = ABSOLUTE_GATE;
        self.short = ABSOLUTE_GATE;
        self.integrated = ABSOLUTE_GATE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The calibration the whole standard hangs off: a −20 dBFS sine at
    /// 1 kHz, on both channels, reads −20 LUFS.
    ///
    /// **This is the one test that would catch a wrong filter.** Every other
    /// property here — that it moves the right way, that gating drops
    /// silence — passes just as happily with the K-weighting mis-derived at
    /// the wrong sample rate.
    #[test]
    fn a_minus_twenty_sine_reads_minus_twenty() {
        for rate in [44_100.0_f32, 48_000.0, 96_000.0] {
            let mut m = Loudness::new(rate);
            let amp = 10f32.powf(-20.0 / 20.0);
            let n = (rate * 4.0) as usize;
            for i in 0..n {
                let t = i as f32 / rate;
                // Stereo, the same signal both sides. Two channels each at
                // −20 dBFS sum to −20 LUFS and not −17: the mean square is
                // summed across channels and the offset is calibrated for it.
                let s = amp * (std::f32::consts::TAU * 1000.0 * t).sin();
                m.push(s, s);
            }
            let (mom, short, int) = m.read();
            for (name, v) in [("momentary", mom), ("short", short), ("integrated", int)] {
                assert!(
                    (v - -20.0).abs() < 0.15,
                    "{name} read {v} at {rate} Hz, not -20"
                );
            }
        }
    }

    /// **The printed 48 kHz coefficients, reproduced by the derivation.**
    ///
    /// BS.1770 prints one table, for 48 kHz. Everything else in this file
    /// computes the filter from the analog prototype instead, so that a
    /// session at 44.1 is measured through a filter whose corner is in the
    /// right place. This is what proves the two agree where they overlap —
    /// without it, a derivation that was wrong at EVERY rate would still
    /// pass the sine test, because a sine at 1 kHz sits where the shelf is
    /// nearly flat.
    #[test]
    fn the_derived_filter_matches_the_standards_own_table() {
        let s = shelf(48_000.0);
        for (name, got, want) in [
            ("b0", s.b0, 1.535_124_859_586_97),
            ("b1", s.b1, -2.691_696_189_406_38),
            ("b2", s.b2, 1.198_392_810_852_85),
            ("a1", s.a1, -1.690_659_293_182_41),
            ("a2", s.a2, 0.732_480_774_215_85),
        ] {
            assert!(
                (got - want).abs() < 1e-6,
                "shelf {name} came out {got}, the standard prints {want}"
            );
        }
        let h = highpass(48_000.0);
        for (name, got, want) in [
            ("a1", h.a1, -1.990_047_454_833_98),
            ("a2", h.a2, 0.990_072_250_366_21),
        ] {
            assert!(
                (got - want).abs() < 1e-6,
                "high-pass {name} came out {got}, the standard prints {want}"
            );
        }
    }

    /// Silence never reads as a number, and never reads as `-inf`.
    #[test]
    fn silence_is_the_gate_and_not_negative_infinity() {
        let mut m = Loudness::new(48_000.0);
        for _ in 0..48_000 * 4 {
            m.push(0.0, 0.0);
        }
        let (mom, short, int) = m.read();
        for v in [mom, short, int] {
            assert!(v.is_finite(), "a reading came back {v}");
            assert!(v <= ABSOLUTE_GATE + 1e-3, "silence read {v}");
        }
    }

    /// **The gate is the whole difference between integrated and a plain
    /// average**, and this is what it buys: two seconds of music followed by
    /// a minute of silence still reads as the music.
    #[test]
    fn the_gate_keeps_a_long_silence_from_dragging_the_programme_down() {
        let rate = 48_000.0_f32;
        let mut m = Loudness::new(rate);
        let amp = 10f32.powf(-20.0 / 20.0);
        for i in 0..(rate * 3.0) as usize {
            let t = i as f32 / rate;
            let s = amp * (std::f32::consts::TAU * 1000.0 * t).sin();
            m.push(s, s);
        }
        let after_music = m.read().2;
        assert!((after_music - -20.0).abs() < 0.2, "{after_music}");
        for _ in 0..(rate * 30.0) as usize {
            m.push(0.0, 0.0);
        }
        let (mom, _, int) = m.read();
        // The momentary reading has followed the signal down, which is what
        // "momentary" means...
        assert!(mom <= ABSOLUTE_GATE + 1e-3, "momentary held at {mom}");
        // ...and the integrated one has barely moved, which is what "gated"
        // means.
        //
        // **Barely, and not "not at all".** Three of the 400 ms blocks
        // straddle the moment the music stops and are part music; they clear
        // both gates honestly and pull the mean down a fraction. A test that
        // demanded no movement at all would be asserting a bug.
        assert!(
            (int - after_music).abs() < 0.4,
            "thirty seconds of silence moved the programme from {after_music} to {int}"
        );
        // And the number that says what the gate actually bought: three
        // seconds of music in thirty-three seconds of programme is a tenth
        // of the energy, so an UNGATED mean would read about −30.4 here.
        // Ten decibels of difference between the right answer and the naive
        // one is the whole reason the gate is in the standard.
        let ungated = -20.0 + 10.0 * (3.0f32 / 33.0).log10();
        assert!(
            int > ungated + 9.0,
            "the reading fell to {int}; an ungated mean would be {ungated}"
        );
    }

    /// Louder reads louder, and by the amount it was raised by.
    #[test]
    fn six_more_decibels_reads_six_lufs_louder() {
        let rate = 48_000.0_f32;
        let read = |db: f32| {
            let mut m = Loudness::new(rate);
            let amp = 10f32.powf(db / 20.0);
            for i in 0..(rate * 2.0) as usize {
                let t = i as f32 / rate;
                let s = amp * (std::f32::consts::TAU * 1000.0 * t).sin();
                m.push(s, s);
            }
            m.read().0
        };
        let (a, b) = (read(-26.0), read(-20.0));
        assert!((b - a - 6.0).abs() < 0.05, "{a} -> {b}");
    }

    /// A reset drops the programme and leaves the windows alone.
    #[test]
    fn a_reset_clears_the_programme_and_nothing_else() {
        let rate = 48_000.0_f32;
        let mut m = Loudness::new(rate);
        let amp = 10f32.powf(-20.0 / 20.0);
        for i in 0..(rate * 3.0) as usize {
            let t = i as f32 / rate;
            let s = amp * (std::f32::consts::TAU * 1000.0 * t).sin();
            m.push(s, s);
        }
        let before = m.read();
        m.reset_integrated();
        let after = m.read();
        assert_eq!(after.0, before.0, "the momentary reading was cleared too");
        assert_eq!(after.1, before.1, "the short-term reading was cleared too");
        assert_eq!(after.2, ABSOLUTE_GATE, "the programme survived a reset");
    }

    /// The low rumble a loudness meter is supposed to ignore.
    ///
    /// The RLB high-pass is why: a 20 Hz tone at −10 dBFS is inaudible on
    /// most speakers and would otherwise read as the loudest thing in the
    /// mix.
    #[test]
    fn subsonic_rumble_barely_registers() {
        let rate = 48_000.0_f32;
        let mut m = Loudness::new(rate);
        let amp = 10f32.powf(-10.0 / 20.0);
        for i in 0..(rate * 2.0) as usize {
            let t = i as f32 / rate;
            let s = amp * (std::f32::consts::TAU * 20.0 * t).sin();
            m.push(s, s);
        }
        let mom = m.read().0;
        // The RLB high-pass is about fourteen decibels down at 20 Hz — a
        // second-order corner at 38 — so a −10 dBFS rumble reads near −24
        // LUFS where a −10 dBFS tone at 1 kHz reads −10.
        assert!(mom < -20.0, "20 Hz at -10 dBFS read {mom} LUFS");
    }
}
