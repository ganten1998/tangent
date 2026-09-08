//! Time-stretching a clip: change how long it lasts without changing what it
//! sounds like.
//!
//! **The other half of the varispeed dial, and the opposite of it.**
//! `crate::varispeed` moves the speed and lets the pitch follow, which is
//! what a tape machine does. This keeps the pitch and moves the time, which
//! is what a sampler does — and it is the one somebody wants when a take is
//! four bars at 96 and the song is at 104.
//!
//! # Why offline
//!
//! Clips are played by a STATELESS read: the renderer is handed an
//! `Arc<Clip>` and a frame range, and asking it for frame 400,000 costs
//! exactly what asking for frame 4 costs. That is what makes locate, loop
//! and export correct, and it is the property the plugin-delay work went out
//! of its way to preserve.
//!
//! A streaming stretcher would take that away: every algorithm here carries
//! state across its own hops, so seeking into one means either replaying
//! from the start or accepting a discontinuity. So a stretched clip is
//! RENDERED ONCE, into a new buffer, and played back statelessly like any
//! other. It costs a second of CPU on an edit and nothing at all on the
//! audio thread, which is the right way round.
//!
//! # The three modes
//!
//! Nobody wants to choose an algorithm; everybody wants the right one. See
//! [`Mode::pick`], which listens to the material and to how far it is being
//! asked to go.

use tangent_record::decode::Clip;

/// Which algorithm stretches a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// **Listen, then decide.** The default, and what nearly every clip
    /// should stay on: see [`Mode::pick`].
    #[default]
    Auto,
    /// A plain resample. **The pitch follows** — this is the tape machine,
    /// and it is here because for a correction of a percent or two it is the
    /// only one of the three with no artefacts at all, and two percent is
    /// a third of a semitone.
    Tape,
    /// WSOLA: overlap-add in the time domain, with each splice searched for
    /// the alignment that matches what came before.
    ///
    /// **Right for anything with edges in it** — drums, a plucked
    /// instrument, speech. It keeps transients sharp because it never
    /// touches the spectrum, and it choruses on dense sustained material
    /// because the splices are audible when nothing else is.
    Rhythmic,
    /// A phase vocoder with peak locking.
    ///
    /// **Right for anything sustained** — pads, strings, a held chord. It
    /// keeps a smooth tone smooth because it works on the spectrum, and it
    /// smears transients for the same reason.
    Tonal,
}

impl Mode {
    /// Every mode, in the order a menu should offer them: the automatic one
    /// first, then the three it chooses between.
    pub const ALL: [Mode; 4] = [Mode::Auto, Mode::Tape, Mode::Rhythmic, Mode::Tonal];

    /// How it is written to the document, and read back. **Append-only.**
    pub fn key(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Tape => "tape",
            Mode::Rhythmic => "rhythmic",
            Mode::Tonal => "tonal",
        }
    }

    pub fn from_key(key: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.key() == key)
    }

    /// **Which algorithm this material and this ratio want.**
    ///
    /// Two questions, in order:
    ///
    /// 1. **Is this barely a stretch at all?** Under a couple of percent,
    ///    a resample moves the pitch by less than a third of a semitone and
    ///    introduces nothing else. Every other algorithm has a cost that
    ///    does not shrink with the ratio — a splice is as audible at 1% as
    ///    at 50% — so for a small correction the tape is genuinely the best
    ///    of the three.
    /// 2. **Does it have edges in it?** Measured, not guessed: see
    ///    [`percussiveness`].
    ///
    /// Answers itself for [`Mode::Auto`] only; anything else is a choice
    /// somebody made and is returned unchanged.
    pub fn pick(self, clip: &Clip, ratio: f64) -> Mode {
        if self != Mode::Auto {
            return self;
        }
        if (ratio - 1.0).abs() <= 0.02 {
            return Mode::Tape;
        }
        if percussiveness(clip) >= PERCUSSIVE_AT {
            Mode::Rhythmic
        } else {
            Mode::Tonal
        }
    }
}

/// Above this, material counts as having edges in it. See [`percussiveness`].
const PERCUSSIVE_AT: f32 = 0.30;

/// How much of this clip is attacks, from 0 (a held tone) to 1 (a click
/// track).
///
/// **The CREST FACTOR of the envelope**, which is peak over rms: a signal
/// made of attacks spends most of its time quiet and a moment loud, so its
/// peak is far above its own average; a sustained one sits at its average.
/// Measured on the envelope rather than on the waveform, because a sine's
/// waveform crest is 1.41 whatever it is doing.
///
/// **This was the ratio of the envelope's rises to its whole travel**, which
/// is a measure of nothing: anything that comes back to where it started
/// rises exactly as much as it falls, so a click track and a held chord both
/// scored one half and the rescaling turned both into zero. Every clip was
/// tonal. It took a test with a click track in it to notice, because the
/// number looked reasonable on its own.
///
/// Leading and trailing near-silence is trimmed first. It is not material —
/// a pad with three seconds of room tone in front of it is a pad — and left
/// in, it is exactly the shape that reads as one enormous attack.
///
/// Cheap on purpose: one pass over a decimated envelope of at most the first
/// minute. This runs on the UI thread when somebody drags a clip's edge.
pub fn percussiveness(clip: &Clip) -> f32 {
    // ~4 ms buckets: short enough to see a drum's attack, long enough that a
    // waveform's own cycles do not read as transients.
    let step = ((clip.rate as usize / 500).max(1)) * 2;
    // A minute is plenty to characterise a clip, and it is what keeps a
    // ten-minute import from costing a visible pause on a drag.
    let frames = clip.frames().min(clip.rate as usize * 60);
    if frames < step * 8 {
        return 0.0;
    }
    let mut env: Vec<f32> = Vec::with_capacity(frames / step + 1);
    let mut i = 0;
    while i + step <= frames {
        let mut peak = 0.0f32;
        for f in i..i + step {
            let a = clip.samples.get(f * 2).copied().unwrap_or(0.0).abs();
            let b = clip.samples.get(f * 2 + 1).copied().unwrap_or(0.0).abs();
            peak = peak.max(a.max(b));
        }
        env.push(peak);
        i += step;
    }
    let peak = env.iter().copied().fold(0.0f32, f32::max);
    if peak <= 1.0e-6 {
        return 0.0;
    }
    // The ends trimmed, and only the ends: an interior quiet passage is part
    // of what the material is.
    let floor = peak * 0.02;
    let first = env.iter().position(|v| *v > floor).unwrap_or(0);
    let last = env.iter().rposition(|v| *v > floor).unwrap_or(env.len() - 1);
    let live = &env[first..=last.max(first)];
    if live.len() < 4 {
        return 0.0;
    }
    let rms = (live.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>()
        / live.len() as f64)
        .sqrt() as f32;
    if rms <= 1.0e-9 {
        return 0.0;
    }
    let crest = peak / rms;
    // **Where the two ends of the scale sit**, measured rather than chosen:
    // a steady tone is 1.0, a held chord 1.5, a plucked note 1.7, a mix with
    // drums in it 2.2, a bare click track 5.6. 1.3 and 1.2 put the threshold
    // between the chord and the pluck, which is where a musician would put
    // it: anything with a pick or a stick in it wants the algorithm that
    // keeps edges.
    ((crest - 1.3) / 1.2).clamp(0.0, 1.0)
}

/// The analysis window, in frames. A power of two for the transform.
///
/// **~46 ms at 44.1 kHz.** Long enough for the phase vocoder to resolve a
/// bass note (a 50 Hz period is 20 ms) and short enough that a transient is
/// smeared over a syllable rather than a bar.
const WINDOW: usize = 2048;

/// How far the analysis walks between windows. A quarter of the window, so
/// four of them overlap and a Hann window sums to a constant.
const HOP: usize = WINDOW / 4;

/// How far WSOLA may look for a better splice, in frames. About 12 ms —
/// a little over one period of the lowest note anybody plays.
const SEARCH: usize = 512;

/// The most a clip may be stretched or squeezed.
///
/// Past this, every algorithm is a special effect rather than a correction,
/// and the buffer is a hundred times the size of the file that made it.
pub const RATIO: (f64, f64) = (0.25, 4.0);

/// The document's own copy of the same two numbers, checked here so the two
/// cannot drift.
///
/// `tangent-ui` clamps a clip's ratio on read and cannot name a constant that
/// lives in the binary with the DSP — the firewall's whole point. This is
/// what makes the duplicate honest.
const _: () = assert!(
    RATIO.0 == tangent_ui::arrangement::STRETCH_RATIO.0
        && RATIO.1 == tangent_ui::arrangement::STRETCH_RATIO.1,
);

/// The same for the mode names, which the clip menu offers and the document
/// stores. A mode added here and not there is a menu that cannot reach it;
/// added there and not here, a menu row that does nothing.
///
/// Checked at run time rather than at compile time because `str` comparison
/// is not something a `const` assertion can do on this toolchain — but it is
/// checked in a `#[test]`, which is the same guarantee at the same moment
/// anybody would find out.
#[cfg(test)]
const MODE_KEYS: [&str; 4] = ["auto", "tape", "rhythmic", "tonal"];

/// Stretch `clip` to `ratio` times its length. 1.0 hands the same clip back.
///
/// `mode` may be [`Mode::Auto`], which chooses — see [`Mode::pick`].
pub fn stretch(clip: &Clip, ratio: f64, mode: Mode) -> Clip {
    let ratio = if ratio.is_finite() {
        ratio.clamp(RATIO.0, RATIO.1)
    } else {
        1.0
    };
    if (ratio - 1.0).abs() < 1.0e-9 || clip.frames() == 0 {
        return clip.clone();
    }
    let samples = match mode.pick(clip, ratio) {
        // Unreachable — `pick` never answers Auto — and written down because
        // the alternative to saying so is a silent clone.
        Mode::Auto | Mode::Tape => tape(clip, ratio),
        Mode::Rhythmic => wsola(clip, ratio),
        Mode::Tonal => vocoder(clip, ratio),
    };
    Clip {
        samples,
        rate: clip.rate,
        source: clip.source.clone(),
    }
}

/// A plain resample. The pitch follows, which is the point.
fn tape(clip: &Clip, ratio: f64) -> Vec<f32> {
    let frames = clip.frames();
    let out_frames = ((frames as f64) * ratio).round().max(1.0) as usize;
    let mut out = vec![0.0f32; out_frames * 2];
    let at = |f: isize, ch: usize| -> f32 {
        let f = f.clamp(0, frames as isize - 1) as usize;
        clip.samples.get(f * 2 + ch).copied().unwrap_or(0.0)
    };
    for j in 0..out_frames {
        let x = j as f64 / ratio;
        let i = x.floor();
        let t = (x - i) as f32;
        let i = i as isize;
        for ch in 0..2 {
            // The same four-point Hermite the varispeed path uses, and for
            // the same reason: linear's error is a low-pass whose corner
            // moves with the ratio.
            let (y0, y1, y2, y3) = (at(i - 1, ch), at(i, ch), at(i + 1, ch), at(i + 2, ch));
            let c0 = y1;
            let c1 = 0.5 * (y2 - y0);
            let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
            let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
            out[j * 2 + ch] = ((c3 * t + c2) * t + c1) * t + c0;
        }
    }
    out
}

/// A Hann window of `n` points.
fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos()
        })
        .collect()
}

/// WSOLA: overlap-add, with every splice searched for the alignment that
/// best continues what came before.
///
/// **The search is what makes it not sound like a stutter.** Plain
/// overlap-add at a fixed hop splices waveforms at arbitrary phases, and two
/// copies of a note a few samples out of step cancel. Looking ±[`SEARCH`]
/// frames for the best match costs a correlation per hop and buys the
/// difference between "stretched" and "broken".
///
/// The search runs on the MID channel and both channels take the same
/// offset. Aligning them independently would move the two sides of a stereo
/// image by different amounts, which collapses it.
fn wsola(clip: &Clip, ratio: f64) -> Vec<f32> {
    let frames = clip.frames();
    let win = hann(WINDOW);
    let out_frames = ((frames as f64) * ratio).round().max(1.0) as usize + WINDOW;
    let mut out = vec![0.0f32; out_frames * 2];
    let mut norm = vec![0.0f32; out_frames];
    let at = |f: usize, ch: usize| clip.samples.get(f * 2 + ch).copied().unwrap_or(0.0);
    let mid = |f: usize| 0.5 * (at(f, 0) + at(f, 1));

    // Where the NEXT input segment ideally starts, and where the last one
    // actually ended — the tail that the next splice has to continue.
    let mut want = 0.0f64;
    let mut tail_at: usize = 0;
    let mut o = 0usize;
    while o + WINDOW < out_frames && (want as usize) < frames {
        let ideal = want as usize;
        // The first segment has nothing to continue and is taken as-is.
        let take = if o == 0 {
            ideal
        } else {
            let lo = ideal.saturating_sub(SEARCH);
            let hi = (ideal + SEARCH).min(frames.saturating_sub(WINDOW + 1));
            let mut best = ideal.min(hi);
            let mut best_score = f32::NEG_INFINITY;
            // The overlap is one hop long: that is the part of this segment
            // that lands on top of what is already written.
            let n = HOP.min(WINDOW);
            let mut cand = lo;
            while cand <= hi {
                let mut acc = 0.0f32;
                for k in 0..n {
                    let a = mid((tail_at + k).min(frames - 1));
                    let b = mid((cand + k).min(frames - 1));
                    acc += a * b;
                }
                if acc > best_score {
                    best_score = acc;
                    best = cand;
                }
                // A stride: a correlation at every one of a thousand offsets
                // is most of the cost of the whole algorithm, and the peak
                // of a cross-correlation is a period wide, not a sample.
                cand += 4;
            }
            best
        };
        for k in 0..WINDOW {
            let src = (take + k).min(frames.saturating_sub(1));
            let dst = o + k;
            if dst >= out_frames {
                break;
            }
            let w = win[k];
            for ch in 0..2 {
                out[dst * 2 + ch] += at(src, ch) * w;
            }
            norm[dst] += w;
        }
        // The tail this splice leaves behind, for the next one to match.
        tail_at = take + HOP;
        o += HOP;
        want += HOP as f64 / ratio;
    }
    normalise(&mut out, &norm);
    let want_frames = ((frames as f64) * ratio).round().max(1.0) as usize;
    out.truncate(want_frames * 2);
    out
}

/// Divide out the summed window, so an overlap-add does not breathe.
///
/// **A floor under the divisor.** The first and last windows overlap fewer
/// times than the middle ones, so the sum there is small — and dividing by a
/// small number is how an overlap-add grows a burst of noise at each end.
fn normalise(out: &mut [f32], norm: &[f32]) {
    for (f, n) in norm.iter().enumerate() {
        let g = if *n > 0.35 { 1.0 / n } else { 0.0 };
        for ch in 0..2 {
            if let Some(v) = out.get_mut(f * 2 + ch) {
                *v *= g;
            }
        }
    }
}

/// An in-place radix-2 FFT. `re`/`im` are `n` long, `n` a power of two.
///
/// Written here rather than pulled in: it is forty lines, it is the only
/// transform this app needs, and a dependency for it would be a dependency
/// in the licence file and the cross-build for both Linux arches.
fn fft(re: &mut [f32], im: &mut [f32], inverse: bool) {
    let n = re.len();
    if n < 2 || n & (n - 1) != 0 || im.len() != n {
        return;
    }
    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = std::f32::consts::TAU / len as f32 * if inverse { 1.0 } else { -1.0 };
        let (wr, wi) = (ang.cos(), ang.sin());
        let mut i = 0;
        while i < n {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (ur, ui) = (re[i + k], im[i + k]);
                let (vr, vi) = (
                    re[i + k + len / 2] * cr - im[i + k + len / 2] * ci,
                    re[i + k + len / 2] * ci + im[i + k + len / 2] * cr,
                );
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + len / 2] = ur - vr;
                im[i + k + len / 2] = ui - vi;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
            i += len;
        }
        len <<= 1;
    }
    if inverse {
        let s = 1.0 / n as f32;
        for k in 0..n {
            re[k] *= s;
            im[k] *= s;
        }
    }
}

/// Wrap a phase difference into `-pi..=pi`.
fn wrap(x: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut v = x;
    while v > std::f32::consts::PI {
        v -= tau;
    }
    while v < -std::f32::consts::PI {
        v += tau;
    }
    v
}

/// A phase vocoder with peak locking.
///
/// **Both channels are rotated by the SAME phase**, worked out from the mid.
/// Two independent vocoders on left and right drift apart bin by bin, and a
/// stereo image whose two sides disagree about phase is one that collapses
/// in mono and wanders in the room. The cost is that a hard-panned pair is
/// treated as one signal, which is the right trade for a mix.
///
/// **Peak locking** is the difference between this and a textbook vocoder:
/// bins are grouped around spectral peaks and each group is rotated as a
/// unit, so a note's partials keep their relative phases instead of each
/// wandering off on its own. Without it a stretched piano chord has the
/// characteristic vocoder swish, and with it a piano chord mostly sounds
/// like a piano chord.
fn vocoder(clip: &Clip, ratio: f64) -> Vec<f32> {
    let frames = clip.frames();
    let win = hann(WINDOW);
    let out_frames = ((frames as f64) * ratio).round().max(1.0) as usize + WINDOW;
    let mut out = vec![0.0f32; out_frames * 2];
    let mut norm = vec![0.0f32; out_frames];
    let at = |f: usize, ch: usize| clip.samples.get(f * 2 + ch).copied().unwrap_or(0.0);

    let bins = WINDOW / 2 + 1;
    let mut prev_phase = vec![0.0f32; bins];
    let mut sum_phase = vec![0.0f32; bins];
    // The synthesis hop is a whole number of frames; the ANALYSIS hop is the
    // fractional one, so the output lands on a grid and the input is what
    // slides. The other way round leaves gaps in the overlap-add.
    let hop_s = HOP;
    let hop_a = (hop_s as f64 / ratio).max(1.0);
    let expected: Vec<f32> = (0..bins)
        .map(|k| std::f32::consts::TAU * k as f32 * hop_a as f32 / WINDOW as f32)
        .collect();

    let mut re = vec![0.0f32; WINDOW];
    let mut im = vec![0.0f32; WINDOW];
    let mut mag = vec![0.0f32; bins];
    let mut rot_re = vec![0.0f32; bins];
    let mut rot_im = vec![0.0f32; bins];
    let mut ch_re = vec![0.0f32; WINDOW];
    let mut ch_im = vec![0.0f32; WINDOW];

    let mut a = 0.0f64;
    let mut o = 0usize;
    let mut first = true;
    while o + WINDOW < out_frames && (a as usize) < frames {
        let base = a as usize;
        // ── analysis, on the mid ──
        for k in 0..WINDOW {
            let f = (base + k).min(frames.saturating_sub(1));
            re[k] = 0.5 * (at(f, 0) + at(f, 1)) * win[k];
            im[k] = 0.0;
        }
        fft(&mut re, &mut im, false);
        for k in 0..bins {
            mag[k] = (re[k] * re[k] + im[k] * im[k]).sqrt();
            let phase = im[k].atan2(re[k]);
            let delta = wrap(phase - prev_phase[k] - expected[k]);
            prev_phase[k] = phase;
            let true_freq = expected[k] + delta;
            // The synthesis advance: the same true frequency over the LONGER
            // (or shorter) hop, which is the whole trick.
            let adv = true_freq * (hop_s as f32 / hop_a as f32);
            if first {
                sum_phase[k] = phase;
            } else {
                sum_phase[k] = wrap(sum_phase[k] + adv);
            }
            // How far this bin has been rotated from where it was analysed.
            let d = wrap(sum_phase[k] - phase);
            rot_re[k] = d.cos();
            rot_im[k] = d.sin();
        }
        first = false;
        // **Peak locking**: a bin that is not a local maximum takes its
        // rotation from the nearest peak below it, so a partial and its
        // skirts move together.
        {
            let mut peak = 0usize;
            for k in 0..bins {
                let up = k + 1 < bins && mag[k + 1] > mag[k];
                let down = k > 0 && mag[k - 1] > mag[k];
                if !up && !down {
                    peak = k;
                } else {
                    rot_re[k] = rot_re[peak];
                    rot_im[k] = rot_im[peak];
                }
            }
        }
        // ── synthesis, per channel, with the mid's rotation ──
        for ch in 0..2 {
            for k in 0..WINDOW {
                let f = (base + k).min(frames.saturating_sub(1));
                ch_re[k] = at(f, ch) * win[k];
                ch_im[k] = 0.0;
            }
            fft(&mut ch_re, &mut ch_im, false);
            for k in 0..bins {
                let (r, i) = (ch_re[k], ch_im[k]);
                let (nr, ni) = (r * rot_re[k] - i * rot_im[k], r * rot_im[k] + i * rot_re[k]);
                ch_re[k] = nr;
                ch_im[k] = ni;
                // The mirror half, so the inverse transform is real.
                if k > 0 && k < WINDOW - k {
                    ch_re[WINDOW - k] = nr;
                    ch_im[WINDOW - k] = -ni;
                }
            }
            fft(&mut ch_re, &mut ch_im, true);
            for k in 0..WINDOW {
                let dst = o + k;
                if dst >= out_frames {
                    break;
                }
                out[dst * 2 + ch] += ch_re[k] * win[k];
            }
        }
        for k in 0..WINDOW {
            let dst = o + k;
            if dst >= out_frames {
                break;
            }
            norm[dst] += win[k] * win[k];
        }
        o += hop_s;
        a += hop_a;
    }
    normalise(&mut out, &norm);
    let want_frames = ((frames as f64) * ratio).round().max(1.0) as usize;
    out.truncate(want_frames * 2);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn clip_of(samples: Vec<f32>) -> Clip {
        Clip {
            samples,
            rate: RATE,
            source: std::path::PathBuf::from("t.wav"),
        }
    }

    /// A steady tone, interleaved stereo.
    fn tone(hz: f64, seconds: f64) -> Clip {
        let n = (f64::from(RATE) * seconds) as usize;
        clip_of(
            (0..n)
                .flat_map(|f| {
                    let v = (std::f64::consts::TAU * hz * f as f64 / f64::from(RATE)).sin()
                        as f32
                        * 0.5;
                    [v, v]
                })
                .collect(),
        )
    }

    /// A chord: three partials, sustained. What `Tonal` is for.
    fn chord(seconds: f64) -> Clip {
        let n = (f64::from(RATE) * seconds) as usize;
        clip_of(
            (0..n)
                .flat_map(|f| {
                    let t = f as f64 / f64::from(RATE);
                    let v = [220.0, 277.18, 329.63]
                        .iter()
                        .map(|hz| (std::f64::consts::TAU * hz * t).sin())
                        .sum::<f64>() as f32
                        * 0.2;
                    [v, v]
                })
                .collect(),
        )
    }

    /// A plucked note: an attack and a long decay, four times over. The
    /// material that sits just on the rhythmic side of the line.
    fn plucked(seconds: f64) -> Clip {
        let n = (f64::from(RATE) * seconds) as usize;
        let period = (f64::from(RATE) * 0.5) as usize;
        clip_of(
            (0..n)
                .flat_map(|f| {
                    let k = f % period;
                    let env = (-(k as f32) / (f64::from(RATE) as f32 * 0.35)).exp();
                    let v = (std::f32::consts::TAU * 196.0 * k as f32
                        / f64::from(RATE) as f32)
                        .sin()
                        * env
                        * 0.6;
                    [v, v]
                })
                .collect(),
        )
    }

    /// A chord with a click track over it: an ordinary mix.
    fn with_drums(seconds: f64) -> Clip {
        let (a, b) = (chord(seconds), clicks(seconds, 0.25));
        clip_of(
            a.samples
                .iter()
                .zip(b.samples.iter())
                .map(|(x, y)| 0.6 * x + 0.5 * y)
                .collect(),
        )
    }

    /// A click track: a short decaying burst every `every` seconds. What
    /// `Rhythmic` is for.
    fn clicks(seconds: f64, every: f64) -> Clip {
        let n = (f64::from(RATE) * seconds) as usize;
        let period = (f64::from(RATE) * every) as usize;
        clip_of(
            (0..n)
                .flat_map(|f| {
                    let k = f % period;
                    let env = (-(k as f32) / (f64::from(RATE) as f32 * 0.01)).exp();
                    let v = (std::f32::consts::TAU * 900.0 * k as f32
                        / f64::from(RATE) as f32)
                        .sin()
                        * env
                        * 0.7;
                    [v, v]
                })
                .collect(),
        )
    }

    /// The frequency of a signal, by zero crossings over its settled middle.
    fn hz_of(clip: &Clip) -> f64 {
        let frames = clip.frames();
        let from = frames / 4;
        let to = frames * 3 / 4;
        let l: Vec<f32> = (from..to).map(|f| clip.samples[f * 2]).collect();
        let crossings = l.windows(2).filter(|w| (w[0] <= 0.0) != (w[1] <= 0.0)).count();
        crossings as f64 / 2.0 / (l.len() as f64 / f64::from(RATE))
    }

    /// **The definition of the feature**: the clip gets longer and the pitch
    /// does not move.
    ///
    /// Both algorithms, both directions. This is what separates a
    /// time-stretch from the varispeed dial, which does the opposite on
    /// purpose.
    #[test]
    fn stretching_changes_the_length_and_not_the_pitch() {
        for mode in [Mode::Rhythmic, Mode::Tonal] {
            for ratio in [0.5_f64, 0.75, 1.5, 2.0] {
                let src = tone(440.0, 2.0);
                let out = stretch(&src, ratio, mode);
                let want = (src.frames() as f64 * ratio).round() as usize;
                assert_eq!(
                    out.frames(),
                    want,
                    "{mode:?} at {ratio}: {} frames, not {want}",
                    out.frames()
                );
                let got = hz_of(&out);
                assert!(
                    (got - 440.0).abs() < 12.0,
                    "{mode:?} at {ratio}: the tone came out at {got:.1} Hz"
                );
            }
        }
    }

    /// **Tape is the opposite, and it is meant to be.** It is in the list
    /// because for a one-percent correction a third of a semitone is nothing
    /// and a splice is not.
    #[test]
    fn tape_moves_the_pitch_by_the_ratio() {
        for ratio in [0.5_f64, 2.0] {
            let out = stretch(&tone(440.0, 2.0), ratio, Mode::Tape);
            let got = hz_of(&out);
            let want = 440.0 / ratio;
            assert!(
                (got - want).abs() / want < 0.03,
                "at {ratio}, tape gave {got:.1} Hz where {want:.1} was wanted"
            );
        }
    }

    /// The stretched audio is audio: no NaNs, and nothing that was under
    /// full scale comes out over it.
    #[test]
    fn nothing_comes_out_broken_or_louder_than_it_went_in() {
        for mode in [Mode::Tape, Mode::Rhythmic, Mode::Tonal] {
            for ratio in [0.25_f64, 0.5, 1.5, 4.0] {
                for src in [tone(220.0, 1.0), chord(1.0), clicks(1.0, 0.25)] {
                    let peak_in = src.samples.iter().fold(0.0f32, |a, b| a.max(b.abs()));
                    let out = stretch(&src, ratio, mode);
                    assert!(
                        out.samples.iter().all(|v| v.is_finite()),
                        "{mode:?} at {ratio} produced a non-finite sample"
                    );
                    let peak = out.samples.iter().fold(0.0f32, |a, b| a.max(b.abs()));
                    assert!(
                        peak <= peak_in * 1.6 + 0.02,
                        "{mode:?} at {ratio}: {peak:.3} out against {peak_in:.3} in"
                    );
                    // ...and it is not silence, which every other assertion
                    // here would pass on.
                    assert!(peak > peak_in * 0.25, "{mode:?} at {ratio} came out at {peak:.3}");
                }
            }
        }
    }

    /// **The overlap-add does not breathe at the ends.**
    ///
    /// The first and last windows overlap fewer times than the middle ones,
    /// so the summed window is small there — and dividing by a small number
    /// is how an overlap-add grows a burst at each end. `normalise` puts a
    /// floor under the divisor; this is what says the floor is doing its
    /// job.
    #[test]
    fn the_ends_are_not_louder_than_the_middle() {
        for mode in [Mode::Rhythmic, Mode::Tonal] {
            let out = stretch(&tone(440.0, 1.0), 1.5, mode);
            let n = out.frames();
            let peak_of = |from: usize, to: usize| {
                (from..to.min(n)).fold(0.0f32, |a, f| a.max(out.samples[f * 2].abs()))
            };
            let middle = peak_of(n / 3, n * 2 / 3);
            let head = peak_of(0, n / 20);
            let tail = peak_of(n - n / 20, n);
            assert!(
                head <= middle * 1.3,
                "{mode:?}: the head peaked at {head:.3} against {middle:.3}"
            );
            assert!(
                tail <= middle * 1.3,
                "{mode:?}: the tail peaked at {tail:.3} against {middle:.3}"
            );
        }
    }

    /// **Auto listens to the material.**
    ///
    /// A click track gets the algorithm that keeps edges; a held chord gets
    /// the one that keeps tone; and a correction of a percent gets the one
    /// with no artefacts at all, whatever it is made of.
    #[test]
    fn auto_picks_by_what_the_clip_sounds_like() {
        let drums = clicks(2.0, 0.25);
        let pad = chord(2.0);
        // **The five signals the threshold was set from**, in order, so the
        // boundary is written down rather than remembered. A change to the
        // measure that keeps the two ends apart and moves the middle is a
        // change this catches.
        let scored: Vec<(&str, f32)> = vec![
            ("a steady tone", percussiveness(&tone(440.0, 2.0))),
            ("a held chord", percussiveness(&pad)),
            ("a plucked note", percussiveness(&plucked(2.0))),
            ("a mix with drums in it", percussiveness(&with_drums(2.0))),
            ("a click track", percussiveness(&drums)),
        ];
        for w in scored.windows(2) {
            assert!(
                w[1].1 >= w[0].1,
                "{} scored {:.2} and {} scored {:.2} - the order is wrong",
                w[0].0,
                w[0].1,
                w[1].0,
                w[1].1
            );
        }
        // Tonal below the line, rhythmic above it, and the line falls
        // between a held chord and a plucked note — which is where a
        // musician would put it.
        for (name, v) in &scored[..2] {
            assert!(*v < PERCUSSIVE_AT, "{name} measured {v:.2} and wants the tonal side");
        }
        for (name, v) in &scored[2..] {
            assert!(*v >= PERCUSSIVE_AT, "{name} measured {v:.2} and wants the rhythmic side");
        }
        // **Silence at the ends is not an attack.** A pad with three seconds
        // of room tone in front of it is a pad; left in the measure, that
        // shape is the single biggest rise in the file.
        let padded = {
            let mut s = vec![0.0f32; (f64::from(RATE) * 1.5) as usize * 2];
            s.extend_from_slice(&pad.samples);
            s.extend(std::iter::repeat(0.0).take((f64::from(RATE) * 1.5) as usize * 2));
            clip_of(s)
        };
        assert!(
            (percussiveness(&padded) - percussiveness(&pad)).abs() < 0.1,
            "silence at the ends moved a pad from {:.2} to {:.2}",
            percussiveness(&pad),
            percussiveness(&padded)
        );
        assert_eq!(Mode::Auto.pick(&drums, 1.5), Mode::Rhythmic);
        assert_eq!(Mode::Auto.pick(&pad, 1.5), Mode::Tonal);
        // A hair off unity is the tape's, whatever the material: two percent
        // is a third of a semitone and a splice is a splice.
        assert_eq!(Mode::Auto.pick(&drums, 1.01), Mode::Tape);
        assert_eq!(Mode::Auto.pick(&pad, 0.99), Mode::Tape);
        // And a mode somebody chose is never second-guessed.
        for m in [Mode::Tape, Mode::Rhythmic, Mode::Tonal] {
            assert_eq!(m.pick(&drums, 2.0), m);
        }
    }

    /// **Rhythmic keeps the edges where they belong.**
    ///
    /// The reason it exists: a click every quarter second, stretched to
    /// double, is a click every half second — still a click, still where the
    /// arithmetic says.
    #[test]
    fn rhythmic_keeps_the_clicks_apart_and_sharp() {
        let src = clicks(2.0, 0.25);
        let out = stretch(&src, 2.0, Mode::Rhythmic);
        // Find the attacks: a frame well above the local floor, with a gap
        // enforced so one click is not counted as several.
        let peak = out.samples.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        let mut hits: Vec<usize> = Vec::new();
        let mut f = 0usize;
        while f < out.frames() {
            if out.samples[f * 2].abs() > peak * 0.5 {
                hits.push(f);
                f += RATE as usize / 20;
            } else {
                f += 1;
            }
        }
        assert!(hits.len() >= 6, "only {} clicks survived", hits.len());
        for w in hits.windows(2) {
            let gap = (w[1] - w[0]) as f64 / f64::from(RATE);
            assert!(
                (gap - 0.5).abs() < 0.06,
                "two clicks landed {gap:.3}s apart, not 0.5"
            );
        }
    }

    /// The transform round-trips, which is the one thing the vocoder cannot
    /// work without and the one thing forty hand-written lines can get
    /// quietly wrong.
    #[test]
    fn the_transform_comes_back_to_where_it_started() {
        let n = 64;
        let orig: Vec<f32> = (0..n)
            .map(|i| (i as f32 * 0.37).sin() + (i as f32 * 1.9).cos() * 0.3)
            .collect();
        let mut re = orig.clone();
        let mut im = vec![0.0f32; n];
        fft(&mut re, &mut im, false);
        // A real signal's spectrum is conjugate-symmetric. If it is not,
        // the transform is wrong in a way that still round-trips.
        for k in 1..n / 2 {
            assert!((re[k] - re[n - k]).abs() < 1e-3, "bin {k} is not symmetric");
            assert!((im[k] + im[n - k]).abs() < 1e-3, "bin {k} is not symmetric");
        }
        fft(&mut re, &mut im, true);
        for i in 0..n {
            assert!(
                (re[i] - orig[i]).abs() < 1e-3,
                "sample {i} came back as {} rather than {}",
                re[i],
                orig[i]
            );
            assert!(im[i].abs() < 1e-3, "sample {i} grew an imaginary part");
        }
        // A pure bin is a pure bin: eight cycles over 64 points is bin 8.
        let mut re: Vec<f32> = (0..n)
            .map(|i| (std::f32::consts::TAU * 8.0 * i as f32 / n as f32).sin())
            .collect();
        let mut im = vec![0.0f32; n];
        fft(&mut re, &mut im, false);
        let mags: Vec<f32> = (0..n / 2).map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt()).collect();
        let loudest = mags
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(k, _)| k);
        assert_eq!(loudest, Some(8), "the transform put 8 cycles in the wrong bin");
    }

    /// **The two sides of the firewall name the same four algorithms.**
    ///
    /// `tangent-ui` cannot see this module, so the clip menu carries its own
    /// copy of the keys. This is what stops the two lists drifting into a
    /// menu row that selects nothing.
    #[test]
    fn the_menus_modes_are_this_modules_modes() {
        let ours: Vec<&str> = Mode::ALL.iter().map(|m| m.key()).collect();
        assert_eq!(ours, MODE_KEYS, "the module's own list moved");
        let theirs: Vec<&str> = tangent_ui::arrangement::STRETCH_MODES
            .iter()
            .map(|(k, _)| *k)
            .collect();
        assert_eq!(ours, theirs, "the clip menu offers a different set");
        for (k, _) in tangent_ui::arrangement::STRETCH_MODES {
            assert!(Mode::from_key(k).is_some(), "the menu offers `{k}`, which is not a mode");
        }
        // An empty string is what a clip that has never been told carries,
        // and it has to read as auto rather than as an error.
        assert_eq!(Mode::from_key("").unwrap_or_default(), Mode::Auto);
        assert_eq!(Mode::from_key("spectral-9").unwrap_or_default(), Mode::Auto);
    }

    /// Unity, nonsense and an empty clip all hand back something playable.
    #[test]
    fn the_edges_of_the_ratio_are_all_safe() {
        let src = tone(440.0, 0.2);
        for mode in Mode::ALL {
            assert_eq!(stretch(&src, 1.0, mode).samples, src.samples, "{mode:?}");
            assert_eq!(stretch(&src, f64::NAN, mode).samples, src.samples, "{mode:?}");
            // Past the limits, CLAMPED rather than refused: a drag that ran
            // off the end of its travel is a drag that meant "as far as it
            // goes".
            let far = stretch(&src, 100.0, mode);
            let cap = (src.frames() as f64 * RATIO.1).round() as usize;
            assert_eq!(far.frames(), cap, "{mode:?} at 100x");
            let tiny = stretch(&src, 0.0001, mode);
            let floor = (src.frames() as f64 * RATIO.0).round() as usize;
            assert_eq!(tiny.frames(), floor, "{mode:?} at 0.0001x");
        }
        // An empty clip is not a crash.
        let empty = clip_of(Vec::new());
        for mode in Mode::ALL {
            assert_eq!(stretch(&empty, 2.0, mode).frames(), 0, "{mode:?}");
        }
        assert_eq!(percussiveness(&empty), 0.0);
    }

    /// **Stereo survives.** Both algorithms make one decision and apply it to
    /// both sides; a pair aligned independently is an image that collapses.
    #[test]
    fn the_two_channels_stay_together() {
        let n = (f64::from(RATE) * 1.0) as usize;
        // The two sides a quarter-cycle apart: a phase relationship that a
        // per-channel stretcher would scramble.
        let src = clip_of(
            (0..n)
                .flat_map(|f| {
                    let t = f as f64 / f64::from(RATE);
                    let l = (std::f64::consts::TAU * 300.0 * t).sin() as f32 * 0.5;
                    let r = (std::f64::consts::TAU * 300.0 * t
                        + std::f64::consts::FRAC_PI_2)
                        .sin() as f32
                        * 0.5;
                    [l, r]
                })
                .collect(),
        );
        for mode in [Mode::Tape, Mode::Rhythmic, Mode::Tonal] {
            let out = stretch(&src, 1.5, mode);
            // The correlation between the two sides is what a quarter-cycle
            // offset makes it — near zero — and NOT one (collapsed to mono)
            // or minus one (one side inverted).
            let from = out.frames() / 4;
            let to = out.frames() * 3 / 4;
            let (mut ll, mut rr, mut lr) = (0.0f64, 0.0f64, 0.0f64);
            for f in from..to {
                let l = f64::from(out.samples[f * 2]);
                let r = f64::from(out.samples[f * 2 + 1]);
                ll += l * l;
                rr += r * r;
                lr += l * r;
            }
            let corr = lr / (ll.sqrt() * rr.sqrt()).max(1e-12);
            assert!(
                corr.abs() < 0.35,
                "{mode:?}: the two sides came out {corr:.2} correlated, \
                 where they went in a quarter-cycle apart"
            );
        }
    }
}
