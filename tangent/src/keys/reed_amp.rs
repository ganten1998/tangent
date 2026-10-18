//! The reed piano's amplifier: the dry patch's tremolo, and the wet patch's
//! whole chain — slap, cabinet, throb, room.
//!
//! **Why an amp at all, on a physical model.** The reed piano never had an
//! acoustic voice: what anyone has heard of it is its amplifier, and the wet
//! patch is "that instrument through that amp". The chain is built from
//! ordinary parts — a delay, a peaking filter, an LFO-driven gain, a small
//! feedback delay network — each voiced to a number measured on the
//! reference's own amp preset (W2 Tremo) rendered with one stage at a time
//! on the study's fx score (a C4–E4–G4 chord, C4 at five velocities, a held
//! C3), the instrument's own voices fed through this amp the same way:
//!
//! | stage   | the reference                      | this amp                 |
//! |---------|------------------------------------|--------------------------|
//! | slap    | 45.0 ms, one pole at 800 Hz, gain  | 45.0 ms, 800 Hz,         |
//! |         | +0.127 left, −0.127 right          | ±0.127                   |
//! | cabinet | +3.0 dB at 504 Hz, +1.5 at 200 and | +3.0, +1.5 and +1.4,     |
//! |         | 1270, +0.5 at 100 and 2540         | +0.5 and +0.5            |
//! | throb   | 4.005 Hz, 3.04 dB p-p, mean −1.47  | 4.005 Hz, 3.01, −1.46;   |
//! |         | dB; gain curve H2..H5 −65/−14/−55/ | −63/−14/−58/−34 (in dB   |
//! |         | −32 dB re H1 (in dB −30/−14/−34/   | −30/−15/−35/−35); the    |
//! |         | −33); L/R gain correlation 0.99995 | two channels one gain    |
//! | room    | −24.4 / −28.3 / −28.0 / −26.9 dB   | −24.5 / −28.1 / −27.1 /  |
//! |         | re dry (chord, C4 v80, C4 v127,    | −27.1; 1.86 / 2.09 /     |
//! |         | held C3; L/R power average); RT60  | 2.22 s; 0.0 dB; +0.64 to |
//! |         | 2.02 / 1.84 / 1.94 s after its key-| +0.97                    |
//! |         | offs; balance −1.9..−0.6 dB; L/R   |                          |
//! |         | correlation +0.56..+0.98 < 300 Hz  |                          |
//!
//! The reference's room is measured as its reverb render less 0.95 of its
//! bare one: that render carries its dry path at 0.950 (the first
//! millisecond of every onset says so). Two differences are measured and
//! left: its early reflections are eight taps where the reference's room is
//! dense from 2 ms (its first 80 ms of a chord stand 1.0 dB under the next
//! 320 on the mid, this room's 5.3), and above 1 kHz a held passage's few
//! partials decide how alike the two channels are (+0.40 / +0.59 here on
//! the chord and the held C3, −0.14 / −0.16 there; white noise through
//! this room reads +0.16..+0.26, `the_room_is_one_room_in_both_channels`).
//!
//! None of those is a knob except the throb's depth (knob 5, "Tremolo"),
//! whose default lands on the measured 3.0 dB and whose zero stops the
//! throb and leaves the rest of the amp — which is what makes the patch
//! wet. The dry patch's tremolo is the one `mod.rs` has always had, moved
//! here unchanged in sound (`ReedAmp::dry`).
//!
//! **Real-time contract.** Every buffer is sized in [`ReedAmp::new`] for
//! the engine's rate; `dry`/`wet` are per-sample arithmetic, no allocation,
//! no locks. Denormals: the room's lines and the filters are flushed below
//! 1e-20 once per block by [`ReedAmp::end_block`].

/// The dry patch's throb, as `mod.rs` always ran it: 5.5 Hz, a level dip of
/// up to `0.6 · depth` (2.4 dB p-p at the old default depth 0.4).
const DRY_HZ: f32 = 5.5;
const DRY_SCALE: f32 = 0.30;

/// The wet throb's rate, Hz (reference 4.005 Hz).
const WET_HZ: f32 = 4.0;
/// The wet throb's depth per unit of the knob, dB peak-to-peak: the knob's
/// default 0.5 is the reference's 3.0 dB (measured 3.04).
pub const WET_DB_PER_KNOB: f32 = 6.0;
/// The throb's shape: its gain curve's third and fifth harmonics re its
/// fundamental, the reference's −14.2 and −32.5 dB (the ratio of the
/// envelopes of W2 Tremo rendered with and without its tremolo), built as
/// that Fourier series —
/// which is the reference's flat-topped trapezoid to 0.005 rms over the
/// twenty points of a period, where a sine through `tanh` with the same
/// third harmonic carries its fifth at −25.8.
const THROB_H3: f32 = 0.195;
const THROB_H5: f32 = 0.023_7;

/// The slap: delay, its level, its one-pole tone, Hz.
const SLAP_S: f32 = 0.045;
const SLAP_GAIN: f32 = 0.127;
const SLAP_HZ: f32 = 800.0;

/// The cabinet's bell: centre, gain, Q (RBJ peaking).
const CAB_HZ: f32 = 500.0;
const CAB_DB: f32 = 3.0;
const CAB_Q: f32 = 0.5;

/// The room's eight delay lines, s: mutually prime lengths from 37 to 83 ms,
/// a room a few metres across.
const ROOM_LINES_S: [f32; 8] = [0.037_3, 0.043_9, 0.049_4, 0.056_0, 0.061_5, 0.069_0, 0.075_8, 0.083_4];
/// The room's decay: RT60 at low frequency, s, and the corner of the
/// one-pole each line is damped through, Hz. Measured on the fx score
/// against the reference's room (its reverb render less 0.95 of its bare
/// one), by octave from 125 Hz to 4 kHz: the room's level re the dry chord
/// −27.0 / −25.8 / −26.9 / −24.8 / −22.1 / −23.3 dB against −26.8 / −25.4 /
/// −24.0 / −25.0 / −23.0 / −21.7 (mid), and its RT60 after the last
/// key-off 2.96 / 2.32 / 2.19 / 1.06 / 0.69 / 0.42 s against 2.42 / 2.33 /
/// 2.31 / 1.59 / 0.89 / 0.89. A 1.2 kHz corner (fitted first, against the
/// reference's room uncorrected for its dry share) left 4 kHz 4.2 dB down
/// and 1–2 kHz ringing 31–40 % short; 2 kHz put 1–2 kHz 2.2–3.5 dB over.
const ROOM_RT_LOW: f32 = 2.2;
const ROOM_DAMP_HZ: f32 = 1_600.0;
/// The room's input gain: sets its level re the dry signal. Measured on the
/// fx score as the L/R power average of the room alone re the dry (the
/// reference's as its reverb render less 0.95 of its bare one): −24.4 /
/// −28.3 / −28.0 / −26.9 dB over its chord, C4 v80, C4 v127 and held C3
/// against the reference's −24.4 / −28.3 / −28.0 / −26.9 at 0.053 before
/// the room's one output fed both channels; at this send −24.5 / −28.1 /
/// −27.1 / −27.1 (see the module table).
const ROOM_SEND: f32 = 0.035;
/// The room's input is high-passed here, Hz: an undamped low end in a
/// feedback network banks bass the reference's room does not have (its
/// level re dry is flat, −22.5 dB at 125 Hz and −22.6 at 500).
const ROOM_HP_HZ: f32 = 60.0;
/// The two channels' all-passes, s (left, right; three Schroeder sections
/// each), and their gain: the room's one output through each, so the two
/// channels are equally loud at every frequency and part only in phase.
/// Lengths of a millisecond or two keep them in phase under ~300 Hz and
/// apart above (simulated on white noise, the channels correlate +0.95 at
/// 125–250 Hz, +0.28 at 250–500, ±0.05 from 500 Hz to 2 kHz).
const ROOM_DECORR_S: [[f32; 3]; 2] = [[0.000_9, 0.001_7, 0.002_9], [0.001_3, 0.002_1, 0.002_5]];
const ROOM_DECORR_G: f32 = 0.6;
/// Early reflections: taps on the room's input, s, alternating channels,
/// and their gain: the reference's room has energy 2 ms after an onset
/// and is 1.5 dB louder in its first 80 ms than over the next 320.
const ER_TAPS_S: [f32; 8] = [0.002_1, 0.004_4, 0.007_7, 0.010_3, 0.013_3, 0.017_1, 0.021_9, 0.027_2];
const ER_GAIN: f32 = 0.5;
/// The wet chain's output trim: the cabinet's bell lifts a fortissimo note's
/// peak by up to 3 dB, and the patch must leave the same headroom as the
/// dry one (single notes ≤ −3 dBFS at v127).
const WET_TRIM: f32 = 0.8;
/// The throbs' knob is followed through a one-pole of this time constant,
/// s: the frame hands knob 5 over once per block, and an amp's knob acts on
/// whatever is ringing — so a jump from 0 to 1 at the throb's trough stepped
/// the wet gain from 1.0 to 0.50 in one sample (the dry one's from 1.0 to
/// 0.40). Through 20 ms the largest one-sample move of either gain is under
/// a thousandth (`the_throb_knob_moves_without_a_step`); a steady knob is
/// exactly the knob, sample for sample.
pub(super) const KNOB_SMOOTH_S: f32 = 0.02;

/// The reed piano's amp, one per instrument.
pub struct ReedAmp {
    rate: f32,
    dry_phase: f32,
    wet_phase: f32,
    slap: [Vec<f32>; 2],
    slap_at: usize,
    slap_n: usize,
    slap_lp: [f32; 2],
    slap_a: f32,
    cab: [f32; 5],
    cab_z: [[f32; 2]; 2],
    lines: [Vec<f32>; 8],
    line_at: [usize; 8],
    line_g: [f32; 8],
    line_a: [f32; 8],
    line_z: [f32; 8],
    er: Vec<f32>,
    er_at: usize,
    er_taps: [usize; 8],
    room_hp: [f32; 3],
    /// Each channel's three all-passes ([`ROOM_DECORR_S`]): their delay
    /// lines and write heads.
    decorr: [[Vec<f32>; 3]; 2],
    decorr_at: [[usize; 3]; 2],
    /// Knob 5 as the throbs use it, smoothed (see [`KNOB_SMOOTH_S`]), and
    /// whether it has been set since the last reset (the first value is
    /// taken as it is, so a steady knob is exactly the knob).
    wet_knob: f32,
    dry_knob: f32,
    knob_a: f32,
    knob_set: [bool; 2],
    /// The room's energy, for `ringing`.
    energy: f32,
    /// Which stages run: slap, cabinet, throb, room. All of them, always,
    /// in the instrument; see `set_stages`.
    stages: [bool; 4],
    /// The throb shape's polynomial in `sin θ` (`sin 3θ` and `sin 5θ`
    /// expanded), scaled so its peak is 1.
    throb: [f32; 3],
}

impl ReedAmp {
    /// Build the amp for `rate`. **Allocates**: called from `Keys::new`.
    pub fn new(rate: f32) -> Self {
        let rate = if rate > 0.0 { rate } else { 48_000.0 };
        let slap_n = ((SLAP_S * rate).round() as usize).max(1);
        let lines = ROOM_LINES_S.map(|s| vec![0.0_f32; ((s * rate) as usize).max(8)]);
        let mut line_g = [0.0; 8];
        let mut line_a = [0.0; 8];
        let pole = (-std::f32::consts::TAU * ROOM_DAMP_HZ / rate).exp();
        for (i, l) in lines.iter().enumerate() {
            // An absorbent line: a one-pole low-pass whose DC gain makes the
            // low RT60 for this length; above its corner the loss grows.
            let n = l.len() as f32;
            let g_dc = 10.0_f32.powf(-3.0 * n / (ROOM_RT_LOW * rate));
            line_a[i] = pole;
            line_g[i] = g_dc * (1.0 - pole);
        }
        let er_n = ((ER_TAPS_S[7] * rate) as usize + 2).max(8);
        // sin θ + h3 sin 3θ + h5 sin 5θ, as a polynomial in s = sin θ, and
        // its peak over a period.
        let poly = [
            1.0 + 3.0 * THROB_H3 + 5.0 * THROB_H5,
            -4.0 * THROB_H3 - 20.0 * THROB_H5,
            16.0 * THROB_H5,
        ];
        let mut peak = 0.0_f32;
        for j in 0..1024 {
            let x = (std::f32::consts::TAU * j as f32 / 1024.0).sin();
            let x2 = x * x;
            peak = peak.max((x * (poly[0] + x2 * (poly[1] + x2 * poly[2]))).abs());
        }
        let throb = poly.map(|c| c / peak);
        ReedAmp {
            rate,
            dry_phase: 0.0,
            wet_phase: 0.0,
            slap: [vec![0.0; slap_n], vec![0.0; slap_n]],
            slap_at: 0,
            slap_n,
            slap_lp: [0.0; 2],
            slap_a: (-std::f32::consts::TAU * SLAP_HZ / rate).exp(),
            cab: peaking(CAB_HZ, CAB_DB, CAB_Q, rate),
            cab_z: [[0.0; 2]; 2],
            lines,
            line_at: [0; 8],
            line_g,
            line_a,
            line_z: [0.0; 8],
            er: vec![0.0; er_n],
            er_at: 0,
            er_taps: ER_TAPS_S.map(|s| ((s * rate) as usize).clamp(1, er_n - 1)),
            room_hp: [(-std::f32::consts::TAU * ROOM_HP_HZ / rate).exp(), 0.0, 0.0],
            decorr: ROOM_DECORR_S.map(|ch| ch.map(|s| vec![0.0_f32; ((s * rate) as usize).max(1)])),
            decorr_at: [[0; 3]; 2],
            wet_knob: 0.0,
            dry_knob: 0.0,
            knob_a: 1.0 - (-1.0 / (KNOB_SMOOTH_S * rate)).exp(),
            knob_set: [false; 2],
            energy: 0.0,
            stages: [true; 4],
            throb,
        }
    }

    /// Run only some of the wet chain's stages. The instrument never calls
    /// this; the lane's bench does, to measure each stage against the
    /// reference's preset rendered with that one stage switched on — which
    /// is how every constant at the top of this file was fitted.
    #[allow(dead_code)]
    pub fn set_stages(&mut self, slap: bool, cabinet: bool, throb: bool, room: bool) {
        self.stages = [slap, cabinet, throb, room];
    }

    /// Everything to rest: a patch change, and the frame falling silent
    /// (`Keys::render`).
    pub fn reset(&mut self) {
        self.dry_phase = 0.0;
        self.wet_phase = 0.0;
        for b in &mut self.slap {
            b.fill(0.0);
        }
        self.slap_at = 0;
        self.slap_lp = [0.0; 2];
        self.cab_z = [[0.0; 2]; 2];
        for l in &mut self.lines {
            l.fill(0.0);
        }
        self.line_at = [0; 8];
        self.line_z = [0.0; 8];
        self.er.fill(0.0);
        self.er_at = 0;
        self.room_hp[1] = 0.0;
        self.room_hp[2] = 0.0;
        for ch in &mut self.decorr {
            for d in ch.iter_mut() {
                d.fill(0.0);
            }
        }
        self.decorr_at = [[0; 3]; 2];
        self.knob_set = [false; 2];
        self.energy = 0.0;
    }

    /// Knob 5 through its smoother: the first value after a reset as it
    /// is, then a one-pole at [`KNOB_SMOOTH_S`] toward each new one, landing
    /// on it exactly within 1e-4 of it (0.000 6 dB of throb; an f32 one-pole
    /// this slow stalls 3e-5 short of 1, and toward 0 it would creep into
    /// the denormals). The suitcase's tremolo, which lives in the frame,
    /// smooths its depth knob through this too.
    #[inline]
    pub(super) fn smooth(state: &mut f32, set: &mut bool, a: f32, knob: f32) -> f32 {
        if *set && (knob - *state).abs() > 1e-4 {
            *state += a * (knob - *state);
        } else {
            *state = knob;
            *set = true;
        }
        *state
    }

    /// Whether the room is still sounding after the notes have stopped: the
    /// frame keeps rendering while it is.
    pub fn ringing(&self) -> bool {
        self.energy > 1e-10
    }

    /// The largest magnitude any of the amp's lines and filters holds: what
    /// is left in an amp the frame has stopped rendering.
    #[cfg(test)]
    pub(super) fn residual(&self) -> f32 {
        let lines = self.slap.iter().chain(&self.lines).chain(std::iter::once(&self.er)).chain(self.decorr.iter().flatten());
        let filters = self.slap_lp.iter().chain(self.cab_z.iter().flatten()).chain(&self.line_z).chain(&self.room_hp[1..]);
        lines.flat_map(|l| l.iter()).chain(filters).fold(0.0_f32, |m, v| m.max(v.abs()))
    }

    /// The dry patch's throb, unchanged from `mod.rs`: `depth` is knob 5.
    #[inline]
    pub fn dry(&mut self, l: f32, r: f32, depth: f32) -> (f32, f32) {
        let depth = Self::smooth(&mut self.dry_knob, &mut self.knob_set[1], self.knob_a, depth.clamp(0.0, 1.0));
        self.dry_phase = (self.dry_phase + std::f32::consts::TAU * DRY_HZ / self.rate) % std::f32::consts::TAU;
        let g = 1.0 - depth * DRY_SCALE * (1.0 + self.dry_phase.sin());
        (l * g, r * g)
    }

    /// The wet chain, one stereo sample: `knob` is knob 5 (the throb's
    /// depth, 0..1, 0.5 = the reference's 3 dB).
    #[inline]
    pub fn wet(&mut self, l: f32, r: f32, knob: f32) -> (f32, f32) {
        // Slap: each channel's own echo, low-passed, opposite signs — wide
        // in stereo, nothing in mono, as the reference's.
        let i = self.slap_at;
        let (el, er) = (self.slap[0][i], self.slap[1][i]);
        self.slap[0][i] = l;
        self.slap[1][i] = r;
        self.slap_at = if i + 1 == self.slap_n { 0 } else { i + 1 };
        let a = self.slap_a;
        self.slap_lp[0] = (1.0 - a) * el + a * self.slap_lp[0];
        self.slap_lp[1] = (1.0 - a) * er + a * self.slap_lp[1];
        let [st_slap, st_cab, st_throb, st_room] = self.stages;
        let sg = if st_slap { SLAP_GAIN } else { 0.0 };
        let mut x = [l + sg * self.slap_lp[0], r - sg * self.slap_lp[1]];
        // Cabinet.
        if st_cab {
            let c = self.cab;
            for (ch, v) in x.iter_mut().enumerate() {
                let z = &mut self.cab_z[ch];
                let y = c[0] * *v + z[0];
                z[0] = c[1] * *v - c[3] * y + z[1];
                z[1] = c[2] * *v - c[4] * y;
                *v = y;
            }
        }
        // Throb: one gain on both channels, a soft-clipped sine in dB.
        self.wet_phase = (self.wet_phase + std::f32::consts::TAU * WET_HZ / self.rate) % std::f32::consts::TAU;
        let knob = Self::smooth(&mut self.wet_knob, &mut self.knob_set[0], self.knob_a, knob.clamp(0.0, 1.0));
        let depth_db = if st_throb { WET_DB_PER_KNOB * knob } else { 0.0 };
        // Symmetric in LINEAR gain, between unity and the trough, as the
        // reference's is: its gain curve has no even harmonics (H2 −65 dB
        // re the fundamental, H4 −55); a curve symmetric in dB has them at
        // −31/−39.
        let sn = self.wet_phase.sin();
        let s2 = sn * sn;
        let [c1, c3, c5] = self.throb;
        let shape = (sn * (c1 + s2 * (c3 + s2 * c5))).clamp(-1.0, 1.0);
        let trough = (-depth_db * (std::f32::consts::LN_10 / 20.0)).exp();
        let g = 1.0 - (1.0 - trough) * 0.5 * (1.0 - shape);
        let (xl, xr) = (x[0] * g, x[1] * g);
        // Room: early reflections on the input, then eight absorbent lines
        // under a Hadamard mix; the two outputs are orthogonal sums of them.
        let mono = 0.5 * (xl + xr);
        let [ha, hx, hy] = self.room_hp;
        let hp = ha * (hy + mono - hx);
        self.room_hp = [ha, mono, hp];
        let input = if st_room { hp * ROOM_SEND } else { 0.0 };
        let ea = self.er_at;
        self.er[ea] = input;
        let n_er = self.er.len();
        let tap = |k: usize| self.er[(ea + n_er - self.er_taps[k]) % n_er];
        // Same sign within a channel (taps a few milliseconds apart with
        // opposite signs comb the low end out), left and right from
        // different delays.
        let er_l = ER_GAIN * (tap(0) + 0.85 * tap(2) + 0.7 * tap(4) + 0.6 * tap(6));
        let er_r = ER_GAIN * (tap(1) + 0.85 * tap(3) + 0.7 * tap(5) + 0.6 * tap(7));
        self.er_at = if ea + 1 == n_er { 0 } else { ea + 1 };
        let mut o = [0.0_f32; 8];
        for (k, line) in self.lines.iter().enumerate() {
            o[k] = line[self.line_at[k]];
        }
        // Fast Walsh-Hadamard, scaled to stay orthonormal.
        let mut h = o;
        let mut len = 1;
        while len < 8 {
            let mut s = 0;
            while s < 8 {
                for j in s..s + len {
                    let (u, v) = (h[j], h[j + len]);
                    h[j] = u + v;
                    h[j + len] = u - v;
                }
                s += 2 * len;
            }
            len *= 2;
        }
        let norm = 1.0 / 8.0_f32.sqrt();
        let mut e = 0.0;
        for k in 0..8 {
            let fb = h[k] * norm + input;
            // Flushed here rather than per block: what a line holds goes
            // round the network again, so a value let into a buffer under
            // 1e-20 decays through the denormals on its next passes.
            let z = self.line_g[k] * fb + self.line_a[k] * self.line_z[k];
            self.line_z[k] = if z.abs() < 1e-20 { 0.0 } else { z };
            let at = self.line_at[k];
            self.lines[k][at] = self.line_z[k];
            self.line_at[k] = if at + 1 == self.lines[k].len() { 0 } else { at + 1 };
            e += o[k] * o[k];
        }
        self.energy = e;
        // **One room, heard from two places.** The network has one output —
        // every line and every early tap, one sign — and each channel hears
        // it through its own three all-passes ([`ROOM_DECORR_S`]): equally
        // loud at every frequency, in phase at the bottom and apart at the
        // top, as the reference's room is (measured as its reverb render less
        // 0.95 of its bare one, the share of dry that render carries: L/R
        // correlation +0.98 below 300 Hz on the fx score's held C3, +0.60 at
        // 300 Hz–1 kHz, −0.16 above; its balance −1.9..−0.6 dB over the four
        // passages). Each channel summing four lines of its own, as this
        // room first ran, left the held C3's room anti-phase (−0.47; −0.61
        // below 300 Hz) and swung the balance −4.9..+6.0 dB; a low end made
        // common under four lines a side still swung it −0.4..+4.2
        // (`the_room_is_one_room_in_both_channels` holds what this one does).
        let m = (0.25 * (o[0] + o[1] + o[2] + o[3] + o[4] + o[5] + o[6] + o[7]) + 0.5 * (er_l + er_r)) * std::f32::consts::SQRT_2;
        let mut wet = [m; 2];
        for (ch, v) in wet.iter_mut().enumerate() {
            for st in 0..3 {
                let buf = &mut self.decorr[ch][st];
                let at = self.decorr_at[ch][st];
                let delayed = buf[at];
                let y = -ROOM_DECORR_G * *v + delayed;
                let w = *v + ROOM_DECORR_G * y;
                buf[at] = if w.abs() < 1e-20 { 0.0 } else { w };
                self.decorr_at[ch][st] = if at + 1 == buf.len() { 0 } else { at + 1 };
                *v = y;
            }
        }
        ((xl + wet[0]) * WET_TRIM, (xr + wet[1]) * WET_TRIM)
    }

    /// Once per block: flush what has decayed to nothing, so no state of the
    /// amp ever walks into the denormals.
    pub fn end_block(&mut self) {
        if self.room_hp[2].abs() < 1e-20 {
            self.room_hp[2] = 0.0;
        }
        for z in self.slap_lp.iter_mut() {
            if z.abs() < 1e-20 {
                *z = 0.0;
            }
        }
        for zz in &mut self.cab_z {
            for z in zz.iter_mut() {
                if z.abs() < 1e-20 {
                    *z = 0.0;
                }
            }
        }
    }
}

/// RBJ peaking biquad: `[b0, b1, b2, a1, a2]`, a0 = 1.
fn peaking(f: f32, db: f32, q: f32, rate: f32) -> [f32; 5] {
    let a = 10.0_f32.powf(db / 40.0);
    let w = std::f32::consts::TAU * f / rate;
    // The sine apart from its cosine below (`super::sine_apart`), so both
    // profiles give the filter the same bits.
    let alpha = super::sine_apart(w) / (2.0 * q);
    let a0 = 1.0 + alpha / a;
    [
        (1.0 + alpha * a) / a0,
        -2.0 * w.cos() / a0,
        (1.0 - alpha * a) / a0,
        -2.0 * w.cos() / a0,
        (1.0 - alpha / a) / a0,
    ]
}

#[cfg(test)]
mod tests {
    //! The amp's every constant, held to what the lane's bench measured on
    //! it and on the reference: the Amp patch's voices through each stage
    //! alone on the study's fx score, against W2 Tremo rendered with one
    //! stage at a time (the module table).
    use super::super::reed::BLOCK;
    use super::super::Patch;
    use super::*;

    const RATE: f32 = 48_000.0;

    fn db(v: f64) -> f64 {
        20.0 * v.max(1e-15).log10()
    }

    /// One stage alone, fed `x` on both channels, the knob at `knob`.
    fn through(stages: [bool; 4], knob: f32, x: &[(f32, f32)]) -> Vec<(f32, f32)> {
        let mut a = ReedAmp::new(RATE);
        a.set_stages(stages[0], stages[1], stages[2], stages[3]);
        let mut out = Vec::with_capacity(x.len());
        for (i, &(l, r)) in x.iter().enumerate() {
            out.push(a.wet(l, r, knob));
            if i % BLOCK == BLOCK - 1 {
                a.end_block();
            }
        }
        out
    }

    /// **The throb: 4 Hz, 3 dB, one gain on both channels, flat-topped.**
    /// The gain curve itself (a steady 1.0 through the throb alone): rate
    /// 4.00 ± 0.05 Hz, 3.0 ± 0.1 dB peak to peak at the knob's default
    /// (the reference's 3.04), its mean −1.46 dB (−1.47), its odd
    /// harmonics within a decibel of the reference's (H3 −14.2, H5 −32.5 dB
    /// re H1, `THROB_H3`/`THROB_H5`) and no even ones to speak of (the
    /// reference's −65 / −55: the curve is symmetric in linear gain), L and
    /// R the same sample. The brief's trapezoid harmonics, −30 / −14 / −35 /
    /// −32 dB, are the same curve's in DECIBELS — the reference's gain curve
    /// in dB reads −30.0 / −14.2 / −34.2 / −33.0 on its W2 Tremo renders with
    /// and without the tremolo, this one −29.9 / −14.5 / −34.6 / −34.7 on the
    /// same score — and are held to the acceptance's ±6 dB here.
    #[test]
    fn the_throb_is_four_hertz_three_db_one_gain() {
        let n = (5.0 * RATE) as usize;
        let ones = vec![(1.0_f32, 1.0_f32); n];
        let out = through([false, false, true, false], 0.5, &ones);
        assert!(out.iter().all(|(l, r)| l == r), "the throb is one gain on both channels");
        let g: Vec<f64> = out.iter().map(|(l, _)| f64::from(*l / WET_TRIM)).collect();
        let (mx, mn) = g.iter().fold((f64::MIN, f64::MAX), |(a, b), v| (a.max(*v), b.min(*v)));
        let pp = db(mx) - db(mn);
        let mean = g.iter().map(|v| db(*v)).sum::<f64>() / n as f64;
        // Rate from the upward crossings of the curve's mean.
        let m = g.iter().sum::<f64>() / n as f64;
        let ups: Vec<usize> = (1..n).filter(|&i| g[i - 1] < m && g[i] >= m).collect();
        let rate = (ups.len() - 1) as f64 * f64::from(RATE) / (ups[ups.len() - 1] - ups[0]) as f64;
        // Harmonics over the twenty whole periods.
        let harm = |k: f64| {
            let w = std::f64::consts::TAU * f64::from(WET_HZ) * k / f64::from(RATE);
            let (mut re, mut im) = (0.0, 0.0);
            for (i, v) in g.iter().enumerate() {
                re += (v - m) * (w * i as f64).cos();
                im += (v - m) * (w * i as f64).sin();
            }
            re.hypot(im)
        };
        let h1 = harm(1.0);
        let h: Vec<f64> = (2..=5).map(|k| db(harm(f64::from(k)) / h1)).collect();
        // The same harmonics of the curve in decibels.
        let gd: Vec<f64> = g.iter().map(|v| db(*v)).collect();
        let md = gd.iter().sum::<f64>() / n as f64;
        let harm_db = |k: f64| {
            let w = std::f64::consts::TAU * f64::from(WET_HZ) * k / f64::from(RATE);
            let (mut re, mut im) = (0.0, 0.0);
            for (i, v) in gd.iter().enumerate() {
                re += (v - md) * (w * i as f64).cos();
                im += (v - md) * (w * i as f64).sin();
            }
            re.hypot(im)
        };
        let hd1 = harm_db(1.0);
        let hd: Vec<f64> = (2..=5).map(|k| db(harm_db(f64::from(k)) / hd1)).collect();
        println!("  throb: {rate:.3} Hz, {pp:.2} dB p-p, mean {mean:.2} dB, H2..H5 {h:.1?} dB re H1, in dB {hd:.1?}");
        for (got, want) in hd.iter().zip([-30.0, -14.0, -35.0, -32.0]) {
            assert!((got - want).abs() <= 6.0, "the throb's dB curve has harmonics {hd:.1?}, the brief's trapezoid -30/-14/-35/-32");
        }
        assert!((rate - 4.0).abs() <= 0.05, "the throb runs at {rate:.3} Hz");
        assert!((pp - 3.0).abs() <= 0.1, "the throb is {pp:.2} dB peak to peak at the default");
        assert!((mean - -1.47).abs() <= 0.2, "the throb's mean is {mean:.2} dB");
        assert!((h[1] - -14.2).abs() <= 1.0 && (h[3] - -32.5).abs() <= 1.0, "the throb's odd harmonics {h:.1?}");
        assert!(h[0] < -40.0 && h[2] < -40.0, "the throb's curve has even harmonics {h:.1?}");
    }

    /// **The knob's zero stops the throb and leaves the amp.** Through the
    /// throb alone the signal comes out at exactly the trim; through the
    /// whole chain an impulse still has its slap and its room — which is
    /// what makes the patch wet. And the knob's default is the reference's
    /// depth exactly: 0.5 × `WET_DB_PER_KNOB` = 3.0 dB.
    #[test]
    fn the_knobs_zero_stops_the_throb_and_keeps_the_amp() {
        let n = RATE as usize;
        let ones = vec![(1.0_f32, 1.0_f32); n];
        let out = through([false, false, true, false], 0.0, &ones);
        assert!(out.iter().all(|(l, r)| *l == WET_TRIM && *r == WET_TRIM), "at zero the throb still moves the level");
        let mut imp = vec![(0.0_f32, 0.0_f32); 2 * n];
        imp[0] = (1.0, 1.0);
        let out = through([true, true, true, true], 0.0, &imp);
        let tail: f64 = out[(0.1 * RATE) as usize..].iter().map(|(l, r)| f64::from(l * l + r * r)).sum();
        assert!(tail > 1e-4, "at zero the room is gone: tail energy {tail:e}");
        assert_eq!(Patch::Wurli140.defaults()[5] * WET_DB_PER_KNOB, 3.0, "the default depth");
    }

    /// **The slap: 45 ms, an 800 Hz pole, ±0.127, opposite in the two
    /// channels** — the reference's to the sample and the third decimal
    /// (least squares of its echo on W2 Tremo with only its delay on: left
    /// +0.1268, right −0.1270). Nothing of it in mono.
    #[test]
    fn the_slap_is_45_ms_dark_and_opposite() {
        let n = (0.2 * RATE) as usize;
        for (ch, sign) in [(0usize, 1.0_f64), (1, -1.0)] {
            let mut imp = vec![(0.0_f32, 0.0_f32); n];
            if ch == 0 {
                imp[0].0 = 1.0;
            } else {
                imp[0].1 = 1.0;
            }
            let out = through([true, false, false, false], 0.5, &imp);
            let y: Vec<f64> = out.iter().map(|(l, r)| f64::from(if ch == 0 { *l } else { *r }) / f64::from(WET_TRIM)).collect();
            let lag = (SLAP_S * RATE).round() as usize;
            assert!(y[1..lag].iter().all(|v| *v == 0.0), "channel {ch}: something before the slap");
            let echo: f64 = y[lag..].iter().sum();
            let pole = y[lag + 1] / y[lag];
            let want_pole = (-std::f64::consts::TAU * f64::from(SLAP_HZ) / f64::from(RATE)).exp();
            println!("  slap, channel {ch}: at {:.3} ms, gain {echo:+.4}, pole {pole:.4}", lag as f64 / f64::from(RATE) * 1000.0);
            assert!((echo - sign * 0.127).abs() < 0.002, "channel {ch}: the slap's gain {echo:+.4}");
            assert!((pole - want_pole).abs() < 1e-4, "channel {ch}: the slap's pole {pole:.4}");
            assert!(((lag as f64 / f64::from(RATE)) - 0.045).abs() < 1e-4);
        }
        let mut both = vec![(0.0_f32, 0.0_f32); n];
        both[0] = (1.0, 1.0);
        let out = through([true, false, false, false], 0.5, &both);
        let mono: f64 = out[1..].iter().map(|(l, r)| f64::from(l + r).abs()).sum();
        assert!(mono < 1e-6, "the slap leaks {mono:e} into mono");
    }

    /// **The cabinet: a broad +3 dB bell at 500 Hz** (the reference's +3.0 at
    /// 504, +1.5 at 200 and 1270, +0.5 at 100 and 2540; bench +3.0, +1.4,
    /// +1.4, +0.5, +0.5).
    #[test]
    fn the_cabinet_is_a_broad_bell_at_500_hz() {
        let gain = |f: f32| {
            let n = (0.5 * RATE) as usize;
            let x: Vec<(f32, f32)> = (0..n)
                .map(|i| {
                    let v = (std::f32::consts::TAU * f * i as f32 / RATE).sin();
                    (v, v)
                })
                .collect();
            let out = through([false, true, false, false], 0.5, &x);
            let a = (0.25 * RATE) as usize;
            let e_in: f64 = x[a..].iter().map(|v| f64::from(v.0 * v.0)).sum();
            let e_out: f64 = out[a..].iter().map(|v| f64::from(v.0 / WET_TRIM).powi(2)).sum();
            10.0 * (e_out / e_in).log10()
        };
        let g: Vec<f64> = [100.0_f32, 200.0, 500.0, 1270.0, 2540.0].iter().map(|&f| gain(f)).collect();
        println!("  cabinet at 100 / 200 / 500 / 1270 / 2540 Hz: {g:+.2?} dB");
        assert!((g[2] - 3.0).abs() < 0.05, "the bell's top {:+.2}", g[2]);
        assert!((g[1] - 1.5).abs() < 0.5 && (g[3] - 1.5).abs() < 0.5, "the bell's shoulders {:+.2} / {:+.2}", g[1], g[3]);
        assert!((g[0] - 0.5).abs() < 0.4 && (g[4] - 0.5).abs() < 0.4, "the bell's skirts {:+.2} / {:+.2}", g[0], g[4]);
    }

    /// **The room: a couple of seconds, under the dry.** Its impulse
    /// response's RT60 (the backward-integrated energy from −5 to −35 dB,
    /// both channels) between 0.7 × the reference's shortest and 1.3 × its
    /// longest after the fx score's key-offs (1.84–2.02 s with the
    /// reference measured as its reverb render less 0.95 of its bare one;
    /// the brief's ±30 %), because a flat impulse weighs the octaves above
    /// 1 kHz, where this room (and the reference's) is shorter — on the
    /// key-offs, which weigh the reed's 100–500 Hz, the bench reads 1.89 /
    /// 2.14 / 2.16 s against 2.02 / 1.84 / 1.94; and its energy 20–35 dB
    /// under the impulse's.
    #[test]
    fn the_room_rings_as_the_references() {
        let n = (4.0 * RATE) as usize;
        let mut imp = vec![(0.0_f32, 0.0_f32); n];
        imp[0] = (1.0, 1.0);
        let out = through([false, false, false, true], 0.5, &imp);
        // The room alone: the impulse itself (the dry path) is sample 0.
        let mut e: Vec<f64> = out.iter().map(|(l, r)| f64::from(l * l + r * r)).collect();
        e[0] = 0.0;
        let mut back = vec![0.0_f64; n + 1];
        for i in (0..n).rev() {
            back[i] = back[i + 1] + e[i];
        }
        let lvl = |i: usize| 10.0 * (back[i] / back[1]).log10();
        let t5 = (0..n).find(|&i| lvl(i) < -5.0).unwrap_or(n);
        let t35 = (0..n).find(|&i| lvl(i) < -35.0).unwrap_or(n);
        let rt = (t35 - t5) as f64 / f64::from(RATE) * 2.0;
        let (mut lr, mut ll, mut rr) = (0.0, 0.0, 0.0);
        for (l, r) in &out[1..] {
            lr += f64::from(l * r);
            ll += f64::from(l * l);
            rr += f64::from(r * r);
        }
        let corr = lr / (ll * rr).sqrt();
        let room = 10.0 * (back[1] / f64::from(2.0 * WET_TRIM * WET_TRIM)).log10();
        println!("  room: RT60 {rt:.2} s, L/R {corr:+.3}, energy {room:+.1} dB re the impulse");
        assert!((1.3..=2.7).contains(&rt), "the room's RT60 {rt:.2} s");
        assert!((-35.0..=-20.0).contains(&room), "the room's energy {room:+.1} dB re the impulse");
    }

    /// **One room in both channels**: white noise through the room alone,
    /// its second to tenth second. The two channels equally loud overall
    /// and in every octave from 125 Hz to 4 kHz within 0.5 dB — the room's
    /// one output through two all-passes (the reference's balance stays
    /// within −1.9..−0.6 dB over the fx score's four passages; four lines a
    /// side swung this room's −4.9..+6.0) — in phase under 250 Hz (L/R
    /// correlation at least +0.8 in 125–250 Hz; the reference's +0.72 /
    /// +0.98 on its chord and held C3 under 300 Hz, where each channel
    /// summing its own lines read −0.61) and apart over 1 kHz (|corr| ≤
    /// 0.3 in 1–2 and 2–4 kHz; the reference's −0.14 / −0.16).
    #[test]
    fn the_room_is_one_room_in_both_channels() {
        let n = (10.0 * RATE) as usize;
        let mut seed = 0x1234_5678_u32;
        let x: Vec<(f32, f32)> = (0..n)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let v = (seed >> 8) as f32 / 8_388_608.0 - 1.0;
                (0.1 * v, 0.1 * v)
            })
            .collect();
        let out = through([false, false, false, true], 0.5, &x);
        // The room alone: the dry path is the input times the trim.
        let wet: Vec<(f64, f64)> = out
            .iter()
            .zip(&x)
            .skip(RATE as usize)
            .map(|((l, r), (a, b))| (f64::from(*l - a * WET_TRIM), f64::from(*r - b * WET_TRIM)))
            .collect();
        // Two RBJ band-passes in cascade (constant 0 dB peak) on each
        // channel: an octave with skirts steep enough not to borrow from
        // the next.
        let band = |lo: f64, hi: f64| -> (f64, f64) {
            let f = (lo * hi).sqrt();
            let q = f / (hi - lo);
            let w = std::f64::consts::TAU * f / f64::from(RATE);
            let alpha = w.sin() / (2.0 * q);
            let a0 = 1.0 + alpha;
            let c = [alpha / a0, 0.0, -alpha / a0, -2.0 * w.cos() / a0, (1.0 - alpha) / a0];
            let mut z = [[[0.0_f64; 2]; 2]; 2];
            let (mut ll, mut rr, mut lr) = (0.0, 0.0, 0.0);
            for (l, r) in &wet {
                let mut y = [0.0; 2];
                for (ch, v) in [*l, *r].into_iter().enumerate() {
                    let mut v = v;
                    for zz in &mut z[ch] {
                        let o = c[0] * v + zz[0];
                        zz[0] = c[1] * v - c[3] * o + zz[1];
                        zz[1] = c[2] * v - c[4] * o;
                        v = o;
                    }
                    y[ch] = v;
                }
                ll += y[0] * y[0];
                rr += y[1] * y[1];
                lr += y[0] * y[1];
            }
            (10.0 * (rr / ll).log10(), lr / (ll * rr).sqrt())
        };
        let (ll, rr) = wet.iter().fold((0.0, 0.0), |(a, b), (l, r)| (a + l * l, b + r * r));
        let all = 10.0 * (rr / ll).log10();
        let rows: Vec<(f64, f64, f64)> =
            [(125.0, 250.0), (250.0, 500.0), (500.0, 1000.0), (1000.0, 2000.0), (2000.0, 4000.0)].iter().map(|&(a, b)| { let (bal, c) = band(a, b); (a, bal, c) }).collect();
        println!("  room: R-L {all:+.2} dB overall; by octave (from, R-L, corr): {rows:+.2?}");
        assert!(all.abs() <= 0.5, "the room is {all:+.2} dB louder on the right");
        for &(f, bal, c) in &rows {
            assert!(bal.abs() <= 0.5, "the room's {f} Hz octave is {bal:+.2} dB louder on the right");
            if f < 200.0 {
                assert!(c >= 0.8, "the room's {f} Hz octave correlates {c:+.2} between channels");
            }
            if f >= 1000.0 {
                assert!(c.abs() <= 0.3, "the room's {f} Hz octave correlates {c:+.2} between channels");
            }
        }
    }

    /// **The throb's knob moves without a step**: knob 5 jumps from 0 to 1
    /// at the throb's trough (wet) and its dip (dry) under a steady signal;
    /// through [`KNOB_SMOOTH_S`] neither gain moves more than a thousandth
    /// in a sample (the knob taken as it came, the wet gain stepped from 1
    /// to the trough's 10^(−6/20) = 0.50, the dry from 1 to 0.40), and once
    /// settled each is exactly the steady knob's.
    #[test]
    fn the_throb_knob_moves_without_a_step() {
        let n = RATE as usize;
        // The wet throb's trough is at sin = −1, a quarter period before
        // a whole one: 0.1875 s into a 4 Hz curve; the dry one's at
        // sin = +1, 1/22 s into 5.5 Hz.
        for (name, at) in [("wet", (0.1875 * RATE) as usize), ("dry", (RATE / 22.0) as usize)] {
            let mut a = ReedAmp::new(RATE);
            let mut b = ReedAmp::new(RATE);
            a.set_stages(false, false, true, false);
            b.set_stages(false, false, true, false);
            let mut worst = 0.0_f32;
            let mut last: Option<f32> = None;
            for i in 0..n {
                let knob = if i < at { 0.0 } else { 1.0 };
                let (g, steady) = if name == "wet" {
                    (a.wet(1.0, 1.0, knob).0, b.wet(1.0, 1.0, 1.0).0)
                } else {
                    (a.dry(1.0, 1.0, knob).0, b.dry(1.0, 1.0, 1.0).0)
                };
                if let Some(p) = last {
                    worst = worst.max((g - p).abs());
                }
                last = Some(g);
                if i == n - 1 {
                    assert_eq!(g, steady, "{name}: the settled throb is not the steady knob's");
                }
                if i % BLOCK == BLOCK - 1 {
                    a.end_block();
                    b.end_block();
                }
            }
            println!("  {name} throb: largest one-sample move of the gain through the knob's jump {worst:.5}");
            assert!(worst <= 1e-3, "{name}: the knob's jump stepped the gain {worst:.4} in one sample");
        }
    }

    /// **The dry patch's throb is the frame's old one, sample for sample**
    /// (5.5 Hz, `1 − 0.3 d (1 + sin)`), and at the 200's default depth 0
    /// it is the identity.
    #[test]
    fn the_dry_throb_is_the_frames_old_one() {
        for depth in [0.0_f32, 0.4, 1.0] {
            let mut a = ReedAmp::new(RATE);
            let mut phase = 0.0_f32;
            let inc = std::f32::consts::TAU * 5.5 / RATE;
            for i in 0..RATE as usize {
                let x = (i as f32 * 0.01).sin();
                phase = (phase + inc) % std::f32::consts::TAU;
                let g = 1.0 - depth * 0.30 * (1.0 + phase.sin());
                let (l, r) = a.dry(x, -x, depth);
                assert!(l == x * g && r == -x * g, "depth {depth} sample {i}: the dry throb moved");
                if depth == 0.0 {
                    assert!(l == x && r == -x, "depth 0 is not the identity");
                }
            }
        }
        assert_eq!(Patch::Wurli200.defaults()[5], 0.0, "the 200's default throb");
    }

    #[test]
    fn the_amp_is_bounded() {
        let mut a = ReedAmp::new(RATE);
        let mut pk = 0.0_f32;
        for n in 0..(RATE as usize) {
            let x = (std::f32::consts::TAU * 262.0 * n as f32 / RATE).sin() * 0.5;
            let (l, r) = a.wet(x, x, 1.0);
            pk = pk.max(l.abs()).max(r.abs());
        }
        assert!(pk.is_finite() && pk < 1.0, "peak {pk}");
    }

    /// **The amp falls silent and stays out of the denormals**: after the
    /// last note, `ringing` goes false within the room's tail, and with
    /// the block flush the output is exact zeros, not a decaying 1e-30.
    #[test]
    fn the_amp_falls_silent() {
        let mut a = ReedAmp::new(RATE);
        for i in 0..(RATE as usize) {
            let x = (std::f32::consts::TAU * 330.0 * i as f32 / RATE).sin() * 0.3;
            a.wet(x, x, 0.5);
            if i % BLOCK == BLOCK - 1 {
                a.end_block();
            }
        }
        let mut quiet_at = None;
        for i in 0..(30.0 * RATE) as usize {
            let (l, r) = a.wet(0.0, 0.0, 0.5);
            assert!(l == 0.0 || l.abs() > 1e-30, "a denormal on the left at {i}");
            assert!(r == 0.0 || r.abs() > 1e-30, "a denormal on the right at {i}");
            if i % BLOCK == BLOCK - 1 {
                a.end_block();
            }
            if quiet_at.is_none() && !a.ringing() {
                quiet_at = Some(i);
            }
        }
        let t = quiet_at.map(|i| i as f32 / RATE);
        println!("  the room stops ringing {t:?} s after the last note");
        assert!(t.is_some_and(|t| t < 15.0), "the room never stopped: {t:?}");
        let (l, r) = a.wet(0.0, 0.0, 0.5);
        assert!(l == 0.0 && r == 0.0, "the amp is not silent: {l:e} {r:e}");
    }

    /// **No clipping: a fortissimo note leaves 3 dB of headroom through
    /// the amp** (the brief), on the path the instrument plays: every third
    /// key from A0 to C8, struck eight times at v127 through `Keys` on the
    /// Amp patch at its shipped knobs — four pairs, the second of each
    /// landing on the first while it rings, as `Keys` lands a re-struck key
    /// on its own voice — peaks at −3 dBFS or under on both channels, and
    /// the dry 200 too (the lane's verifier: with a knock drawn anew at
    /// every strike, twelve strikes of C4 through `Keys` reached −2.3 dBFS
    /// here where one fresh voice read −5.1; now −7.0 dBFS at worst on both
    /// patches, G#2 and D#2).
    #[test]
    fn a_fortissimo_note_leaves_headroom_through_the_amp() {
        use super::super::Keys;
        for patch in [Patch::Wurli140, Patch::Wurli200] {
            let mut worst = (f32::MIN, 0_u8);
            for key in (21_u8..=108).step_by(3) {
                let mut k = Keys::new(RATE);
                k.set_patch(patch);
                let mut scratch = vec![0.0_f32; BLOCK * 2];
                let mut pk = 0.0_f32;
                let mut run = |k: &mut Keys, secs: f32, pk: &mut f32| {
                    for _ in 0..(secs * RATE) as usize / BLOCK {
                        scratch.fill(0.0);
                        k.render(&mut scratch, BLOCK, 2);
                        *pk = scratch.iter().fold(*pk, |m, s| m.max(s.abs()));
                    }
                };
                for _ in 0..4 {
                    k.note_on(key, 1.0);
                    run(&mut k, 0.3, &mut pk);
                    k.note_on(key, 1.0);
                    run(&mut k, 0.3, &mut pk);
                    k.note_off(key);
                    run(&mut k, 0.4, &mut pk);
                }
                let p = 20.0 * pk.max(1e-12).log10();
                if p > worst.0 {
                    worst = (p, key);
                }
            }
            println!("  {patch:?}: loudest fortissimo through Keys {:.1} dBFS at key {}", worst.0, worst.1);
            assert!(worst.0 <= -3.0, "{patch:?}: key {} peaks at {:.1} dBFS", worst.1, worst.0);
        }
    }
    /// **A reset amp is a new one.** After a history of signal through the
    /// wet chain and the dry throb at random knobs, levels and lengths,
    /// `reset` leaves nothing of it: the same signal and knob from there
    /// render exactly what a new amp renders, to the bit, through the slap's
    /// delay, the room's longest line and their tails. Kept on purpose: the
    /// stages a bench switched (`set_stages`, which the instrument never
    /// calls), and the knob smoother's last value, which `reset` marks
    /// unset so the next knob is taken as it comes, as a new amp takes it.
    #[test]
    fn a_reset_amp_is_a_new_one() {
        let mut seed = 0x0dd5_eed5_u32;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32
        };
        for trial in 0..10 {
            let mut used = ReedAmp::new(RATE);
            let level = 10.0_f32.powf(-2.0 * rnd());
            for i in 0..(2_000 + (rnd() * 60_000.0) as usize) {
                let knob = if i % 4_000 < 2_000 { rnd() } else { 0.7 };
                let (x, y) = (level * (2.0 * rnd() - 1.0), level * (2.0 * rnd() - 1.0));
                if trial % 2 == 0 {
                    used.wet(x, y, knob);
                } else {
                    used.dry(x, y, knob);
                }
                if i % BLOCK == BLOCK - 1 {
                    used.end_block();
                }
            }
            used.reset();
            let mut fresh = ReedAmp::new(RATE);
            for i in 0..(5.0 * RATE) as usize {
                let x = if (2_000..2_400).contains(&i) { 0.3 * (2.0 * rnd() - 1.0) } else { 0.0 };
                let knob = if i < 50_000 { 0.4 } else { 0.9 };
                let (a, b) = if trial % 2 == 0 { (used.wet(x, -x, knob), fresh.wet(x, -x, knob)) } else { (used.dry(x, -x, knob), fresh.dry(x, -x, knob)) };
                assert!(a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits(), "trial {trial}: the reset amp differs from a new one {i} samples on ({:e} against {:e})", a.0, b.0);
                if i % BLOCK == BLOCK - 1 {
                    used.end_block();
                    fresh.end_block();
                }
                assert_eq!(used.ringing(), fresh.ringing(), "trial {trial}: the reset amp rings where a new one does not, {i} samples on");
            }
        }
    }
}
