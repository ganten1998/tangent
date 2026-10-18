//! The grand's action noises: the resonators the knock, the click and the
//! key's release ring in (`ActionNoise`, one per voice, and one the frame
//! keeps for the keys whose voices have finished, `grand::KeyNoise`), and
//! the sustain pedal's trapwork, heard through the case when the dampers
//! are thrown off the strings and when they land (`PedalNoise`).
//!
//! **Why the pedal's is not in a voice.** The knock and the key's release
//! are a key's own action (`grand::GrandVoice` rings them in its own
//! `ActionNoise`, so a key that is restruck knocks again on top of what is
//! ringing), but the pedal moves one rod for the whole instrument: one sound
//! per pedal movement, whatever is held. So it lives beside the frame's
//! shared body, one per instrument, and the frame rings it when CC 64
//! crosses the damper's lift point (`Keys::set_pedals`, MERGE-ME).
//!
//! **What it is.** A few low resonances of the trapwork and the case,
//! struck once per movement and left to ring out: the reference's (the
//! intimate preset is the only one with pedal noise, +3) measures -39.8 dB
//! re a C4 MIDI 80 attack when the pedal goes down and -41.1 dB when it
//! comes up (0.5 s rms), both peaking at 59 Hz (critic, uncovered 2). The
//! two microphones hear each resonance at its own level, so the thump is
//! not a point in the middle of the stereo field.
//!
//! Real-time: fixed size, no allocation; idle when every resonance has
//! fallen under 1e-9 of full scale (a compare a sample).

use super::modal::{BLOCK, C64};

/// How many resonances one `ActionNoise` rings at once: the knock's and
/// the click's sets with their rise twins, the treble's, and the key's
/// release, with room for a restrike's on top. A full set refuses a new
/// resonance (`kick` returns false) rather than allocate.
pub const ACTION_MODES: usize = 96;

/// Lanes of one group: the render runs eight resonators side by side.
const LANES: usize = 8;

/// One key's action noises as complex one-pole resonators of their own,
/// `z[n+1] = p z[n]`, `p = e^(-sigma / fs + i 2 pi f / fs)`, each heard in
/// the two microphones as `Re(w z)` for its own complex weight `w`, and
/// kicked by adding to `z` — which is how a second blow's knock lands on
/// what the first left ringing, in its own phase.
///
/// **Why not modes of the strings' bank.** They were, and they cost the
/// contact: while the felt touches, `modal::Bank::render_contact` runs every
/// mode of the bank through three passes a block, and the knock's twenty
/// or so went through all three though the hammer feels none of them. And a
/// knock kicked at the strike put the bank's state off zero, so a fresh
/// strike's first block took the contact's slow path (a free run beside the
/// impulse run) where a bank at rest needs only the impulse. Here a noise
/// costs a rotation and two taps a sample, and the contact sees strings.
///
/// `f32` throughout: these resonances live for well under a second above
/// 40 Hz, where `f32`'s rounding of the pole moves a frequency by
/// millionths and a decay by hundredths of a percent; the strings' bank
/// needs its difference form for 27 Hz and seventy-second decays, these do
/// not.
pub struct ActionNoise {
    rate: f32,
    count: usize,
    zr: [f32; ACTION_MODES],
    zi: [f32; ACTION_MODES],
    pr: [f32; ACTION_MODES],
    pi: [f32; ACTION_MODES],
    lr: [f32; ACTION_MODES],
    li: [f32; ACTION_MODES],
    rr: [f32; ACTION_MODES],
    ri: [f32; ACTION_MODES],
    /// The louder microphone's weight: `|z| loud` bounds every reading.
    loud: [f32; ACTION_MODES],
    tag: [u32; ACTION_MODES],
    /// What the last `cull` left: the root of the sum of the remaining
    /// resonances' loudest readings squared.
    level: f32,
}

impl ActionNoise {
    pub fn new(sample_rate: f32) -> Self {
        ActionNoise {
            rate: if sample_rate > 0.0 { sample_rate } else { 48_000.0 },
            count: 0,
            zr: [0.0; ACTION_MODES],
            zi: [0.0; ACTION_MODES],
            pr: [0.0; ACTION_MODES],
            pi: [0.0; ACTION_MODES],
            lr: [0.0; ACTION_MODES],
            li: [0.0; ACTION_MODES],
            rr: [0.0; ACTION_MODES],
            ri: [0.0; ACTION_MODES],
            loud: [0.0; ACTION_MODES],
            tag: [0; ACTION_MODES],
            level: 0.0,
        }
    }

    /// Every resonance gone.
    pub fn clear(&mut self) {
        // Zero what was in use, so a partly filled group computes zeros.
        let used = self.count.div_ceil(LANES) * LANES;
        for a in [&mut self.zr, &mut self.zi, &mut self.pr, &mut self.pi, &mut self.lr, &mut self.li, &mut self.rr, &mut self.ri, &mut self.loud] {
            a[..used].fill(0.0);
        }
        self.count = 0;
        self.level = 0.0;
    }

    /// Whether anything is ringing.
    pub fn active(&self) -> bool {
        self.count > 0
    }

    /// Kick resonance `tag` by `dz` (added to its coordinate): the one
    /// already ringing under that tag, or a new one at `f` Hz decaying at
    /// `sigma` nepers per second, heard as `Re(left z)` and `Re(right z)`,
    /// starting from `dz`. False, and nothing changed, when the set is full
    /// or the request cannot be a resonator (a frequency outside the band, a
    /// decay or a weight that is not finite).
    pub fn kick(&mut self, tag: u32, f: f64, sigma: f64, left: C64, right: C64, dz: C64) -> bool {
        if !(dz.re.is_finite() && dz.im.is_finite()) {
            return false;
        }
        if let Some(k) = self.tag[..self.count].iter().position(|&t| t == tag) {
            self.zr[k] += dz.re as f32;
            self.zi[k] += dz.im as f32;
            return true;
        }
        let fs = f64::from(self.rate);
        let finite = |z: C64| z.re.is_finite() && z.im.is_finite();
        if self.count >= ACTION_MODES || !(f > 0.0 && f < 0.49 * fs) || !(sigma >= 0.0 && sigma.is_finite()) || !finite(left) || !finite(right) {
            return false;
        }
        let k = self.count;
        let p = C64::polar((-sigma / fs).exp(), std::f64::consts::TAU * f / fs);
        self.zr[k] = dz.re as f32;
        self.zi[k] = dz.im as f32;
        self.pr[k] = p.re as f32;
        self.pi[k] = p.im as f32;
        self.lr[k] = left.re as f32;
        self.li[k] = left.im as f32;
        self.rr[k] = right.re as f32;
        self.ri[k] = right.im as f32;
        self.loud[k] = left.abs().max(right.abs()) as f32;
        self.tag[k] = tag;
        self.count = k + 1;
        true
    }

    /// Run every resonance for `l.len()` frames (at most `modal::BLOCK`) and
    /// ADD the two microphones into the slices: eight resonators at a time
    /// through the whole run, their states in registers, summed per lane and
    /// reduced once at the end, as the strings' bank does it.
    pub fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len()).min(BLOCK);
        if n == 0 || self.count == 0 {
            return;
        }
        let mut al = [[0.0_f32; LANES]; BLOCK];
        let mut ar = [[0.0_f32; LANES]; BLOCK];
        for g in 0..self.count.div_ceil(LANES) {
            let o = g * LANES;
            let lane = |a: &[f32; ACTION_MODES]| -> [f32; LANES] { std::array::from_fn(|j| a[o + j]) };
            let (mut zr, mut zi) = (lane(&self.zr), lane(&self.zi));
            let (pr, pi, lr, li, rr, ri) = (lane(&self.pr), lane(&self.pi), lane(&self.lr), lane(&self.li), lane(&self.rr), lane(&self.ri));
            for (sl, sr) in al.iter_mut().zip(ar.iter_mut()).take(n) {
                for j in 0..LANES {
                    let a = zr[j] * pr[j] - zi[j] * pi[j];
                    let b = zr[j] * pi[j] + zi[j] * pr[j];
                    zr[j] = a;
                    zi[j] = b;
                    sl[j] += lr[j] * a - li[j] * b;
                    sr[j] += rr[j] * a - ri[j] * b;
                }
            }
            // A resonance left alone walks down into the subnormals; 1e-20 is
            // 400 dB under full scale.
            for j in 0..LANES {
                if zr[j].abs() + zi[j].abs() < 1.0e-20 {
                    zr[j] = 0.0;
                    zi[j] = 0.0;
                }
            }
            self.zr[o..o + LANES].copy_from_slice(&zr);
            self.zi[o..o + LANES].copy_from_slice(&zi);
        }
        for ((x, y), (sl, sr)) in l.iter_mut().zip(r.iter_mut()).zip(al.iter().zip(&ar)).take(n) {
            *x += sl.iter().sum::<f32>();
            *y += sr.iter().sum::<f32>();
        }
    }

    /// Remove every resonance whose loudest reading is under `floor`, and
    /// measure what is left: returned, the sum of the remaining loudest
    /// readings (no output can exceed it); kept, their root sum of squares
    /// (`amplitude`). A removal is a swap with the last, every array at
    /// once, and the vacated slot zeroed.
    pub fn cull(&mut self, floor: f32) -> f32 {
        let (mut bound, mut total) = (0.0_f32, 0.0_f32);
        let mut k = self.count;
        while k > 0 {
            k -= 1;
            let a = (self.zr[k] * self.zr[k] + self.zi[k] * self.zi[k]).sqrt() * self.loud[k];
            if a >= floor {
                bound += a;
                total += a * a;
            } else {
                let last = self.count - 1;
                for arr in [&mut self.zr, &mut self.zi, &mut self.pr, &mut self.pi, &mut self.lr, &mut self.li, &mut self.rr, &mut self.ri, &mut self.loud] {
                    arr[k] = arr[last];
                    arr[last] = 0.0;
                }
                self.tag[k] = self.tag[last];
                self.tag[last] = 0;
                self.count = last;
            }
        }
        self.level = total.sqrt();
        bound
    }

    /// The root of the sum of the loudest readings squared, as the last
    /// `cull` measured it: the stealing policy's level, as the bank's.
    pub fn amplitude(&self) -> f32 {
        self.level
    }
}

/// The trapwork's resonances: Hz, T60 seconds, dB re the strongest, and the
/// level difference left over right, dB.
const MODES: [(f64, f64, f64, f64); 4] = [(59.0, 0.30, 0.0, 3.0), (47.0, 0.25, -6.0, -4.0), (88.0, 0.20, -6.0, 2.0), (131.0, 0.12, -12.0, -3.0)];

/// Under this a resonance is silent and the struct idles.
const QUIET: f64 = 1.0e-9;

pub struct PedalNoise {
    /// Per resonance: its complex state, its pole, its two taps.
    z: [(f64, f64); MODES.len()],
    pole: [(f64, f64); MODES.len()],
    tap: [(f64, f64); MODES.len()],
    awake: bool,
}

impl PedalNoise {
    pub fn new(sample_rate: f32) -> Self {
        let fs = if sample_rate > 0.0 { f64::from(sample_rate) } else { 48_000.0 };
        let pole = MODES.map(|(f, t60, _, _)| {
            let r = (-6.907_755 / (t60 * fs)).exp();
            let w = std::f64::consts::TAU * f / fs;
            (r * w.cos(), r * w.sin())
        });
        let tap = MODES.map(|(_, _, _, ild)| {
            let a = (10.0_f64.powf(-ild / 20.0)).atan();
            (a.cos() * std::f64::consts::SQRT_2, a.sin() * std::f64::consts::SQRT_2)
        });
        PedalNoise { z: [(0.0, 0.0); MODES.len()], pole, tap, awake: false }
    }

    /// The pedal moved past the dampers' lift point: ring the trapwork by
    /// `amp` output units (the strongest resonance's peak), added to what is
    /// still ringing. Each resonance starts at zero displacement moving, so
    /// the thump has no step in it.
    pub fn ring(&mut self, amp: f32) {
        let a = f64::from(amp);
        // A NaN is not finite, so this turns it away with the infinities.
        if !a.is_finite() || a <= 0.0 {
            return;
        }
        for (z, &(_, _, db, _)) in self.z.iter_mut().zip(&MODES) {
            // Re(z) is the output; a kick of -i a starts it as a sine.
            z.1 -= a * 10.0_f64.powf(db / 20.0);
        }
        self.awake = true;
    }

    pub fn active(&self) -> bool {
        self.awake
    }

    /// Everything off.
    pub fn clear(&mut self) {
        self.z = [(0.0, 0.0); MODES.len()];
        self.awake = false;
    }

    /// One sample, (left, right).
    pub fn tick(&mut self) -> (f32, f32) {
        if !self.awake {
            return (0.0, 0.0);
        }
        let (mut l, mut r, mut most) = (0.0_f64, 0.0_f64, 0.0_f64);
        for ((z, p), t) in self.z.iter_mut().zip(&self.pole).zip(&self.tap) {
            let (re, im) = (z.0 * p.0 - z.1 * p.1, z.0 * p.1 + z.1 * p.0);
            *z = (re, im);
            l += t.0 * re;
            r += t.1 * re;
            most = most.max(re * re + im * im);
        }
        if most < QUIET * QUIET {
            self.clear();
        }
        (l as f32, r as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// **One thump per movement, at 59 Hz, with no step, and it goes back
    /// to sleep.** The thump rung at a known amplitude peaks a few dB over
    /// it (four resonances), its strongest frequency is the trapwork's 59 Hz, its first sample
    /// is zero (a sine start, not a click), the two channels differ, and two
    /// seconds on the struct is idle and costs a compare.
    #[test]
    fn a_pedal_thump_rings_and_sleeps() {
        let mut p = PedalNoise::new(RATE);
        assert!(!p.active());
        p.ring(0.01);
        let n = (2.0 * RATE) as usize;
        let mut l = vec![0.0_f32; n];
        let mut r = vec![0.0_f32; n];
        for i in 0..n {
            let (a, b) = p.tick();
            l[i] = a;
            r[i] = b;
        }
        let pk = l.iter().chain(&r).fold(0.0_f32, |m, s| m.max(s.abs()));
        // A sine one sample in is 2 pi f / fs of its amplitude (under 1 %
        // at 59 Hz); a step would be the amplitude.
        assert!(l[0].abs() < 0.05 * pk, "the thump starts with a step: {}", l[0]);
        // The four resonances and the louder channel's tap (up to +3 dB) sum
        // to a peak about 5 dB over the strongest one's amplitude (measured
        // 0.0185 for 0.01).
        let over = 20.0 * (pk / 0.01).log10();
        assert!((0.0..7.0).contains(&over), "peak {pk} ({over:+.1} dB)");
        assert!(l.iter().zip(&r).any(|(a, b)| (a - b).abs() > 1.0e-4 * pk), "the two channels are one");
        // The strongest frequency, by a coarse Goertzel scan.
        let mag = |f: f64| {
            let w = std::f64::consts::TAU * f / f64::from(RATE);
            let (mut re, mut im) = (0.0, 0.0);
            for (i, s) in l.iter().enumerate().take((0.5 * RATE) as usize) {
                re += f64::from(*s) * (w * i as f64).cos();
                im += f64::from(*s) * (w * i as f64).sin();
            }
            re * re + im * im
        };
        let best = (30..200).map(|f| (f, mag(f64::from(f)))).fold((0, 0.0), |b, x| if x.1 > b.1 { x } else { b });
        assert!((55..=64).contains(&best.0), "peak at {} Hz", best.0);
        assert!(!p.active(), "still awake after two seconds");
    }
    /// **A cleared trapwork is a new one.** After throws rung at random
    /// amplitudes and times, `clear` leaves nothing ringing: the same throws
    /// from there tick exactly what a new one ticks, to the bit. The same of
    /// the key release's resonators (`ActionNoise::clear`), kicked at random
    /// and culled: a new set fed the same kicks renders the same.
    #[test]
    fn a_cleared_noise_is_a_new_one() {
        let mut seed = 0x7a3c_91e5_u32;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32
        };
        for trial in 0..12 {
            let mut used = PedalNoise::new(RATE);
            for _ in 0..(1 + (rnd() * 6.0) as usize) {
                used.ring(0.02 * rnd());
                for _ in 0..(rnd() * 20_000.0) as usize {
                    used.tick();
                }
            }
            used.clear();
            let mut fresh = PedalNoise::new(RATE);
            for i in 0..30_000 {
                if i == 500 || i == 9_000 {
                    used.ring(0.01);
                    fresh.ring(0.01);
                }
                let (a, b) = (used.tick(), fresh.tick());
                assert!(a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits() && used.active() == fresh.active(), "trial {trial}: the cleared trapwork differs from a new one at {i}");
            }
            let mut used = ActionNoise::new(RATE);
            let kick = |a: &mut ActionNoise, tag: u32, x: f32| {
                let f = 40.0 + f64::from(x) * 3000.0;
                a.kick(tag, f, 20.0 + f64::from(x) * 200.0, C64::new(0.3, 0.1), C64::new(0.2, -0.1), C64::new(0.0, -1e-3 * f64::from(x)));
            };
            for _ in 0..(1 + (rnd() * 40.0) as usize) {
                kick(&mut used, (rnd() * 30.0) as u32, rnd());
                let (mut l, mut r) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK]);
                for _ in 0..(rnd() * 8.0) as usize {
                    used.render(&mut l, &mut r);
                }
                used.cull(1.0e-7 * rnd());
            }
            used.clear();
            let mut fresh = ActionNoise::new(RATE);
            for b in 0..40 {
                if b % 7 == 0 {
                    kick(&mut used, b, 0.5);
                    kick(&mut fresh, b, 0.5);
                }
                let (mut l, mut r, mut fl, mut fr) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK], [0.0_f32; BLOCK], [0.0_f32; BLOCK]);
                used.render(&mut l, &mut r);
                fresh.render(&mut fl, &mut fr);
                used.cull(1.0e-9);
                fresh.cull(1.0e-9);
                for i in 0..BLOCK {
                    assert!(l[i].to_bits() == fl[i].to_bits() && r[i].to_bits() == fr[i].to_bits(), "trial {trial}: the cleared action noise differs from a new one at block {b}, sample {i}");
                }
            }
        }
    }
}
