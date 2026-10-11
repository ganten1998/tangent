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

/// **The longest delay the desk can compensate**, frames: `MAX_PDC_FRAMES`
/// in `instrument.rs`, restated because this module must stand without it
/// (a test there pins the two). A loop whose latency would pass it cannot
/// be lined up, so the bay refuses it rather than compensating part of it.
pub const PDC_LIMIT: u32 = 8_192;

/// How long a ping request may wait for the bay to run before it is
/// dropped, seconds. A bay that is bypassed or stood down never runs, and a
/// request that waited out a bypass must not fire a click into the box the
/// moment the bay comes back, possibly in the middle of a take.
pub const PING_EXPIRE_S: f64 = 2.0;

/// The last stretch of the settle the return's level is measured over,
/// seconds: the quiet the click has to stand out of.
const QUIET_S: f64 = 0.05;

/// How many times the quiet's loudest sample the click must reach.
const QUIET_RATIO: f32 = 4.0;

/// Seconds of an epoch the clock check lets pass before it starts to judge:
/// two streams starting up (WASAPI's start-up burst above all) land their
/// first callbacks wherever they like.
const DRIFT_SETTLE_S: f64 = 2.0;

/// The clock check's window, seconds: it judges the input's lead by its
/// lowest point in each window, which the block-sized sawtooth does not
/// move, and calls two clocks only when that point walks the same way
/// window after window.
const DRIFT_WINDOW_S: f64 = 4.0;

/// The window lows the clock check keeps: four it judges and one more,
/// so every judged low has a neighbour on each side to be checked against
/// (see [`drift_verdict`]).
const DRIFT_KEEP: usize = 5;

/// The least a window's low must move to count as a step of a walk, frames.
/// 100 ppm moves it 19 frames a window at 48 kHz; a still one moves it
/// none.
const DRIFT_MIN_STEP: i64 = 4;

/// **The clock check's verdict** over the last [`DRIFT_KEEP`] window lows,
/// oldest first, given whether two clocks were already called (`was`) and
/// how many verdicts in a row the lows have held still since (`still`).
///
/// A window whose low fell an input block or more below both its
/// neighbours is one late input callback, not a trend, and reads as its
/// neighbours. Two clocks are called when the lows walked at least three
/// steps' worth one way across the four judged windows, with at least two
/// of the three steps going that way: a one-off step (an underrun, a USB
/// hiccup) walks in one step and then holds, and is not. Once called, the
/// verdict stands until the lows have held still for two verdicts in a
/// row, so one odd window does not flip the bay between wet and dry.
fn drift_verdict(lows: &[i64; DRIFT_KEEP], in_block: i64, was: bool, still: &mut u32) -> bool {
    let mut l = *lows;
    for i in 1..DRIFT_KEEP - 1 {
        let floor = lows[i - 1].min(lows[i + 1]);
        if lows[i] <= floor - in_block.max(1) {
            l[i] = floor;
        }
    }
    let judged = &l[..DRIFT_KEEP - 1];
    let net = judged[judged.len() - 1] - judged[0];
    let way = net.signum();
    let agree = judged.windows(2).filter(|w| (w[1] - w[0]) * way >= DRIFT_MIN_STEP).count();
    let walking = way != 0 && net.abs() >= 3 * DRIFT_MIN_STEP && agree >= 2;
    if walking {
        *still = 0;
        return true;
    }
    if !was {
        return false;
    }
    if net.abs() < 2 * DRIFT_MIN_STEP {
        *still += 1;
    } else {
        *still = 0;
    }
    *still < 2
}

/// The callback size's window, seconds: the size the stream is keyed on is
/// the largest in a window, and changes only when two windows in a row agree
/// on a new one, so one oversized callback moves nothing and a real change
/// of buffer (another client resizing the device) lands within two seconds.
const BLOCK_WINDOW_S: f64 = 1.0;

/// The callbacks at a stream's start the size is not keyed on: its
/// start-up burst.
const STARTUP_CALLBACKS: u32 = 2;

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
    /// The input's lead over the output keeps moving: two clocks.
    ClockDrift = 11,
    /// The input is a different device from the output: two clocks.
    SeparateDevice = 12,
    /// The loop is longer than delay compensation can absorb.
    TooLong = 13,
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
            11 => Link::ClockDrift,
            12 => Link::SeparateDevice,
            13 => Link::TooLong,
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
            Link::ClockDrift => {
                "Disconnected: the input and the output are running on different clocks, so the loop \
                 cannot stay in time. Use one interface for both, or an aggregate device."
            }
            Link::SeparateDevice => {
                "Disconnected: the input is a different device from the output, so the two run on \
                 different clocks. Use one interface for both, or an aggregate device."
            }
            Link::TooLong => {
                "Refused: the loop is longer than delay compensation can absorb (8192 samples). \
                 Lower the buffer size, or shorten the loop."
            }
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
/// already arrived), plus TWO input blocks of guard. One covers the input's
/// sawtooth (it arrives a block at a time); the other covers what a lane's
/// rack cannot see yet, because a lane runs before the chunk's input is
/// read and the monitor may leave up to a block unread. The figure is the
/// same wherever the bay sits, so a bay moved from a lane to the master, or
/// to the room when its track is armed, keeps its timing.
pub fn latency_for(loop_frames: u32, out_block: u32, in_block: u32) -> u32 {
    loop_frames
        .saturating_add(chunk_of(out_block))
        .saturating_add(in_block.saturating_mul(2))
}

/// The loop an unmeasured insert assumes: the one that makes its reported
/// latency exactly the law's round trip.
pub fn estimated_loop(out_block: u32, in_block: u32) -> u32 {
    law_frames(out_block, in_block).saturating_sub(chunk_of(out_block).saturating_add(in_block.saturating_mul(2)))
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
    /// The return was not quiet before the click: the click did not stand
    /// far enough above what was already coming back.
    Busy = 4,
    /// The bay did not run for so long after the ask (bypassed, stood
    /// down) that the ask was dropped rather than fired late.
    Expired = 5,
    /// The measured loop is longer than delay compensation can absorb.
    TooLong = 6,
}

impl PingFail {
    pub fn from_code(c: i64) -> Option<Self> {
        match c {
            -1 => Some(PingFail::Silent),
            -2 => Some(PingFail::Unclear),
            -3 => Some(PingFail::Disconnected),
            -4 => Some(PingFail::Busy),
            -5 => Some(PingFail::Expired),
            -6 => Some(PingFail::TooLong),
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
            PingFail::Busy => {
                "The return was not quiet before the click: stop playback, let the unit's tail die \
                 away, and ping again."
            }
            PingFail::Expired => "The ping was dropped: the insert was not running. Ping again.",
            PingFail::TooLong => {
                "The loop is longer than delay compensation can absorb (8192 samples): lower the \
                 buffer size or shorten the loop."
            }
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
/// times above the average, which is noise or a box still ringing; and,
/// given the loudest the return was in the quiet before the click (`quiet`),
/// one whose peak does not stand [`QUIET_RATIO`] times above it, because a
/// tail or a programme that loud can read as the click.
pub fn find_click(capture: &[f32], quiet: f32) -> Result<usize, PingFail> {
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
    if peak < QUIET_RATIO * quiet {
        return Err(PingFail::Busy);
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
    /// Device input frames received, the stale ones included: what has
    /// been written into the ports' return rings.
    pub in_clock: AtomicU64,
    /// **Device input frames PRODUCED by the chunk's start**: `in_clock`
    /// plus what was still waiting in the input ring. The lead the loops
    /// align by is taken from this, once a chunk, so it does not depend on
    /// how much the monitor had drained (a backlog) or on where in the chunk
    /// a bay runs (before or after the input pass).
    pub in_base: AtomicU64,
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
    /// The host's word that the input is a different device from the
    /// output (see [`same_device`]).
    pub separate: AtomicBool,
    /// The renderer's finding, this epoch, that the input's lead keeps
    /// moving: two clocks it was not told about.
    pub drifting: AtomicBool,
}

/// **Whether an input and an output name one device**: `Some(true)` one,
/// `Some(false)` two, `None` when the names cannot say (a system default).
///
/// CoreAudio and ASIO name both halves alike. WASAPI wraps them, and the
/// device is the LAST parenthesised group ("Analog (1+2) (RME Fireface)",
/// "Speakers (Scarlett 2i2 USB)"); comparing from the first bracket to the
/// last would read "1+2) (RME Fireface" and refuse one interface as two.
/// On Linux the output is often "default", "pipewire" or a JACK port while
/// the input names its card: those say nothing about which device plays,
/// so there is no verdict and the renderer's own clock check decides. An
/// aggregate device is one device.
pub fn same_device(input: &str, output: &str) -> Option<bool> {
    fn core(name: &str) -> &str {
        match name.rfind('(') {
            Some(a) => match name[a..].find(')') {
                Some(b) if b > 1 => &name[a + 1..a + b],
                _ => name,
            },
            None => name,
        }
    }
    fn generic(name: &str) -> bool {
        let n = name.trim().to_ascii_lowercase();
        ["default", "sysdefault", "pipewire", "pulse", "pulseaudio", "jack", "system"]
            .iter()
            .any(|g| n == *g || n.starts_with(&format!("{g}:")))
    }
    if generic(input) || generic(output) {
        return None;
    }
    Some(input == output || core(input) == core(output))
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
    /// Which claim holds it: bumped when a reload hands the port from the
    /// tenant going out to the one coming in, so the old claim's drop
    /// leaves it alone.
    gen: AtomicU32,
    /// How many claims hold it (a reload's new tenant and its old one, for
    /// the moment between them); the last to drop frees it.
    holders: AtomicU32,
    /// The output frame a ping was asked at (see [`PING_EXPIRE_S`]).
    ping_asked_at: AtomicU64,
    /// The unit's own finding that its loop is past [`PDC_LIMIT`].
    over: AtomicBool,
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
            gen: AtomicU32::new(0),
            holders: AtomicU32::new(0),
            ping_asked_at: AtomicU64::new(0),
            over: AtomicBool::new(false),
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

    /// The link as the face should read it: the renderer's, or the unit's
    /// own refusal of a loop too long to compensate.
    pub fn link(&self) -> Link {
        let link = Link::from_u8(self.status.load(Ordering::Relaxed));
        if link.connected() && self.over.load(Ordering::Relaxed) {
            Link::TooLong
        } else {
            link
        }
    }

    /// Ask the unit for a ping, at output frame `now` (the hub's
    /// `out_clock`). Answered on [`HwPort::ping_answer`].
    pub fn ask_ping(&self, now: u64) {
        self.ping_asked_at.store(now, Ordering::Relaxed);
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

#[derive(Debug, Default, Clone, Copy)]
struct Seen {
    input: Option<u64>,
    varispeed: bool,
    /// The clock check: where its judging began, the window it is in and
    /// that window's lowest lead, and the last windows' lows.
    judge_from: u64,
    win_end: u64,
    win_low: i64,
    lows: [i64; DRIFT_KEEP],
    lows_n: usize,
    still: u32,
    /// Callbacks seen this stream: the first few are the start-up burst.
    blk_calls: u32,
    /// The callback size: the window's end and largest, the size a window
    /// last proposed, and the size the stream is keyed on.
    blk_end: u64,
    blk_max: u32,
    blk_cand: u32,
    blk_steady: u32,
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
    /// Input frames delivered and not yet read: what the input ring holds
    /// at the chunk's start.
    pub in_backlog: usize,
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
    ///
    /// **A bay that already holds one hands it on.** Every jack change and
    /// every state restore is a reload, and the new tenant is built while
    /// the old one still runs; claiming a second port for it would run a
    /// desk of sixteen loops out of ports on its first reload. So the port
    /// answering for `key` passes to the new claim, keeping its rings, its
    /// place and its ping road.
    ///
    /// **The handover is revocable until the new tenant runs.** Its
    /// generation moves at once, so the old tenant, still seated, passes dry
    /// and touches nothing on the port; but if the new one never takes the
    /// seat (the hand-off ring refused it) its drop puts the old generation,
    /// jacks and lane back and the old tenant carries on. The new tenant
    /// commits the moment it first runs (`HwInsert::process`).
    ///
    /// `None` when all [`HW_PORTS`] are taken by other bays.
    pub fn claim(&self, key: u32, lane: Option<usize>) -> Option<HwClaim> {
        let held = self
            .ports
            .iter()
            .find(|p| p.is_claimed() && p.key.load(Ordering::Relaxed) == key);
        let (port, prev) = match held {
            Some(p) => {
                // A ping the old tenant has running is left alone here: if
                // this handover is refused the old tenant finishes it. The
                // new tenant answers it when it commits (`HwClaim::commit`).
                let prev = Prev {
                    gen: p.gen.load(Ordering::Acquire),
                    send: p.send.load(Ordering::Relaxed),
                    ret: p.ret.load(Ordering::Relaxed),
                    lane: p.lane.load(Ordering::Relaxed),
                };
                (p, Some(prev))
            }
            None => {
                let p = self.ports.iter().find(|p| {
                    p.claimed
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                        .is_ok()
                })?;
                self.claimed.fetch_add(1, Ordering::Relaxed);
                let mut io = p.io.lock().unwrap_or_else(|e| e.into_inner());
                if io.send.len() != SEND_CAP * 2 {
                    io.send = vec![0.0; SEND_CAP * 2];
                } else {
                    io.send.fill(0.0);
                }
                if io.ret.len() != RET_CAP * 2 {
                    io.ret = vec![0.0; RET_CAP * 2];
                }
                io.ret_from = self.clocks.in_clock.load(Ordering::Relaxed);
                drop(io);
                p.status.store(Link::Waiting as u8, Ordering::Relaxed);
                p.slips.store(0, Ordering::Relaxed);
                p.pinging.store(false, Ordering::Relaxed);
                p.over.store(false, Ordering::Relaxed);
                // A fresh bay has asked nothing and been answered nothing:
                // the bay this port served before must not lend it a
                // measurement.
                p.ping_req.store(0, Ordering::Relaxed);
                p.ping_done.store(0, Ordering::Relaxed);
                p.ping_result.store(0, Ordering::Relaxed);
                p.adopted.store(0, Ordering::Relaxed);
                p.key.store(key, Ordering::Relaxed);
                (p, None)
            }
        };
        let gen = port.gen.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        port.holders.fetch_add(1, Ordering::AcqRel);
        port.lane.store(
            lane.and_then(|l| u32::try_from(l).ok()).unwrap_or(u32::MAX),
            Ordering::Relaxed,
        );
        Some(HwClaim {
            port: Arc::clone(port),
            clocks: Arc::clone(&self.clocks),
            count: Arc::clone(&self.claimed),
            gen,
            prev,
            committed: false,
        })
    }

    /// The counters, shared, for a bay that has no port to carry them.
    pub fn clocks_arc(&self) -> Arc<HwClocks> {
        Arc::clone(&self.clocks)
    }

    /// How many ports no bay holds.
    pub fn free_ports(&self) -> usize {
        self.ports.iter().filter(|p| !p.is_claimed()).count()
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
        c.in_block.store(dev.in_block, Ordering::Relaxed);
        c.rate.store(dev.rate, Ordering::Relaxed);
        let backlog = dev.in_backlog as u64;
        let out = c.out_clock.load(Ordering::Relaxed);
        let input = dev.input.map(|(id, _)| id);
        let rate = f64::from(dev.rate.max(8_000));
        let (rebase, drifting) = match self.seen.try_lock() {
            Ok(mut seen) => {
                // **The size the stream is keyed on.** The largest callback
                // in a window, adopted when two windows in a row agree:
                // WASAPI in event mode and CoreAudio converting a rate hand
                // over callbacks of varying length, and a figure keyed on
                // the last one would move every block (the desk re-aligning
                // each time) and read a measured loop as unmeasured half the
                // time; one keyed on the largest ever would never come down
                // when another client shrinks the device's buffer.
                //
                // **And at the start**: the first callbacks of a stream are
                // its start-up burst (WASAPI's above all, or a short first
                // CoreAudio callback), so they are passed over, the size is
                // provisional (the largest since) through the first window,
                // and the first window's largest is adopted at once rather
                // than after a second window agrees: keyed wrong for the
                // first seconds, a saved measurement would read unmeasured
                // and the desk would re-align a moment later, with a click.
                seen.blk_calls = seen.blk_calls.saturating_add(1);
                if seen.blk_calls > STARTUP_CALLBACKS {
                    if seen.blk_end == 0 {
                        seen.blk_end = out + (rate * BLOCK_WINDOW_S) as u64;
                    }
                    seen.blk_max = seen.blk_max.max(dev.out_block);
                    if out >= seen.blk_end {
                        let proposed = seen.blk_max;
                        if seen.blk_steady == 0 || proposed == seen.blk_cand {
                            seen.blk_steady = proposed;
                        }
                        seen.blk_cand = proposed;
                        seen.blk_max = 0;
                        seen.blk_end = out + (rate * BLOCK_WINDOW_S) as u64;
                    }
                    let keyed = if seen.blk_steady == 0 { seen.blk_max } else { seen.blk_steady };
                    c.out_block.store(keyed, Ordering::Relaxed);
                }
                let rebase = seen.input != input || seen.varispeed != dev.varispeed;
                if rebase {
                    seen.input = input;
                    seen.varispeed = dev.varispeed;
                    // Re-based so the input frame produced last stands one
                    // before the output frame about to render: the counters
                    // mean the same thing in every epoch, whatever the ring
                    // held when it began.
                    c.in_clock.store(out.saturating_sub(backlog), Ordering::Relaxed);
                    c.epoch.fetch_add(1, Ordering::Relaxed);
                }
                // A new epoch is a new pair of streams: the judgement starts
                // again. A bay starving and relocking is NOT a reason to:
                // the lead it judges (produced minus rendered) is the
                // device's, untouched by any bay's alignment, and a drift
                // that makes a bay starve every few seconds must still be
                // called.
                if rebase {
                    seen.judge_from = out + (rate * DRIFT_SETTLE_S) as u64;
                    seen.win_end = 0;
                    seen.lows_n = 0;
                    seen.still = 0;
                    c.drifting.store(false, Ordering::Relaxed);
                }
                // **The clock check, by trend.** On one crystal the input's
                // lead over the output wanders in a band about an input block
                // wide and its lowest point holds still; on two it walks
                // away. The low point of each window is compared with the
                // last ones: walking the same way, by more than a few frames,
                // window after window, is two clocks; one step and then
                // stillness (a hiccup) is not.
                let produced = c.in_clock.load(Ordering::Relaxed) + backlog;
                let here = produced as i64 - out as i64;
                if input.is_some() && !dev.varispeed && out >= seen.judge_from {
                    if seen.win_end == 0 {
                        seen.win_end = out + (rate * DRIFT_WINDOW_S) as u64;
                        seen.win_low = here;
                    }
                    seen.win_low = seen.win_low.min(here);
                    if out >= seen.win_end {
                        let low = seen.win_low;
                        if seen.lows_n == DRIFT_KEEP {
                            seen.lows.copy_within(1.., 0);
                            seen.lows_n -= 1;
                        }
                        let n = seen.lows_n;
                        seen.lows[n] = low;
                        seen.lows_n += 1;
                        if seen.lows_n == DRIFT_KEEP {
                            let was = c.drifting.load(Ordering::Relaxed);
                            let lows = seen.lows;
                            let verdict = drift_verdict(&lows, i64::from(dev.in_block), was, &mut seen.still);
                            c.drifting.store(verdict, Ordering::Relaxed);
                        }
                        seen.win_end = out + (rate * DRIFT_WINDOW_S) as u64;
                        seen.win_low = here;
                    }
                }
                (rebase, c.drifting.load(Ordering::Relaxed))
            }
            Err(_) => (false, c.drifting.load(Ordering::Relaxed)),
        };
        if rebase {
            let at = c.in_clock.load(Ordering::Relaxed);
            for p in self.claimed_ports() {
                if let Ok(mut io) = p.io.try_lock() {
                    io.ret_from = at;
                }
            }
        }
        c.in_base.store(c.in_clock.load(Ordering::Relaxed) + backlog, Ordering::Relaxed);
        if self.claimed.load(Ordering::Relaxed) == 0 {
            return;
        }
        let separate = c.separate.load(Ordering::Relaxed);
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
                                    } else if separate {
                                        Link::SeparateDevice
                                    } else if drifting {
                                        Link::ClockDrift
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
    gen: u32,
    /// What the port was before a handover gave it to this claim.
    prev: Option<Prev>,
    /// Whether this claim's tenant has run: past that, a handover is final.
    committed: bool,
}

/// A port as the tenant before a handover had it.
#[derive(Debug, Clone, Copy)]
struct Prev {
    gen: u32,
    send: u32,
    ret: u32,
    lane: u32,
}

impl HwClaim {
    /// **The new tenant takes the port for good**, on its first block. A
    /// ping the tenant before it had running ends here, answered as not
    /// running: that tenant has stopped touching the port and nobody else
    /// would answer it. Done once; later calls do nothing.
    fn commit(&mut self) {
        if self.committed {
            return;
        }
        self.committed = true;
        let p = &self.port;
        if self.prev.is_some() && p.pinging.swap(false, Ordering::Relaxed) {
            p.ping_result.store(PingFail::Expired.code(), Ordering::Relaxed);
            p.ping_done.store(p.ping_req.load(Ordering::Relaxed), Ordering::Release);
        }
    }

    /// Whether the port is this claim's to touch: it is still claimed and
    /// no newer tenant of the same bay has taken it over.
    pub fn current(&self) -> bool {
        self.port.is_claimed() && self.port.gen.load(Ordering::Acquire) == self.gen
    }

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
        let left = p.holders.fetch_sub(1, Ordering::AcqRel).saturating_sub(1);
        if left > 0 {
            // Another tenant of the bay still holds it. If this was the new
            // one and it never ran, the handover is taken back: the old
            // tenant's generation, jacks and lane, and it carries on.
            if !self.committed && p.gen.load(Ordering::Acquire) == self.gen {
                if let Some(prev) = self.prev {
                    p.send.store(prev.send, Ordering::Relaxed);
                    p.ret.store(prev.ret, Ordering::Relaxed);
                    p.lane.store(prev.lane, Ordering::Relaxed);
                    p.gen.store(prev.gen, Ordering::Release);
                }
            }
            return;
        }
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

#[derive(Clone, Copy, Debug, PartialEq)]
enum Ping {
    Idle,
    /// The send is silent while the box lets go; fires when `left` reaches
    /// 0. `scan` is the next return frame to look at, and `quiet` the
    /// loudest the return was over the settle's last [`QUIET_S`].
    Settle { left: usize, scan: u64, quiet: f32 },
    /// The click went out; capturing the return from input frame `from`.
    /// `dip` is how far the input stood below its usual lead when it fired
    /// (see [`HwInsert::lead`]), taken off the answer.
    Listen { from: u64, got: usize, dip: u32, quiet: f32 },
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
    /// The engine's counters: from the claim, or handed over on their own
    /// to a bay seated without a port, which still has to tell its two
    /// passes in a chunk apart.
    clocks: Option<Arc<HwClocks>>,
    /// The dry signal's delay line, interleaved stereo: the bay's output is
    /// late by its reported latency whether the loop is up or not, so a
    /// disconnected insert passes dry IN TIME.
    dry: Vec<f32>,
    dry_at: usize,
    /// The second pass's own delay line (a send destination's rack runs
    /// twice a chunk): its dry share is as late as the first pass's, which
    /// is what the bay tells delay compensation.
    dry2: Vec<f32>,
    dry2_at: usize,
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
    /// The output frame the programme started going round the loop again
    /// (after a load, a bypass, a ping, a disconnect): until its return can
    /// have come back, the bay plays the delayed dry instead of an empty
    /// return, so going into circuit is seamless and in time.
    sent_from: Option<u64>,
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
        let dry_frames = PDC_LIMIT as usize + MAX_CHUNK as usize + 1;
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
            clocks: None,
            dry: vec![0.0; dry_frames * 2],
            dry_at: 0,
            dry2: vec![0.0; dry_frames * 2],
            dry2_at: 0,
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
            sent_from: None,
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
            self.clocks = Some(Arc::clone(&c.clocks));
            c.set_routes(self.send, self.ret);
            self.ping_seen = c.port.ping_req.load(Ordering::Relaxed);
        }
        self.delta = None;
    }

    /// The engine's counters, for a bay that got no port.
    pub fn set_clocks(&mut self, clocks: Arc<HwClocks>) {
        if self.clocks.is_none() {
            self.clocks = Some(clocks);
        }
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
        self.dry2.fill(0.0);
        self.dry2_at = 0;
        self.delta = None;
        self.starved = 0;
        self.lead_seen = 0;
        self.last_c = None;
        self.sent_from = None;
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
        delay_step(&mut self.dry, &mut self.dry_at, l, r, delay)
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
        let first = self.chunk_mark(frames);
        let (wanted, loop_now, measured) = self.figure();
        let over = wanted > PDC_LIMIT;
        self.latency = wanted.min(PDC_LIMIT);
        let latency = self.latency;
        let Some(claim) = self.claim.as_mut() else {
            self.link = Link::NoPort;
            self.pass_dry(first, input, frames, out);
            return;
        };
        // **A port handed on to a newer tenant of this bay is not ours**:
        // pass dry and touch nothing on it. A port that is ours and runs is
        // this tenant's for good (see `HwHub::claim`).
        if !claim.current() {
            self.link = Link::Waiting;
            self.pass_dry(first, input, frames, out);
            return;
        }
        claim.commit();
        let clocks = Arc::clone(&claim.clocks);
        let port = Arc::clone(&claim.port);
        let c = clocks.out_clock.load(Ordering::Relaxed);
        // The renderer's word, not `HwPort::link`, which folds in the
        // over-limit flag this block is about to decide afresh.
        let link = Link::from_u8(port.status.load(Ordering::Relaxed));
        let (ob, ib, rate) = (
            clocks.out_block.load(Ordering::Relaxed),
            clocks.in_block.load(Ordering::Relaxed),
            clocks.rate.load(Ordering::Relaxed),
        );
        // **Past what the desk can compensate, refused**: a loop the PDC
        // can absorb only part of would leave the track partly late, so the
        // bay passes dry at the limit and says why.
        port.latency.store(latency, Ordering::Relaxed);
        port.loop_frames.store(loop_now, Ordering::Relaxed);
        port.measured.store(measured, Ordering::Relaxed);
        port.over.store(over, Ordering::Relaxed);
        let link = if over && link.connected() { Link::TooLong } else { link };
        self.link = link;

        // A ping asked for since the last block. One already under way
        // absorbs a second press (its answer answers both): restarting would
        // measure the first click as the second's. An ask that waited out
        // a bypass is dropped rather than fired late.
        if first {
            let req = port.ping_req.load(Ordering::Acquire);
            if req != self.ping_seen {
                self.ping_seen = req;
                let asked = port.ping_asked_at.load(Ordering::Relaxed);
                let stale = c.saturating_sub(asked) as f64 > f64::from(self.rate) * PING_EXPIRE_S;
                if self.ping != Ping::Idle {
                    // Absorbed: the ping under way answers this ask too.
                } else if stale {
                    self.finish_ping(PingFail::Expired.code());
                } else if link.connected() {
                    port.pinging.store(true, Ordering::Relaxed);
                    self.ping = Ping::Settle {
                        left: (f64::from(self.rate) * PING_SETTLE_S) as usize,
                        scan: clocks.in_base.load(Ordering::Relaxed),
                        quiet: 0.0,
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
            self.pass_dry(first, input, frames, out);
            return;
        }

        let Ok(mut io) = port.io.try_lock() else {
            // Only the host's claim takes this lock, for the moment it sizes
            // the rings; a block that meets it passes dry.
            self.pass_dry(first, input, frames, out);
            return;
        };
        if io.send.len() < SEND_CAP * 2 || io.ret.len() < RET_CAP * 2 {
            drop(io);
            self.pass_dry(first, input, frames, out);
            return;
        }
        let in_now = clocks.in_clock.load(Ordering::Relaxed);
        let epoch = clocks.epoch.load(Ordering::Relaxed);

        if !first {
            // **A second pass in the same chunk** (a send destination's rack
            // is run again over its arrivals). The loop is one cable: what
            // this pass sends joins the first pass's send, and comes back in
            // the return that the FIRST pass reads, next chunk on. So this
            // pass sends and adds no wet of its own; its dry share goes
            // through its own delay line, as late as the first pass's.
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
                let (dl, dr) = self.dry2_step(l, r, latency as usize);
                if let Some(d) = out.get_mut(0).and_then(|ch| ch.get_mut(f)) {
                    *d = dl * keep;
                }
                if let Some(d) = out.get_mut(1).and_then(|ch| ch.get_mut(f)) {
                    *d = dr * keep;
                }
            }
            return;
        }

        if self.lock_epoch != epoch {
            self.lead_seen = 0;
            self.delta = None;
        }
        // From the chunk's base, not the live counter: a bay on a lane runs
        // before this chunk's input is read and one on the master after it,
        // and the alignment must not depend on which.
        let base = clocks.in_base.load(Ordering::Relaxed);
        let here = base as i64 - c as i64;
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
            Ping::Settle { left, scan, quiet } => {
                // What came back while the send was silent: its loudest over
                // the settle's last stretch is the quiet the click must
                // stand out of.
                let tail = (f64::from(self.rate) * QUIET_S) as usize;
                let mut quiet = quiet;
                let mut at = scan.max(io.ret_from);
                while at < in_now {
                    let i = (at % RET_CAP as u64) as usize * 2;
                    if left <= tail {
                        quiet = quiet.max(io.ret[i].abs()).max(io.ret[i + 1].abs());
                    }
                    at += 1;
                }
                if left == 0 {
                    fire = true;
                    let dip = u32::try_from((lead - here).max(0)).unwrap_or(u32::MAX);
                    self.ping = Ping::Listen { from: base, got: 0, dip, quiet };
                } else {
                    self.ping = Ping::Settle { left: left.saturating_sub(frames), scan: at, quiet };
                }
                true
            }
            Ping::Listen { .. } => true,
        };

        // The programme is going round the loop from here (or not, while a
        // ping has the send). See `sent_from`.
        if pinging {
            self.sent_from = None;
        } else if self.sent_from.is_none() {
            self.sent_from = Some(c);
        }
        let warm_from = self.sent_from.map(|s| s + u64::from(latency));
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
            // Before the loop carries the programme (just in circuit, or a
            // ping holding the send), the delayed dry stands in for the
            // wet: in time, and never a hole.
            let (ol, or) = if warm_from.is_none_or(|w| o < w) {
                (dl, dr)
            } else {
                let m = self.mix_g;
                (dl * (1.0 - m) + wl * g * m, dr * (1.0 - m) + wr * g * m)
            };
            if let Some(d) = out.get_mut(0).and_then(|ch| ch.get_mut(f)) {
                *d = ol;
            }
            if let Some(d) = out.get_mut(1).and_then(|ch| ch.get_mut(f)) {
                *d = or;
            }
        }

        // The ping's capture: every return frame that has arrived since the
        // click went out, as a magnitude, until the window is full.
        if let Ping::Listen { from, got, dip, quiet } = self.ping {
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
                // A click found before the reference could not have come
                // round the loop: something else was in the return.
                let found = find_click(&self.capture, quiet).and_then(|k| {
                    let k = u32::try_from(k).unwrap_or(u32::MAX);
                    let loop_frames = k.checked_sub(dip).ok_or(PingFail::Unclear)?;
                    if latency_for(loop_frames, ob, ib) > PDC_LIMIT {
                        return Err(PingFail::TooLong);
                    }
                    Ok(loop_frames)
                });
                match found {
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
                        self.latency = latency_for(m.loop_frames, ob, ib);
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
                self.ping = Ping::Listen { from, got, dip, quiet };
            }
        }

        if starved {
            self.starved += 1;
            port.slips.fetch_add(1, Ordering::Relaxed);
            if self.starved >= RELOCK_AFTER {
                // A new alignment from a fresh look at the lead: the window
                // still holds the old reference, and taking it again would
                // starve on for as long as it lasted.
                self.delta = None;
                self.lead_seen = 0;
            }
        } else {
            self.starved = 0;
        }
    }

    /// The second pass's dry share, through its own line.
    #[inline]
    fn dry2_step(&mut self, l: f32, r: f32, delay: usize) -> (f32, f32) {
        delay_step(&mut self.dry2, &mut self.dry2_at, l, r, delay)
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

    /// **The bay bypassed** (the owner's ruling): its input, delayed by
    /// exactly the latency it reports, through the same line the dry path
    /// uses, so switching bypass in or out mid-play keeps the timing. No
    /// send goes out and no ping runs: a ping under way, or asked while the
    /// bay was out, answers that the insert was not running.
    pub fn bypassed(&mut self, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        let first = self.chunk_mark(frames);
        // The figure, worked out here as in `process`: a bay that has been
        // bypassed since it loaded must already be as late as it will be in
        // circuit, or switching it in would jump (the rack publishes this).
        let (wanted, loop_now, measured) = self.figure();
        self.latency = wanted.min(PDC_LIMIT);
        if let Some(claim) = self.claim.as_mut().filter(|c| c.current()) {
            claim.commit();
            let port = Arc::clone(&claim.port);
            port.latency.store(self.latency, Ordering::Relaxed);
            port.loop_frames.store(loop_now, Ordering::Relaxed);
            port.measured.store(measured, Ordering::Relaxed);
            port.over.store(wanted > PDC_LIMIT, Ordering::Relaxed);
            let req = port.ping_req.load(Ordering::Acquire);
            if req != self.ping_seen || self.ping != Ping::Idle {
                self.ping_seen = req;
                self.finish_ping(PingFail::Expired.code());
            }
        }
        self.pass_dry(first, input, frames, out);
    }

    /// **Which pass of the chunk this is**: the first (or the only one) by
    /// the output counter, and a gap since the last chunk this bay ran
    /// clears both delay lines, which would otherwise replay the past. A
    /// bay with no counters at all is always on its first pass.
    fn chunk_mark(&mut self, frames: usize) -> bool {
        let Some(c) = self.clocks.as_ref().map(|k| k.out_clock.load(Ordering::Relaxed)) else {
            return true;
        };
        let first = self.last_c != Some(c);
        if first {
            if self.last_c.is_some() && c != self.next_c {
                self.dry.fill(0.0);
                self.dry2.fill(0.0);
            }
            self.last_c = Some(c);
            self.next_c = c + frames as u64;
        }
        first
    }

    /// **The bay's figure**: what it would report (before the PDC cap), the
    /// loop that assumes, and whether a ping measured it at the stream's
    /// steady rate and buffers. One place, for `process` and `bypassed`.
    fn figure(&self) -> (u32, u32, bool) {
        let Some(clocks) = self.clocks.as_ref() else {
            return match self.measured {
                Some(m) => (latency_for(m.loop_frames, m.out_block, m.in_block), m.loop_frames, false),
                None => (self.latency, 0, false),
            };
        };
        let (ob, ib, rate) = (
            clocks.out_block.load(Ordering::Relaxed),
            clocks.in_block.load(Ordering::Relaxed),
            clocks.rate.load(Ordering::Relaxed),
        );
        // The stream's size is not known yet (its start-up burst): a saved
        // measurement stands as it was saved, rather than reading unmeasured
        // for a block or two and moving the desk.
        if ob == 0 {
            if let Some(m) = self.measured {
                return (latency_for(m.loop_frames, m.out_block, m.in_block), m.loop_frames, true);
            }
        }
        let (loop_now, measured) = match self.measured {
            Some(m) if m.rate == rate && m.out_block == ob && m.in_block == ib => (m.loop_frames, true),
            _ => (estimated_loop(ob, ib), false),
        };
        (latency_for(loop_now, ob, ib), loop_now, measured)
    }

    /// The bay's input, delayed by its latency, as its output: what an
    /// insert with no loop does, in time with everything else.
    ///
    /// A second pass in the chunk (`first` false: a send destination's rack
    /// run again) goes through its own line, as the connected path's does,
    /// or the two passes would interleave in one line and garble both.
    fn pass_dry(&mut self, first: bool, input: &[Vec<f32>], frames: usize, out: &mut [Vec<f32>]) {
        if first {
            self.sent_from = None;
        }
        let latency = self.latency as usize;
        for f in 0..frames {
            let l = input.first().and_then(|ch| ch.get(f)).copied().unwrap_or(0.0);
            let r = input.get(1).and_then(|ch| ch.get(f)).copied().unwrap_or(l);
            let (dl, dr) = if first { self.dry_step(l, r, latency) } else { self.dry2_step(l, r, latency) };
            if let Some(d) = out.get_mut(0).and_then(|ch| ch.get_mut(f)) {
                *d = dl;
            }
            if let Some(d) = out.get_mut(1).and_then(|ch| ch.get_mut(f)) {
                *d = dr;
            }
        }
    }
}

/// One frame through an interleaved stereo delay line: write it, read the
/// one `delay` frames back.
#[inline]
fn delay_step(line: &mut [f32], at: &mut usize, l: f32, r: f32, delay: usize) -> (f32, f32) {
    let cap = line.len() / 2;
    let w = *at;
    line[w * 2] = l;
    line[w * 2 + 1] = r;
    let back = (w + cap - delay.min(cap - 1)) % cap;
    *at = (w + 1) % cap;
    (line[back * 2], line[back * 2 + 1])
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
                assert_eq!(find_click(&mag, 0.0), Ok(d), "delay {d}, shape {shape}");
            }
        }
        assert_eq!(find_click(&vec![0.0; n], 0.0), Err(PingFail::Silent));
        let busy: Vec<f32> = (0..n).map(|k| 0.3 + 0.01 * ((k % 7) as f32)).collect();
        assert_eq!(find_click(&busy, 0.0), Err(PingFail::Unclear));
    }

    /// A fake interface, one crystal for both directions: the renderer's
    /// three calls in the order a chunk makes them, a loop that carries the
    /// device's output channels back to its inputs `delay` frames later, and
    /// the bay's unit run in between.
    pub(crate) struct FakeDevice {
        pub hub: HwHub,
        pub out_ch: usize,
        pub in_ch: usize,
        /// The nominal callback, and the input's block unless `in_every`
        /// widens it.
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
        /// Produced by the input and not yet delivered by its driver.
        held: Vec<f32>,
        /// Delivered into the input ring and not yet read: the monitor's
        /// backlog.
        pending: Vec<f32>,
        blocks: usize,
        /// Whether the bay runs AFTER this chunk's input is read (the room
        /// pass, the master, a bus) rather than before it (a lane).
        pub after_input: bool,
        /// Frames the monitor leaves unread, chunk by chunk (cycled): a
        /// backlog that comes and goes.
        pub hold_back: Vec<usize>,
        /// The callback sizes, cycled; empty is always `block`.
        pub sizes: Vec<usize>,
        /// Two clocks: every `drift.abs()` output frames the input produces
        /// one frame more (positive) or one fewer (negative). 0 is one clock.
        pub drift: i64,
        /// A box that also plays a tone of this level back, whatever it is
        /// sent: a return that is never quiet.
        pub hum: f32,
        /// Input frames the driver loses, once, at the next delivery: a
        /// glitch that moves the input's lead for good.
        pub lose: usize,
        /// Every this many blocks the input's delivery is one block late (a
        /// late input callback), then catches up. 0 is never.
        pub late_every: usize,
        ticks: u64,
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
                pending: Vec::new(),
                blocks: 0,
                after_input: false,
                hold_back: Vec::new(),
                sizes: Vec::new(),
                drift: 0,
                hum: 0.0,
                lose: 0,
                late_every: 0,
                ticks: 0,
            }
        }

        /// The output frame a ping asked now would be stamped with.
        pub fn now(&self) -> u64 {
            self.hub.clocks().out_clock.load(Ordering::Relaxed)
        }

        /// The size of the next callback.
        pub fn next_frames(&self) -> usize {
            if self.sizes.is_empty() {
                self.block
            } else {
                self.sizes[self.blocks % self.sizes.len()]
            }
        }

        fn view(&self, frames: usize) -> DeviceView {
            DeviceView {
                out_channels: self.out_ch,
                input: Some((1, self.in_ch)),
                out_block: frames as u32,
                in_block: (self.block * self.in_every) as u32,
                rate: 48_000,
                varispeed: false,
                in_backlog: self.pending.len() / self.in_ch,
            }
        }

        /// The monitor's read: everything delivered but this chunk's hold-back.
        fn read_input(&mut self) {
            let hold = if self.hold_back.is_empty() {
                0
            } else {
                self.hold_back[self.blocks % self.hold_back.len()]
            };
            let frames = self.pending.len() / self.in_ch;
            let take = frames.saturating_sub(hold) * self.in_ch;
            let read: Vec<f32> = self.pending.drain(..take).collect();
            self.hub.feed(&read, &[], self.in_ch);
        }

        /// The rest of a callback whose chunk has already begun: the bay
        /// around the input's read, the fold, and the device's play and
        /// record. `frames` is that callback's size.
        pub fn blocks_step(&mut self, unit: &mut HwInsert, input: &[Vec<f32>], out: &mut [Vec<f32>], frames: usize) {
            self.rest(unit, input, out, frames);
        }

        /// One device callback: the chunk's start, the bay before or after
        /// the input is read, the fold, then the device plays the block and
        /// its input side records what comes back.
        pub fn block(&mut self, unit: &mut HwInsert, input: &[Vec<f32>], out: &mut [Vec<f32>]) {
            let frames = self.next_frames();
            let in_ch = self.in_ch;
            self.hub.begin_chunk(self.view(frames), |c| (c < in_ch).then_some(c));
            self.rest(unit, input, out, frames);
        }

        fn rest(&mut self, unit: &mut HwInsert, input: &[Vec<f32>], out: &mut [Vec<f32>], frames: usize) {
            let in_ch = self.in_ch;
            if self.after_input {
                self.read_input();
                unit.process(input, frames, out);
            } else {
                unit.process(input, frames, out);
                self.read_input();
            }
            let mut dev = vec![0.0f32; frames * self.out_ch];
            self.hub.fold(&mut dev, self.out_ch, frames);
            for f in 0..frames {
                let mut frame = vec![0.0f32; in_ch];
                for &(o, i, g) in &self.patch {
                    if o < self.out_ch && i < in_ch {
                        frame[i] += dev[f * self.out_ch + o] * g + self.hum * (self.ticks as f32 * 0.3).sin();
                    }
                }
                self.line.push_back(frame);
                let got = self.line.pop_front().unwrap_or_default();
                self.ticks += 1;
                let every = self.drift.unsigned_abs();
                let slip = every > 0 && self.ticks % every == 0;
                if slip && self.drift < 0 {
                    continue;
                }
                self.held.extend_from_slice(&got);
                if slip && self.drift > 0 {
                    self.held.extend_from_slice(&got);
                }
            }
            self.blocks += 1;
            let late = self.late_every > 0 && self.blocks % self.late_every == 0;
            if !late && self.blocks % self.in_every.max(1) == 0 {
                let lost = (self.lose * in_ch).min(self.held.len());
                self.lose -= lost / in_ch;
                self.held.drain(..lost);
                self.pending.append(&mut self.held);
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
        let mut heard = Vec::new();
        for k in 0..blocks {
            let b = dev.next_frames();
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
            dev.hub.port_for(7).expect("claimed").ask_ping(dev.now());
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
            dev.hub.port_for(7).unwrap().ask_ping(dev.now());
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
            dev.hub.port_for(7).unwrap().ask_ping(dev.now());
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
            // Past the stream's start-up callbacks, which the size is not
            // keyed on.
            for _ in 0..=STARTUP_CALLBACKS {
                dev.hub.begin_chunk(
                    DeviceView { in_block: in_b, ..dev.view(out_b as usize) },
                    |c| Some(c),
                );
            }
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
        dev.hub.port_for(7).unwrap().ask_ping(dev.now());
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
        dev.hub.port_for(7).unwrap().ask_ping(dev.now());
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
        // A reload of the same bay: the port is handed on, not doubled.
        let b = hub.claim(3, Some(0)).expect("the same port");
        assert!(std::ptr::eq(a.port(), b.port()), "a reload took a second port");
        assert_eq!(hub.free_ports(), HW_PORTS - 1);
        let mut lanes = [0u32; HW_PORTS];
        assert_eq!(hub.held_lanes(&mut lanes), 1);
        // The tenant going out lets go and the port stays the new one's.
        b.set_routes(Some(Jacks::pair(2)), Some(Jacks::pair(2)));
        drop(a);
        assert!(std::ptr::eq(hub.port_for(3).expect("still held"), b.port()));
        assert_eq!(b.port().routes().0, Some(Jacks::pair(2)), "the old tenant's drop cleared the new one's jacks");
        drop(b);
        assert!(hub.port_for(3).is_none());
        assert_eq!(hub.held_lanes(&mut lanes), 0);
        assert_eq!(hub.free_ports(), HW_PORTS);
        // Sixteen bays, each reloaded, never run out.
        let mut all: Vec<_> = (0..HW_PORTS as u32).map(|k| hub.claim(k, None).expect("a port")).collect();
        for k in 0..HW_PORTS as u32 {
            let next = hub.claim(k, None).expect("a reload of a full desk ran out of ports");
            all[k as usize] = next;
        }
        assert_eq!(hub.free_ports(), 0);
        assert!(hub.claim(99, None).is_none(), "a seventeenth loop got a port");
        drop(all);
        assert_eq!(hub.free_ports(), HW_PORTS);
    }

    /// Settle, ping, wait for the answer; the answer.
    fn ping(dev: &mut FakeDevice, u: &mut HwInsert) -> i64 {
        dev.hub.port_for(7).expect("claimed").ask_ping(dev.now());
        let mut n = 0;
        while dev.hub.port_for(7).expect("claimed").pinging() {
            let _ = run_impulse(dev, u, 1, usize::MAX);
            n += 1;
            assert!(n < 3_000, "the ping never finished");
        }
        dev.hub.port_for(7).expect("claimed").ping_answer().1
    }

    /// Where an impulse into the bay comes out, against where it went in.
    fn lands(dev: &mut FakeDevice, u: &mut HwInsert) -> usize {
        let start = 4;
        let mut at = 0;
        for k in 0..start {
            at += if dev.sizes.is_empty() { dev.block } else { dev.sizes[(dev.blocks + k) % dev.sizes.len()] };
        }
        let heard = run_impulse(dev, u, start + 120, start);
        heard.iter().position(|(l, _)| l.abs() > 0.5).expect("the impulse came back") - at
    }

    /// **Where in the chunk a bay runs does not move its loop.** A lane's
    /// rack runs before the chunk's input is read, the room's and the
    /// master's after it; a ping taken in one place and played in the other
    /// lands exactly at the reported latency both ways.
    #[test]
    fn a_bay_that_moves_across_the_input_pass_keeps_its_timing() {
        for (ping_after, play_after) in [(false, true), (true, false), (false, false), (true, true)] {
            let mut dev = FakeDevice::new(4, 4, 128, 61);
            dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
            let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
            dev.after_input = ping_after;
            let _ = run_impulse(&mut dev, &mut u, 8, usize::MAX);
            assert!(ping(&mut dev, &mut u) >= 0);
            dev.after_input = play_after;
            let _ = run_impulse(&mut dev, &mut u, 8, usize::MAX);
            assert_eq!(lands(&mut dev, &mut u), u.latency() as usize, "pinged after: {ping_after}, played after: {play_after}");
        }
    }

    /// **A backlog in the input ring does not move the loop**: the monitor
    /// leaves a varying number of frames unread chunk to chunk, and the
    /// measured loop and the landing are what they are with none.
    #[test]
    fn a_backlog_that_comes_and_goes_does_not_move_the_loop() {
        let clean = {
            let mut dev = FakeDevice::new(4, 4, 128, 90);
            dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
            let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
            let _ = run_impulse(&mut dev, &mut u, 8, usize::MAX);
            ping(&mut dev, &mut u)
        };
        for pattern in [vec![0usize, 37, 100, 5, 64, 0, 120], vec![64; 3], vec![120, 0]] {
            let mut dev = FakeDevice::new(4, 4, 128, 90);
            dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
            dev.hold_back = pattern.clone();
            let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
            let _ = run_impulse(&mut dev, &mut u, 8, usize::MAX);
            assert_eq!(ping(&mut dev, &mut u), clean, "the backlog {pattern:?} moved the measured loop");
            let slips = dev.hub.port_for(7).unwrap().slips();
            assert_eq!(lands(&mut dev, &mut u), u.latency() as usize, "{pattern:?}");
            assert_eq!(dev.hub.port_for(7).unwrap().slips(), slips, "{pattern:?} starved the loop");
        }
    }

    /// **Callbacks of varying length keep one latency**: the figure is
    /// keyed on the largest callback the stream has handed over, so it
    /// neither moves block to block nor flips a measured loop to unmeasured.
    #[test]
    fn callbacks_of_varying_length_keep_one_latency() {
        let mut dev = FakeDevice::new(4, 4, 128, 40);
        dev.sizes = vec![128, 96, 128, 64, 112, 128];
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut u, 12, usize::MAX);
        assert!(ping(&mut dev, &mut u) >= 0);
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..60 {
            let _ = run_impulse(&mut dev, &mut u, 1, usize::MAX);
            seen.insert((u.latency(), dev.hub.port_for(7).unwrap().loop_frames().1));
        }
        assert_eq!(seen.len(), 1, "the latency or its measured flag moved: {seen:?}");
        assert!(seen.iter().all(|(_, measured)| *measured));
        assert_eq!(lands(&mut dev, &mut u), u.latency() as usize);
    }

    /// **Two clocks are found and refused, either way round**: an input
    /// running fast or slow against the output walks the lead away, the
    /// link says so, and the bay passes dry in time rather than crackling.
    #[test]
    fn an_input_on_another_clock_is_found_and_passes_dry() {
        // 100 ppm either way: one frame in ten thousand.
        for drift in [10_000i64, -10_000] {
            let mut dev = FakeDevice::new(4, 4, 128, 30);
            dev.drift = drift;
            dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
            let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
            let mut n = 0;
            while u.link() != Link::ClockDrift {
                let _ = run_impulse(&mut dev, &mut u, 1, usize::MAX);
                n += 1;
                assert!(n < 12_000, "drift {drift}: never found ({:?})", u.link());
            }
            let latency = u.latency() as usize;
            assert_eq!(lands(&mut dev, &mut u), latency, "drift {drift}: not dry in time");
        }
        // One clock never trips it, however long it runs.
        let mut dev = FakeDevice::new(4, 4, 128, 30);
        dev.in_every = 3;
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut u, 2_000, usize::MAX);
        assert_eq!(u.link(), Link::Connected);
        // And the host's word that the input is another device.
        dev.hub.clocks().separate.store(true, Ordering::Relaxed);
        let _ = run_impulse(&mut dev, &mut u, 1, usize::MAX);
        assert_eq!(u.link(), Link::SeparateDevice);
        assert_eq!(same_device("Scarlett 18i20 USB", "Scarlett 18i20 USB"), Some(true));
        assert_eq!(same_device("Analogue 1 + 2 (Scarlett 2i2 USB)", "Speakers (Scarlett 2i2 USB)"), Some(true));
        assert_eq!(same_device("MacBook Pro Microphone", "Scarlett 18i20 USB"), Some(false));
    }

    /// **A loop the desk cannot compensate is refused**: an estimate past
    /// the PDC limit links TooLong and passes dry at the limit, and a ping
    /// that measures one refuses it.
    #[test]
    fn a_loop_past_the_compensation_limit_is_refused() {
        // 4096-frame buffers: the law alone is 12288 frames.
        let mut dev = FakeDevice::new(4, 4, 4096, 30);
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut u, 3, usize::MAX);
        assert_eq!(u.link(), Link::TooLong);
        assert_eq!(dev.hub.port_for(7).unwrap().link(), Link::TooLong);
        assert_eq!(u.latency(), PDC_LIMIT);
        // A 9000-frame cable at small buffers: the ping says so.
        let mut dev = FakeDevice::new(4, 4, 128, 9_000);
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut u, 8, usize::MAX);
        assert_eq!(PingFail::from_code(ping(&mut dev, &mut u)), Some(PingFail::TooLong));
        assert!(u.measured().is_none());
    }

    /// **A ping refuses what it cannot trust**: a return still loud with
    /// programme when the click fires, an ask that waited out a stood-down
    /// bay, and a second press while the first is listening (absorbed: one
    /// click, one answer for both).
    #[test]
    fn a_ping_refuses_a_busy_return_a_stale_ask_and_a_double_press() {
        // Busy: a box that keeps playing a tone back whatever it is sent.
        let mut dev = FakeDevice::new(4, 4, 128, 20);
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut u, 8, usize::MAX);
        dev.hub.port_for(7).unwrap().ask_ping(dev.now());
        dev.hum = 0.4;
        let mut n = 0u32;
        while dev.hub.port_for(7).unwrap().pinging() {
            let _ = run_impulse(&mut dev, &mut u, 1, usize::MAX);
            n += 1;
            assert!(n < 3_000);
        }
        assert_eq!(PingFail::from_code(dev.hub.port_for(7).unwrap().ping_answer().1), Some(PingFail::Busy));

        // Stale: asked, then the bay does not run for three seconds.
        let mut dev = FakeDevice::new(4, 4, 128, 20);
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut u, 8, usize::MAX);
        dev.hub.port_for(7).unwrap().ask_ping(dev.now());
        let mut idle = HwInsert::new(48_000.0);
        let _ = run_impulse(&mut dev, &mut idle, 48_000 * 3 / 128, usize::MAX);
        let start = dev.last_out.len();
        let _ = run_impulse(&mut dev, &mut u, 2, usize::MAX);
        let _ = start;
        assert_eq!(PingFail::from_code(dev.hub.port_for(7).unwrap().ping_answer().1), Some(PingFail::Expired));
        assert!(!dev.hub.port_for(7).unwrap().pinging());

        // A double press: one click goes out, one answer, the right one.
        let mut dev = FakeDevice::new(4, 4, 128, 77);
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut u, 8, usize::MAX);
        dev.hub.port_for(7).unwrap().ask_ping(dev.now());
        let mut clicks = 0;
        let mut n = 0;
        while dev.hub.port_for(7).unwrap().pinging() {
            if n == 70 {
                dev.hub.port_for(7).unwrap().ask_ping(dev.now());
            }
            let _ = run_impulse(&mut dev, &mut u, 1, usize::MAX);
            clicks += dev.last_out.chunks(4).filter(|f| f[2] > 0.4).count();
            n += 1;
            assert!(n < 3_000);
        }
        assert_eq!(clicks, 1, "a second press fired a second click");
        let (seq, result) = dev.hub.port_for(7).unwrap().ping_answer();
        assert_eq!(seq, 2, "the second press was left unanswered");
        assert!(result >= 0);
        assert_eq!(lands(&mut dev, &mut u), u.latency() as usize);
    }

    /// **A glitch that moves the input for good is recovered from at
    /// once**: the input loses 300 frames, the loop starves, and within a
    /// few blocks it has a new alignment, because the relock takes a fresh
    /// look at the lead rather than the window that still holds the old one.
    #[test]
    fn a_lost_stretch_of_input_is_recovered_from_quickly() {
        let mut dev = FakeDevice::new(4, 4, 128, 30);
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut u, 80, usize::MAX);
        dev.lose = 300;
        let before = dev.hub.port_for(7).unwrap().slips();
        let _ = run_impulse(&mut dev, &mut u, 30, usize::MAX);
        let after = dev.hub.port_for(7).unwrap().slips();
        assert!(after > before, "fixture: the loss did not starve the loop");
        assert!(after - before <= RELOCK_AFTER + 2, "starved {} blocks before it recovered", after - before);
        let _ = run_impulse(&mut dev, &mut u, 30, usize::MAX);
        assert_eq!(dev.hub.port_for(7).unwrap().slips(), after, "it went on starving");
        assert_eq!(u.link(), Link::Connected);
    }

    /// **A one-off step in the input is not two clocks.** An underrun or a
    /// USB hiccup moves the input's lead once and then holds it: the loop
    /// relocks and stays connected for good, however long it runs after.
    #[test]
    fn a_one_off_step_in_the_input_is_not_two_clocks() {
        let mut dev = FakeDevice::new(4, 4, 128, 30);
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        // Well into the judging, then a step of 900 frames (more than the
        // old range band) and half a minute more.
        let _ = run_impulse(&mut dev, &mut u, 48_000 * 10 / 128, usize::MAX);
        let before = lands(&mut dev, &mut u);
        dev.lose = 900;
        for _ in 0..(48_000 * 30 / 128) {
            let _ = run_impulse(&mut dev, &mut u, 1, usize::MAX);
            assert_ne!(u.link(), Link::ClockDrift, "a hiccup was called two clocks");
        }
        assert_eq!(u.link(), Link::Connected);
        assert_eq!(lands(&mut dev, &mut u), before, "the loop did not come back in time");
    }

    /// **The buffer the stream is keyed on follows a real change both ways
    /// and ignores one odd callback.** Another client resizing the device
    /// 512 to 128 (or back) moves the figure within two seconds, so a
    /// measurement taken at the old size reads unmeasured instead of landing
    /// early; one oversized callback moves nothing.
    #[test]
    fn the_buffer_size_follows_a_real_change_and_ignores_a_spike() {
        for (from, to) in [(512usize, 128usize), (128, 512)] {
            let mut dev = FakeDevice::new(4, 4, from, 30);
            dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
            let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
            let _ = run_impulse(&mut dev, &mut u, 48_000 * 3 / from, usize::MAX);
            assert!(ping(&mut dev, &mut u) >= 0);
            assert!(dev.hub.port_for(7).unwrap().loop_frames().1, "{from}: unmeasured after a ping");
            dev.block = to;
            let _ = run_impulse(&mut dev, &mut u, 48_000 * 3 / to, usize::MAX);
            assert_eq!(dev.hub.clocks().out_block.load(Ordering::Relaxed), to as u32, "{from} to {to}: the size did not follow");
            assert!(!dev.hub.port_for(7).unwrap().loop_frames().1, "{from} to {to}: a stale measurement still matched");
            assert!(ping(&mut dev, &mut u) >= 0);
            assert_eq!(lands(&mut dev, &mut u), u.latency() as usize, "{from} to {to}");
        }
        let mut dev = FakeDevice::new(4, 4, 128, 30);
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut u, 48_000 * 3 / 128, usize::MAX);
        assert!(ping(&mut dev, &mut u) >= 0);
        let latency = u.latency();
        dev.sizes = std::iter::once(512).chain(std::iter::repeat_n(128, 1_000)).collect();
        for _ in 0..1_000 {
            let _ = run_impulse(&mut dev, &mut u, 1, usize::MAX);
            assert_eq!(u.latency(), latency, "one oversized callback moved the figure");
        }
    }

    /// **A bus's two passes stay apart when the bay is not in its loop**:
    /// bypassed, disconnected or with no port, the second pass of a chunk
    /// goes through its own delay line, so each pass comes out as its own
    /// input delayed by the latency, not the two interleaved.
    #[test]
    fn two_passes_stay_apart_when_the_bay_is_out_of_its_loop() {
        for case in 0..3 {
            let mut dev = FakeDevice::new(4, 4, 128, 20);
            let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
            if case == 1 {
                u.set_routes(None, Some(Jacks::pair(2)));
            }
            if case == 2 {
                u.attach(None);
                u.set_clocks(dev.hub.clocks_arc());
            }
            let mut a = Vec::new();
            let mut b = Vec::new();
            for k in 0..20 {
                let n = dev.next_frames();
                let first_in = vec![vec![if k == 4 { 1.0 } else { 0.0 }; n]; 2];
                let second_in = vec![vec![if k == 6 { 0.5 } else { 0.0 }; n]; 2];
                let in_ch = dev.in_ch;
                dev.hub.begin_chunk(dev.view(n), |c| (c < in_ch).then_some(c));
                let mut o1 = vec![vec![0.0f32; n]; 2];
                let mut o2 = vec![vec![0.0f32; n]; 2];
                if case == 0 {
                    u.bypassed(&first_in, n, &mut o1);
                    u.bypassed(&second_in, n, &mut o2);
                } else {
                    u.process(&first_in, n, &mut o1);
                    u.process(&second_in, n, &mut o2);
                }
                dev.read_input();
                let mut d = vec![0.0f32; n * 4];
                dev.hub.fold(&mut d, 4, n);
                a.extend_from_slice(&o1[0]);
                b.extend_from_slice(&o2[0]);
            }
            let l = u.latency() as usize;
            assert!(a.iter().all(|v| *v == 0.0 || *v == 1.0), "case {case}: the first pass carries the second's");
            assert!(b.iter().all(|v| *v == 0.0 || *v == 0.5), "case {case}: the second pass carries the first's");
            assert_eq!(a.iter().position(|v| *v == 1.0), Some(4 * 128 + l), "case {case}");
            assert_eq!(b.iter().position(|v| *v == 0.5), Some(6 * 128 + l), "case {case}");
        }
    }

    /// **A handover that never lands is taken back.** A reload claims the
    /// bay's port for its new tenant while the old one still runs (it then
    /// passes dry and leaves the port alone); if the new tenant never takes
    /// the seat, its drop gives the port, its jacks and its ping road back
    /// to the old one, which carries on as it was.
    #[test]
    fn a_handover_that_never_lands_is_taken_back() {
        let mut dev = FakeDevice::new(6, 6, 128, 30);
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut old = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut old, 8, usize::MAX);
        let looped = lands(&mut dev, &mut old);
        let gen = dev.hub.port_for(7).unwrap().gen.load(Ordering::Relaxed);
        // The reload's new tenant, on other jacks, built and never seated.
        let mut new = HwInsert::new(48_000.0);
        new.set_routes(Some(Jacks::pair(4)), Some(Jacks::pair(4)));
        new.attach(dev.hub.claim(7, Some(1)));
        assert_eq!(dev.hub.free_ports(), HW_PORTS - 1, "the reload took a second port");
        // Meanwhile the old tenant leaves the port alone and passes dry.
        let steady = vec![vec![0.25f32; 128]; 2];
        let mut o = vec![vec![0.0f32; 128]; 2];
        for _ in 0..2 {
            dev.block(&mut old, &steady, &mut o);
            assert!(dev.last_out.iter().all(|v| *v == 0.0), "the superseded tenant still sent");
        }
        // The hand-off is refused: the new tenant drops without running.
        drop(new);
        let port = dev.hub.port_for(7).expect("the old tenant's port");
        assert_eq!(port.gen.load(Ordering::Relaxed), gen);
        assert_eq!(port.routes(), (Some(Jacks::pair(2)), Some(Jacks::pair(2))));
        let _ = run_impulse(&mut dev, &mut old, 8, usize::MAX);
        assert_eq!(old.link(), Link::Connected);
        assert_eq!(lands(&mut dev, &mut old), looped, "the old tenant did not carry on in time");
        // A handover that lands is final: the new tenant runs, the old one
        // drops, and the port stays the new one's.
        let mut next = HwInsert::new(48_000.0);
        next.set_routes(Some(Jacks::pair(2)), Some(Jacks::pair(2)));
        next.attach(dev.hub.claim(7, Some(1)));
        let _ = run_impulse(&mut dev, &mut next, 4, usize::MAX);
        drop(old);
        assert!(dev.hub.port_for(7).is_some(), "the old tenant's drop freed the new one's port");
        assert_eq!(dev.hub.free_ports(), HW_PORTS - 1);
        drop(next);
        assert_eq!(dev.hub.free_ports(), HW_PORTS);
    }

    /// **A reload does not start a ping the old tenant never asked for, or
    /// leave one hanging.** The ping road is not reset under a running
    /// tenant, and a ping the old tenant had running when the bay reloaded
    /// is answered rather than left pinging for ever.
    #[test]
    fn a_reload_neither_starts_nor_strands_a_ping() {
        let mut dev = FakeDevice::new(4, 4, 128, 30);
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut old = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut old, 8, usize::MAX);
        assert!(ping(&mut dev, &mut old) >= 0);
        // A reload claims (and is refused); the old tenant must not read the
        // claim as a new ask.
        let new = {
            let mut n = HwInsert::new(48_000.0);
            n.attach(dev.hub.claim(7, Some(1)));
            n
        };
        drop(new);
        let _ = run_impulse(&mut dev, &mut old, 20, usize::MAX);
        assert!(!dev.hub.port_for(7).unwrap().pinging(), "the old tenant started a ping nobody asked for");
        // A ping running when the bay reloads is answered.
        dev.hub.port_for(7).unwrap().ask_ping(dev.now());
        let _ = run_impulse(&mut dev, &mut old, 3, usize::MAX);
        assert!(dev.hub.port_for(7).unwrap().pinging());
        let mut next = HwInsert::new(48_000.0);
        next.set_routes(Some(Jacks::pair(2)), Some(Jacks::pair(2)));
        next.attach(dev.hub.claim(7, Some(1)));
        let _ = run_impulse(&mut dev, &mut next, 3, usize::MAX);
        assert!(!dev.hub.port_for(7).unwrap().pinging(), "a ping was left hanging across a reload");
    }

    #[test]
    fn device_names_say_one_device_two_or_nothing() {
        assert_eq!(same_device("Analog (1+2) (RME Fireface UCX)", "Analog (3+4) (RME Fireface UCX)"), Some(true));
        assert_eq!(same_device("Analogue 1 + 2 (Focusrite USB Audio)", "Speakers (Focusrite USB Audio)"), Some(true));
        assert_eq!(same_device("Microphone (USB Mic)", "Speakers (Realtek Audio)"), Some(false));
        for generic in ["default", "pipewire", "jack", "sysdefault:CARD=PCH", "Default"] {
            assert_eq!(same_device("Scarlett 18i20 USB", generic), None, "{generic}");
            assert_eq!(same_device(generic, "Scarlett 18i20 USB"), None, "{generic}");
        }
    }

    /// Run until the bay's link reads `want` or `limit` seconds pass, and
    /// how many blocks that took. A second bay on the same hub runs beside
    /// it, the way a desk has several.
    fn until_link(dev: &mut FakeDevice, u: &mut HwInsert, other: &mut HwInsert, want: Link, limit_s: usize) -> Option<usize> {
        let blocks = 48_000 * limit_s / dev.block;
        for k in 0..blocks {
            let n = dev.next_frames();
            let input = vec![vec![0.1f32; n]; 2];
            let mut out = vec![vec![0.0f32; n]; 2];
            let in_ch = dev.in_ch;
            dev.hub.begin_chunk(dev.view(n), |c| (c < in_ch).then_some(c));
            let mut o2 = vec![vec![0.0f32; n]; 2];
            other.process(&input, n, &mut o2);
            dev.blocks_step(u, &input, &mut out, n);
            if u.link() == want {
                return Some(k);
            }
        }
        None
    }

    /// **An input that falls behind is called two clocks even while it makes
    /// the loop starve.** At small buffers a drift starves the bay every few
    /// seconds; a starve used to restart the judgement (for every bay on the
    /// engine), so the verdict never came. It must, at 100 and 300 ppm, at
    /// 64- and 32-frame buffers, with another bay on the hub beside it.
    #[test]
    fn an_input_falling_behind_is_called_two_clocks_through_its_starves() {
        for block in [64usize, 32] {
            for every in [10_000i64, 3_333] {
                let mut dev = FakeDevice::new(6, 6, block, 30);
                dev.drift = -every;
                dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0), (4, 4, 1.0), (5, 5, 1.0)];
                let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
                let mut other = HwInsert::new(48_000.0);
                other.set_routes(Some(Jacks::pair(4)), Some(Jacks::pair(4)));
                other.attach(dev.hub.claim(8, Some(2)));
                let found = until_link(&mut dev, &mut u, &mut other, Link::ClockDrift, 40);
                assert!(found.is_some(), "{block}-frame buffers, 1 in {every}: never called");
                assert!(dev.hub.port_for(7).unwrap().slips() > 0, "fixture: the drift never starved the loop");
                assert_eq!(other.link(), Link::ClockDrift, "the other bay was not called with it");
            }
        }
    }

    /// **The verdict does not flap, and one clock with late callbacks is
    /// never called.** A late input callback drops one window's low by a
    /// block: one clock with those every dozen seconds stays connected for a
    /// minute, and 100 ppm with the same stays called once it is called.
    #[test]
    fn late_callbacks_neither_trip_one_clock_nor_flap_a_drift() {
        let mut dev = FakeDevice::new(6, 6, 128, 30);
        dev.late_every = 4_700;
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let mut other = HwInsert::new(48_000.0);
        assert_eq!(until_link(&mut dev, &mut u, &mut other, Link::ClockDrift, 60), None, "one clock was called two");
        let mut dev = FakeDevice::new(6, 6, 128, 30);
        dev.late_every = 4_700;
        dev.drift = 10_000;
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        assert!(until_link(&mut dev, &mut u, &mut other, Link::ClockDrift, 40).is_some(), "100 ppm was never called");
        assert_eq!(until_link(&mut dev, &mut u, &mut other, Link::Connected, 40), None, "the verdict flapped back");
    }

    /// The verdict itself, on lows written down.
    #[test]
    fn the_drift_verdict_walks_ignores_outliers_and_holds() {
        let mut still = 0;
        // Still: no.
        assert!(!drift_verdict(&[0, 0, 0, 0, 0], 128, false, &mut still));
        // 100 ppm (19 a window) either way: yes.
        assert!(drift_verdict(&[0, 19, 38, 57, 76], 128, false, &mut still));
        assert!(drift_verdict(&[0, -19, -38, -57, -76], 128, false, &mut still));
        // A late callback in one window, one clock: no.
        assert!(!drift_verdict(&[0, -128, 0, 0, 0], 128, false, &mut still));
        assert!(!drift_verdict(&[0, 0, -130, 0, 0], 128, false, &mut still));
        // The same under a drift: still yes.
        assert!(drift_verdict(&[0, -19, -170, -57, -76], 128, false, &mut still));
        // A one-off step: no.
        assert!(!drift_verdict(&[0, 0, -900, -900, -900], 128, false, &mut still));
        // Called, then one odd window: stays called; still for two verdicts:
        // released.
        let mut still = 0;
        assert!(drift_verdict(&[0, 19, 19, 19, 19], 128, true, &mut still));
        assert!(drift_verdict(&[19, 19, 19, 19, 19], 128, true, &mut still));
        assert!(!drift_verdict(&[19, 19, 19, 19, 19], 128, true, &mut still));
    }

    /// **A stream's start-up burst does not key the size.** The first
    /// callbacks come in bigger than the stream runs; a bay reopened with a
    /// measurement taken at the stream's real size reports that figure from
    /// its first block and never moves, and lands exactly.
    #[test]
    fn a_start_up_burst_does_not_move_a_measured_figure() {
        // The loop measured on a settled stream.
        let mut dev = FakeDevice::new(4, 4, 128, 30);
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut u, 400, usize::MAX);
        assert!(ping(&mut dev, &mut u) >= 0);
        let m = u.measured().expect("measured");
        let figure = u.latency();
        // Reopened on a new stream that starts with a burst of big callbacks.
        let mut dev = FakeDevice::new(4, 4, 128, 30);
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        dev.sizes = [512, 512].into_iter().chain(std::iter::repeat_n(128, 2_000)).collect();
        let mut u = HwInsert::new(48_000.0);
        u.set_routes(Some(Jacks::pair(2)), Some(Jacks::pair(2)));
        u.set_measured(Some(m));
        u.attach(dev.hub.claim(7, Some(1)));
        for k in 0..1_500 {
            let _ = run_impulse(&mut dev, &mut u, 1, usize::MAX);
            assert_eq!(u.latency(), figure, "block {k}: the figure moved");
            assert!(dev.hub.port_for(7).unwrap().loop_frames().1, "block {k}: the measurement read unmeasured");
        }
        assert_eq!(lands(&mut dev, &mut u), figure as usize);
    }

    /// **A refused reload leaves the old tenant's ping to it.** The claim
    /// does not answer the ping; the old tenant, back on its port, finishes
    /// it with a real answer, and the port reads pinging until it does.
    #[test]
    fn a_refused_reload_leaves_the_old_tenants_ping_running() {
        let mut dev = FakeDevice::new(4, 4, 128, 30);
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut old = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let _ = run_impulse(&mut dev, &mut old, 8, usize::MAX);
        dev.hub.port_for(7).unwrap().ask_ping(dev.now());
        let _ = run_impulse(&mut dev, &mut old, 80, usize::MAX);
        assert!(dev.hub.port_for(7).unwrap().pinging());
        let new = {
            let mut n = HwInsert::new(48_000.0);
            n.attach(dev.hub.claim(7, Some(1)));
            n
        };
        assert!(dev.hub.port_for(7).unwrap().pinging(), "the claim cancelled the old tenant's ping");
        drop(new);
        let mut n = 0;
        while dev.hub.port_for(7).unwrap().pinging() {
            let _ = run_impulse(&mut dev, &mut old, 1, usize::MAX);
            n += 1;
            assert!(n < 3_000);
        }
        let (_, result) = dev.hub.port_for(7).unwrap().ping_answer();
        assert!(result >= 0, "the old tenant's ping did not finish with an answer: {result}");
        assert_eq!(lands(&mut dev, &mut old), old.latency() as usize);
    }

    /// **A ping leaves no hole**: while it holds the send, and until the
    /// programme has come round the loop again, the bay plays its input
    /// delayed by its latency, so a steady signal through a wire stays
    /// steady from before the ping to after it.
    #[test]
    fn a_ping_leaves_no_hole_in_the_track() {
        let mut dev = FakeDevice::new(4, 4, 128, 25);
        dev.patch = vec![(2, 2, 1.0), (3, 3, 1.0)];
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        let input = vec![vec![0.25f32; 128]; 2];
        let mut out = vec![vec![0.0f32; 128]; 2];
        for _ in 0..40 {
            dev.block(&mut u, &input, &mut out);
        }
        dev.hub.port_for(7).unwrap().ask_ping(dev.now());
        let mut n = 0;
        while n < 40 || dev.hub.port_for(7).unwrap().pinging() {
            dev.block(&mut u, &input, &mut out);
            for v in out.iter().flatten() {
                assert!((v - 0.25).abs() < 1e-4, "block {n}: a {v} in a steady 0.25");
            }
            n += 1;
            assert!(n < 3_000);
        }
        for _ in 0..40 {
            dev.block(&mut u, &input, &mut out);
            assert!(out.iter().flatten().all(|v| (v - 0.25).abs() < 1e-4));
        }
    }

    /// **A send destination's second pass is in time too**: fully dry, an
    /// impulse through the second pass comes out at the bay's latency.
    #[test]
    fn a_second_pass_in_a_chunk_is_as_late_as_the_first() {
        let mut dev = FakeDevice::new(4, 4, 128, 20);
        let mut u = seated(&dev, Jacks::pair(2), Jacks::pair(2));
        u.set_param(2, 0.0);
        u.clear();
        let mut heard = Vec::new();
        for k in 0..20 {
            let b = dev.next_frames();
            let silence = vec![vec![0.0f32; b]; 2];
            let mut l = vec![0.0f32; b];
            if k == 4 {
                l[0] = 1.0;
            }
            let hit = vec![l.clone(), l];
            let in_ch = dev.in_ch;
            dev.hub.begin_chunk(dev.view(b), |c| (c < in_ch).then_some(c));
            let mut first = vec![vec![0.0f32; b]; 2];
            let mut second = vec![vec![0.0f32; b]; 2];
            u.process(&silence, b, &mut first);
            u.process(&hit, b, &mut second);
            dev.read_input();
            let mut d = vec![0.0f32; b * 4];
            dev.hub.fold(&mut d, 4, b);
            heard.extend_from_slice(&second[0]);
        }
        let at = heard.iter().position(|v| v.abs() > 0.5).expect("the second pass's dry came out");
        assert_eq!(at - 4 * 128, u.latency() as usize);
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
        let view = dev.view(block);
        // Warm up outside the window.
        for _ in 0..4 {
            dev.block(&mut u, &input, &mut out);
        }
        dev.hub.port_for(7).unwrap().ask_ping(dev.now());
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
