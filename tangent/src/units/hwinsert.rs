//! **Tangent Insert**: an insert bay whose effect is a box on the other end
//! of two cables.
//!
//! The bay's signal leaves on one or two OUTPUT channels of the interface
//! Tangent is playing through, goes through whatever is patched there, and
//! comes back on one or two INPUT channels. The unit is the bay's half of
//! that loop; the [`HwHub`] is the engine's half, and the two meet in a
//! [`HwPort`] that one bay claims at load.
//!
//! **Two streams, two counters, one alignment.** Tangent opens its output
//! and its input as separate streams (see `tangent-monitor-latency` in the
//! repo notes, and `Renderer::mix_monitor`), so there is no duplex callback
//! in which "the frame that went out" and "the frame that came in" share an
//! index. The hub makes one: `out_clock` counts device frames rendered,
//! `in_clock` counts device input frames received (including the ones the
//! monitor path throws away as stale, so a drop never shifts the count), and
//! both are re-based together whenever the input stream changes. On one
//! interface the two run off one crystal, so a sample sent at output frame
//! `O` comes back at input frame `O + delta` for a `delta` that does not
//! move. The unit reads its return by that index rather than by "whatever
//! arrived", so a late input callback costs a glitch and never a permanent
//! shift in the timing.
//!
//! **What the unit reports as its latency** is `loop + chunk + input block`:
//! the physical loop (what a PING measures, see [`find_click`]), plus the
//! engine chunk and one input block of guard, because a return frame can
//! only be read once it has arrived and the input arrives a block at a time.
//! Until a ping, the loop is taken from the round-trip law this repo
//! measured (`round trip = input buffer + 2 x output buffer`) and the face
//! says the figure is unmeasured.
//!
//! **What the audio thread does here**: atomics, one uncontended
//! `Mutex::try_lock` per port per pass (the host only takes the lock at
//! claim time, to size the buffers), copies and arithmetic. Every buffer is
//! sized on the host before the unit crosses to the audio thread.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// How many hardware inserts one engine can run at once. Each one needs
/// physical jacks of its own, so sixteen is past any interface's patience
/// and the table stays a fixed array the audio thread can walk.
pub const HW_PORTS: usize = 16;

/// Frames of return history a port keeps, stereo. The unit reads a block
/// and an input block behind what has arrived (see the module notes), so
/// this only has to outlast one chunk plus one input block plus jitter; 16k
/// is a third of a second at 48 kHz and every ASIO buffer anybody runs.
pub const RET_CAP: usize = 1 << 14;

/// Frames of send a port keeps, stereo. A unit writes its chunk and the
/// fold takes the same chunk out in the same callback, so this only has to
/// hold one engine chunk (512 frames); four is room for the ring to wrap
/// without a frame ever meaning two things.
pub const SEND_CAP: usize = 1 << 11;

/// The engine's chunk, restated: `MAX_BLOCK` in `instrument.rs`, which this
/// module must stand without.
pub const MAX_CHUNK: u32 = 512;

/// How long a ping listens for its click to come back, in seconds. A
/// digital reverb's pre-delay plus two converters plus the biggest buffers
/// anybody runs is a fraction of this.
pub const PING_WINDOW_S: f64 = 1.0;

/// How long a ping keeps the send silent before it fires, so the box has
/// let go of the programme and the click is the loudest thing in the return.
pub const PING_SETTLE_S: f64 = 0.15;

/// The click a ping fires: one sample at -6 dBFS. One sample, because the
/// detector's answer for a clean loop is then the exact delay; -6 dBFS,
/// because a box on the loop may have gain and must not clip on it.
pub const CLICK_LEVEL: f32 = 0.5;

/// The send and return trims' travel, dB, centred on unity.
pub const GAIN_DB: (f32, f32) = (-24.0, 24.0);

/// Starved blocks in a row before the unit decides its alignment no longer
/// describes the streams and takes a new one (the input stopped, or a
/// driver restarted under the same epoch).
const RELOCK_AFTER: u32 = 8;

/// The trims' and mix's per-sample glide at 44.1 kHz: the trim's figure,
/// for the trim's reason (a stepped gain is a click).
const SLEW_AT_44K: f32 = 0.0004;

// ── the jacks ───────────────────────────────────────────────────────────────

/// One side of the loop: the first device channel (zero-based) and whether
/// it is a pair. `None` where this type is optional means "not chosen yet".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Jacks {
    pub first: u16,
    pub stereo: bool,
}

impl Jacks {
    pub fn mono(first: u16) -> Self {
        Self { first, stereo: false }
    }

    pub fn pair(first: u16) -> Self {
        Self { first, stereo: true }
    }

    pub fn width(self) -> u16 {
        if self.stereo {
            2
        } else {
            1
        }
    }

    /// The last channel this side uses.
    pub fn last(self) -> u16 {
        self.first.saturating_add(self.width() - 1)
    }

    /// Whether a device `channels` wide has every channel this side names.
    pub fn fits(self, channels: usize) -> bool {
        usize::from(self.last()) < channels
    }

    /// Whether two sides share a channel.
    pub fn overlaps(self, other: Jacks) -> bool {
        self.first <= other.last() && other.first <= self.last()
    }

    /// One-based, the way an interface's front panel is printed: "3/4", "5".
    pub fn label(self) -> String {
        if self.stereo {
            format!("{}/{}", self.first + 1, self.first + 2)
        } else {
            format!("{}", self.first + 1)
        }
    }

    /// One word for an atomic: 0 is "not chosen".
    pub fn pack(j: Option<Jacks>) -> u32 {
        match j {
            None => 0,
            Some(j) => ((u32::from(j.first) + 1) << 1) | u32::from(j.stereo),
        }
    }

    pub fn unpack(w: u32) -> Option<Jacks> {
        if w == 0 {
            return None;
        }
        let first = u16::try_from((w >> 1).checked_sub(1)?).ok()?;
        Some(Jacks { first, stereo: w & 1 == 1 })
    }
}

/// The device outputs the master plays on: `map_frame` in `instrument.rs`
/// puts the mix on the first two channels (the first one alone on a mono
/// device) and zero-fills the rest. `None` for a device with no outputs.
pub fn main_outs(out_channels: usize) -> Option<Jacks> {
    match out_channels {
        0 => None,
        1 => Some(Jacks::mono(0)),
        _ => Some(Jacks::pair(0)),
    }
}

/// Whether a send would land on the master's own jacks. Refused rather than
/// warned about: the box would get the whole mix on top of the bay's signal,
/// and the speakers would get the bay's dry signal on top of the mix.
pub fn takes_main_outs(send: Jacks, out_channels: usize) -> bool {
    main_outs(out_channels).is_some_and(|m| m.overlaps(send))
}

// ── the link ────────────────────────────────────────────────────────────────

/// Whether a port can run its loop this block, and if not, why. Written by
/// the renderer once a chunk, read by the unit (which passes dry unless
/// `Connected`) and by the face (which prints [`Link::sentence`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Link {
    /// Claimed, and no chunk has looked at it yet.
    Waiting = 0,
    Connected = 1,
    /// No send jacks chosen.
    NoSend = 2,
    /// No return jacks chosen.
    NoReturn = 3,
    /// The device has no such outputs (it changed, or it is unplugged).
    SendMissing = 4,
    /// No input stream is open.
    NoInput = 5,
    /// The input stream has no such channels.
    ReturnMissing = 6,
    /// The send is on the master's own jacks.
    MainOuts = 7,
    /// The varispeed is engaged: the programme runs at another rate than the
    /// device, and a loop timed in device frames cannot be compensated.
    Varispeed = 8,
    /// An engine with no device: an offline render.
    Offline = 9,
    /// Every port is taken.
    NoPort = 10,
}

impl Link {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Link::Connected,
            2 => Link::NoSend,
            3 => Link::NoReturn,
            4 => Link::SendMissing,
            5 => Link::NoInput,
            6 => Link::ReturnMissing,
            7 => Link::MainOuts,
            8 => Link::Varispeed,
            9 => Link::Offline,
            10 => Link::NoPort,
            _ => Link::Waiting,
        }
    }

    pub fn connected(self) -> bool {
        self == Link::Connected
    }

    /// The status line, in the face's own words: what is wrong and what to
    /// do. Every state but `Connected` passes the audio dry.
    pub fn sentence(self) -> &'static str {
        match self {
            Link::Waiting => "Waiting for the audio device.",
            Link::Connected => "Connected.",
            Link::NoSend => "Disconnected: choose the outputs to send on.",
            Link::NoReturn => "Disconnected: choose the inputs it returns on.",
            Link::SendMissing => {
                "Disconnected: the device has no such outputs. Passing dry; the choice is kept for when it returns."
            }
            Link::NoInput => "Disconnected: no audio input is open. Pick an input device in Audio Setup.",
            Link::ReturnMissing => {
                "Disconnected: the input has no such channels. Passing dry; the choice is kept for when it returns."
            }
            Link::MainOuts => "Refused: those outputs carry the main mix. Choose other outputs.",
            Link::Varispeed => "Paused while the varispeed is engaged. Passing dry.",
            Link::Offline => "No device: an offline render cannot play through hardware.",
            Link::NoPort => "Disconnected: every hardware insert port is in use.",
        }
    }
}

// ── the measurement ─────────────────────────────────────────────────────────

/// A ping's answer and the settings it was taken at. A loop measured at one
/// rate or buffer says nothing exact about another, so a mismatch reads as
/// unmeasured rather than as a figure that is wrong by a buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measured {
    /// The physical loop, frames: from the click leaving to it arriving,
    /// net of where the two counters stood (see the module notes).
    pub loop_frames: u32,
    pub rate: u32,
    pub out_block: u32,
    pub in_block: u32,
}

/// **Tangent's round-trip law**: input buffer + 2 x output buffer, measured
/// on the Scarlett 18i20 at five buffer pairs to the hundredth of a
/// millisecond. The output buffer is paid twice because the output stream
/// can only see input that arrived before its callback started.
pub fn law_frames(out_block: u32, in_block: u32) -> u32 {
    in_block.saturating_add(out_block.saturating_mul(2))
}

/// The engine chunk a block of `out_block` frames is rendered in.
pub fn chunk_of(out_block: u32) -> u32 {
    out_block.min(MAX_CHUNK)
}

/// What the bay reports for a loop of `loop_frames`: the loop, plus the
/// chunk the unit renders at once (its last frame must find its return
/// already arrived), plus one input block of guard for the input's jitter.
pub fn latency_for(loop_frames: u32, out_block: u32, in_block: u32) -> u32 {
    loop_frames
        .saturating_add(chunk_of(out_block))
        .saturating_add(in_block)
}

/// The loop an unmeasured insert assumes: the one that makes its reported
/// latency exactly the law's round trip.
pub fn estimated_loop(out_block: u32, in_block: u32) -> u32 {
    law_frames(out_block, in_block).saturating_sub(chunk_of(out_block).saturating_add(in_block))
}

/// Why a ping did not come back with a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PingFail {
    /// Nothing above -80 dBFS came back.
    Silent = 1,
    /// Something came back, but nothing stood out of it like a click.
    Unclear = 2,
    /// The loop was not connected when the ping was asked for, or came apart
    /// while it listened.
    Disconnected = 3,
}

impl PingFail {
    pub fn from_code(c: i64) -> Option<Self> {
        match c {
            -1 => Some(PingFail::Silent),
            -2 => Some(PingFail::Unclear),
            -3 => Some(PingFail::Disconnected),
            _ => None,
        }
    }

    pub fn code(self) -> i64 {
        -(self as i64)
    }

    pub fn sentence(self) -> &'static str {
        match self {
            PingFail::Silent => "The ping did not come back: check the cables, the levels and that the unit is not muted.",
            PingFail::Unclear => "The ping came back buried: set the unit dry or bypassed and ping again.",
            PingFail::Disconnected => "The ping needs a connected loop: choose both sides first.",
        }
    }
}

/// **Find the click in a ping's capture**: the index of the first lobe that
/// reaches half the loudest sample, walked up to that lobe's own peak.
///
/// For a clean wire the click is one sample and the answer is exact. For a
/// real loop the converters' filters spread it into a symmetric pulse whose
/// peak is the delay, and a box that colours it (an EQ, a compressor) moves
/// the peak by less than a sample. A reverb puts its own tail behind the
/// click, which is why the FIRST lobe at half the peak is taken rather than
/// the loudest sample: a reflection louder than the direct sound is rare,
/// and a direct sound at half a reflection still wins.
///
/// `capture` is magnitudes, one per frame. Refuses a capture with nothing in
/// it (under -80 dBFS) and one where the peak does not stand at least eight
/// times above the average, which is noise or a box still ringing.
pub fn find_click(capture: &[f32]) -> Result<usize, PingFail> {
    let mut peak = 0.0f32;
    let mut sum = 0.0f64;
    for &v in capture {
        let a = v.abs();
        if a.is_finite() {
            peak = peak.max(a);
            sum += f64::from(a);
        }
    }
    if peak < 1.0e-4 {
        return Err(PingFail::Silent);
    }
    let mean = (sum / capture.len().max(1) as f64) as f32;
    if peak < 8.0 * mean {
        return Err(PingFail::Unclear);
    }
    let half = 0.5 * peak;
    let mut k = capture.iter().position(|v| v.abs() >= half).ok_or(PingFail::Silent)?;
    while k + 1 < capture.len() && capture[k + 1].abs() > capture[k].abs() {
        k += 1;
    }
    Ok(k)
}

// ── the engine's half ───────────────────────────────────────────────────────

/// The counters and the device facts every port reads. Written by the
/// renderer (and `live` by the engine's constructor), read by the units in
/// the racks and by the host.
#[derive(Debug, Default)]
pub struct HwClocks {
    /// Device output frames rendered. The chunk being rendered starts here.
    pub out_clock: AtomicU64,
    /// Device input frames received, the stale ones included.
    pub in_clock: AtomicU64,
    /// Bumped whenever the two counters were re-based together: a new
    /// input stream, the input going away, the varispeed engaging.
    pub epoch: AtomicU32,
    pub out_block: AtomicU32,
    pub in_block: AtomicU32,
    pub rate: AtomicU32,
    pub out_channels: AtomicU32,
    /// Input channels the open input stream can be read at; 0 with none.
    pub in_channels: AtomicU32,
    /// False on an engine with no device (an offline render).
    pub live: AtomicBool,
}

/// What a port owns that is not a number: the two rings.
#[derive(Debug, Default)]
struct PortIo {
    /// Interleaved stereo, indexed by output frame modulo [`SEND_CAP`].
    send: Vec<f32>,
    /// Interleaved stereo, indexed by input frame modulo [`RET_CAP`].
    ret: Vec<f32>,
    /// The first input frame written in this epoch: anything below it is
    /// from another stream and is read as silence.
    ret_from: u64,
}

/// One claimed loop: what the unit and the renderer say to each other.
#[derive(Debug)]
pub struct HwPort {
    claimed: AtomicBool,
    /// `seat * INSERTS + bay` of the bay that claimed it, `u32::MAX` none.
    key: AtomicU32,
    /// The lane that bay's rack belongs to, `u32::MAX` for a bus or the
    /// master: a lane holding a loop must render every chunk, live.
    lane: AtomicU32,
    send: AtomicU32,
    ret: AtomicU32,
    status: AtomicU8,
    /// The return's two device input columns in the ring, resolved once a
    /// chunk, packed low/high.
    ret_cols: AtomicU32,
    /// Bumped by the host to ask for a ping.
    ping_req: AtomicU32,
    /// The request the last finished ping answered.
    ping_done: AtomicU32,
    /// Its answer: the loop in frames, or a negative [`PingFail`] code.
    ping_result: AtomicI64,
    /// The rate and the two buffers the last good ping measured at.
    ping_at: [AtomicU32; 3],
    /// The last answer the HOST took into the bay's state; the host's
    /// alone, so an answer is adopted once and a later change of jacks
    /// (which drops the measurement) is not undone by adopting it again.
    adopted: AtomicU32,
    pinging: AtomicBool,
    /// What the unit reported last block, and the loop it assumed.
    latency: AtomicU32,
    loop_frames: AtomicU32,
    measured: AtomicBool,
    /// Blocks whose return had not arrived in time, since the claim.
    slips: AtomicU32,
    io: Mutex<PortIo>,
}

impl HwPort {
    fn new() -> Self {
        Self {
            claimed: AtomicBool::new(false),
            key: AtomicU32::new(u32::MAX),
            lane: AtomicU32::new(u32::MAX),
            send: AtomicU32::new(0),
            ret: AtomicU32::new(0),
            status: AtomicU8::new(Link::Waiting as u8),
            ret_cols: AtomicU32::new(0),
            ping_req: AtomicU32::new(0),
            ping_done: AtomicU32::new(0),
            ping_result: AtomicI64::new(0),
            ping_at: [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)],
            adopted: AtomicU32::new(0),
            pinging: AtomicBool::new(false),
            latency: AtomicU32::new(0),
            loop_frames: AtomicU32::new(0),
            measured: AtomicBool::new(false),
            slips: AtomicU32::new(0),
            io: Mutex::new(PortIo::default()),
        }
    }

    pub fn is_claimed(&self) -> bool {
        self.claimed.load(Ordering::Relaxed)
    }

    pub fn key(&self) -> u32 {
        self.key.load(Ordering::Relaxed)
    }

    pub fn routes(&self) -> (Option<Jacks>, Option<Jacks>) {
        (
            Jacks::unpack(self.send.load(Ordering::Relaxed)),
            Jacks::unpack(self.ret.load(Ordering::Relaxed)),
        )
    }

    pub fn link(&self) -> Link {
        Link::from_u8(self.status.load(Ordering::Relaxed))
    }

    /// Ask the unit for a ping. Answered on [`HwPort::ping_answer`].
    pub fn ask_ping(&self) {
        self.ping_req.fetch_add(1, Ordering::Release);
    }

    /// `(request it answered, result)` of the last finished ping.
    pub fn ping_answer(&self) -> (u32, i64) {
        let seq = self.ping_done.load(Ordering::Acquire);
        (seq, self.ping_result.load(Ordering::Relaxed))
    }

    /// The last ping's measurement, when it was a good one.
    pub fn measured_ping(&self) -> Option<Measured> {
        let (seq, result) = self.ping_answer();
        if seq == 0 || result < 0 {
            return None;
        }
        Some(Measured {
            loop_frames: u32::try_from(result).ok()?,
            rate: self.ping_at[0].load(Ordering::Relaxed),
            out_block: self.ping_at[1].load(Ordering::Relaxed),
            in_block: self.ping_at[2].load(Ordering::Relaxed),
        })
    }

    /// **A good answer the host has not taken yet**, taken now: `Some`
    /// once per ping, the host's road into the bay's state.
    pub fn take_measurement(&self) -> Option<Measured> {
        let (seq, _) = self.ping_answer();
        if self.adopted.swap(seq, Ordering::Relaxed) == seq {
            return None;
        }
        self.measured_ping()
    }

    pub fn pinging(&self) -> bool {
        self.pinging.load(Ordering::Relaxed)
            || self.ping_req.load(Ordering::Relaxed) != self.ping_done.load(Ordering::Relaxed)
    }

    pub fn latency(&self) -> u32 {
        self.latency.load(Ordering::Relaxed)
    }

    pub fn loop_frames(&self) -> (u32, bool) {
        (self.loop_frames.load(Ordering::Relaxed), self.measured.load(Ordering::Relaxed))
    }

    pub fn slips(&self) -> u32 {
        self.slips.load(Ordering::Relaxed)
    }
}

/// The engine's table of loops. Lives in the engine's `Shared`.
#[derive(Debug)]
pub struct HwHub {
    ports: Vec<Arc<HwPort>>,
    clocks: Arc<HwClocks>,
    /// How many ports are claimed: the audio thread's early out. Shared
    /// with every claim, which gives its count back when it drops.
    claimed: Arc<AtomicUsize>,
    /// The renderer's own memory of what it last re-based on.
    seen: Mutex<Seen>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Seen {
    input: Option<u64>,
    varispeed: bool,
}

/// What the renderer knows about the device this chunk.
#[derive(Clone, Copy, Debug, Default)]
pub struct DeviceView {
    pub out_channels: usize,
    /// The open input stream's identity (any number that changes when the
    /// stream does) and its width, or `None` with no input open.
    pub input: Option<(u64, usize)>,
    pub out_block: u32,
    pub in_block: u32,
    pub rate: u32,
    pub varispeed: bool,
}

impl Default for HwHub {
    fn default() -> Self {
        Self::new()
    }
}

impl HwHub {
    pub fn new() -> Self {
        Self {
            ports: (0..HW_PORTS).map(|_| Arc::new(HwPort::new())).collect(),
            clocks: Arc::new(HwClocks::default()),
            claimed: Arc::new(AtomicUsize::new(0)),
            seen: Mutex::new(Seen::default()),
        }
    }

    pub fn clocks(&self) -> &HwClocks {
        &self.clocks
    }

    pub fn set_live(&self, live: bool) {
        self.clocks.live.store(live, Ordering::Relaxed);
    }

    /// **Claim a port for the bay `key`**, on the host, sizing its rings.
    /// `None` when all [`HW_PORTS`] are taken. Any other port still naming
    /// the same bay (the tenant this load replaces, on its way out) stops
    /// answering for it, so the host's questions reach the new one.
    pub fn claim(&self, key: u32, lane: Option<usize>) -> Option<HwClaim> {
        let port = self.ports.iter().find(|p| {
            p.claimed
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
        })?;
        for other in &self.ports {
            if !Arc::ptr_eq(other, port) && other.key.load(Ordering::Relaxed) == key {
                other.key.store(u32::MAX, Ordering::Relaxed);
            }
        }
        {
            let mut io = port.io.lock().unwrap_or_else(|p| p.into_inner());
            if io.send.len() != SEND_CAP * 2 {
                io.send = vec![0.0; SEND_CAP * 2];
            } else {
                io.send.fill(0.0);
            }
            if io.ret.len() != RET_CAP * 2 {
                io.ret = vec![0.0; RET_CAP * 2];
            }
            io.ret_from = self.clocks.in_clock.load(Ordering::Relaxed);
        }
        port.lane.store(
            lane.and_then(|l| u32::try_from(l).ok()).unwrap_or(u32::MAX),
            Ordering::Relaxed,
        );
        port.status.store(Link::Waiting as u8, Ordering::Relaxed);
        port.slips.store(0, Ordering::Relaxed);
        port.pinging.store(false, Ordering::Relaxed);
        // A fresh bay has asked nothing and been answered nothing: the
        // tenant this port served before must not lend it a measurement.
        port.ping_req.store(0, Ordering::Relaxed);
        port.ping_done.store(0, Ordering::Relaxed);
        port.ping_result.store(0, Ordering::Relaxed);
        port.adopted.store(0, Ordering::Relaxed);
        port.key.store(key, Ordering::Relaxed);
        self.claimed.fetch_add(1, Ordering::Relaxed);
        Some(HwClaim {
            port: Arc::clone(port),
            clocks: Arc::clone(&self.clocks),
            count: Arc::clone(&self.claimed),
        })
    }

    /// The port answering for bay `key`, if one is claimed for it.
    pub fn port_for(&self, key: u32) -> Option<&HwPort> {
        self.ports
            .iter()
            .find(|p| p.is_claimed() && p.key() == key)
            .map(|p| &**p)
    }

    /// Every claimed port.
    pub fn claimed_ports(&self) -> impl Iterator<Item = &HwPort> {
        self.ports.iter().filter(|p| p.is_claimed()).map(|p| &**p)
    }

    /// The lanes holding a loop, written into `out`; answers how many. The
    /// renderer pins these: never rendered ahead, never rested, never
    /// parked, because a loop renders in device time or not at all.
    pub fn held_lanes(&self, out: &mut [u32; HW_PORTS]) -> usize {
        if self.claimed.load(Ordering::Relaxed) == 0 {
            return 0;
        }
        let mut n = 0;
        for p in &self.ports {
            if p.is_claimed() {
                let lane = p.lane.load(Ordering::Relaxed);
                if lane != u32::MAX && !out[..n].contains(&lane) {
                    out[n] = lane;
                    n += 1;
                }
            }
        }
        n
    }

    /// **The renderer's first word each chunk**: the device as it stands,
    /// every claimed port's link, and a re-base of the two counters when the
    /// input stream changed or the varispeed moved. `col_of` maps a device
    /// input channel to its column in the input ring, `None` for a channel
    /// the stream does not carry.
    pub fn begin_chunk(&self, dev: DeviceView, col_of: impl Fn(usize) -> Option<usize>) {
        let c = &self.clocks;
        c.out_channels.store(dev.out_channels as u32, Ordering::Relaxed);
        c.in_channels.store(dev.input.map_or(0, |(_, w)| w as u32), Ordering::Relaxed);
        c.out_block.store(dev.out_block, Ordering::Relaxed);
        c.in_block.store(dev.in_block, Ordering::Relaxed);
        c.rate.store(dev.rate, Ordering::Relaxed);
        let now = Seen { input: dev.input.map(|(id, _)| id), varispeed: dev.varispeed };
        let rebase = match self.seen.try_lock() {
            Ok(mut seen) => {
                let changed = *seen != now;
                *seen = now;
                changed
            }
            Err(_) => false,
        };
        if rebase {
            let out = c.out_clock.load(Ordering::Relaxed);
            c.in_clock.store(out, Ordering::Relaxed);
            c.epoch.fetch_add(1, Ordering::Relaxed);
            for p in self.claimed_ports() {
                if let Ok(mut io) = p.io.try_lock() {
                    io.ret_from = out;
                }
            }
        }
        if self.claimed.load(Ordering::Relaxed) == 0 {
            return;
        }
        let live = c.live.load(Ordering::Relaxed);
        for p in self.claimed_ports() {
            let (send, ret) = p.routes();
            let mut cols = 0u32;
            let link = if !live {
                Link::Offline
            } else {
                match (send, ret) {
                    (None, _) => Link::NoSend,
                    (_, None) => Link::NoReturn,
                    (Some(s), Some(r)) => {
                        if !s.fits(dev.out_channels) {
                            Link::SendMissing
                        } else if takes_main_outs(s, dev.out_channels) {
                            Link::MainOuts
                        } else if dev.input.is_none() {
                            Link::NoInput
                        } else {
                            match (col_of(usize::from(r.first)), col_of(usize::from(r.last()))) {
                                (Some(a), Some(b)) if a < 0xFFFF && b < 0xFFFF => {
                                    cols = (a as u32) | ((b as u32) << 16);
                                    if dev.varispeed {
                                        Link::Varispeed
                                    } else {
                                        Link::Connected
                                    }
                                }
                                _ => Link::ReturnMissing,
                            }
                        }
                    }
                }
            };
            p.ret_cols.store(cols, Ordering::Relaxed);
            let was = Link::from_u8(p.status.swap(link as u8, Ordering::Relaxed));
            if link.connected() && !was.connected() {
                // Whatever the ring holds is from before the loop came apart:
                // the feed skips a port that is not connected, so those slots
                // were never rewritten, and read now they would be the past.
                if let Ok(mut io) = p.io.try_lock() {
                    io.ret_from = c.in_clock.load(Ordering::Relaxed);
                }
            }
        }
    }

    /// **Device input arriving**, interleaved `ch_in` wide: every connected
    /// port keeps its return columns, indexed by input frame, and the input
    /// counter moves past them. Called for every frame the input delivers,
    /// the ones the monitor path then drops as stale included, so the count
    /// is the device's and not the monitor's.
    ///
    /// The input ring hands its frames over as two slices (the part before
    /// its end and the part after it wraps) and a frame may straddle them,
    /// so the two are read as one run.
    pub fn feed(&self, a: &[f32], b: &[f32], ch_in: usize) {
        let ch_in = ch_in.max(1);
        let count = (a.len() + b.len()) / ch_in;
        let at_sample = |i: usize| if i < a.len() { a[i] } else { b.get(i - a.len()).copied().unwrap_or(0.0) };
        if count == 0 {
            return;
        }
        let at = self.clocks.in_clock.load(Ordering::Relaxed);
        if self.claimed.load(Ordering::Relaxed) > 0 {
            for p in self.claimed_ports() {
                if !p.link().connected() {
                    continue;
                }
                let cols = p.ret_cols.load(Ordering::Relaxed);
                let (ca, cb) = ((cols & 0xFFFF) as usize, (cols >> 16) as usize);
                if ca >= ch_in || cb >= ch_in {
                    continue;
                }
                let Ok(mut io) = p.io.try_lock() else {
                    continue;
                };
                if io.ret.len() < RET_CAP * 2 {
                    continue;
                }
                for f in 0..count {
                    let slot = ((at + f as u64) % RET_CAP as u64) as usize * 2;
                    io.ret[slot] = at_sample(f * ch_in + ca);
                    io.ret[slot + 1] = at_sample(f * ch_in + cb);
                }
            }
        }
        self.clocks.in_clock.store(at + count as u64, Ordering::Relaxed);
    }

    /// **The sends onto the device**, for the chunk just rendered: `out` is
    /// that chunk's interleaved device frames, `dev_ch` wide. Each connected
    /// port's send is ADDED to its jacks, the hardware-route idiom; every
    /// port's slots for the chunk are emptied whether or not they played, so
    /// a unit that stops running leaves silence and not its last block.
    /// Then the output counter moves past the chunk.
    pub fn fold(&self, out: &mut [f32], dev_ch: usize, n: usize) {
        let at = self.clocks.out_clock.load(Ordering::Relaxed);
        if self.claimed.load(Ordering::Relaxed) > 0 && dev_ch > 0 {
            let n = n.min(out.len() / dev_ch);
            for p in self.claimed_ports() {
                let Ok(mut io) = p.io.try_lock() else {
                    continue;
                };
                if io.send.len() < SEND_CAP * 2 {
                    continue;
                }
                let play = if p.link().connected() { p.routes().0 } else { None };
                for f in 0..n {
                    let slot = ((at + f as u64) % SEND_CAP as u64) as usize * 2;
                    let (l, r) = (io.send[slot], io.send[slot + 1]);
                    io.send[slot] = 0.0;
                    io.send[slot + 1] = 0.0;
                    if let Some(j) = play {
                        let first = usize::from(j.first);
                        if first < dev_ch {
                            out[f * dev_ch + first] += l;
                        }
                        if j.stereo && first + 1 < dev_ch {
                            out[f * dev_ch + first + 1] += r;
                        }
                    }
                }
            }
        }
        self.clocks.out_clock.store(at + n as u64, Ordering::Relaxed);
    }
}

/// **A bay's hold on a port.** Dropping it (on the host: units are retired
/// there) gives the port back.
#[derive(Debug)]
pub struct HwClaim {
    port: Arc<HwPort>,
    clocks: Arc<HwClocks>,
    count: Arc<AtomicUsize>,
}

impl HwClaim {
    pub fn port(&self) -> &HwPort {
        &self.port
    }

    pub fn clocks(&self) -> &HwClocks {
        &self.clocks
    }

    /// Write the bay's routes where the renderer reads them.
    pub fn set_routes(&self, send: Option<Jacks>, ret: Option<Jacks>) {
        self.port.send.store(Jacks::pack(send), Ordering::Relaxed);
        self.port.ret.store(Jacks::pack(ret), Ordering::Relaxed);
    }
}

impl Drop for HwClaim {
    fn drop(&mut self) {
        let p = &self.port;
        p.send.store(0, Ordering::Relaxed);
        p.ret.store(0, Ordering::Relaxed);
        p.key.store(u32::MAX, Ordering::Relaxed);
        p.lane.store(u32::MAX, Ordering::Relaxed);
        p.pinging.store(false, Ordering::Relaxed);
        if p.claimed.swap(false, Ordering::AcqRel) {
            self.count.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

// ── the bay's half ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ping {
    Idle,
    /// The send is silent while the box lets go; fires when this reaches 0.
    Settle { left: usize },
    /// The click went out; capturing the return from input frame `from`.
    /// `dip` is how far the input stood below its usual lead when it fired
    /// (see [`HwInsert::lead`]), taken off the answer.
    Listen { from: u64, got: usize, dip: u32 },
}

/// Chunks of input lead the unit remembers; see [`HwInsert::lead`]. An
/// input buffer up to this many output chunks long is covered, which is
/// 4096 frames of input against a 64-frame output.
const LEAD_SPAN: usize = 64;

/// The kernel behind the bay. Built on the host with every buffer it will
/// ever touch; [`HwInsert::attach`] gives it a port.
pub struct HwInsert {
    rate: f32,
    slew: f32,
    send_t: f32,
    send_g: f32,
    ret_t: f32,
    ret_g: f32,
    mix_t: f32,
    mix_g: f32,
    invert: bool,
    send: Option<Jacks>,
    ret: Option<Jacks>,
    measured: Option<Measured>,
    claim: Option<HwClaim>,
    /// The dry signal's delay line, interleaved stereo: the bay's output is
    /// late by its reported latency whether the loop is up or not, so a
    /// disconnected insert passes dry IN TIME.
    dry: Vec<f32>,
    dry_at: usize,
    /// The input frame read for output frame `O` is `O + delta - latency`.
    delta: Option<i64>,
    lock_epoch: u32,
    lock_loop: u32,
    lock_lead: i64,
    starved: u32,
    /// **The input's lead over the output, chunk by chunk**: `in_clock -
    /// out_clock` as this unit sees it. It is a sawtooth, not a number: the
    /// input arrives a whole input buffer at a time, so the lead jumps up
    /// when a buffer lands and falls by a chunk every chunk until the next.
    /// Its MAXIMUM over the cycle does not depend on where in the cycle it
    /// is asked, which is what makes it the reference both the ping and the
    /// alignment measure against.
    leads: [i64; LEAD_SPAN],
    lead_at: usize,
    lead_seen: usize,
    /// The chunk the unit last ran in, and the one it expects next: a second
    /// visit in one chunk (a send destination's rack runs twice) and a gap
    /// (the bay was bypassed or the lane stood down) both change what it does.
    last_c: Option<u64>,
    next_c: u64,
    ping_seen: u32,
    ping: Ping,
    capture: Vec<f32>,
    latency: u32,
    link: Link,
    /// The last ping's answer, kept for [`HwInsert::take_measured`].
    fresh: Option<Measured>,
}

impl HwInsert {
    pub fn new(sample_rate: f32) -> Self {
        let rate = sample_rate.max(8_000.0);
        let window = (f64::from(rate) * PING_WINDOW_S).ceil() as usize;
        // The delay line covers the longest loop a ping can report, plus the
        // chunk and an input block of guard, plus room for the biggest input
        // block a driver hands over.
        let dry_frames = window + MAX_CHUNK as usize + 8_192 + 1;
        Self {
            rate,
            slew: (f64::from(SLEW_AT_44K) * 44_100.0 / f64::from(rate)) as f32,
            send_t: 1.0,
            send_g: 1.0,
            ret_t: 1.0,
            ret_g: 1.0,
            mix_t: 1.0,
            mix_g: 1.0,
            invert: false,
            send: None,
            ret: None,
            measured: None,
            claim: None,
            dry: vec![0.0; dry_frames * 2],
            dry_at: 0,
            delta: None,
            lock_epoch: 0,
            lock_loop: 0,
            lock_lead: 0,
            starved: 0,
            leads: [0; LEAD_SPAN],
            lead_at: 0,
            lead_seen: 0,
            last_c: None,
            next_c: 0,
            ping_seen: 0,
            ping: Ping::Idle,
            capture: vec![0.0; window],
            latency: 0,
            link: Link::NoPort,
            fresh: None,
        }
    }

    fn db(v: f32) -> f32 {
        let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.5 };
        let db = GAIN_DB.0 + (GAIN_DB.1 - GAIN_DB.0) * v;
        if db == 0.0 {
            1.0
        } else {
            10f32.powf(db / 20.0)
        }
    }

    /// One normalised knob: 0 send trim, 1 return trim, 2 mix, 3 invert.
    pub fn set_param(&mut self, id: u32, value: f32) {
        match id {
            0 => self.send_t = Self::db(value),
            1 => self.ret_t = Self::db(value),
            2 => self.mix_t = if value.is_finite() { value.clamp(0.0, 1.0) } else { 1.0 },
            3 => self.invert = value.is_finite() && value >= 0.5,
            _ => {}
        }
    }

    pub fn routes(&self) -> (Option<Jacks>, Option<Jacks>) {
        (self.send, self.ret)
    }

    /// The bay's jacks, from its state. Reaches the renderer through the
    /// port when there is one.
    pub fn set_routes(&mut self, send: Option<Jacks>, ret: Option<Jacks>) {
        self.send = send;
        self.ret = ret;
        if let Some(c) = &self.claim {
            c.set_routes(send, ret);
        }
    }

    pub fn measured(&self) -> Option<Measured> {
        self.measured
    }

    pub fn set_measured(&mut self, m: Option<Measured>) {
        self.measured = m;
        self.latency = m.map_or(self.latency, |m| latency_for(m.loop_frames, m.out_block, m.in_block));
    }

    /// The bay's port. `None` leaves it disconnected (every port taken, or
    /// an engine that has none to give).
    pub fn attach(&mut self, claim: Option<HwClaim>) {
        self.claim = claim;
        if let Some(c) = &self.claim {
            c.set_routes(self.send, self.ret);
            self.ping_seen = c.port.ping_req.load(Ordering::Relaxed);
        }
        self.delta = None;
    }

    pub fn claim(&self) -> Option<&HwClaim> {
        self.claim.as_ref()
    }

    /// Frames the bay delays its signal by, as last worked out.
    pub fn latency(&self) -> u32 {
        self.latency
    }

    pub fn link(&self) -> Link {
        self.link
    }

    /// A ping that finished since this was last asked: the unit has adopted
    /// it already; the host takes it into the bay's state.
    pub fn take_measured(&mut self) -> Option<Measured> {
        self.fresh.take()
    }

    /// Forget the signal: the delay line, the alignment, any ping under way.
    pub fn clear(&mut self) {
        self.dry.fill(0.0);
        self.dry_at = 0;
        self.delta = None;
        self.starved = 0;
        self.lead_seen = 0;
        self.last_c = None;
        self.ping = Ping::Idle;
        self.send_g = self.send_t;
        self.ret_g = self.ret_t;
        self.mix_g = self.mix_t;
        if let Some(c) = &self.claim {
            c.port.pinging.store(false, Ordering::Relaxed);
        }
    }

    fn finish_ping(&mut self, result: i64) {
        if let Some(c) = &self.claim {
            c.port.ping_result.store(result, Ordering::Relaxed);
            c.port.ping_done.store(self.ping_seen, Ordering::Release);
            c.port.pinging.store(false, Ordering::Relaxed);
        }
        self.ping = Ping::Idle;
    }

    /// The dry delay line: write this frame, read the one `delay` back.
    #[inline]
    fn dry_step(&mut self, l: f32, r: f32, delay: usize) -> (f32, f32) {
        let cap = self.dry.len() / 2;
        let w = self.dry_at;
        self.dry[w * 2] = l;
        self.dry[w * 2 + 1] = r;
        let back = (w + cap - delay.min(cap - 1)) % cap;
        self.dry_at = (w + 1) % cap;
        (self.dry[back * 2], self.dry[back * 2 + 1])
    }

    /// **One block through the loop.** Planar in, planar out, `frames` of
    /// each; a mono side is read twice and written once.
    ///
    /// The send is the bay's input times the send trim, a stereo pair onto a
    /// stereo send and the SUM AT -6 dB onto a mono one (the average, the
    /// device send's own rule, so a hard-panned part is not lost). The
    /// return is read by input frame (see the module notes), a mono return
    /// onto both sides, times the return trim and the polarity switch. The
    /// output is `dry * (1 - mix) + wet * mix`, the dry delayed by the
    /// bay's reported latency so the two line up.
    pub fn process(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let Some(claim) = self.claim.as_ref() else {
            self.link = Link::NoPort;
            self.pass_dry(input, frames, out);
            return;
        };
        let clocks = Arc::clone(&claim.clocks);
        let port = Arc::clone(&claim.port);
        let c = clocks.out_clock.load(Ordering::Relaxed);
        let first = self.last_c != Some(c);
        if first && self.last_c.is_some() && c != self.next_c {
            // A gap: the bay did not run for a while. What the delay line
            // holds is from before it, and played now it would be a burst of
            // the past.
            self.dry.fill(0.0);
        }
        if first {
            self.last_c = Some(c);
            self.next_c = c + frames as u64;
        }
        let link = port.link();
        self.link = link;
        let (ob, ib, rate) = (
            clocks.out_block.load(Ordering::Relaxed),
            clocks.in_block.load(Ordering::Relaxed),
            clocks.rate.load(Ordering::Relaxed),
        );
        let (loop_now, measured) = match self.measured {
            Some(m) if m.rate == rate && m.out_block == ob && m.in_block == ib => (m.loop_frames, true),
            _ => (estimated_loop(ob, ib), false),
        };
        let cap = self.dry.len() / 2;
        let latency = latency_for(loop_now, ob, ib).min((cap - 1 - MAX_CHUNK as usize) as u32);
        self.latency = latency;
        port.latency.store(latency, Ordering::Relaxed);
        port.loop_frames.store(loop_now, Ordering::Relaxed);
        port.measured.store(measured, Ordering::Relaxed);

        // A ping asked for since the last block.
        if first {
            let req = port.ping_req.load(Ordering::Acquire);
            if req != self.ping_seen {
                self.ping_seen = req;
                if link.connected() {
                    port.pinging.store(true, Ordering::Relaxed);
                    self.ping = Ping::Settle {
                        left: (f64::from(self.rate) * PING_SETTLE_S) as usize,
                    };
                } else {
                    self.finish_ping(PingFail::Disconnected.code());
                }
            }
        }

        if !link.connected() {
            if self.ping != Ping::Idle {
                self.finish_ping(PingFail::Disconnected.code());
            }
            self.delta = None;
            self.pass_dry(input, frames, out);
            return;
        }

        let Ok(mut io) = port.io.try_lock() else {
            // Only the host's claim takes this lock, for the moment it sizes
            // the rings; a block that meets it passes dry.
            self.pass_dry(input, frames, out);
            return;
        };
        if io.send.len() < SEND_CAP * 2 || io.ret.len() < RET_CAP * 2 {
            drop(io);
            self.pass_dry(input, frames, out);
            return;
        }
        let in_now = clocks.in_clock.load(Ordering::Relaxed);
        let epoch = clocks.epoch.load(Ordering::Relaxed);

        if !first {
            // **A second pass in the same chunk** (a send destination's rack
            // is run again over its arrivals). The loop is one cable: what
            // this pass sends joins the first pass's send, and comes back in
            // the return that the FIRST pass reads, next chunk on. So this
            // pass sends and adds no wet of its own; its dry share is
            // undelayed, the one place the bay is not in time.
            let mono = !self.send.is_some_and(|j| j.stereo);
            for f in 0..frames {
                let l = input.first().and_then(|ch| ch.get(f)).copied().unwrap_or(0.0);
                let r = input.get(1).and_then(|ch| ch.get(f)).copied().unwrap_or(l);
                let slot = ((c + f as u64) % SEND_CAP as u64) as usize * 2;
                if self.ping == Ping::Idle {
                    if mono {
                        io.send[slot] += 0.5 * (l + r) * self.send_g;
                    } else {
                        io.send[slot] += l * self.send_g;
                        io.send[slot + 1] += r * self.send_g;
                    }
                }
                let keep = 1.0 - self.mix_g;
                if let Some(d) = out.get_mut(0).and_then(|ch| ch.get_mut(f)) {
                    *d = l * keep;
                }
                if let Some(d) = out.get_mut(1).and_then(|ch| ch.get_mut(f)) {
                    *d = r * keep;
                }
            }
            return;
        }

        if self.lock_epoch != epoch {
            self.lead_seen = 0;
            self.delta = None;
        }
        let here = in_now as i64 - c as i64;
        let lead = self.lead(here);
        // The alignment: the input frame that carries output frame `O`'s
        // return is `O + loop + lead`. Taken again when the epoch, the loop
        // or the lead's reference moves UP (more of the input's cycle has
        // been seen); a reference that only seems to fall is a cycle not yet
        // seen whole, and starving is what says a real fall happened.
        if self.delta.is_none() || self.lock_epoch != epoch || self.lock_loop != loop_now || lead > self.lock_lead {
            self.delta = Some(i64::from(loop_now) + lead);
            self.lock_epoch = epoch;
            self.lock_loop = loop_now;
            self.lock_lead = lead;
            self.starved = 0;
        }
        let delta = self.delta.unwrap_or(0);

        // The ping's part of this block: what the send carries, and whether
        // the wet is muted (the return is carrying a click).
        let mut fire = false;
        let pinging = match self.ping {
            Ping::Idle => false,
            Ping::Settle { left } => {
                if left == 0 {
                    fire = true;
                    let dip = u32::try_from((lead - here).max(0)).unwrap_or(u32::MAX);
                    self.ping = Ping::Listen { from: in_now, got: 0, dip };
                } else {
                    self.ping = Ping::Settle { left: left.saturating_sub(frames) };
                }
                true
            }
            Ping::Listen { .. } => true,
        };

        let mono_send = !self.send.is_some_and(|j| j.stereo);
        let mono_ret = !self.ret.is_some_and(|j| j.stereo);
        let sign = if self.invert { -1.0 } else { 1.0 };
        let ret_from = io.ret_from;
        let mut starved = false;
        for f in 0..frames {
            self.send_g += (self.send_t - self.send_g) * self.slew;
            self.ret_g += (self.ret_t - self.ret_g) * self.slew;
            self.mix_g += (self.mix_t - self.mix_g) * self.slew;
            let l = input.first().and_then(|ch| ch.get(f)).copied().unwrap_or(0.0);
            let r = input.get(1).and_then(|ch| ch.get(f)).copied().unwrap_or(l);
            let o = c + f as u64;
            let slot = (o % SEND_CAP as u64) as usize * 2;
            if pinging {
                if fire && f == 0 {
                    io.send[slot] += CLICK_LEVEL;
                    io.send[slot + 1] += CLICK_LEVEL;
                }
            } else if mono_send {
                io.send[slot] += 0.5 * (l + r) * self.send_g;
            } else {
                io.send[slot] += l * self.send_g;
                io.send[slot + 1] += r * self.send_g;
            }
            let (dl, dr) = self.dry_step(l, r, latency as usize);
            let want = o as i64 + delta - i64::from(latency);
            let (mut wl, mut wr) = (0.0f32, 0.0f32);
            if want >= in_now as i64 {
                starved = true;
            } else if want >= ret_from as i64 && (in_now as i64 - want) <= RET_CAP as i64 {
                let at = (want as u64 % RET_CAP as u64) as usize * 2;
                wl = io.ret[at];
                wr = if mono_ret { wl } else { io.ret[at + 1] };
            }
            let g = self.ret_g * sign;
            let (wl, wr) = if pinging { (0.0, 0.0) } else { (wl * g, wr * g) };
            let m = self.mix_g;
            let (ol, or) = (dl * (1.0 - m) + wl * m, dr * (1.0 - m) + wr * m);
            if let Some(d) = out.get_mut(0).and_then(|ch| ch.get_mut(f)) {
                *d = ol;
            }
            if let Some(d) = out.get_mut(1).and_then(|ch| ch.get_mut(f)) {
                *d = or;
            }
        }

        // The ping's capture: every return frame that has arrived since the
        // click went out, as a magnitude, until the window is full.
        if let Ping::Listen { from, got, dip } = self.ping {
            let window = self.capture.len();
            let mut got = got;
            while got < window {
                let idx = from + got as u64;
                if idx >= in_now {
                    break;
                }
                let at = (idx % RET_CAP as u64) as usize * 2;
                let (a, b) = (io.ret[at].abs(), io.ret[at + 1].abs());
                self.capture[got] = if mono_ret { a } else { a.max(b) };
                got += 1;
            }
            drop(io);
            if got >= window {
                // The click is `k` frames into a capture that began at the
                // input's lead at the time it fired; against the lead's
                // reference it is `dip` frames earlier than that.
                match find_click(&self.capture).map(|k| u32::try_from(k).unwrap_or(u32::MAX).saturating_sub(dip)) {
                    Ok(k) => {
                        let m = Measured {
                            loop_frames: k,
                            rate,
                            out_block: ob,
                            in_block: ib,
                        };
                        self.measured = Some(m);
                        self.fresh = Some(m);
                        // Reported from this answer on, so the host's next
                        // read of the bay's figure is the measured one.
                        self.latency = latency_for(m.loop_frames, ob, ib).min((cap - 1 - MAX_CHUNK as usize) as u32);
                        port.latency.store(self.latency, Ordering::Relaxed);
                        port.loop_frames.store(m.loop_frames, Ordering::Relaxed);
                        port.measured.store(true, Ordering::Relaxed);
                        for (cell, v) in port.ping_at.iter().zip([rate, ob, ib]) {
                            cell.store(v, Ordering::Relaxed);
                        }
                        self.finish_ping(i64::from(m.loop_frames));
                    }
                    Err(why) => self.finish_ping(why.code()),
                }
            } else {
                self.ping = Ping::Listen { from, got, dip };
            }
        }

        if starved {
            self.starved += 1;
            port.slips.fetch_add(1, Ordering::Relaxed);
            if self.starved >= RELOCK_AFTER {
                self.delta = None;
            }
        } else {
            self.starved = 0;
        }
    }

    /// Note this chunk's lead of the input over the output and answer the
    /// reference: the largest lead in the last [`LEAD_SPAN`] chunks.
    fn lead(&mut self, here: i64) -> i64 {
        self.leads[self.lead_at] = here;
        self.lead_at = (self.lead_at + 1) % LEAD_SPAN;
        self.lead_seen = (self.lead_seen + 1).min(LEAD_SPAN);
        let mut most = here;
        for k in 0..self.lead_seen {
            let at = (self.lead_at + LEAD_SPAN - 1 - k) % LEAD_SPAN;
            most = most.max(self.leads[at]);
        }
        most
    }

    /// The bay's input, delayed by its latency, as its output: what an
    /// insert with no loop does, in time with everything else.
    fn pass_dry(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let latency = self.latency as usize;
        for f in 0..frames {
            let l = input.first().and_then(|ch| ch.get(f)).copied().unwrap_or(0.0);
            let r = input.get(1).and_then(|ch| ch.get(f)).copied().unwrap_or(l);
            let (dl, dr) = self.dry_step(l, r, latency);
            if let Some(d) = out.get_mut(0).and_then(|ch| ch.get_mut(f)) {
                *d = dl;
            }
            if let Some(d) = out.get_mut(1).and_then(|ch| ch.get_mut(f)) {
                *d = dr;
            }
        }
    }
}

// ── the state lines ─────────────────────────────────────────────────────────

/// The extra-state symbols the bay owns, beyond its four knobs. Append-only,
/// like every symbol space in a state blob.
pub const EXTRA_SYMBOLS: [&str; 8] = [
    "send_first",
    "send_width",
    "ret_first",
    "ret_width",
    "loop_frames",
    "loop_rate",
    "loop_out",
    "loop_in",
];

/// The bay's routing and measurement as state lines.
pub fn state_lines(send: Option<Jacks>, ret: Option<Jacks>, m: Option<Measured>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (side, j) in [("send", send), ("ret", ret)] {
        if let Some(j) = j {
            out.push((format!("{side}_first"), j.first.to_string()));
            out.push((format!("{side}_width"), j.width().to_string()));
        }
    }
    if let Some(m) = m {
        out.push(("loop_frames".to_owned(), m.loop_frames.to_string()));
        out.push(("loop_rate".to_owned(), m.rate.to_string()));
        out.push(("loop_out".to_owned(), m.out_block.to_string()));
        out.push(("loop_in".to_owned(), m.in_block.to_string()));
    }
    out
}

/// Read [`state_lines`] back. Forgiving: a side with no `first` is not
/// chosen, a width that is not 2 is mono, and a measurement missing any of
/// its four numbers is no measurement.
pub fn parse_lines(extra: &[(String, String)]) -> (Option<Jacks>, Option<Jacks>, Option<Measured>) {
    let get = |k: &str| extra.iter().rev().find(|(s, _)| s == k).map(|(_, v)| v.as_str());
    let side = |s: &str| {
        let first = get(&format!("{s}_first"))?.trim().parse::<u16>().ok()?;
        let stereo = get(&format!("{s}_width")).and_then(|w| w.trim().parse::<u16>().ok()) == Some(2);
        Some(Jacks { first, stereo })
    };
    let num = |k: &str| get(k).and_then(|v| v.trim().parse::<u32>().ok());
    let m = match (num("loop_frames"), num("loop_rate"), num("loop_out"), num("loop_in")) {
        (Some(loop_frames), Some(rate), Some(out_block), Some(in_block)) => Some(Measured {
            loop_frames,
            rate,
            out_block,
            in_block,
        }),
        _ => None,
    };
    (side("send"), side("ret"), m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jacks_pack_and_unpack_and_say_what_they_overlap() {
        for j in [None, Some(Jacks::mono(0)), Some(Jacks::pair(2)), Some(Jacks::mono(17)), Some(Jacks::pair(u16::MAX - 2))] {
            assert_eq!(Jacks::unpack(Jacks::pack(j)), j);
        }
        assert!(Jacks::pair(2).overlaps(Jacks::mono(3)));
        assert!(!Jacks::pair(2).overlaps(Jacks::mono(4)));
        assert!(Jacks::pair(2).fits(4) && !Jacks::pair(3).fits(4));
        assert_eq!(Jacks::pair(2).label(), "3/4");
        assert_eq!(Jacks::mono(4).label(), "5");
        // The master's jacks: a pair, or the one channel of a mono device.
        assert!(takes_main_outs(Jacks::mono(1), 8));
        assert!(takes_main_outs(Jacks::pair(0), 8));
        assert!(!takes_main_outs(Jacks::pair(2), 8));
        assert!(takes_main_outs(Jacks::mono(0), 1));
        assert!(!takes_main_outs(Jacks::mono(1), 1));
    }

    #[test]
    fn the_state_lines_round_trip_and_forgive_what_they_should() {
        let m = Measured { loop_frames: 391, rate: 48_000, out_block: 128, in_block: 64 };
        for (s, r, m) in [
            (None, None, None),
            (Some(Jacks::pair(2)), Some(Jacks::mono(5)), Some(m)),
            (Some(Jacks::mono(7)), None, None),
        ] {
            assert_eq!(parse_lines(&state_lines(s, r, m)), (s, r, m));
        }
        // A width that is not two is mono; a measurement missing a number is
        // none at all.
        let lines = vec![
            ("send_first".to_owned(), "3".to_owned()),
            ("send_width".to_owned(), "7".to_owned()),
            ("loop_frames".to_owned(), "12".to_owned()),
        ];
        assert_eq!(parse_lines(&lines), (Some(Jacks::mono(3)), None, None));
        for s in EXTRA_SYMBOLS {
            assert!(state_lines(Some(Jacks::pair(2)), Some(Jacks::pair(2)), Some(m)).iter().any(|(k, _)| k == s));
        }
    }

    /// **The detector finds a synthetic loop's delay exactly**, for a bare
    /// wire, a gain, an inversion, a converter-shaped pulse and a reverb tail
    /// behind the click, at delays across the window.
    #[test]
    fn the_ping_detector_finds_a_delayed_click_exactly() {
        let n = 48_000;
        let mut seed = 0x2545_f491u32;
        let mut noise = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed as f32 / u32::MAX as f32 - 0.5) * 2.0e-4
        };
        for d in [0usize, 1, 37, 391, 4_800, 47_000] {
            for shape in 0..4 {
                let mut cap: Vec<f32> = (0..n).map(|_| noise()).collect();
                match shape {
                    0 => cap[d] += CLICK_LEVEL,
                    1 => cap[d] -= CLICK_LEVEL * 0.3,
                    2 => {
                        // A symmetric sinc-ish pulse: the converters' filters.
                        for k in -8i64..=8 {
                            let at = d as i64 + k;
                            if (0..n as i64).contains(&at) {
                                let x = k as f32 * 0.7;
                                let v = if k == 0 { 1.0 } else { x.sin() / x };
                                cap[at as usize] += 0.4 * v;
                            }
                        }
                    }
                    _ => {
                        // A tail behind the click, quieter than it.
                        cap[d] += 0.5;
                        for k in 1..2_000 {
                            if d + k < n {
                                cap[d + k] += 0.2 * (-(k as f32) / 300.0).exp() * if k % 2 == 0 { 1.0 } else { -1.0 };
                            }
                        }
                    }
                }
                let mag: Vec<f32> = cap.iter().map(|v| v.abs()).collect();
                assert_eq!(find_click(&mag), Ok(d), "delay {d}, shape {shape}");
            }
        }
        assert_eq!(find_click(&vec![0.0; n]), Err(PingFail::Silent));
        let busy: Vec<f32> = (0..n).map(|k| 0.3 + 0.01 * ((k % 7) as f32)).collect();
        assert_eq!(find_click(&busy), Err(PingFail::Unclear));
    }

    /// A fake interface, one crystal for both directions: the renderer's
    /// three calls in the order a chunk makes them, a loop that carries the
    /// device's output channels back to its inputs `delay` frames later, and
    /// the bay's unit run in between.
    pub(crate) struct FakeDevice {
        pub hub: HwHub,
        pub out_ch: usize,
        pub in_ch: usize,
        pub block: usize,
        /// Device frames in flight between the jacks: every output frame
        /// arrives at the input this many frames later.
        pub delay: usize,
        /// (output channel, input channel, gain): what is patched.
        pub patch: Vec<(usize, usize, f32)>,
        line: std::collections::VecDeque<Vec<f32>>,
        pub last_out: Vec<f32>,
        /// The input is delivered every this many output blocks, in one
        /// burst: an input buffer bigger than the output's.
        pub in_every: usize,
        held: Vec<f32>,
        blocks: usize,
    }

    impl FakeDevice {
        pub fn new(out_ch: usize, in_ch: usize, block: usize, delay: usize) -> Self {
            let hub = HwHub::new();
            hub.set_live(true);
            Self {
                hub,
                out_ch,
                in_ch,
                block,
                delay,
                patch: Vec::new(),
                line: std::iter::repeat_n(vec![0.0; in_ch], delay).collect(),
                last_out: Vec::new(),
                in_every: 1,
                held: Vec::new(),
                blocks: 0,
            }
        }

        fn view(&self) -> DeviceView {
            DeviceView {
                out_channels: self.out_ch,
                input: Some((1, self.in_ch)),
                out_block: self.block as u32,
                in_block: (self.block * self.in_every) as u32,
                rate: 48_000,
                varispeed: false,
            }
        }

        /// One device block: begin, the unit, the fold, then the device
        /// plays the block and records the next input block. The input
        /// arrives a block late, as two separate streams deliver it.
        pub fn block(&mut self, unit: &mut HwInsert, input: &[Vec<f32>], out: &mut [Vec<f32>]) {
            let in_ch = self.in_ch;
            self.hub.begin_chunk(self.view(), |c| (c < in_ch).then_some(c));
            unit.process(input, self.block, out);
            let mut dev = vec![0.0f32; self.block * self.out_ch];
            self.hub.fold(&mut dev, self.out_ch, self.block);
            let mut arrived = Vec::with_capacity(self.block * in_ch);
            for f in 0..self.block {
                let mut frame = vec![0.0f32; in_ch];
                for &(o, i, g) in &self.patch {
                    if o < self.out_ch && i < in_ch {
                        frame[i] += dev[f * self.out_ch + o] * g;
                    }
                }
                self.line.push_back(frame);
                let got = self.line.pop_front().unwrap_or_default();
                arrived.extend_from_slice(&got);
            }
            self.held.extend_from_slice(&arrived);
            self.blocks += 1;
            if self.blocks % self.in_every.max(1) == 0 {
                self.hub.feed(&self.held, &[], in_ch);
                self.held.clear();
            }
            self.last_out = dev;
        }
    }

    fn seated(dev: &FakeDevice, send: Jacks, ret: Jacks) -> HwInsert {
        let mut u = HwInsert::new(48_000.0);
        u.set_routes(Some(send), Some(ret));
        u.attach(dev.hub.claim(7, Some(1)));
        u
    }

    fn run_impulse(dev: &mut FakeDevice, u: &mut HwInsert, blocks: usize, at_block: usize) -> Vec<(f32, f32)> {
        let b = dev.block;
        let mut heard = Vec::new();
        for k in 0..blocks {
            let mut l = vec![0.0f32; b];
            if k == at_block {
                l[0] = 1.0;
            }
            let input = vec![l.clone(), l];
            let mut out = vec![vec![0.0f32; b]; 2];
            dev.block(u, &input, &mut out);
            heard.extend(out[0].iter().zip(&out[1]).map(|(a, c)| (*a, *c)));
        }
        heard
    }

    /// **A ping measures the loop, and the bay's output then lands exactly
    /// where it says it does**: an impulse into the bay comes out of it
    /// `latency()` frames later, through a fake interface whose cable is a
    /// pure delay. The figure the bay reports is what delay compensation is
    /// given, so this is the whole promise.
    #[test]
    fn a_pinged_loop_comes_out_exactly_at_the_latency_it_reports() {
        for (block, delay) in [(128usize, 37usize), (64, 300), (256, 5), (512, 1_000)] {
            let mut dev = FakeDevice::new(6, 6, block, delay);
            dev.patch = vec![(2, 4, 1.0), (3, 5, 1.0)];
            let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(4));
            // Settle, then ping.
            let _ = run_impulse(&mut dev, &mut u, 8, usize::MAX);
            assert_eq!(u.link(), Link::Connected);
            dev.hub.port_for(7).expect("claimed").ask_ping();
            let mut blocks = 0;
            while dev.hub.port_for(7).expect("claimed").pinging() {
                let _ = run_impulse(&mut dev, &mut u, 1, usize::MAX);
                blocks += 1;
                assert!(blocks < 2_000, "the ping never finished");
            }
            let (seq, result) = dev.hub.port_for(7).unwrap().ping_answer();
            assert_eq!(seq, 1);
            assert!(result >= 0, "the ping failed: {result}");
            let m = u.take_measured().expect("a measurement");
            assert_eq!(m.loop_frames as i64, result);
            let reported = u.latency() as usize;
            assert_eq!(
                reported,
                latency_for(m.loop_frames, block as u32, block as u32) as usize
            );
            // An impulse through the loop now.
            let heard = run_impulse(&mut dev, &mut u, 40, 4);
            let at = heard.iter().position(|(l, _)| l.abs() > 0.5).expect("the impulse came back");
            assert_eq!(at - 4 * block, reported, "block {block}, cable {delay}");
            assert!((heard[at].0 - 1.0).abs() < 1e-3 && (heard[at].1 - 1.0).abs() < 1e-3);
            // And nothing else: the loop is a wire.
            let stray = heard.iter().enumerate().filter(|(k, (l, r))| *k != at && (l.abs() > 1e-3 || r.abs() > 1e-3)).count();
            assert_eq!(stray, 0, "the loop smeared the impulse");
            // A second ping on the same cable answers the same number.
            dev.hub.port_for(7).unwrap().ask_ping();
            while dev.hub.port_for(7).unwrap().pinging() {
                let _ = run_impulse(&mut dev, &mut u, 1, usize::MAX);
            }
            assert_eq!(dev.hub.port_for(7).unwrap().ping_answer().1, result);
        }
    }

    /// **An input that arrives in bursts bigger than the output's block
    /// costs nothing**: the guard of one input block covers it, the loop
    /// never starves, and the impulse lands where the bay says.
    #[test]
    fn an_input_that_arrives_in_bursts_keeps_the_loop_exact() {
        for (block, every, delay) in [(128usize, 2usize, 40usize), (64, 4, 500), (256, 3, 7)] {
            let mut dev = FakeDevice::new(4, 4, block, delay);
            dev.in_every = every;
            dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
            let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
            let _ = run_impulse(&mut dev, &mut u, 12, usize::MAX);
            dev.hub.port_for(7).unwrap().ask_ping();
            while dev.hub.port_for(7).unwrap().pinging() {
                let _ = run_impulse(&mut dev, &mut u, 1, usize::MAX);
            }
            let slips = dev.hub.port_for(7).unwrap().slips();
            for start in [3usize, 4, 5] {
                let heard = run_impulse(&mut dev, &mut u, 40, start);
                let at = heard.iter().position(|(l, _)| l.abs() > 0.5).expect("came back");
                assert_eq!(at - start * block, u.latency() as usize, "block {block} x{every}");
            }
            assert_eq!(dev.hub.port_for(7).unwrap().slips(), slips, "the loop starved");
        }
    }

    /// **The send lands on its jacks and nowhere else**, mono as the sum at
    /// -6 dB, stereo as the pair; and a mono return reaches both sides.
    #[test]
    fn the_send_lands_on_its_jacks_and_a_mono_return_feeds_both_sides() {
        let block = 128;
        // Mono send of a stereo signal: the average, on output 5 alone.
        let mut dev = FakeDevice::new(8, 8, block, 10);
        dev.patch = vec![(4, 6, 1.0)];
        let mut u = seated(&dev, Jacks::mono(4), Jacks::mono(6));
        let input = vec![vec![0.8f32; block], vec![0.2f32; block]];
        let mut out = vec![vec![0.0f32; block]; 2];
        for _ in 0..4 {
            dev.block(&mut u, &input, &mut out);
        }
        for f in 0..block {
            for c in 0..8 {
                let v = dev.last_out[f * 8 + c];
                if c == 4 {
                    assert!((v - 0.5).abs() < 1e-6, "mono send carried {v}");
                } else {
                    assert_eq!(v, 0.0, "output {c} carried {v}");
                }
            }
        }
        // ...and the mono return comes back on BOTH sides of the bay.
        for _ in 0..40 {
            dev.block(&mut u, &input, &mut out);
        }
        assert!((out[0][block - 1] - 0.5).abs() < 1e-4 && (out[1][block - 1] - 0.5).abs() < 1e-4);

        // A stereo send keeps its sides apart.
        let mut dev = FakeDevice::new(8, 8, block, 10);
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        for _ in 0..4 {
            dev.block(&mut u, &input, &mut out);
        }
        for f in 0..block {
            assert!((dev.last_out[f * 8 + 2] - 0.8).abs() < 1e-6);
            assert!((dev.last_out[f * 8 + 3] - 0.2).abs() < 1e-6);
            assert_eq!(dev.last_out[f * 8], 0.0);
            assert_eq!(dev.last_out[f * 8 + 1], 0.0);
        }
    }

    /// **The trims, the mix and the polarity do what their names say**,
    /// once the glides have arrived.
    #[test]
    fn the_trims_mix_and_polarity_shape_the_wet_and_the_dry() {
        let block = 128;
        let run = |send: f32, ret: f32, mix: f32, inv: f32| {
            let mut dev = FakeDevice::new(4, 4, block, 20);
            dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
            let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
            u.set_param(0, send);
            u.set_param(1, ret);
            u.set_param(2, mix);
            u.set_param(3, inv);
            u.clear();
            let input = vec![vec![0.25f32; block]; 2];
            let mut out = vec![vec![0.0f32; block]; 2];
            for _ in 0..60 {
                dev.block(&mut u, &input, &mut out);
            }
            out[0][block - 1]
        };
        let db = |d: f32| 10f32.powf(d / 20.0);
        // Unity, fully wet: the wire.
        assert!((run(0.5, 0.5, 1.0, 0.0) - 0.25).abs() < 1e-4);
        // +6 dB out, -6 dB back: unity again.
        let up = (6.0 - GAIN_DB.0) / (GAIN_DB.1 - GAIN_DB.0);
        let down = (-6.0 - GAIN_DB.0) / (GAIN_DB.1 - GAIN_DB.0);
        assert!((run(up, down, 1.0, 0.0) - 0.25).abs() < 1e-3);
        assert!((run(up, 0.5, 1.0, 0.0) - 0.25 * db(6.0)).abs() < 1e-3);
        // Inverted, fully wet.
        assert!((run(0.5, 0.5, 1.0, 1.0) + 0.25).abs() < 1e-4);
        // Inverted at half mix: the dry and the wet cancel.
        assert!(run(0.5, 0.5, 0.5, 1.0).abs() < 1e-4);
        // Fully dry: the input, whatever the loop does.
        assert!((run(1.0, 1.0, 0.0, 1.0) - 0.25).abs() < 1e-4);
    }

    /// **Disconnected passes dry, in time, and keeps the routes**: the
    /// device losing the send's outputs, then the input going away, then
    /// both coming back.
    #[test]
    fn a_missing_device_passes_dry_in_time_and_keeps_the_choice() {
        let block = 128;
        let mut dev = FakeDevice::new(6, 6, block, 30);
        dev.patch = vec![(4, 4, 1.0), (5, 5, 1.0)];
        let mut u = seated(&dev, Jacks::pair(4), Jacks::pair(4));
        let _ = run_impulse(&mut dev, &mut u, 6, usize::MAX);
        assert_eq!(u.link(), Link::Connected);
        let latency = u.latency() as usize;
        // Where the loop puts an impulse while it is up. Unpinged, that is
        // the cable's 30 frames plus what the estimate adds, not the estimate
        // itself; what matters below is that it is the same place after.
        let heard = run_impulse(&mut dev, &mut u, 30, 4);
        let looped = heard.iter().position(|(l, _)| l.abs() > 0.5).expect("the loop is up") - 4 * block;
        // The interface shrinks to two outputs: the bay goes dry, still late
        // by its latency, and the send leaves nothing on any jack.
        dev.out_ch = 2;
        let heard = run_impulse(&mut dev, &mut u, 30, 4);
        assert_eq!(u.link(), Link::SendMissing);
        let at = heard.iter().position(|(l, _)| l.abs() > 0.5).expect("dry came through");
        assert_eq!(at - 4 * block, latency);
        assert!(dev.last_out.iter().all(|v| *v == 0.0));
        assert_eq!(u.routes(), (Some(Jacks::pair(4)), Some(Jacks::pair(4))));
        // It comes back and the loop is live again.
        dev.out_ch = 6;
        let heard = run_impulse(&mut dev, &mut u, 40, 10);
        assert_eq!(u.link(), Link::Connected);
        let at = heard.iter().rposition(|(l, _)| l.abs() > 0.5).expect("the loop came back");
        assert_eq!(at - 10 * block, looped);
        // With no port at all (every port taken), dry as well.
        let mut lonely = HwInsert::new(48_000.0);
        let input = vec![vec![0.5f32; 64]; 2];
        let mut out = vec![vec![0.0f32; 64]; 2];
        lonely.process(&input, 64, &mut out);
        assert_eq!(lonely.link(), Link::NoPort);
        assert!(out[0].iter().all(|v| *v == 0.5));
    }

    /// **The master's jacks are refused**, and the renderer says so rather
    /// than letting the loop run on them.
    #[test]
    fn a_send_on_the_main_outs_is_refused() {
        let mut dev = FakeDevice::new(4, 4, 128, 10);
        let mut u = seated(&dev, Jacks::pair(0), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut u, 4, 1);
        assert_eq!(u.link(), Link::MainOuts);
        assert!(dev.last_out.iter().all(|v| *v == 0.0));
        let mut u2 = seated(&dev, Jacks::mono(1), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut u2, 2, 0);
        assert_eq!(u2.link(), Link::MainOuts);
    }

    /// **Unmeasured, the bay reports the law's round trip**: input buffer
    /// plus twice the output buffer.
    #[test]
    fn an_unpinged_insert_reports_the_round_trip_law() {
        for (out_b, in_b) in [(128u32, 128u32), (256, 64), (64, 64), (1024, 256)] {
            let dev = FakeDevice::new(4, 4, out_b as usize, 10);
            let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
            dev.hub.begin_chunk(
                DeviceView { in_block: in_b, ..dev.view() },
                |c| Some(c),
            );
            let input = vec![vec![0.0f32; 16]; 2];
            let mut out = vec![vec![0.0f32; 16]; 2];
            u.process(&input, 16, &mut out);
            assert_eq!(u.latency(), in_b + 2 * out_b, "{out_b}/{in_b}");
            assert!(!dev.hub.port_for(7).unwrap().loop_frames().1);
        }
    }

    /// **A ping refuses honestly**: a loop with nothing patched answers
    /// Silent, and a ping on a disconnected bay answers Disconnected.
    #[test]
    fn a_ping_that_hears_nothing_says_so() {
        let mut dev = FakeDevice::new(4, 4, 256, 10);
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut u, 4, usize::MAX);
        dev.hub.port_for(7).unwrap().ask_ping();
        let mut n = 0;
        while dev.hub.port_for(7).unwrap().pinging() {
            let _ = run_impulse(&mut dev, &mut u, 1, usize::MAX);
            n += 1;
            assert!(n < 1_000);
        }
        assert_eq!(PingFail::from_code(dev.hub.port_for(7).unwrap().ping_answer().1), Some(PingFail::Silent));
        assert!(u.take_measured().is_none());

        let mut dev = FakeDevice::new(4, 4, 256, 10);
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        dev.out_ch = 2;
        let _ = run_impulse(&mut dev, &mut u, 2, usize::MAX);
        dev.hub.port_for(7).unwrap().ask_ping();
        let _ = run_impulse(&mut dev, &mut u, 2, usize::MAX);
        assert_eq!(
            PingFail::from_code(dev.hub.port_for(7).unwrap().ping_answer().1),
            Some(PingFail::Disconnected)
        );
    }

    /// **A claim names one bay and gives its port back when it goes**; a
    /// second claim for the same bay takes the questions over.
    #[test]
    fn claims_are_one_per_port_and_come_back() {
        let hub = HwHub::new();
        let a = hub.claim(3, Some(0)).expect("a port");
        let b = hub.claim(3, Some(0)).expect("another port");
        assert!(std::ptr::eq(hub.port_for(3).unwrap(), b.port()));
        assert_eq!(a.port().key(), u32::MAX);
        let mut lanes = [0u32; HW_PORTS];
        assert_eq!(hub.held_lanes(&mut lanes), 1);
        drop(a);
        drop(b);
        assert!(hub.port_for(3).is_none());
        assert_eq!(hub.held_lanes(&mut lanes), 0);
        let all: Vec<_> = (0..HW_PORTS as u32).map(|k| hub.claim(k, None)).collect();
        assert!(all.iter().all(Option::is_some));
        assert!(hub.claim(99, None).is_none(), "a seventeenth loop got a port");
    }

    /// **Nothing on the audio thread allocates**: the unit's block, a ping
    /// from request to answer, and the hub's three calls.
    #[test]
    fn the_loop_allocates_nothing_on_the_audio_thread() {
        let block = 128;
        let mut dev = FakeDevice::new(6, 6, block, 50);
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let input = vec![vec![0.1f32; block]; 2];
        let mut out = vec![vec![0.0f32; block]; 2];
        let mut dev_out = vec![0.0f32; block * 6];
        let arrived = vec![0.0f32; block * 6];
        let view = dev.view();
        // Warm up outside the window.
        for _ in 0..4 {
            dev.block(&mut u, &input, &mut out);
        }
        dev.hub.port_for(7).unwrap().ask_ping();
        let ((), counted) = crate::rt_alloc::count_allocs(|| {
            for _ in 0..800 {
                dev.hub.begin_chunk(view, |c| (c < 6).then_some(c));
                u.process(&input, block, &mut out);
                dev.hub.fold(&mut dev_out, 6, block);
                dev.hub.feed(&arrived, &[], 6);
            }
        });
        assert_eq!(counted.total(), 0, "the loop allocated on the audio thread: {counted:?}");
        // The ping ran its course inside the window (it heard nothing, since
        // nothing was fed back, and said so).
        assert!(!dev.hub.port_for(7).unwrap().pinging());
    }
}
