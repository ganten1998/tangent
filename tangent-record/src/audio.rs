//! Audio input capture: the callback, the ring, the meters, and the clock tap.
//!
//! This module is the boundary between a real audio device and everything else
//! in the recorder. Everything on the far side of that boundary — the WAV
//! writer, the mixer, the meters the UI paints — runs on an ordinary thread and
//! may allocate, lock and block. Everything on the near side may do none of
//! those things, ever, and this file exists to make that distinction structural
//! rather than a rule somebody has to remember.
//!
//! # The three rules of the callback, and how each one is enforced here
//!
//! **Never allocate.** [`CaptureSource`] owns a fixed scratch buffer sized at
//! construction, and the rings are `rtrb`, which allocates once and never again.
//! [`Producer::push_entire_slice`](rtrb::Producer::push_entire_slice) copies
//! into already-owned memory.
//!
//! **Never lock.** The callback touches one SPSC ring for samples, one for
//! timing marks, and a handful of relaxed atomics. No `Mutex`. The published
//! [`Meters`] the UI reads *are* behind a `Mutex`, and the callback never
//! touches them: levels are computed on the writer thread, exactly as
//! `docs/RECORDER-PLAN.md` §4 specifies.
//!
//! **Never write a file.** Nothing here knows what a path is.
//!
//! # What cpal 0.16 actually gives you
//!
//! Three of the plan's premises were checked against the vendored source rather
//! than against docs.rs, and one of them is wrong for this version:
//!
//! 1. **There is no stable device identifier. At all.** `DeviceTrait` has
//!    exactly one identity method, `name() -> Result<String, DeviceNameError>`
//!    (`cpal-0.16.0/src/traits.rs:89`). The CoreAudio backend holds an
//!    `AudioDeviceID` but it is `pub(crate)`
//!    (`host/coreaudio/macos/mod.rs:159`); the WASAPI backend calls `GetId()`
//!    inside its own `PartialEq` and never hands the string out
//!    (`host/wasapi/device.rs:767-781`). So the plan's `record_audio_device`
//!    key cannot be a UID no matter how much we would like it to be. See
//!    [`DeviceKey`] for what is possible instead and what it does not buy.
//! 2. **`StreamInstant`'s fields are private** (`secs: i64`, `nanos: u32`,
//!    `src/lib.rs:337-341`) and `as_nanos` is private too. The only way out is
//!    `duration_since`, which returns `None` rather than a negative duration —
//!    so reading one is a two-sided operation. [`stream_instant_nanos`] is that
//!    operation, and it exists because the obvious one-liner silently returns
//!    `None` on ALSA, where a `StreamInstant` legitimately goes negative at the
//!    start of a stream (`host/alsa/mod.rs:833`, `capture = callback - delay`).
//!    `StreamInstant::new` *is* public, which is what lets the tests below cover
//!    this without a device.
//! 3. **The plan's §3a row about cpal subtracting
//!    `kAudioDevicePropertyLatency + kAudioDevicePropertySafetyOffset` describes
//!    cpal 0.18, not the 0.16 pinned here.** In 0.16 the macOS input callback
//!    computes `capture = callback - frames_to_duration(buffer_frames)` under a
//!    literal `// TODO: Need a better way to get delay, for now we assume a
//!    double-buffer offset` (`host/coreaudio/macos/mod.rs:594-606`). Neither
//!    latency property is read anywhere in the crate. So the audio input's own
//!    device latency is **assumed zero on every platform**, and `take.json` must
//!    say `"assumed"` rather than `"os_reported"` for it. [`ClockTap::set_latency`]
//!    is where a measured value goes when there is one.
//!
//! Per-backend `StreamInstant` sources, from cpal's own table
//! (`src/lib.rs:326-336`), because they decide whether the fit measures anything:
//! CoreAudio `mach_absolute_time` (a real hardware clock, same domain as
//! `Instant`), WASAPI `QueryPerformanceCounter` (a real clock, but a *different
//! origin* from Rust's `Instant`, which is QPC plus `u64::MAX / 4` ns), ALSA
//! `snd_pcm_status_get_htstamp` re-based to stream start — and, when the driver
//! returns no usable htstamp, ALSA falls back to `Instant::now()` itself
//! (`host/alsa/mod.rs:914-925`), at which point the fit measures the host clock
//! against the frame count rather than a crystal against the host clock. That
//! is still a useful number; it is just not a *device* rate, and the take's sync
//! report should not claim otherwise.
//!
//! # The invariant that keeps a stereo take in stereo
//!
//! Samples cross the ring interleaved. If a ring-full event ever splits a frame
//! — pushes L without R — then every sample after it is off by one lane and the
//! rest of the take has its channels swapped. Nothing downstream can detect
//! that, and it survives every listening test done in mono. So **only whole
//! frames are ever pushed and only whole frames are ever popped**, on both
//! sides, and both guards have a test.
//!
//! # The invariant that keeps sample N equal to device frame N
//!
//! Every buffer the device delivers produces exactly one [`TimingMark`], and
//! that mark carries the buffer's **absolute** first frame index. So a dropout
//! is visible two ways and both are recoverable: as `frames_dropped` inside a
//! mark, and as a jump in `first_frame` between consecutive marks when even the
//! mark could not be queued. [`FrameCursor`] turns either into "write this many
//! frames of silence here", which is what makes the take's central promise —
//! WAV sample 0, MIDI tick 0 and video frame 0 are the same instant — survive a
//! machine that stuttered.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, FromSample, Sample, SampleFormat, SampleRate, StreamConfig, StreamError,
    SupportedStreamConfig, SupportedStreamConfigRange,
};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::clock::{Nanos, RateFit, SourceClock, Timeline};

/// How long the anchor keeps improving before it is frozen. Two seconds is
/// `clock.rs`'s recommendation and there is no reason for audio to differ.
pub const DEFAULT_SETTLE_NS: Nanos = 2_000_000_000;

/// How much audio the ring holds. Four seconds of 48 kHz stereo `f32` is 1.5 MB,
/// which is nothing, and it means the writer thread can be starved for four
/// whole seconds — a spinning beachball, a Spotlight index, a plugin scan —
/// without the take losing a sample.
pub const DEFAULT_RING_SECONDS: f32 = 4.0;

/// Frames converted per pass through the scratch buffer.
///
/// The scratch is `CHUNK_FRAMES * channels` long and is allocated once. A
/// callback larger than this loops; it never reallocates. 1024 frames is 21 ms
/// at 48 kHz, comfortably above any buffer size a human would choose.
const CHUNK_FRAMES: usize = 1024;

/// How long after a stream opens the clip latch stays disarmed.
///
/// **A converter warming up is not a performance clipping.** On Linux the app
/// opens `default`, which is plug-routed: PipeWire builds a rate and channel
/// converter chain under the stream, and rebuilds it when the system default
/// source moves. A topology change was observed latching BOTH clip lamps on a
/// source that was silent — one garbage buffer during warm-up is enough — and
/// with the latch un-clearable (see `Cmd::ClearClip`) that phantom was
/// permanent. It is very likely where "clipping" came from for people who
/// never clipped.
///
/// 50 ms: long enough to swallow a converter's first buffers at any buffer
/// size this app offers, short enough that it cannot hide real clipping. The
/// stream opens when the Recorder band does, and nobody is a twentieth of a
/// second from their first note at that moment.
///
/// Peaks are NOT suppressed during the window — only the latch. A meter that
/// went blind for 50 ms would be a worse lie than the one this fixes.
pub const CLIP_WARMUP_MS: f64 = 50.0;

/// A sample at or above this magnitude counts as clipped.
///
/// **Deliberately not 1.0.** `dasp` maps `i16` to float as `s / 32768.0`
/// (`dasp_sample-0.11.0/src/conv.rs:229-231`), so a signal slammed against the
/// positive rail of a 16-bit converter arrives as 0.999_969 and never reaches
/// 1.0 — while `i16::MIN` maps to exactly -1.0. A threshold of 1.0 therefore
/// detects only the negative half of the clipping, which presents as "the clip
/// light only comes on for loud bass notes" and gets blamed on the meter.
pub const CLIP_LEVEL: f32 = 0.9999;

/// Below this take peak, the take is silence rather than quiet playing.
///
/// -60 dBFS is roughly a room tone floor. A digital piano's line out at its
/// quietest usable setting is far above it, so a take under this floor means a
/// cable in the wrong socket, not a pianissimo.
pub const SILENCE_FLOOR_DBFS: f32 = -60.0;

/// Peak-hold decay, in dB per second. 20 dB/s is the broadcast convention.
const HOLD_DECAY_DB_PER_SEC: f32 = 20.0;

// ───────────────────────────────────────────────────────────────────────────
// The timebase
// ───────────────────────────────────────────────────────────────────────────

/// The recorder's zero: the one place `std::time::Instant` is read.
///
/// `clock.rs` is deliberately free of `Instant` so that a twenty-minute
/// adversarial take is a one-second unit test. Something still has to read the
/// clock, and it has to be the same something for every source, or MIDI and
/// audio end up on two epochs that differ by however long the recorder took to
/// start up.
///
/// **Reading this inside the audio callback is fine and is not a compromise.**
/// `Instant::now()` on every platform this ships to is a vDSO/`QueryPerformanceCounter`
/// read: no syscall, no allocation, no lock, tens of nanoseconds. It is what
/// every DAW meter does. And it is *required*, because
/// [`SourceClock::observe`](crate::clock::SourceClock::observe) anchors by
/// pairing a device stamp with a timebase reading taken at the same moment; a
/// reading taken later on the writer thread carries the queue delay it is
/// supposed to be measuring.
#[derive(Debug, Clone, Copy)]
pub struct Timebase {
    epoch: Instant,
}

impl Timebase {
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }

    pub fn now(&self) -> Nanos {
        self.at(Instant::now())
    }

    /// Place an `Instant` in the timebase.
    ///
    /// Signed, and the negative branch is not defensive padding: the pre-roll
    /// ring and the always-on MIDI tap both carry events from before the
    /// recorder's epoch, and `Instant::duration_since` saturates to zero rather
    /// than telling you so.
    pub fn at(&self, instant: Instant) -> Nanos {
        match instant.checked_duration_since(self.epoch) {
            Some(d) => d.as_nanos() as Nanos,
            None => -(self.epoch.duration_since(instant).as_nanos() as Nanos),
        }
    }
}

impl Default for Timebase {
    fn default() -> Self {
        Self::new()
    }
}

/// Read a `cpal::StreamInstant` as signed nanoseconds.
///
/// cpal keeps `secs`/`nanos` private and `duration_since` returns `None` rather
/// than a negative `Duration`, so the naive
/// `t.duration_since(&StreamInstant::new(0, 0))` silently yields `None` for any
/// instant before the origin — which is exactly what ALSA produces for the first
/// callbacks of a stream, where `capture = callback - buffer_delay` and
/// `callback` is measured from the stream's own trigger. Losing those marks
/// would quietly shorten the anchor window on Linux and nothing would say so.
pub fn stream_instant_nanos(t: cpal::StreamInstant) -> Option<Nanos> {
    let zero = cpal::StreamInstant::new(0, 0);
    match t.duration_since(&zero) {
        Some(d) => i64::try_from(d.as_nanos()).ok(),
        None => zero
            .duration_since(&t)
            .and_then(|d| i64::try_from(d.as_nanos()).ok())
            .map(|n| -n),
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Device identity
// ───────────────────────────────────────────────────────────────────────────

/// Shown for a device whose name cannot be read. Not an error: an interface
/// that has just been unplugged fails `name()` while still appearing in the
/// enumeration, and dropping it from the list is worse than showing it.
pub const UNNAMED_DEVICE: &str = "(unnamed device)";

/// The best identifier cpal 0.16 permits: a name plus which one of that name.
///
/// **Selecting by display name alone is wrong and this only half-fixes it.**
/// Two identical interfaces report the same string, so a name-only setting can
/// pick the wrong one; a name also changes with the OS locale and with a driver
/// update. `occurrence` disambiguates duplicates by their position in the host's
/// enumeration, which is stable within a session and *usually* stable across
/// them, because CoreAudio and WASAPI both enumerate in a device-registration
/// order that does not change while the devices do not.
///
/// What this does **not** give you, stated plainly so nobody assumes otherwise:
/// unplugging the first of two identical interfaces promotes the second to
/// occurrence 0, and the saved setting then points at the survivor. A real UID
/// would catch that. cpal 0.16 exposes none — see the module docs — so the
/// honest options are this or a platform-specific enumeration of our own, and
/// the latter is three backends of work for a case that costs one re-selection.
///
/// The string form is what `record_audio_device` stores.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceKey {
    pub name: String,
    pub occurrence: u32,
}

impl DeviceKey {
    pub fn new(name: impl Into<String>, occurrence: u32) -> Self {
        Self {
            name: name.into(),
            occurrence,
        }
    }

    /// The first (usually only) device with this name.
    pub fn named(name: impl Into<String>) -> Self {
        Self::new(name, 0)
    }

    /// The form written into settings.
    ///
    /// `#` in the device's own name is doubled, and the occurrence is appended
    /// after a single `#` only when it is non-zero — so the common case is the
    /// plain readable name and nothing else.
    ///
    /// **The escaping is not decoration.** "Microphone #2" is a real Windows
    /// device name. Without it, that name at occurrence 0 serialises to
    /// "Microphone #2" and parses back as *"Microphone " at occurrence 2*, a
    /// device that does not exist — so the saved input silently reverts to the
    /// default on the next launch and the user re-picks it every session
    /// without ever learning why.
    pub fn to_setting(&self) -> String {
        let escaped = self.name.replace('#', "##");
        if self.occurrence == 0 {
            escaped
        } else {
            format!("{escaped}#{}", self.occurrence)
        }
    }

    /// Parse [`to_setting`](Self::to_setting).
    ///
    /// A trailing run of digits is an occurrence only if the run of `#` before
    /// it has **odd** length: an even run is entirely escaped `#`s belonging to
    /// the name, and no separator is present. That one parity check is what
    /// makes "Mic##3" (the device "Mic#3") and "Mic#3" (the device "Mic", third
    /// of its name) distinguishable.
    ///
    /// A string with no separator is taken as a bare name at occurrence 0, so a
    /// hand-edited `settings.json` holding just the device's name does the
    /// obvious thing rather than failing.
    pub fn from_setting(s: &str) -> Self {
        // ASCII digits and '#' are one byte each, so the char counts below are
        // also byte offsets.
        let digits_at = s.len() - s.chars().rev().take_while(char::is_ascii_digit).count();
        let (head, digits) = s.split_at(digits_at);
        let hashes = head.len() - head.trim_end_matches('#').len();
        if !digits.is_empty() && hashes % 2 == 1 {
            if let Ok(occurrence) = digits.parse::<u32>() {
                let name = head[..head.len() - 1].replace("##", "#");
                return Self::new(name, occurrence);
            }
        }
        Self::named(s.replace("##", "#"))
    }
}

impl fmt::Display for DeviceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.occurrence == 0 {
            f.write_str(&self.name)
        } else {
            write!(f, "{} ({})", self.name, self.occurrence + 1)
        }
    }
}

/// Assign an occurrence to each name in enumeration order.
///
/// Split out from [`input_devices`] because it is the only part of enumeration
/// that has a bug in it, and it is the only part that can be tested without an
/// interface plugged in.
fn keys_for_names<I, S>(names: I) -> Vec<DeviceKey>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut seen: HashMap<String, u32> = HashMap::new();
    names
        .into_iter()
        .map(|n| {
            let name = n.into();
            let slot = seen.entry(name.clone()).or_insert(0);
            let occurrence = *slot;
            *slot += 1;
            DeviceKey::new(name, occurrence)
        })
        .collect()
}

/// One row of the device picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDeviceInfo {
    pub key: DeviceKey,
    /// From the device's own default config. `None` when the device refused to
    /// report one, which a disconnected interface does.
    pub channels: Option<u16>,
    pub sample_rate: Option<u32>,
    /// True for the host's default input — matched **by name**, so with two
    /// identical interfaces the flag lands on the first of them whether or not
    /// it is the one the OS means. cpal gives no way to compare two `Device`s on
    /// macOS or Linux, so this is a label, never a selection.
    pub is_default: bool,
}

// ───────────────────────────────────────────────────────────────────────────
// PipeWire, on Linux
// ───────────────────────────────────────────────────────────────────────────

/// One capture node PipeWire knows about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipewireSource {
    /// `node.name`. Stable across sessions, and what `PIPEWIRE_NODE` takes.
    pub node: String,
    /// `node.description` — what a person calls it.
    pub description: String,
    pub channels: Option<u16>,
}

/// Every capture node PipeWire can see.
///
/// **Why this exists at all.** On a PipeWire machine, ALSA-level enumeration
/// cannot see the interfaces. PipeWire holds each card exclusively, so cpal's
/// card walk opens `plughw:N` and gets `EBUSY` — measured on the owner's box,
/// where `arecord -D plughw:1` says "Device or resource busy" for the Scarlett
/// while the built-in opens fine, and which of the two is busy depends on what
/// PipeWire happens to be routing at that moment. The one PCM that always
/// opens is `pipewire` itself, which follows the system DEFAULT source — so
/// the app offered exactly one input, called `pipewire`, that silently captured
/// whatever the desktop was pointed at. "No sound from my interface" was the
/// correct description of that.
///
/// **Why a subprocess and not the PipeWire library.** `pipewire-rs` links
/// `libpipewire` and needs its headers at BUILD time, and the Linux artifacts
/// are cross-compiled from a Mac — see `scripts/build-cross.sh`. Adding that
/// dependency would trade a working release process for a tidier lookup.
/// `pw-dump` ships with PipeWire itself, emits exactly this as JSON, and its
/// absence is a clean fallback to the old behaviour rather than a build that
/// cannot be made.
///
/// Empty on any failure at all — no PipeWire, no `pw-dump`, a JSON shape from
/// a future version — and the caller falls back to cpal's own enumeration.
#[cfg(target_os = "linux")]
pub fn pipewire_sources() -> Vec<PipewireSource> {
    let out = match crate::proc::command("pw-dump")
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out) else {
        return Vec::new();
    };
    let Some(objects) = json.as_array() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for o in objects {
        if o.get("type").and_then(|t| t.as_str()) != Some("PipeWire:Interface:Node") {
            continue;
        }
        let Some(props) = o.pointer("/info/props") else {
            continue;
        };
        let class = props
            .get("media.class")
            .and_then(|c| c.as_str())
            .unwrap_or_default();
        // Sources and duplex devices; NOT monitors of a sink, which are named
        // as sources and are the loopback the owner used as a workaround. They
        // are legitimate inputs and they are also how somebody accidentally
        // records their own output, so they are listed — but a camera is not
        // an audio source and `v4l2_input` nodes carry `Video/Source`.
        if !class.starts_with("Audio/Source") && class != "Audio/Duplex" {
            continue;
        }
        let Some(node) = props.get("node.name").and_then(|n| n.as_str()) else {
            continue;
        };
        found.push(PipewireSource {
            node: node.to_owned(),
            description: props
                .get("node.description")
                .and_then(|d| d.as_str())
                .unwrap_or(node)
                .to_owned(),
            channels: props
                .get("audio.channels")
                .and_then(serde_json::Value::as_u64)
                .and_then(|c| u16::try_from(c).ok()),
        });
    }
    found
}

/// Nothing to enumerate: CoreAudio and WASAPI list their own devices properly.
#[cfg(not(target_os = "linux"))]
pub fn pipewire_sources() -> Vec<PipewireSource> {
    Vec::new()
}

/// The PCM that PipeWire always provides, whatever it is holding.
const PIPEWIRE_PCM: &str = "pipewire";

/// Run `f` with `PIPEWIRE_NODE` set, and put the environment back.
///
/// **The whole binding mechanism, and it is an environment variable because
/// that is the only knob pipewire-alsa offers.** Verified on the owner's box:
/// `PIPEWIRE_NODE=<node> arecord -D pipewire` captures from that node, by name
/// or by id, while the same device is `EBUSY` through ALSA directly.
///
/// It has to wrap the ENUMERATION as well as the stream build, because cpal's
/// ALSA host opens each device's PCM handles while listing them and the stream
/// then reuses the handle that is already open — so a variable set after the
/// lookup would arrive too late to bind anything.
///
/// `set_var` is process-global and this is a UI-thread call. Both audio streams
/// are opened from that thread, one at a time, and nothing else in the process
/// reads this variable — so the window in which it is set is a few
/// milliseconds inside one function, with no other opener able to run.
#[cfg(target_os = "linux")]
fn with_pipewire_node<T>(node: &str, f: impl FnOnce() -> T) -> T {
    let previous = std::env::var_os("PIPEWIRE_NODE");
    std::env::set_var("PIPEWIRE_NODE", node);
    let out = f();
    match previous {
        Some(v) => std::env::set_var("PIPEWIRE_NODE", v),
        None => std::env::remove_var("PIPEWIRE_NODE"),
    }
    out
}

#[cfg(not(target_os = "linux"))]
fn with_pipewire_node<T>(_node: &str, f: impl FnOnce() -> T) -> T {
    f()
}

// ───────────────────────────────────────────────────────────────────────────
// Dropouts, from the one place on a PipeWire desktop that can see them
// ───────────────────────────────────────────────────────────────────────────

/// The graph's own xrun count for THIS process's audio nodes, watched from a
/// timer thread.
///
/// **This exists because the client-side count is structurally blind here.**
/// cpal's ALSA backend now reports every EPIPE it sees — all three sites —
/// and on a pipewire-alsa desktop it sees none: the plugin absorbs the xrun,
/// re-prepares, and the error never crosses the plugin boundary. Verified on
/// the machine this was written for, from both ends at once: a 64-frame
/// buffer the user described as "total noise", zero backend errors in the
/// app's log, and PipeWire's own counter for the node climbing 24 → 72 in
/// the same window. The truth exists in exactly one place, and it is the
/// server's.
///
/// Read the way [`pipewire_sources`] and [`graph_clock`] read — by shelling
/// to tools that ship with PipeWire — and for the same reason: linking
/// libpipewire needs headers at build time and the Linux artifacts are
/// cross-compiled. `pw-dump` maps this process's pid to its node ids;
/// `pw-top -b` carries the per-node ERR column that `pw-dump` does not.
///
/// **Two rules, both scars:**
///
/// - **It polls on its own thread, on a timer.** Never from the audio thread
///   and never per-frame from the UI loop — a process spawn in either is a
///   new fault of the same family this is trying to observe. The thread
///   sleeps in slices so dropping the watcher never waits out an interval.
/// - **Unreadable is `None`, never zero.** A machine without `pw-top`, a
///   graph that is not PipeWire, a parse that fails on a future format — all
///   of those are "nobody knows", and reporting 0 for "nobody knows" is the
///   exact lie `latency_source: "assumed_zero"` exists to avoid one field
///   over. The counter that read a confident zero through total noise has
///   already been believed once.
pub struct GraphDropouts {
    /// Latest reading: -1 is "unknown", anything else is the count.
    latest: std::sync::Arc<std::sync::atomic::AtomicI64>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl GraphDropouts {
    /// How often the graph is asked. Coarse on purpose: a dropout display is
    /// read by a human, and two spawned processes per poll is a real cost on
    /// the small machines this matters most on.
    #[cfg(target_os = "linux")]
    const POLL: std::time::Duration = std::time::Duration::from_secs(3);

    /// Start watching. On platforms with no PipeWire this returns a watcher
    /// that answers `None` forever and starts no thread at all.
    pub fn start() -> Self {
        use std::sync::atomic::{AtomicBool, AtomicI64};
        let latest = std::sync::Arc::new(AtomicI64::new(-1));
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        #[cfg(target_os = "linux")]
        {
            let latest_t = std::sync::Arc::clone(&latest);
            let stop_t = std::sync::Arc::clone(&stop);
            let spawned = std::thread::Builder::new()
                .name("graph-dropouts".to_owned())
                .spawn(move || {
                    use std::sync::atomic::Ordering;
                    let pid = std::process::id();
                    loop {
                        let count = read_graph_dropouts(pid);
                        latest_t.store(count.map_or(-1, |c| c as i64), Ordering::Relaxed);
                        // Sleep in slices so a drop is honoured within a
                        // tenth of a second — the same shape `Reprobe` uses,
                        // and for the same reason.
                        let slices = Self::POLL.as_millis() / 100;
                        for _ in 0..slices.max(1) {
                            if stop_t.load(Ordering::Relaxed) {
                                return;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                    }
                });
            if spawned.is_err() {
                // No thread means no readings: `latest` stays -1, which reads
                // as `None`, which is the honest answer.
            }
        }
        Self { latest, stop }
    }

    /// The most recent count, or `None` where nothing can see one.
    ///
    /// Cheap enough for a per-frame read — one relaxed load — which is the
    /// point of the thread: the SPAWNING is on a timer, the READING is free.
    pub fn latest(&self) -> Option<u64> {
        let v = self.latest.load(std::sync::atomic::Ordering::Relaxed);
        u64::try_from(v).ok()
    }
}

impl Drop for GraphDropouts {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The one number to show, from two counters that are each blind somewhere.
///
/// **Neither layer sees the whole fault, and this was measured twice with the
/// answers reversed.** With a Scarlett at a 64-frame buffer, the graph's ERR
/// climbed 48 while the client saw zero EPIPEs — pipewire-alsa absorbed them
/// below cpal. With the built-in card at the same buffer, the CLIENT counted
/// 246 EPIPEs while the graph's ERR for the same node sat at zero — the
/// plugin ate the underruns without the node ever missing a cycle. Same
/// machine, same week, opposite blindnesses.
///
/// So preferring either source lets a confident zero from the wrong layer
/// mask the other's count, which is the exact lie a dropout display exists to
/// end. Both are lower bounds on "gaps in what was heard"; the max is the
/// best lower bound available; and only when NEITHER could count is the
/// answer unknown.
pub fn merge_dropouts(graph: Option<u64>, client: u64) -> Option<u64> {
    match graph {
        Some(g) => Some(g.max(client)),
        None if client > 0 => Some(client),
        // A client zero with no graph reading is "nobody could see", not a
        // clean bill: the client's counter has already been proven blind on
        // one of the two paths this ships on.
        None => None,
    }
}

/// One poll: this process's nodes by pid, then their ERR total.
#[cfg(target_os = "linux")]
fn read_graph_dropouts(pid: u32) -> Option<u64> {
    let dump = crate::proc::command("pw-dump")
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !dump.status.success() {
        return None;
    }
    let ids = our_audio_nodes(&String::from_utf8_lossy(&dump.stdout), pid);
    if ids.is_empty() {
        // No nodes is a real answer while no stream is open, but it is not a
        // COUNT: reporting 0 here would claim the take was clean when nothing
        // was measured at all.
        return None;
    }
    let top = crate::proc::command("pw-top")
        .args(["-b", "-n", "2"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !top.status.success() {
        return None;
    }
    sum_errs(&String::from_utf8_lossy(&top.stdout), &ids)
}

/// The audio node ids belonging to `pid`, from `pw-dump` JSON.
///
/// Matched through the CLIENT rather than trusting the node to carry the pid:
/// PipeWire puts `application.process.id` on the client object, and the node
/// points at its client with `client.id`. Nodes that carry the pid directly
/// are taken too, because pipewire-alsa has done both across versions.
#[cfg(target_os = "linux")]
fn our_audio_nodes(dump_json: &str, pid: u32) -> Vec<u64> {
    let Ok(objects) = serde_json::from_str::<Vec<serde_json::Value>>(dump_json) else {
        return Vec::new();
    };
    let pid_matches = |v: Option<&serde_json::Value>| -> bool {
        match v {
            Some(serde_json::Value::Number(n)) => n.as_u64() == Some(u64::from(pid)),
            Some(serde_json::Value::String(s)) => s.parse::<u32>().ok() == Some(pid),
            _ => false,
        }
    };
    let mut our_clients = Vec::new();
    for o in &objects {
        if o.get("type").and_then(|t| t.as_str()) == Some("PipeWire:Interface:Client")
            && pid_matches(o.pointer("/info/props/application.process.id"))
        {
            if let Some(id) = o.get("id").and_then(serde_json::Value::as_u64) {
                our_clients.push(id);
            }
        }
    }
    let mut ids = Vec::new();
    for o in &objects {
        if o.get("type").and_then(|t| t.as_str()) != Some("PipeWire:Interface:Node") {
            continue;
        }
        let props = o.pointer("/info/props");
        let is_audio = props
            .and_then(|p| p.get("media.class"))
            .and_then(|c| c.as_str())
            .is_some_and(|c| c.contains("Audio"));
        if !is_audio {
            continue;
        }
        let owned = pid_matches(props.and_then(|p| p.get("application.process.id")))
            || props
                .and_then(|p| p.get("client.id"))
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|c| our_clients.contains(&c));
        if owned {
            if let Some(id) = o.get("id").and_then(serde_json::Value::as_u64) {
                ids.push(id);
            }
        }
    }
    ids
}

/// Total ERR over `ids`, from `pw-top -b` output. `None` when none of the ids
/// appear — a snapshot that missed every node measured nothing.
///
/// The batch format is column-stable enough to split on whitespace: every data
/// row is `S ID QUANT RATE WAIT BUSY W/Q B/Q ERR ...`, so ERR is the ninth
/// token wherever the row's id parses. The LAST occurrence of an id wins,
/// because `-n 2` prints two snapshots and the first is often idle.
fn sum_errs(top_output: &str, ids: &[u64]) -> Option<u64> {
    let mut latest: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    for line in top_output.lines() {
        let mut t = line.split_whitespace();
        let _state = t.next();
        let Some(id) = t.next().and_then(|v| v.parse::<u64>().ok()) else {
            continue;
        };
        if !ids.contains(&id) {
            continue;
        }
        // QUANT RATE WAIT BUSY W/Q B/Q and THEN err — six to skip, and the
        // first version skipped five and read B/Q, whose "0.00" never parses
        // and quietly turned every row into "unknown". The test below carries
        // the captured rows precisely so this cannot regress by inspection.
        if let Some(err) = t.nth(6).and_then(|v| v.parse::<u64>().ok()) {
            latest.insert(id, err);
        }
    }
    if latest.is_empty() {
        None
    } else {
        Some(latest.values().sum())
    }
}

/// Whether the host in use reaches interfaces through the `pipewire` PCM.
///
/// **The PipeWire node list is a fix for cpal's ALSA host, and applying it to
/// any other host is the bug it was written to cure.** [`pipewire_sources`]
/// exists because PipeWire holds every card exclusively, so through ALSA the
/// only PCM that opens is `pipewire` and it has to be aimed with
/// `PIPEWIRE_NODE`. None of that is true of the JACK host: it is a client of
/// the same graph, its ports carry the interfaces' real names, and it has no
/// device called `pipewire` at all.
///
/// So under JACK the substitution fails twice over. The picker would list nodes
/// that cannot be opened — [`resolve`] looks for a PCM named `pipewire` in the
/// host's own devices and a JACK host has one input client and no PCMs — and
/// because that list REPLACES cpal's rather than joining it, every workable
/// entry would have been hidden behind entries that all fail. Choosing JACK
/// would have taken the input away while leaving the output playing, which is
/// the hardest shape of fault to read from the outside.
///
/// Asked of the host actually built rather than of the saved setting, because
/// [`build_host`] falls back — a settings file naming JACK on a machine with no
/// server running is running ALSA, and it needs the node list.
///
/// Goes through [`with_host`], so on the UI thread — where every caller of this
/// lives — it reads the cached host and costs nothing. Called from a thread
/// that has never built one, it would build one, and under JACK that means
/// cpal registering and immediately dropping a pair of probe clients
/// (`jack/device.rs` makes one per direction to read the server's rate, and
/// drops it). Harmless, and not free; keep the callers on the one thread.
pub fn pipewire_pcm_route_applies() -> bool {
    !cfg!(target_os = "linux") || with_host(|host| host.id().name() != "JACK")
}

// ───────────────────────────────────────────────────────────────────────────
// The graph's own buffer size
// ───────────────────────────────────────────────────────────────────────────

/// What the PipeWire graph will actually serve, in frames.
///
/// **The buffer control on Linux has been a control that controls nothing, and
/// this is what it was missing.** A PipeWire graph runs ONE period for every
/// client, clamped to `clock.min-quantum`, and a client asking for less is not
/// refused — it is quietly given the floor. So a user who picks 256 on a graph
/// whose floor is 512 gets 512, and the app has never been able to say so:
/// `pick_buffer` asks, cpal reports what it got, and the panel guessed at a
/// reason ("another app owns this device") that is a CoreAudio explanation for
/// a PipeWire fact and is simply false here.
///
/// Worse than false, it is unactionable. The number that decided the outcome is
/// in `~/.config/pipewire/pipewire.conf.d/`, and nothing in the app pointed
/// there. Measured on the machine this was written for: at a floor of 512 a
/// take recorded at 256 came back **48.6 % digital silence** — 726 whole
/// periods padded out — while the panel showed no fault of any kind.
///
/// Read through `pw-metadata`, for exactly the reason [`pipewire_sources`]
/// reads through `pw-dump`: it ships with PipeWire, it needs no headers at
/// build time, and the Linux artifacts are cross-compiled. Absent PipeWire it
/// returns `None` and every caller falls back to what it did before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphClock {
    /// The period every client on this graph runs at, right now.
    pub quantum: u32,
    /// The floor. **A request below this is not honoured and not refused.**
    pub min_quantum: u32,
    /// Non-zero when the quantum is pinned, by us or by somebody else.
    pub forced: u32,
    pub rate: u32,
}

impl GraphClock {
    /// The smallest period this graph will actually serve.
    ///
    /// `force-quantum` wins over `min-quantum` when it is set, which is why it
    /// cannot be read off the floor alone.
    pub fn floor(&self) -> u32 {
        if self.forced > 0 {
            self.forced
        } else {
            self.min_quantum.max(1)
        }
    }

    /// Whether a request for `frames` can be met as asked.
    pub fn will_serve(&self, frames: u32) -> bool {
        frames >= self.floor()
    }
}

/// Read the graph's clock, or `None` where there is no PipeWire to ask.
#[cfg(target_os = "linux")]
pub fn graph_clock() -> Option<GraphClock> {
    let out = crate::proc::command("pw-metadata")
        .args(["-n", "settings"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // Lines read `update: id:0 key:'clock.quantum' value:'512' type:''`. Parsed
    // by key rather than by position: the set is not ordered and has grown
    // between PipeWire versions.
    let field = |key: &str| -> Option<u32> {
        text.lines()
            .find(|l| l.contains(&format!("key:'{key}'")))
            .and_then(|l| l.split("value:'").nth(1))
            .and_then(|v| v.split('\'').next())
            .and_then(|v| v.trim().parse().ok())
    };
    Some(GraphClock {
        quantum: field("clock.quantum")?,
        min_quantum: field("clock.min-quantum").unwrap_or(0),
        forced: field("clock.force-quantum").unwrap_or(0),
        rate: field("clock.rate").unwrap_or(48_000),
    })
}

#[cfg(not(target_os = "linux"))]
pub fn graph_clock() -> Option<GraphClock> {
    None
}

/// Ask the graph to run at `frames`, or release the request with `None`.
///
/// **This is global, and that is the whole reason it is opt-in.** One graph
/// serves every client, so pinning it moves the DAW in the next window too —
/// which is precisely why the machine this was written for had it pinned at 512
/// in the first place, to stop some other client dragging a Qtractor session
/// down. So it is only ever called from an explicit buffer choice, never
/// speculatively, and [`release_graph_quantum`] puts it back.
///
/// Returns whether the graph took it. `false` is not an error worth a dialog:
/// it means no PipeWire, no `pw-metadata`, or a graph that declined — and the
/// stream still opens at whatever the floor turns out to be, exactly as before.
#[cfg(target_os = "linux")]
pub fn request_graph_quantum(frames: Option<u32>) -> bool {
    let value = frames.unwrap_or(0).to_string();
    crate::proc::command("pw-metadata")
        .args(["-n", "settings", "0", "clock.force-quantum", &value])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(not(target_os = "linux"))]
pub fn request_graph_quantum(_frames: Option<u32>) -> bool {
    false
}

/// The ALSA ring a `Fixed` capture request must actually ask for on this graph.
///
/// **A ring smaller than the graph's cycle does not underrun — it silently
/// keeps `ring/quantum` of the audio.** Measured with `takeprobe` on the
/// machine that lost takes to this (PipeWire, `min-quantum = 512`): a
/// 44100 Hz stream at `Fixed(256)` delivered a steady 22,016 frames per
/// second — every graph cycle hands the `pipewire` PCM one 512-frame quantum,
/// the 256-frame ring keeps what fits, and the rest vanishes with no EPIPE, no
/// counter and no error on any layer. The stream looked *clean* at exactly
/// half rate: stamps honest, the rate fit at 22 050 Hz, and a fifteen-second
/// performance wrote a "7.5-second" take whose audio plays at double speed
/// against its own video. At `Fixed(512)` and `Fixed(1024)` the same stream
/// delivered every frame.
///
/// Twice the FLOOR is a ring no renegotiation can outgrow: the floor bounds
/// the served quantum from below, and our own period request — which stays at
/// the asked-for number's scale, `ring/4 = floor/2 < floor` — caps it from
/// above, so the graph is never dragged coarser than its floor by this and the
/// delivered cycle can never exceed `max(floor, ring/4) < ring`. The asked-for
/// number still decides the request; the ring is protection, and a capture
/// ring runs nearly empty, so the depth costs memory and not latency.
pub fn ring_covering_graph(asked: u32, floor: u32) -> u32 {
    asked.max(floor.saturating_mul(2))
}

/// Holds a quantum request for as long as it exists, and gives it back.
///
/// **This type exists because of a specific mistake, and its shape is the
/// apology.** Pinning the graph by hand during an investigation left the
/// machine's audio disturbed after the investigation ended: `force-quantum` was
/// released, but **a node that is already running does not renegotiate**, so
/// the interface stayed on a stale period while the metadata said the graph was
/// free. The next recordings made on that machine came back with whole periods
/// missing, and the state that caused it was invisible in every place anybody
/// would look — `pw-metadata` said `force-quantum: 0`, and it was true, and it
/// was not the whole truth.
///
/// A `set` with a matching `release` somewhere else in the code is the same
/// trap with better manners. So the request is a VALUE: while it is alive the
/// graph is pinned, when it is dropped the graph is freed, and there is no path
/// — early return, error, panic, a window closed, the process quitting — that
/// releases one without the other.
///
/// **Dropping it is not the whole job and the caller must know that.** Freeing
/// the graph does not move a running node back; only reopening the stream does.
/// [`GraphQuantum::release_needs_reopen`] says so at the type level rather than
/// in a comment somebody has to find.
#[derive(Debug)]
pub struct GraphQuantum {
    frames: u32,
    held: bool,
}

impl GraphQuantum {
    /// Ask the graph for `frames`, and hold it until this value is dropped.
    ///
    /// `None` when there is no PipeWire to ask or it declined — which is not an
    /// error: the stream still opens at whatever the graph does serve, exactly
    /// as it did before any of this existed.
    pub fn request(frames: u32) -> Option<Self> {
        if frames == 0 {
            return None;
        }
        request_graph_quantum(Some(frames)).then_some(Self { frames, held: true })
    }

    /// The size being held.
    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// **Whether letting go is enough.** It is not: a node already running keeps
    /// the period it was started with, so the caller has to reopen its streams
    /// to actually leave. Always true; a method rather than a constant so the
    /// answer is impossible to read past.
    pub fn release_needs_reopen(&self) -> bool {
        true
    }
}

impl Drop for GraphQuantum {
    fn drop(&mut self) {
        if self.held {
            release_graph_quantum();
        }
    }
}

/// Give the graph back to whatever it was doing.
///
/// Called when the app is leaving the buffer alone again — a different audio
/// system chosen, or the process shutting down. Leaving a machine pinned
/// because an app exited badly is the kind of thing that gets blamed on the
/// next program to open.
pub fn release_graph_quantum() -> bool {
    request_graph_quantum(None)
}

// ── the claim: full control, no config files ───────────────────────────────
//
// `request_graph_quantum` sat here fully built and called by NOBODY, waiting
// on the owner's ruling. The ruling came 2026-08-31: **Tangent must be
// usable by somebody who has never heard of a quantum, on a distro whose
// audio stack was configured by nobody. Full control, all the time, and the
// user never edits a config file.** So the claim is policy now: whenever a
// stream asks for a size the graph will not serve, Tangent takes the clock,
// holds it while it runs, and puts back exactly what it found on the way
// out. `pw-metadata` state is LIVE, not configuration — nothing on disk
// changes, and releasing the claim restores the machine's own settings.
//
// FreeBSD needs no twin of this: there is no graph daemon between the app
// and the driver, so the `Fixed` request already IS the whole authority.

/// What we forced, or 0 when nothing is held. The prior force (somebody
/// else's pin, put back on release) rides in the high half of a second
/// atomic so a claim survives being asked twice without stacking.
static CLAIMED_QUANTUM: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static PRIOR_FORCE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Where the claim writes its crash marker, set once by the binary at boot.
/// A crash cannot release the claim, so the NEXT launch does: if the marker
/// names the same force the graph still carries, it is ours and it comes
/// off. In-process exits (quit, panic hook) release directly.
static CLAIM_MARKER: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

pub fn set_claim_marker(path: std::path::PathBuf) {
    let _ = CLAIM_MARKER.set(path);
}

/// What a claim should do, decided purely so every branch is testable on
/// every platform: the imperative shell below stays four lines per arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimAction {
    /// The graph already serves the ask (or we already hold this size).
    Keep,
    /// Take the clock at this size, remembering `prior` to restore.
    Force { frames: u32, prior: u32 },
    /// We hold a claim the new ask no longer needs — give the clock back.
    Release,
}

pub fn claim_action(g: GraphClock, asked: u32, held: u32, prior: u32) -> ClaimAction {
    if held == asked && g.forced == asked {
        return ClaimAction::Keep;
    }
    if held != 0 && g.forced == held {
        // We hold the clock at some other size. If the graph would serve
        // the new ask WITHOUT us (its own floor is fine), let go; otherwise
        // move our claim to the new size, keeping the ORIGINAL prior — the
        // machine's own state, not our last claim — for the release.
        let without_us = GraphClock { forced: prior, ..g };
        return if without_us.will_serve(asked) {
            ClaimAction::Release
        } else {
            ClaimAction::Force { frames: asked, prior }
        };
    }
    if g.will_serve(asked) {
        ClaimAction::Keep
    } else {
        // Somebody else's force pin is overridden too — the ruling is that
        // the person at the microphone outranks whatever a config file or
        // another app decided — but it is remembered and restored.
        ClaimAction::Force { frames: asked, prior: g.forced }
    }
}

/// The graph's clock, after TAKING it if it would not serve `asked`.
///
/// Returns the post-claim clock, so ring sizing downstream sees the floor
/// that is actually in force. Every failure degrades to the old behaviour:
/// no PipeWire, no `pw-metadata`, or a declined write all fall back to
/// reading whatever is there, and the ring guard covers the rest.
#[cfg(target_os = "linux")]
pub fn claim_graph_clock(asked: u32) -> Option<GraphClock> {
    use std::sync::atomic::Ordering;
    let g = graph_clock()?;
    let held = CLAIMED_QUANTUM.load(Ordering::SeqCst);
    let prior = PRIOR_FORCE.load(Ordering::SeqCst);
    match claim_action(g, asked, held, prior) {
        ClaimAction::Keep => Some(g),
        ClaimAction::Release => {
            release_graph_claim();
            graph_clock().or(Some(g))
        }
        ClaimAction::Force { frames, prior } => {
            if !request_graph_quantum(Some(frames)) {
                return Some(g);
            }
            CLAIMED_QUANTUM.store(frames, Ordering::SeqCst);
            PRIOR_FORCE.store(prior, Ordering::SeqCst);
            if let Some(m) = CLAIM_MARKER.get() {
                let _ = std::fs::write(m, frames.to_string());
            }
            graph_clock().or(Some(g))
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn claim_graph_clock(_asked: u32) -> Option<GraphClock> {
    None
}

/// Put back what the claim displaced: the prior pin if there was one,
/// nothing if there was not. Safe to call unconditionally on every exit
/// road — quit, panic, the pre-`_exit` hardening — a no-op when nothing is
/// held.
pub fn release_graph_claim() {
    use std::sync::atomic::Ordering;
    let held = CLAIMED_QUANTUM.swap(0, Ordering::SeqCst);
    if held == 0 {
        return;
    }
    let prior = PRIOR_FORCE.swap(0, Ordering::SeqCst);
    let _ = request_graph_quantum(if prior > 0 { Some(prior) } else { None });
    if let Some(m) = CLAIM_MARKER.get() {
        let _ = std::fs::remove_file(m);
    }
}

/// The crash road: a launch finding a marker whose value matches the force
/// the graph still carries is looking at its own predecessor's unreleased
/// claim, and takes it off. A force that does NOT match is somebody else's
/// and is left alone; the stale marker goes either way.
#[cfg(target_os = "linux")]
pub fn recover_graph_claim() {
    let Some(m) = CLAIM_MARKER.get() else { return };
    let Ok(text) = std::fs::read_to_string(m) else { return };
    let _ = std::fs::remove_file(m);
    let Ok(was): Result<u32, _> = text.trim().parse() else { return };
    if graph_clock().is_some_and(|g| g.forced == was && was > 0) {
        let _ = request_graph_quantum(None);
    }
}

#[cfg(not(target_os = "linux"))]
pub fn recover_graph_claim() {}

// ───────────────────────────────────────────────────────────────────────────
// The audio system
// ───────────────────────────────────────────────────────────────────────────

/// Which audio system every stream in this process opens through.
///
/// **Process-global, and it has to be.** A host is not a handle you can keep:
/// `cpal::Host` is not `Sync`, so it cannot live in a struct shared with the
/// audio thread, and building one is a lookup rather than a resource — which
/// is why this stores the *name* and rebuilds on demand instead. The
/// alternative was threading a `&Host` through `input_devices`, `resolve`,
/// `open_input`, the output stream and every caller of those, to express one
/// thing that is genuinely one per process: which driver stack the machine is
/// talking to.
///
/// Empty means the platform default, which is what every build before this one
/// did unconditionally.
static SYSTEM: Mutex<Option<String>> = Mutex::new(None);

/// Every audio system this build can open, in cpal's own order.
///
/// **One on macOS and Windows, two on Linux and FreeBSD.** cpal compiles in
/// exactly one host per platform unless a feature asks for more: CoreAudio on
/// macOS, ALSA on Linux (which is what PipeWire and PulseAudio both present
/// themselves as), WASAPI on Windows — plus JACK wherever the `jack` feature is
/// on, which `tangent-record/Cargo.toml` turns on for Linux and FreeBSD.
///
/// This doc comment used to say that JACK, like ASIO, "needs a development
/// library present at BUILD time" and so could not be shipped. **That was
/// wrong about JACK**, and it is worth recording why rather than just deleting:
/// `jack` 0.13 enables `dynamic_loading` by default, so libjack is `dlopen`ed
/// on first use and never linked. The artifact gains no `DT_NEEDED` entry and
/// runs unchanged on a desktop that has never heard of JACK. ASIO is still
/// genuinely build-gated — the SDK is a download with a licence to accept —
/// which is why that one is still a cargo feature and this one is not.
///
/// Listed from the real enumeration either way, so the control says what the
/// build actually has rather than what it was hoped to have. A name appearing
/// here is not a promise that the system is RUNNING: cpal's
/// `JackHost::is_available` is a hardcoded `true`, and whether a server
/// answered is settled in [`build_host`].
pub fn systems() -> Vec<String> {
    cpal::available_hosts()
        .into_iter()
        .map(|id| id.name().to_owned())
        .collect()
}

/// Choose the audio system by name. An unknown or empty name means the default.
///
/// Takes effect on the next stream OPEN, not immediately: the caller closes and
/// reopens, which is the same contract the buffer-size control already has.
pub fn set_system(name: Option<&str>) {
    let mut slot = match SYSTEM.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    *slot = name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_owned);
}

/// The chosen system's name, or `None` for the platform default.
pub fn system() -> Option<String> {
    match SYSTEM.lock() {
        Ok(g) => g.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Build the host for the wanted system name.
///
/// Falls back to the default host when the saved name names a system this
/// build does not have — which is what a settings file written on a machine
/// with JACK, opened on one without it, looks like. Silently, because the
/// alternative is refusing to open any audio at all over a preference.
///
/// **And falls back the same way when JACK is named but no server answered.**
/// cpal's JACK host reports `is_available() == true` unconditionally and its
/// constructor cannot fail: with no server running it prints its error, creates
/// no devices, and hands back a perfectly valid `Host` that owns nothing. Seated
/// on that, the app is silent — no output, no input, no message — and the only
/// way out is a settings panel the user has no reason to suspect. Since "JACK"
/// is a name every Linux and FreeBSD build now offers whether or not the
/// machine runs it, that is a trap the picker would otherwise set on the
/// majority of desktops. A host with neither an input nor an output device is
/// not a preference worth honouring, so it is not honoured.
fn build_host(want: &Option<String>) -> cpal::Host {
    #[cfg(test)]
    HOSTS_BUILT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if let Some(want) = want {
        return cpal::available_hosts()
            .into_iter()
            .find(|id| id.name() == *want)
            .and_then(|id| cpal::host_from_id(id).ok())
            .filter(|host| host_has_a_device(host))
            .unwrap_or_else(cpal::default_host);
    }
    // **No stated preference: the PLATFORM DEFAULT — WASAPI on Windows.**
    //
    // This used to be "ASIO first where it exists", on the theory that
    // every tester with an interface has its driver. The first real
    // Windows tester had no interface at all — and Realtek ships a
    // generic ASIO wrapper, so the probe found "an ASIO device", seated
    // the whole app on a driver stack this codebase has never run in
    // anger, and the output opened i24 and failed: a silent app on first
    // launch, on stock hardware, with a settings knob as the only way
    // out. The safe door is the one every Windows machine actually has.
    // ASIO is still fully offered — as an explicit CHOICE in settings,
    // where the person picking it knows they have an interface.
    cpal::default_host()
}

/// Whether a freshly built host owns any device at all.
///
/// **Asked of JACK and of nothing else, and that narrowness is the point.**
/// The question is cheap and side-effect-free for the JACK host — its
/// constructor has already made its clients and pushed whatever it got into a
/// `Vec`, so this reads a field. It is neither of those things for ASIO, where
/// walking the device list is the very operation the `HOST_CACHE` note above
/// exists to keep away from a live stream: asio-sys's loader is process-global
/// and enumerating drivers can unload the one currently playing. So a general
/// "does this host have devices" probe would buy a Linux and FreeBSD
/// improvement by reintroducing a Windows fault that took a tester's silence
/// to find. It is matched by NAME rather than by `HostId::Jack` because that
/// variant only exists where the `jack` feature is compiled in, and this
/// function should read the same on every platform.
///
/// Everything else is trusted: a CoreAudio, WASAPI or ALSA host that enumerates
/// nothing is a machine with no sound card, and the default host will report
/// exactly the same nothing. There is no better answer to fall back TO.
fn host_has_a_device(host: &cpal::Host) -> bool {
    use cpal::traits::HostTrait;
    if host.id().name() != "JACK" {
        return true;
    }
    host.default_output_device().is_some() || host.default_input_device().is_some()
}

/// How many times a host was actually constructed, for the cache test.
#[cfg(test)]
static HOSTS_BUILT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

thread_local! {
    /// The ONE host per thread, keyed by the system name it was built for.
    ///
    /// **This cache is the ASIO fix, not an optimisation.** cpal builds a
    /// fresh backend context per `Host` (`asio::Host::new` makes a fresh
    /// `sys::Asio`), and asio-sys's one-driver-at-a-time guard lives on that
    /// instance — while the loader underneath (the ASIO SDK's `AsioDrivers`)
    /// is PROCESS-GLOBAL and unloads the current driver to load the next.
    /// So a second `Host`, with its empty guard, happily asks the SDK to load
    /// a driver for enumeration and thereby TEARS DOWN the driver the first
    /// host's stream is playing through. That is "switch to ASIO and the app
    /// goes silent": the output opened on one fresh host, then the input
    /// enumeration walked every registered driver through another.
    ///
    /// One instance per thread ends it, verified against asio-sys 0.2.6:
    /// through a SHARED instance, `load_driver` returns the already-loaded
    /// driver for the same name and `Err(DriverAlreadyExists)` for any other —
    /// it never touches the SDK loader while a driver is live. Enumeration
    /// while a stream runs then lists exactly the live driver, and the input
    /// opens on the same duplex device the output owns, which is the only
    /// correct answer a single-client ASIO driver has.
    ///
    /// Thread-local rather than global because `cpal::Host` is not guaranteed
    /// `Send`/`Sync` across every backend — and every real open in this app
    /// happens on the one UI thread anyway (`cpal::Stream` is `!Send`, so the
    /// streams already live there).
    static HOST_CACHE: std::cell::RefCell<Option<(Option<String>, cpal::Host)>> =
        const { std::cell::RefCell::new(None) };
}

/// Run `f` against THE host for the chosen system — cached, never rebuilt
/// until the system choice changes.
///
/// Every device lookup and stream open in the process goes through here; a
/// fresh `cpal::Host` outside this cache is how the ASIO teardown bug happened,
/// so none are made (see [`HOST_CACHE`]). Not re-entrant: `f` must not call
/// `with_host` again — internal helpers take `&cpal::Host` instead.
pub fn with_host<T>(f: impl FnOnce(&cpal::Host) -> T) -> T {
    let want = system();
    HOST_CACHE.with(|cache| {
        let mut slot = cache.borrow_mut();
        let stale = match &*slot {
            Some((key, _)) => *key != want,
            None => true,
        };
        if stale {
            // Build BEFORE dropping the old one so a panic in construction
            // leaves the previous host usable. Dropping the old host is safe
            // even mid-life: no cpal backend unloads anything on Host drop —
            // devices and streams own their driver handles (ASIO's `Driver`
            // Arc included), so a stream opened through the old host plays on.
            let built = build_host(&want);
            *slot = Some((want, built));
        }
        let (_, host) = slot.as_ref().expect("the cache was just filled");
        f(host)
    })
}

/// Which device to open.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum InputSelection {
    /// Whatever the host says is default, opened through cpal's own
    /// `default_input_device()` handle.
    ///
    /// **Deliberately not "look up the default's name and then find it again".**
    /// That round trip is where a two-identical-interfaces setup picks the wrong
    /// one, and it is avoidable for the default because cpal hands us the actual
    /// device object.
    #[default]
    Default,
    Key(DeviceKey),
    /// A PipeWire node, opened through the `pipewire` PCM bound to it.
    ///
    /// The only way to reach a real interface on a PipeWire machine: PipeWire
    /// holds each card exclusively, so ALSA hands cpal `EBUSY` for everything
    /// except the `pipewire` PCM itself — which follows the system default
    /// source unless it is told which node to bind. See
    /// [`pipewire_sources`] and [`with_pipewire_node`].
    PipewireNode(String),
}

impl fmt::Display for InputSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => f.write_str("(system default)"),
            Self::Key(k) => fmt::Display::fmt(k, f),
            Self::PipewireNode(n) => f.write_str(n),
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Errors
// ───────────────────────────────────────────────────────────────────────────

/// Everything that can go wrong before a stream is running.
///
/// Device loss *after* it is running is not here: it is not an error the caller
/// can be handed, it is a state the session has to react to mid-take. See
/// [`DeviceState`].
#[derive(Debug)]
pub enum AudioError {
    /// The host could not enumerate at all. On macOS with a hardened runtime and
    /// no `com.apple.security.device.audio-input` entitlement this is what an
    /// empty world looks like — see RECORDER-PLAN §0.
    Enumeration(String),
    NoDefaultInput,
    NotFound(DeviceKey),
    /// The device exists but offers nothing this module can convert.
    NoUsableConfig {
        device: String,
        offered: Vec<SampleFormat>,
    },
    Build(String),
    Play(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Enumeration(e) => write!(f, "could not list audio inputs: {e}"),
            Self::NoDefaultInput => f.write_str("this machine has no default audio input"),
            Self::NotFound(k) => write!(f, "audio input \"{k}\" is not connected"),
            Self::NoUsableConfig { device, offered } => write!(
                f,
                "\"{device}\" offers no sample format this recorder can read (offered: {offered:?})"
            ),
            Self::Build(e) => write!(f, "could not open the audio input: {e}"),
            Self::Play(e) => write!(f, "could not start the audio input: {e}"),
        }
    }
}

impl Error for AudioError {}

// ───────────────────────────────────────────────────────────────────────────
// Choosing a configuration
// ───────────────────────────────────────────────────────────────────────────

/// Which of a device's inputs to record.
///
/// **Any channel, and any PAIR of channels — including 2/3.** An interface's
/// inputs are numbered on its front panel and people use them in whatever
/// arrangement the session needs: a mono overhead on 5, a stereo pair on 2 and
/// 3 because 1 has the vocal in it. cpal can express none of that: asking for
/// two channels gives you the first two (see [`ConfigWish::channels`]), so the
/// device is opened with everything it has and the pick happens where the
/// interleaved buffer enters the app.
///
/// Channel numbers here are ZERO-based, because that is what indexes a frame.
/// Everything a person reads is one-based, because that is what is printed on
/// the box, and the conversion happens at the edge where the label is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPick {
    /// One input, recorded as mono.
    Mono(u16),
    /// Two inputs as a stereo pair — left, right. They need not be adjacent
    /// and they need not be in order: `Stereo(3, 1)` is a legitimate request
    /// from somebody whose interface is patched that way.
    Stereo(u16, u16),
}

impl ChannelPick {
    /// How many channels come OUT of it.
    pub fn channels(self) -> usize {
        match self {
            ChannelPick::Mono(_) => 1,
            ChannelPick::Stereo(..) => 2,
        }
    }

    /// The source index for output channel `c`, if it is in range.
    fn source(self, c: usize, device_channels: usize) -> Option<usize> {
        let want = match (self, c) {
            (ChannelPick::Mono(a), 0) | (ChannelPick::Stereo(a, _), 0) => a,
            (ChannelPick::Stereo(_, b), 1) => b,
            _ => return None,
        };
        Some(usize::from(want)).filter(|w| *w < device_channels)
    }

    /// Whether every channel it names exists on a device this wide.
    ///
    /// A pick that half-fits is refused entirely rather than half-honoured: a
    /// stereo pair whose right channel has gone missing should fall back to the
    /// whole device, not silently become "left twice".
    pub fn fits(self, device_channels: usize) -> bool {
        (0..self.channels()).all(|c| self.source(c, device_channels).is_some())
    }
}

/// How many inputs of one interface can be open at once.
///
/// **One interface, several inputs.** A second DEVICE is a second clock and is
/// declined on purpose; anyone with that rig makes an aggregate device, which
/// presents as one device with one clock and arrives here as the ordinary case.
pub const MAX_PICKS: usize = 4;

/// The inputs to keep, in the order they appear in the stream.
///
/// **One stream, one ring, channels laid end to end.** Picking input 6 and
/// inputs 1/2 out of an eighteen-channel interface produces a three-channel
/// stream: the mono one, then the pair. Every consumer downstream sees one
/// interleaved block and needs to know nothing about where it came from — and
/// there is exactly one callback, one clock and one set of timing marks, which
/// is what keeps several inputs sample-aligned with each other for free.
///
/// Fixed size rather than a `Vec` so it stays `Copy` and allocates nothing:
/// this is read on the audio callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Picks([Option<ChannelPick>; MAX_PICKS]);

impl Picks {
    /// Everything the device has, in its own order.
    pub const EVERYTHING: Self = Self([None; MAX_PICKS]);

    /// One input, which is what every caller wanted before there could be more.
    pub fn one(pick: ChannelPick) -> Self {
        let mut out = Self::EVERYTHING;
        out.0[0] = Some(pick);
        out
    }

    /// As many as fit, in the order given. Extras beyond [`MAX_PICKS`] are
    /// dropped — a desk with a fixed number of input strips cannot show them.
    pub fn from_slice(picks: &[ChannelPick]) -> Self {
        let mut out = Self::EVERYTHING;
        for (slot, pick) in out.0.iter_mut().zip(picks) {
            *slot = Some(*pick);
        }
        out
    }

    pub fn iter(&self) -> impl Iterator<Item = ChannelPick> + '_ {
        self.0.iter().flatten().copied()
    }

    /// How many inputs, not how many channels.
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// No picks at all means the whole device, which is a real and common
    /// answer rather than a missing one.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many channels come OUT, summed over every pick.
    pub fn channels(&self) -> usize {
        self.iter().map(ChannelPick::channels).sum()
    }

    /// Which output channel the `i`th pick's own first channel lands on.
    pub fn offset(&self, i: usize) -> usize {
        self.iter().take(i).map(ChannelPick::channels).sum()
    }

    /// The device channel that feeds output channel `c`, if it is in range.
    ///
    /// Walks the picks rather than indexing, because a pick is one or two
    /// channels and the answer depends on every pick before it.
    pub fn source(&self, c: usize, device_channels: usize) -> Option<usize> {
        let mut at = 0;
        for pick in self.iter() {
            let w = pick.channels();
            if c < at + w {
                return pick.source(c - at, device_channels);
            }
            at += w;
        }
        None
    }

    /// How many channels each pick contributes, in order — the layout the
    /// monitor's other end shares out between input strips.
    ///
    /// On `Picks` rather than only on the sink, because the UI thread has to
    /// compute it when picks change LIVE, without the sink (which lives on
    /// the writer's thread).
    pub fn widths<const N: usize>(&self, device_channels: usize) -> [u8; N] {
        if self.is_empty() {
            let mut out = [0u8; N];
            if let Some(first) = out.first_mut() {
                *first = device_channels.min(u8::MAX as usize) as u8;
            }
            return out;
        }
        let mut it = self.iter();
        std::array::from_fn(|_| it.next().map_or(0, |p| p.channels() as u8))
    }

    /// Drop the picks this device cannot honour, keeping the rest.
    ///
    /// **Per pick, not all-or-nothing.** An interface that came back from a hub
    /// with fewer inputs should cost you the input that is gone and not the two
    /// that are still there. A pick that half-fits is dropped whole — see
    /// [`ChannelPick::fits`] — and if nothing is left the stream falls back to
    /// the whole device, which is what "no picks" already means.
    pub fn fitted(self, device_channels: usize) -> Self {
        let kept: Vec<ChannelPick> = self.iter().filter(|p| p.fits(device_channels)).collect();
        Self::from_slice(&kept)
    }
}

/// What the caller would like, all of it optional.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConfigWish {
    /// `None` means the device's own default rate. Resampling is out of scope
    /// (RECORDER-PLAN §3: correct the timestamps, never resample), so this is
    /// the rate the WAV will declare.
    pub sample_rate: Option<u32>,
    /// `None` means the device's own default channel count.
    ///
    /// Overriding it is a sharper knife than it looks: cpal numbers channels
    /// from the device's first input, so asking an 18-in interface for 2
    /// channels records inputs 1 and 2 and silently ignores a piano plugged into
    /// 3 and 4.
    ///
    /// Which is what [`pick_channel`](Self::pick_channel) is for, and why the
    /// two must not be used together.
    pub channels: Option<u16>,
    pub buffer_frames: Option<u32>,
    /// Keep ONE of the device's inputs, counted from 0, and present it as mono.
    ///
    /// **The answer to the paragraph above.** The piano plugged into input 3 of
    /// an 18-in interface cannot be reached by asking for fewer channels — that
    /// takes them from the front. So the device is opened with everything it
    /// has, exactly as it would be otherwise, and the one channel is taken at
    /// the point where the callback's interleaved buffer enters the app
    /// ([`CaptureSource::accept`]). Everything downstream — the ring, the
    /// marks, the meter, the WAV's channel count — then sees a mono stream and
    /// needs to know nothing about it.
    ///
    /// Out of range for the device that actually opens is ignored rather than
    /// refused: an interface can come back from a hub with fewer inputs than it
    /// had, and a saved "channel 12" should cost you the channel, not the
    /// session.
    pub picks: Picks,
}

/// Fidelity ranking of the sample formats this module can convert to `f32`.
///
/// `None` means "cannot read it", and that has to be a real branch rather than a
/// fallback, because `SampleFormat` is `#[non_exhaustive]` and picking a format
/// FTZ + DAZ on this thread (x86 only; a no-op elsewhere) — bit 15 is
/// flush-to-zero, bit 6 denormals-are-zero. A tail decaying into
/// subnormals runs orders of magnitude slower on x86 without them.
#[inline]
fn flush_denormals() {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: MXCSR is per-thread state; these bits only change how this
    // thread rounds subnormals, which is the point.
    unsafe {
        use core::arch::x86_64::{_mm_getcsr, _mm_setcsr};
        _mm_setcsr(_mm_getcsr() | 0x8040);
    }
}

/// we cannot convert would fail at `build_input_stream` with cpal's own
/// `expect("host supplied incorrect sample type")` — a panic inside the audio
/// callback, on the user's machine, with no take.
fn format_rank(f: SampleFormat) -> Option<u8> {
    Some(match f {
        // F32 first because it is what CoreAudio delivers natively and because
        // f32's 24-bit mantissa holds a 24-bit converter's output exactly, so
        // preferring I32 for "more bits" would buy nothing and cost a
        // conversion.
        SampleFormat::F32 => 6,
        SampleFormat::I32 => 5,
        SampleFormat::I24 => 4,
        SampleFormat::I16 => 3,
        SampleFormat::U16 => 2,
        SampleFormat::I8 => 1,
        SampleFormat::U8 => 0,
        _ => return None,
    })
}

/// Pick the best supported configuration, or `None` if nothing is readable.
///
/// Priority, in order: reaches the wanted rate; matches the wanted channel
/// count; then highest-fidelity readable format; then, as a last tie-break, the
/// lowest channel count, so a device that offers both 2 and 18 channels does not
/// hand us an eighteen-channel WAV by default.
///
/// A pure function over the ranges cpal reports, so the policy is testable with
/// no interface attached — which matters, because the failure it prevents (an
/// unreadable format chosen and then panicking inside the callback) only ever
/// reproduces on hardware nobody on the project owns.
pub fn pick_input_config(
    ranges: impl IntoIterator<Item = SupportedStreamConfigRange>,
    wish: &ConfigWish,
    device_default: Option<&SupportedStreamConfig>,
) -> Option<SupportedStreamConfig> {
    let want_rate = wish
        .sample_rate
        .or_else(|| device_default.map(|d| d.sample_rate().0));
    let want_channels = wish
        .channels
        .or_else(|| device_default.map(|d| d.channels()));

    let mut best: Option<(u8, bool, bool, std::cmp::Reverse<u16>, SupportedStreamConfig)> = None;
    for range in ranges {
        let Some(rank) = format_rank(range.sample_format()) else {
            continue;
        };
        let rate_hits = want_rate.is_some_and(|r| {
            range.min_sample_rate().0 <= r && r <= range.max_sample_rate().0
        });
        let rate = match want_rate {
            Some(r) if rate_hits => SampleRate(r),
            // Not the minimum: a device whose range starts at 8 kHz would give a
            // telephone-quality take rather than an obviously broken one, and
            // obviously broken is easier to report.
            _ => range.max_sample_rate(),
        };
        let channels_hit = want_channels == Some(range.channels());
        let candidate = (
            rank,
            rate_hits,
            channels_hit,
            std::cmp::Reverse(range.channels()),
            range.with_sample_rate(rate),
        );
        let better = match &best {
            None => true,
            Some(current) => {
                (candidate.1, candidate.2, candidate.0, candidate.3)
                    > (current.1, current.2, current.0, current.3)
            }
        };
        if better {
            best = Some(candidate);
        }
    }
    best.map(|(_, _, _, _, config)| config)
}

/// The configuration a stream was actually opened with, for the take manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenConfig {
    pub device: String,
    pub channels: u16,
    /// The **nominal** rate: what the WAV will declare, and what
    /// [`RateFit::epsilon`] is measured against.
    pub sample_rate: u32,
    pub sample_format: SampleFormat,
    pub buffer_size: BufferSize,
}

// ───────────────────────────────────────────────────────────────────────────
// Stream state and counters
// ───────────────────────────────────────────────────────────────────────────

/// What the device is doing, as far as the callback and the error callback know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Running,
    /// cpal reported `StreamError::DeviceNotAvailable`: the interface was
    /// unplugged, browned out, or the hub re-enumerated. Policy (RECORDER-PLAN
    /// §4) is to stop the take, finalise every file that has bytes and mark it
    /// incomplete. **Never discard what was already captured.**
    Lost,
    /// A backend-specific error. Recoverable in principle; treated as fatal to
    /// the take because a take with an unexplained hole is worse than a short one.
    Errored,
}

impl DeviceState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Lost,
            2 => Self::Errored,
            _ => Self::Running,
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Self::Running => 0,
            Self::Lost => 1,
            Self::Errored => 2,
        }
    }

    pub fn is_running(self) -> bool {
        matches!(self, Self::Running)
    }
}

/// Counters the callback writes and everyone else reads.
///
/// All `Relaxed`: these are statistics, not synchronisation. Nothing is
/// published *through* them — the samples travel through the ring, which does
/// its own acquire/release — so a reader that sees a stale count is one repaint
/// behind and nothing more.
///
/// **Why this exists at all, given the marks already carry the drop counts:** the
/// UI wants a number every frame without draining anything, and a take that
/// dropped samples has to be able to say so even if the writer thread is the
/// thing that died.
#[derive(Debug, Default)]
pub struct CaptureStats {
    frames_captured: AtomicU64,
    /// Frames per callback, **counted rather than requested**. Zero until the
    /// first callback arrives.
    ///
    /// The input's half of the number the output has carried since the panel
    /// started disagreeing with the room. It matters more here than it looks:
    /// under pipewire-alsa the delivered cadence is the GRAPH's quantum and
    /// has nothing to do with what was asked for, and a capture ring smaller
    /// than that quantum keeps `ring / quantum` of the audio and reports
    /// nothing at all — no EPIPE, no xrun, a clean stream at a fraction of the
    /// rate. This is the number that makes that visible.
    block_seen: AtomicU32,
    /// The ring asked of the host when the stream opened, in frames. Zero
    /// where no fixed size was asked for.
    ///
    /// Recorded because it is no longer the same as what the app requested:
    /// a ring has to cover the graph's cycle, so the opened ring can be
    /// deliberately wider than the setting. A manifest that states neither
    /// cannot tell a widening from a user's choice.
    ring_asked: AtomicU32,
    frames_dropped: AtomicU64,
    xruns: AtomicU64,
    marks_dropped: AtomicU64,
    callbacks: AtomicU64,
    stream_errors: AtomicU64,
    device_state: AtomicU8,
}

impl CaptureStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Frames the **device** produced, including any the ring refused. This is
    /// the number the rate fit is indexed by, and it is deliberately not the
    /// number of frames in the file.
    pub fn frames_captured(&self) -> u64 {
        self.frames_captured.load(Ordering::Relaxed)
    }

    /// What the callback was really handed. `None` before the first one.
    pub fn block_seen(&self) -> Option<u32> {
        let n = self.block_seen.load(Ordering::Relaxed);
        (n > 0).then_some(n)
    }

    /// The ring asked of the host, `None` where none was asked for — which is
    /// a different claim from a ring that happens to equal the block, and is
    /// why this is not derived from [`block_seen`](Self::block_seen).
    pub fn ring_asked(&self) -> Option<u32> {
        let n = self.ring_asked.load(Ordering::Relaxed);
        (n > 0).then_some(n)
    }

    /// Record what the stream was opened with. Called once, from `open_input`.
    pub fn set_ring_asked(&self, frames: u32) {
        self.ring_asked.store(frames, Ordering::Relaxed);
    }

    /// Frames that never reached the ring. Non-zero means the take has a splice
    /// in it. No clock model fixes that; the take has to say so.
    pub fn frames_dropped(&self) -> u64 {
        self.frames_dropped.load(Ordering::Relaxed)
    }

    /// Callbacks that lost at least one frame. Distinct from
    /// [`frames_dropped`](Self::frames_dropped) because one long stall and a
    /// thousand tiny ones are different problems with different fixes.
    pub fn xruns(&self) -> u64 {
        self.xruns.load(Ordering::Relaxed)
    }

    /// Buffers whose timing mark could not be queued. Their samples are dropped
    /// too, on purpose — see [`CaptureSource::accept`].
    pub fn marks_dropped(&self) -> u64 {
        self.marks_dropped.load(Ordering::Relaxed)
    }

    pub fn callbacks(&self) -> u64 {
        self.callbacks.load(Ordering::Relaxed)
    }

    pub fn stream_errors(&self) -> u64 {
        self.stream_errors.load(Ordering::Relaxed)
    }

    pub fn device_state(&self) -> DeviceState {
        DeviceState::from_u8(self.device_state.load(Ordering::Relaxed))
    }

    /// Called from cpal's error callback. Never panics, never allocates, never
    /// blocks: on some backends this runs on the audio thread.
    pub fn note_error(&self, err: &StreamError) {
        self.stream_errors.fetch_add(1, Ordering::Relaxed);
        let state = match err {
            StreamError::DeviceNotAvailable => DeviceState::Lost,
            StreamError::BackendSpecific { .. } => DeviceState::Errored,
        };
        self.device_state.store(state.as_u8(), Ordering::Relaxed);
    }

    /// Clear the counters for a new take. Called between takes, never during one.
    pub fn reset(&self) {
        self.frames_captured.store(0, Ordering::Relaxed);
        self.frames_dropped.store(0, Ordering::Relaxed);
        self.xruns.store(0, Ordering::Relaxed);
        self.marks_dropped.store(0, Ordering::Relaxed);
        self.callbacks.store(0, Ordering::Relaxed);
        // **`block_seen` and `ring_asked` are deliberately NOT cleared.** They
        // are properties of the open stream rather than counters of a take:
        // the ring was asked for once when the device opened, and the block is
        // a rolling observation of a cadence that does not restart. Clearing
        // them would make the first report of every take say "nobody knows"
        // until a callback happened to land, which is the shape of answer this
        // whole family of fields exists to stop giving.
    }
}

// ───────────────────────────────────────────────────────────────────────────
// The ring and its two ends
// ───────────────────────────────────────────────────────────────────────────

/// One callback's worth of evidence about where the device is in time.
///
/// Small, `Copy`, and pushed *after* the samples it describes, so a consumer
/// that has a mark can rely on the samples already being committed — `rtrb`'s
/// release on the sample push happens-before the release on the mark push.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimingMark {
    /// Absolute index of the first frame of this buffer, counting every frame
    /// the device produced since the stream opened — **including** frames the
    /// ring later refused. Absolute rather than incremental so that a mark lost
    /// to a full mark-ring is still visible as a gap. See [`FrameCursor`].
    pub first_frame: u64,
    pub frames_pushed: u32,
    /// Frames of this buffer that did not fit. They are always the **tail** of
    /// the buffer, never a hole in the middle: see [`CaptureSource::accept`].
    pub frames_dropped: u32,
    /// The device's capture stamp, re-based onto the first stamp this stream
    /// ever produced. `None` when cpal's `StreamInstant` could not be read or
    /// went backwards past that first stamp.
    pub device_ns: Option<u64>,
    /// A timebase reading taken in the same callback. This is the other half of
    /// the anchor.
    pub host_ns: Nanos,
}

impl TimingMark {
    /// Frames the device delivered in this callback, kept or not.
    pub fn frames_delivered(&self) -> u64 {
        self.frames_pushed as u64 + self.frames_dropped as u64
    }

    /// The first frame index of the *next* buffer, if nothing is lost between.
    pub fn end_frame(&self) -> u64 {
        self.first_frame + self.frames_delivered()
    }
}

/// Build a capture channel. `ring_frames` is the audio ring's depth in frames;
/// `mark_slots` is how many callbacks the writer thread may fall behind by
/// before marks start being refused.
pub fn capture_channel(
    channels: usize,
    ring_frames: usize,
    mark_slots: usize,
    stats: Arc<CaptureStats>,
) -> (CaptureSource, CaptureSink) {
    capture_channel_from(channels, Picks::EVERYTHING, ring_frames, mark_slots, stats)
}

/// As [`capture_channel`], but keeping only one of the device's inputs.
///
/// `device_channels` is what the callback will deliver per frame;
/// `pick` is the one to keep, counted from 0. With `pick` set the ring, the
/// sink and everything downstream are MONO — that is the whole point, and it is
/// why the ring is sized from the output count and not the device's.
///
/// An out-of-range `pick` is dropped rather than refused, so a saved channel
/// that no longer exists costs the channel and not the session. See
/// [`ConfigWish::pick_channel`].
pub fn capture_channel_from(
    device_channels: usize,
    picks: Picks,
    ring_frames: usize,
    mark_slots: usize,
    stats: Arc<CaptureStats>,
) -> (CaptureSource, CaptureSink) {
    capture_channel_monitored(device_channels, picks, ring_frames, mark_slots, stats, 0).0
}

/// As [`capture_channel_from`], with a shallow tap for live monitoring.
///
/// `monitor_frames` of 0 means no tap at all, which is what every caller that
/// is not opening a real input wants.
pub fn capture_channel_monitored(
    device_channels: usize,
    picks: Picks,
    ring_frames: usize,
    mark_slots: usize,
    stats: Arc<CaptureStats>,
    monitor_frames: usize,
) -> ((CaptureSource, CaptureSink), Option<Consumer<f32>>) {
    assert!(
        device_channels > 0,
        "a capture channel needs at least one channel"
    );
    // A pick that does not fit the device is dropped whole. See
    // `ChannelPick::fits`: half-honouring a stereo pair whose right channel has
    // gone missing would record "left twice" and say nothing. The picks that DO
    // fit are kept — losing one input is not a reason to lose the others.
    let picks = picks.fitted(device_channels);
    let channels = if picks.is_empty() {
        device_channels
    } else {
        picks.channels()
    };
    // **The rings carry WHOLE DEVICE FRAMES, whatever is picked.**
    //
    // The picks used to decide the ring's width, which is why choosing an
    // input had to close the device and open it again: the layout was baked
    // into the buffers. That reopen is tens of milliseconds of silence in the
    // middle of a session, on the one gesture people make most while
    // building a desk.
    //
    // Fixing the width at the device's own channel count makes a pick change
    // free. It also makes it SAFE, which matters more here than the
    // milliseconds: a frame in this ring means exactly one thing for the life
    // of the stream, so a pick that changes while the callback is mid-block
    // cannot leave a half-old frame for the writer to interleave wrongly -
    // the failure this module's rule 1 exists to prevent. The picks move to
    // the two places that READ the ring, both of which can change their mind
    // between blocks without telling the audio thread anything.
    //
    // The cost is honest: the ring is as wide as the interface rather than as
    // wide as what is used, and the callback copies every channel. Three
    // seconds of an eighteen-in interface is about ten megabytes, and the
    // copy is a straight run rather than the strided per-sample lookup it
    // replaces - the callback does LESS work than it did.
    let (sample_tx, sample_rx) = RingBuffer::<f32>::new(ring_frames.max(1) * device_channels);
    let (mark_tx, mark_rx) = RingBuffer::<TimingMark>::new(mark_slots.max(1));
    let mut source = CaptureSource {
        samples: sample_tx,
        marks: mark_tx,
        stats: Arc::clone(&stats),
        channels: device_channels,
        device_channels,
        frames: 0,
        stamp_epoch: None,
        scratch: vec![0.0; CHUNK_FRAMES * device_channels],
        monitor: None,
    };
    let sink = CaptureSink {
        samples: sample_rx,
        marks: mark_rx,
        stats,
        channels,
        device_channels,
        picks,
        lift: Vec::new(),
    };
    let monitor = (monitor_frames > 0).then(|| {
        let (tx, rx) = RingBuffer::<f32>::new(monitor_frames * device_channels);
        source.monitor = Some(tx);
        rx
    });
    ((source, sink), monitor)
}

/// The audio-callback half. Everything it needs and nothing that can block.
///
/// Deliberately usable without cpal: [`accept`](Self::accept) takes a plain
/// slice and two already-converted numbers, so every behaviour that matters —
/// frame alignment, drop accounting, the mark stream, the sample-format
/// conversion — is exercised in the tests below with no device in the room.
#[derive(Debug)]
pub struct CaptureSource {
    samples: Producer<f32>,
    marks: Producer<TimingMark>,
    stats: Arc<CaptureStats>,
    /// Channels going OUT, into the ring. One when a channel is picked.
    channels: usize,
    /// Channels coming IN, from the callback. The two differ only when a
    /// channel is picked, and confusing them swaps the interleaving for the
    /// rest of the take — which is rule 1 of `accept`, one level down.
    device_channels: usize,
    frames: u64,
    stamp_epoch: Option<Nanos>,
    scratch: Vec<f32>,
    /// A copy of every frame, for hearing the input live.
    ///
    /// **Its own ring, not the take's.** The take's ring is drained by the
    /// writer thread on its own schedule and is four seconds deep on purpose;
    /// a monitor needs the newest audio a few milliseconds after it arrives, so
    /// it gets a shallow ring of its own and the two never wait on each other.
    ///
    /// Whether anybody is listening is not this side's business. The renderer
    /// drains this every block regardless — see `Renderer::mix_monitor` — so a
    /// monitor switched on plays what is happening now rather than whatever was
    /// left in a ring that had been filling since the device opened.
    monitor: Option<Producer<f32>>,
}

impl CaptureSource {
    /// Take one callback's buffer.
    ///
    /// `device_ns` is this buffer's capture stamp from
    /// [`stream_instant_nanos`]; `host_ns` is a timebase reading from the same
    /// callback.
    ///
    /// Three rules are enforced here and each one is a named bug:
    ///
    /// 1. **Whole frames only.** A partial push that splits an interleaved frame
    ///    swaps the channels for the rest of the take.
    /// 2. **A short push stops the loop.** Once a chunk does not fit, the rest of
    ///    the buffer is dropped even if the consumer frees room in the meantime,
    ///    so the lost frames are always a contiguous tail. Otherwise a dropout
    ///    could be a hole in the *middle* of a buffer, and `frames_dropped`
    ///    would no longer say where it was.
    /// 3. **No mark, no samples.** If the mark ring is full the whole buffer is
    ///    dropped rather than pushed undescribed, because a frame in the ring
    ///    that no mark accounts for is a frame the writer thread will place at
    ///    the wrong time — silent desync, which is the failure this entire
    ///    feature exists to avoid.
    pub fn accept<T>(&mut self, data: &[T], device_ns: Option<Nanos>, host_ns: Nanos)
    where
        T: Copy,
        f32: FromSample<T>,
    {
        self.stats.callbacks.fetch_add(1, Ordering::Relaxed);

        // IN and OUT, which are the same number unless a channel is picked.
        let ch_in = self.device_channels;
        let ch = self.channels;
        // A trailing partial frame is a malformed buffer, not audio. Truncating
        // is the only option that keeps the interleaving honest.
        let frames_in = data.len() / ch_in;
        let first_frame = self.frames;
        self.frames += frames_in as u64;
        self.stats
            .frames_captured
            .fetch_add(frames_in as u64, Ordering::Relaxed);
        // Counted here rather than asked for at open: what the graph delivers
        // per cycle is its business, not ours. See `CaptureStats::block_seen`.
        self.stats
            .block_seen
            .store(frames_in as u32, Ordering::Relaxed);
        if frames_in == 0 {
            return;
        }

        if self.marks.slots() == 0 {
            self.stats.marks_dropped.fetch_add(1, Ordering::Relaxed);
            self.note_drop(frames_in);
            return;
        }

        let stamp = device_ns.and_then(|ns| self.rebase(ns));

        let chunk_frames = self.scratch.len() / ch;
        let mut pushed = 0usize;
        while pushed < frames_in {
            let want = chunk_frames.min(frames_in - pushed);
            let n = want * ch;
            let dst = &mut self.scratch[..n];
            // **Everything, in order.** `ch == ch_in` for the life of the
            // stream now (see `capture_channel_monitored`), so this is a flat
            // run over the buffer and the only case there is. Choosing inputs
            // happens where the ring is READ.
            let src = &data[pushed * ch_in..pushed * ch_in + want * ch_in];
            for (d, s) in dst.iter_mut().zip(src) {
                *d = f32::from_sample(*s);
            }
            // The monitor gets the same samples, at the same point: whole
            // device frames, before the take's ring. What does not fit whole is
            // dropped and not counted — a listener missing a millisecond is not
            // a take losing a frame, and putting the two in one counter would
            // make an idle monitor look like a failing recorder. Rule 1 binds
            // this ring too, and it was missed here once: rtrb counts
            // **samples**, so the moment the renderer's drain pops a partial
            // frame — its scratch is a fixed sample count that need not divide
            // by `ch` — the free space ends mid-frame, and `n.min(m.slots())`
            // committed the split. From then on every monitor frame played the
            // wrong lanes until the device was reopened, the exact silent swap
            // the module doc promises cannot happen.
            if let Some(m) = self.monitor.as_mut() {
                let fit = m.slots() / ch * ch;
                let _ = m
                    .write_chunk_uninit(n.min(fit))
                    .map(|c| c.fill_from_iter(dst[..n].iter().copied()));
            }
            // Rule 1: round the room down to whole frames before asking.
            let room = self.samples.slots() / ch;
            let take = want.min(room);
            if take == 0 || self.samples.push_entire_slice(&dst[..take * ch]).is_err() {
                break;
            }
            pushed += take;
            // Rule 2.
            if take < want {
                break;
            }
        }

        let dropped = frames_in - pushed;
        if dropped > 0 {
            self.note_drop(dropped);
        }

        let mark = TimingMark {
            first_frame,
            frames_pushed: pushed as u32,
            frames_dropped: dropped as u32,
            device_ns: stamp,
            host_ns,
        };
        // Cannot fail: we are the only producer and we checked for a slot above,
        // and only the consumer frees slots. Counted rather than asserted
        // because an assert here is a panic in the audio callback.
        if self.marks.push(mark).is_err() {
            self.stats.marks_dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn note_drop(&self, frames: usize) {
        self.stats
            .frames_dropped
            .fetch_add(frames as u64, Ordering::Relaxed);
        self.stats.xruns.fetch_add(1, Ordering::Relaxed);
    }

    /// Re-base a device stamp onto the first one this stream produced.
    ///
    /// Three problems die here at once. WASAPI's `StreamInstant` is raw QPC
    /// nanos while Rust's `Instant` is QPC plus `u64::MAX / 4` ns, so the two
    /// differ by ~73 years and any direct comparison overflows or lies. ALSA's
    /// starts near zero and goes *negative* for the first buffers. CoreAudio's
    /// is nanoseconds since boot, which is fine but arbitrary. Re-basing turns
    /// all three into a small non-negative number, and the constant that was
    /// removed reappears — correctly, and estimated rather than assumed — as
    /// [`SourceClock`]'s offset.
    fn rebase(&mut self, ns: Nanos) -> Option<u64> {
        let epoch = *self.stamp_epoch.get_or_insert(ns);
        u64::try_from(ns.checked_sub(epoch)?).ok()
    }
}

/// The writer-thread half.
#[derive(Debug)]
pub struct CaptureSink {
    samples: Consumer<f32>,
    marks: Consumer<TimingMark>,
    stats: Arc<CaptureStats>,
    channels: usize,
    /// The picks this stream actually honoured, AFTER the ones the device
    /// could not fit were dropped.
    ///
    /// **The layout has to come from here and not from what was asked for.**
    /// An interface narrower than it was drops a pick, and everything
    /// downstream that shares the block out between input strips would then be
    /// reading the wrong columns — silently, and with plausible audio in them.
    picks: Picks,
    /// The device's own width, which is what the ring carries.
    device_channels: usize,
    /// Scratch for lifting device frames out of the ring before the picked
    /// channels are copied out of them. Owned so the writer thread's drain
    /// allocates once rather than once a block.
    lift: Vec<f32>,
}

impl CaptureSink {
    /// The width the RINGS carry — the device's own channel count, fixed for
    /// the life of the stream. Different from [`channels`](Self::channels),
    /// which is what the TAKE contains after the picks are applied.
    pub fn device_channels(&self) -> usize {
        self.device_channels
    }

    /// The picks in force, for the monitor's other end to map with.
    pub fn picks(&self) -> Picks {
        self.picks
    }

    /// Choose different inputs, without touching the device.
    ///
    /// **This is what a pick change costs now: a field write.** The ring
    /// carries whole device frames whatever is picked, so nothing about the
    /// stream, the buffers or the callback has to change — only which columns
    /// are copied out of it, which is decided here on the writer's own
    /// thread.
    ///
    /// The caller must not do this during a take: the WAV header is written
    /// with a channel count, and changing the layout mid-file would leave the
    /// second half interleaved differently from the first. `Session` holds
    /// that rule.
    pub fn set_picks(&mut self, picks: Picks) {
        let picks = picks.fitted(self.device_channels);
        self.channels = if picks.is_empty() {
            self.device_channels
        } else {
            picks.channels()
        };
        self.picks = picks;
    }
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// How the block's channels are shared out between the inputs, as a width
    /// each. No picks at all is one input as wide as the device.
    pub fn widths<const N: usize>(&self) -> [u8; N] {
        if self.picks.is_empty() {
            let mut out = [0u8; N];
            if let Some(first) = out.first_mut() {
                *first = self.channels.min(u8::MAX as usize) as u8;
            }
            return out;
        }
        let mut it = self.picks.iter();
        std::array::from_fn(|_| it.next().map_or(0, |p| p.channels() as u8))
    }

    pub fn stats(&self) -> &Arc<CaptureStats> {
        &self.stats
    }

    /// Frames sitting in the ring right now.
    pub fn frames_available(&self) -> usize {
        self.samples.slots() / self.device_channels
    }

    /// Take the next timing mark, if the callback has produced one.
    pub fn next_mark(&mut self) -> Option<TimingMark> {
        self.marks.pop().ok()
    }

    /// Move up to `frames` whole frames into `out`, appending. Returns frames moved.
    pub fn pop_frames(&mut self, frames: usize, out: &mut Vec<f32>) -> usize {
        let dev = self.device_channels;
        let ch = self.channels;
        let available = self.samples.slots() / dev;
        let take = frames.min(available);
        if take == 0 {
            return 0;
        }
        // Lifted whole-device-frame first, then the picked columns copied out
        // of it. Two steps rather than one because the ring's width and the
        // file's width are different questions now — see `set_picks`.
        self.lift.clear();
        self.lift.resize(take * dev, 0.0);
        let (popped, _) = self.samples.pop_partial_slice(&mut self.lift);
        // Round down to whole DEVICE frames. Belt and braces: the push side
        // already guarantees whole frames, but a half frame reaching the WAV
        // writer swaps the channels for the rest of the file and there is no
        // downstream check that would notice.
        let got = popped.len() / dev;
        if got == 0 {
            return 0;
        }
        let start = out.len();
        out.resize(start + got * ch, 0.0);
        if self.picks.is_empty() {
            // The whole device, in its own order: one flat run.
            out[start..start + got * ch].copy_from_slice(&self.lift[..got * dev]);
        } else {
            for f in 0..got {
                for c in 0..ch {
                    // `unwrap_or(0)` rather than an index: a pick that
                    // slipped through must not panic the writer thread.
                    let src = self.picks.source(c, dev).unwrap_or(0);
                    out[start + f * ch + c] = self.lift[f * dev + src];
                }
            }
        }
        got
    }

    /// Move every whole frame currently in the ring into `out`.
    ///
    /// For the idle path — metering the input before Record is pressed — where
    /// there is no file and the marks are only wanted for the anchor. During a
    /// take, drive the drain from the marks instead ([`next_mark`](Self::next_mark)
    /// plus [`FrameCursor`]), or a dropout shifts everything after it.
    pub fn drain_samples(&mut self, out: &mut Vec<f32>) -> usize {
        self.pop_frames(self.frames_available(), out)
    }
}

/// The writer thread's picture of where the device is.
///
/// Turns the mark stream into "write this much silence, then these frames, then
/// this much silence", which is what keeps **file sample N equal to device frame
/// N** across a dropout. Padding rather than closing the gap is deliberate: a
/// closed gap shortens the take and slides every note after it earlier, which is
/// exactly the drift this feature exists to prevent, while padded silence is
/// audible, visible in a waveform, and reported in `take.json`.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameCursor {
    next: u64,
    started: bool,
}

/// What to write for one [`TimingMark`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WritePlan {
    /// Silence owed *before* this mark's samples: frames lost with their whole
    /// buffer, because the mark ring was full and no mark was queued for them.
    pub silence_before: u64,
    /// Frames to pop from the ring.
    pub frames: u32,
    /// Silence owed *after*: the tail of this buffer that did not fit.
    pub silence_after: u32,
}

impl WritePlan {
    pub fn total_frames(&self) -> u64 {
        self.silence_before + self.frames as u64 + self.silence_after as u64
    }
}

impl FrameCursor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Plan one mark. Call once per mark, in order.
    ///
    /// The first mark defines the origin, so a stream whose first callbacks were
    /// consumed before the take started does not get a silent pad the length of
    /// the pre-roll.
    pub fn plan(&mut self, mark: &TimingMark) -> WritePlan {
        let silence_before = if self.started {
            mark.first_frame.saturating_sub(self.next)
        } else {
            self.started = true;
            0
        };
        self.next = mark.end_frame();
        WritePlan {
            silence_before,
            frames: mark.frames_pushed,
            silence_after: mark.frames_dropped,
        }
    }

    /// Device frame index of the next frame this cursor expects.
    pub fn next_frame(&self) -> u64 {
        self.next
    }
}

// ───────────────────────────────────────────────────────────────────────────
// The clock hookup
// ───────────────────────────────────────────────────────────────────────────

/// Turns the mark stream into the take's rate measurement.
///
/// This is the point of the whole module. Each callback contributes one
/// `(first sample index, host time)` pair; the incremental least-squares fit in
/// [`RateFit`] recovers the device's *true* rate against the host clock, and the
/// difference from its nominal rate is the 60 ms per twenty minutes that every
/// competing tool loses (RECORDER-PLAN §3).
///
/// Two decisions inside [`observe`](Self::observe) worth defending:
///
/// **The fit is fed `to_timebase(stamp)`, not the raw stamp**, so `beta` comes
/// out in the timebase and [`Timeline::from_fit`] can use it directly. The
/// objection is that the anchor's offset is still converging during the settle
/// window, so the earliest points carry a shifting constant `δ`. For OLS the
/// resulting bias on the slope is `6·Σδ / (N·X)`, and it scales with the take:
///
/// * **Twenty minutes** (`N ≈ 112_500` callbacks, `X ≈ 5.76e7` samples): with a
///   2 s settle and `Σδ ≈ 1.8e8 ns`, that is `1.6e-4 ns/sample` on an alpha of
///   `20_833 ns/sample` — **0.008 ppm**, four orders of magnitude under the
///   50 ppm being measured.
/// * **Forty seconds** (`N ≈ 4_000`): the same `Σδ` over a twenty-eighth of the
///   span gives **~6 ppm**, which is 12% of the signal and sounds alarming right
///   up until it is converted into the only unit that matters: 6 ppm over 43 s
///   is **0.26 ms**, and a short take has almost no drift to correct in the
///   first place. The bias is large exactly where the correction is worthless.
///
/// Both numbers have a test, and the second one is a test that the *error in the
/// file* stays under half a millisecond rather than a test that the ppm figure
/// is clean — because the ppm figure is not clean and pretending otherwise would
/// be the more comfortable lie.
///
/// **The index is device frames, not file frames.** A dropout means the file is
/// short by the dropped frames, so file index and device index diverge. The
/// device index is the one the crystal actually advanced by, so it is the one
/// that makes alpha correct; keeping the file's index equal to it is
/// [`FrameCursor`]'s job, not the fit's.
#[derive(Debug, Clone)]
pub struct ClockTap {
    clock: SourceClock,
    fit: RateFit,
    unstamped: u64,
}

impl ClockTap {
    /// cpal's `StreamInstant` is already nanoseconds on every backend, so the
    /// scale is 1.0 — unlike midir (microseconds) and Media Foundation (100 ns).
    pub fn new(settle_ns: Nanos) -> Self {
        Self {
            clock: SourceClock::new(1.0, settle_ns),
            fit: RateFit::new(),
            unstamped: 0,
        }
    }

    pub fn observe(&mut self, mark: &TimingMark) {
        let Some(stamp) = mark.device_ns else {
            self.unstamped += 1;
            return;
        };
        self.clock.observe(stamp, mark.host_ns);
        if let Some(t) = self.clock.to_timebase(stamp) {
            self.fit.push(mark.first_frame, t);
        }
    }

    /// Marks that arrived with no usable device timestamp. Non-zero means the
    /// backend is not giving us a clock and the take's sync report should say
    /// so rather than presenting a fit made from a handful of points.
    pub fn unstamped(&self) -> u64 {
        self.unstamped
    }

    pub fn fit(&self) -> &RateFit {
        &self.fit
    }

    pub fn source(&self) -> &SourceClock {
        &self.clock
    }

    /// The device's measured rate in Hz. `None` until two marks with distinct
    /// frame indices have arrived.
    pub fn true_rate(&self) -> Option<f64> {
        self.fit.true_rate()
    }

    /// Rate error against the nominal rate, as a fraction. Multiply by 1e6 for ppm.
    pub fn epsilon(&self, nominal_rate: f64) -> Option<f64> {
        self.fit.epsilon(nominal_rate)
    }

    /// The timebase instant of device frame 0.
    ///
    /// This is `T_audio_sample_0` in RECORDER-PLAN §3's
    /// `T0 = max(T_audio_sample_0, T_video_frame_0, T_arm)`, and it is the only
    /// reason the anchor's *offset* matters at all: inside the audio timeline
    /// itself the offset cancels, because [`Timeline::file_seconds`] subtracts
    /// `at(t0)` from every answer.
    pub fn sample_zero(&self) -> Option<Nanos> {
        self.fit.beta().map(|b| b as Nanos)
    }

    /// A measured or OS-reported input latency, subtracted from converted stamps.
    ///
    /// **cpal 0.16 reports none.** Its macOS input path subtracts one buffer of
    /// frames and nothing else, with a `TODO` saying as much
    /// (`host/coreaudio/macos/mod.rs:594-606`); `kAudioDevicePropertyLatency` and
    /// `kAudioDevicePropertySafetyOffset` appear nowhere in the crate. So the
    /// default here is zero and `take.json` must record it as *assumed*, not
    /// *reported*. RECORDER-PLAN §3a is emphatic about the difference and it is
    /// the field that explains a constant A/V offset after the fact.
    pub fn set_latency(&mut self, latency_ns: Nanos) {
        self.clock.set_latency(latency_ns);
    }

    /// Build the take's timeline. Degrades to
    /// [`Timeline::synthetic`] on its own if the fit is too short, which is
    /// correct: 0 ppm measured and 0 ppm assumed must not look alike.
    pub fn timeline(&self, t0: Nanos, t1: Nanos, nominal_rate: f64) -> Timeline {
        Timeline::from_fit(t0, t1, nominal_rate, &self.fit)
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Levels
// ───────────────────────────────────────────────────────────────────────────

/// Linear amplitude to dBFS. `0.0` is `-inf`, not `NaN` and not a panic.
pub fn dbfs(linear: f32) -> f32 {
    if linear <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * linear.log10()
    }
}

/// The snapshot the UI reads, once per frame, under a `Mutex`.
///
/// A `Mutex` and not a channel because the UI wants the **latest** value and not
/// a history: a channel makes the meter lag by however far behind the drain the
/// UI has fallen, which is precisely wrong for a meter.
///
/// The writer thread mutates one of these in place, so the `Vec`s are allocated
/// once and reused; the UI clones. `channels` is fixed for a take.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meters {
    pub channels: usize,
    /// Highest magnitude since the previous publish.
    pub peak: Vec<f32>,
    /// Root mean square since the previous publish.
    pub rms: Vec<f32>,
    /// Peak with a decaying hold, for a meter that is readable from the bench.
    pub hold: Vec<f32>,
    /// Highest magnitude since the take was armed. **This is what kills the "I
    /// recorded silence" failure class**, and it has to be separate from `peak`
    /// because a peak over the last 20 ms says nothing about the last 20 minutes.
    pub take_peak: Vec<f32>,
    /// Latched per channel, cleared only by [`LevelTracker::arm`]. Survives the
    /// whole take by construction.
    pub clipped: Vec<bool>,
    pub clipped_samples: u64,
    /// Frames metered since arming.
    pub frames: u64,
}

impl Meters {
    pub fn new(channels: usize) -> Self {
        Self {
            channels,
            peak: vec![0.0; channels],
            rms: vec![0.0; channels],
            hold: vec![0.0; channels],
            take_peak: vec![0.0; channels],
            clipped: vec![false; channels],
            clipped_samples: 0,
            frames: 0,
        }
    }

    pub fn any_clipped(&self) -> bool {
        self.clipped.iter().any(|c| *c)
    }

    /// Put out the clip latch, and nothing else.
    ///
    /// **Not the take's history.** `take_peak` and `frames` are what say
    /// whether anything was recorded at all; a user acknowledging a red light
    /// must not also erase the evidence that the take was silent.
    pub fn clear_clip(&mut self) {
        self.clipped.iter_mut().for_each(|c| *c = false);
        self.clipped_samples = 0;
    }

    pub fn loudest_take_peak(&self) -> f32 {
        self.take_peak.iter().copied().fold(0.0f32, f32::max)
    }

    /// True when the take never rose above [`SILENCE_FLOOR_DBFS`].
    ///
    /// Reported after Stop, not during: a pianist who has not started yet is not
    /// a problem, and a warning that fires while they are getting settled is
    /// noise that teaches people to ignore warnings.
    pub fn recorded_silence(&self) -> bool {
        self.frames > 0 && dbfs(self.loudest_take_peak()) < SILENCE_FLOOR_DBFS
    }
}

/// Accumulates levels on the **writer thread**.
///
/// Not in the callback, per RECORDER-PLAN §4: the callback's only job is to get
/// samples out, and an RMS accumulator in it would be work done under a
/// real-time deadline for a number nobody reads for another 16 ms.
///
/// **The honest limitation, because it is not obvious:** this sees only what
/// reached the ring. Frames lost to an xrun are never metered, and if the writer
/// thread pads them with silence (as [`FrameCursor`] says it should) the meter
/// sees silence where the performance was. So a clip latch that is clear is only
/// as trustworthy as [`CaptureStats::frames_dropped`] is zero, and the UI must
/// show them together.
#[derive(Debug, Clone)]
pub struct LevelTracker {
    channels: usize,
    sample_rate: f64,
    window_peak: Vec<f32>,
    window_sumsq: Vec<f64>,
    window_frames: u64,
    hold: Vec<f32>,
    take_peak: Vec<f32>,
    clipped: Vec<bool>,
    clipped_samples: u64,
    frames: u64,
    /// Frames still to go before the clip latch arms. See [`CLIP_WARMUP_MS`].
    warmup: u64,
}

impl LevelTracker {
    pub fn new(channels: usize, sample_rate: f64) -> Self {
        Self {
            channels,
            sample_rate,
            window_peak: vec![0.0; channels],
            window_sumsq: vec![0.0; channels],
            window_frames: 0,
            hold: vec![0.0; channels],
            take_peak: vec![0.0; channels],
            clipped: vec![false; channels],
            clipped_samples: 0,
            frames: 0,
            // Armed. The warm-up is asked for at the site that knows a STREAM
            // just opened — see `warm_up` — because that is the only thing it
            // is about, and a tracker fed from a buffer in a test has not just
            // opened anything.
            warmup: 0,
        }
    }

    /// Hold the clip latch off for the first `ms` of audio.
    ///
    /// Called when a capture stream OPENS, which is the only moment it makes
    /// sense: see [`CLIP_WARMUP_MS`] for the converter glitch it exists to
    /// swallow. Peaks, RMS and the take peak are unaffected throughout — the
    /// meter still moves, it just will not latch.
    pub fn warm_up(&mut self, ms: f64) {
        self.warmup = (self.sample_rate * ms / 1000.0).max(0.0) as u64;
    }

    /// Clear the latch and the take peak for a new take. The *only* thing that
    /// clears them: publishing must not, or the clip that happened in bar 3 is
    /// gone by bar 4 and the whole point of a latch is lost.
    pub fn arm(&mut self) {
        self.take_peak.iter_mut().for_each(|p| *p = 0.0);
        self.clear_clip();
        self.frames = 0;
    }

    /// Put out the clip latch, and nothing else.
    ///
    /// **Not `arm`.** Arming also forgets `take_peak` and the frame count,
    /// which are the take's history — a user acknowledging a red light must
    /// not also erase the evidence that the take was silent.
    ///
    /// The warm-up is NOT re-armed either. It exists to cover a converter
    /// starting up, which has already happened by the time anybody clicks; a
    /// dismiss that also went blind for 50 ms would hide a clip still going on.
    pub fn clear_clip(&mut self) {
        self.clipped.iter_mut().for_each(|c| *c = false);
        self.clipped_samples = 0;
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Absorb interleaved frames. A trailing partial frame is ignored rather
    /// than folded into channel 0, which would put the right channel's peak on
    /// the left meter every time a drain landed mid-frame.
    pub fn absorb(&mut self, interleaved: &[f32]) {
        let ch = self.channels;
        let frames = interleaved.len() / ch;
        for frame in interleaved[..frames * ch].chunks_exact(ch) {
            for (c, sample) in frame.iter().enumerate() {
                let mag = sample.abs();
                if mag > self.window_peak[c] {
                    self.window_peak[c] = mag;
                }
                if mag > self.take_peak[c] {
                    self.take_peak[c] = mag;
                }
                if mag >= CLIP_LEVEL && self.warmup == 0 {
                    self.clipped[c] = true;
                    self.clipped_samples += 1;
                }
                self.window_sumsq[c] += (*sample as f64) * (*sample as f64);
            }
        }
        self.window_frames += frames as u64;
        self.frames += frames as u64;
        self.warmup = self.warmup.saturating_sub(frames as u64);
    }

    /// Copy the current levels into `dst` and start a new window.
    ///
    /// **When no audio arrived since the last publish, `peak` and `rms` are left
    /// alone.** A 60 Hz repaint against 10 ms buffers polls empty roughly a third
    /// of the time, and zeroing on an empty window makes the meter strobe to
    /// silence while a chord is ringing. The latch, the take peak and the frame
    /// count are always copied, because they are cumulative and cannot lie.
    pub fn publish(&mut self, dst: &mut Meters) {
        if dst.channels != self.channels {
            *dst = Meters::new(self.channels);
        }
        if self.window_frames > 0 {
            let dt = self.window_frames as f64 / self.sample_rate;
            let decay = 10f32.powf(-HOLD_DECAY_DB_PER_SEC * dt as f32 / 20.0);
            for c in 0..self.channels {
                dst.peak[c] = self.window_peak[c];
                dst.rms[c] = (self.window_sumsq[c] / self.window_frames as f64).sqrt() as f32;
                self.hold[c] = (self.hold[c] * decay).max(self.window_peak[c]);
                dst.hold[c] = self.hold[c];
                self.window_peak[c] = 0.0;
                self.window_sumsq[c] = 0.0;
            }
            self.window_frames = 0;
        }
        dst.take_peak.copy_from_slice(&self.take_peak);
        dst.clipped.copy_from_slice(&self.clipped);
        dst.clipped_samples = self.clipped_samples;
        dst.frames = self.frames;
    }
}

// ───────────────────────────────────────────────────────────────────────────
// cpal: enumeration and opening a stream
// ───────────────────────────────────────────────────────────────────────────

/// Every input the host can see, in enumeration order.
///
/// Not cached. An interface plugged in while the Recorder band is open must
/// appear without a restart, and cpal has no change notification to hang a cache
/// invalidation on.
pub fn input_devices() -> Result<Vec<InputDeviceInfo>, AudioError> {
    with_host(input_devices_on)
}

fn input_devices_on(host: &cpal::Host) -> Result<Vec<InputDeviceInfo>, AudioError> {
    let devices = host
        .input_devices()
        .map_err(|e| AudioError::Enumeration(e.to_string()))?;

    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok());

    let mut names = Vec::new();
    let mut configs = Vec::new();
    for device in devices {
        names.push(device.name().unwrap_or_else(|_| UNNAMED_DEVICE.to_string()));
        configs.push(device.default_input_config().ok());
    }

    let mut default_taken = false;
    Ok(keys_for_names(names)
        .into_iter()
        .zip(configs)
        .map(|(key, config)| {
            let is_default = !default_taken && Some(&key.name) == default_name.as_ref();
            default_taken |= is_default;
            InputDeviceInfo {
                channels: config.as_ref().map(|c| c.channels()),
                sample_rate: config.as_ref().map(|c| c.sample_rate().0),
                key,
                is_default,
            }
        })
        .collect())
}

/// The rates a device will open at, from the menu of rates worth offering.
///
/// **Filtered against the device rather than listed from a constant**, because
/// a control offering 192 kHz to an interface that stops at 48 is a control
/// that produces a "could not open" every time somebody tries the biggest
/// number. cpal reports ranges, so a rate counts as offered when any range
/// contains it — which is also how [`pick_input_config`] decides, and the two
/// disagreeing is how a rate gets offered and then silently not used.
///
/// Empty when the device cannot be reached at all; the caller shows the
/// device's own default in that case and changes nothing.
pub fn input_rates(selection: &InputSelection) -> Vec<u32> {
    const OFFERED: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
    let Ok(device) = with_host(|host| resolve(host, selection)) else {
        return Vec::new();
    };
    let Ok(ranges) = device.supported_input_configs() else {
        return Vec::new();
    };
    let ranges: Vec<_> = ranges.collect();
    OFFERED
        .into_iter()
        .filter(|r| {
            ranges
                .iter()
                .any(|c| c.min_sample_rate().0 <= *r && *r <= c.max_sample_rate().0)
        })
        .collect()
}

fn resolve(host: &cpal::Host, selection: &InputSelection) -> Result<cpal::Device, AudioError> {
    match selection {
        InputSelection::Default => host.default_input_device().ok_or(AudioError::NoDefaultInput),
        // The caller has already set `PIPEWIRE_NODE`; this just finds the PCM
        // that honours it. Looked up by name rather than taken as the default,
        // because the default may be `default` or `pulse` on any given box.
        InputSelection::PipewireNode(_) => host
            .input_devices()
            .map_err(|e| AudioError::Enumeration(e.to_string()))?
            .find(|d| d.name().is_ok_and(|n| n == PIPEWIRE_PCM))
            .ok_or_else(|| AudioError::NotFound(DeviceKey::named(PIPEWIRE_PCM))),
        InputSelection::Key(key) => {
            let devices = host
                .input_devices()
                .map_err(|e| AudioError::Enumeration(e.to_string()))?;
            let mut seen = 0u32;
            for device in devices {
                let name = device.name().unwrap_or_else(|_| UNNAMED_DEVICE.to_string());
                if name == key.name {
                    if seen == key.occurrence {
                        return Ok(device);
                    }
                    seen += 1;
                }
            }
            Err(AudioError::NotFound(key.clone()))
        }
    }
}

/// A running input stream.
///
/// **This type is `!Send` and that is cpal's doing, not ours.** Every platform's
/// `Stream` carries `NotSendSyncAcrossAllPlatforms`
/// (`cpal-0.16.0/src/platform/mod.rs:755`), so the stream has to be built,
/// held and dropped on one thread — in practice the UI thread. The writer half
/// ([`CaptureSink`]) is `Send` and is what moves to the worker.
///
/// Dropping this stops the stream.
pub struct InputStream {
    stream: cpal::Stream,
    config: OpenConfig,
    stats: Arc<CaptureStats>,
    fault: Arc<Mutex<Option<String>>>,
}

impl fmt::Debug for InputStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // cpal::Stream is not Debug.
        f.debug_struct("InputStream")
            .field("config", &self.config)
            .field("state", &self.stats.device_state())
            .finish()
    }
}

impl InputStream {
    pub fn config(&self) -> &OpenConfig {
        &self.config
    }

    pub fn stats(&self) -> &Arc<CaptureStats> {
        &self.stats
    }

    pub fn device_state(&self) -> DeviceState {
        self.stats.device_state()
    }

    /// The last error cpal reported, for `take.log`. The [`DeviceState`] is what
    /// the session acts on; this is only ever diagnostics.
    pub fn fault(&self) -> Option<String> {
        self.fault.lock().ok().and_then(|g| g.clone())
    }

    pub fn play(&self) -> Result<(), AudioError> {
        self.stream
            .play()
            .map_err(|e| AudioError::Play(e.to_string()))
    }

    pub fn pause(&self) -> Result<(), AudioError> {
        self.stream
            .pause()
            .map_err(|e| AudioError::Play(e.to_string()))
    }
}

/// Open an input device and start it.
///
/// Per RECORDER-PLAN §3, this is called when the Recorder band *opens*, not when
/// Record is pressed: the level meter has to be live before arming (that is what
/// kills the "I recorded silence" class), and a device opened at Record costs
/// warm-up time inside the take.
pub fn open_input(
    selection: &InputSelection,
    wish: &ConfigWish,
    ring_seconds: f32,
    timebase: Timebase,
    duplex_with: Option<&cpal::Device>,
) -> Result<(InputStream, CaptureSink, Option<Consumer<f32>>), AudioError> {
    // **Under ASIO the input opens on the OUTPUT's own device object.** An
    // ASIO driver has one buffer set, and cpal keys stream state to the
    // `Device` object: `ASIOCreateBuffers` from a second object on the same
    // driver destroys the buffers the running output is playing through
    // (asio-sys documents this on `prepare_output_stream`) — the exact
    // "switched to ASIO, everything looks alive, total silence" the owner
    // reported. Sharing the object takes cpal's real duplex path instead:
    // the input is ADDED to the existing stream set. The stored selection is
    // deliberately ignored here — a single-client driver has no other
    // device to offer.
    let duplex = duplex_with.filter(|_| system().as_deref() == Some("ASIO"));
    // **The binding covers the LOOKUP as well as the open.** cpal's ALSA host
    // opens each device's PCM handles while enumerating, and the stream reuses
    // the handle that is already open — so a variable set after `resolve` would
    // arrive too late to bind anything. See `with_pipewire_node`.
    let node = match selection {
        InputSelection::PipewireNode(n) => n.clone(),
        _ => String::new(),
    };
    let opened = with_pipewire_node(&node, || {
        let owned = match duplex {
            Some(_) => None,
            None => Some(with_host(|host| resolve(host, selection))?),
        };
        let device = duplex.unwrap_or_else(|| owned.as_ref().expect("one of the two is Some"));
        let default_config = device.default_input_config().ok();
        let ranges: Vec<_> = device
            .supported_input_configs()
            .map_err(|e| AudioError::Enumeration(e.to_string()))?
            .collect();
        Ok::<_, AudioError>((owned, default_config, ranges))
    });
    let (owned_device, default_config, ranges) = opened?;
    let device: &cpal::Device =
        duplex.unwrap_or_else(|| owned_device.as_ref().expect("resolved above"));
    let device_name = match selection {
        // The NODE's name, not the PCM's. Every PipeWire input opens through a
        // PCM called `pipewire`, so reporting that would put the same name on
        // every device in the band, the take manifest and the status panel.
        InputSelection::PipewireNode(n) => n.clone(),
        _ => device.name().unwrap_or_else(|_| UNNAMED_DEVICE.to_string()),
    };

    let offered: Vec<SampleFormat> = ranges.iter().map(|r| r.sample_format()).collect();
    let chosen = pick_input_config(ranges, wish, default_config.as_ref()).ok_or_else(|| {
        AudioError::NoUsableConfig {
            device: device_name.clone(),
            offered,
        }
    })?;

    // **On macOS the input runs SMALLER than the output, on purpose.**
    //
    // Two AudioUnits stand between the microphone and the speaker here, so
    // the round trip pays the input buffer, the output buffer, AND whatever
    // sits in the monitor ring between them — a duplex host like Live pays
    // two buffers, which is the whole of the tester's 18.7 ms vs 6.6 ms.
    // The one term this side can shrink is the INPUT: its callback only
    // copies into a ring — no DSP — so it runs at a QUARTER of the chosen
    // buffer (floor 64, clamped to what the device will admit to) without
    // earning dropouts, and the ring's in-flight allowance shrinks with it
    // (`in_block` follows this very value). The un-picked case used to open
    // at `BufferSize::Default` — 512 on Sonoma, and counted as 1024 by the
    // in-flight math — which is why "no choice" was the worst choice.
    //
    // macOS only: WASAPI's `Fixed` is the TOTAL shared-mode buffer (see
    // BUFFER_PERIODS) and ASIO ties both sides to one driver setting, and
    // small ALSA periods overrun — those keep the symmetric mapping below.
    #[cfg(target_os = "macos")]
    let buffer_size = {
        const INPUT_DEFAULT_FRAMES: u32 = 256;
        let quarter = (wish.buffer_frames.unwrap_or(INPUT_DEFAULT_FRAMES) / 4).max(64);
        match chosen.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => {
                BufferSize::Fixed(quarter.clamp(*min, (*max).max(*min)))
            }
            // A device that will not even name a range: the quarter wish,
            // unclamped, beats gambling on a 512-frame Default.
            cpal::SupportedBufferSize::Unknown => BufferSize::Fixed(quarter),
        }
    };
    #[cfg(not(target_os = "macos"))]
    let buffer_size = match wish.buffer_frames {
        Some(frames) => {
            // Under ASIO the request must land inside the driver's range —
            // `Fixed` outside it is a build error, and a failed duplex open
            // falls back in a way that re-grants the driver's ONE buffer set
            // at a different size than the running output was built for,
            // which is the "len is 2 but the index is 2" startup crash. The
            // clamp mirrors `pick_buffer`'s ASIO arm exactly, so the two
            // directions of the same setting always ask the same number.
            let clamped = match (
                system().as_deref() == Some("ASIO"),
                chosen.buffer_size(),
            ) {
                (true, cpal::SupportedBufferSize::Range { min, max }) if *min <= *max => {
                    frames.clamp(*min, *max)
                }
                _ => frames,
            };
            BufferSize::Fixed(clamped)
        }
        // **"Device" is not a size, and letting the backend choose one made it
        // the WORST setting rather than the safest.**
        //
        // The output side has never deferred here: with no stated choice
        // `pick_buffer` still asks for `WANT_BUFFER_FRAMES`. Only the input
        // fell through to `BufferSize::Default`, so choosing "Device" and
        // choosing 256 by hand produced an IDENTICAL output request and two
        // different input ones — and on Linux the difference was audible.
        // Reported from the room: default crackles, an explicit 256 is clean,
        // and every size down to 128 is clean. The two settings a user expects
        // to be the same thing were not.
        //
        // `LINUX-4.16-FINDINGS` §3 already names this shape — "the un-picked
        // case used to open at `BufferSize::Default` … which is why 'no choice'
        // was the worst choice" — and it was only ever fixed on the macOS arm
        // above, which carries its own `INPUT_DEFAULT_FRAMES` for exactly this
        // reason. This is the same fix on the other side of the cfg.
        //
        // **ASIO keeps deferring, and that is not an oversight.** `pick_buffer`'s
        // ASIO arm answers `None` with `Default` so that both directions land on
        // the driver's own preferred size; a driver has ONE buffer set, and an
        // input asking for a number while the output asks for the default
        // re-grants it underneath the running stream — the "len is 2 but the
        // index is 2" crash. The equality is the safety, so it stays.
        None if system().as_deref() == Some("ASIO") => BufferSize::Default,
        None => {
            // The same number the output asks for with no choice made, so the
            // two directions of "Device" mean one thing. Kept here rather than
            // imported because `WANT_BUFFER_FRAMES` lives in the binary crate
            // and this is a library — the macOS arm above duplicates it for the
            // same reason, and the pair must move together.
            const INPUT_DEFAULT_FRAMES: u32 = 256;
            match chosen.buffer_size() {
                cpal::SupportedBufferSize::Range { min, max } if *min <= *max => {
                    BufferSize::Fixed(INPUT_DEFAULT_FRAMES.clamp(*min, *max))
                }
                // A device that will not name a range gets the wish unclamped,
                // which is still a considered number rather than none at all.
                _ => BufferSize::Fixed(INPUT_DEFAULT_FRAMES),
            }
        }
    };
    // **The ring must cover the graph's cycle, or the capture silently
    // halves.** See [`ring_covering_graph`] for the measurement. Behind the
    // PCM-route guard because a JACK host owns its own buffers and there is
    // nothing to cover; behind `graph_clock` because a machine with no
    // PipeWire has no cycle to be covered from and keeps the asked-for
    // number exactly. Clamped back into the device's own range last — a ring
    // the device cannot allocate is a failed open, not protection.
    #[cfg(target_os = "linux")]
    let buffer_size = match buffer_size {
        BufferSize::Fixed(v) if pipewire_pcm_route_applies() => {
            // CLAIM first, then size the ring to what is actually in force:
            // on a graph that would not serve `v`, the claim takes the
            // clock (the owner's full-control ruling — see the claim block
            // above `claim_graph_clock`), so the floor the ring covers is
            // usually `v` itself and the person gets the latency they asked
            // for. Where the claim cannot land, the ring guard still keeps
            // every frame at the graph's own pace.
            let ring = claim_graph_clock(v).map_or(v, |g| ring_covering_graph(v, g.floor()));
            BufferSize::Fixed(match chosen.buffer_size() {
                cpal::SupportedBufferSize::Range { min, max } if *min <= *max => {
                    ring.clamp(*min, *max)
                }
                _ => ring,
            })
        }
        other => other,
    };
    let stream_config = StreamConfig {
        channels: chosen.channels(),
        sample_rate: chosen.sample_rate(),
        buffer_size,
    };

    let channels = chosen.channels() as usize;
    let rate = chosen.sample_rate().0;
    let ring_frames = (ring_seconds.max(0.25) * rate as f32) as usize;
    // One mark per callback. Sized from the ring's depth at a pessimistically
    // small buffer, so the mark ring never runs out before the audio ring does —
    // if it did, whole buffers would be dropped (rule 3 of `accept`) while there
    // was still room for their samples.
    let mark_slots = (ring_frames / 64).clamp(64, 8192);

    let stats = Arc::new(CaptureStats::new());
    // What was actually asked of the host, recorded once. `Default` asked for
    // no size at all, and that is a different claim from any number — see
    // `CaptureStats::ring_asked`.
    stats.set_ring_asked(match buffer_size {
        BufferSize::Fixed(n) => n,
        BufferSize::Default => 0,
    });
    // **A shallow monitor ring: 120 ms.** Deep enough to ride a scheduling
    // hiccup on either side, shallow enough that what you hear is what is
    // happening. Latency is the two buffers, not this — the ring runs nearly
    // empty in the steady state because the renderer drains it every block.
    let monitor_frames = (rate as usize / 8).max(1024);
    let ((source, sink), monitor) = capture_channel_monitored(
        channels,
        wish.picks,
        ring_frames,
        mark_slots,
        Arc::clone(&stats),
        monitor_frames,
    );
    let fault = Arc::new(Mutex::new(None));

    let stream = build_typed_stream(
        device,
        &stream_config,
        chosen.sample_format(),
        source,
        timebase,
        Arc::clone(&stats),
        Arc::clone(&fault),
    )?;

    let input = InputStream {
        stream,
        config: OpenConfig {
            device: device_name,
            // **What comes out, not what the device offers.** One channel
            // picked out of eighteen is a mono stream, and this number is what
            // the WAV declares and what the meter divides by — so reporting the
            // device's count here would write an 18-channel header over mono
            // samples and read every take as one-eighteenth of a second long.
            channels: sink.channels() as u16,
            sample_rate: rate,
            sample_format: chosen.sample_format(),
            buffer_size,
        },
        stats,
        fault,
    };
    input.play()?;
    Ok((input, sink, monitor))
}

fn build_typed_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    format: SampleFormat,
    source: CaptureSource,
    timebase: Timebase,
    stats: Arc<CaptureStats>,
    fault: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, AudioError> {
    macro_rules! open {
        ($t:ty) => {
            build_for::<$t>(device, config, source, timebase, stats, fault)
        };
    }
    match format {
        SampleFormat::F32 => open!(f32),
        SampleFormat::I32 => open!(i32),
        SampleFormat::I24 => open!(cpal::I24),
        SampleFormat::I16 => open!(i16),
        SampleFormat::U16 => open!(u16),
        SampleFormat::I8 => open!(i8),
        SampleFormat::U8 => open!(u8),
        other => Err(AudioError::NoUsableConfig {
            device: device.name().unwrap_or_else(|_| UNNAMED_DEVICE.to_string()),
            offered: vec![other],
        }),
    }
}

/// The entire cpal-facing body of the capture path.
///
/// Kept to a dozen lines on purpose: everything it does that could be wrong is
/// in [`CaptureSource::accept`] and [`stream_instant_nanos`], both of which are
/// tested without a device.
///
/// `capture` rather than `callback` is the timestamp used, because cpal
/// documents it as "the instant that data was captured from the device"
/// (`src/lib.rs:347-350`) and that is what pairs with this buffer's first frame.
/// Be aware of what it is on macOS in 0.16: `callback - buffer_frames/rate`,
/// derived rather than measured. That derivation is a *constant* for a constant
/// buffer size, so it moves the fit's intercept and leaves its slope — the
/// drift correction — untouched.
fn build_for<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut source: CaptureSource,
    timebase: Timebase,
    stats: Arc<CaptureStats>,
    fault: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, AudioError>
where
    T: cpal::SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    device
        .build_input_stream::<T, _, _>(
            config,
            move |data: &[T], info: &cpal::InputCallbackInfo| {
                // Denormals flushed here too — the monitor racks run on
                // this thread's floats. See the output callback's twin.
                flush_denormals();
                let host_ns = timebase.now();
                let device_ns = stream_instant_nanos(info.timestamp().capture);
                source.accept(data, device_ns, host_ns);
            },
            move |err| {
                // Never panic here: this can run on the audio thread, and on
                // macOS it fires for a bus-powered interface browning out — a
                // routine event, not a bug. The state is what the session reads;
                // the message is a courtesy, so a contended lock is skipped
                // rather than waited on.
                stats.note_error(&err);
                if let Ok(mut slot) = fault.try_lock() {
                    *slot = Some(err.to_string());
                }
            },
            None,
        )
        .map_err(|e| AudioError::Build(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpal::{StreamInstant, SupportedBufferSize};

    const RATE: f64 = 48_000.0;

    /// **The host is built once per system choice, not once per call.**
    ///
    /// This is the ASIO teardown fix in miniature: every fresh `cpal::Host`
    /// under ASIO is a fresh driver-guard, and a fresh guard is a license to
    /// ask the SDK's process-global loader to unload the driver a running
    /// stream is playing through. The cache is what makes that impossible, so
    /// the cache is what this test pins. Thread-local state, and the whole
    /// test on one thread, so parallel tests cannot race the counter.
    #[test]
    fn the_host_is_cached_until_the_system_choice_changes() {
        // Serialize against any other test that touches SYSTEM.
        let baseline_system = system();
        with_host(|_| ()); // whatever key is current: cached from here on
        let start = HOSTS_BUILT.load(std::sync::atomic::Ordering::Relaxed);
        with_host(|_| ());
        with_host(|_| ());
        assert_eq!(
            HOSTS_BUILT.load(std::sync::atomic::Ordering::Relaxed),
            start,
            "a second and third with_host on the same system rebuilt the host \
             - under ASIO that rebuild is the driver-teardown bug"
        );
        // A DIFFERENT system name (even one this build does not have - it
        // falls back to the default host) is a new key: built once, then
        // cached again.
        set_system(Some("not-a-real-system-name"));
        with_host(|_| ());
        let after_switch = HOSTS_BUILT.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(after_switch, start + 1, "the new key should build exactly once");
        with_host(|_| ());
        assert_eq!(
            HOSTS_BUILT.load(std::sync::atomic::Ordering::Relaxed),
            after_switch,
            "the new key should then be cached"
        );
        // Put the process-global choice back for whoever runs next.
        set_system(baseline_system.as_deref());
        with_host(|_| ());
    }

    /// The merge keeps whichever layer could see, and stays silent only when
    /// neither could.
    ///
    /// The pairs below are the two MEASURED cases with the sources reversed —
    /// Scarlett (graph 48, client 0) and built-in (graph 0, client 246) — and
    /// the merge must report the seeing layer in both, because each zero here
    /// was, on the day it was measured, a lie from a blind counter.
    #[test]
    fn a_blind_counters_zero_never_masks_the_seeing_ones_count() {
        assert_eq!(merge_dropouts(Some(48), 0), Some(48), "scarlett path");
        assert_eq!(merge_dropouts(Some(0), 246), Some(246), "built-in path");
        assert_eq!(merge_dropouts(Some(3), 5), Some(5), "both saw: best lower bound");
        assert_eq!(merge_dropouts(None, 7), Some(7), "client alone still counts");
        // No graph AND a client zero is "nobody could see" — the client has
        // been proven blind on one shipping path, so its zero alone clears
        // nothing.
        assert_eq!(merge_dropouts(None, 0), None);
    }

    /// **A capture ring smaller than the graph's cycle does not fail — it
    /// keeps `ring/quantum` of the audio and reports nothing.** Measured with
    /// **The claim takes the clock exactly when the person's ask would not
    /// be served, and always remembers what it displaced.** The owner's
    /// full-control ruling, as arithmetic: a graph whose floor is coarser
    /// than the ask gets forced to the ask; a graph already fine enough is
    /// left alone; somebody else's pin is overridden but carried in
    /// `prior` so release puts it back; and a claim we already hold moves
    /// or releases as the new ask requires.
    #[test]
    fn the_claim_forces_exactly_when_the_ask_would_not_be_served() {
        let g = |quantum, min, forced| GraphClock { quantum, min_quantum: min, forced, rate: 48_000 };

        // An ordinary desktop (floor 32): nothing to take.
        assert_eq!(claim_action(g(1024, 32, 0), 256, 0, 0), ClaimAction::Keep);
        // The wound machine: min-quantum pinned 512, ask 256 — taken.
        assert_eq!(
            claim_action(g(512, 512, 0), 256, 0, 0),
            ClaimAction::Force { frames: 256, prior: 0 }
        );
        // Somebody else's force pin: overridden, and remembered for release.
        assert_eq!(
            claim_action(g(512, 32, 512), 256, 0, 0),
            ClaimAction::Force { frames: 256, prior: 512 }
        );
        // Already holding the ask, and the graph still shows it: nothing to do.
        assert_eq!(claim_action(g(256, 512, 256), 256, 256, 0), ClaimAction::Keep);
        // Holding 256, the ask coarsens to 1024, and the machine's own floor
        // (512, once our force is gone) serves it: let the clock go.
        assert_eq!(claim_action(g(256, 512, 256), 1024, 256, 0), ClaimAction::Release);
        // Holding 256 over somebody's 512 pin, ask moves to 128: the pin
        // still would not serve it, so the claim moves and the PRIOR rides
        // along — never our own last claim.
        assert_eq!(
            claim_action(g(256, 32, 256), 128, 256, 512),
            ClaimAction::Force { frames: 128, prior: 512 }
        );
        // A third hand re-pinned the clock out from under our claim: the
        // held-branch does not apply (forced != held), and the stranger's
        // pin is treated like any other — overridden, remembered.
        assert_eq!(
            claim_action(g(2048, 32, 2048), 256, 256, 0),
            ClaimAction::Force { frames: 256, prior: 2048 }
        );
    }

    /// `takeprobe`: at a 512-frame floor a `Fixed(256)` stream ran at a steady
    /// 22,016 frames per second with zero drops on every counter, and three
    /// real takes came back with their audio at double speed. The rule under
    /// test is the protection: the ring always covers the widest cycle the
    /// graph can serve, and never touches a request that already does.
    #[test]
    fn the_ring_always_covers_the_cycle_the_graph_can_serve() {
        // An ordinary desktop floor: the asked-for number stands untouched.
        assert_eq!(ring_covering_graph(256, 32), 256);
        // The machine this was measured on: floor 512, asked 256 — the ring
        // that halved every take. Twice the floor, so a cycle at the floor
        // and a cycle at our own period both fit with room to spare.
        assert_eq!(ring_covering_graph(256, 512), 1024);
        // A graph pinned coarser than anything the app would ask for.
        assert_eq!(ring_covering_graph(256, 2048), 4096);
        // A generous ask needs no protection and keeps its number.
        assert_eq!(ring_covering_graph(4096, 512), 4096);
        // A pathological floor saturates rather than wrapping to a sliver.
        assert_eq!(ring_covering_graph(256, u32::MAX), u32::MAX);
    }

    /// The ERR column survives every row shape `pw-top -b` actually prints.
    ///
    /// The rows below are captured from the machine this was built for, not
    /// composed: a driver row with a format, a follower row whose name is
    /// pushed right by a `+`, an idle row that prints only dashes, and a JACK
    /// client row with no format at all. The parser reads the ninth token
    /// wherever the second parses as an id, and the test exists because the
    /// first version of the take-audit's WAV reader ALSO "obviously" worked
    /// on the format it assumed and read the middle of a timestamp.
    #[test]
    fn the_err_column_is_found_in_every_row_shape_pw_top_prints() {
        let captured = "\
S   ID  QUANT   RATE    WAIT    BUSY   W/Q   B/Q  ERR FORMAT           NAME \n\
I   31      0      0   0.0us   0.0us  ???   ???     0                  Dummy-Driver\n\
R   59    128  44100  92.3us   1.2us  0.03  0.00  14646    S32LE 2 44100 alsa_input.usb-Focusrite\n\
R   58      0      0  12.2us  15.2us  0.00  0.01   13    S32LE 2 44100  + alsa_output.usb-Focusrite\n\
R  101      0      0  16.9us 470.6us  0.00  0.08    7                   + Tangent_out\n\
C   77      0      0    ---     ---   ---   ---     0                  alsa_playback.tangent\n";
        // Ids present sum their LAST value; absent ids measure nothing.
        assert_eq!(sum_errs(captured, &[58, 101]), Some(20));
        assert_eq!(sum_errs(captured, &[59]), Some(14646));
        // An idle row prints dashes for its timings but ERR still prints, and
        // a real 0 from an idle node is a genuine count — cumulative, per
        // node, zero because nothing has gone wrong yet. Only absence is
        // unknown.
        assert_eq!(sum_errs(captured, &[77]), Some(0));
        // A snapshot that misses every asked-for node is UNKNOWN, not zero —
        // the whole module exists because a confident zero was believed once.
        assert_eq!(sum_errs(captured, &[9999]), None);
        // Two snapshots in one batch: the later reading wins.
        let two = format!("{captured}R   58      0      0  12.2us  15.2us  0.00  0.01   21    S32LE 2 44100  + alsa_output.usb-Focusrite\n");
        assert_eq!(sum_errs(&two, &[58]), Some(21));
    }

    /// Node ownership is resolved through the client, and directly, and only
    /// for audio.
    #[cfg(target_os = "linux")]
    #[test]
    fn our_nodes_are_found_by_pid_through_either_route() {
        let dump = r#"[
          { "id": 40, "type": "PipeWire:Interface:Client",
            "info": { "props": { "application.process.id": 4242 } } },
          { "id": 58, "type": "PipeWire:Interface:Node",
            "info": { "props": { "media.class": "Stream/Output/Audio", "client.id": 40 } } },
          { "id": 59, "type": "PipeWire:Interface:Node",
            "info": { "props": { "media.class": "Stream/Input/Audio", "application.process.id": "4242" } } },
          { "id": 64, "type": "PipeWire:Interface:Node",
            "info": { "props": { "media.class": "Video/Source", "client.id": 40 } } },
          { "id": 75, "type": "PipeWire:Interface:Node",
            "info": { "props": { "media.class": "Audio/Sink", "client.id": 99 } } }
        ]"#;
        let ids = our_audio_nodes(dump, 4242);
        // 58 via its client, 59 via its own pid (as a STRING, which pipewire
        // has emitted across versions), 64 refused for being video, 75 refused
        // for belonging to somebody else.
        assert_eq!(ids, vec![58, 59]);
        // A pid that owns nothing measures nothing.
        assert!(our_audio_nodes(dump, 1).is_empty());
        // Garbage in, empty out — the caller turns empty into None.
        assert!(our_audio_nodes("not json", 4242).is_empty());
    }

    /// **The feature flag, asserted rather than assumed.**
    ///
    /// `cpal`'s `jack` feature is turned on in `tangent-record/Cargo.toml` per
    /// target, which is a place a change can silently drop it — a merge, a
    /// dependency bump, a tidy-up of a table that looks redundant next to the
    /// unconditional `cpal = "0.16"` three lines up. Nothing would fail to
    /// build; the Audio Setup panel would just quietly go back to offering one
    /// entry, and the only symptom is a user on a pro-audio session wondering
    /// where JACK went. This is deterministic without a server running:
    /// `JackHost::is_available` is a hardcoded `true`, so the name is listed
    /// whenever the backend is COMPILED, which is exactly what this asserts.
    #[test]
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn the_jack_host_is_compiled_in() {
        assert!(
            systems().iter().any(|s| s == "JACK"),
            "this build should offer JACK; it offers {:?}",
            systems()
        );
    }

    /// The guard rejects only JACK, and never the host it would fall back to.
    ///
    /// Guards the ASIO property in [`host_has_a_device`]'s own note: the probe
    /// must never be the thing that walks a host's device list, because on
    /// Windows that is what tears down a live driver. A non-JACK host is passed
    /// through on the strength of its NAME, before anything is enumerated, and
    /// this holds on every platform including the one where the default host
    /// genuinely has no devices.
    #[test]
    fn the_empty_host_guard_only_asks_jack() {
        let default = cpal::default_host();
        assert_ne!(default.id().name(), "JACK", "no platform defaults to JACK");
        assert!(
            host_has_a_device(&default),
            "the default host must pass the guard unenumerated, or build_host \
             could reject the very host it falls back to"
        );
    }

    fn stats() -> Arc<CaptureStats> {
        Arc::new(CaptureStats::new())
    }

    /// A callback's worth of interleaved frames whose value encodes the channel,
    /// so a swapped lane is visible rather than merely suspected.
    fn frames(channels: usize, count: usize, first: usize) -> Vec<f32> {
        (0..count * channels)
            .map(|i| {
                let frame = first + i / channels;
                let ch = i % channels;
                frame as f32 + ch as f32 / 10.0
            })
            .collect()
    }

    fn range(
        channels: u16,
        min: u32,
        max: u32,
        format: SampleFormat,
    ) -> SupportedStreamConfigRange {
        SupportedStreamConfigRange::new(
            channels,
            SampleRate(min),
            SampleRate(max),
            SupportedBufferSize::Range { min: 64, max: 4096 },
            format,
        )
    }

    // ── reading cpal's opaque instant ───────────────────────────────────────

    #[test]
    fn a_negative_stream_instant_survives_the_only_accessor_cpal_gives_us() {
        // ALSA's `capture` is `callback - buffer_delay` measured from the
        // stream's own trigger, so the first callbacks of a stream are before
        // the origin. `duration_since` returns None there, and a naive reader
        // would silently discard exactly the marks the anchor needs most.
        assert_eq!(stream_instant_nanos(StreamInstant::new(3, 500_000_000)), Some(3_500_000_000));
        assert_eq!(stream_instant_nanos(StreamInstant::new(0, 0)), Some(0));
        assert_eq!(
            stream_instant_nanos(StreamInstant::new(-1, 500_000_000)),
            Some(-500_000_000),
            "cpal represents -0.5 s as secs=-1 plus 5e8 subsec nanos"
        );
    }

    // ── device identity ─────────────────────────────────────────────────────

    #[test]
    fn two_identical_interfaces_get_different_keys() {
        let keys = keys_for_names(["Scarlett 2i2 USB", "MacBook Pro Microphone", "Scarlett 2i2 USB"]);
        assert_eq!(keys[0], DeviceKey::new("Scarlett 2i2 USB", 0));
        assert_eq!(keys[2], DeviceKey::new("Scarlett 2i2 USB", 1));
        assert_eq!(
            keys[1],
            DeviceKey::new("MacBook Pro Microphone", 0),
            "an unrelated device between two duplicates must not consume an occurrence"
        );
    }

    #[test]
    fn a_device_name_containing_a_hash_still_round_trips() {
        // "Microphone #2" is a real Windows device name. Splitting from the left
        // would store it as name="Microphone " occurrence=2 and then never find
        // the device again.
        for key in [
            DeviceKey::named("Microphone #2"),
            DeviceKey::new("Microphone #2", 1),
            DeviceKey::named("Scarlett 2i2 USB"),
            DeviceKey::new("Scarlett 2i2 USB", 3),
            DeviceKey::named("weird#"),
        ] {
            assert_eq!(
                DeviceKey::from_setting(&key.to_setting()),
                key,
                "{key:?} did not survive the settings round trip via {:?}",
                key.to_setting()
            );
        }
    }

    // ── configuration choice ────────────────────────────────────────────────

    #[test]
    fn a_sample_format_this_module_cannot_read_is_never_chosen() {
        // Choosing F64 would build a stream whose callback immediately hits
        // cpal's own `expect("host supplied incorrect sample type")` — a panic
        // on the audio thread, on a machine we do not own.
        let chosen = pick_input_config(
            [
                range(2, 48_000, 48_000, SampleFormat::F64),
                range(2, 48_000, 48_000, SampleFormat::I16),
            ],
            &ConfigWish {
                sample_rate: Some(48_000),
                ..ConfigWish::default()
            },
            None,
        )
        .expect("the I16 range is readable");
        assert_eq!(chosen.sample_format(), SampleFormat::I16);
    }

    #[test]
    fn nothing_readable_means_no_config_rather_than_a_bad_one() {
        assert!(pick_input_config(
            [range(2, 48_000, 48_000, SampleFormat::F64)],
            &ConfigWish::default(),
            None,
        )
        .is_none());
    }

    #[test]
    fn reaching_the_requested_rate_outranks_a_nicer_sample_format() {
        // A device offering f32 only at 96k and i16 at 48k, asked for 48k. Taking
        // the f32 range would silently record at 96k and the WAV would declare a
        // rate the user did not choose.
        let chosen = pick_input_config(
            [
                range(2, 96_000, 96_000, SampleFormat::F32),
                range(2, 44_100, 48_000, SampleFormat::I16),
            ],
            &ConfigWish {
                sample_rate: Some(48_000),
                ..ConfigWish::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(chosen.sample_rate(), SampleRate(48_000));
        assert_eq!(chosen.sample_format(), SampleFormat::I16);
    }

    #[test]
    fn f32_wins_over_i16_when_both_reach_the_rate() {
        let chosen = pick_input_config(
            [
                range(2, 44_100, 48_000, SampleFormat::I16),
                range(2, 44_100, 48_000, SampleFormat::F32),
            ],
            &ConfigWish {
                sample_rate: Some(48_000),
                ..ConfigWish::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(chosen.sample_format(), SampleFormat::F32);
    }

    #[test]
    fn an_eighteen_input_interface_does_not_hand_back_an_eighteen_channel_take() {
        // No channel wish and no device default: the tie-break must not be "most
        // channels", or a Scarlett 18i20 produces a 3.5 MB/s WAV nobody asked for.
        let chosen = pick_input_config(
            [
                range(18, 48_000, 48_000, SampleFormat::F32),
                range(2, 48_000, 48_000, SampleFormat::F32),
            ],
            &ConfigWish {
                sample_rate: Some(48_000),
                ..ConfigWish::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(chosen.channels(), 2);
    }

    // ── the ring ────────────────────────────────────────────────────────────

    #[test]
    fn a_full_ring_never_leaves_half_a_frame_behind() {
        // The ring holds 10 frames; the callback delivers 16. If the overflow
        // guard worked in samples rather than frames, the 21st sample (frame 10,
        // left) would be pushed without its right, and every frame after it in
        // the take would have its channels swapped.
        let (mut src, mut sink) = capture_channel(2, 10, 64, stats());
        src.accept(&frames(2, 16, 0), Some(0), 0);

        let mut out = Vec::new();
        let got = sink.drain_samples(&mut out);
        assert_eq!(out.len() % 2, 0, "an odd sample count is a split frame");
        for (frame, pair) in out.chunks_exact(2).enumerate() {
            assert_eq!(pair[0], frame as f32, "left lane at frame {frame}");
            assert_eq!(pair[1], frame as f32 + 0.1, "right lane at frame {frame}");
        }
        assert_eq!(got, out.len() / 2);
        assert_eq!(got, 10, "the ring's whole depth, and not one sample more");
    }

    #[test]
    fn a_full_monitor_ring_never_commits_half_a_frame() {
        // The monitor-side twin of the test above, for the push that once
        // truncated in SAMPLES. The monitor ring's capacity is a frame
        // multiple, but its free space stops being one the moment the consumer
        // pops a partial frame — which the renderer's fixed-size scratch can
        // legitimately do. After that, a push clamped to `slots()` commits a
        // split frame and every monitor frame that follows plays the wrong
        // lanes until the device is reopened.
        let ((mut src, _sink), monitor) =
            capture_channel_monitored(2, Picks::EVERYTHING, 64, 64, stats(), 8);
        let mut mon = monitor.expect("monitor_frames > 0 means a tap");

        // Six of the tap's eight frames arrive...
        src.accept(&frames(2, 6, 0), Some(0), 0);
        // ...and the consumer missteps: three samples is a frame and a half,
        // leaving seven free slots — not a whole number of stereo frames.
        let mut seen = Vec::new();
        for _ in 0..3 {
            seen.push(mon.pop().unwrap());
        }
        // The next callback wants to push six frames into that room.
        src.accept(&frames(2, 6, 6), Some(0), 0);

        while let Ok(s) = mon.pop() {
            seen.push(s);
        }
        assert_eq!(
            seen.len() % 2,
            0,
            "the producer committed a split frame into the monitor ring"
        );
        // And the lanes must still be the lanes: everything the producer ever
        // committed, in order, has to chunk back into (left, right) frames.
        for (frame, pair) in seen.chunks_exact(2).enumerate() {
            assert_eq!(
                pair[1],
                pair[0] + 0.1,
                "monitor frame {frame} plays rotated lanes"
            );
        }
    }

    /// **One input of eight, and the right one.**
    ///
    /// The test that has to exist for `pick_channel`, because every way of
    /// getting it wrong is SILENT: a stride computed from the output count
    /// instead of the device's reads a walking mixture of the lanes, an
    /// off-by-one reads the neighbouring input, and both produce a take full of
    /// plausible audio that is not what was plugged in. `frames` encodes the
    /// lane in the tenths, so a wrong lane is visible rather than suspected.
    #[test]
    fn a_picked_channel_is_the_one_that_reaches_the_ring() {
        for ch in 0..8u16 {
            let (mut src, mut sink) = capture_channel_from(8, Picks::one(ChannelPick::Mono(ch)), 64, 64, stats());
            assert_eq!(sink.channels(), 1, "a picked channel is a mono stream");
            src.accept(&frames(8, 16, 0), Some(0), 0);

            let mut out = Vec::new();
            let got = sink.drain_samples(&mut out);
            // Frames, not samples. Sixteen either way here, which is the point:
            // reading the device's 128 samples as 8-channel frames would give
            // 16 and as 1-channel frames would give 128.
            assert_eq!(got, 16, "channel {ch} delivered {got} frames");
            assert_eq!(out.len(), 16, "one sample per frame, not one per lane");
            for (frame, sample) in out.iter().enumerate() {
                assert_eq!(
                    *sample,
                    frame as f32 + f32::from(ch) / 10.0,
                    "frame {frame} of channel {ch} came from another lane"
                );
            }
        }
    }

    /// **Several inputs of one interface, laid end to end in one stream.**
    ///
    /// The owner's case exactly: a microphone on input 6 and a synth across
    /// 4/5, live at the same time. One callback, one ring and one set of marks,
    /// which is what makes them sample-aligned with each other for nothing —
    /// and the reason a second DEVICE is declined rather than deferred.
    ///
    /// Every way of getting the layout wrong is silent, so the lane is encoded
    /// in the tenths: a pick's channels landing at the wrong offset reads a
    /// neighbouring input, and the take sounds plausible and is not what was
    /// plugged in.
    #[test]
    fn several_picks_arrive_side_by_side_in_pick_order() {
        use ChannelPick::{Mono, Stereo};
        // Mono 6 first, then the pair 4/5 — the order they were chosen in, not
        // the order the device has them in.
        let picks = Picks::from_slice(&[Mono(5), Stereo(3, 4)]);
        assert_eq!(picks.len(), 2, "two inputs");
        assert_eq!(picks.channels(), 3, "one channel plus two");
        assert_eq!(picks.offset(0), 0);
        assert_eq!(picks.offset(1), 1, "the pair starts after the mono one");

        let (mut src, mut sink) = capture_channel_from(8, picks, 64, 64, stats());
        assert_eq!(sink.channels(), 3, "the stream is as wide as the picks are");
        src.accept(&frames(8, 16, 0), Some(0), 0);

        let mut out = Vec::new();
        let got = sink.drain_samples(&mut out);
        assert_eq!(got, 16, "sixteen frames of three channels");
        assert_eq!(out.len(), 48);
        for frame in 0..16usize {
            let at = frame * 3;
            let want = |lane: u16| frame as f32 + f32::from(lane) / 10.0;
            assert_eq!(out[at], want(5), "frame {frame}: the mono input moved");
            assert_eq!(out[at + 1], want(3), "frame {frame}: the pair's left moved");
            assert_eq!(out[at + 2], want(4), "frame {frame}: the pair's right moved");
        }
    }

    /// **Choosing a different input on an OPEN stream changes what the take
    /// keeps, and changes nothing else.**
    ///
    /// This is the whole point of the device-wide rings. Picking an input used
    /// to close the device and open it again — tens of milliseconds of
    /// silence, on the gesture people make most while building a desk. Now the
    /// stream, the buffers and the callback are untouched and only the reading
    /// of the ring moves.
    ///
    /// The strong claim here is the second half: frames the callback pushed
    /// BEFORE the change are still whole device frames, so they are read
    /// correctly under the new picks. There is no window in which a frame
    /// means two things.
    #[test]
    fn picking_a_different_input_on_an_open_stream_needs_no_reopen() {
        use ChannelPick::Mono;
        let (mut src, mut sink) = capture_channel_from(4, Picks::one(Mono(0)), 64, 64, stats());
        assert_eq!(sink.channels(), 1);

        // Two frames arrive under the FIRST pick and are left in the ring.
        src.accept(&frames(4, 2, 0), Some(0), 0);
        // The choice changes with the stream still open and running.
        sink.set_picks(Picks::one(Mono(2)));
        assert_eq!(sink.channels(), 1, "still one channel wide");
        // Two more arrive after it.
        src.accept(&frames(4, 2, 2), Some(0), 0);

        let mut out = Vec::new();
        let got = sink.drain_samples(&mut out);
        assert_eq!(got, 4, "every frame survived the change");
        // EVERY frame reads as channel 2 — including the two that were
        // already sitting in the ring when the pick changed. They were stored
        // whole, so the new choice applies to them as cleanly as to the rest.
        for (f, v) in out.iter().enumerate() {
            let want = f as f32 + 2.0 / 10.0;
            assert_eq!(*v, want, "frame {f} did not come from channel 2");
        }
    }

    /// **A pick change is refused where it would corrupt a file.**
    ///
    /// Widths are the WAV header. This is the sink half of the rule; the
    /// writer thread and `Session` hold the other two.
    #[test]
    fn a_pick_change_moves_the_width_the_take_would_be_written_at() {
        use ChannelPick::{Mono, Stereo};
        let (_src, mut sink) = capture_channel_from(8, Picks::one(Mono(0)), 64, 64, stats());
        assert_eq!(sink.channels(), 1);
        assert_eq!(sink.widths::<4>(), [1, 0, 0, 0]);
        sink.set_picks(Picks::from_slice(&[Mono(5), Stereo(3, 4)]));
        assert_eq!(sink.channels(), 3, "one plus two");
        assert_eq!(sink.widths::<4>(), [1, 2, 0, 0], "the strips' shares moved");
        // A pick the device cannot honour is still dropped whole.
        sink.set_picks(Picks::one(Stereo(6, 99)));
        assert_eq!(sink.channels(), 8, "no picks left means the whole device");
    }

    /// **One input going missing costs that input and not the others.**
    ///
    /// An interface can come back from a hub narrower than it was, and a saved
    /// channel 12 that is no longer there must not take the microphone on
    /// channel 1 with it. A pick that half-fits is still dropped whole — half a
    /// stereo pair is "left twice", which says nothing and sounds fine.
    #[test]
    fn a_pick_that_no_longer_fits_is_dropped_alone() {
        use ChannelPick::{Mono, Stereo};
        let picks = Picks::from_slice(&[Mono(0), Stereo(6, 7), Mono(11)]);
        let kept = picks.fitted(2);
        assert_eq!(kept.len(), 1, "only channel 1 exists on a 2-channel device");
        assert_eq!(kept.channels(), 1);
        assert_eq!(kept.iter().next(), Some(Mono(0)));

        // And a device with none of them left falls back to the whole device
        // rather than to silence.
        let (_, sink) = capture_channel_from(2, Picks::one(Mono(9)), 64, 64, stats());
        assert_eq!(
            sink.channels(),
            2,
            "a pick nothing can honour opens the device instead of a dead stream"
        );
    }

    /// **Any pair, adjacent or not, in either order.**
    ///
    /// The one everybody hits is 2/3 — a stereo pair patched across the middle
    /// of an interface because input 1 has the vocal in it. cpal cannot ask for
    /// it (fewer channels means the FIRST ones), so it is taken here, and every
    /// way of getting it wrong is silent: a stride from the wrong channel count
    /// walks the lanes, an off-by-one records the neighbours, and a pair
    /// applied per-sample instead of per-frame swaps left and right on
    /// alternate frames. `frames` encodes the lane in the tenths so a wrong one
    /// is visible rather than suspected.
    #[test]
    fn a_stereo_pair_can_be_any_two_inputs() {
        for (l, r) in [(1_u16, 2_u16), (0, 1), (6, 7), (3, 0), (0, 0), (7, 2)] {
            let (mut src, mut sink) =
                capture_channel_from(8, Picks::one(ChannelPick::Stereo(l, r)), 64, 64, stats());
            assert_eq!(sink.channels(), 2, "a pair is a stereo stream");
            src.accept(&frames(8, 16, 0), Some(0), 0);

            let mut out = Vec::new();
            let got = sink.drain_samples(&mut out);
            assert_eq!(got, 16, "pair {l}/{r} delivered {got} frames");
            assert_eq!(out.len(), 32, "two samples per frame");
            for (frame, pair) in out.chunks_exact(2).enumerate() {
                assert_eq!(
                    pair[0],
                    frame as f32 + f32::from(l) / 10.0,
                    "frame {frame}: left came from the wrong input"
                );
                assert_eq!(
                    pair[1],
                    frame as f32 + f32::from(r) / 10.0,
                    "frame {frame}: right came from the wrong input"
                );
            }
        }
    }

    /// A pair that half-fits is refused whole.
    ///
    /// Honouring the half that exists would record the left channel twice and
    /// say nothing — a stereo file that is secretly mono, which survives a
    /// listen.
    #[test]
    fn a_pair_with_one_channel_missing_falls_back_to_the_device() {
        let (mut src, mut sink) =
            capture_channel_from(2, Picks::one(ChannelPick::Stereo(0, 5)), 64, 64, stats());
        assert_eq!(sink.channels(), 2, "the whole device, not half a pair");
        src.accept(&frames(2, 4, 0), Some(0), 0);
        let mut out = Vec::new();
        assert_eq!(sink.drain_samples(&mut out), 4);
        // The DEVICE's own two channels, untouched — not channel 0 twice.
        assert_eq!(out[0], 0.0);
        assert_eq!(out[1], 0.1);
    }

    /// The frame accounting is the DEVICE's, whatever is kept from it.
    ///
    /// Counting the mono samples as frames would report eight times the audio
    /// the device produced, and every ratio built on it — the drop rate, the
    /// rate fit, the take's length — would be wrong by the channel count.
    #[test]
    fn picking_a_channel_does_not_change_what_the_device_is_said_to_have_sent() {
        let stats = stats();
        let (mut src, _sink) = capture_channel_from(8, Picks::one(ChannelPick::Mono(3)), 64, 64, Arc::clone(&stats));
        src.accept(&frames(8, 16, 0), Some(0), 0);
        assert_eq!(stats.frames_captured(), 16, "the device produced 16 frames");
    }

    /// A channel that is not there costs the channel, not the session.
    ///
    /// An interface can come back from a hub with fewer inputs than it had, and
    /// the saved choice then names one that does not exist. Refusing to open
    /// would leave somebody with no microphone and a message about a number.
    #[test]
    fn a_channel_the_device_does_not_have_falls_back_to_all_of_them() {
        let (mut src, mut sink) = capture_channel_from(2, Picks::one(ChannelPick::Mono(9)), 64, 64, stats());
        assert_eq!(sink.channels(), 2, "the whole device, not one lane of it");
        src.accept(&frames(2, 4, 0), Some(0), 0);
        let mut out = Vec::new();
        assert_eq!(sink.drain_samples(&mut out), 4);
        assert_eq!(out.len(), 8);
    }

    /// Asking for no channel is the path every build before this one took, and
    /// it must be byte-for-byte what it was.
    #[test]
    fn no_pick_is_the_straight_through_path() {
        let (mut src, mut sink) = capture_channel_from(2, Picks::EVERYTHING, 64, 64, stats());
        assert_eq!(sink.channels(), 2);
        src.accept(&frames(2, 8, 0), Some(0), 0);
        let mut out = Vec::new();
        sink.drain_samples(&mut out);
        for (frame, pair) in out.chunks_exact(2).enumerate() {
            assert_eq!(pair[0], frame as f32);
            assert_eq!(pair[1], frame as f32 + 0.1);
        }
    }

    /// **Clearing the published copy is not clearing the latch.**
    ///
    /// The trap behind Tangent 4.19.0's un-resettable clip lamp, written down
    /// at the level it lives at. `Session::clear_clip` cleared the `Meters`
    /// behind the mutex — the copy the UI reads — and the writer thread's
    /// `pump()` published its own still-latched `clipped[]` over the top within
    /// about 4 ms, forever, for as long as the stream stayed open.
    ///
    /// The symptom was precise and looked absurd: the lamp cleared only while
    /// NO input was selected, because unselecting one destroys the thread that
    /// owns the surviving latch. It was filmed at 60 fps and did not go dark
    /// for a single frame across two clicks.
    #[test]
    fn clearing_a_published_copy_is_undone_by_the_next_publish() {
        let mut tracker = LevelTracker::new(1, RATE);
        tracker.absorb(&[1.0]);
        let mut meters = Meters::new(1);
        tracker.publish(&mut meters);
        assert!(meters.any_clipped(), "a full-scale sample did not latch");

        // Everything the dismiss used to do.
        meters.clear_clip();
        assert!(!meters.any_clipped(), "the copy did not clear");
        tracker.publish(&mut meters);
        assert!(
            meters.any_clipped(),
            "**this is the bug**: the source republished over the cleared copy"
        );

        // And what it does now — the source as well. The copy is still cleared
        // first, because that is what makes the lamp go dark on THIS frame
        // rather than at the next poll.
        meters.clear_clip();
        tracker.clear_clip();
        tracker.publish(&mut meters);
        assert!(!meters.any_clipped(), "the dismiss still does not hold");
        // Twice, because "cleared once then re-latched" is the failure and one
        // publish cannot tell it from "cleared for good".
        tracker.publish(&mut meters);
        assert!(!meters.any_clipped(), "it came back one cycle later");

        // The take's history survives the dismiss: acknowledging a red light
        // must not erase the evidence that the take was silent.
        assert!(meters.take_peak[0] >= 1.0, "the dismiss erased the take peak");
    }

    /// **A converter warming up is not a performance clipping.**
    ///
    /// Observed on Linux: moving the system default source under an open
    /// stream, in a way that forced a rate-and-channel converter swap, latched
    /// both clip lamps on a source that was silent. One garbage buffer is
    /// enough — and with the latch un-clearable (the test above) the phantom
    /// was permanent. It is very likely where "it says I clipped" came from for
    /// people who had not.
    #[test]
    fn a_stream_that_has_just_opened_does_not_latch_a_converter_glitch() {
        let mut t = LevelTracker::new(2, RATE);
        t.warm_up(CLIP_WARMUP_MS);
        // A full-scale garbage buffer in the first millisecond.
        let garbage = vec![1.0_f32; 2 * (RATE / 1000.0) as usize];
        t.absorb(&garbage);
        let mut m = Meters::new(2);
        t.publish(&mut m);
        assert!(!m.any_clipped(), "a converter warming up latched a clip");
        // **The meter still MOVED.** Only the latch is held off; a meter that
        // went blind for 50 ms would be a worse lie than the one this fixes.
        assert!(m.peak[0] >= 1.0, "the warm-up blinded the meter as well");

        // Past the window, a real clip latches exactly as before.
        let quiet = vec![0.0_f32; 2 * (RATE * CLIP_WARMUP_MS / 1000.0) as usize];
        t.absorb(&quiet);
        t.absorb(&garbage);
        t.publish(&mut m);
        assert!(m.any_clipped(), "the latch never armed after the warm-up");
    }

    /// A tracker built for a buffer, not a stream, latches from the first
    /// sample — the warm-up is asked for, never assumed.
    #[test]
    fn the_warm_up_is_opt_in() {
        let mut t = LevelTracker::new(1, RATE);
        t.absorb(&[1.0]);
        let mut m = Meters::new(1);
        t.publish(&mut m);
        assert!(m.any_clipped());
    }

    #[test]
    fn an_overflowing_callback_says_exactly_how_many_frames_it_lost() {
        let stats = stats();
        let (mut src, _sink) = capture_channel(2, 10, 64, Arc::clone(&stats));
        src.accept(&frames(2, 16, 0), Some(0), 0);
        assert_eq!(stats.frames_captured(), 16, "the device produced 16");
        assert_eq!(
            stats.frames_captured() - stats.frames_dropped(),
            10,
            "captured minus dropped must be what the ring holds"
        );
        assert_eq!(stats.xruns(), 1, "one callback lost something");
    }

    #[test]
    fn the_dropped_frames_are_the_tail_and_the_mark_says_so() {
        let (mut src, mut sink) = capture_channel(2, 10, 64, stats());
        src.accept(&frames(2, 16, 0), Some(0), 0);
        let mark = sink.next_mark().expect("one mark per callback");
        assert_eq!(mark.first_frame, 0);
        assert_eq!(
            mark.frames_delivered(),
            16,
            "pushed plus dropped must equal what the device delivered"
        );
        assert!(mark.frames_dropped > 0);

        let mut out = Vec::new();
        sink.pop_frames(mark.frames_pushed as usize, &mut out);
        // The surviving frames are 0..pushed, contiguous — not a scatter.
        for (frame, pair) in out.chunks_exact(2).enumerate() {
            assert_eq!(pair[0], frame as f32);
        }
    }

    #[test]
    fn every_frame_in_the_ring_is_described_by_exactly_one_mark() {
        let (mut src, mut sink) = capture_channel(2, 4_096, 64, stats());
        for i in 0..20u64 {
            src.accept(&frames(2, 64, i as usize * 64), Some(i as Nanos * 1_000), 0);
        }
        let mut described = 0u64;
        let mut marks = 0;
        while let Some(m) = sink.next_mark() {
            described += m.frames_pushed as u64;
            marks += 1;
        }
        assert_eq!(marks, 20);
        assert_eq!(
            described,
            sink.frames_available() as u64,
            "the marks must account for the ring exactly"
        );
    }

    #[test]
    fn a_full_mark_ring_drops_the_samples_too_rather_than_orphaning_them() {
        // Rule 3. Two mark slots, three callbacks. The third buffer's samples
        // must NOT reach the ring: an undescribed frame is placed at the wrong
        // time by the writer thread, which is silent desync.
        let stats = stats();
        let (mut src, mut sink) = capture_channel(2, 4_096, 2, Arc::clone(&stats));
        for i in 0..3usize {
            src.accept(&frames(2, 32, i * 32), Some(0), 0);
        }
        let mut described = 0u64;
        while let Some(m) = sink.next_mark() {
            described += m.frames_pushed as u64;
        }
        assert_eq!(described, 64, "only the two described buffers got in");
        assert_eq!(sink.frames_available(), 64);
        assert_eq!(stats.marks_dropped(), 1);
        assert_eq!(stats.frames_dropped(), 32);
    }

    // ── reconstructing the device's timeline ────────────────────────────────

    #[test]
    fn the_frame_cursor_pads_a_dropout_so_file_sample_n_is_device_frame_n() {
        let mut cursor = FrameCursor::new();
        let a = TimingMark {
            first_frame: 0,
            frames_pushed: 64,
            frames_dropped: 0,
            device_ns: None,
            host_ns: 0,
        };
        // 8 frames of this buffer did not fit.
        let b = TimingMark {
            first_frame: 64,
            frames_pushed: 56,
            frames_dropped: 8,
            device_ns: None,
            host_ns: 0,
        };
        let c = TimingMark {
            first_frame: 128,
            frames_pushed: 64,
            frames_dropped: 0,
            device_ns: None,
            host_ns: 0,
        };
        let total: u64 = [a, b, c].iter().map(|m| cursor.plan(m).total_frames()).sum();
        assert_eq!(
            total, 192,
            "the file must be as long as the device's frame count, or every note \
             after the dropout slides earlier"
        );
        assert_eq!(cursor.next_frame(), 192);
    }

    #[test]
    fn a_lost_mark_is_still_visible_as_a_jump_in_the_frame_index() {
        // The mark for frames 64..128 never made it. Because `first_frame` is
        // absolute, the gap is recoverable from the next mark alone.
        let mut cursor = FrameCursor::new();
        cursor.plan(&TimingMark {
            first_frame: 0,
            frames_pushed: 64,
            frames_dropped: 0,
            device_ns: None,
            host_ns: 0,
        });
        let plan = cursor.plan(&TimingMark {
            first_frame: 128,
            frames_pushed: 64,
            frames_dropped: 0,
            device_ns: None,
            host_ns: 0,
        });
        assert_eq!(plan.silence_before, 64);
        assert_eq!(cursor.next_frame(), 192);
    }

    #[test]
    fn the_first_mark_never_pads_even_when_the_stream_started_long_ago() {
        // The stream runs from the moment the Recorder band opens, so by the
        // time a take starts `first_frame` is in the millions. Treating that as
        // a gap would prepend minutes of silence to the WAV.
        let mut cursor = FrameCursor::new();
        let plan = cursor.plan(&TimingMark {
            first_frame: 9_000_000,
            frames_pushed: 64,
            frames_dropped: 0,
            device_ns: None,
            host_ns: 0,
        });
        assert_eq!(plan.silence_before, 0);
    }

    // ── the clock hookup ────────────────────────────────────────────────────

    /// Drive a tap with `count` callbacks of `block` frames from a device whose
    /// crystal runs at `true_rate`, with `late_ns` of delivery lateness on each.
    fn run_tap(count: u64, block: u64, true_rate: f64, late: impl Fn(u64) -> Nanos) -> ClockTap {
        let mut tap = ClockTap::new(DEFAULT_SETTLE_NS);
        for k in 0..count {
            let first_frame = k * block;
            let device_ns = (first_frame as f64 / true_rate * 1e9) as u64;
            let host_ns = device_ns as Nanos + late(k);
            tap.observe(&TimingMark {
                first_frame,
                frames_pushed: block as u32,
                frames_dropped: 0,
                device_ns: Some(device_ns),
                host_ns,
            });
        }
        tap
    }

    #[test]
    fn the_fit_recovers_a_fifty_ppm_device_from_callbacks_alone() {
        let tap = run_tap(4_000, 512, 48_002.4, |_| 0);
        let ppm = tap.epsilon(RATE).unwrap() * 1e6;
        assert!(
            (ppm - 50.0).abs() < 0.5,
            "expected ~50 ppm from the device's own stamps, fitted {ppm:.3}"
        );
    }

    /// Delivery that starts 2 ms late and settles over the first ~200
    /// callbacks: what a real anchor sees while it is still finding its minimum.
    fn settling(k: u64) -> Nanos {
        if k < 200 {
            (200 - k) as Nanos * 10_000
        } else {
            0
        }
    }

    #[test]
    fn a_converging_anchor_does_not_bias_a_twenty_minute_fit() {
        // The first of the two numbers on `ClockTap`, checked rather than
        // asserted. 112_500 callbacks of 512 frames is twenty minutes.
        let tap = run_tap(112_500, 512, 48_002.4, settling);
        let ppm = tap.epsilon(RATE).unwrap() * 1e6;
        assert!(
            (ppm - 50.0).abs() < 0.5,
            "the settle window must not bend the slope of a real take; fitted \
             {ppm:.3} ppm"
        );
    }

    #[test]
    fn the_settle_bias_left_on_a_short_take_is_worth_under_a_millisecond() {
        // The second number, and the honest one: at 43 seconds the same
        // perturbation is worth ~6 ppm of fitted error. Asserted in the unit
        // that matters — seconds of misplacement in the exported file — rather
        // than in ppm, because in ppm it looks like a defect and in milliseconds
        // it is smaller than one MIDI tick.
        let t1 = 43 * 1_000_000_000;
        let biased = run_tap(4_000, 512, 48_002.4, settling);
        let clean = run_tap(4_000, 512, 48_002.4, |_| 0);
        let error_ms = (biased.timeline(0, t1, RATE).file_seconds(t1)
            - clean.timeline(0, t1, RATE).file_seconds(t1))
        .abs()
            * 1_000.0;
        assert!(
            error_ms < 0.5,
            "a settling anchor must not move the end of a short take by more \
             than half a millisecond; it moved it {error_ms:.3} ms"
        );
        assert!(
            error_ms > 0.0,
            "if this is exactly zero the perturbation is not reaching the fit \
             and the other assertion is vacuous"
        );
    }

    #[test]
    fn dropped_frames_do_not_bend_the_fitted_rate() {
        // The fit is indexed by DEVICE frames, so a buffer the ring refused
        // still advances the index. Index it by frames that reached the ring
        // instead and the fitted rate reads low by the drop fraction — a
        // dropout would masquerade as a slow crystal and the correction would
        // then be applied to the whole take.
        let mut tap = ClockTap::new(DEFAULT_SETTLE_NS);
        let true_rate = 48_002.4;
        for k in 0..4_000u64 {
            let first_frame = k * 512;
            let device_ns = (first_frame as f64 / true_rate * 1e9) as u64;
            // Every hundredth callback loses half its frames.
            let pushed = if k % 100 == 0 { 256 } else { 512 };
            tap.observe(&TimingMark {
                first_frame,
                frames_pushed: pushed,
                frames_dropped: 512 - pushed,
                device_ns: Some(device_ns),
                host_ns: device_ns as Nanos,
            });
        }
        let ppm = tap.epsilon(RATE).unwrap() * 1e6;
        assert!(
            (ppm - 50.0).abs() < 0.5,
            "drops must not change the measured rate; fitted {ppm:.3} ppm"
        );
    }

    #[test]
    fn a_callback_with_no_usable_timestamp_is_counted_and_not_fitted() {
        let mut tap = ClockTap::new(DEFAULT_SETTLE_NS);
        for k in 0..10u64 {
            tap.observe(&TimingMark {
                first_frame: k * 512,
                frames_pushed: 512,
                frames_dropped: 0,
                device_ns: None,
                host_ns: k as Nanos * 10_000_000,
            });
        }
        assert_eq!(tap.unstamped(), 10);
        assert_eq!(tap.fit().observations(), 0);
        assert!(
            tap.timeline(0, 1_000_000_000, RATE).is_synthetic(),
            "a take with no device stamps must report a synthetic clock, not a \
             0 ppm measurement it never made"
        );
    }

    #[test]
    fn a_stream_instant_that_goes_backwards_past_its_own_epoch_is_dropped_not_wrapped() {
        // `rebase` returns u64. A stamp earlier than the first one seen would
        // wrap to ~1.8e19 and drag the fit's slope to nonsense; it has to be
        // discarded instead.
        let (mut src, mut sink) = capture_channel(1, 256, 16, stats());
        src.accept(&frames(1, 8, 0), Some(1_000_000), 0);
        src.accept(&frames(1, 8, 8), Some(500_000), 0);
        let first = sink.next_mark().unwrap();
        let second = sink.next_mark().unwrap();
        assert_eq!(first.device_ns, Some(0), "the first stamp defines the epoch");
        assert_eq!(second.device_ns, None, "a backwards stamp is unusable");
    }

    #[test]
    fn twenty_minutes_of_correction_is_worth_sixty_milliseconds() {
        // The number the whole module is for, end to end from marks.
        let tap = run_tap(112_500, 512, 48_002.4, |_| 0);
        let t1 = 1_200 * 1_000_000_000;
        let corrected = tap.timeline(0, t1, RATE);
        let uncorrected = Timeline::synthetic(0, t1, RATE);
        let delta_ms =
            (corrected.file_seconds(t1) - uncorrected.file_seconds(t1)) * 1_000.0;
        assert!(
            (delta_ms - 60.0).abs() < 1.0,
            "expected ~60 ms of correction over twenty minutes, got {delta_ms:.2}"
        );
    }

    // ── levels ──────────────────────────────────────────────────────────────

    #[test]
    fn peak_and_rms_are_per_channel_and_never_mixed() {
        let mut tracker = LevelTracker::new(2, RATE);
        let mut meters = Meters::new(2);
        // Left at full scale, right at a tenth.
        let buf: Vec<f32> = (0..200)
            .flat_map(|i| {
                let s = if i % 2 == 0 { 0.5 } else { -0.5 };
                [s, s / 10.0]
            })
            .collect();
        tracker.absorb(&buf);
        tracker.publish(&mut meters);
        assert!((meters.peak[0] - 0.5).abs() < 1e-6);
        assert!((meters.peak[1] - 0.05).abs() < 1e-6);
        assert!(
            (meters.rms[0] - 0.5).abs() < 1e-6,
            "a square wave's RMS is its amplitude"
        );
        assert!((meters.rms[1] - 0.05).abs() < 1e-6);
    }

    #[test]
    fn the_clip_latch_survives_after_the_signal_comes_back_down() {
        // The failure this kills: a pianist clips one fortissimo chord in bar 3,
        // plays quietly for eighteen minutes, and the meter shows a clean take.
        let mut tracker = LevelTracker::new(2, RATE);
        let mut meters = Meters::new(2);
        tracker.arm();
        tracker.absorb(&[0.2, 1.0]);
        tracker.publish(&mut meters);
        assert!(meters.clipped[1] && !meters.clipped[0]);

        for _ in 0..1_000 {
            tracker.absorb(&[0.01, 0.01]);
            tracker.publish(&mut meters);
        }
        assert!(
            meters.clipped[1],
            "publishing must not clear the latch; only arming a new take may"
        );
        assert_eq!(meters.clipped_samples, 1);

        tracker.arm();
        tracker.absorb(&[0.01, 0.01]);
        tracker.publish(&mut meters);
        assert!(!meters.clipped[1], "arming a new take starts clean");
    }

    #[test]
    fn an_i16_signal_slammed_against_the_positive_rail_registers_as_clipping() {
        // dasp maps i16 as s/32768, so i16::MAX arrives as 0.999_969 and a
        // threshold of exactly 1.0 would see only the negative half of the
        // clipping — "the clip light only works on bass notes".
        let (mut src, mut sink) = capture_channel(1, 256, 16, stats());
        src.accept(&[i16::MAX; 32], Some(0), 0);
        let mut out = Vec::new();
        sink.drain_samples(&mut out);

        let mut tracker = LevelTracker::new(1, RATE);
        let mut meters = Meters::new(1);
        tracker.absorb(&out);
        tracker.publish(&mut meters);
        assert!(
            meters.clipped[0],
            "peak was {} ({} dBFS), which CLIP_LEVEL must catch",
            meters.take_peak[0],
            dbfs(meters.take_peak[0])
        );
    }

    #[test]
    fn a_take_that_recorded_silence_says_so_and_a_quiet_one_does_not() {
        let mut tracker = LevelTracker::new(1, RATE);
        let mut meters = Meters::new(1);
        tracker.arm();
        tracker.absorb(&[0.0; 4_800]);
        tracker.publish(&mut meters);
        assert!(meters.recorded_silence(), "digital black is silence");

        tracker.arm();
        // -40 dBFS: a genuinely quiet pianissimo, and not a fault.
        tracker.absorb(&[0.01; 4_800]);
        tracker.publish(&mut meters);
        assert!(
            !meters.recorded_silence(),
            "a quiet take must not be reported as a failed one"
        );
    }

    #[test]
    fn an_untouched_meter_is_not_a_silent_take() {
        // `recorded_silence` must be false before any audio arrives, or the
        // warning fires while the pianist is still sitting down.
        assert!(!Meters::new(2).recorded_silence());
    }

    #[test]
    fn the_meter_does_not_strobe_to_silence_when_the_ui_outruns_the_audio() {
        // 60 Hz repaint against 10 ms buffers polls an empty ring roughly a
        // third of the time. Zeroing on an empty window makes a ringing chord
        // flicker to nothing.
        let mut tracker = LevelTracker::new(1, RATE);
        let mut meters = Meters::new(1);
        tracker.absorb(&[0.5; 480]);
        tracker.publish(&mut meters);
        let held = meters.peak[0];
        tracker.publish(&mut meters);
        assert_eq!(meters.peak[0], held, "an empty window must hold, not zero");
        assert!(held > 0.0);
    }

    #[test]
    fn the_peak_hold_decays_rather_than_sticking_forever() {
        let mut tracker = LevelTracker::new(1, RATE);
        let mut meters = Meters::new(1);
        tracker.absorb(&[1.0; 480]);
        tracker.publish(&mut meters);
        let start = meters.hold[0];
        // One second of silence at 20 dB/s should drop the hold by ~20 dB.
        for _ in 0..100 {
            tracker.absorb(&[0.0; 480]);
            tracker.publish(&mut meters);
        }
        let drop_db = dbfs(start) - dbfs(meters.hold[0]);
        assert!(
            (drop_db - HOLD_DECAY_DB_PER_SEC).abs() < 1.0,
            "expected ~{HOLD_DECAY_DB_PER_SEC} dB of decay in one second, got {drop_db:.2}"
        );
    }

    #[test]
    fn a_partial_frame_at_the_end_of_a_drain_is_ignored_not_folded_into_channel_zero() {
        let mut tracker = LevelTracker::new(2, RATE);
        let mut meters = Meters::new(2);
        // Three samples: one whole frame plus a stray left. The stray is loud.
        tracker.absorb(&[0.1, 0.2, 1.0]);
        tracker.publish(&mut meters);
        assert!((meters.peak[0] - 0.1).abs() < 1e-6);
        assert!((meters.peak[1] - 0.2).abs() < 1e-6);
        assert!(!meters.any_clipped(), "the stray sample is not a frame");
    }

    #[test]
    fn dbfs_of_zero_is_negative_infinity_rather_than_a_nan() {
        assert_eq!(dbfs(0.0), f32::NEG_INFINITY);
        assert_eq!(dbfs(-1.0), f32::NEG_INFINITY);
        assert!((dbfs(1.0)).abs() < 1e-5);
        assert!((dbfs(0.5) + 6.0206).abs() < 1e-3);
    }

    // ── device loss ─────────────────────────────────────────────────────────

    #[test]
    fn a_lost_device_becomes_state_the_session_can_read_and_never_a_panic() {
        let stats = CaptureStats::new();
        assert_eq!(stats.device_state(), DeviceState::Running);
        stats.note_error(&StreamError::DeviceNotAvailable);
        assert_eq!(
            stats.device_state(),
            DeviceState::Lost,
            "a bus-powered interface browning out is a routine event that must \
             leave the take finalisable"
        );
        assert_eq!(stats.stream_errors(), 1);
    }

    #[test]
    fn a_backend_error_is_distinguished_from_an_unplugged_device() {
        // They lead to different `aborted_reason` strings and different advice.
        let stats = CaptureStats::new();
        stats.note_error(&StreamError::BackendSpecific {
            err: cpal::BackendSpecificError {
                description: "kAudioUnitErr_CannotDoInCurrentContext".into(),
            },
        });
        assert_eq!(stats.device_state(), DeviceState::Errored);
    }

    /// **Arming a take must not forget what the stream is.**
    ///
    /// `block_seen` and `ring_asked` are properties of the open device, not
    /// counters of a take: the ring was asked for once when it opened, and the
    /// cadence does not restart because somebody pressed Record. Clearing them
    /// in `reset` would make the first report of every take say "nobody knows"
    /// until a callback happened to land — which is the exact shape of answer
    /// this family of fields exists to stop giving, and it would look like a
    /// tidy-up rather than a regression.
    #[test]
    fn arming_a_take_does_not_forget_the_block_and_the_ring() {
        let stats = CaptureStats::new();
        stats.set_ring_asked(1024);
        stats.block_seen.store(512, Ordering::Relaxed);
        stats.frames_captured.fetch_add(48_000, Ordering::Relaxed);

        stats.reset();

        assert_eq!(stats.frames_captured(), 0, "the take's counters do reset");
        assert_eq!(
            stats.block_seen(),
            Some(512),
            "the delivered cadence is a property of the stream, not of the take"
        );
        assert_eq!(
            stats.ring_asked(),
            Some(1024),
            "the ring was asked for once, when the device opened"
        );
    }

    #[test]
    fn resetting_the_counters_does_not_clear_a_lost_device() {
        // Arming a second take must not paper over an interface that is gone.
        let stats = CaptureStats::new();
        stats.note_error(&StreamError::DeviceNotAvailable);
        stats.reset();
        assert_eq!(stats.device_state(), DeviceState::Lost);
        assert_eq!(stats.frames_dropped(), 0);
    }

    // ── the timebase ────────────────────────────────────────────────────────

    #[test]
    fn an_instant_before_the_epoch_is_negative_rather_than_saturated_to_zero() {
        // `Instant::duration_since` saturates, which would place every pre-roll
        // event at exactly T0 and collapse the controller snapshot.
        let timebase = Timebase::new();
        let now = Instant::now();
        let before = now
            .checked_sub(std::time::Duration::from_millis(50))
            .expect("50 ms before now is representable");
        assert!(
            timebase.at(before) < timebase.at(now),
            "an earlier instant must produce a smaller timebase value"
        );
    }

    // ── hardware ────────────────────────────────────────────────────────────

    /// Ignored: opens a real device. Run with `--ignored` on a machine with an
    /// interface attached, and never in CI — a build box has no audio input, and
    /// on macOS this raises a TCC prompt that blocks the run forever.
    #[test]
    #[ignore = "opens a real audio device"]
    fn the_default_input_delivers_callbacks_and_fits_a_rate() {
        let timebase = Timebase::new();
        let (input, mut sink, _monitor) = open_input(
            &InputSelection::Default,
            &ConfigWish::default(),
            DEFAULT_RING_SECONDS,
            timebase,
            None,
        )
        .expect("a default input");
        let mut tap = ClockTap::new(DEFAULT_SETTLE_NS);
        let mut audio = Vec::new();
        let deadline = Instant::now() + std::time::Duration::from_secs(3);
        while Instant::now() < deadline {
            while let Some(mark) = sink.next_mark() {
                tap.observe(&mark);
            }
            sink.drain_samples(&mut audio);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(input.stats().callbacks() > 0, "no callbacks in three seconds");
        assert_eq!(input.device_state(), DeviceState::Running);
        assert_eq!(input.stats().frames_dropped(), 0, "an idle machine must not xrun");
        let ppm = tap
            .epsilon(input.config().sample_rate as f64)
            .expect("three seconds is enough for a fit")
            * 1e6;
        assert!(
            ppm.abs() < 500.0,
            "a real crystal should be within a few hundred ppm; measured {ppm:.1}"
        );
    }

    /// Ignored: enumerates real devices.
    #[test]
    #[ignore = "enumerates real audio devices"]
    fn every_enumerated_input_can_be_reopened_by_its_key() {
        for info in input_devices().expect("enumeration") {
            let key = DeviceKey::from_setting(&info.key.to_setting());
            assert_eq!(key, info.key);
            with_host(|host| resolve(host, &InputSelection::Key(key)))
                .expect("a key from enumeration must resolve");
        }
    }
}
