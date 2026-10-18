//! The wah patch's pedal: an envelope follower dragging a resonant band-pass
//! up with the playing. One per instrument — the pedal sits on the sum, as
//! the suitcase's tremolo does — and the AMP side of the frame calls it per
//! sample.
//!
//! **What it is voiced to.** The reference's envelope-wah preset, measured as
//! an effect: the same events rendered with the pedal in and out, the ratio of
//! the two spectra per 23 ms frame (`effects.py wah`). Its resonance stands
//! +13 dB over the dry signal at rest and +11 open, with a −3 dB span of
//! 0.29..0.43 octave by that tool (a Q near 4), and its centre rests near
//! 380 Hz. On single notes it stays at
//! rest up to about velocity 70, then climbs about a quarter octave per
//! decibel of the note's level — C3 / C4 / C5 at velocity 127 reach 0.9 /
//! 1.4 / 1.7 kHz in their first 300 ms — and it saturates under 2 kHz. The
//! threshold is a LEVEL, so it is set here relative to this instrument's
//! own level: the follower's peak on a fresh C4 at velocity 127
//! ([`WAH_REF`]), measured and pinned, and the opening point 7.5 dB under
//! it. The follower listens through a first-order high-pass at 600 Hz, as
//! a pedal's detector that keys on the pick rather than the body does, and
//! rises over 10 ms: fitted on this instrument's own dry renders against
//! the reference's per-note centres (C3/C4/C5 × nine velocities, the peak
//! in the first 300 ms and the median over 0.6..1.1 s): all 27 within a
//! third of an octave in the first 300 ms by the clav verifier's frames,
//! 22 of 27 late.
//!
//! **Real-time contract.** Sized in `new`, no allocation, no locks; per
//! sample a follower, one `log10`, one `exp2`, one `powf` for the peak's
//! height and a zero-delay-feedback state-variable filter per channel,
//! stable at any centre.

/// The follower's attack and release, seconds: up in a few cycles of the
/// note, down over the note's first second — the reference's centre peaks
/// in the first 300 ms and is still 0.9 octave over rest at 0.6..1.1 s on
/// a loud note, which a follower that rides the strike's transient cannot
/// do: it reads the note's sustained level instead.
const ATTACK_SECS: f32 = 0.010;
const RELEASE_SECS: f32 = 0.600;

/// Where the pedal rests and where it stops, Hz.
const REST_HZ: f32 = 380.0;
const CEILING_HZ: f32 = 1700.0;

/// How fast the centre climbs, octaves per decibel of the follower above
/// the threshold: the reference's C3 / C4 / C5 rise from velocity 70 to
/// 127 is 1.04 / 1.15 / 1.23 octaves over 5.1 / 5.5 / 5.7 dB of its level.
const OCT_PER_DB: f32 = 0.26;

/// The detector's high-pass, Hz.
const DETECT_HZ: f32 = 600.0;

/// **The follower's level for a fresh C4 at velocity 127**, through the
/// detector, as this instrument plays it — the reference point the
/// threshold is measured from (`the_wah_reference_is_a_c4_at_fortissimo`
/// measures it).
pub(super) const WAH_REF: f32 = 0.1135;

/// The threshold at the knob's default, dB re [`WAH_REF`]: the reference
/// opens between velocity 70 and 85. Re-fitted when the voice's output
/// stage made the follower read a fortissimo C4 10 % lower (0.1261 →
/// 0.1135): −8.2 dB put C3 at v127 a third of an octave over the
/// reference's 923 Hz and C5 at v100 0.37 over its 1312.
const THRESHOLD_DB: f32 = -7.5;

/// What the WAH knob moves: the threshold, by this many dB per unit of
/// knob away from the default — "how hard the filter chases your touch".
const KNOB_DB: f32 = 30.0;

/// The default the knob ships at, where the threshold is the measured one.
const KNOB_DEFAULT: f32 = 0.55;

/// **The resonance**: Q and the peak's height over the dry signal, at rest
/// and fully open. Q 6 read 0.19..0.32 octave in `effects.py`'s frames on
/// the fx score, under the reference's 0.29..0.43 by the same tool (its
/// 1/6-octave smoothing widens a peak, but by the same for both); Q 4.2
/// reads 0.27..0.37. The reference's peak stands +13.1..+13.4 dB at rest
/// and +11.2 open, so the height falls with the opening, in dB linear in
/// octaves; the whole 1 dB under the reference's keeps the patch's loudest
/// single note at −3.25 dBFS and a five-note v127 bass chord at −2.1 (the
/// reference's rest boosts a bass note's strongest partials at 380 Hz).
/// The loudest bin of a pair of renders still reads +17..+19 dB: its
/// frames are the strikes' first 40 ms, where the centre runs up in 10 ms
/// and |W/D| on a frame is not the filter's response.
const Q: f32 = 4.2;
const PEAK_DB: f32 = 12.5;
const PEAK_OPEN_DB: f32 = 10.5;

/// What of the low-pass output the pedal lets through beside the
/// resonance: the reference's off-peak floor is 12..22 dB under the dry
/// signal, which a pure band-pass would put 25 dB down a few octaves out.
const LOW_SHARE: f32 = 0.08;

/// The pedal.
pub struct Wah {
    rate: f32,
    env: f32,
    atk: f32,
    rel: f32,
    /// The detector's high-pass: its pole and its last input, per channel.
    det_b: f32,
    det_x: [f32; 2],
    det_y: [f32; 2],
    /// The two integrators of the state-variable filter, per channel.
    ic1: [f32; 2],
    ic2: [f32; 2],
    /// The band output's weight: the peak's height over the dry signal,
    /// divided by the filter's own peak gain `Q`; set per sample from how
    /// far the pedal is open.
    wet: f32,
    bypass: bool,
}

impl Wah {
    pub fn new(rate: f32) -> Self {
        let rate = if rate > 0.0 { rate } else { 48_000.0 };
        Wah {
            rate,
            env: 0.0,
            atk: 1.0 - (-1.0 / (ATTACK_SECS * rate)).exp(),
            rel: 1.0 - (-1.0 / (RELEASE_SECS * rate)).exp(),
            det_b: (-std::f32::consts::TAU * DETECT_HZ / rate).exp(),
            det_x: [0.0; 2],
            det_y: [0.0; 2],
            ic1: [0.0; 2],
            ic2: [0.0; 2],
            wet: 10.0_f32.powf(PEAK_DB / 20.0) / Q,
            bypass: false,
        }
    }

    /// Silence: what a patch change does.
    pub fn reset(&mut self) {
        self.env = 0.0;
        self.det_x = [0.0; 2];
        self.det_y = [0.0; 2];
        self.ic1 = [0.0; 2];
        self.ic2 = [0.0; 2];
    }

    /// The pedal out of circuit: the bench renders the wah patch through it
    /// both ways to measure the wah as an effect.
    #[allow(dead_code)]
    pub fn set_bypass(&mut self, on: bool) {
        self.bypass = on;
    }

    /// The centre for a follower level `env` and a knob `sens`, Hz: what
    /// the tests read the law through (`process` takes the opening itself,
    /// for the peak's height as well).
    #[cfg(test)]
    pub(super) fn centre(env: f32, sens: f32) -> f32 {
        REST_HZ * Self::opening(env, sens).exp2()
    }

    /// How far the pedal is open, octaves over rest.
    fn opening(env: f32, sens: f32) -> f32 {
        let thr = THRESHOLD_DB - (sens.clamp(0.0, 1.0) - KNOB_DEFAULT) * KNOB_DB;
        let over = 20.0 * (env.max(1.0e-9) / WAH_REF).log10() - thr;
        (OCT_PER_DB * over.max(0.0)).min((CEILING_HZ / REST_HZ).log2())
    }

    /// One stereo sample through the pedal.
    #[inline]
    pub fn process(&mut self, left: f32, right: f32, sens: f32) -> (f32, f32) {
        // The detector: a first-order high-pass per channel, unity at the
        // top, then the mean of the two magnitudes.
        let b = self.det_b;
        let g = 0.5 * (1.0 + b);
        let mut lvl = 0.0_f32;
        for (ch, x) in [left, right].into_iter().enumerate() {
            let y = g * (x - self.det_x[ch]) + b * self.det_y[ch];
            self.det_x[ch] = x;
            self.det_y[ch] = if y.is_finite() { y } else { 0.0 };
            lvl += 0.5 * y.abs();
        }
        let c = if lvl > self.env { self.atk } else { self.rel };
        self.env += (lvl - self.env) * c;
        if self.bypass {
            return (left, right);
        }
        let oct = Self::opening(self.env, sens);
        let fc = REST_HZ * oct.exp2();
        let open = oct / (CEILING_HZ / REST_HZ).log2();
        self.wet = 10.0_f32.powf((PEAK_DB + (PEAK_OPEN_DB - PEAK_DB) * open) / 20.0) / Q;
        self.filter(left, right, fc)
    }

    /// The resonance at a centre of `fc` Hz, one stereo sample.
    #[inline]
    fn filter(&mut self, left: f32, right: f32, fc: f32) -> (f32, f32) {
        // The zero-delay-feedback state-variable filter (Zavalishin's
        // trapezoidal form): `g = tan(π fc / fs)`, here to fifth order — at
        // the 2 kHz ceiling and 44.1 kHz the argument is 0.14 and the
        // truncation 2e−6.
        let x = std::f32::consts::PI * fc / self.rate;
        let x2 = x * x;
        let g = x * (1.0 + x2 * (1.0 / 3.0 + x2 * (2.0 / 15.0)));
        let k = 1.0 / Q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let mut out = [left, right];
        for (ch, o) in out.iter_mut().enumerate() {
            let v3 = *o - self.ic2[ch];
            let v1 = a1 * self.ic1[ch] + a2 * v3;
            let v2 = self.ic2[ch] + a2 * self.ic1[ch] + a3 * v3;
            self.ic1[ch] = 2.0 * v1 - self.ic1[ch];
            self.ic2[ch] = 2.0 * v2 - self.ic2[ch];
            if !self.ic1[ch].is_finite() || !self.ic2[ch].is_finite() {
                self.ic1[ch] = 0.0;
                self.ic2[ch] = 0.0;
            }
            *o = v1 * self.wet + v2 * LOW_SHARE;
        }
        (out[0], out[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::clav::{ClavBase, ClavVoice};
    use crate::keys::{ClavKnobs, Patch};

    const RATE: f32 = 48_000.0;

    /// **The law, as fitted.** At rest 380 Hz (the reference's rest, its
    /// binned median 379..401 Hz); 0.26 octave per dB over a threshold 7.5
    /// dB under the C4 fortissimo reference; the ceiling at 1.7 kHz (the
    /// reference's C5 at v115 and v127); the knob moving the threshold, the
    /// shipped 0.55 exactly the measured one.
    #[test]
    fn the_wah_rests_climbs_and_stops_where_the_reference_does() {
        assert_eq!(Wah::centre(0.0, KNOB_DEFAULT), REST_HZ);
        let at = |db_over_thr: f32| WAH_REF * 10.0_f32.powf((THRESHOLD_DB + db_over_thr) / 20.0);
        assert!((Wah::centre(at(0.0), KNOB_DEFAULT) - REST_HZ).abs() < 1.0);
        let five = Wah::centre(at(5.0), KNOB_DEFAULT);
        assert!((five / (REST_HZ * (OCT_PER_DB * 5.0).exp2()) - 1.0).abs() < 1e-4, "{five}");
        assert!((Wah::centre(at(40.0), KNOB_DEFAULT) - CEILING_HZ).abs() < 0.5);
        // More knob, an earlier pedal: at the top it opens 13.5 dB sooner.
        assert!(Wah::centre(at(-6.0), 1.0) > REST_HZ * 2.0);
        assert_eq!(Wah::centre(at(6.0), 0.0), REST_HZ);
        let d = Patch::ClavWah.defaults()[4];
        assert_eq!(d, KNOB_DEFAULT, "the shipped knob is not the measured law");
    }

    /// The resonance itself, a steady sine through the filter at a fixed
    /// centre: +12.5 dB over the dry signal at rest and +10.5 fully open,
    /// and the −3 dB points 0.25..0.45 octave apart, the brief's band (the
    /// reference's frames read 0.29..0.43; this filter's read 0.27..0.37 on
    /// the fx score, its own span here 0.34).
    #[test]
    fn the_resonance_is_its_measured_height_and_width() {
        let gain = |f: f32, fc: f32| {
            let mut w = Wah::new(RATE);
            let open = (fc / REST_HZ).log2() / (CEILING_HZ / REST_HZ).log2();
            w.wet = 10.0_f32.powf((PEAK_DB + (PEAK_OPEN_DB - PEAK_DB) * open) / 20.0) / Q;
            let mut pk = 0.0_f32;
            for i in 0..(RATE as usize) {
                let x = (std::f32::consts::TAU * f * i as f32 / RATE).sin();
                let (l, _) = w.filter(x, x, fc);
                if i > RATE as usize / 2 {
                    pk = pk.max(l.abs());
                }
            }
            20.0 * pk.log10()
        };
        for (fc, want) in [(380.0_f32, PEAK_DB), (1000.0, PEAK_DB + (PEAK_OPEN_DB - PEAK_DB) * (1000.0_f32 / REST_HZ).log2() / (CEILING_HZ / REST_HZ).log2()), (1700.0, PEAK_OPEN_DB)] {
            let top = gain(fc, fc);
            let mut lo = fc;
            while gain(lo, fc) > top - 3.0 {
                lo /= 1.01;
            }
            let mut hi = fc;
            while gain(hi, fc) > top - 3.0 {
                hi *= 1.01;
            }
            let span = (hi / lo).log2();
            println!("  fc {fc}: peak {top:+.1} dB re dry, -3 dB span {span:.2} oct");
            assert!((top - want).abs() < 0.5, "fc {fc}: peak {top:+.1} dB, wanted {want:+.1}");
            assert!((0.25..=0.45).contains(&span), "fc {fc}: span {span:.2} oct");
        }
    }

    /// An in-place radix-2 FFT, `re`/`im` of a power-of-two length.
    fn fft(re: &mut [f64], im: &mut [f64]) {
        let n = re.len();
        let mut j = 0;
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
        let mut len = 2;
        while len <= n {
            let ang = -std::f64::consts::TAU / len as f64;
            for start in (0..n).step_by(len) {
                for k in 0..len / 2 {
                    let (s, c) = (ang * k as f64).sin_cos();
                    let (a, b) = (start + k, start + k + len / 2);
                    let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len <<= 1;
        }
    }

    /// **The pedal's centre, read as the reference's was read**: the clav
    /// verifier's frames on a (dry, wet) pair of one note — 2048-sample
    /// Hann frames every 256, `|W/D|` on the bins within 40 dB of the
    /// frame's loudest between 150 Hz and 6 kHz, smoothed over a twelfth of
    /// an octave on a log grid, the centre its argmax — and the highest
    /// centre from 20 to 300 ms.
    fn frame_centre(dry: &[f32], wet: &[f32]) -> f64 {
        let n = 2048;
        let df = f64::from(RATE) / n as f64;
        let grid: Vec<f64> = (0..240).map(|i| 150.0_f64.log2() + (6000.0_f64 / 150.0).log2() * f64::from(i) / 239.0).collect();
        let mut best = 0.0_f64;
        let mut at = 0;
        while at + n <= dry.len() {
            let t = (at + n / 2) as f64 / f64::from(RATE);
            if t > 0.02 && t < 0.3 {
                let spec = |x: &[f32]| {
                    let mut re: Vec<f64> = (0..n).map(|i| f64::from(x[at + i]) * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos())).collect();
                    let mut im = vec![0.0; n];
                    fft(&mut re, &mut im);
                    (0..=n / 2).map(|k| re[k].hypot(im[k])).collect::<Vec<f64>>()
                };
                let (d, w) = (spec(dry), spec(wet));
                let lo = (150.0 / df).ceil() as usize;
                let hi = (6000.0 / df) as usize;
                let top = d[lo..=hi].iter().fold(0.0_f64, |m, &v| m.max(v));
                let pts: Vec<(f64, f64)> = (lo..=hi).filter(|&k| d[k] > top * 1e-2).map(|k| ((k as f64 * df).log2(), 20.0 * (w[k] / d[k]).log10())).collect();
                let mut peak = (f64::MIN, 0.0);
                for &g in &grid {
                    let near: Vec<f64> = pts.iter().filter(|p| (p.0 - g).abs() < 1.0 / 12.0).map(|p| p.1).collect();
                    if !near.is_empty() {
                        let m = near.iter().sum::<f64>() / near.len() as f64;
                        if m > peak.0 {
                            peak = (m, g);
                        }
                    }
                }
                best = best.max(peak.1.exp2());
            }
            at += 256;
        }
        best
    }

    /// **[`WAH_REF`] is a fresh C4 at fortissimo**, measured: the wah
    /// patch's voice rendered through its block path into the pedal,
    /// pedal out, the follower's peak in the first 300 ms. And the pedal
    /// opens with the touch where the reference's does, within a third of
    /// an octave, measured the way the reference was ([`frame_centre`]):
    /// its centres are 449 / 574 / 885 Hz at C3 v40 / v85 / v127, 602 /
    /// 845 / 1406 at C4, 602 / 1151 / 1666 at C5 (the clav verifier's
    /// frames on this lane's reference renders; 602 is how they read C4's
    /// and C5's partials through a pedal at rest).
    #[test]
    fn the_wah_reference_is_a_c4_at_fortissimo() {
        let k = ClavKnobs::from_params_wah(&Patch::ClavWah.defaults());
        // One note's first 330 ms through the pedal: (dry, wet) mono, and
        // the follower's peak.
        let note = |key: u8, vel: f32| {
            let mut v = ClavVoice::new(RATE);
            v.strike_based(key, vel, &k, &ClavBase::WAH);
            let mut w = Wah::new(RATE);
            let (mut dry, mut wet, mut top) = (Vec::new(), Vec::new(), 0.0_f32);
            for _ in 0..((0.33 * RATE) as usize / 128) {
                let (mut l, mut r, mut d) = ([0.0_f32; 128], [0.0_f32; 128], [0.0_f32; 128]);
                v.render(&mut l, &mut r, &mut d);
                for i in 0..128 {
                    let (wl, wr) = w.process(l[i], r[i], KNOB_DEFAULT);
                    top = top.max(w.env);
                    dry.push(0.5 * (l[i] + r[i]));
                    wet.push(0.5 * (wl + wr));
                }
            }
            (dry, wet, top)
        };
        let (_, _, c4) = note(60, 1.0);
        println!("  C4 v127 follower peak {c4:.4} (WAH_REF {WAH_REF})");
        assert!((c4 / WAH_REF - 1.0).abs() < 0.05, "the reference level moved: {c4:.4}");
        for (key, vel, want) in [
            (48u8, 40u8, 449.0_f64),
            (48, 85, 574.0),
            (48, 127, 885.0),
            (60, 40, 602.0),
            (60, 85, 845.0),
            (60, 127, 1406.0),
            (72, 40, 602.0),
            (72, 85, 1151.0),
            (72, 127, 1666.0),
        ] {
            let (dry, wet, top) = note(key, f32::from(vel) / 127.0);
            let fc = frame_centre(&dry, &wet);
            let oct = (fc / want).log2();
            println!("  key {key} v{vel}: centre {fc:.0} Hz (the law at the follower's peak {:.0}), reference {want:.0} ({oct:+.2} oct)", Wah::centre(top, KNOB_DEFAULT));
            assert!(oct.abs() <= 1.0 / 3.0, "key {key} v{vel}: centre {fc:.0} against {want:.0}");
        }
    }

    /// **The pedal keeps its headroom.** Every key from F1 to E6 at MIDI
    /// 127, alone, through the frame and the pedal: no channel over −3 dBFS
    /// (the reference's pedal sits 2..5 dB over its dry signal; this one
    /// measured +2.7 / +3.9 dB peak / RMS over the fx score against the
    /// reference's +4.3 / +4.7, so the level is the dry patch's to keep).
    #[test]
    fn the_pedal_keeps_its_headroom() {
        use crate::keys::Keys;
        let mut worst = (0u8, f32::MIN);
        for key in 29u8..=88 {
            let mut k = Keys::new(RATE);
            k.set_patch(Patch::ClavWah);
            k.note_on(key, 1.0);
            let mut buf = vec![0.0_f32; 256];
            let mut pk = 0.0_f32;
            for _ in 0..((0.5 * RATE) as usize / 128) {
                buf.fill(0.0);
                k.render(&mut buf, 128, 2);
                pk = buf.iter().fold(pk, |m, &v| m.max(v.abs()));
            }
            let d = 20.0 * pk.log10();
            if d > worst.1 {
                worst = (key, d);
            }
        }
        println!("  the wah patch's loudest single note: key {} at {:+.2} dBFS", worst.0, worst.1);
        assert!(worst.1 <= -3.0, "key {} peaks at {:+.2} dBFS through the pedal", worst.0, worst.1);
    }

    /// **A reset pedal is a new one.** After a history of signal at random
    /// levels and sensitivities, `reset` leaves the follower, the detector
    /// and the filter at rest: the same signal from there renders what a
    /// new pedal renders, to the bit. The band's weight is left as it was
    /// on purpose, because every sample sets it before the filter reads it;
    /// the bypass is the bench's.
    #[test]
    fn a_reset_wah_is_a_new_one() {
        let mut seed = 0x3a7f_0c11_u32;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32
        };
        let rate = 48_000.0_f32;
        for trial in 0..10 {
            let mut used = Wah::new(rate);
            let level = 10.0_f32.powf(-2.0 * rnd());
            let sens = rnd();
            for _ in 0..(500 + (rnd() * 40_000.0) as usize) {
                used.process(level * (2.0 * rnd() - 1.0), level * (2.0 * rnd() - 1.0), sens);
            }
            used.reset();
            let mut fresh = Wah::new(rate);
            for i in 0..48_000 {
                let x = if (1_000..9_000).contains(&i) { 0.5 * (std::f32::consts::TAU * 220.0 * i as f32 / rate).sin() } else { 0.0 };
                let (a, b) = (used.process(x, 0.5 * x, 0.6), fresh.process(x, 0.5 * x, 0.6));
                assert!(a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits(), "trial {trial}: the reset wah differs from a new one {i} samples on");
            }
        }
    }
}
