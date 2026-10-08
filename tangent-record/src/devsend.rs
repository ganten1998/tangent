//! The device send: Tangent's master out, written to a SECOND output device
//! while the first one plays it — so a virtual cable (VB-Audio's "CABLE
//! Input" on Windows, BlackHole or Loopback on a Mac) carries the mix to OBS,
//! Discord or anything else that can record an input.
//!
//! The same shape as [`crate::reastream`], and only the end of the road
//! differs. The engine copies the stereo master, post-varispeed and at the
//! ENGINE's device rate, into a ring; this drains the ring into a cpal output
//! stream on the device the user picked. The ReaStream send needs the receiver
//! to host a VST2; a cable needs nothing but an input picker, which every
//! streaming and chat app already has.
//!
//! **Why this is more than a copy loop: two devices, two clocks.** The engine's
//! output runs on the interface's crystal, the cable on whatever clock its
//! driver keeps, and no two crystals agree. A plain copy fills the ring or
//! empties it at the difference between them — 100 ppm is 4.8 frames a second
//! at 48 kHz, an underrun or an overrun every few minutes, for ever. So the
//! read side is a RESAMPLER whose ratio a small controller trims, a few hundred
//! ppm either way, to hold the ring's fill at a target. The nominal ratio is
//! `engine rate / device rate`, so a 44.1 kHz interface feeding a 48 kHz cable
//! is the same arithmetic with a bigger number in it (and the dialog says the
//! rates differ, because a big-ratio conversion is a lower-quality road than a
//! trim).
//!
//! **Why WASAPI, by name, on Windows.** The engine may be on ASIO, and ASIO's
//! SDK keeps ONE loaded driver per process behind a global guard (see
//! `vendor/asio-sys`). Anything here that went through `audio::with_host`,
//! `audio::output_device_names` or the engine's `resolve_output` would follow
//! the CHOSEN system — and under ASIO that loads drivers, which is the
//! teardown `RatePoll` was doing on 2026-09-05. So [`send_host`] asks cpal for
//! the WASAPI host directly, a unit struct with no state and no driver behind
//! it, and nothing in this file can reach the ASIO road. Elsewhere it is the
//! platform default host, which is what the cable lives on anyway.
//!
//! **What the audio threads are allowed to do.** The ENGINE's callback only
//! ever does `push_entire_slice` behind an on-flag, and a full ring DROPS that
//! block: a stalled cable can cost the send audio, never the interface. The
//! SINK's callback runs on cpal's thread for the cable and only ever
//! `try_lock`s the puller (the UI thread locks it only with no stream running,
//! so the try never fails in practice and never waits when it does), pops
//! samples and does arithmetic. Every buffer it touches is sized before the
//! stream is built.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The floor on the ring's target fill: 20 ms, whatever the two blocks are.
/// Under that, a scheduler hiccup on either side is an underrun; over it is
/// latency OBS has to be told about, and 20 ms is less than a video frame.
pub const TARGET_FLOOR_S: f64 = 0.020;

/// **The most the controller may move the ratio off nominal**, either way:
/// 500 ppm. The plan said "of order 200 ppm", which is what two decent
/// crystals disagree by; it is 500 because a USB interface on its own PLL
/// against a virtual cable on the system clock has been seen past 200, and
/// a trim that saturates below the real drift is an underrun every few
/// minutes. 500 ppm is under a cent of pitch — nobody hears it, and the
/// controller only goes there when the clocks do.
pub const MAX_TRIM: f64 = 500e-6;

/// Proportional gain, per second of fill error. With the fill error in
/// SECONDS the loop's crossover is this, in rad/s, whatever the target —
/// a target normalised to its own size moved the crossover with it and lost
/// phase margin at large blocks.
const KP: f64 = 0.2;
/// Integral time: the integral term takes the steady drift off the
/// proportional one, so the fill settles ON the target rather than a drift's
/// worth off it. Ten seconds keeps the PI zero (0.1 rad/s) under the
/// crossover, about 50 degrees of margin with the filter below.
const TI_S: f64 = 10.0;
/// The fill is measured at each sink callback, and it saw-tooths by a block
/// on each side as the two cadences beat. One second of low-pass takes that
/// out before it reaches the ratio (a block-rate wobble in the ratio is a
/// vibrato).
const FILTER_S: f64 = 1.0;
/// How long the loop watches the fill before adopting a setpoint; see
/// [`Pull::setpoint`]. Two seconds is dozens of engine blocks at any buffer
/// a session runs, so the mean is the saw-tooth's and not a phase of it.
const WATCH_S: f64 = 2.0;

/// The largest sink callback rendered in one pass; a larger one is done in
/// several. Matches the engine's `MAX_CALLBACK_FRAMES`.
const MAX_SINK_FRAMES: usize = 8192;

/// The engine-side ring, in f32 samples (interleaved stereo): 32768 frames,
/// 680 ms at 48 kHz. Bigger than the ReaStream ring because the target here
/// grows with the engine's block (see [`Pull::target`]), and a 4096-frame ASIO
/// buffer must still leave the overrun ceiling under the ring's capacity.
pub const RING_SAMPLES: usize = 1 << 16;

/// Four-point, third-order Hermite — **the same interpolator as
/// `tangent/src/varispeed.rs`'s `hermite`, copied rather than shared.** That
/// one is private to the `tangent` binary, which depends on this crate; the
/// other direction is a cycle. Same coefficients, so the two resamplers sound
/// the same; change one, change both.
#[inline]
fn hermite(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c0 = y1;
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + c0
}

/// **One stereo frame onto a device's channels.** Mono gets the average —
/// not the left, which would lose a hard-right part — and a device wider than
/// two gets L and R in its first pair with the rest silent: a "CABLE Input
/// 16ch" is a cable, not a surround bus.
pub fn spread(l: f32, r: f32, out: &mut [f32]) {
    match out.len() {
        0 => {}
        1 => out[0] = 0.5 * (l + r),
        _ => {
            out[0] = l;
            out[1] = r;
            for s in &mut out[2..] {
                *s = 0.0;
            }
        }
    }
}

/// **The clock-drift controller**: a PI loop on the ring's low-passed fill,
/// answering a ratio trim.
///
/// Pure arithmetic and no ring, so the loop can be driven by a simulated pair
/// of clocks in a test. The plant is an integrator — the fill moves at the
/// difference between what arrives and what the trimmed ratio takes — so a
/// proportional term alone would park the fill a drift's worth off target
/// (300 ppm over a gain of 0.2/s is 1.5 ms); the integral term takes that.
#[derive(Debug, Clone)]
pub struct Drift {
    filtered: f64,
    integral: f64,
    trim: f64,
}

impl Drift {
    /// A controller holding `fill` frames, no trim learned yet.
    pub fn new(fill: f64) -> Self {
        Self {
            filtered: fill,
            integral: 0.0,
            trim: 0.0,
        }
    }

    /// Restart the filter at `fill` — after a re-prime or an overrun drop,
    /// where the fill jumped and the jump is not drift. **The integral is
    /// kept**: what it learned is the clocks' disagreement, which a glitch
    /// did not change.
    pub fn refill(&mut self, fill: f64) {
        self.filtered = fill;
    }

    /// One look: `fill` and `target` in source frames, `rate` the source
    /// rate, `dt` the seconds since the last look. Answers the trim, a
    /// relative ratio offset clamped to [`MAX_TRIM`]. Positive = the ring is
    /// too full, so read faster.
    pub fn step(&mut self, fill: f64, target: f64, rate: f64, dt: f64) -> f64 {
        let a = (dt / FILTER_S).min(1.0);
        self.filtered += (fill - self.filtered) * a;
        let err = (self.filtered - target) / rate.max(1.0);
        // Clamped as it integrates — the anti-windup a saturating output
        // needs, or a long underrun would wind it past the limit and
        // overshoot for as long again once the audio came back.
        self.integral = (self.integral + err * dt * (KP / TI_S)).clamp(-MAX_TRIM, MAX_TRIM);
        self.trim = (KP * err + self.integral).clamp(-MAX_TRIM, MAX_TRIM);
        self.trim
    }

    /// The trim while the loop is open: what the integral has learned of
    /// the clocks, and no proportional term (there is no error to act on
    /// yet).
    pub fn hold(&mut self) -> f64 {
        self.trim = self.integral;
        self.trim
    }

    /// The trim the last look answered.
    pub fn trim(&self) -> f64 {
        self.trim
    }

    /// The low-passed fill, in source frames.
    pub fn filtered(&self) -> f64 {
        self.filtered
    }
}

/// **The read side of the ring**: the consumer, the resampler and the drift
/// controller, everything one sink callback needs.
///
/// Owned through a `Mutex` by [`DeviceSend`] rather than moved into a stream's
/// callback because the engine's tap is ONCE per engine (`Option::take`): a
/// device change or a re-enable builds a new stream over the SAME consumer,
/// and a consumer moved into a dropped closure would be gone with it.
pub struct Pull {
    frames: rtrb::Consumer<f32>,
    src_rate: f64,
    dst_rate: f64,
    /// `src_rate / dst_rate`, the ratio with no trim.
    nominal: f64,
    /// The engine's block, in source frames, when it said (see [`Pull::target`]).
    src_block: usize,
    /// The largest sink callback seen, in device frames. Not known until the
    /// first callback; WASAPI picks its own period.
    period: usize,
    /// `[x(n-1), x(n), x(n+1), x(n+2)]` with the read at `n + frac`.
    hist: [[f32; 2]; 4],
    frac: f64,
    /// Silence until the ring holds the target — at the start and after an
    /// underrun — so a resume is a clean edge and never a stale sample
    /// repeated.
    priming: bool,
    /// Seconds of output left to WATCH before the loop closes, after a
    /// prime or a cut; see [`Pull::setpoint`].
    watch: f64,
    watch_sum: f64,
    watch_n: u32,
    /// The adopted setpoint's distance from [`Pull::target`].
    offset: f64,
    drift: Drift,
    underruns: u32,
    overruns: u32,
}

impl Pull {
    /// The read side over `frames` (interleaved stereo at `src_rate`),
    /// writing to a device at `dst_rate`. `src_block` is the engine's buffer
    /// when it named one.
    pub fn new(frames: rtrb::Consumer<f32>, src_rate: u32, dst_rate: u32, src_block: usize) -> Self {
        let mut p = Self {
            frames,
            src_rate: 0.0,
            dst_rate: 0.0,
            nominal: 1.0,
            src_block,
            period: 0,
            hist: [[0.0; 2]; 4],
            frac: 0.0,
            priming: true,
            watch: 0.0,
            watch_sum: 0.0,
            watch_n: 0,
            offset: 0.0,
            drift: Drift::new(0.0),
            underruns: 0,
            overruns: 0,
        };
        p.restart(src_rate, dst_rate);
        p
    }

    /// Start over for a device at `dst_rate`: whatever the ring holds is
    /// thrown away (it was queued for a stream that is gone, or while the
    /// send was off), the controller forgets its trim, and the output primes.
    pub fn restart(&mut self, src_rate: u32, dst_rate: u32) {
        self.src_rate = f64::from(src_rate.max(1));
        self.dst_rate = f64::from(dst_rate.max(1));
        self.nominal = self.src_rate / self.dst_rate;
        self.period = 0;
        self.discard(self.frames.slots() / 2);
        self.hist = [[0.0; 2]; 4];
        self.frac = 0.0;
        self.priming = true;
        self.offset = 0.0;
        self.watch = 0.0;
        self.drift = Drift::new(self.target());
        self.underruns = 0;
        self.overruns = 0;
    }

    /// **The fill the controller holds, in source frames**: the larger of
    /// [`TARGET_FLOOR_S`] and twice the larger of the two blocks.
    ///
    /// Twice the BLOCK and not only twice the sink's period, which is what
    /// the plan said: the engine delivers in bursts of its own buffer, so
    /// the fill a callback sees saw-tooths by a whole engine block, and a
    /// 2048-frame ASIO buffer against a 20 ms target would underrun at every
    /// beat of the two cadences. Holding twice the larger block keeps the
    /// trough of that saw-tooth a whole sink period above empty.
    pub fn target(&self) -> f64 {
        let sink_in_src = self.period as f64 * self.nominal;
        let block = (self.src_block as f64).max(sink_in_src);
        let cap = self.capacity_frames() as f64;
        (TARGET_FLOOR_S * self.src_rate)
            .max(2.0 * block)
            .min(cap / 3.0)
    }

    /// **The fill the loop actually holds**: [`Pull::target`], moved by
    /// what the first seconds after a prime measured.
    ///
    /// The ring is primed to the target and then saw-tooths by an engine
    /// block, so the mean fill it settles into is the target LESS about half
    /// a block, or more, by where in the engine's cadence the prime landed.
    /// Closing the loop on the bare target turns that half block into an
    /// error the trim must work off at its bound: 1000 frames at 500 ppm of
    /// 44.1 kHz is most of a minute of a send held a cent flat. So the loop
    /// first WATCHES (`WATCH_S`, trim held at what it has learned), adopts
    /// the mean it saw, clamped to half a target either way — still a whole
    /// block above empty and well under the ceiling — and holds THAT.
    pub fn setpoint(&self) -> f64 {
        self.target() + self.offset
    }

    fn begin_watch(&mut self) {
        self.watch = WATCH_S;
        self.watch_sum = 0.0;
        self.watch_n = 0;
    }

    /// Above this the ring is dropped back to the target in one cut: the
    /// sink stalled (a debugger, a suspended cable) and trimming off a
    /// second of backlog at 500 ppm would take half an hour.
    fn ceiling(&self) -> f64 {
        let cap = self.capacity_frames() as f64;
        (3.0 * self.target()).min(cap - self.src_block as f64)
    }

    fn capacity_frames(&self) -> usize {
        self.frames.buffer().capacity() / 2
    }

    fn discard(&mut self, frames: usize) {
        let n = (frames * 2).min(self.frames.slots());
        if let Ok(chunk) = self.frames.read_chunk(n) {
            chunk.commit_all();
        }
    }

    /// Frames in the ring right now.
    pub fn fill(&self) -> usize {
        self.frames.slots() / 2
    }

    /// The trim the controller is applying.
    pub fn trim(&self) -> f64 {
        self.drift.trim()
    }

    /// The low-passed fill the controller sees.
    pub fn filtered_fill(&self) -> f64 {
        self.drift.filtered()
    }

    /// Underruns since the last restart (each one a re-prime).
    pub fn underruns(&self) -> u32 {
        self.underruns
    }

    /// Overrun cuts since the last restart.
    pub fn overruns(&self) -> u32 {
        self.overruns
    }

    /// Whether the output is waiting for the ring to reach the target.
    pub fn priming(&self) -> bool {
        self.priming
    }

    /// **One sink callback's worth**: `out` is `dev_ch`-interleaved device
    /// frames. Pops what it reads, allocates nothing.
    pub fn render(&mut self, out: &mut [f32], dev_ch: usize) {
        let dev_ch = dev_ch.max(1);
        let frames = out.len() / dev_ch;
        if frames == 0 {
            out.fill(0.0);
            return;
        }
        self.period = self.period.max(frames);
        let target = self.target();
        let fill = self.fill();
        if self.priming {
            if (fill as f64) < target {
                out.fill(0.0);
                return;
            }
            // Resume from zeros: the first frames ramp in through the
            // interpolator's history rather than repeating whatever it held
            // when it ran dry.
            self.priming = false;
            self.hist = [[0.0; 2]; 4];
            self.frac = 0.0;
            self.begin_watch();
        } else if fill as f64 > self.ceiling() {
            let cut = fill.saturating_sub(target as usize);
            self.discard(cut);
            self.overruns = self.overruns.saturating_add(1);
            self.begin_watch();
        }
        let dt = frames as f64 / self.dst_rate;
        let fill = self.fill() as f64;
        let trim = if self.watch > 0.0 {
            self.watch_sum += fill;
            self.watch_n += 1;
            self.watch -= dt;
            if self.watch <= 0.0 {
                let mean = self.watch_sum / f64::from(self.watch_n.max(1));
                self.offset = (mean - target).clamp(-0.5 * target, 0.5 * target);
                self.drift.refill(mean);
            }
            self.drift.hold()
        } else {
            self.drift.step(fill, target + self.offset, self.src_rate, dt)
        };
        let ratio = self.nominal * (1.0 + trim);
        // What this callback will pop, plus one for rounding in the phase
        // sum. Checked BEFORE reading anything: a callback is either whole or
        // silent, never audio with a hole in it.
        let needed = (self.frac + frames as f64 * ratio).floor() as usize + 1;
        if self.fill() < needed {
            out.fill(0.0);
            self.priming = true;
            self.underruns = self.underruns.saturating_add(1);
            return;
        }
        for frame in out.chunks_exact_mut(dev_ch) {
            let t = self.frac as f32;
            let h = &self.hist;
            let l = hermite(h[0][0], h[1][0], h[2][0], h[3][0], t);
            let r = hermite(h[0][1], h[1][1], h[2][1], h[3][1], t);
            spread(l, r, frame);
            self.frac += ratio;
            while self.frac >= 1.0 {
                self.frac -= 1.0;
                self.hist.copy_within(1.., 0);
                let l = self.frames.pop().unwrap_or(0.0);
                let r = self.frames.pop().unwrap_or(0.0);
                self.hist[3] = [l, r];
            }
        }
        // A trailing partial frame, silenced.
        let whole = frames * dev_ch;
        out[whole..].fill(0.0);
    }
}

/// **What a running send reports**, refreshed by the sink's callback and read
/// by the UI thread without a lock.
#[derive(Default)]
struct Live {
    /// Set by the stream's error callback on a device that went away.
    gone: AtomicBool,
    /// Callbacks served, so "sending" means the cable is pulling.
    callbacks: AtomicU64,
    fill: AtomicU32,
    trim_ppm: AtomicI32,
    underruns: AtomicU32,
    overruns: AtomicU32,
    /// A stream error that is not an xrun, in words; `try_lock` only.
    fault: Mutex<Option<String>>,
}

/// A snapshot of a running send, for the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendStats {
    /// The device as cpal named it.
    pub device: String,
    /// The engine's rate, which the ring carries.
    pub src_rate: u32,
    /// The device's own rate, which the stream was opened at.
    pub dst_rate: u32,
    /// The device's channel count.
    pub channels: u16,
    /// Callbacks the device has asked for.
    pub callbacks: u64,
    /// Frames in the ring at the last callback.
    pub fill: u32,
    /// The drift trim, in ppm.
    pub trim_ppm: i32,
    /// Underruns since the stream opened.
    pub underruns: u32,
    /// Overrun cuts since the stream opened.
    pub overruns: u32,
}

/// Why [`DeviceSend::open`] did not open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// No output of the send's host goes by that name.
    Missing(String),
    /// It is there and would not play; the backend's reason.
    Failed(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Missing(name) => write!(f, "{name} is not connected"),
            OpenError::Failed(why) => f.write_str(why),
        }
    }
}

struct Running {
    /// Held, never touched: dropping it stops the device. `!Send`, which is
    /// why the whole `DeviceSend` lives on the UI thread beside the engine.
    _stream: cpal::Stream,
    live: Arc<Live>,
    device: String,
    dst_rate: u32,
    channels: u16,
}

/// **The send's owner**, one per engine: the read side of the engine's tap,
/// and the stream draining it when one is open.
///
/// Opening, closing and re-targeting are the UI thread's, at the rate the
/// user changes the setting; the stream's callback is the only thing that
/// runs at audio rate.
pub struct DeviceSend {
    pull: Arc<Mutex<Pull>>,
    src_rate: u32,
    running: Option<Running>,
}

impl DeviceSend {
    /// The send over the engine's tap `frames` (interleaved stereo at
    /// `src_rate`), no device open yet. `src_block` is the engine's buffer
    /// when it named one.
    pub fn new(frames: rtrb::Consumer<f32>, src_rate: u32, src_block: Option<u32>) -> Self {
        let block = src_block.unwrap_or(0) as usize;
        Self {
            pull: Arc::new(Mutex::new(Pull::new(frames, src_rate, src_rate, block))),
            src_rate,
            running: None,
        }
    }

    /// Stop the stream, if one is running. The tap is kept for the next open.
    pub fn close(&mut self) {
        self.running = None;
    }

    /// Whether a stream is open and has not reported its device gone.
    pub fn is_open(&self) -> bool {
        self.running.is_some() && self.fault().is_none()
    }

    /// The open stream's failure, if it has had one: its device gone, or a
    /// backend error that is not an xrun.
    pub fn fault(&self) -> Option<String> {
        let run = self.running.as_ref()?;
        if run.live.gone.load(Ordering::Relaxed) {
            return Some(format!("{} went away", run.device));
        }
        run.live.fault.try_lock().ok().and_then(|g| g.clone())
    }

    /// What the open stream is doing, or `None` with none open.
    pub fn stats(&self) -> Option<SendStats> {
        let run = self.running.as_ref()?;
        let l = &run.live;
        Some(SendStats {
            device: run.device.clone(),
            src_rate: self.src_rate,
            dst_rate: run.dst_rate,
            channels: run.channels,
            callbacks: l.callbacks.load(Ordering::Relaxed),
            fill: l.fill.load(Ordering::Relaxed),
            trim_ppm: l.trim_ppm.load(Ordering::Relaxed),
            underruns: l.underruns.load(Ordering::Relaxed),
            overruns: l.overruns.load(Ordering::Relaxed),
        })
    }

    /// **Open the device named `want`** on the send's host and start
    /// draining into it, closing whatever was open first.
    ///
    /// Opened at the device's OWN config (`default_output_config`): WASAPI
    /// shared mode refuses any rate but the mix format's, since cpal asks it
    /// with no auto-convert flag, so the rate conversion is all ours.
    pub fn open(&mut self, want: &str) -> Result<(), OpenError> {
        self.close();
        let host = send_host();
        let device = find_output(&host, want).ok_or_else(|| OpenError::Missing(want.to_owned()))?;
        let name = device.name().unwrap_or_else(|_| want.to_owned());
        let supported = device
            .default_output_config()
            .map_err(|e| OpenError::Failed(e.to_string()))?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.config();
        let dst_rate = config.sample_rate.0;
        let channels = config.channels;
        if let Ok(mut p) = self.pull.lock() {
            p.restart(self.src_rate, dst_rate);
        }
        let live = Arc::new(Live::default());
        let stream = build_stream(&device, &config, format, Arc::clone(&self.pull), Arc::clone(&live))
            .map_err(OpenError::Failed)?;
        stream
            .play()
            .map_err(|e| OpenError::Failed(e.to_string()))?;
        self.running = Some(Running {
            _stream: stream,
            live,
            device: name,
            dst_rate,
            channels,
        });
        Ok(())
    }
}

/// **The host the send opens on**: WASAPI on Windows, by id, so an ASIO
/// build never routes a send through the driver guard; the platform default
/// everywhere else. See the module doc.
pub fn send_host() -> cpal::Host {
    #[cfg(windows)]
    {
        cpal::host_from_id(cpal::HostId::Wasapi).unwrap_or_else(|_| cpal::default_host())
    }
    #[cfg(not(windows))]
    {
        cpal::default_host()
    }
}

/// **Every output the send can open**, by name, in the host's order. For the
/// dialog, at open; never per frame (WASAPI activates a client per endpoint
/// to answer, and on macOS cpal builds an AUHAL per device).
pub fn output_names() -> Vec<String> {
    send_host()
        .output_devices()
        .map(|ds| {
            ds.map(|d| d.name().unwrap_or_else(|_| crate::audio::UNNAMED_DEVICE.to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

/// The index of `want` in `names`: the name exactly, then as a case-blind
/// substring. **The same rule as `tangent_ui::ports::match_output_name`**,
/// restated because this crate cannot see `tangent-ui`, so the row the
/// dialog lights and the device this opens agree.
pub fn match_name(names: &[String], want: &str) -> Option<usize> {
    if let Some(i) = names.iter().position(|n| n == want) {
        return Some(i);
    }
    let lower = want.to_lowercase();
    names.iter().position(|n| n.to_lowercase().contains(&lower))
}

/// The output `want` names, in ONE walk of the host's outputs: listing them
/// is the expensive part (an audio client per WASAPI endpoint, an AUHAL per
/// CoreAudio device), so the names and the devices are kept together rather
/// than listed again to fetch the one that matched.
fn find_output(host: &cpal::Host, want: &str) -> Option<cpal::Device> {
    if want.trim().is_empty() {
        return None;
    }
    let mut found: Vec<(String, cpal::Device)> = host
        .output_devices()
        .ok()?
        .map(|d| (d.name().unwrap_or_default(), d))
        .collect();
    let names: Vec<String> = found.iter().map(|(n, _)| n.clone()).collect();
    let i = match_name(&names, want)?;
    Some(found.swap_remove(i).1)
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: cpal::SampleFormat,
    pull: Arc<Mutex<Pull>>,
    live: Arc<Live>,
) -> Result<cpal::Stream, String> {
    macro_rules! open {
        ($t:ty) => {
            build_for::<$t>(device, config, pull, live)
        };
    }
    match format {
        cpal::SampleFormat::F32 => open!(f32),
        cpal::SampleFormat::F64 => open!(f64),
        cpal::SampleFormat::I32 => open!(i32),
        cpal::SampleFormat::I24 => open!(cpal::I24),
        cpal::SampleFormat::I16 => open!(i16),
        cpal::SampleFormat::I64 => open!(i64),
        cpal::SampleFormat::U16 => open!(u16),
        cpal::SampleFormat::I8 => open!(i8),
        cpal::SampleFormat::U8 => open!(u8),
        other => Err(format!("the device speaks {other}, which is not supported")),
    }
}

fn build_for<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    pull: Arc<Mutex<Pull>>,
    live: Arc<Live>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32> + Send + 'static,
{
    let dev_ch = (config.channels as usize).max(1);
    // Sized here, on the UI thread. Nothing after this allocates.
    let mut scratch = vec![0.0f32; MAX_SINK_FRAMES * dev_ch];
    let fed = Arc::clone(&live);
    device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let Ok(mut p) = pull.try_lock() else {
                    // The UI thread is between streams. Never wait.
                    for d in data.iter_mut() {
                        *d = T::from_sample(0.0f32);
                    }
                    return;
                };
                let mut done = 0usize;
                while done + dev_ch <= data.len() {
                    let frames = ((data.len() - done) / dev_ch).min(MAX_SINK_FRAMES);
                    let n = frames * dev_ch;
                    p.render(&mut scratch[..n], dev_ch);
                    for (d, s) in data[done..done + n].iter_mut().zip(&scratch[..n]) {
                        *d = T::from_sample(*s);
                    }
                    done += n;
                }
                for d in data[done..].iter_mut() {
                    *d = T::from_sample(0.0f32);
                }
                fed.callbacks.fetch_add(1, Ordering::Relaxed);
                fed.fill.store(p.fill() as u32, Ordering::Relaxed);
                fed.trim_ppm
                    .store((p.trim() * 1e6).round() as i32, Ordering::Relaxed);
                fed.underruns.store(p.underruns(), Ordering::Relaxed);
                fed.overruns.store(p.overruns(), Ordering::Relaxed);
            },
            move |err| {
                // Never panic, never wait: this is the cable's thread.
                if matches!(err, cpal::StreamError::DeviceNotAvailable) {
                    live.gone.store(true, Ordering::Relaxed);
                    return;
                }
                if matches!(&err, cpal::StreamError::BackendSpecific { err: inner }
                    if inner.description.starts_with("xrun"))
                {
                    return;
                }
                if let Ok(mut slot) = live.fault.try_lock() {
                    *slot = Some(err.to_string());
                }
            },
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Two clocks, simulated**: an engine pushing `src_block`-frame blocks
    /// at `src_rate × (1 + src_ppm)` against a sink pulling
    /// `dst_block`-frame callbacks at `dst_rate × (1 + dst_ppm)`, through the
    /// real ring and the real [`Pull`], for `seconds`. Returns the trace the
    /// assertions read: (time, fill, trim) per sink callback, and the pull.
    struct Sim {
        trace: Vec<(f64, f64, f64, u32)>,
        pull: Pull,
        engine_drops: u32,
    }

    #[allow(clippy::too_many_arguments)]
    fn simulate(
        src_rate: u32,
        dst_rate: u32,
        src_ppm: f64,
        dst_ppm: f64,
        src_block: usize,
        dst_block: usize,
        seconds: f64,
    ) -> Sim {
        let (mut tx, rx) = rtrb::RingBuffer::<f32>::new(RING_SAMPLES);
        let mut pull = Pull::new(rx, src_rate, dst_rate, src_block);
        let src_dt = src_block as f64 / (f64::from(src_rate) * (1.0 + src_ppm * 1e-6));
        let dst_dt = dst_block as f64 / (f64::from(dst_rate) * (1.0 + dst_ppm * 1e-6));
        let block: Vec<f32> = vec![0.25; src_block * 2];
        let mut out = vec![0.0f32; dst_block * 2];
        let (mut tp, mut tc) = (0.0f64, dst_dt * 0.37);
        let mut trace = Vec::new();
        let mut engine_drops = 0u32;
        while tc < seconds {
            if tp <= tc {
                if tx.push_entire_slice(&block).is_err() {
                    engine_drops += 1;
                }
                tp += src_dt;
            } else {
                // The fill as the controller sees it: at the callback,
                // before it reads.
                let fill = pull.fill() as f64;
                pull.render(&mut out, 2);
                trace.push((tc, fill, pull.trim(), pull.underruns()));
                tc += dst_dt;
            }
        }
        Sim {
            trace,
            pull,
            engine_drops,
        }
    }

    /// The rule the controller exists for, against every pairing it must
    /// survive: after 60 s of settling the mean fill sits within half a
    /// millisecond of the setpoint (a proportional-only loop parks 1.5 ms
    /// off it at 300 ppm) and the trim inside its bound, and across the next
    /// minute nothing under- or overruns. Mutating the integral term (TI_S
    /// huge), the sign of the trim, or the clamp turns this red.
    #[test]
    fn the_ring_settles_on_its_target_against_two_drifting_clocks() {
        let cases = [
            // (src, dst, src ppm, dst ppm, src block, dst block)
            (48_000, 48_000, 300.0, 0.0, 512, 480),
            (48_000, 48_000, -300.0, 0.0, 512, 480),
            (48_000, 48_000, 0.0, 300.0, 256, 441),
            (44_100, 48_000, 150.0, -150.0, 512, 480),
            (44_100, 48_000, -300.0, 0.0, 2048, 480),
            (48_000, 44_100, 0.0, -300.0, 64, 1024),
        ];
        for (src, dst, sp, dp, sb, db) in cases {
            let sim = simulate(src, dst, sp, dp, sb, db, 120.0);
            let p = &sim.pull;
            let target = p.setpoint();
            let bare = p.target();
            assert!(
                (target - bare).abs() <= 0.5 * bare,
                "{src}->{dst}: setpoint {target:.0} strayed from target {bare:.0}"
            );
            let rate = f64::from(src);
            let settled: Vec<_> = sim.trace.iter().filter(|s| s.0 >= 60.0).collect();
            assert!(!settled.is_empty());
            let mean_fill = settled.iter().map(|s| s.1).sum::<f64>() / settled.len() as f64;
            assert!(
                (mean_fill - target).abs() / rate < 0.0005,
                "{src}->{dst} {sp}/{dp} ppm: mean fill {mean_fill:.0} vs target {target:.0}"
            );
            for (t, f, trim, _) in &settled {
                assert!(trim.abs() <= MAX_TRIM + 1e-12, "trim {trim} out of bounds at {t}");
                assert!(*f < p.ceiling(), "the ring reached its ceiling at {t}");
            }
            // The one underrun allowed is the start-up prime (the first
            // callback can find the ring short before the first block);
            // none once settled.
            let at_60 = settled[0].3;
            assert!(at_60 <= 1, "{src}->{dst}: {at_60} underruns while settling");
            assert_eq!(p.underruns(), at_60, "{src}->{dst}: underran after settling");
            assert_eq!(p.overruns(), 0, "{src}->{dst}: an overrun cut");
            assert_eq!(sim.engine_drops, 0, "{src}->{dst}: the engine's ring filled");
            // And the trim found the drift: within 50 ppm of what the two
            // clocks actually disagree by.
            let want = (1.0 + sp * 1e-6) / (1.0 + dp * 1e-6) - 1.0;
            let mean_trim =
                settled.iter().map(|s| s.2).sum::<f64>() / settled.len() as f64;
            assert!(
                (mean_trim - want).abs() < 50e-6,
                "{src}->{dst}: trim {:.0} ppm for a drift of {:.0}",
                mean_trim * 1e6,
                want * 1e6
            );
        }
    }

    #[test]
    fn a_drift_past_the_bound_saturates_and_never_runs_away() {
        // 2000 ppm is four times what the trim may do: the fill must drift,
        // the trim must sit at its bound and no further, and the overrun cut
        // must catch it rather than the engine's ring filling.
        let sim = simulate(48_000, 48_000, 2000.0, 0.0, 512, 480, 120.0);
        let last = *sim.trace.last().unwrap();
        assert!((last.2 - MAX_TRIM).abs() < 1e-12, "trim {} not at the bound", last.2);
        assert!(sim.pull.overruns() >= 1, "the backlog was never cut");
        assert_eq!(sim.engine_drops, 0);
    }

    #[test]
    fn a_resampled_dc_level_comes_through_at_unity() {
        // 44.1k -> 48k, a constant 0.25 in: once primed and ramped in, every
        // frame is 0.25 — the Hermite passes DC exactly and nothing stale
        // or doubled sneaks in.
        let sim = simulate(44_100, 48_000, 0.0, 0.0, 512, 480, 5.0);
        let mut p = sim.pull;
        let mut out = vec![0.0f32; 480 * 2];
        // Feed it directly: one more block keeps it fed for this callback.
        p.render(&mut out, 2);
        assert!(out.iter().all(|s| (s - 0.25).abs() < 1e-6), "{:?}", &out[..8]);
    }

    #[test]
    fn an_underrun_is_silence_then_a_clean_reprime() {
        let (mut tx, rx) = rtrb::RingBuffer::<f32>::new(RING_SAMPLES);
        let mut p = Pull::new(rx, 48_000, 48_000, 480);
        let mut out = vec![9.0f32; 480 * 2];
        // Empty ring: priming, silence.
        p.render(&mut out, 2);
        assert!(out.iter().all(|&s| s == 0.0));
        assert!(p.priming());
        // Prime it (target = 2 × 480 at least, 20 ms = 960 here).
        let target = p.target() as usize;
        tx.push_entire_slice(&vec![0.5; (target + 480) * 2]).unwrap();
        p.render(&mut out, 2);
        assert!(!p.priming(), "a ring at target resumes");
        // Let it run dry.
        while p.fill() > 480 {
            p.render(&mut out, 2);
        }
        out.fill(9.0);
        p.render(&mut out, 2);
        assert!(p.priming(), "a short ring re-primes");
        assert_eq!(p.underruns(), 1);
        assert!(
            out.iter().all(|&s| s == 0.0),
            "an underrun repeats nothing: it is silence"
        );
        // And a short refill stays silent until the target is back.
        tx.push_entire_slice(&vec![0.5; 200 * 2]).unwrap();
        out.fill(9.0);
        p.render(&mut out, 2);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn a_stalled_sink_is_cut_back_to_target_not_followed() {
        let (mut tx, rx) = rtrb::RingBuffer::<f32>::new(RING_SAMPLES);
        let mut p = Pull::new(rx, 48_000, 48_000, 512);
        let mut out = vec![0.0f32; 480 * 2];
        tx.push_entire_slice(&vec![0.1; 2000 * 2]).unwrap();
        p.render(&mut out, 2);
        // A second of backlog arrives while the sink is stalled.
        tx.push_entire_slice(&vec![0.1; 12_000 * 2]).unwrap();
        p.render(&mut out, 2);
        assert_eq!(p.overruns(), 1);
        assert!((p.fill() as f64 - p.target()).abs() < 600.0, "fill {} target {}", p.fill(), p.target());
    }

    /// **The engine side's rule**: a full ring drops the block, and the push
    /// returns at once. This is `push_entire_slice`'s contract, pinned here
    /// because the engine's safety rests on it: a cable nobody drains must
    /// cost the send its audio and the interface nothing.
    #[test]
    fn a_full_ring_drops_the_block_and_never_blocks() {
        let (mut tx, rx) = rtrb::RingBuffer::<f32>::new(1024);
        let block = vec![0.5f32; 300];
        let t0 = std::time::Instant::now();
        let mut kept = 0;
        for _ in 0..10_000 {
            if tx.push_entire_slice(&block).is_ok() {
                kept += 1;
            }
        }
        assert_eq!(kept, 3, "only whole blocks that fit go in");
        assert_eq!(rx.slots(), 900, "a dropped block leaves nothing half-written");
        assert!(t0.elapsed() < std::time::Duration::from_millis(500), "a push waited");
    }

    #[test]
    fn stereo_maps_onto_mono_stereo_and_wide_devices() {
        let mut mono = [9.0f32; 1];
        spread(0.5, -0.25, &mut mono);
        assert_eq!(mono, [0.125], "mono is the average, not the left");
        let mut st = [9.0f32; 2];
        spread(0.5, -0.25, &mut st);
        assert_eq!(st, [0.5, -0.25]);
        let mut wide = [9.0f32; 6];
        spread(0.5, -0.25, &mut wide);
        assert_eq!(wide, [0.5, -0.25, 0.0, 0.0, 0.0, 0.0], "the rest silent");
        // Through the pull: a 4-channel device gets L,R,0,0 per frame.
        let (mut tx, rx) = rtrb::RingBuffer::<f32>::new(RING_SAMPLES);
        let mut p = Pull::new(rx, 48_000, 48_000, 64);
        let mut frames = Vec::new();
        for _ in 0..4000 {
            frames.extend_from_slice(&[0.5f32, -0.25]);
        }
        tx.push_entire_slice(&frames).unwrap();
        let mut out = vec![9.0f32; 64 * 4];
        p.render(&mut out, 4);
        p.render(&mut out, 4);
        for f in out.chunks_exact(4) {
            assert!((f[0] - 0.5).abs() < 1e-6 && (f[1] + 0.25).abs() < 1e-6);
            assert_eq!(&f[2..], &[0.0, 0.0]);
        }
    }

    #[test]
    fn the_target_covers_the_larger_block_twice_and_the_floor() {
        let (_tx, rx) = rtrb::RingBuffer::<f32>::new(RING_SAMPLES);
        let p = Pull::new(rx, 48_000, 48_000, 2048);
        assert_eq!(p.target(), 4096.0);
        let (_tx, rx) = rtrb::RingBuffer::<f32>::new(RING_SAMPLES);
        let p = Pull::new(rx, 48_000, 48_000, 64);
        assert_eq!(p.target(), 960.0, "20 ms floor");
        assert!(p.ceiling() < p.capacity_frames() as f64);
    }

    #[test]
    fn names_match_exactly_first_then_loosely() {
        let names = vec![
            "CABLE Input (VB-Audio Virtual Cable)".to_owned(),
            "CABLE Input 16ch (VB-Audio Virtual Cable)".to_owned(),
        ];
        assert_eq!(match_name(&names, "CABLE Input 16ch (VB-Audio Virtual Cable)"), Some(1));
        assert_eq!(match_name(&names, "cable input"), Some(0));
        assert_eq!(match_name(&names, "BlackHole"), None);
    }

    /// The real road on a Mac with BlackHole 2ch installed: open, drain for a
    /// moment, report. Needs the device, so ignored.
    #[test]
    #[ignore = "needs a BlackHole (or other) output named in DEVSEND_DEVICE"]
    fn a_real_device_opens_and_pulls() {
        let want = std::env::var("DEVSEND_DEVICE").unwrap_or_else(|_| "BlackHole".into());
        let (mut tx, rx) = rtrb::RingBuffer::<f32>::new(RING_SAMPLES);
        let mut send = DeviceSend::new(rx, 48_000, Some(512));
        send.open(&want).expect("open");
        let block = vec![0.0f32; 1024];
        for _ in 0..200 {
            let _ = tx.push_entire_slice(&block);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let s = send.stats().expect("running");
        assert!(s.callbacks > 10, "{s:?}");
        assert!(send.fault().is_none());
    }
}
