//! The realtime convolution kernel: uniform partitioned convolution in the
//! frequency domain, for playing captured impulse responses — the playback
//! half of the IR story whose capture math lives next door in `sweep.rs`.
//!
//! # The shape
//!
//! A frequency-domain delay line (Wefers & Vorlander, DAFx-11): the IR is
//! chopped into 512-sample partitions, each held as a 1024-point spectrum;
//! every 512 input samples one forward FFT goes in, the spectrum ring
//! shifts, and each partition multiplies the input spectrum from its own
//! age — one complex multiply-accumulate per bin per partition, one inverse
//! FFT per output channel. Cost grows linearly with IR length while the
//! per-sample work of direct convolution would be the whole IR every
//! sample; a 6-second IR at 48 k is ~563 partitions and runs comfortably.
//! Only bins 0..=512 are kept and accumulated — the input is real, the
//! spectrum Hermitian, so the mirror half is reconstruction, not work.
//!
//! **The partition SIZE is fixed; the partition COUNT is not.** Nothing here
//! caps IR length — `Engine::build` sizes every buffer from the IR it is
//! handed — and the capture road's own ceiling is the silence recorded after
//! the sweep, at most twenty seconds. That is 1875 partitions, and
//! `the_longest_ir_the_wizard_can_produce_is_not_beyond_the_kernel` runs it,
//! because "the IR came back too short" is a question that arrives here
//! first and the answer is that it was never this.
//!
//! **Which makes twenty seconds a ceiling somebody has to hold, and it is
//! not held here.** This kernel would cheerfully build 28,000 partitions for
//! a five-minute file and bill the audio callback for them every block,
//! forever, because nothing in a convolver knows what a reasonable impulse
//! response is. The road that loads somebody else's IR is where the length
//! is judged, before a bay is ever pointed at a file:
//! `desktop::IR_LOAD_MAX_S`, which names this measurement as its reason.
//!
//! The partition is FIXED at 512 to match the engine's MAX_BLOCK chunking,
//! and the engine's variable chunk sizes are absorbed by internal staging:
//! input collects until a partition is full, output drains from a FIFO
//! primed with exactly one partition of silence. That priming IS the
//! latency — [`Convolver::latency`] reports 512, honest and exact, and the
//! dry half of the mix rides a 512-sample line of its own so dry and wet
//! stay phase-coherent through the blend. Mix at zero remains a bit-exact
//! wire (and clears once), like every unit in this directory.
//!
//! # The allocation contract
//!
//! Everything is preallocated in [`Convolver::set_ir`]: filter spectra,
//! spectrum rings, scratch, FIFOs. `set_ir` is the HOST side of the
//! handoff — call it from the loading thread when an IR arrives or swaps,
//! never from the audio callback; [`Convolver::process`] itself touches no
//! allocator, ever. When the suite's engine lane registers this unit, that
//! split is the contract to keep.
//!
//! # Swapping without a click
//!
//! An IR swap builds the new engine into the spare slot and crossfades:
//! one partition of WAIT (the new engine's FIFO is still priming — fading
//! toward it during that window would fade toward silence and then click
//! when its first real block lands at full weight), then a half-cosine
//! crossfade over one partition. Old and new streams sit at the same
//! 512-sample latency throughout, so the blend is between time-aligned
//! signals. The first load fades up from silence through the same
//! machinery, so loading an IR mid-playback cannot click either. A second
//! swap arriving inside a still-running fade snaps the first fade closed —
//! audible only if the first pair differed loudly; a host that cares waits
//! the 21 ms.

// Nothing reaches this module yet: the suite's engine lane wires it later,
// and proving the DSP before any panel exists is the point.
#![allow(dead_code)]

// ── what the dials map to ───────────────────────────────────────────────────

/// The partition size in samples, fixed to the engine's MAX_BLOCK so one
/// engine-sized chunk is exactly one convolution step. Also the reported
/// latency: the output FIFO is primed with one partition of silence.
pub const CONV_PARTITION: usize = 512;

/// **How many distinct step phases instances are spread across.**
///
/// Every convolver on the desk used to step on the same block. `fill`
/// starts at zero in every instance, so sixteen IR bays crossed the
/// partition boundary together and the whole cost of all of them landed in
/// one block out of four at 128 frames, with three near-empty blocks
/// around it. The peak is what glitches, not the mean, so that was the
/// worst possible arrangement of the same work.
///
/// Staggering where each instance starts in its staging buffer spreads the
/// steps across the whole period, and costs nothing: `set_ir` already
/// primes the output FIFO by what is staged, so a unit that starts part
/// full reports and delivers the same one-partition latency as one that
/// starts empty. Sixteen divides the partition evenly and covers every
/// block size down to 32 frames.
const CONV_SPREAD: usize = 16;

/// FFT size: two partitions, the textbook K = 2B — each 512-sample block is
/// transformed alongside the previous one so the circular convolution's
/// contaminated half falls entirely in the discarded first partition.
const FFT_SIZE: usize = 2 * CONV_PARTITION;

/// Retained spectrum bins, 0..=Nyquist. The input is real, so the upper
/// half of every spectrum is the conjugate mirror of the lower — storing
/// and multiplying it would double the hot loop for information already
/// in hand.
const BINS: usize = FFT_SIZE / 2 + 1;

/// What the output trim spans, in dB, linear, unity at noon. ±24 dB covers
/// any sanely captured IR set; unlike the compressor's makeup this dial
/// does NOT reach silence at the bottom — a trim is a level match, and
/// muting the wet path is what the mix dial is for.
pub const CONV_TRIM_DB: (f32, f32) = (-24.0, 24.0);

/// Below this a 0..=1 dial is at its resting position and means OFF —
/// the same figure the fixed chain uses, duplicated because this kernel
/// must stand alone.
const CONV_OFF: f32 = 0.0005;

// ── the parameters ──────────────────────────────────────────────────────────

/// Every dial, normalised 0..=1. The mapping to what the DSP wants lives in
/// the consts above, next to the reasons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConvolverParams {
    /// Dry/wet. 1 is the convolution, 0 is a bit-exact wire. Anywhere
    /// between, the dry half is delayed to the wet half's latency so the
    /// blend is phase-coherent rather than a 512-sample comb.
    pub mix: f32,
    /// Wet-path output level across [`CONV_TRIM_DB`]; unity at noon.
    pub trim: f32,
}

/// The shipped sound: fully wet at unity trim — an insert that IS the
/// loaded IR, which is what a convolver is until somebody reaches for
/// parallel.
impl Default for ConvolverParams {
    fn default() -> Self {
        Self { mix: 1.0, trim: 0.5 }
    }
}

/// How a handed-in IR set maps onto the stereo bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrLayout {
    /// Two IRs: the left channel through `irs[0]`, the right through
    /// `irs[1]` — a mono-in capture played as a stereo pair, the common
    /// case for a hardware-loop capture whose two returns matched.
    Stereo,
    /// Four IRs from two capture passes, `[L→L, L→R, R→L, R→R]`: each
    /// output is the sum of both inputs through their own IRs — true
    /// stereo, the QuadraVerb's cross-feeding algorithms kept honest.
    TrueStereo,
}

// ── the FFT ─────────────────────────────────────────────────────────────────

/// The twiddle factors for one [`FFT_SIZE`] transform, built once.
///
/// **The recurrence this replaces was the most expensive thing in the
/// transform.** Rotating the twiddle by a complex multiply inside the
/// butterfly loop costs four multiplies and two adds per butterfly, on top
/// of the four and six the butterfly itself needs: better than a third of
/// the transform spent working out an angle that is the same on every block
/// of every instance for the life of the process. It also drifts, because a
/// rotation applied five hundred times in f32 does not land where it
/// started.
///
/// One table serves every stage: stage `len` wants `w^(k * FFT_SIZE / len)`,
/// so the stride does the work. The inverse transform is the conjugate, so
/// it serves that too.
struct Twiddles {
    re: [f32; FFT_SIZE / 2],
    im: [f32; FFT_SIZE / 2],
}

/// **Built off the audio thread, always.** `Convolver::new` does not
/// transform, but `Engine::build` does, and it is host-side by contract, so
/// by the time `step` reads this the table is already here and the
/// `get_or_init` is a load and a branch. The `OnceLock` is belt and braces
/// against a future caller that is not.
static TWIDDLES: std::sync::OnceLock<Twiddles> = std::sync::OnceLock::new();

fn twiddles() -> &'static Twiddles {
    TWIDDLES.get_or_init(|| {
        let mut t = Twiddles {
            re: [0.0; FFT_SIZE / 2],
            im: [0.0; FFT_SIZE / 2],
        };
        for i in 0..FFT_SIZE / 2 {
            // f64 for the table, f32 for the transform: the angles are
            // worked out once and every block afterwards leans on them.
            let ang = -std::f64::consts::TAU * i as f64 / FFT_SIZE as f64;
            t.re[i] = ang.cos() as f32;
            t.im[i] = ang.sin() as f32;
        }
        t
    })
}

/// In-place radix-2 complex FFT of exactly [`FFT_SIZE`] points, f32 - the
/// audio-path transform, the same house idiom as `stretch.rs`, duplicated
/// because a unit stands alone. f32 is enough here: 1024 points keeps the
/// roundtrip error near -140 dB, far under any IR's noise floor.
///
/// The length is fixed rather than free because [`twiddles`] is sized for
/// it, and every caller in this file transforms a whole block. A slice of
/// any other length is left alone, as the old guard did.
fn fft(re: &mut [f32], im: &mut [f32], inverse: bool) {
    if re.len() != FFT_SIZE || im.len() != FFT_SIZE {
        return;
    }
    let n = FFT_SIZE;
    let tw = twiddles();
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
        let half = len / 2;
        let stride = n / len;
        let mut i = 0;
        while i < n {
            for k in 0..half {
                let t = k * stride;
                // The forward table is the conjugate of the inverse one,
                // which is why there is only one of them.
                let (cr, ci) = (tw.re[t], if inverse { -tw.im[t] } else { tw.im[t] });
                let (ur, ui) = (re[i + k], im[i + k]);
                let (vr, vi) = (
                    re[i + k + half] * cr - im[i + k + half] * ci,
                    re[i + k + half] * ci + im[i + k + half] * cr,
                );
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + half] = ur - vr;
                im[i + k + half] = ui - vi;
            }
            i += len;
        }
        len <<= 1;
    }
    if inverse {
        let s = 1.0 / n as f32;
        for v in re.iter_mut() {
            *v *= s;
        }
        for v in im.iter_mut() {
            *v *= s;
        }
    }
}

// ── one engine: a loaded IR and its delay line ──────────────────────────────

/// One IR's worth of machinery. Two live at once only while a swap fades.
struct Engine {
    /// Whether this slot is in the circuit. A retired slot keeps its
    /// allocations (dropping them in `process` would be a hidden free);
    /// the next `set_ir` overwrites them wholesale.
    live: bool,
    n_part: usize,
    /// The signal routes as `(input, output)` channel pairs, one per
    /// filter in `filt` — two for [`IrLayout::Stereo`], four for
    /// [`IrLayout::TrueStereo`].
    paths: Vec<(usize, usize)>,
    /// Per path: `n_part` partitions of `[re[BINS] | im[BINS]]`.
    filt: Vec<Vec<f32>>,
    /// Input spectra, newest at `head`, same flat layout — partition age
    /// `p` reads `(head + p) % n_part`.
    ring: [Vec<f32>; 2],
    head: usize,
    /// The previous partition of raw input per channel: each FFT block is
    /// `[previous | current]`, the overlap-save arrangement.
    prev: [Vec<f32>; 2],
    /// FFT scratch, one block.
    re: Vec<f32>,
    im: Vec<f32>,
    /// Spectrum accumulators per output channel, `[re | im]`.
    acc: [Vec<f32>; 2],
    /// Output FIFO per channel, a fixed ring of two partitions — the
    /// occupancy provably stays within [0, P+1].
    out: [Vec<f32>; 2],
    out_head: usize,
    out_len: usize,
}

impl Engine {
    /// A dead slot: no allocations to speak of until an IR arrives.
    fn empty() -> Self {
        Self {
            live: false,
            n_part: 0,
            paths: Vec::new(),
            filt: Vec::new(),
            ring: [Vec::new(), Vec::new()],
            head: 0,
            prev: [Vec::new(), Vec::new()],
            re: Vec::new(),
            im: Vec::new(),
            acc: [Vec::new(), Vec::new()],
            out: [Vec::new(), Vec::new()],
            out_head: 0,
            out_len: 0,
        }
    }

    /// Build a live engine for an IR set — every allocation this unit will
    /// ever need for it happens HERE, on the host side of the handoff.
    /// `prime` is how much silence the output FIFO starts with; see
    /// `Convolver::set_ir` for why it is not always a full partition.
    fn build(irs: &[Vec<f32>], layout: IrLayout, prime: usize) -> Self {
        let max_len = irs.iter().map(|ir| ir.len()).max().unwrap_or(0).max(1);
        let n_part = max_len.div_ceil(CONV_PARTITION);
        let paths: Vec<(usize, usize)> = match layout {
            IrLayout::Stereo => vec![(0, 0), (1, 1)],
            IrLayout::TrueStereo => vec![(0, 0), (0, 1), (1, 0), (1, 1)],
        };
        let mut re = vec![0.0f32; FFT_SIZE];
        let mut im = vec![0.0f32; FFT_SIZE];
        let mut filt = Vec::with_capacity(irs.len());
        for ir in irs {
            let mut f = vec![0.0f32; n_part * 2 * BINS];
            for p in 0..n_part {
                re.fill(0.0);
                im.fill(0.0);
                let start = p * CONV_PARTITION;
                if start < ir.len() {
                    let end = (start + CONV_PARTITION).min(ir.len());
                    for (d, &s) in re.iter_mut().zip(&ir[start..end]) {
                        // A poisoned sample in an IR file must not poison
                        // every buffer this unit ever touches.
                        *d = if s.is_finite() { s } else { 0.0 };
                    }
                }
                fft(&mut re, &mut im, false);
                f[p * 2 * BINS..][..BINS].copy_from_slice(&re[..BINS]);
                f[p * 2 * BINS + BINS..][..BINS].copy_from_slice(&im[..BINS]);
            }
            filt.push(f);
        }
        Self {
            live: true,
            n_part,
            paths,
            filt,
            ring: [vec![0.0; n_part * 2 * BINS], vec![0.0; n_part * 2 * BINS]],
            head: 0,
            prev: [vec![0.0; CONV_PARTITION], vec![0.0; CONV_PARTITION]],
            re,
            im,
            acc: [vec![0.0; 2 * BINS], vec![0.0; 2 * BINS]],
            out: [vec![0.0; 2 * CONV_PARTITION], vec![0.0; 2 * CONV_PARTITION]],
            out_head: 0,
            out_len: prime,
        }
    }

    /// Forget the audio, keep the IR: rings, history and FIFO to silence,
    /// FIFO re-primed to `prime`, which is a full partition less whatever
    /// the owner's staging restarts holding. See [`CONV_SPREAD`].
    fn reset(&mut self, prime: usize) {
        for r in &mut self.ring {
            r.fill(0.0);
        }
        for p in &mut self.prev {
            p.fill(0.0);
        }
        for o in &mut self.out {
            o.fill(0.0);
        }
        self.head = 0;
        self.out_head = 0;
        self.out_len = if self.live { prime.min(CONV_PARTITION) } else { 0 };
    }

    /// One partition in, one partition out: transform the staged input,
    /// walk the delay line, and append 512 wet samples per channel to the
    /// FIFO. Allocation-free by construction — every slice here was sized
    /// in `build`.
    fn step(&mut self, stage: &[Vec<f32>; 2]) {
        let np = self.n_part;
        self.head = (self.head + np - 1) % np;
        // **Both channels in one transform.** The input is real, so half of
        // a complex transform was being thrown away: the old code filled
        // `im` with zeros, transformed, and kept only the Hermitian half of
        // the answer, twice. Riding the left channel in the real part and
        // the right in the imaginary part gives both spectra from one
        // transform, and they come apart below by symmetry alone.
        self.re[..CONV_PARTITION].copy_from_slice(&self.prev[0]);
        self.re[CONV_PARTITION..].copy_from_slice(&stage[0]);
        self.im[..CONV_PARTITION].copy_from_slice(&self.prev[1]);
        self.im[CONV_PARTITION..].copy_from_slice(&stage[1]);
        self.prev[0].copy_from_slice(&stage[0]);
        self.prev[1].copy_from_slice(&stage[1]);
        fft(&mut self.re, &mut self.im, false);
        {
            // `Z = L + jR` splits as `L[k] = (Z[k] + conj(Z[-k]))/2` and
            // `R[k] = (Z[k] - conj(Z[-k]))/2j`, with `-k` taken mod the
            // transform length so that DC and Nyquist, which are their own
            // mirrors, need no special case.
            let base = self.head * 2 * BINS;
            let (re, im) = (&self.re, &self.im);
            let [left, right] = &mut self.ring;
            let l = &mut left[base..][..2 * BINS];
            let r = &mut right[base..][..2 * BINS];
            for k in 0..BINS {
                let m = (FFT_SIZE - k) % FFT_SIZE;
                let (zr, zi) = (re[k], im[k]);
                let (mr, mi) = (re[m], im[m]);
                l[k] = 0.5 * (zr + mr);
                l[BINS + k] = 0.5 * (zi - mi);
                r[k] = 0.5 * (zi + mi);
                r[BINS + k] = 0.5 * (mr - zr);
            }
        }
        self.acc[0].fill(0.0);
        self.acc[1].fill(0.0);
        // The hot loop: one complex multiply-accumulate per bin per
        // partition per path, on the kept half-spectrum only.
        for (pi, &(in_ch, out_ch)) in self.paths.iter().enumerate() {
            let filt = &self.filt[pi];
            let ring = &self.ring[in_ch];
            let (ar, ai) = self.acc[out_ch].split_at_mut(BINS);
            for p in 0..np {
                let x = &ring[((self.head + p) % np) * 2 * BINS..][..2 * BINS];
                let h = &filt[p * 2 * BINS..][..2 * BINS];
                let (xr, xi) = x.split_at(BINS);
                let (hr, hi) = h.split_at(BINS);
                for k in 0..BINS {
                    ar[k] += xr[k] * hr[k] - xi[k] * hi[k];
                    ai[k] += xr[k] * hi[k] + xi[k] * hr[k];
                }
            }
        }
        let cap = 2 * CONV_PARTITION;
        {
            // The same trick backwards. Both accumulated spectra are
            // Hermitian, because every term in them is, so `L + jR` inverts
            // to a complex signal whose real part is the left output and
            // whose imaginary part is the right one. The mirror half is
            // still built by hand, because only the kept bins were summed,
            // but it is built once now instead of twice.
            let (ar, ai) = self.acc[0].split_at(BINS);
            let (br, bi) = self.acc[1].split_at(BINS);
            let (re, im) = (&mut self.re, &mut self.im);
            for k in 0..BINS {
                re[k] = ar[k] - bi[k];
                im[k] = ai[k] + br[k];
            }
            for k in 1..FFT_SIZE / 2 {
                re[FFT_SIZE - k] = ar[k] + bi[k];
                im[FFT_SIZE - k] = br[k] - ai[k];
            }
        }
        fft(&mut self.re, &mut self.im, true);
        // Overlap-save: the first partition of the inverse transform is
        // circular contamination; the second is the answer.
        for i in 0..CONV_PARTITION {
            let idx = (self.out_head + self.out_len + i) % cap;
            self.out[0][idx] = self.re[CONV_PARTITION + i];
            self.out[1][idx] = self.im[CONV_PARTITION + i];
        }
        self.out_len += CONV_PARTITION;
    }

    /// One wet frame off the FIFO. The occupancy argument in `process`
    /// guarantees there is one; an empty pop hands back silence rather
    /// than trusting that argument with a panic in the audio thread.
    fn pop(&mut self) -> [f32; 2] {
        if self.out_len == 0 {
            return [0.0, 0.0];
        }
        let i = self.out_head;
        self.out_head = (self.out_head + 1) % (2 * CONV_PARTITION);
        self.out_len -= 1;
        [self.out[0][i], self.out[1][i]]
    }
}

// ── the convolver ───────────────────────────────────────────────────────────

/// Uniform partitioned convolution with true-stereo IR sets, click-free
/// swapping and a phase-coherent dry blend. See the module header for the
/// design; see [`Convolver::process`] for the contract.
/// Handed out in order so that instances built together do not step
/// together. See [`CONV_SPREAD`].
static NEXT_PHASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub struct Convolver {
    sample_rate: f32,
    /// Where this instance's staging starts, and returns to after a
    /// [`Convolver::clear`]. See [`CONV_SPREAD`].
    phase: usize,
    /// Input staging toward the next partition, per channel.
    stage: [Vec<f32>; 2],
    fill: usize,
    /// The dry half's delay line, always running so an IR loaded
    /// mid-stream finds real history rather than a gap.
    dry: [Vec<f32>; 2],
    dry_pos: usize,
    /// Two engine slots: the active IR and, during a swap, the one on its
    /// way out (or in).
    slots: [Engine; 2],
    active: usize,
    /// Swap crossfade position in output samples: one partition of wait,
    /// then one partition of half-cosine ramp. Inactive when `fading` is
    /// down.
    fading: bool,
    fade_pos: usize,
    /// Whether the last call did nothing, so the off-clear runs once.
    idle: bool,
}

impl Convolver {
    /// A convolver whose steps fall where the last one's did not. See
    /// [`CONV_SPREAD`] for why that matters and why it is free.
    pub fn new(sample_rate: f32) -> Self {
        let n = NEXT_PHASE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self::at_phase(sample_rate, (n % CONV_SPREAD) * (CONV_PARTITION / CONV_SPREAD))
    }

    /// The same, at a stated phase. Production takes [`Convolver::new`];
    /// this exists so a test can assert an exact sample position without
    /// depending on how many convolvers the rest of the suite built first.
    pub fn at_phase(sample_rate: f32, phase: usize) -> Self {
        let phase = phase % CONV_PARTITION;
        Self {
            sample_rate: sample_rate.max(8_000.0),
            phase,
            stage: [vec![0.0; CONV_PARTITION], vec![0.0; CONV_PARTITION]],
            fill: phase,
            dry: [vec![0.0; CONV_PARTITION], vec![0.0; CONV_PARTITION]],
            dry_pos: 0,
            slots: [Engine::empty(), Engine::empty()],
            active: 0,
            fading: false,
            fade_pos: 0,
            idle: true,
        }
    }

    /// The unit's input-to-output delay in samples: exactly one partition —
    /// the output FIFO's priming, which is also all the internal buffering
    /// there is. Constant across IR length, layout, swaps and chunk sizes;
    /// the host's delay compensation can take it at its word.
    pub fn latency(&self) -> u32 {
        CONV_PARTITION as u32
    }

    /// Load or replace the IR set. HOST-SIDE ONLY: this is where every
    /// allocation happens (filter spectra, rings, FIFOs), so it must be
    /// called from the loading thread, never the audio callback — that is
    /// the handoff contract `process`'s allocation-freedom stands on.
    ///
    /// `irs` must match the layout — two IRs for [`IrLayout::Stereo`],
    /// four for [`IrLayout::TrueStereo`] — and hold at least one sample
    /// somewhere; a mismatched set is refused (returns false) rather than
    /// guessed at. Replacing a running IR crossfades over one partition
    /// after one partition of wait; the first load fades up from silence
    /// the same way, so neither can click.
    pub fn set_ir(&mut self, irs: &[Vec<f32>], layout: IrLayout) -> bool {
        let needed = match layout {
            IrLayout::Stereo => 2,
            IrLayout::TrueStereo => 4,
        };
        if irs.len() != needed || irs.iter().all(|ir| ir.is_empty()) {
            return false;
        }
        // The new engine's FIFO priming accounts for input already staged:
        // a full partition of priming would land its stream `fill` samples
        // late and the crossfade would blend time-skewed copies.
        let prime = CONV_PARTITION - self.fill;
        let spare = if self.slots[self.active].live {
            1 - self.active
        } else {
            self.active
        };
        // A swap arriving mid-fade overwrites the outgoing slot and snaps
        // that fade closed; the new fade then runs from the current engine.
        self.slots[spare] = Engine::build(irs, layout, prime);
        self.active = spare;
        self.fading = true;
        self.fade_pos = 0;
        true
    }

    /// The crossfade weight toward the incoming engine at `fade_pos` pops
    /// since the swap: 0 through the wait partition, half-cosine up
    /// through the ramp partition.
    #[inline]
    fn fade_weight(pos: usize) -> f32 {
        if pos < CONV_PARTITION {
            0.0
        } else {
            let t = (pos - CONV_PARTITION) as f32 / CONV_PARTITION as f32;
            0.5 * (1.0 - (std::f32::consts::PI * t).cos())
        }
    }

    /// Apply the convolution in place to an interleaved buffer.
    ///
    /// `channels` is the stride; anything past the first two channels is
    /// left alone and fewer than two is returned untouched, as the fixed
    /// chain does. With no IR loaded, or mix at zero, this is a bit-exact
    /// wire. Allocation-free: see [`Convolver::set_ir`] for the contract.
    pub fn process(
        &mut self,
        buf: &mut [f32],
        frames: usize,
        channels: usize,
        params: &ConvolverParams,
    ) {
        // A dial is a number out of a settings file somebody may have
        // edited: clamp, and treat anything non-finite as the resting place.
        let dial = |v: f32, rest: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                rest
            }
        };
        let mix = dial(params.mix, 1.0);
        if mix < CONV_OFF {
            // Off is a wire AND a reset, once: yesterday's tail must not
            // still be ringing when the mix comes back up.
            if !self.idle {
                self.clear();
                self.idle = true;
            }
            return;
        }
        self.idle = false;
        if channels < 2 {
            return;
        }
        // Design math in f64, once per block; the audio path stays f32.
        let trim_db = f64::from(CONV_TRIM_DB.0)
            + f64::from(CONV_TRIM_DB.1 - CONV_TRIM_DB.0) * f64::from(dial(params.trim, 0.5));
        let trim = 10.0f64.powf(trim_db / 20.0) as f32;

        for frame in buf.chunks_exact_mut(channels).take(frames) {
            let (in_l, in_r) = (frame[0], frame[1]);
            // The dry line runs even while no IR is loaded, so the moment
            // one lands the delayed dry half has history, not a hole.
            let dry_l = self.dry[0][self.dry_pos];
            let dry_r = self.dry[1][self.dry_pos];
            self.dry[0][self.dry_pos] = in_l;
            self.dry[1][self.dry_pos] = in_r;
            self.dry_pos = (self.dry_pos + 1) % CONV_PARTITION;

            if !self.slots[self.active].live {
                continue; // No IR is a wire, bit-exact.
            }

            self.stage[0][self.fill] = in_l;
            self.stage[1][self.fill] = in_r;
            self.fill += 1;
            if self.fill == CONV_PARTITION {
                self.fill = 0;
                self.slots[self.active].step(&self.stage);
                if self.fading && self.slots[1 - self.active].live {
                    self.slots[1 - self.active].step(&self.stage);
                }
            }

            let new = self.slots[self.active].pop();
            let wet = if self.fading {
                let old_slot = 1 - self.active;
                let old = if self.slots[old_slot].live {
                    self.slots[old_slot].pop()
                } else {
                    [0.0, 0.0] // First load: fading up from silence.
                };
                let w = Self::fade_weight(self.fade_pos);
                self.fade_pos += 1;
                if self.fade_pos >= 2 * CONV_PARTITION {
                    self.fading = false;
                    // Retired, not freed: dropping buffers here would be
                    // an allocator call on the audio thread.
                    self.slots[old_slot].live = false;
                }
                [old[0] + (new[0] - old[0]) * w, old[1] + (new[1] - old[1]) * w]
            } else {
                new
            };
            let (wet_l, wet_r) = (wet[0] * trim, wet[1] * trim);
            // dry + (wet − dry) · mix, so mix at one is exactly the wet.
            frame[0] = dry_l + (wet_l - dry_l) * mix;
            frame[1] = dry_r + (wet_r - dry_r) * mix;
        }
    }

    /// Drop every trace of what was playing — staging, delay lines, tails,
    /// any fade in flight — but KEEP the loaded IR: the unit leaving the
    /// circuit and returning should sound like itself, from silence.
    pub fn clear(&mut self) {
        for s in &mut self.stage {
            s.fill(0.0);
        }
        // Back to this instance's own phase, not to zero: a transport
        // stop must not re-align every convolver on the desk.
        self.fill = self.phase;
        for d in &mut self.dry {
            d.fill(0.0);
        }
        self.dry_pos = 0;
        if self.fading {
            // A fade cut short by a stop resolves to the incoming engine.
            self.slots[1 - self.active].live = false;
            self.fading = false;
        }
        self.fade_pos = 0;
        let prime = CONV_PARTITION - self.phase;
        for slot in &mut self.slots {
            slot.reset(prime);
        }
    }

    /// Every byte of Vec capacity in the unit, for the tests' proof that
    /// `process` never allocates: equal before and after means no internal
    /// buffer so much as considered growing.
    #[cfg(test)]
    fn footprint(&self) -> usize {
        let engine = |e: &Engine| -> usize {
            e.paths.capacity() * size_of::<(usize, usize)>()
                + e.filt.iter().map(|v| v.capacity() * 4).sum::<usize>()
                + e.filt.capacity() * size_of::<Vec<f32>>()
                + e.ring.iter().map(|v| v.capacity() * 4).sum::<usize>()
                + e.prev.iter().map(|v| v.capacity() * 4).sum::<usize>()
                + e.re.capacity() * 4
                + e.im.capacity() * 4
                + e.acc.iter().map(|v| v.capacity() * 4).sum::<usize>()
                + e.out.iter().map(|v| v.capacity() * 4).sum::<usize>()
        };
        self.stage.iter().map(|v| v.capacity() * 4).sum::<usize>()
            + self.dry.iter().map(|v| v.capacity() * 4).sum::<usize>()
            + engine(&self.slots[0])
            + engine(&self.slots[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// A deterministic little generator, so tests never pull in a crate.
    fn lcg(seed: &mut u64) -> f32 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*seed >> 33) as f32 / (1u64 << 31) as f32) - 1.0
    }

    fn noise(frames: usize, seed: u64) -> Vec<f32> {
        let mut s = seed;
        (0..frames * 2).map(|_| lcg(&mut s) * 0.5).collect()
    }

    /// An interleaved stereo sine, both channels in phase.
    fn sine(hz: f32, amp: f32, frames: usize, sr: f32) -> Vec<f32> {
        let mut buf = vec![0.0f32; frames * 2];
        let mut phase = 0.0f32;
        for f in buf.chunks_exact_mut(2) {
            phase += hz * std::f32::consts::TAU / sr;
            let v = amp * phase.sin();
            f[0] = v;
            f[1] = v;
        }
        buf
    }

    /// **A unit impulse IR is a null, modulo the honest latency** — and the
    /// same null at 44.1, 48 and 96 k, because nothing in the kernel is a
    /// length table: partitions are samples, whatever the second is.
    #[test]
    fn a_unit_impulse_ir_is_a_null_modulo_the_latency() {
        for sr in [44_100.0f32, 48_000.0, 96_000.0] {
            let mut c = Convolver::at_phase(sr, 0);
            assert_eq!(c.latency(), 512);
            assert!(c.set_ir(&[vec![1.0], vec![1.0]], IrLayout::Stereo));
            let frames = 8_192;
            let input = noise(frames, 7);
            let mut buf = input.clone();
            c.process(&mut buf, frames, 2, &ConvolverParams::default());
            // The first-load fade occupies two partitions; after it, wet is
            // the input delayed by exactly latency().
            for f in 2 * 1024..frames {
                for ch in 0..2 {
                    let got = buf[f * 2 + ch];
                    let want = input[(f - 512) * 2 + ch];
                    assert!(
                        (got - want).abs() < 1.0e-5,
                        "at {sr} Hz frame {f} ch {ch}: {got} vs {want}"
                    );
                }
            }
        }
    }

    /// **A three-tap IR round-trips exactly through any chunking**: sizes
    /// 1, 100, 512, 513 and 4096 all produce bit-identical output — the
    /// engine steps at partition boundaries whatever the caller's chunks
    /// do — and that output is the direct-form convolution, delayed by the
    /// reported latency.
    #[test]
    fn a_three_tap_ir_round_trips_exactly_through_any_chunking() {
        let taps_l = [0.6f32, -0.4, 0.25];
        let taps_r = [0.5f32, 0.3, -0.2];
        let irs = [taps_l.to_vec(), taps_r.to_vec()];
        let frames = 12_000;
        let input = noise(frames, 99);
        let p = ConvolverParams::default();

        let run = |chunk: usize| -> Vec<f32> {
            let mut c = Convolver::at_phase(SR, 0);
            assert!(c.set_ir(&irs, IrLayout::Stereo));
            let mut buf = input.clone();
            let mut pos = 0;
            while pos < frames {
                let n = chunk.min(frames - pos);
                c.process(&mut buf[pos * 2..], n, 2, &p);
                pos += n;
            }
            buf
        };

        let reference = run(512);
        for chunk in [1usize, 100, 513, 4096] {
            assert_eq!(
                run(chunk),
                reference,
                "chunk size {chunk} produced different samples"
            );
        }

        // And the samples are the mathematics: direct convolution, 512 late.
        for f in 2 * 1024..frames {
            for (ch, taps) in [(0usize, &taps_l), (1, &taps_r)] {
                let mut want = 0.0f32;
                for (k, &t) in taps.iter().enumerate() {
                    if f >= 512 + k {
                        want += t * input[(f - 512 - k) * 2 + ch];
                    }
                }
                let got = reference[f * 2 + ch];
                assert!(
                    (got - want).abs() < 1.0e-4,
                    "frame {f} ch {ch}: {got} vs direct {want}"
                );
            }
        }
    }

    /// **The partition COUNT is not a ceiling**, and the longest IR the
    /// wizard can hand over proves it.
    ///
    /// Asked directly, because "the IR is too short" pointed here first: the
    /// partition SIZE is fixed at 512 to match the engine's chunking, and it
    /// would be easy to read that as a fixed number of partitions too. There
    /// is no such number. `Engine::build` sizes everything from the IR it is
    /// given, so the only ceiling is what the machine will run — and the
    /// capture road's own ceiling is the post-sweep silence, at most 20 s
    /// from the wizard's tail row.
    ///
    /// Twenty seconds at 48 k is 1875 partitions, three times the six-second
    /// case next door. The realtime assertion is release-only for the same
    /// reason it is there: debug bounds checks are not the shipping speed.
    #[test]
    fn the_longest_ir_the_wizard_can_produce_is_not_beyond_the_kernel() {
        let ir_len: usize = 20 * 48_000;
        assert_eq!(
            ir_len.div_ceil(CONV_PARTITION),
            1875,
            "the partition arithmetic moved"
        );
        let mut seed = 99u64;
        let make = |seed: &mut u64| -> Vec<f32> {
            (0..ir_len)
                .map(|i| {
                    let t = i as f32 / 48_000.0;
                    lcg(seed) * 0.02 * (-t / 4.0).exp()
                })
                .collect()
        };
        let irs = [make(&mut seed), make(&mut seed)];
        let mut c = Convolver::at_phase(SR, 0);
        assert!(c.set_ir(&irs, IrLayout::Stereo), "a 20 s IR was refused");
        assert_eq!(c.latency(), CONV_PARTITION as u32, "a long IR moved the latency");

        let seconds = 4.0f64;
        let frames = (seconds * 48_000.0) as usize;
        let mut buf = noise(frames, 11);
        let before = c.footprint();
        let p = ConvolverParams::default();
        let start = std::time::Instant::now();
        let mut pos = 0;
        while pos < frames {
            let n = 480.min(frames - pos);
            c.process(&mut buf[pos * 2..], n, 2, &p);
            pos += n;
        }
        let elapsed = start.elapsed().as_secs_f64();
        assert_eq!(
            c.footprint(),
            before,
            "process grew a buffer somewhere - the allocation contract is broken"
        );
        assert!(
            buf.iter().all(|s| s.is_finite()),
            "a twenty-second IR produced a non-finite sample"
        );
        let factor = seconds / elapsed;
        println!("convolver: 20 s IR over 4 s of audio = {factor:.1}x realtime");
        #[cfg(not(debug_assertions))]
        assert!(
            factor > 2.0,
            "only {factor:.1}x realtime in release for the longest IR the \
             wizard can hand over"
        );
    }


    /// **A six-second IR processes ten seconds of audio without allocating
    /// and well under realtime.** Capacities are byte-identical before and
    /// after — `process` never touched the allocator — and the wall-clock
    /// figure is printed for the record. The realtime assertion only runs
    /// in release builds: debug-mode bounds checks are not the shipping
    /// performance and have no business failing the suite.
    #[test]
    fn a_six_second_ir_runs_ten_seconds_without_allocating_and_under_realtime() {
        let ir_len = 6 * 48_000;
        let mut seed = 1234u64;
        let make = |seed: &mut u64| -> Vec<f32> {
            (0..ir_len)
                .map(|i| {
                    let t = i as f32 / 48_000.0;
                    lcg(seed) * 0.02 * (-t / 1.5).exp()
                })
                .collect()
        };
        let irs = [make(&mut seed), make(&mut seed)];
        let mut c = Convolver::at_phase(SR, 0);
        assert!(c.set_ir(&irs, IrLayout::Stereo));

        let seconds = 10.0f64;
        let frames = (seconds * 48_000.0) as usize;
        let mut buf = noise(frames, 5);
        let before = c.footprint();
        let p = ConvolverParams::default();
        let start = std::time::Instant::now();
        let mut pos = 0;
        while pos < frames {
            let n = 480.min(frames - pos);
            c.process(&mut buf[pos * 2..], n, 2, &p);
            pos += n;
        }
        let elapsed = start.elapsed().as_secs_f64();
        assert_eq!(
            c.footprint(),
            before,
            "process grew a buffer somewhere - the allocation contract is broken"
        );
        assert!(
            buf.iter().all(|s| s.is_finite()),
            "a six-second IR produced a non-finite sample"
        );
        let factor = seconds / elapsed;
        println!("convolver: 6 s IR over 10 s of audio in {elapsed:.2} s = {factor:.1}x realtime");
        #[cfg(not(debug_assertions))]
        assert!(
            factor > 2.0,
            "only {factor:.1}x realtime in release - a 6 s IR must run comfortably"
        );
    }

    /// **The stagger never moves the latency.** Every instance starts its
    /// staging at a different offset so that the desk's IR bays do not all
    /// step on the same block; the whole argument that this is free is that
    /// the priming in `set_ir` cancels the offset exactly. This walks every
    /// phase a real instance can be handed and asserts the impulse comes
    /// out at the reported latency, to the sample, at each one.
    #[test]
    fn a_staggered_convolver_reports_the_same_latency_as_any_other() {
        let step = CONV_PARTITION / CONV_SPREAD;
        for i in 0..CONV_SPREAD {
            let phase = i * step;
            let mut c = Convolver::at_phase(SR, phase);
            let mut ir = vec![0.0f32; 8];
            ir[0] = 1.0;
            assert!(c.set_ir(&[ir.clone(), ir], IrLayout::Stereo));
            let lat = c.latency() as usize;
            // The first load fades up over two partitions, so the impulse
            // goes in after that rather than into the ramp.
            let at = 2 * CONV_PARTITION;
            let frames = 5 * CONV_PARTITION;
            let mut buf = vec![0.0f32; frames * 2];
            buf[at * 2] = 1.0;
            buf[at * 2 + 1] = 1.0;
            let p = ConvolverParams::default();
            // Chunked unevenly on purpose: the phase must survive a block
            // boundary landing anywhere inside the staging buffer.
            let mut pos = 0;
            while pos < frames {
                let n = 97.min(frames - pos);
                c.process(&mut buf[pos * 2..], n, 2, &p);
                pos += n;
            }
            let want = at + lat;
            for ch in 0..2 {
                assert!(
                    (buf[want * 2 + ch] - 1.0).abs() < 1e-3,
                    "phase {phase}: the impulse was not at the reported latency"
                );
            }
            let strays: Vec<usize> = (0..frames)
                .filter(|&i| i != want && buf[i * 2].abs() > 1e-3)
                .collect();
            assert!(strays.is_empty(), "phase {phase}: stray energy at {strays:?}");
        }
    }

    /// **A stop must not re-align the desk.** `clear` used to send every
    /// instance back to phase zero, which put them all back on the same
    /// block the moment the transport stopped, which is exactly when a
    /// session is most likely to be rebuilt from silence.
    #[test]
    fn a_stop_leaves_each_convolver_on_its_own_phase() {
        let phase = 3 * (CONV_PARTITION / CONV_SPREAD);
        let mut c = Convolver::at_phase(SR, phase);
        let mut ir = vec![0.0f32; 8];
        ir[0] = 1.0;
        assert!(c.set_ir(&[ir.clone(), ir], IrLayout::Stereo));
        let mut buf = noise(CONV_PARTITION * 3, 11);
        let p = ConvolverParams::default();
        c.process(&mut buf, CONV_PARTITION * 3, 2, &p);
        c.clear();
        let frames = 4 * CONV_PARTITION;
        let mut imp = vec![0.0f32; frames * 2];
        imp[0] = 1.0;
        imp[1] = 1.0;
        c.process(&mut imp, frames, 2, &p);
        let lat = c.latency() as usize;
        assert!(
            (imp[lat * 2] - 1.0).abs() < 1e-3,
            "after a stop the impulse moved off the reported latency"
        );
    }

    /// **Instances built in a row do not step in a row.**
    #[test]
    fn convolvers_built_together_do_not_all_step_on_the_same_block() {
        let built: Vec<Convolver> = (0..CONV_SPREAD).map(|_| Convolver::new(SR)).collect();
        let phases: std::collections::HashSet<usize> = built.iter().map(|c| c.phase).collect();
        assert_eq!(
            phases.len(),
            CONV_SPREAD,
            "{CONV_SPREAD} convolvers in a row landed on {} phases",
            phases.len()
        );
    }

    /// **The fixed cost of a step, which is what a short IR pays.**
    ///
    /// Not a test, a measurement: `cargo test --release ... -- --ignored
    /// --nocapture`. A cabinet or a plate is one or two partitions, so
    /// nearly all of its cost is the transforms, and sixteen such bays on a
    /// desk is an ordinary guitar session rather than a stress case.
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn the_fixed_cost_of_a_step() {
        for (name, ir_len) in [
            ("1 partition (a cabinet)", CONV_PARTITION),
            ("4 partitions (a room)", 4 * CONV_PARTITION),
            ("2 s (a hall)", 2 * 48_000),
        ] {
            let mut seed = 99u64;
            let make = |seed: &mut u64| -> Vec<f32> {
                (0..ir_len).map(|_| lcg(seed) * 0.02).collect()
            };
            let irs = [make(&mut seed), make(&mut seed)];
            let mut c = Convolver::at_phase(SR, 0);
            assert!(c.set_ir(&irs, IrLayout::Stereo));
            let steps = 2_000;
            let frames = steps * CONV_PARTITION;
            let mut buf = noise(frames, 7);
            let p = ConvolverParams::default();
            // Twice, reporting the second: the first row otherwise reads
            // the cost of warming this code path rather than running it.
            let mut per_step = 0.0;
            for _ in 0..2 {
                let start = std::time::Instant::now();
                let mut pos = 0;
                while pos < frames {
                    let n = CONV_PARTITION.min(frames - pos);
                    c.process(&mut buf[pos * 2..], n, 2, &p);
                    pos += n;
                }
                per_step = start.elapsed().as_secs_f64() / steps as f64 * 1e6;
            }
            println!("convolver {name}: {per_step:.1} us per step");
            assert!(buf.iter().all(|s| s.is_finite()));
        }
    }

    /// **An IR swap mid-stream never clicks**: through a steady sine, a
    /// swap between two tap IRs moves the level smoothly — no
    /// sample-to-sample jump beyond what the sine itself moves, within a
    /// −60 dB allowance — and the levels either side are each IR's own.
    #[test]
    fn an_ir_swap_mid_stream_never_clicks() {
        let frames = 19_200;
        let swap_at = 9_600;
        let amp = 0.5f32;
        let input = sine(1_000.0, amp, frames, SR);
        let mut buf = input.clone();
        let p = ConvolverParams::default();

        let mut c = Convolver::at_phase(SR, 0);
        assert!(c.set_ir(&[vec![1.0], vec![1.0]], IrLayout::Stereo));
        c.process(&mut buf[..swap_at * 2], swap_at, 2, &p);
        assert!(c.set_ir(&[vec![0.9], vec![0.9]], IrLayout::Stereo));
        c.process(&mut buf[swap_at * 2..], frames - swap_at, 2, &p);

        // The natural per-sample movement of the sine itself, plus a
        // −60 dB (re the signal) allowance for the splice.
        let natural = 2.0 * amp * (std::f32::consts::PI * 1_000.0 / SR).sin();
        let allowance = amp * 10.0f32.powf(-60.0 / 20.0);
        let left: Vec<f32> = buf.chunks_exact(2).map(|f| f[0]).collect();
        for f in 2_000..frames {
            let jump = (left[f] - left[f - 1]).abs();
            assert!(
                jump <= natural + allowance,
                "frame {f}: jump {jump} against a natural {natural} - the splice clicked"
            );
        }

        // Before the swap the wire's level; after it, 0.9 of it.
        let peak = |from: usize, to: usize| left[from..to].iter().fold(0.0f32, |m, &v| m.max(v.abs()));
        let pre = peak(4_000, 9_500);
        let post = peak(swap_at + 3 * 1024, frames);
        assert!((pre - amp).abs() < 0.01, "pre-swap level {pre}, wanted {amp}");
        assert!(
            (post - 0.9 * amp).abs() < 0.01,
            "post-swap level {post}, wanted {}",
            0.9 * amp
        );
    }

    /// **Mix at zero is a wire whatever is loaded**, trim at either
    /// extreme, IR loaded and warm — bit-exact, because "not doing
    /// anything" must survive a null test.
    #[test]
    fn mix_at_zero_is_a_wire_whatever_is_loaded() {
        for trim in [0.0f32, 0.5, 1.0] {
            let mut c = Convolver::at_phase(SR, 0);
            assert!(c.set_ir(&[noise(600, 3).into_iter().take(600).collect(), noise(600, 4).into_iter().take(600).collect()], IrLayout::Stereo));
            // Warm it up wet first, so the wire also proves the off-clear.
            let mut warm = noise(2_048, 8);
            c.process(&mut warm, 2_048, 2, &ConvolverParams { mix: 1.0, trim });
            let mut buf = noise(2_048, 9);
            let before = buf.clone();
            c.process(&mut buf, 2_048, 2, &ConvolverParams { mix: 0.0, trim });
            assert_eq!(buf, before, "mix at zero touched the signal (trim {trim})");
        }
    }

    /// **No setting the panel can reach may blow up.** Both dials at both
    /// extremes, a true-stereo IR loaded, against a signal well past full
    /// scale: the output stays finite and bounded.
    #[test]
    fn no_setting_the_panel_can_reach_makes_it_explode() {
        // A true-stereo set scaled so each IR's tap-sum is at most one:
        // the bound below is then arithmetic, not hope.
        let mut seed = 77u64;
        let irs: Vec<Vec<f32>> = (0..4)
            .map(|_| {
                let raw: Vec<f32> = (0..1_500).map(|_| lcg(&mut seed)).collect();
                let sum: f32 = raw.iter().map(|v| v.abs()).sum();
                raw.into_iter().map(|v| v / sum).collect()
            })
            .collect();
        let frames = 4_096;
        for bits in 0..4u32 {
            let d = |bit: u32| [0.0f32, 1.0][(bits >> bit & 1) as usize];
            let p = ConvolverParams { mix: d(0).max(0.01), trim: d(1) };
            let mut c = Convolver::at_phase(SR, 0);
            assert!(c.set_ir(&irs, IrLayout::TrueStereo));
            for _ in 0..2 {
                let mut buf: Vec<f32> = noise(frames, 11).iter().map(|v| v * 4.0).collect();
                c.process(&mut buf, frames, 2, &p);
                assert!(
                    buf.iter().all(|s| s.is_finite()),
                    "dials {bits:02b} produced a non-finite sample"
                );
                // 2× full scale in, two unit-sum IRs summed, +24 dB trim:
                // 2 · 2 · 15.9 ≈ 64 is the ceiling a runaway would pass.
                let peak = buf.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
                assert!(peak < 64.0, "dials {bits:02b} reached {peak}");
            }
        }
    }

    /// Mono and odd channel counts: fewer than two channels is left alone,
    /// and channels past the first two are never written.
    #[test]
    fn mono_is_left_alone_and_a_third_channel_is_never_touched() {
        let mut c = Convolver::at_phase(SR, 0);
        assert!(c.set_ir(&[vec![1.0], vec![1.0]], IrLayout::Stereo));
        let mut mono: Vec<f32> = (0..256).map(|i| (i as f32 * 0.02).sin()).collect();
        let before = mono.clone();
        c.process(&mut mono, 256, 1, &ConvolverParams::default());
        assert_eq!(mono, before, "a mono buffer was written to");

        let frames = 2_048;
        let mut wide = vec![0.0f32; frames * 3];
        for (i, frame) in wide.chunks_exact_mut(3).enumerate() {
            frame[0] = (i as f32 * 0.01).sin();
            frame[1] = (i as f32 * 0.013).sin();
            frame[2] = 0.777; // The sentinel a stride bug would smear.
        }
        c.process(&mut wide, frames, 3, &ConvolverParams::default());
        assert!(
            wide.chunks_exact(3).all(|f| f[2] == 0.777),
            "the third channel was written to"
        );
    }

    /// **Clear forgets the audio but keeps the IR.** After a clear,
    /// silence in is silence out — no tail survives — and an impulse in
    /// finds the same IR still loaded, at the same latency.
    #[test]
    fn clear_forgets_the_audio_but_keeps_the_ir() {
        let mut ir = vec![0.0f32; 800];
        ir[0] = 0.8;
        ir[700] = -0.5; // A tap in the second partition, so tails cross steps.
        let irs = [ir.clone(), ir.clone()];
        let mut c = Convolver::at_phase(SR, 0);
        assert!(c.set_ir(&irs, IrLayout::Stereo));
        let p = ConvolverParams::default();
        let mut warm = noise(4_096, 21);
        c.process(&mut warm, 4_096, 2, &p);

        c.clear();
        let frames = 4_096;
        let mut silence = vec![0.0f32; frames * 2];
        c.process(&mut silence, frames, 2, &p);
        assert!(
            silence.iter().all(|&v| v == 0.0),
            "a tail from before the clear is still ringing"
        );

        let mut click = vec![0.0f32; frames * 2];
        click[0] = 1.0;
        click[1] = 1.0;
        c.process(&mut click, frames, 2, &p);
        assert!(
            (click[512 * 2] - 0.8).abs() < 1.0e-4,
            "the direct tap moved or vanished: {}",
            click[512 * 2]
        );
        assert!(
            (click[(512 + 700) * 2] - (-0.5)).abs() < 1.0e-4,
            "the second-partition tap moved or vanished: {}",
            click[(512 + 700) * 2]
        );
    }
}
