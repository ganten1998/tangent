//! The oscilloscope on the LCD's right half.
//!
//! A live trace of the master mix WITHOUT the click — the tap reads `self.mix`
//! after the limiter and the master fader, before `click_out` is added, which
//! is the one point in the block where "everything the app is doing, minus the
//! metronome" exists as a buffer.
//!
//! Pure painter, like the rest of the cluster: `draw` takes `&self` and a
//! rect and paints; all mutation happens in [`Scope::tick`], which the app
//! calls once a frame with whatever the engine's ring delivered. The split
//! exists because the panel painters are immutable views and must stay so.
//!
//! Original implementation. REAPER's stock scope (`gfxscope`) is LGPL, which
//! a statically-linked closed app must not absorb — nothing here is derived
//! from it. A green trace on black glass is older than Cockos.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};

/// Stereo, matching the engine's `TAP_CHANNELS`.
const CHANNELS: usize = 2;

/// The rolling window the scope keeps, in frames. At 48 kHz this is ~340 ms —
/// enough that the display window (~40 ms) can hunt backwards for a trigger
/// without ever running off the front.
const CAPACITY: usize = 1 << 14;

/// How much of the face the display window shows, in seconds.
const WINDOW_S: f32 = 0.040;

/// Auto-gain never boosts a quiet signal by more than this (12 dB): a solo
/// piano grows to fill the face, the noise floor between songs does not.
const ENV_FLOOR: f32 = 0.25;

/// Below this window peak the signal is silence and the scope draws its
/// resting line instead of amplified nothing.
const SILENCE: f32 = 0.004;

/// The scope's colours. Two ready-made faces; the widget itself has no
/// opinion about dark mode — the caller decides which face the mode wears.
#[derive(Clone, Copy)]
pub struct ScopeStyle {
    pub face: Color32,
    /// The hairline around the face. `None` when the cluster already drew one.
    pub bezel: Option<Color32>,
    pub grid: Color32,
    pub trace: Color32,
    /// The wide understroke that makes the core line glow.
    pub glow: Color32,
    /// The per-column min..max body fill under the line.
    pub fill: Color32,
    /// The clip lamp, lit when the window touched 0 dBFS.
    pub clip: Color32,
}

impl ScopeStyle {
    /// The cluster's own face: tape-green on black, both modes — the same
    /// glass the timecode is behind.
    pub fn lcd() -> Self {
        Self::themed(
            Color32::from_rgb(0xA3, 0xE0, 0x48),
            Color32::from_rgb(0x0B, 0x0E, 0x0A),
        )
    }

    /// The same glass in a theme's colours: one trace colour, one face, and
    /// every derived shade (grid, glow, fill) taken from the trace so a theme
    /// sets ONE value and the whole instrument follows.
    pub fn themed(trace: Color32, face: Color32) -> Self {
        let green = trace;
        Self {
            face,
            bezel: None,
            grid: Color32::from_rgba_unmultiplied(green.r(), green.g(), green.b(), 26),
            trace: green,
            glow: Color32::from_rgba_unmultiplied(green.r(), green.g(), green.b(), 46),
            fill: Color32::from_rgba_unmultiplied(green.r(), green.g(), green.b(), 62),
            clip: Color32::from_rgb(0xE8, 0x3A, 0x4E),
        }
    }

}

/// The scope's state: a ring of recent stereo frames and the auto-gain
/// envelope. Owned by the app beside the other session-only view state.
pub struct Scope {
    /// Interleaved stereo, a ring. `head` is the next write, so the newest
    /// frame is just behind it.
    buf: Vec<f32>,
    head: usize,
    filled: usize,
    /// Peak envelope the vertical gain follows: instant attack, slow release,
    /// so a hit fills the face at once and the decay is watchable.
    env: f32,
    rate: f32,
}

impl Scope {
    pub fn new(rate: f32) -> Self {
        Self {
            buf: vec![0.0; CAPACITY * CHANNELS],
            head: 0,
            filled: 0,
            env: ENV_FLOOR,
            rate: rate.max(8_000.0),
        }
    }

    /// A device restart changes the rate; the picture survives, the window
    /// math follows. The app calls this from `start_engine`; the harness has
    /// one rate and never does.
    #[allow(dead_code)]
    pub fn set_rate(&mut self, rate: f32) {
        self.rate = rate.max(8_000.0);
    }

    /// Feed what the engine's ring delivered since last frame — interleaved
    /// stereo, oldest first — and advance the envelope by `dt` seconds.
    pub fn tick(&mut self, interleaved: &[f32], dt: f32) {
        let mut peak = 0.0f32;
        for pair in interleaved.chunks_exact(CHANNELS) {
            let at = self.head * CHANNELS;
            self.buf[at] = pair[0];
            self.buf[at + 1] = pair[1];
            self.head = (self.head + 1) % CAPACITY;
            self.filled = (self.filled + 1).min(CAPACITY);
            peak = peak.max(pair[0].abs().max(pair[1].abs()));
        }
        // Release ~350 ms to floor; attack is the max() — immediate.
        self.env = (self.env * (-dt / 0.35).exp()).max(peak).max(ENV_FLOOR);
    }

    /// How many frames the ring holds, saturating at its capacity.
    pub fn frames(&self) -> usize {
        self.filled
    }

    /// Stereo frame `i` frames BEHIND the newest — the analyzer's read,
    /// which needs both sides for its mid and side channels where the
    /// trace only ever needed the sum.
    pub fn stereo_back(&self, i: usize) -> [f32; 2] {
        let idx = (self.head + CAPACITY - 1 - (i % CAPACITY)) % CAPACITY;
        let at = idx * CHANNELS;
        [self.buf[at], self.buf[at + 1]]
    }

    /// Mono frame `i` frames BEHIND the newest.
    fn mono_back(&self, i: usize) -> f32 {
        let idx = (self.head + CAPACITY - 1 - i) % CAPACITY;
        let at = idx * CHANNELS;
        (self.buf[at] + self.buf[at + 1]) * 0.5
    }

    /// Where the display window starts, in frames-behind-newest: the most
    /// recent RISING zero crossing that leaves a whole window in front of it,
    /// or a free run when none shows up (noise, silence, the first frames).
    /// Interpolation is deliberately absent: at ~8 samples a pixel the
    /// sub-sample phase is below the paint.
    fn trigger(&self, window: usize) -> usize {
        let reach = (window * 2).min(self.filled.saturating_sub(1));
        let mut i = window; // the newest start that still fits a window
        while i < reach {
            let a = self.mono_back(i + 1);
            let b = self.mono_back(i);
            if a <= 0.0 && b > 0.0 {
                return i;
            }
            i += 1;
        }
        window
    }

    /// Paint the scope into `r`. The face is painted first, so this owns its
    /// whole rectangle the way the timecode half owns its own.
    pub fn draw(&self, painter: &Painter, r: Rect, style: &ScopeStyle) {
        if !r.is_positive() {
            return;
        }
        painter.rect_filled(r, 2.0, style.face);
        if let Some(bezel) = style.bezel {
            painter.rect_stroke(
                r,
                2.0,
                Stroke::new(1.0_f32, bezel),
                egui::StrokeKind::Inside,
            );
        }
        let inner = r.shrink2(egui::vec2(4.0, 3.0));
        if !inner.is_positive() {
            return;
        }
        let mid = inner.center().y;
        let half = inner.height() * 0.5;

        // ── the graticule: quiet, and under everything ────────────────────
        // Verticals every 10 ms of the 40 ms window; horizontals at the
        // centre and half amplitude. Faint enough to read as glass etching.
        let thin = Stroke::new(1.0_f32, style.grid);
        for q in 1..4 {
            let x = inner.left() + inner.width() * (q as f32 / 4.0);
            painter.add(Shape::dashed_line(
                &[Pos2::new(x, inner.top()), Pos2::new(x, inner.bottom())],
                thin,
                1.0,
                3.0,
            ));
        }
        for f in [-0.5f32, 0.5] {
            let y = mid - half * f;
            painter.add(Shape::dashed_line(
                &[Pos2::new(inner.left(), y), Pos2::new(inner.right(), y)],
                thin,
                1.0,
                3.0,
            ));
        }
        painter.line_segment(
            [Pos2::new(inner.left(), mid), Pos2::new(inner.right(), mid)],
            Stroke::new(1.0_f32, style.grid.gamma_multiply(1.6)),
        );

        let window = ((WINDOW_S * self.rate) as usize).clamp(64, CAPACITY / 2);
        if self.filled < window {
            return;
        }

        // Window peak decides silence; the envelope decides gain. The window
        // runs FORWARD in time from the trigger: sample `s` of the display,
        // oldest first, is `start - s` frames behind the newest.
        let start = self.trigger(window);
        let mut wpeak = 0.0f32;
        for s in 0..window {
            wpeak = wpeak.max(self.mono_back(start - s).abs());
        }
        if wpeak < SILENCE {
            // The resting line: alive, quiet — brighter than the graticule,
            // dimmer than a signal.
            painter.line_segment(
                [Pos2::new(inner.left(), mid), Pos2::new(inner.right(), mid)],
                Stroke::new(1.2_f32, style.trace.gamma_multiply(0.35)),
            );
            return;
        }

        // 86% of the half-height at the envelope: a full-scale hit sits just
        // inside the glass, and the auto-gain never crops.
        let gain = half * 0.86 / self.env.max(ENV_FLOOR);

        // ── the trace: per-pixel min/max body, then the glowing core ──────
        // One column per pixel, each reduced over its span of samples —
        // peak-preserving, so a transient is a spike and not an alias.
        let cols = (inner.width().floor() as usize).max(8);
        let mut body = Vec::with_capacity(cols);
        let mut line = Vec::with_capacity(cols);
        for c in 0..cols {
            let s0 = window * c / cols;
            let s1 = (window * (c + 1) / cols).max(s0 + 1);
            let (mut lo, mut hi, mut sum) = (f32::MAX, f32::MIN, 0.0f32);
            for s in s0..s1 {
                let v = self.mono_back(start - s);
                lo = lo.min(v);
                hi = hi.max(v);
                sum += v;
            }
            let x = inner.left() + (c as f32 + 0.5) * inner.width() / cols as f32;
            let (y0, y1) = (mid - hi * gain, mid - lo * gain);
            body.push((x, y0.min(y1), y0.max(y1)));
            line.push(Pos2::new(x, mid - (sum / (s1 - s0) as f32) * gain));
        }
        for &(x, y0, y1) in &body {
            if y1 - y0 > 0.6 {
                painter.line_segment(
                    [Pos2::new(x, y0), Pos2::new(x, y1)],
                    Stroke::new(1.0_f32, style.fill),
                );
            }
        }
        painter.add(Shape::line(line.clone(), Stroke::new(3.0_f32, style.glow)));
        painter.add(Shape::line(line, Stroke::new(1.4_f32, style.trace)));

        // The clip lamp: three points square, top right, only when the
        // window really touched the ceiling.
        if wpeak >= 0.995 {
            let p = Pos2::new(inner.right() - 4.0, inner.top() + 4.0);
            painter.circle_filled(p, 2.0, style.clip);
        }
    }
}
